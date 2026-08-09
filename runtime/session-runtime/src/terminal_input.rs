use crate::virtual_controller::{ControllerButton, VirtualController};
use std::io::{self, Read};
use std::mem::MaybeUninit;
use std::time::Duration;

const TAP_DURATION: Duration = Duration::from_millis(120);

struct RawTerminal {
    original: libc::termios,
}

impl RawTerminal {
    fn enter() -> io::Result<Self> {
        let mut original = MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, original.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let original = unsafe { original.assume_init() };
        let mut raw = original;
        unsafe { libc::cfmakeraw(&mut raw) };
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { original })
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original) } != 0 {
            eprintln!("Failed to restore terminal: {}", io::Error::last_os_error());
        }
    }
}

pub fn run_terminal_input(controller: &mut VirtualController) -> io::Result<()> {
    println!("Terminal controls: W/A/S/D move, J/K/L/; actions, U/I shoulders");
    println!("                   1 coin, 2 start, Q ends the session");
    println!("SSH terminals do not report releases; each key is a 120 ms tap.");

    let _terminal = RawTerminal::enter()?;
    let mut stdin = io::stdin().lock();
    let mut input = [0_u8; 1];

    loop {
        stdin.read_exact(&mut input)?;
        let handled = match input[0].to_ascii_lowercase() {
            b'w' => controller.tap_direction(0, -1, TAP_DURATION),
            b'a' => controller.tap_direction(-1, 0, TAP_DURATION),
            b's' => controller.tap_direction(0, 1, TAP_DURATION),
            b'd' => controller.tap_direction(1, 0, TAP_DURATION),
            b'j' => controller.tap_button(ControllerButton::South, TAP_DURATION),
            b'k' => controller.tap_button(ControllerButton::East, TAP_DURATION),
            b'l' => controller.tap_button(ControllerButton::North, TAP_DURATION),
            b';' => controller.tap_button(ControllerButton::West, TAP_DURATION),
            b'u' => controller.tap_button(ControllerButton::LeftShoulder, TAP_DURATION),
            b'i' => controller.tap_button(ControllerButton::RightShoulder, TAP_DURATION),
            b'1' => controller.tap_button(ControllerButton::Coin, TAP_DURATION),
            b'2' => controller.tap_button(ControllerButton::Start, TAP_DURATION),
            b'q' => break,
            _ => continue,
        };
        handled?;
    }

    controller.neutralize()
}
