use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, UinputAbsSetup,
};
use input_protocol::{ControllerState, button};
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
    state: ControllerState,
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
        Ok(Self {
            device,
            state: ControllerState::default(),
        })
    }

    pub fn apply_state(&mut self, state: ControllerState) -> io::Result<()> {
        let mut events = Vec::new();
        let old_x = axis_direction(self.state.axis_x);
        let old_y = axis_direction(self.state.axis_y);
        let new_x = axis_direction(state.axis_x);
        let new_y = axis_direction(state.axis_y);

        if old_x != new_x {
            events.push(InputEvent::new(
                EventType::ABSOLUTE.0,
                AbsoluteAxisCode::ABS_X.0,
                new_x,
            ));
        }
        if old_y != new_y {
            events.push(InputEvent::new(
                EventType::ABSOLUTE.0,
                AbsoluteAxisCode::ABS_Y.0,
                new_y,
            ));
        }

        for (mask, key) in [
            (button::SOUTH, KeyCode::BTN_SOUTH),
            (button::EAST, KeyCode::BTN_EAST),
            (button::NORTH, KeyCode::BTN_NORTH),
            (button::WEST, KeyCode::BTN_WEST),
            (button::LEFT_SHOULDER, KeyCode::BTN_TL),
            (button::RIGHT_SHOULDER, KeyCode::BTN_TR),
            (button::COIN, KeyCode::BTN_SELECT),
            (button::START, KeyCode::BTN_START),
        ] {
            let was_pressed = self.state.buttons & mask != 0;
            let is_pressed = state.buttons & mask != 0;
            if was_pressed != is_pressed {
                events.push(InputEvent::new(
                    EventType::KEY.0,
                    key.0,
                    i32::from(is_pressed),
                ));
            }
        }

        if !events.is_empty() {
            self.device.emit(&events)?;
        }
        self.state = state;
        Ok(())
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
        self.device.emit(&events)?;
        self.state = ControllerState::default();
        Ok(())
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

fn axis_direction(value: i16) -> i32 {
    i32::from(value.signum())
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
