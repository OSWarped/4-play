use std::{net::IpAddr, path::PathBuf};

use control_protocol::{ConnectionGrant, Session, SpectatorGrant};
use input_protocol::SessionToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatClientConfig {
    pub control_plane_url: String,
    pub seat_id: String,
    pub destination_address: IpAddr,
    pub seat_api_token: String,
    pub ffplay_path: PathBuf,
}

impl SeatClientConfig {
    pub fn validate(&self) -> Result<(), SeatClientConfigError> {
        validate_url(&self.control_plane_url)?;
        validate_identifier("seat ID", &self.seat_id)?;
        if self.seat_api_token.trim().is_empty() {
            return Err(SeatClientConfigError::MissingSeatApiToken);
        }
        if self.ffplay_path.as_os_str().is_empty() {
            return Err(SeatClientConfigError::MissingFfplayPath);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeatClientConfigError {
    InvalidControlPlaneUrl,
    InvalidIdentifier { field: &'static str },
    MissingSeatApiToken,
    MissingFfplayPath,
}

impl std::fmt::Display for SeatClientConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidControlPlaneUrl => write!(formatter, "control-plane URL must be http(s)"),
            Self::InvalidIdentifier { field } => write!(
                formatter,
                "{field} must contain 1-64 ASCII letters, digits, dots, dashes, or underscores"
            ),
            Self::MissingSeatApiToken => write!(formatter, "seat API token is required"),
            Self::MissingFfplayPath => write!(formatter, "ffplay path is required"),
        }
    }
}

impl std::error::Error for SeatClientConfigError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeatAction {
    StartGame {
        game_id: String,
    },
    JoinSlot {
        session_id: String,
        player_number: u32,
    },
    RejoinSlot {
        session_id: String,
        player_number: u32,
    },
    LeaveSlot {
        session_id: String,
        player_number: u32,
    },
    Spectate {
        session_id: String,
    },
    StopMedia,
    StopInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeatActionResult {
    Browsing,
    Player(PlayerRuntimePlan),
    Spectator(SpectatorRuntimePlan),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRuntimePlan {
    pub session_id: String,
    pub player_number: u32,
    pub media: MediaReceiverPlan,
    pub input: InputForwardingPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpectatorRuntimePlan {
    pub session_id: String,
    pub grant_id: String,
    pub media: MediaReceiverPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaReceiverPlan {
    pub udp_port: u16,
    pub receiver_url: String,
    pub ffplay_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputForwardingPlan {
    pub destination: String,
    pub token: SessionToken,
    pub player_number: u32,
}

pub trait SeatRuntimeBackend {
    type Error;

    fn start_media(&mut self, plan: &MediaReceiverPlan) -> Result<(), Self::Error>;
    fn stop_media(&mut self) -> Result<(), Self::Error>;
    fn start_input(&mut self, plan: &InputForwardingPlan) -> Result<(), Self::Error>;
    fn stop_input(&mut self) -> Result<(), Self::Error>;
}

pub fn media_receiver_plan(media_udp_port: u16) -> MediaReceiverPlan {
    let receiver_url =
        format!("udp://0.0.0.0:{media_udp_port}?fifo_size=1000000&overrun_nonfatal=1");
    MediaReceiverPlan {
        udp_port: media_udp_port,
        ffplay_args: vec![
            "-f".to_owned(),
            "mpegts".to_owned(),
            "-fflags".to_owned(),
            "nobuffer".to_owned(),
            "-flags".to_owned(),
            "low_delay".to_owned(),
            "-framedrop".to_owned(),
            "-probesize".to_owned(),
            "32768".to_owned(),
            "-analyzeduration".to_owned(),
            "0".to_owned(),
            receiver_url.clone(),
        ],
        receiver_url,
    }
}

pub fn player_runtime_plan(
    session: &Session,
    player_number: u32,
) -> Result<PlayerRuntimePlan, RuntimePlanError> {
    let player_number_u8 =
        u8::try_from(player_number).map_err(|_| RuntimePlanError::InvalidPlayerNumber)?;
    if player_number_u8 == 0 {
        return Err(RuntimePlanError::InvalidPlayerNumber);
    }
    Ok(PlayerRuntimePlan {
        session_id: session.id.clone(),
        player_number,
        media: media_receiver_plan(session.connection_grant.media_udp_port),
        input: input_forwarding_plan(&session.connection_grant, player_number_u8)?,
    })
}

pub fn spectator_runtime_plan(grant: &SpectatorGrant) -> SpectatorRuntimePlan {
    SpectatorRuntimePlan {
        session_id: grant.session_id.clone(),
        grant_id: grant.id.clone(),
        media: media_receiver_plan(grant.media_udp_port),
    }
}

pub fn input_forwarding_plan(
    grant: &ConnectionGrant,
    player_number: u8,
) -> Result<InputForwardingPlan, RuntimePlanError> {
    if player_number == 0 {
        return Err(RuntimePlanError::InvalidPlayerNumber);
    }
    let token = grant
        .token
        .parse::<SessionToken>()
        .map_err(|_| RuntimePlanError::InvalidInputToken)?;
    Ok(InputForwardingPlan {
        destination: format!("{}:{}", grant.runtime_host_address, grant.input_udp_port),
        token,
        player_number: u32::from(player_number),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePlanError {
    InvalidInputToken,
    InvalidPlayerNumber,
}

impl std::fmt::Display for RuntimePlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInputToken => write!(formatter, "connection grant input token is invalid"),
            Self::InvalidPlayerNumber => write!(formatter, "player number must be 1-255"),
        }
    }
}

impl std::error::Error for RuntimePlanError {}

fn validate_url(value: &str) -> Result<(), SeatClientConfigError> {
    let value = value.trim();
    if value.starts_with("http://") || value.starts_with("https://") {
        Ok(())
    } else {
        Err(SeatClientConfigError::InvalidControlPlaneUrl)
    }
}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), SeatClientConfigError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(SeatClientConfigError::InvalidIdentifier { field })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use control_protocol::{
        ConnectionGrant, GameRuntimeProfile, PlayerSlot, PlayerSlotPresentation, PlayerSlotState,
        Session, SessionState,
    };

    #[test]
    fn builds_low_latency_ffplay_plan() {
        let plan = media_receiver_plan(41_002);

        assert_eq!(
            plan.receiver_url,
            "udp://0.0.0.0:41002?fifo_size=1000000&overrun_nonfatal=1"
        );
        assert_eq!(plan.ffplay_args.first().map(String::as_str), Some("-f"));
        assert!(plan.ffplay_args.iter().any(|arg| arg == "low_delay"));
        assert_eq!(plan.ffplay_args.last(), Some(&plan.receiver_url));
    }

    #[test]
    fn builds_input_forwarding_plan_from_connection_grant() {
        let plan = input_forwarding_plan(&connection_grant(), 2).unwrap();

        assert_eq!(plan.destination, "192.0.2.68:42000");
        assert_eq!(plan.player_number, 2);
    }

    #[test]
    fn validates_required_seat_config() {
        let config = SeatClientConfig {
            control_plane_url: "http://192.0.2.68:8080".to_owned(),
            seat_id: "windows-seat-1".to_owned(),
            destination_address: "192.0.2.10".parse().unwrap(),
            seat_api_token: "token".to_owned(),
            ffplay_path: PathBuf::from("ffplay.exe"),
        };

        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn builds_player_runtime_plan() {
        let session = session();
        let plan = player_runtime_plan(&session, 1).unwrap();

        assert_eq!(plan.session_id, "session-1");
        assert_eq!(plan.media.udp_port, 41_000);
        assert_eq!(plan.input.destination, "192.0.2.68:42000");
    }

    fn session() -> Session {
        Session {
            id: "session-1".to_owned(),
            game_id: "tmnt".to_owned(),
            seat_id: "windows-seat-1".to_owned(),
            destination_address: "192.0.2.10".to_owned(),
            runtime_host_id: "reference-linux".to_owned(),
            runtime_profile: GameRuntimeProfile {
                width: 320,
                height: 224,
                refresh_hz: 60.0,
                rotation_degrees: 0,
                max_players: 4,
                buttons_per_player: 2,
                supports_save_state: true,
            },
            state: SessionState::Active,
            connection_grant: connection_grant(),
            player_slots: vec![PlayerSlot {
                player_number: 1,
                state: PlayerSlotState::Occupied,
                seat_id: Some("windows-seat-1".to_owned()),
                lease_expires_unix_ms: None,
                presentation: PlayerSlotPresentation {
                    label: "P1".to_owned(),
                    position: None,
                    character: None,
                    artwork_path: None,
                },
            }],
            active_spectator_count: 0,
            created_unix_ms: 1,
            updated_unix_ms: 2,
            failure_reason: None,
        }
    }

    fn connection_grant() -> ConnectionGrant {
        ConnectionGrant {
            token: "00112233-4455-6677-8899-aabbccddeeff".to_owned(),
            expires_unix_ms: 999,
            runtime_host_id: "reference-linux".to_owned(),
            runtime_host_address: "192.0.2.68".to_owned(),
            media_udp_port: 41_000,
            input_udp_port: 42_000,
        }
    }
}
