pub const PACKET_SIZE: usize = 17;
pub const AUTHENTICATED_PACKET_SIZE: usize = 33;
pub const FLAG_STOP: u8 = 1;

pub mod button {
    // Six arcade action buttons
    pub const ACTION_1: u16 = 1 << 0;
    pub const ACTION_2: u16 = 1 << 1;
    pub const ACTION_3: u16 = 1 << 2;
    pub const ACTION_4: u16 = 1 << 3;
    pub const ACTION_5: u16 = 1 << 4;
    pub const ACTION_6: u16 = 1 << 5;

    // Arcade system buttons
    pub const COIN: u16 = 1 << 6;
    pub const START: u16 = 1 << 7;

    // Optional aliases for gamepad-style clients
    pub const SOUTH: u16 = ACTION_1;
    pub const EAST: u16 = ACTION_2;
    pub const NORTH: u16 = ACTION_3;
    pub const WEST: u16 = ACTION_4;
    pub const LEFT_SHOULDER: u16 = ACTION_5;
    pub const RIGHT_SHOULDER: u16 = ACTION_6;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerState {
    pub sequence: u32,
    pub buttons: u16,
    pub axis_x: i16,
    pub axis_y: i16,
    pub flags: u8,
    pub player_slot: u8,
}

impl Default for ControllerState {
    fn default() -> Self {
        Self {
            sequence: 0,
            buttons: 0,
            axis_x: 0,
            axis_y: 0,
            flags: 0,
            player_slot: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionToken(pub [u8; 16]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthenticatedControllerState {
    pub token: SessionToken,
    pub state: ControllerState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    Length,
    Magic,
    Version,
    PlayerSlot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenParseError;

impl std::fmt::Display for TokenParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "session token must be a UUID containing 32 hexadecimal digits"
        )
    }
}

impl std::error::Error for TokenParseError {}

impl std::str::FromStr for SessionToken {
    type Err = TokenParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let compact = value
            .bytes()
            .filter(|byte| *byte != b'-')
            .collect::<Vec<_>>();
        if compact.len() != 32 {
            return Err(TokenParseError);
        }
        let mut token = [0_u8; 16];
        for (index, pair) in compact.chunks_exact(2).enumerate() {
            let high = hex_digit(pair[0]).ok_or(TokenParseError)?;
            let low = hex_digit(pair[1]).ok_or(TokenParseError)?;
            token[index] = (high << 4) | low;
        }
        Ok(Self(token))
    }
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

impl AuthenticatedControllerState {
    pub fn encode(self) -> [u8; AUTHENTICATED_PACKET_SIZE] {
        let mut packet = [0; AUTHENTICATED_PACKET_SIZE];
        packet[0..4].copy_from_slice(b"4PLY");
        packet[4] = 2;
        packet[5..21].copy_from_slice(&self.token.0);
        packet[21..25].copy_from_slice(&self.state.sequence.to_le_bytes());
        packet[25..27].copy_from_slice(&self.state.buttons.to_le_bytes());
        packet[27..29].copy_from_slice(&self.state.axis_x.to_le_bytes());
        packet[29..31].copy_from_slice(&self.state.axis_y.to_le_bytes());
        packet[31] = self.state.flags;
        packet[32] = self.state.player_slot;
        packet
    }

    pub fn decode(packet: &[u8]) -> Result<Self, DecodeError> {
        if packet.len() != AUTHENTICATED_PACKET_SIZE {
            return Err(DecodeError::Length);
        }
        if &packet[0..4] != b"4PLY" {
            return Err(DecodeError::Magic);
        }
        if packet[4] != 2 {
            return Err(DecodeError::Version);
        }
        if packet[32] == 0 {
            return Err(DecodeError::PlayerSlot);
        }
        Ok(Self {
            token: SessionToken(packet[5..21].try_into().unwrap()),
            state: ControllerState {
                sequence: u32::from_le_bytes(packet[21..25].try_into().unwrap()),
                buttons: u16::from_le_bytes(packet[25..27].try_into().unwrap()),
                axis_x: i16::from_le_bytes(packet[27..29].try_into().unwrap()),
                axis_y: i16::from_le_bytes(packet[29..31].try_into().unwrap()),
                flags: packet[31],
                player_slot: packet[32],
            },
        })
    }
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
        packet[16] = self.player_slot;
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
        if packet[16] == 0 {
            return Err(DecodeError::PlayerSlot);
        }

        Ok(Self {
            sequence: u32::from_le_bytes(packet[5..9].try_into().unwrap()),
            buttons: u16::from_le_bytes(packet[9..11].try_into().unwrap()),
            axis_x: i16::from_le_bytes(packet[11..13].try_into().unwrap()),
            axis_y: i16::from_le_bytes(packet[13..15].try_into().unwrap()),
            flags: packet[15],
            player_slot: packet[16],
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
            player_slot: 2,
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

    #[test]
    fn authenticated_state_round_trips_with_uuid_token() {
        let token = "00112233-4455-6677-8899-aabbccddeeff"
            .parse::<SessionToken>()
            .unwrap();
        let authenticated = AuthenticatedControllerState {
            token,
            state: ControllerState {
                sequence: 99,
                buttons: button::ACTION_1,
                axis_x: 12,
                axis_y: -34,
                flags: 0,
                player_slot: 3,
            },
        };
        assert_eq!(
            AuthenticatedControllerState::decode(&authenticated.encode()),
            Ok(authenticated)
        );
    }
}
