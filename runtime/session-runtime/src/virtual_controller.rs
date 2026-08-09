use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, UinputAbsSetup,
};
use std::io;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum ControllerButton {
    South,
    East,
    North,
    West,
    LeftShoulder,
    RightShoulder,
    Coin,
    Start,
}

pub struct VirtualController {
    device: VirtualDevice,
}

impl VirtualController {
    pub fn create(player_number: u8) -> io::Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
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
            keys.insert(key);
        }

        let abs_x = UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, AbsInfo::new(0, -1, 1, 0, 0, 0));
        let abs_y = UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, AbsInfo::new(0, -1, 1, 0, 0, 0));
        let name = format!("4-Play Player {player_number}");

        let device = VirtualDevice::builder()?
            .name(&name)
            .with_keys(&keys)?
            .with_absolute_axis(&abs_x)?
            .with_absolute_axis(&abs_y)?
            .build()?;

        println!("Created virtual controller: {name}");
        Ok(Self { device })
    }

    pub fn tap_button(&mut self, button: ControllerButton, duration: Duration) -> io::Result<()> {
        self.set_button(button, true)?;
        thread::sleep(duration);
        self.set_button(button, false)
    }

    pub fn tap_direction(&mut self, x: i32, y: i32, duration: Duration) -> io::Result<()> {
        self.set_direction(x, y)?;
        thread::sleep(duration);
        self.set_direction(0, 0)
    }

    pub fn neutralize(&mut self) -> io::Result<()> {
        let mut events = vec![
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 0),
        ];
        events.extend(
            [
                ControllerButton::South,
                ControllerButton::East,
                ControllerButton::North,
                ControllerButton::West,
                ControllerButton::LeftShoulder,
                ControllerButton::RightShoulder,
                ControllerButton::Coin,
                ControllerButton::Start,
            ]
            .map(|button| InputEvent::new(EventType::KEY.0, key_code(button).0, 0)),
        );
        self.device.emit(&events)
    }

    fn set_button(&mut self, button: ControllerButton, pressed: bool) -> io::Result<()> {
        self.device.emit(&[InputEvent::new(
            EventType::KEY.0,
            key_code(button).0,
            i32::from(pressed),
        )])
    }

    fn set_direction(&mut self, x: i32, y: i32) -> io::Result<()> {
        self.device.emit(&[
            InputEvent::new(
                EventType::ABSOLUTE.0,
                AbsoluteAxisCode::ABS_X.0,
                x.clamp(-1, 1),
            ),
            InputEvent::new(
                EventType::ABSOLUTE.0,
                AbsoluteAxisCode::ABS_Y.0,
                y.clamp(-1, 1),
            ),
        ])
    }
}

impl Drop for VirtualController {
    fn drop(&mut self) {
        if let Err(error) = self.neutralize() {
            eprintln!("Failed to neutralize virtual controller: {error}");
        }
    }
}

fn key_code(button: ControllerButton) -> KeyCode {
    match button {
        ControllerButton::South => KeyCode::BTN_SOUTH,
        ControllerButton::East => KeyCode::BTN_EAST,
        ControllerButton::North => KeyCode::BTN_NORTH,
        ControllerButton::West => KeyCode::BTN_WEST,
        ControllerButton::LeftShoulder => KeyCode::BTN_TL,
        ControllerButton::RightShoulder => KeyCode::BTN_TR,
        ControllerButton::Coin => KeyCode::BTN_SELECT,
        ControllerButton::Start => KeyCode::BTN_START,
    }
}
