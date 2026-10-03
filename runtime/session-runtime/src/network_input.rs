use crate::virtual_controller::VirtualController;
use input_protocol::{ControllerState, FLAG_STOP, PACKET_SIZE};
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

const RECEIVE_POLL: Duration = Duration::from_millis(10);
const INPUT_TIMEOUT: Duration = Duration::from_millis(250);

pub fn run_network_input<F>(
    controller: &mut VirtualController,
    port: u16,
    mut emulator_exited: F,
) -> io::Result<()>
where
    F: FnMut() -> io::Result<bool>,
{
    let socket = UdpSocket::bind(("0.0.0.0", port))?;
    socket.set_read_timeout(Some(RECEIVE_POLL))?;
    println!("Waiting for seat controller state on UDP port {port}.");

    let mut buffer = [0_u8; PACKET_SIZE];
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
                let Ok(state) = ControllerState::decode(&buffer[..size]) else {
                    continue;
                };
                if active_source.is_some_and(|active| active != source) {
                    continue;
                }
                if last_sequence.is_some_and(|last| !sequence_is_newer(state.sequence, last)) {
                    continue;
                }

                if active_source.is_none() {
                    println!("Seat input connected from {source}.");
                    active_source = Some(source);
                }
                last_sequence = Some(state.sequence);
                last_packet = Some(Instant::now());
                timed_out = false;
                controller.apply_state(state)?;

                if state.flags & FLAG_STOP != 0 {
                    println!("Seat requested session stop.");
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
            println!("Seat input timed out; controls neutralized.");
            active_source = None;
            last_sequence = None;
            last_packet = None;
        }
    }

    controller.neutralize()
}

fn sequence_is_newer(candidate: u32, previous: u32) -> bool {
    let difference = candidate.wrapping_sub(previous);
    difference != 0 && difference < (1 << 31)
}

#[cfg(test)]
mod tests {
    use super::sequence_is_newer;

    #[test]
    fn sequence_comparison_handles_wraparound() {
        assert!(sequence_is_newer(11, 10));
        assert!(!sequence_is_newer(10, 10));
        assert!(!sequence_is_newer(9, 10));
        assert!(sequence_is_newer(0, u32::MAX));
    }
}
