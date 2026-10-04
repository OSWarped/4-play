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
    use super::{API_VERSION, ApiInfo, ServiceStatus, SessionState, StatusResponse};

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
}
