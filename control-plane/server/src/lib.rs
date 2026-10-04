use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use control_protocol::{
    ApiInfo, ErrorResponse, RegisterRuntimeHost, RuntimeHost, RuntimeHostHeartbeat,
    RuntimeHostList, RuntimeHostStatus, ServiceStatus, StatusResponse,
};
use tokio::sync::RwLock;

#[derive(Clone, Default)]
pub struct AppState {
    runtime_hosts: Arc<RwLock<HashMap<String, RuntimeHost>>>,
}

pub fn app() -> Router {
    app_with_state(AppState::default())
}

pub fn app_with_state(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(readiness))
        .route("/api/v1", get(api_info))
        .route("/api/v1/runtime-hosts", get(list_runtime_hosts))
        .route(
            "/api/v1/runtime-hosts/{host_id}",
            get(get_runtime_host).put(register_runtime_host),
        )
        .route(
            "/api/v1/runtime-hosts/{host_id}/heartbeat",
            post(record_runtime_host_heartbeat),
        )
        .with_state(state)
}

async fn health() -> Json<StatusResponse> {
    Json(StatusResponse {
        status: ServiceStatus::Ok,
    })
}

async fn readiness() -> Json<StatusResponse> {
    Json(StatusResponse {
        status: ServiceStatus::Ready,
    })
}

async fn api_info() -> Json<ApiInfo> {
    Json(ApiInfo::control_plane())
}

async fn list_runtime_hosts(State(state): State<AppState>) -> Json<RuntimeHostList> {
    let mut hosts = state
        .runtime_hosts
        .read()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    hosts.sort_by(|left, right| left.id.cmp(&right.id));
    Json(RuntimeHostList { hosts })
}

async fn get_runtime_host(
    State(state): State<AppState>,
    Path(host_id): Path<String>,
) -> Result<Json<RuntimeHost>, ApiError> {
    let hosts = state.runtime_hosts.read().await;
    hosts
        .get(&host_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::not_found("runtime_host_not_found", "runtime host was not found"))
}

async fn register_runtime_host(
    State(state): State<AppState>,
    Path(host_id): Path<String>,
    Json(registration): Json<RegisterRuntimeHost>,
) -> Result<(StatusCode, Json<RuntimeHost>), ApiError> {
    validate_host_id(&host_id)?;
    validate_registration(&registration)?;

    let mut hosts = state.runtime_hosts.write().await;
    let status = if hosts.contains_key(&host_id) {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    let previous = hosts.get(&host_id);
    let host = RuntimeHost {
        id: host_id.clone(),
        display_name: registration.display_name,
        agent_version: registration.agent_version,
        capabilities: registration.capabilities,
        status: RuntimeHostStatus::Online,
        last_seen_unix_ms: unix_time_ms(),
        heartbeat_sequence: previous.map_or(0, |host| host.heartbeat_sequence),
        active_session_count: previous.map_or(0, |host| host.active_session_count),
    };
    hosts.insert(host_id, host.clone());

    Ok((status, Json(host)))
}

async fn record_runtime_host_heartbeat(
    State(state): State<AppState>,
    Path(host_id): Path<String>,
    Json(heartbeat): Json<RuntimeHostHeartbeat>,
) -> Result<Json<RuntimeHost>, ApiError> {
    let mut hosts = state.runtime_hosts.write().await;
    let host = hosts.get_mut(&host_id).ok_or_else(|| {
        ApiError::not_found("runtime_host_not_found", "runtime host was not found")
    })?;

    if heartbeat.sequence < host.heartbeat_sequence {
        return Err(ApiError::conflict(
            "stale_heartbeat",
            "heartbeat sequence is older than the last accepted sequence",
        ));
    }
    if heartbeat.sequence == host.heartbeat_sequence
        && heartbeat.active_session_count != host.active_session_count
    {
        return Err(ApiError::conflict(
            "heartbeat_sequence_conflict",
            "a heartbeat with this sequence was already accepted with different data",
        ));
    }

    host.heartbeat_sequence = heartbeat.sequence;
    host.active_session_count = heartbeat.active_session_count;
    host.last_seen_unix_ms = unix_time_ms();
    host.status = RuntimeHostStatus::Online;

    Ok(Json(host.clone()))
}

fn validate_host_id(host_id: &str) -> Result<(), ApiError> {
    let valid = !host_id.is_empty()
        && host_id.len() <= 64
        && host_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "invalid_runtime_host_id",
            "runtime host ID must contain 1-64 ASCII letters, digits, dots, dashes, or underscores",
        ))
    }
}

