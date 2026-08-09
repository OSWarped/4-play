pub const PACKET_SIZE: usize = 17;
pub const FLAG_STOP: u8 = 1;

pub mod button {
    pub const SOUTH: u16 = 1 << 0;
    pub const EAST: u16 = 1 << 1;
    pub const NORTH: u16 = 1 << 2;
    pub const WEST: u16 = 1 << 3;
    pub const LEFT_SHOULDER: u16 = 1 << 4;
    pub const RIGHT_SHOULDER: u16 = 1 << 5;
    pub const COIN: u16 = 1 << 6;
    pub const START: u16 = 1 << 7;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControllerState {
    pub sequence: u32,
    pub buttons: u16,
    pub axis_x: i16,
    pub axis_y: i16,
    pub flags: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    Length,
    Magic,
    Version,
    PlayerSlot,
}

impl ControllerState {
    pub fn encode(self) -> [u8; PACKET_SIZE] {
        let mut packet = [0; PACKET_SIZE];
        packet[0..4].copy_from_slice(b"4PLY");
        packet[4] = 1;
        packet[5..9].copy_from_slice(&self.sequence.to_le_bytes());
        packet[9..11].copy_from_slice(&self.buttons.to_le_bytes());
        packet[11..13].copy_from_slice(&self.axis_x.to_le_bytes());
        packet[13..15].copy_from_slice(&self.axis_y.to_le_bytes());
        packet[15] = self.flags;
        packet[16] = 1; // Player slot; fixed to player one for this experiment.
        packet
    }

    pub fn decode(packet: &[u8]) -> Result<Self, DecodeError> {
        if packet.len() != PACKET_SIZE {
            return Err(DecodeError::Length);
        }
        if &packet[0..4] != b"4PLY" {
            return Err(DecodeError::Magic);
        }
        if packet[4] != 1 {
            return Err(DecodeError::Version);
        }
        if packet[16] != 1 {
            return Err(DecodeError::PlayerSlot);
        }

        Ok(Self {
            sequence: u32::from_le_bytes(packet[5..9].try_into().unwrap()),
            buttons: u16::from_le_bytes(packet[9..11].try_into().unwrap()),
            axis_x: i16::from_le_bytes(packet[11..13].try_into().unwrap()),
            axis_y: i16::from_le_bytes(packet[13..15].try_into().unwrap()),
            flags: packet[15],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_state_round_trips() {
        let state = ControllerState {
            sequence: 42,
            buttons: button::SOUTH | button::START,
            axis_x: -32767,
            axis_y: 32767,
            flags: FLAG_STOP,
        };

        assert_eq!(ControllerState::decode(&state.encode()), Ok(state));
    }

    #[test]
    fn rejects_wrong_packet_shape() {
        assert_eq!(ControllerState::decode(&[]), Err(DecodeError::Length));
        let mut packet = ControllerState::default().encode();
        packet[0] = b'X';
        assert_eq!(ControllerState::decode(&packet), Err(DecodeError::Magic));
    }
}
