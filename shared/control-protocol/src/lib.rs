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
    #[serde(default)]
    pub data_plane_address: String,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameMetadata {
    pub sort_title: Option<String>,
    pub description: Option<String>,
    pub genre: Option<String>,
    pub release_year: Option<u16>,
    pub manufacturer: Option<String>,
    pub player_count: Option<u32>,
    pub artwork_path: Option<String>,
    pub marquee_path: Option<String>,
    pub screenshot_path: Option<String>,
    pub logo_path: Option<String>,
    pub control_notes: Option<String>,
    #[serde(default)]
    pub player_slots: Vec<GamePlayerSlotMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GamePlayerSlotMetadata {
    pub player_number: u32,
    pub label: Option<String>,
    pub position: Option<String>,
    pub character: Option<String>,
    pub artwork_path: Option<String>,
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
    #[serde(default)]
    pub metadata: GameMetadata,
    pub availability: Vec<GameAvailability>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogGameList {
    pub games: Vec<CatalogGame>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateGameMetadataRequest {
    pub metadata: GameMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub game_id: String,
    pub seat_id: String,
    pub destination_address: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReservePlayerSlotRequest {
    pub seat_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSpectatorGrantRequest {
    pub seat_id: String,
    pub destination_address: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionGrant {
    pub token: String,
    pub expires_unix_ms: u64,
    pub runtime_host_id: String,
    pub runtime_host_address: String,
    pub media_udp_port: u16,
    pub input_udp_port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerSlotState {
    Open,
    Reserved,
    Occupied,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerSlotPresentation {
    pub label: String,
    pub position: Option<String>,
    pub character: Option<String>,
    pub artwork_path: Option<String>,
}

impl Default for PlayerSlotPresentation {
    fn default() -> Self {
        Self {
            label: "Player".to_owned(),
            position: None,
            character: None,
            artwork_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerSlot {
    pub player_number: u32,
    pub state: PlayerSlotState,
    pub seat_id: Option<String>,
    pub lease_expires_unix_ms: Option<u64>,
    #[serde(default)]
    pub presentation: PlayerSlotPresentation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpectatorGrant {
    pub id: String,
    pub session_id: String,
    pub seat_id: String,
    pub destination_address: String,
    pub runtime_host_id: String,
    pub runtime_host_address: String,
    pub media_udp_port: u16,
    pub expires_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub game_id: String,
    pub seat_id: String,
    pub destination_address: String,
    pub runtime_host_id: String,
    pub runtime_profile: GameRuntimeProfile,
    pub state: SessionState,
    pub connection_grant: ConnectionGrant,
    #[serde(default)]
    pub player_slots: Vec<PlayerSlot>,
    #[serde(default)]
    pub active_spectator_count: u32,
    pub created_unix_ms: u64,
    pub updated_unix_ms: u64,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionList {
    pub sessions: Vec<Session>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewStatus {
    Unavailable,
    ArtworkAvailable,
    SpectatorAvailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub game_id: String,
    pub runtime_host_id: String,
    pub runtime_profile: GameRuntimeProfile,
    pub state: SessionState,
    #[serde(default)]
    pub player_slots: Vec<PlayerSlot>,
    #[serde(default)]
    pub active_spectator_count: u32,
    pub preview_status: PreviewStatus,
    pub preview_asset_path: Option<String>,
    pub updated_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummaryList {
    pub sessions: Vec<SessionSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSessionAssignment {
    pub session_id: String,
    pub game_id: String,
    pub rom_name: String,
    pub destination_address: String,
    pub media_udp_port: u16,
    #[serde(default)]
    pub spectator_media_ports: Vec<u16>,
    pub input_udp_port: u16,
    pub input_token: String,
    pub runtime_profile: GameRuntimeProfile,
    pub state: SessionState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSessionAssignmentList {
    pub sessions: Vec<RuntimeSessionAssignment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSessionState {
    pub state: SessionState,
    pub failure_reason: Option<String>,
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
        API_VERSION, ApiInfo, GameMetadata, GamePlayerSlotMetadata, GameRuntimeProfile, PlayerSlot,
        PlayerSlotPresentation, PlayerSlotState, PreviewStatus, RuntimeHost,
        RuntimeHostCapabilities, RuntimeHostStatus, ServiceStatus, SessionState, SessionSummary,
        SpectatorGrant, StatusResponse, UpdateGameMetadataRequest,
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
    fn spectator_grant_payload_has_stable_field_names() {
        let grant = SpectatorGrant {
            id: "grant-one".to_owned(),
            session_id: "session-one".to_owned(),
            seat_id: "seat-two".to_owned(),
            destination_address: "192.0.2.25".to_owned(),
            runtime_host_id: "reference-linux".to_owned(),
            runtime_host_address: "192.0.2.10".to_owned(),
            media_udp_port: 41_001,
            expires_unix_ms: 123_456,
        };

        let value = serde_json::to_value(grant).unwrap();
        assert_eq!(value["session_id"], "session-one");
        assert_eq!(value["media_udp_port"], 41_001);
        assert_eq!(value["expires_unix_ms"], 123_456);
    }

    #[test]
    fn session_summary_payload_has_stable_field_names() {
        let summary = SessionSummary {
            id: "session-one".to_owned(),
            game_id: "tmnt".to_owned(),
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
            player_slots: Vec::new(),
            active_spectator_count: 1,
            preview_status: PreviewStatus::SpectatorAvailable,
            preview_asset_path: Some("media/tmnt/screenshot.svg".to_owned()),
            updated_unix_ms: 123_456,
        };

        let value = serde_json::to_value(summary).unwrap();
        assert_eq!(value["id"], "session-one");
        assert_eq!(value["game_id"], "tmnt");
        assert_eq!(value["runtime_host_id"], "reference-linux");
        assert_eq!(value["state"], "active");
        assert_eq!(value["active_spectator_count"], 1);
        assert_eq!(value["preview_status"], "spectator_available");
        assert_eq!(value["preview_asset_path"], "media/tmnt/screenshot.svg");
    }

    #[test]
    fn player_slot_payload_has_stable_presentation_fields() {
        let slot = PlayerSlot {
            player_number: 2,
            state: PlayerSlotState::Open,
            seat_id: None,
            lease_expires_unix_ms: None,
            presentation: PlayerSlotPresentation {
                label: "Donatello".to_owned(),
                position: Some("P2".to_owned()),
                character: Some("Donatello".to_owned()),
                artwork_path: Some("media/tmnt/p2.svg".to_owned()),
            },
        };

        let value = serde_json::to_value(slot).unwrap();
        assert_eq!(value["player_number"], 2);
        assert_eq!(value["state"], "open");
        assert_eq!(value["presentation"]["label"], "Donatello");
        assert_eq!(value["presentation"]["position"], "P2");
        assert_eq!(value["presentation"]["character"], "Donatello");
        assert_eq!(value["presentation"]["artwork_path"], "media/tmnt/p2.svg");
    }

    #[test]
    fn game_metadata_update_payload_has_stable_field_names() {
        let request = UpdateGameMetadataRequest {
            metadata: GameMetadata {
                sort_title: Some("Teenage Mutant Ninja Turtles".to_owned()),
                description: Some("Four-player arcade brawler.".to_owned()),
                genre: Some("Beat 'em up".to_owned()),
                release_year: Some(1989),
                manufacturer: Some("Konami".to_owned()),
                player_count: Some(4),
                artwork_path: Some("media/tmnt/artwork.png".to_owned()),
                marquee_path: Some("media/tmnt/marquee.png".to_owned()),
                screenshot_path: Some("media/tmnt/screen.png".to_owned()),
                logo_path: Some("media/tmnt/logo.png".to_owned()),
                control_notes: Some("Jump and attack.".to_owned()),
                player_slots: vec![GamePlayerSlotMetadata {
                    player_number: 2,
                    label: Some("Donatello".to_owned()),
                    position: Some("P2".to_owned()),
                    character: Some("Donatello".to_owned()),
                    artwork_path: Some("media/tmnt/p2.svg".to_owned()),
                }],
            },
        };

        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["metadata"]["player_count"], 4);
        assert_eq!(value["metadata"]["marquee_path"], "media/tmnt/marquee.png");
        assert_eq!(value["metadata"]["release_year"], 1989);
        assert_eq!(value["metadata"]["player_slots"][0]["player_number"], 2);
        assert_eq!(value["metadata"]["player_slots"][0]["label"], "Donatello");
    }

    #[test]
    fn runtime_host_payload_has_stable_field_names() {
        let host = RuntimeHost {
            id: "reference-linux".to_owned(),
            display_name: "Reference Linux Host".to_owned(),
            agent_version: "0.1.0".to_owned(),
            capabilities: RuntimeHostCapabilities {
                data_plane_address: "192.0.2.10".to_owned(),
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
