use std::path::PathBuf;

use control_protocol::{Session, SpectatorGrant};
use input_protocol::SessionToken;
use seat_client_core::{
    InputForwardingPlan, MediaReceiverPlan, PublicRuntimeHandoff, RuntimePlanError,
    public_player_handoff, public_spectator_handoff, spectator_runtime_plan,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePlayerLaunchPlan {
    pub public_handoff: PublicRuntimeHandoff,
    pub media_process: ProcessSpec,
    pub input: NativeInputForwardingPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSpectatorLaunchPlan {
    pub public_handoff: PublicRuntimeHandoff,
    pub media_process: ProcessSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeInputForwardingPlan {
    pub destination: String,
    pub token: SessionToken,
    pub player_number: u32,
}

impl From<InputForwardingPlan> for NativeInputForwardingPlan {
    fn from(plan: InputForwardingPlan) -> Self {
        Self {
            destination: plan.destination,
            token: plan.token,
            player_number: plan.player_number,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeRuntimeStatus {
    pub mode: NativeRuntimeMode,
    pub session_id: String,
    pub game_id: String,
    pub media_udp_port: u16,
    pub input_destination: Option<String>,
    pub player_number: Option<u32>,
    pub media_running: bool,
    pub input_running: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeRuntimeMode {
    Player,
    Spectator,
}

pub fn plan_native_player_launch(
    session: &Session,
    player_number: u32,
    ffplay_path: PathBuf,
) -> Result<NativePlayerLaunchPlan, RuntimePlanError> {
    let private_plan = seat_client_core::player_runtime_plan(session, player_number)?;
    Ok(NativePlayerLaunchPlan {
        public_handoff: public_player_handoff(session, player_number)?,
        media_process: media_process_spec(&private_plan.media, ffplay_path),
        input: private_plan.input.into(),
    })
}

pub fn plan_native_spectator_launch(
    session: &Session,
    grant: &SpectatorGrant,
    ffplay_path: PathBuf,
) -> NativeSpectatorLaunchPlan {
    let private_plan = spectator_runtime_plan(grant);
    NativeSpectatorLaunchPlan {
        public_handoff: public_spectator_handoff(session, grant),
        media_process: media_process_spec(&private_plan.media, ffplay_path),
    }
}

pub fn media_process_spec(plan: &MediaReceiverPlan, ffplay_path: PathBuf) -> ProcessSpec {
    ProcessSpec {
        program: ffplay_path,
        args: plan.ffplay_args.clone(),
    }
}

impl NativePlayerLaunchPlan {
    pub fn safe_status(&self) -> NativeRuntimeStatus {
        NativeRuntimeStatus {
            mode: NativeRuntimeMode::Player,
            session_id: self.public_handoff.session_id.clone(),
            game_id: self.public_handoff.game_id.clone(),
            media_udp_port: self.public_handoff.media.udp_port,
            input_destination: Some(self.input.destination.clone()),
            player_number: Some(self.input.player_number),
            media_running: false,
            input_running: false,
        }
    }
}

impl NativeSpectatorLaunchPlan {
    pub fn safe_status(&self) -> NativeRuntimeStatus {
        NativeRuntimeStatus {
            mode: NativeRuntimeMode::Spectator,
            session_id: self.public_handoff.session_id.clone(),
            game_id: self.public_handoff.game_id.clone(),
            media_udp_port: self.public_handoff.media.udp_port,
            input_destination: None,
            player_number: None,
            media_running: false,
            input_running: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use control_protocol::{
        ConnectionGrant, GameRuntimeProfile, PlayerSlot, PlayerSlotPresentation, PlayerSlotState,
        SessionState,
    };

    #[test]
    fn player_launch_plan_keeps_input_token_out_of_process_args_and_status() {
        let launch = plan_native_player_launch(&session(), 1, PathBuf::from("ffplay.exe")).unwrap();

        assert_eq!(launch.media_process.program, PathBuf::from("ffplay.exe"));
        assert!(
            launch
                .media_process
                .args
                .iter()
                .any(|arg| arg == "low_delay")
        );
        assert_eq!(launch.input.destination, "192.0.2.68:42000");

        let token = "00112233-4455-6677-8899-aabbccddeeff";
        let process_json = serde_json::to_string(&launch.media_process).unwrap();
        let status_json = serde_json::to_string(&launch.safe_status()).unwrap();
        let public_json = serde_json::to_string(&launch.public_handoff).unwrap();

        assert!(!process_json.contains(token));
        assert!(!status_json.contains(token));
        assert!(!public_json.contains(token));
        assert!(!status_json.contains("token"));
    }

    #[test]
    fn spectator_launch_plan_has_media_only_status() {
        let launch =
            plan_native_spectator_launch(&session(), &spectator_grant(), PathBuf::from("ffplay"));
        let status = launch.safe_status();

        assert_eq!(status.mode, NativeRuntimeMode::Spectator);
        assert_eq!(status.media_udp_port, 41_002);
        assert_eq!(status.input_destination, None);
        assert_eq!(status.player_number, None);
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
                presentation: PlayerSlotPresentation::default(),
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

    fn spectator_grant() -> SpectatorGrant {
        SpectatorGrant {
            id: "grant-1".to_owned(),
            session_id: "session-1".to_owned(),
            seat_id: "windows-seat-2".to_owned(),
            destination_address: "192.0.2.10".to_owned(),
            media_udp_port: 41_002,
            runtime_host_id: "reference-linux".to_owned(),
            runtime_host_address: "192.0.2.68".to_owned(),
            expires_unix_ms: 999,
        }
    }
}
