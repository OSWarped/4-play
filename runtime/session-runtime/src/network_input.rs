use crate::virtual_controller::VirtualController;
use input_protocol::{
    AUTHENTICATED_PACKET_SIZE, AuthenticatedControllerState, ControllerState, FLAG_STOP,
    PACKET_SIZE, SessionToken,
};
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

const RECEIVE_POLL: Duration = Duration::from_millis(10);
const INPUT_TIMEOUT: Duration = Duration::from_millis(250);

pub fn run_network_input<F>(
    controller: &mut VirtualController,
    port: u16,
    required_token: Option<SessionToken>,
    debug_input: bool,
    mut emulator_exited: F,
) -> io::Result<()>
where
    F: FnMut() -> io::Result<bool>,
{
    let socket = UdpSocket::bind(("0.0.0.0", port))?;
    socket.set_read_timeout(Some(RECEIVE_POLL))?;
    eprintln!("Waiting for seat controller state on UDP port {port}.");

    let mut buffer = [0_u8; AUTHENTICATED_PACKET_SIZE];
    let mut active_source: Option<SocketAddr> = None;
    let mut last_sequence: Option<u32> = None;
    let mut last_packet: Option<Instant> = None;
    let mut timed_out = false;

    loop {
        if emulator_exited()? {
            break;
        }

        match socket.recv_from(&mut buffer) {
            Ok((size, source)) => {
                let state = if let Some(required_token) = required_token {
                    let authenticated = match AuthenticatedControllerState::decode(&buffer[..size])
                    {
                        Ok(authenticated) => authenticated,
                        Err(error) => {
                            log_input_rejection(
                                debug_input,
                                port,
                                source,
                                format_args!(
                                    "decode failed: {error:?}; size={size} expected={AUTHENTICATED_PACKET_SIZE}"
                                ),
                            );
                            continue;
                        }
                    };
                    if !tokens_equal(authenticated.token, required_token) {
                        log_input_rejection(
                            debug_input,
                            port,
                            source,
                            format_args!(
                                "token mismatch; seq={} buttons=0x{:04x} axis=({}, {}) flags=0x{:02x}",
                                authenticated.state.sequence,
                                authenticated.state.buttons,
                                authenticated.state.axis_x.signum(),
                                authenticated.state.axis_y.signum(),
                                authenticated.state.flags,
                            ),
                        );
                        continue;
                    }
                    authenticated.state
                } else {
                    let state = match ControllerState::decode(&buffer[..size.min(PACKET_SIZE)]) {
                        Ok(state) => state,
                        Err(error) => {
                            log_input_rejection(
                                debug_input,
                                port,
                                source,
                                format_args!(
                                    "decode failed: {error:?}; size={size} expected={PACKET_SIZE}"
                                ),
                            );
                            continue;
                        }
                    };
                    state
                };
                if active_source.is_some_and(|active| active != source) {
                    log_input_rejection(
                        debug_input,
                        port,
                        source,
                        format_args!(
                            "wrong source; active={:?}; seq={} buttons=0x{:04x} axis=({}, {}) flags=0x{:02x}",
                            active_source,
                            state.sequence,
                            state.buttons,
                            state.axis_x.signum(),
                            state.axis_y.signum(),
                            state.flags,
                        ),
                    );
                    continue;
                }
                if let Some(last) = last_sequence
                    && !sequence_is_newer(state.sequence, last)
                {
                    log_input_rejection(
                        debug_input,
                        port,
                        source,
                        format_args!(
                            "stale sequence; previous={last} candidate={} buttons=0x{:04x} axis=({}, {}) flags=0x{:02x}",
                            state.sequence,
                            state.buttons,
                            state.axis_x.signum(),
                            state.axis_y.signum(),
                            state.flags,
                        ),
                    );
                    continue;
                }

                if active_source.is_none() {
                    eprintln!("Seat input connected from {source}.");
                    active_source = Some(source);
                }
                log_input_acceptance(debug_input, port, source, state);
                last_sequence = Some(state.sequence);
                last_packet = Some(Instant::now());
                timed_out = false;
                controller.apply_state(state)?;

                if state.flags & FLAG_STOP != 0 {
                    eprintln!("Seat requested session stop.");
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }

        if !timed_out && last_packet.is_some_and(|last| last.elapsed() >= INPUT_TIMEOUT) {
            controller.neutralize()?;
            timed_out = true;
            eprintln!("Seat input timed out; controls neutralized.");
            log_input_event(
                debug_input,
                port,
                format_args!("source lock cleared after input timeout"),
            );
            active_source = None;
            last_sequence = None;
            last_packet = None;
        }
    }

    controller.neutralize()
}

fn log_input_acceptance(debug_input: bool, port: u16, source: SocketAddr, state: ControllerState) {
    log_input_event(
        debug_input,
        port,
        format_args!(
            "accepted from {source}; seq={} buttons=0x{:04x} axis=({}, {}) flags=0x{:02x}",
            state.sequence,
            state.buttons,
            state.axis_x.signum(),
            state.axis_y.signum(),
            state.flags,
        ),
    );
}

fn log_input_rejection(
    debug_input: bool,
    port: u16,
    source: SocketAddr,
    reason: std::fmt::Arguments<'_>,
) {
    log_input_event(
        debug_input,
        port,
        format_args!("rejected from {source}: {reason}"),
    );
}

fn log_input_event(debug_input: bool, port: u16, message: std::fmt::Arguments<'_>) {
    if debug_input {
        eprintln!("input-debug port={port}: {message}");
    }
}

fn tokens_equal(candidate: SessionToken, required: SessionToken) -> bool {
    candidate
        .0
        .iter()
        .zip(required.0)
        .fold(0_u8, |difference, (candidate, required)| {
            difference | (*candidate ^ required)
        })
        == 0
}

fn sequence_is_newer(candidate: u32, previous: u32) -> bool {
    let difference = candidate.wrapping_sub(previous);
    difference != 0 && difference < (1 << 31)
}

#[cfg(test)]
mod tests {
    use super::{sequence_is_newer, tokens_equal};
    use input_protocol::SessionToken;

    #[test]
    fn sequence_comparison_handles_wraparound() {
        assert!(sequence_is_newer(11, 10));
        assert!(!sequence_is_newer(10, 10));
        assert!(!sequence_is_newer(9, 10));
        assert!(sequence_is_newer(0, u32::MAX));
    }

    #[test]
    fn session_tokens_must_match_every_byte() {
        assert!(tokens_equal(SessionToken([7; 16]), SessionToken([7; 16])));
        assert!(!tokens_equal(SessionToken([7; 16]), SessionToken([8; 16])));
    }
}
