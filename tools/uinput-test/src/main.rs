use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, UinputAbsSetup,
};
use std::error::Error;
use std::thread;
use std::time::Duration;

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
        .name("4-Play Virtual Controller")
        .with_keys(&keys)?
        .with_absolute_axis(&abs_x)?
        .with_absolute_axis(&abs_y)?
        .build()?;

    println!("Created virtual controller with 2 axes and 8 buttons.");
    println!("Repeating right + Button 1 every two seconds.");

    for cycle in 1..=10 {
        println!("Cycle {cycle}: right + Button 1 pressed");

        device.emit(&[
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 1),
            InputEvent::new(EventType::KEY.0, KeyCode::BTN_SOUTH.0, 1),
        ])?;

        thread::sleep(Duration::from_secs(1));

        println!("Cycle {cycle}: controls released");

        device.emit(&[
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 0),
            InputEvent::new(EventType::KEY.0, KeyCode::BTN_SOUTH.0, 0),
        ])?;

        thread::sleep(Duration::from_secs(1));
    }

    println!("Virtual controller test complete.");

    Ok(())
}
