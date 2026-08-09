use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, UinputAbsSetup,
};
use std::error::Error;
use std::io::{self, BufRead, Write};
use std::thread;
use std::time::Duration;

const TAP_DURATION: Duration = Duration::from_millis(300);

fn emit_key(device: &mut VirtualDevice, key: KeyCode, pressed: bool) -> io::Result<()> {
    device.emit(&[InputEvent::new(EventType::KEY.0, key.0, i32::from(pressed))])
}

fn tap_key(device: &mut VirtualDevice, key: KeyCode) -> io::Result<()> {
    emit_key(device, key, true)?;
    thread::sleep(TAP_DURATION);
    emit_key(device, key, false)
}

fn set_direction(device: &mut VirtualDevice, horizontal: i32, vertical: i32) -> io::Result<()> {
    device.emit(&[
        InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_X.0,
            horizontal.clamp(-1, 1),
        ),
        InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_Y.0,
            vertical.clamp(-1, 1),
        ),
    ])
}

fn tap_direction(device: &mut VirtualDevice, horizontal: i32, vertical: i32) -> io::Result<()> {
    set_direction(device, horizontal, vertical)?;
    thread::sleep(TAP_DURATION);
    set_direction(device, 0, 0)
}

fn neutralize(device: &mut VirtualDevice) -> io::Result<()> {
    set_direction(device, 0, 0)?;

    for key in [
        KeyCode::BTN_SOUTH,
        KeyCode::BTN_EAST,
        KeyCode::BTN_NORTH,
        KeyCode::BTN_WEST,
        KeyCode::BTN_TL,
        KeyCode::BTN_TR,
        KeyCode::BTN_SELECT,
        KeyCode::BTN_START,
    ] {
        emit_key(device, key, false)?;
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut keys = AttributeSet::<KeyCode>::new();

    keys.insert(KeyCode::BTN_SOUTH);
    keys.insert(KeyCode::BTN_EAST);
    keys.insert(KeyCode::BTN_NORTH);
    keys.insert(KeyCode::BTN_WEST);
    keys.insert(KeyCode::BTN_TL);
    keys.insert(KeyCode::BTN_TR);
    keys.insert(KeyCode::BTN_SELECT);
    keys.insert(KeyCode::BTN_START);

    let abs_x = UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, AbsInfo::new(0, -1, 1, 0, 0, 0));

    let abs_y = UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, AbsInfo::new(0, -1, 1, 0, 0, 0));

    let mut device = VirtualDevice::builder()?
        .name("4-Play Player 1")
        .with_keys(&keys)?
        .with_absolute_axis(&abs_x)?
        .with_absolute_axis(&abs_y)?
        .build()?;

    neutralize(&mut device)?;

    println!("Created interactive virtual controller: 4-Play Player 1");
    println!();
    println!("Commands:");
    println!("  coin     Start     left     right");
    println!("  up       down      b1       b2");
    println!("  b3       b4        b5       b6");
    println!("  neutral  quit");
    println!();

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("4play-input> ");
        stdout.flush()?;

        let mut line = String::new();

        if stdin.lock().read_line(&mut line)? == 0 {
            break;
        }

        let command = line.trim().to_ascii_lowercase();

        match command.as_str() {
            "coin" => tap_key(&mut device, KeyCode::BTN_SELECT)?,
            "start" => tap_key(&mut device, KeyCode::BTN_START)?,
            "left" => tap_direction(&mut device, -1, 0)?,
            "right" => tap_direction(&mut device, 1, 0)?,
            "up" => tap_direction(&mut device, 0, -1)?,
            "down" => tap_direction(&mut device, 0, 1)?,
            "b1" => tap_key(&mut device, KeyCode::BTN_SOUTH)?,
            "b2" => tap_key(&mut device, KeyCode::BTN_EAST)?,
            "b3" => tap_key(&mut device, KeyCode::BTN_NORTH)?,
            "b4" => tap_key(&mut device, KeyCode::BTN_WEST)?,
            "b5" => tap_key(&mut device, KeyCode::BTN_TL)?,
            "b6" => tap_key(&mut device, KeyCode::BTN_TR)?,
            "neutral" => neutralize(&mut device)?,
            "quit" | "exit" => {
                neutralize(&mut device)?;
                break;
            }
            "" => continue,
            unknown => {
                println!("Unknown command: {unknown}");
                continue;
            }
        }

        println!("Sent: {command}");
    }

    neutralize(&mut device)?;
    println!("Virtual controller removed.");

    Ok(())
}
