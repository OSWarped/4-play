use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use input_protocol::{ControllerState, FLAG_STOP, button};
use std::collections::HashSet;
use std::env;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::process;
use std::time::{Duration, Instant};

const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(50);

struct RawMode;

impl RawMode {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if let Err(error) = disable_raw_mode() {
            eprintln!("Failed to restore terminal: {error}");
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let destination = parse_destination();
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect(destination)?;

    println!("4-Play seat input -> {destination}");
    println!("W/A/S/D move; J/K/L/; actions; U/I shoulders; 1 coin; 2 start");
    println!("Press Esc to disconnect and stop the development session.");

    let _raw_mode = RawMode::enter()?;
    let mut held = HashSet::new();
    let mut sequence = 0_u32;
    let mut last_send = Instant::now() - HEARTBEAT_INTERVAL;

    loop {
        let wait = HEARTBEAT_INTERVAL.saturating_sub(last_send.elapsed());
        let mut changed = false;

        if event::poll(wait)? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Esc && key.kind == KeyEventKind::Press {
                    send_state(&socket, &held, &mut sequence, FLAG_STOP)?;
                    break;
                }
                changed = update_held_keys(&mut held, key);
            }
        }

        if changed || last_send.elapsed() >= HEARTBEAT_INTERVAL {
            send_state(&socket, &held, &mut sequence, 0)?;
            last_send = Instant::now();
        }
    }

    Ok(())
}

fn parse_destination() -> SocketAddr {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "seat-input".into());
    let destination = args.next().unwrap_or_else(|| {
        eprintln!("Usage: {program} <runtime-address:input-port>");
        process::exit(2);
    });
    if args.next().is_some() {
        eprintln!("Usage: {program} <runtime-address:input-port>");
        process::exit(2);
    }
    destination.parse().unwrap_or_else(|error| {
        eprintln!("Invalid runtime address '{destination}': {error}");
        process::exit(2);
    })
}

fn update_held_keys(held: &mut HashSet<KeyCode>, key: KeyEvent) -> bool {
    let code = normalize_key(key.code);
    if !is_control_key(code) {
        return false;
    }
    match key.kind {
        KeyEventKind::Press => held.insert(code),
        KeyEventKind::Release => held.remove(&code),
        KeyEventKind::Repeat => false,
    }
}

fn normalize_key(key: KeyCode) -> KeyCode {
    match key {
        KeyCode::Char(character) => KeyCode::Char(character.to_ascii_lowercase()),
        other => other,
    }
}

fn is_control_key(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::Char('w' | 'a' | 's' | 'd' | 'j' | 'k' | 'l' | ';' | 'u' | 'i' | '1' | '2')
    )
}

fn send_state(
    socket: &UdpSocket,
    held: &HashSet<KeyCode>,
    sequence: &mut u32,
    flags: u8,
) -> io::Result<()> {
    *sequence = sequence.wrapping_add(1);
    let state = state_from_keys(held, *sequence, flags);
    socket.send(&state.encode())?;
    Ok(())
}

fn state_from_keys(held: &HashSet<KeyCode>, sequence: u32, flags: u8) -> ControllerState {
    let is_held = |character| held.contains(&KeyCode::Char(character));
    let axis_x = (i16::from(is_held('d')) - i16::from(is_held('a'))) * i16::MAX;
    let axis_y = (i16::from(is_held('s')) - i16::from(is_held('w'))) * i16::MAX;

    let mut buttons = 0;
    for (key, mask) in [
        ('j', button::SOUTH),
        ('k', button::EAST),
        ('l', button::NORTH),
        (';', button::WEST),
        ('u', button::LEFT_SHOULDER),
        ('i', button::RIGHT_SHOULDER),
        ('1', button::COIN),
        ('2', button::START),
    ] {
        if is_held(key) {
            buttons |= mask;
        }
    }

    ControllerState {
        sequence,
        buttons,
        axis_x,
        axis_y,
        flags,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simultaneous_keys_produce_combined_state() {
        let held = HashSet::from([
            KeyCode::Char('w'),
            KeyCode::Char('d'),
            KeyCode::Char('j'),
            KeyCode::Char('k'),
        ]);
        let state = state_from_keys(&held, 7, 0);

        assert_eq!(state.axis_x, i16::MAX);
        assert_eq!(state.axis_y, -i16::MAX);
        assert_eq!(state.buttons, button::SOUTH | button::EAST);
    }
}
