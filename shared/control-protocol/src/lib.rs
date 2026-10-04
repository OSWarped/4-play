use serde::{Deserialize, Serialize};

pub const API_VERSION: &str = "v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiInfo {
    pub service: String,
    pub api_version: String,
}

impl ApiInfo {
    pub fn control_plane() -> Self {
        Self {
            service: "4-play-control-plane".to_owned(),
            api_version: API_VERSION.to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    Ok,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponse {
    pub status: ServiceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeHostCapabilities {
    pub operating_system: String,
    pub architecture: String,
    pub logical_cpu_count: u32,
    pub memory_bytes: u64,
    pub encoder_names: Vec<String>,
    pub emulator_adapters: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterRuntimeHost {
    pub display_name: String,
    pub agent_version: String,
    pub capabilities: RuntimeHostCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeHostHeartbeat {
    pub sequence: u64,
    pub active_session_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeHostStatus {
    Online,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeHost {
    pub id: String,
    pub display_name: String,
    pub agent_version: String,
    pub capabilities: RuntimeHostCapabilities,
    pub status: RuntimeHostStatus,
    pub last_seen_unix_ms: u64,
    pub heartbeat_sequence: u64,
    pub active_session_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeHostList {
    pub hosts: Vec<RuntimeHost>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameRuntimeProfile {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f64,
    pub rotation_degrees: u16,
    pub max_players: u32,
    pub buttons_per_player: u32,
    pub supports_save_state: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveredGame {
    pub id: String,
    pub display_name: String,
    pub rom_name: String,
    pub profile: GameRuntimeProfile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeHostCatalog {
    pub games: Vec<DiscoveredGame>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameAvailability {
    pub runtime_host_id: String,
    pub runtime_host_status: RuntimeHostStatus,
    pub profile: GameRuntimeProfile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogGame {
    pub id: String,
    pub display_name: String,
    pub rom_name: String,
    pub availability: Vec<GameAvailability>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogGameList {
    pub games: Vec<CatalogGame>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Requested,
    Allocating,
    Starting,
    Ready,
    Active,
    Stopping,
    Stopped,
    AllocationFailed,
    LaunchFailed,
    RuntimeLost,
    Unhealthy,
    Terminated,
}

impl SessionState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Stopped
                | Self::AllocationFailed
                | Self::LaunchFailed
                | Self::RuntimeLost
                | Self::Terminated
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        API_VERSION, ApiInfo, RuntimeHost, RuntimeHostCapabilities, RuntimeHostStatus,
        ServiceStatus, SessionState, StatusResponse,
    };

    #[test]
    fn session_states_use_stable_snake_case_names() {
        let encoded = serde_json::to_string(&SessionState::AllocationFailed).unwrap();
        assert_eq!(encoded, "\"allocation_failed\"");
        assert_eq!(
            serde_json::from_str::<SessionState>(&encoded).unwrap(),
            SessionState::AllocationFailed
        );
    }

    #[test]
    fn terminal_states_are_explicit() {
        assert!(SessionState::Stopped.is_terminal());
        assert!(SessionState::RuntimeLost.is_terminal());
        assert!(!SessionState::Active.is_terminal());
        assert!(!SessionState::Unhealthy.is_terminal());
    }

    #[test]
    fn api_and_status_payloads_are_serializable() {
        let info = ApiInfo::control_plane();
        assert_eq!(info.api_version, API_VERSION);
        assert_eq!(
            serde_json::to_value(StatusResponse {
                status: ServiceStatus::Ready,
            })
            .unwrap(),
            serde_json::json!({ "status": "ready" })
        );
    }

    #[test]
    fn runtime_host_payload_has_stable_field_names() {
        let host = RuntimeHost {
            id: "reference-linux".to_owned(),
            display_name: "Reference Linux Host".to_owned(),
            agent_version: "0.1.0".to_owned(),
            capabilities: RuntimeHostCapabilities {
                operating_system: "linux".to_owned(),
                architecture: "x86_64".to_owned(),
                logical_cpu_count: 4,
                memory_bytes: 16 * 1024 * 1024 * 1024,
                encoder_names: vec!["libx264".to_owned()],
                emulator_adapters: vec!["mame".to_owned()],
            },
            status: RuntimeHostStatus::Online,
            last_seen_unix_ms: 1_000,
            heartbeat_sequence: 7,
            active_session_count: 2,
        };

        let value = serde_json::to_value(host).unwrap();
        assert_eq!(value["status"], "online");
        assert_eq!(value["heartbeat_sequence"], 7);
        assert_eq!(value["capabilities"]["emulator_adapters"][0], "mame");
    }
}
