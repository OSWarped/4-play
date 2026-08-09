#[cfg(target_os = "linux")]
mod linux {
    use evdev::uinput::VirtualDevice;
    use evdev::{
        AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, UinputAbsSetup,
    };
    use std::error::Error;
    use std::io::{self, BufRead, Write};
    use std::thread;
    use std::time::Duration;

    const BUTTONS: [KeyCode; 8] = [
        KeyCode::BTN_SOUTH,
        KeyCode::BTN_EAST,
        KeyCode::BTN_NORTH,
        KeyCode::BTN_WEST,
        KeyCode::BTN_TL,
        KeyCode::BTN_TR,
        KeyCode::BTN_SELECT,
        KeyCode::BTN_START,
    ];

    fn button(name: &str) -> Option<KeyCode> {
        match name.to_ascii_lowercase().as_str() {
            "south" | "b1" | "button1" => Some(KeyCode::BTN_SOUTH),
            "east" | "b2" | "button2" => Some(KeyCode::BTN_EAST),
            "north" | "b3" | "button3" => Some(KeyCode::BTN_NORTH),
            "west" | "b4" | "button4" => Some(KeyCode::BTN_WEST),
            "tl" | "b5" | "button5" => Some(KeyCode::BTN_TL),
            "tr" | "b6" | "button6" => Some(KeyCode::BTN_TR),
            "select" => Some(KeyCode::BTN_SELECT),
            "start" => Some(KeyCode::BTN_START),
            _ => None,
        }
    }

    fn set_button(device: &mut VirtualDevice, key: KeyCode, pressed: bool) -> io::Result<()> {
        device.emit(&[InputEvent::new(EventType::KEY.0, key.0, i32::from(pressed))])
    }

    fn set_axis(device: &mut VirtualDevice, axis: AbsoluteAxisCode, value: i32) -> io::Result<()> {
        device.emit(&[InputEvent::new(EventType::ABSOLUTE.0, axis.0, value)])
    }

    fn release_all(device: &mut VirtualDevice) -> io::Result<()> {
        let mut events = Vec::with_capacity(BUTTONS.len() + 2);
        events.extend(
            BUTTONS
                .iter()
                .map(|key| InputEvent::new(EventType::KEY.0, key.0, 0)),
        );
        events.push(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_X.0,
            0,
        ));
        events.push(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_Y.0,
            0,
        ));
        device.emit(&events)
    }

    fn print_help() {
        println!("Commands:");
        println!("  press <button>          Hold a button until it is released");
        println!("  release <button>        Release a held button");
        println!("  tap <button> [ms]       Press, wait (default 200 ms), and release");
        println!("  axis <x|y> <-1|0|1>     Set a directional axis");
        println!("  neutral                 Release every button and center both axes");
        println!("  help                    Show this help");
        println!("  quit                    Neutralize controls and exit");
        println!("Buttons: south/b1, east/b2, north/b3, west/b4, tl/b5, tr/b6, select, start");
    }

    pub fn run() -> Result<(), Box<dyn Error>> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for key in BUTTONS {
            keys.insert(key);
        }

        let abs_x = UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, AbsInfo::new(0, -1, 1, 0, 0, 0));
        let abs_y = UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, AbsInfo::new(0, -1, 1, 0, 0, 0));

        let mut device = VirtualDevice::builder()?
            .name("4-Play Virtual Controller")
            .with_keys(&keys)?
            .with_absolute_axis(&abs_x)?
            .with_absolute_axis(&abs_y)?
            .build()?;

        println!("Created virtual controller with 2 axes and 8 buttons.");
        print_help();

        let stdin = io::stdin();
        let mut lines = stdin.lock().lines();
        loop {
            print!("> ");
            io::stdout().flush()?;

            let Some(line) = lines.next() else {
                break;
            };
            let line = line?;
            let parts: Vec<_> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            match parts.as_slice() {
                ["press", name] => match button(name) {
                    Some(key) => {
                        set_button(&mut device, key, true)?;
                        println!("{name} pressed (held)");
                    }
                    None => eprintln!("Unknown button: {name}"),
                },
                ["release", name] => match button(name) {
                    Some(key) => {
                        set_button(&mut device, key, false)?;
                        println!("{name} released");
                    }
                    None => eprintln!("Unknown button: {name}"),
                },
                ["tap", name] | ["tap", name, _] => match button(name) {
                    Some(key) => {
                        let milliseconds = parts
                            .get(2)
                            .map(|value| value.parse::<u64>())
                            .transpose()?
                            .unwrap_or(200);
                        set_button(&mut device, key, true)?;
                        println!("{name} pressed for {milliseconds} ms");
                        thread::sleep(Duration::from_millis(milliseconds));
                        set_button(&mut device, key, false)?;
                        println!("{name} released");
                    }
                    None => eprintln!("Unknown button: {name}"),
                },
                ["axis", axis, value] => {
                    let axis = match *axis {
                        "x" => Some(AbsoluteAxisCode::ABS_X),
                        "y" => Some(AbsoluteAxisCode::ABS_Y),
                        _ => None,
                    };
                    let value = value.parse::<i32>();
                    match (axis, value) {
                        (Some(axis), Ok(value @ -1..=1)) => {
                            set_axis(&mut device, axis, value)?;
                            println!("{axis:?} set to {value}");
                        }
                        _ => eprintln!("Usage: axis <x|y> <-1|0|1>"),
                    }
                }
                ["neutral"] => {
                    release_all(&mut device)?;
                    println!("All controls neutralized");
                }
                ["help"] => print_help(),
                ["quit" | "exit"] => break,
                _ => eprintln!("Unknown command. Enter 'help' for available commands."),
            }
        }

        release_all(&mut device)?;
        println!("All controls neutralized; virtual controller test complete.");
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    linux::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("uinput-test requires Linux and /dev/uinput");
}