fn validate_registration(registration: &RegisterRuntimeHost) -> Result<(), ApiError> {
    if registration.display_name.trim().is_empty() {
        return Err(ApiError::bad_request(
            "invalid_display_name",
            "display name must not be empty",
        ));
    }
    if registration.agent_version.trim().is_empty() {
        return Err(ApiError::bad_request(
            "invalid_agent_version",
            "agent version must not be empty",
        ));
    }
    if registration.capabilities.logical_cpu_count == 0 {
        return Err(ApiError::bad_request(
            "invalid_logical_cpu_count",
            "logical CPU count must be greater than zero",
        ));
    }
    Ok(())
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

struct ApiError {
    status: StatusCode,
    body: ErrorResponse,
}

impl ApiError {
    fn bad_request(code: &str, message: &str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    fn not_found(code: &str, message: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, message)
    }

    fn conflict(code: &str, message: &str) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    fn new(status: StatusCode, code: &str, message: &str) -> Self {
        Self {
            status,
            body: ErrorResponse {
                code: code.to_owned(),
                message: message.to_owned(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::app;
    use axum::{
        body::Body,
        http::{Method, Request},
    };
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    fn registration() -> Value {
        json!({
            "display_name": "Reference Linux Host",
            "agent_version": "0.1.0",
            "capabilities": {
                "operating_system": "linux",
                "architecture": "x86_64",
                "logical_cpu_count": 4,
                "memory_bytes": 17_179_869_184_u64,
                "encoder_names": ["libx264", "h264_qsv"],
                "emulator_adapters": ["mame"]
            }
        })
    }

    async fn request_json(
        app: axum::Router,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = Request::builder().method(method).uri(path);
        let body = if let Some(body) = body {
            request = request.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&body).unwrap())
        } else {
            Body::empty()
        };
        let response = app.oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status().as_u16();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&body).unwrap())
    }

    async fn get_json(path: &str) -> (u16, Value) {
        request_json(app(), Method::GET, path, None).await
    }

    #[tokio::test]
    async fn health_endpoint_reports_ok() {
        let (status, body) = get_json("/health").await;
        assert_eq!(status, 200);
        assert_eq!(body, json!({ "status": "ok" }));
    }

    #[tokio::test]
    async fn readiness_endpoint_reports_ready() {
        let (status, body) = get_json("/ready").await;
        assert_eq!(status, 200);
        assert_eq!(body, json!({ "status": "ready" }));
    }

    #[tokio::test]
    async fn versioned_api_root_describes_the_service() {
        let (status, body) = get_json("/api/v1").await;
        assert_eq!(status, 200);
        assert_eq!(
            body,
            json!({
                "service": "4-play-control-plane",
                "api_version": "v1"
            })
        );
    }

    #[tokio::test]
    async fn runtime_host_registration_is_idempotent_and_listed() {
        let service = app();
        let (created_status, created) = request_json(
            service.clone(),
            Method::PUT,
            "/api/v1/runtime-hosts/reference-linux",
            Some(registration()),
        )
        .await;
        assert_eq!(created_status, 201);
        assert_eq!(created["id"], "reference-linux");
        assert_eq!(created["status"], "online");

        let (updated_status, updated) = request_json(
            service.clone(),
            Method::PUT,
            "/api/v1/runtime-hosts/reference-linux",
            Some(registration()),
        )
        .await;
        assert_eq!(updated_status, 200);
        assert_eq!(updated["id"], created["id"]);

        let (list_status, list) =
            request_json(service, Method::GET, "/api/v1/runtime-hosts", None).await;
        assert_eq!(list_status, 200);
        assert_eq!(list["hosts"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn heartbeat_updates_registered_host_and_accepts_exact_retry() {
        let service = app();
        request_json(
            service.clone(),
            Method::PUT,
            "/api/v1/runtime-hosts/reference-linux",
            Some(registration()),
        )
        .await;
        let heartbeat = json!({ "sequence": 1, "active_session_count": 2 });

        let (status, host) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/runtime-hosts/reference-linux/heartbeat",
            Some(heartbeat.clone()),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(host["heartbeat_sequence"], 1);
        assert_eq!(host["active_session_count"], 2);

        let (retry_status, _) = request_json(
            service,
            Method::POST,
            "/api/v1/runtime-hosts/reference-linux/heartbeat",
            Some(heartbeat),
        )
        .await;
        assert_eq!(retry_status, 200);
    }

    #[tokio::test]
    async fn stale_or_conflicting_heartbeats_are_rejected() {
        let service = app();
        request_json(
            service.clone(),
            Method::PUT,
            "/api/v1/runtime-hosts/reference-linux",
            Some(registration()),
        )
        .await;
        request_json(
            service.clone(),
            Method::POST,
            "/api/v1/runtime-hosts/reference-linux/heartbeat",
            Some(json!({ "sequence": 2, "active_session_count": 1 })),
        )
        .await;

        let (stale_status, stale) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/runtime-hosts/reference-linux/heartbeat",
            Some(json!({ "sequence": 1, "active_session_count": 1 })),
        )
        .await;
        assert_eq!(stale_status, 409);
        assert_eq!(stale["code"], "stale_heartbeat");

        let (conflict_status, conflict) = request_json(
            service,
            Method::POST,
            "/api/v1/runtime-hosts/reference-linux/heartbeat",
            Some(json!({ "sequence": 2, "active_session_count": 3 })),
        )
        .await;
        assert_eq!(conflict_status, 409);
        assert_eq!(conflict["code"], "heartbeat_sequence_conflict");
    }

    #[tokio::test]
    async fn heartbeat_requires_a_registered_host() {
        let (status, body) = request_json(
            app(),
            Method::POST,
            "/api/v1/runtime-hosts/missing/heartbeat",
            Some(json!({ "sequence": 1, "active_session_count": 0 })),
        )
        .await;
        assert_eq!(status, 404);
        assert_eq!(body["code"], "runtime_host_not_found");
    }

    #[tokio::test]
    async fn registration_validates_host_identity_and_capabilities() {
        let (invalid_id_status, invalid_id) = request_json(
            app(),
            Method::PUT,
            "/api/v1/runtime-hosts/not%20safe",
            Some(registration()),
        )
        .await;
        assert_eq!(invalid_id_status, 400);
        assert_eq!(invalid_id["code"], "invalid_runtime_host_id");

        let mut invalid_registration = registration();
        invalid_registration["capabilities"]["logical_cpu_count"] = json!(0);
        let (invalid_capabilities_status, invalid_capabilities) = request_json(
            app(),
            Method::PUT,
            "/api/v1/runtime-hosts/reference-linux",
            Some(invalid_registration),
        )
        .await;
        assert_eq!(invalid_capabilities_status, 400);
        assert_eq!(invalid_capabilities["code"], "invalid_logical_cpu_count");
    }

    #[tokio::test]
    async fn unknown_routes_return_not_found() {
        let response = app()
            .oneshot(Request::get("/api/v2").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
    }
}
