use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, UinputAbsSetup,
};
use std::io;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy)]
enum MixedInput {
    Direction(i32, i32),
    Button(ControllerButton),
}

pub struct VirtualController {
    device: VirtualDevice,
    axis_x: i32,
    axis_y: i32,
}

impl VirtualController {
    pub fn create(player_number: u8) -> io::Result<Self> {
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
            axis_x: 0,
            axis_y: 0,
        })
    }

    pub fn set_direction(&mut self, horizontal: i32, vertical: i32) -> io::Result<()> {
        let horizontal = horizontal.clamp(-1, 1);
        let vertical = vertical.clamp(-1, 1);

        let mut events = Vec::new();

        if horizontal != self.axis_x {
            events.push(InputEvent::new(
                EventType::ABSOLUTE.0,
                AbsoluteAxisCode::ABS_X.0,
                horizontal,
            ));

            self.axis_x = horizontal;
        }

        if vertical != self.axis_y {
            events.push(InputEvent::new(
                EventType::ABSOLUTE.0,
                AbsoluteAxisCode::ABS_Y.0,
                vertical,
            ));

            self.axis_y = vertical;
        }

        if !events.is_empty() {
            self.device.emit(&events)?;
        }

        Ok(())
    }

    pub fn press(&mut self, button: ControllerButton) -> io::Result<()> {
        self.set_button(button, true)
    }

    pub fn release(&mut self, button: ControllerButton) -> io::Result<()> {
        self.set_button(button, false)
    }

    pub fn tap(&mut self, button: ControllerButton) -> io::Result<()> {
        self.press(button)?;
        thread::sleep(Duration::from_millis(250));
        self.release(button)
    }

    pub fn neutralize(&mut self) -> io::Result<()> {
        self.set_direction(0, 0)?;

        let buttons = [
            ControllerButton::South,
            ControllerButton::East,
            ControllerButton::North,
            ControllerButton::West,
            ControllerButton::LeftShoulder,
            ControllerButton::RightShoulder,
            ControllerButton::Coin,
            ControllerButton::Start,
        ];

        for button in buttons {
            self.release(button)?;
        }

        Ok(())
    }

    pub fn run_diagnostic_sequence(&mut self) -> io::Result<()> {
        println!("Controller diagnostic will begin after a 45-second KI boot delay.");

        thread::sleep(Duration::from_secs(45));

        println!("Diagnostic: Coin 1.");
        self.tap(ControllerButton::Coin)?;

        thread::sleep(Duration::from_millis(750));

        println!("Diagnostic: Coin 2.");
        self.tap(ControllerButton::Coin)?;

        thread::sleep(Duration::from_secs(2));

        println!("Diagnostic: Player 1 Start.");
        self.tap(ControllerButton::Start)?;

        thread::sleep(Duration::from_secs(2));

        println!("Diagnostic: Left ×3.");
        for step in 1..=3 {
            println!("  Left {step}/3");
            self.tap_direction(-1, 0)?;
        }

        println!("Diagnostic: Right ×4.");
        for step in 1..=4 {
            println!("  Right {step}/4");
            self.tap_direction(1, 0)?;
        }

        println!("Diagnostic: Button 1 ×2.");
        for step in 1..=2 {
            println!("  Button 1 {step}/2");
            self.tap(ControllerButton::South)?;
            thread::sleep(Duration::from_millis(500));
        }

        println!("Diagnostic: pausing for six seconds.");
        thread::sleep(Duration::from_secs(6));

        println!("Diagnostic: mixed input sequence.");

        let mixed_sequence = [
            MixedInput::Direction(-1, 0),
            MixedInput::Button(ControllerButton::South),
            MixedInput::Direction(1, 0),
            MixedInput::Button(ControllerButton::East),
            MixedInput::Direction(0, -1),
            MixedInput::Button(ControllerButton::North),
            MixedInput::Direction(0, 1),
            MixedInput::Button(ControllerButton::West),
            MixedInput::Direction(-1, -1),
            MixedInput::Button(ControllerButton::LeftShoulder),
            MixedInput::Direction(1, -1),
            MixedInput::Button(ControllerButton::RightShoulder),
            MixedInput::Direction(-1, 1),
            MixedInput::Button(ControllerButton::South),
            MixedInput::Direction(1, 1),
            MixedInput::Button(ControllerButton::East),
            MixedInput::Direction(-1, 0),
            MixedInput::Button(ControllerButton::North),
            MixedInput::Direction(1, 0),
            MixedInput::Button(ControllerButton::West),
        ];

        for (index, input) in mixed_sequence.into_iter().enumerate() {
            println!("  Mixed input {}/20", index + 1);

            match input {
                MixedInput::Direction(horizontal, vertical) => {
                    self.tap_direction(horizontal, vertical)?;
                }
                MixedInput::Button(button) => {
                    self.tap(button)?;
                    thread::sleep(Duration::from_millis(350));
                }
            }
        }

        self.neutralize()?;

        println!("Controller diagnostic complete; controls are neutral.");

        Ok(())
    }

    fn tap_direction(&mut self, horizontal: i32, vertical: i32) -> io::Result<()> {
        self.set_direction(horizontal, vertical)?;

        thread::sleep(Duration::from_millis(300));

        self.set_direction(0, 0)?;

        thread::sleep(Duration::from_millis(300));

        Ok(())
    }

    fn set_button(&mut self, button: ControllerButton, pressed: bool) -> io::Result<()> {
        let key = match button {
            ControllerButton::South => KeyCode::BTN_SOUTH,
            ControllerButton::East => KeyCode::BTN_EAST,
            ControllerButton::North => KeyCode::BTN_NORTH,
            ControllerButton::West => KeyCode::BTN_WEST,
            ControllerButton::LeftShoulder => KeyCode::BTN_TL,
            ControllerButton::RightShoulder => KeyCode::BTN_TR,
            ControllerButton::Coin => KeyCode::BTN_SELECT,
            ControllerButton::Start => KeyCode::BTN_START,
        };

        self.device
            .emit(&[InputEvent::new(EventType::KEY.0, key.0, i32::from(pressed))])
    }
}

impl Drop for VirtualController {
    fn drop(&mut self) {
        if let Err(error) = self.neutralize() {
            eprintln!("Failed to neutralize virtual controller during drop: {error}");
        }
    }
}
