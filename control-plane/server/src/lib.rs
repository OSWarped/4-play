mod store;

use std::{
    net::IpAddr,
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, Request, StatusCode, header::CONTENT_TYPE},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use control_protocol::{
    ApiInfo, CatalogGame, CatalogGameList, CreateSessionRequest, CreateSpectatorGrantRequest,
    ErrorResponse, GameMetadata, PreviewStatus, RegisterRuntimeHost, ReservePlayerSlotRequest,
    RuntimeHost, RuntimeHostCatalog, RuntimeHostHeartbeat, RuntimeHostList,
    RuntimeSessionAssignmentList, ServiceStatus, Session, SessionList, SessionSummary,
    SessionSummaryList, SpectatorGrant, StatusResponse, UpdateGameMetadataRequest,
    UpdateSessionState,
};
use store::RuntimeHostStore;
pub use store::StoreError;

pub const DEFAULT_OFFLINE_AFTER: Duration = Duration::from_secs(15);
pub const DEFAULT_GRANT_TTL: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortPoolConfig {
    pub media_port_start: u16,
    pub media_port_count: u16,
    pub input_port_start: u16,
    pub input_port_count: u16,
}

impl Default for PortPoolConfig {
    fn default() -> Self {
        Self {
            media_port_start: 41_000,
            media_port_count: 1_000,
            input_port_start: 42_000,
            input_port_count: 1_000,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    runtime_hosts: RuntimeHostStore,
    offline_after_ms: u64,
    grant_ttl_ms: u64,
    port_pools: PortPoolConfig,
    seat_api_token: String,
    runtime_host_api_token: String,
    asset_root: Option<PathBuf>,
}

impl AppState {
    async fn in_memory(offline_after: Duration) -> Result<Self, StoreError> {
        Ok(Self {
            runtime_hosts: RuntimeHostStore::in_memory().await?,
            offline_after_ms: duration_ms(offline_after),
            grant_ttl_ms: duration_ms(DEFAULT_GRANT_TTL),
            port_pools: PortPoolConfig::default(),
            seat_api_token: "test-seat-token".to_owned(),
            runtime_host_api_token: "test-runtime-host-token".to_owned(),
            asset_root: None,
        })
    }

    async fn persistent(
        path: impl AsRef<Path>,
        offline_after: Duration,
        seat_api_token: String,
        runtime_host_api_token: String,
        port_pools: PortPoolConfig,
        asset_root: Option<PathBuf>,
    ) -> Result<Self, StoreError> {
        Ok(Self {
            runtime_hosts: RuntimeHostStore::open(path).await?,
            offline_after_ms: duration_ms(offline_after),
            grant_ttl_ms: duration_ms(DEFAULT_GRANT_TTL),
            port_pools,
            seat_api_token,
            runtime_host_api_token,
            asset_root,
        })
    }
}

pub async fn app() -> Result<Router, StoreError> {
    Ok(app_with_state(
        AppState::in_memory(DEFAULT_OFFLINE_AFTER).await?,
    ))
}

pub async fn app_with_database(
    path: impl AsRef<Path>,
    offline_after: Duration,
) -> Result<Router, StoreError> {
    Ok(app_with_state(
        AppState::persistent(
            path,
            offline_after,
            "test-seat-token".to_owned(),
            "test-runtime-host-token".to_owned(),
            PortPoolConfig::default(),
            None,
        )
        .await?,
    ))
}

pub async fn app_with_database_and_tokens(
    path: impl AsRef<Path>,
    offline_after: Duration,
    seat_api_token: String,
    runtime_host_api_token: String,
) -> Result<Router, StoreError> {
    app_with_database_tokens_and_ports(
        path,
        offline_after,
        seat_api_token,
        runtime_host_api_token,
        PortPoolConfig::default(),
    )
    .await
}

pub async fn app_with_database_tokens_and_ports(
    path: impl AsRef<Path>,
    offline_after: Duration,
    seat_api_token: String,
    runtime_host_api_token: String,
    port_pools: PortPoolConfig,
) -> Result<Router, StoreError> {
    app_with_database_tokens_ports_and_assets(
        path,
        offline_after,
        seat_api_token,
        runtime_host_api_token,
        port_pools,
        None,
    )
    .await
}

pub async fn app_with_database_tokens_ports_and_assets(
    path: impl AsRef<Path>,
    offline_after: Duration,
    seat_api_token: String,
    runtime_host_api_token: String,
    port_pools: PortPoolConfig,
    asset_root: Option<PathBuf>,
) -> Result<Router, StoreError> {
    Ok(app_with_state(
        AppState::persistent(
            path,
            offline_after,
            seat_api_token,
            runtime_host_api_token,
            port_pools,
            asset_root,
        )
        .await?,
    ))
}

pub fn app_with_state(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/v1/runtime-hosts", get(list_runtime_hosts))
        .route("/api/v1/games", get(list_catalog_games))
        .route("/api/v1/games/{game_id}", get(get_catalog_game))
        .route(
            "/api/v1/games/{game_id}/metadata",
            get(get_game_metadata).put(update_game_metadata),
        )
        .route("/api/v1/assets/{*asset_path}", get(get_asset))
        .route(
            "/api/v1/active-sessions",
            get(list_active_session_summaries),
        )
        .route("/api/v1/sessions", get(list_sessions).post(create_session))
        .route("/api/v1/sessions/{session_id}", get(get_session))
        .route(
            "/api/v1/sessions/{session_id}/spectators",
            post(create_spectator_grant),
        )
        .route(
            "/api/v1/sessions/{session_id}/spectators/{grant_id}",
            delete(release_spectator_grant),
        )
        .route(
            "/api/v1/sessions/{session_id}/player-slots/{player_number}/reserve",
            post(reserve_player_slot),
        )
        .route(
            "/api/v1/sessions/{session_id}/player-slots/{player_number}/connect",
            post(connect_player_slot),
        )
        .route(
            "/api/v1/sessions/{session_id}/player-slots/{player_number}/disconnect",
            post(disconnect_player_slot),
        )
        .route(
            "/api/v1/sessions/{session_id}/player-slots/{player_number}/release",
            post(release_player_slot),
        )
        .route(
            "/api/v1/sessions/{session_id}/stop",
            post(request_session_stop),
        )
        .route(
            "/api/v1/runtime-hosts/{host_id}",
            get(get_runtime_host).put(register_runtime_host),
        )
        .route(
            "/api/v1/runtime-hosts/{host_id}/heartbeat",
            post(record_runtime_host_heartbeat),
        )
        .route(
            "/api/v1/runtime-hosts/{host_id}/catalog",
            axum::routing::put(replace_runtime_host_catalog),
        )
        .route(
            "/api/v1/runtime-hosts/{host_id}/sessions",
            get(list_runtime_assignments),
        )
        .route(
            "/api/v1/runtime-hosts/{host_id}/sessions/{session_id}/state",
            axum::routing::put(update_runtime_session_state),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_api_auth,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/ready", get(readiness))
        .route("/api/v1", get(api_info))
        .merge(protected)
        .with_state(state)
}

async fn require_api_auth(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let authenticated = token.is_some_and(|token| {
        constant_time_equal(token, &state.seat_api_token)
            || constant_time_equal(token, &state.runtime_host_api_token)
    });
    if !authenticated {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "a valid seat or runtime-host bearer token is required",
        ));
    }
    Ok(next.run(request).await)
}

fn constant_time_equal(candidate: &str, expected: &str) -> bool {
    if candidate.len() != expected.len() {
        return false;
    }
    candidate
        .bytes()
        .zip(expected.bytes())
        .fold(0_u8, |difference, (candidate, expected)| {
            difference | (candidate ^ expected)
        })
        == 0
}

async fn health() -> Json<StatusResponse> {
    Json(StatusResponse {
        status: ServiceStatus::Ok,
    })
}

async fn readiness(State(state): State<AppState>) -> Result<Json<StatusResponse>, ApiError> {
    state.runtime_hosts.ping().await.map_err(ApiError::store)?;
    Ok(Json(StatusResponse {
        status: ServiceStatus::Ready,
    }))
}

async fn api_info() -> Json<ApiInfo> {
    Json(ApiInfo::control_plane())
}

async fn list_runtime_hosts(
    State(state): State<AppState>,
) -> Result<Json<RuntimeHostList>, ApiError> {
    let hosts = state
        .runtime_hosts
        .list(unix_time_ms(), state.offline_after_ms)
        .await
        .map_err(ApiError::store)?;
    Ok(Json(RuntimeHostList { hosts }))
}

async fn get_runtime_host(
    State(state): State<AppState>,
    AxumPath(host_id): AxumPath<String>,
) -> Result<Json<RuntimeHost>, ApiError> {
    state
        .runtime_hosts
        .get(host_id, unix_time_ms(), state.offline_after_ms)
        .await
        .map(Json)
        .map_err(ApiError::store)
}

async fn register_runtime_host(
    State(state): State<AppState>,
    AxumPath(host_id): AxumPath<String>,
    Json(registration): Json<RegisterRuntimeHost>,
) -> Result<(StatusCode, Json<RuntimeHost>), ApiError> {
    validate_host_id(&host_id)?;
    validate_registration(&registration)?;

    let (created, host) = state
        .runtime_hosts
        .upsert_registration(host_id, registration, unix_time_ms())
        .await
        .map_err(ApiError::store)?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(host)))
}

async fn record_runtime_host_heartbeat(
    State(state): State<AppState>,
    AxumPath(host_id): AxumPath<String>,
    Json(heartbeat): Json<RuntimeHostHeartbeat>,
) -> Result<Json<RuntimeHost>, ApiError> {
    state
        .runtime_hosts
        .heartbeat(host_id, heartbeat, unix_time_ms())
        .await
        .map(Json)
        .map_err(ApiError::store)
}

async fn replace_runtime_host_catalog(
    State(state): State<AppState>,
    AxumPath(host_id): AxumPath<String>,
    Json(catalog): Json<RuntimeHostCatalog>,
) -> Result<Json<RuntimeHostCatalog>, ApiError> {
    validate_catalog(&catalog)?;
    state
        .runtime_hosts
        .replace_host_catalog(host_id, catalog.clone())
        .await
        .map_err(ApiError::store)?;
    Ok(Json(catalog))
}

async fn list_catalog_games(
    State(state): State<AppState>,
) -> Result<Json<CatalogGameList>, ApiError> {
    let games = state
        .runtime_hosts
        .list_catalog(unix_time_ms(), state.offline_after_ms)
        .await
        .map_err(ApiError::store)?;
    Ok(Json(CatalogGameList { games }))
}

async fn get_catalog_game(
    State(state): State<AppState>,
    AxumPath(game_id): AxumPath<String>,
) -> Result<Json<CatalogGame>, ApiError> {
    state
        .runtime_hosts
        .get_catalog_game(game_id, unix_time_ms(), state.offline_after_ms)
        .await
        .map(Json)
        .map_err(ApiError::store)
}

async fn get_game_metadata(
    State(state): State<AppState>,
    AxumPath(game_id): AxumPath<String>,
) -> Result<Json<GameMetadata>, ApiError> {
    state
        .runtime_hosts
        .get_game_metadata(game_id)
        .await
        .map(Json)
        .map_err(ApiError::store)
}

async fn update_game_metadata(
    State(state): State<AppState>,
    AxumPath(game_id): AxumPath<String>,
    Json(request): Json<UpdateGameMetadataRequest>,
) -> Result<Json<CatalogGame>, ApiError> {
    validate_game_metadata(&request.metadata)?;
    state
        .runtime_hosts
        .update_game_metadata(game_id, request.metadata)
        .await
        .map(Json)
        .map_err(ApiError::store)
}

async fn get_asset(
    State(state): State<AppState>,
    AxumPath(asset_path): AxumPath<String>,
) -> Result<Response, ApiError> {
    let Some(asset_root) = state.asset_root.as_ref() else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "asset_root_not_configured",
            "asset serving is not configured",
        ));
    };
    let relative_path = safe_relative_asset_path(&asset_path)?;
    let full_path = asset_root.join(&relative_path);
    let bytes = tokio::fs::read(&full_path).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "asset_not_found",
                "asset was not found",
            )
        } else {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asset_read_failed",
                "asset could not be read",
            )
        }
    })?;
    Ok((
        [(CONTENT_TYPE, content_type_for_asset(&relative_path))],
        Body::from(bytes),
    )
        .into_response())
}

async fn create_session(
    State(state): State<AppState>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<Session>), ApiError> {
    validate_session_request(&request)?;
    let now = unix_time_ms();
    let session = state
        .runtime_hosts
        .allocate_session(
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
            request,
            now,
            now.saturating_add(state.grant_ttl_ms),
            state.offline_after_ms,
            state.port_pools.media_port_start,
            state.port_pools.media_port_count,
            state.port_pools.input_port_start,
            state.port_pools.input_port_count,
        )
        .await
        .map_err(ApiError::store)?;
    Ok((StatusCode::CREATED, Json(session)))
}

async fn list_sessions(State(state): State<AppState>) -> Result<Json<SessionList>, ApiError> {
    state
        .runtime_hosts
        .list_sessions()
        .await
        .map(|sessions| Json(SessionList { sessions }))
        .map_err(ApiError::store)
}

async fn list_active_session_summaries(
    State(state): State<AppState>,
) -> Result<Json<SessionSummaryList>, ApiError> {
    let mut sessions = state
        .runtime_hosts
        .list_active_session_summaries()
        .await
        .map_err(ApiError::store)?;
    let games = state
        .runtime_hosts
        .list_catalog(unix_time_ms(), state.offline_after_ms)
        .await
        .map_err(ApiError::store)?;
    apply_preview_fallbacks(&mut sessions, &games);
    Ok(Json(SessionSummaryList { sessions }))
}

fn apply_preview_fallbacks(sessions: &mut [SessionSummary], games: &[CatalogGame]) {
    for session in sessions {
        if session.preview_asset_path.is_some() {
            continue;
        }
        let Some(game) = games.iter().find(|game| game.id == session.game_id) else {
            continue;
        };
        let fallback = game
            .metadata
            .screenshot_path
            .as_deref()
            .or(game.metadata.artwork_path.as_deref())
            .or(game.metadata.marquee_path.as_deref())
            .or(game.metadata.logo_path.as_deref());
        if let Some(path) = fallback {
            session.preview_asset_path = Some(path.to_owned());
            if session.preview_status == PreviewStatus::Unavailable {
                session.preview_status = PreviewStatus::ArtworkAvailable;
            }
        }
    }
}

async fn get_session(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Session>, ApiError> {
    state
        .runtime_hosts
        .get_session(session_id)
        .await
        .map(Json)
        .map_err(ApiError::store)
}

async fn request_session_stop(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Session>, ApiError> {
    state
        .runtime_hosts
        .request_session_stop(session_id, unix_time_ms())
        .await
        .map(Json)
        .map_err(ApiError::store)
}

async fn create_spectator_grant(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    Json(request): Json<CreateSpectatorGrantRequest>,
) -> Result<(StatusCode, Json<SpectatorGrant>), ApiError> {
    validate_seat_id(&request.seat_id)?;
    request.destination_address.parse::<IpAddr>().map_err(|_| {
        ApiError::bad_request(
            "invalid_destination_address",
            "destination address must be an IP address",
        )
    })?;
    let now = unix_time_ms();
    let grant = state
        .runtime_hosts
        .create_spectator_grant(
            uuid::Uuid::new_v4().to_string(),
            session_id,
            request.seat_id,
            request.destination_address,
            now,
            now.saturating_add(state.grant_ttl_ms),
            state.port_pools.media_port_start,
            state.port_pools.media_port_count,
        )
        .await
        .map_err(ApiError::store)?;
    Ok((StatusCode::CREATED, Json(grant)))
}

async fn release_spectator_grant(
    State(state): State<AppState>,
    AxumPath((session_id, grant_id)): AxumPath<(String, String)>,
    Json(request): Json<ReservePlayerSlotRequest>,
) -> Result<Json<Session>, ApiError> {
    validate_seat_id(&request.seat_id)?;
    state
        .runtime_hosts
        .release_spectator_grant(session_id, grant_id, request.seat_id, unix_time_ms())
        .await
        .map(Json)
        .map_err(ApiError::store)
}

async fn reserve_player_slot(
    State(state): State<AppState>,
    AxumPath((session_id, player_number)): AxumPath<(String, u32)>,
    Json(request): Json<ReservePlayerSlotRequest>,
) -> Result<Json<Session>, ApiError> {
    validate_seat_id(&request.seat_id)?;
    state
        .runtime_hosts
        .reserve_player_slot(
            session_id,
            player_number,
            request.seat_id,
            unix_time_ms(),
            state.grant_ttl_ms,
        )
        .await
        .map(Json)
        .map_err(ApiError::store)
}

async fn release_player_slot(
    State(state): State<AppState>,
    AxumPath((session_id, player_number)): AxumPath<(String, u32)>,
    Json(request): Json<ReservePlayerSlotRequest>,
) -> Result<Json<Session>, ApiError> {
    validate_seat_id(&request.seat_id)?;
    state
        .runtime_hosts
        .release_player_slot(session_id, player_number, request.seat_id, unix_time_ms())
        .await
        .map(Json)
        .map_err(ApiError::store)
}

async fn connect_player_slot(
    State(state): State<AppState>,
    AxumPath((session_id, player_number)): AxumPath<(String, u32)>,
    Json(request): Json<ReservePlayerSlotRequest>,
) -> Result<Json<Session>, ApiError> {
    validate_seat_id(&request.seat_id)?;
    state
        .runtime_hosts
        .connect_player_slot(session_id, player_number, request.seat_id, unix_time_ms())
        .await
        .map(Json)
        .map_err(ApiError::store)
}

async fn disconnect_player_slot(
    State(state): State<AppState>,
    AxumPath((session_id, player_number)): AxumPath<(String, u32)>,
    Json(request): Json<ReservePlayerSlotRequest>,
) -> Result<Json<Session>, ApiError> {
    validate_seat_id(&request.seat_id)?;
    state
        .runtime_hosts
        .disconnect_player_slot(
            session_id,
            player_number,
            request.seat_id,
            unix_time_ms(),
            state.grant_ttl_ms,
        )
        .await
        .map(Json)
        .map_err(ApiError::store)
}

async fn list_runtime_assignments(
    State(state): State<AppState>,
    AxumPath(host_id): AxumPath<String>,
) -> Result<Json<RuntimeSessionAssignmentList>, ApiError> {
    state
        .runtime_hosts
        .list_runtime_assignments(host_id)
        .await
        .map(|sessions| Json(RuntimeSessionAssignmentList { sessions }))
        .map_err(ApiError::store)
}

async fn update_runtime_session_state(
    State(state): State<AppState>,
    AxumPath((host_id, session_id)): AxumPath<(String, String)>,
    Json(update): Json<UpdateSessionState>,
) -> Result<Json<Session>, ApiError> {
    if update
        .failure_reason
        .as_ref()
        .is_some_and(|reason| reason.len() > 1_024)
    {
        return Err(ApiError::bad_request(
            "failure_reason_too_long",
            "failure reason must not exceed 1024 bytes",
        ));
    }
    if let Some(path) = update.preview_asset_path.as_deref() {
        validate_asset_path("preview_asset_path", path)?;
    }
    state
        .runtime_hosts
        .update_session_state(
            host_id,
            session_id,
            update.state,
            update.failure_reason,
            update.preview_asset_path,
            unix_time_ms(),
        )
        .await
        .map(Json)
        .map_err(ApiError::store)
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
    if registration
        .capabilities
        .data_plane_address
        .trim()
        .is_empty()
        || registration.capabilities.data_plane_address.len() > 253
        || registration
            .capabilities
            .data_plane_address
            .chars()
            .any(char::is_whitespace)
    {
        return Err(ApiError::bad_request(
            "invalid_data_plane_address",
            "data-plane address must be a nonempty IP address or hostname without whitespace",
        ));
    }
    Ok(())
}

fn validate_session_request(request: &CreateSessionRequest) -> Result<(), ApiError> {
    validate_identity("invalid_game_id", "game ID", &request.game_id)?;
    validate_seat_id(&request.seat_id)?;
    request.destination_address.parse::<IpAddr>().map_err(|_| {
        ApiError::bad_request(
            "invalid_destination_address",
            "destination address must be an IPv4 or IPv6 address",
        )
    })?;
    Ok(())
}

fn validate_seat_id(seat_id: &str) -> Result<(), ApiError> {
    validate_identity("invalid_seat_id", "seat ID", seat_id)
}

fn validate_identity(code: &str, label: &str, value: &str) -> Result<(), ApiError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            code,
            &format!(
                "{label} must contain 1-64 ASCII letters, digits, dots, dashes, or underscores"
            ),
        ))
    }
}

fn validate_catalog(catalog: &RuntimeHostCatalog) -> Result<(), ApiError> {
    for game in &catalog.games {
        for (field, value) in [("game ID", &game.id), ("ROM name", &game.rom_name)] {
            let valid = !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
            if !valid {
                return Err(ApiError::bad_request(
                    "invalid_catalog_identity",
                    &format!(
                        "{field} must contain 1-64 ASCII letters, digits, dots, dashes, or underscores"
                    ),
                ));
            }
        }
        if game.display_name.trim().is_empty()
            || game.profile.width == 0
            || game.profile.height == 0
            || !game.profile.refresh_hz.is_finite()
            || game.profile.refresh_hz <= 0.0
            || game.profile.max_players == 0
        {
            return Err(ApiError::bad_request(
                "invalid_runtime_profile",
                "catalog games require a name, dimensions, refresh rate, and player count",
            ));
        }
    }
    Ok(())
}

fn validate_game_metadata(metadata: &GameMetadata) -> Result<(), ApiError> {
    for (field, value, max_len) in [
        ("sort_title", metadata.sort_title.as_deref(), 256),
        ("description", metadata.description.as_deref(), 4_096),
        ("genre", metadata.genre.as_deref(), 128),
        ("manufacturer", metadata.manufacturer.as_deref(), 256),
        ("control_notes", metadata.control_notes.as_deref(), 2_048),
    ] {
        if let Some(value) = value
            && value.len() > max_len
        {
            return Err(ApiError::bad_request(
                "game_metadata_too_long",
                &format!("{field} must not exceed {max_len} bytes"),
            ));
        }
    }
    if metadata
        .player_count
        .is_some_and(|count| count == 0 || count > 16)
    {
        return Err(ApiError::bad_request(
            "invalid_player_count",
            "player count must be between 1 and 16",
        ));
    }
    for (field, value) in [
        ("artwork_path", metadata.artwork_path.as_deref()),
        ("marquee_path", metadata.marquee_path.as_deref()),
        ("screenshot_path", metadata.screenshot_path.as_deref()),
        ("logo_path", metadata.logo_path.as_deref()),
    ] {
        if let Some(value) = value {
            validate_asset_path(field, value)?;
        }
    }
    let mut seen_player_numbers = Vec::new();
    for slot in &metadata.player_slots {
        if slot.player_number == 0 || slot.player_number > 16 {
            return Err(ApiError::bad_request(
                "invalid_player_slot",
                "player slot metadata player_number must be between 1 and 16",
            ));
        }
        if seen_player_numbers.contains(&slot.player_number) {
            return Err(ApiError::bad_request(
                "duplicate_player_slot",
                "player slot metadata must not contain duplicate player_number values",
            ));
        }
        seen_player_numbers.push(slot.player_number);
        for (field, value, max_len) in [
            ("player_slot.label", slot.label.as_deref(), 128),
            ("player_slot.position", slot.position.as_deref(), 64),
            ("player_slot.character", slot.character.as_deref(), 128),
        ] {
            if let Some(value) = value
                && (value.trim().is_empty()
                    || value.len() > max_len
                    || value.chars().any(char::is_control))
            {
                return Err(ApiError::bad_request(
                    "invalid_player_slot",
                    &format!("{field} must be nonempty text up to {max_len} bytes"),
                ));
            }
        }
        if let Some(path) = slot.artwork_path.as_deref() {
            validate_asset_path("player_slot.artwork_path", path)?;
        }
    }
    Ok(())
}

fn validate_asset_path(field: &str, value: &str) -> Result<(), ApiError> {
    let invalid = value.is_empty()
        || value.len() > 512
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains("..")
        || value.contains(':')
        || value.chars().any(char::is_control);
    if invalid {
        return Err(ApiError::bad_request(
            "invalid_asset_path",
            &format!("{field} must be a relative asset path without traversal"),
        ));
    }
    Ok(())
}

fn safe_relative_asset_path(value: &str) -> Result<PathBuf, ApiError> {
    validate_asset_path("asset_path", value)?;
    let path = Path::new(value);
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => relative.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ApiError::bad_request(
                    "invalid_asset_path",
                    "asset path must be relative and must not contain traversal",
                ));
            }
        }
    }
    if relative.as_os_str().is_empty() {
        return Err(ApiError::bad_request(
            "invalid_asset_path",
            "asset path must not be empty",
        ));
    }
    Ok(relative)
}

fn content_type_for_asset(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("gif") => "image/gif",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("json") => "application/json",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

struct ApiError {
    status: StatusCode,
    body: ErrorResponse,
}

impl ApiError {
    fn bad_request(code: &str, message: &str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    fn store(error: StoreError) -> Self {
        match error {
            StoreError::NotFound => Self::new(
                StatusCode::NOT_FOUND,
                "runtime_host_not_found",
                "runtime host was not found",
            ),
            StoreError::GameNotFound => Self::new(
                StatusCode::NOT_FOUND,
                "catalog_game_not_found",
                "catalog game was not found",
            ),
            StoreError::GameUnavailable => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "catalog_game_unavailable",
                "catalog game has no online runtime host",
            ),
            StoreError::SessionNotFound => Self::new(
                StatusCode::NOT_FOUND,
                "session_not_found",
                "session was not found",
            ),
            StoreError::SeatBusy => Self::new(
                StatusCode::CONFLICT,
                "seat_session_conflict",
                "seat already has a nonterminal session",
            ),
            StoreError::PortsExhausted => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "session_ports_exhausted",
                "runtime host has no available session ports",
            ),
            StoreError::PlayerSlotNotFound => Self::new(
                StatusCode::NOT_FOUND,
                "player_slot_not_found",
                "player slot was not found",
            ),
            StoreError::PlayerSlotUnavailable => Self::new(
                StatusCode::CONFLICT,
                "player_slot_unavailable",
                "player slot is not currently open",
            ),
            StoreError::SpectatorGrantNotFound => Self::new(
                StatusCode::NOT_FOUND,
                "spectator_grant_not_found",
                "spectator grant was not found",
            ),
            StoreError::MissingDataPlaneAddress => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "runtime_host_address_unavailable",
                "runtime host did not advertise a usable data-plane address",
            ),
            StoreError::WrongRuntimeHost => Self::new(
                StatusCode::CONFLICT,
                "session_runtime_host_conflict",
                "session belongs to another runtime host",
            ),
            StoreError::InvalidSessionTransition => Self::new(
                StatusCode::CONFLICT,
                "invalid_session_transition",
                "session state transition is invalid",
            ),
            StoreError::MissingFailureReason => Self::new(
                StatusCode::BAD_REQUEST,
                "missing_failure_reason",
                "failure state requires a nonempty reason",
            ),
            StoreError::ConflictingStateRetry => Self::new(
                StatusCode::CONFLICT,
                "conflicting_state_retry",
                "state retry conflicts with the stored failure reason",
            ),
            StoreError::StaleHeartbeat => Self::new(
                StatusCode::CONFLICT,
                "stale_heartbeat",
                "heartbeat sequence is older than the last accepted sequence",
            ),
            StoreError::HeartbeatSequenceConflict => Self::new(
                StatusCode::CONFLICT,
                "heartbeat_sequence_conflict",
                "a heartbeat with this sequence was already accepted with different data",
            ),
            other => {
                eprintln!("Control-plane storage failure: {other}");
                Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "storage_error",
                    "control-plane storage operation failed",
                )
            }
        }
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
    use std::{path::PathBuf, time::Duration};

    use super::{
        PortPoolConfig, app, app_with_database, app_with_database_tokens_and_ports,
        app_with_database_tokens_ports_and_assets,
    };
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
                "data_plane_address": "127.0.0.1",
                "operating_system": "linux",
                "architecture": "x86_64",
                "logical_cpu_count": 4,
                "memory_bytes": 17_179_869_184_u64,
                "encoder_names": ["libx264", "h264_qsv"],
                "emulator_adapters": ["mame"]
            }
        })
    }

    fn catalog() -> Value {
        json!({
            "games": [{
                "id": "tmnt",
                "display_name": "Teenage Mutant Ninja Turtles",
                "rom_name": "tmnt",
                "profile": {
                    "width": 320,
                    "height": 224,
                    "refresh_hz": 60.0,
                    "rotation_degrees": 0,
                    "max_players": 4,
                    "buttons_per_player": 2,
                    "supports_save_state": true
                }
            }]
        })
    }

    async fn request_json(
        app: axum::Router,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = Request::builder().method(method).uri(path);
        if path.starts_with("/api/v1/") {
            request = request.header("authorization", "Bearer test-seat-token");
        }
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

    async fn request_bytes(
        app: axum::Router,
        method: Method,
        path: &str,
    ) -> (u16, Option<String>, Vec<u8>) {
        let mut request = Request::builder().method(method).uri(path);
        if path.starts_with("/api/v1/") {
            request = request.header("authorization", "Bearer test-seat-token");
        }
        let response = app
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, content_type, body.to_vec())
    }

    async fn get_json(path: &str) -> (u16, Value) {
        request_json(app().await.unwrap(), Method::GET, path, None).await
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
    async fn protected_api_requires_a_bearer_token() {
        let response = app()
            .await
            .unwrap()
            .oneshot(Request::get("/api/v1/games").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let error: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(error["code"], "authentication_required");
    }

    #[tokio::test]
    async fn runtime_host_registration_is_idempotent_and_listed() {
        let service = app().await.unwrap();
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
        let service = app().await.unwrap();
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
        let service = app().await.unwrap();
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
            app().await.unwrap(),
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
            app().await.unwrap(),
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
            app().await.unwrap(),
            Method::PUT,
            "/api/v1/runtime-hosts/reference-linux",
            Some(invalid_registration),
        )
        .await;
        assert_eq!(invalid_capabilities_status, 400);
        assert_eq!(invalid_capabilities["code"], "invalid_logical_cpu_count");
    }

    #[tokio::test]
    async fn registrations_survive_database_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("control-plane.sqlite3");
        let service = app_with_database(&database, Duration::from_secs(15))
            .await
            .unwrap();
        request_json(
            service,
            Method::PUT,
            "/api/v1/runtime-hosts/reference-linux",
            Some(registration()),
        )
        .await;

        let reopened = app_with_database(&database, Duration::from_secs(15))
            .await
            .unwrap();
        let (status, host) = request_json(
            reopened,
            Method::GET,
            "/api/v1/runtime-hosts/reference-linux",
            None,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(host["id"], "reference-linux");
    }

    #[tokio::test]
    async fn expired_host_is_offline_until_the_next_heartbeat() {
        let directory = tempfile::tempdir().unwrap();
        let service = app_with_database(
            directory.path().join("control-plane.sqlite3"),
            Duration::from_millis(10),
        )
        .await
        .unwrap();
        request_json(
            service.clone(),
            Method::PUT,
            "/api/v1/runtime-hosts/reference-linux",
            Some(registration()),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(20)).await;

        let (_, offline) = request_json(
            service.clone(),
            Method::GET,
            "/api/v1/runtime-hosts/reference-linux",
            None,
        )
        .await;
        assert_eq!(offline["status"], "offline");

        let (_, online) = request_json(
            service,
            Method::POST,
            "/api/v1/runtime-hosts/reference-linux/heartbeat",
            Some(json!({ "sequence": 1, "active_session_count": 0 })),
        )
        .await;
        assert_eq!(online["status"], "online");
    }

    #[tokio::test]
    async fn host_catalog_is_persisted_and_exposes_liveness() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("control-plane.sqlite3");
        let service = app_with_database(&database, Duration::from_millis(200))
            .await
            .unwrap();
        request_json(
            service.clone(),
            Method::PUT,
            "/api/v1/runtime-hosts/reference-linux",
            Some(registration()),
        )
        .await;
        let (publish_status, _) = request_json(
            service.clone(),
            Method::PUT,
            "/api/v1/runtime-hosts/reference-linux/catalog",
            Some(catalog()),
        )
        .await;
        assert_eq!(publish_status, 200);

        let reopened = app_with_database(&database, Duration::from_millis(200))
            .await
            .unwrap();
        let (game_status, game) =
            request_json(reopened.clone(), Method::GET, "/api/v1/games/tmnt", None).await;
        assert_eq!(game_status, 200);
        assert_eq!(game["availability"][0]["runtime_host_status"], "online");
        assert_eq!(game["availability"][0]["profile"]["width"], 320);

        tokio::time::sleep(Duration::from_millis(250)).await;
        let (_, expired_game) =
            request_json(reopened, Method::GET, "/api/v1/games/tmnt", None).await;
        assert_eq!(
            expired_game["availability"][0]["runtime_host_status"],
            "offline"
        );
    }

    #[tokio::test]
    async fn game_metadata_can_be_updated_and_overrides_player_count() {
        let service = app().await.unwrap();
        register_host_and_catalog(&service).await;

        let metadata_path = "/api/v1/games/tmnt/metadata";
        let (metadata_status, metadata) =
            request_json(service.clone(), Method::GET, metadata_path, None).await;
        assert_eq!(metadata_status, 200);
        assert!(metadata["player_count"].is_null());

        let (update_status, updated_game) = request_json(
            service.clone(),
            Method::PUT,
            metadata_path,
            Some(json!({
                "metadata": {
                    "sort_title": "Teenage Mutant Ninja Turtles",
                    "description": "Four-player arcade brawler.",
                    "genre": "Beat 'em up",
                    "release_year": 1989,
                    "manufacturer": "Konami",
                    "player_count": 3,
                    "artwork_path": "media/tmnt/artwork.png",
                    "marquee_path": "media/tmnt/marquee.png",
                    "screenshot_path": "media/tmnt/screen.png",
                    "logo_path": "media/tmnt/logo.png",
                    "control_notes": "Jump and attack."
                }
            })),
        )
        .await;
        assert_eq!(update_status, 200);
        assert_eq!(updated_game["metadata"]["player_count"], 3);
        assert_eq!(updated_game["availability"][0]["profile"]["max_players"], 3);

        let (_, created) = request_json(
            service,
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;
        assert_eq!(created["player_slots"].as_array().unwrap().len(), 3);
        assert_eq!(created["runtime_profile"]["max_players"], 3);
    }

    #[tokio::test]
    async fn active_session_summaries_include_artwork_preview_fallbacks() {
        let service = app().await.unwrap();
        register_host_and_catalog(&service).await;
        let (_, _) = request_json(
            service.clone(),
            Method::PUT,
            "/api/v1/games/tmnt/metadata",
            Some(json!({
                "metadata": {
                    "screenshot_path": "media/tmnt/screenshot.svg"
                }
            })),
        )
        .await;
        let (_, _) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;

        let (status, summaries) =
            request_json(service, Method::GET, "/api/v1/active-sessions", None).await;

        assert_eq!(status, 200);
        assert_eq!(
            summaries["sessions"][0]["preview_asset_path"],
            "media/tmnt/screenshot.svg"
        );
        assert_eq!(
            summaries["sessions"][0]["preview_status"],
            "artwork_available"
        );
    }

    #[tokio::test]
    async fn active_session_summaries_publish_runtime_still_previews() {
        let service = app().await.unwrap();
        register_host_and_catalog(&service).await;
        let (_, created) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;
        let session_id = created["id"].as_str().unwrap();
        let state_path =
            format!("/api/v1/runtime-hosts/reference-linux/sessions/{session_id}/state");
        for state in ["starting", "ready"] {
            request_json(
                service.clone(),
                Method::PUT,
                &state_path,
                Some(json!({ "state": state, "failure_reason": null })),
            )
            .await;
        }
        let (active_status, _) = request_json(
            service.clone(),
            Method::PUT,
            &state_path,
            Some(json!({
                "state": "active",
                "failure_reason": null,
                "preview_asset_path": format!("previews/{session_id}.bmp")
            })),
        )
        .await;
        assert_eq!(active_status, 200);

        let (status, summaries) =
            request_json(service, Method::GET, "/api/v1/active-sessions", None).await;

        assert_eq!(status, 200);
        assert_eq!(
            summaries["sessions"][0]["preview_status"],
            "still_available"
        );
        assert_eq!(
            summaries["sessions"][0]["preview_asset_path"],
            format!("previews/{session_id}.bmp")
        );
    }

    #[tokio::test]
    async fn runtime_state_update_rejects_unsafe_preview_paths() {
        let service = app().await.unwrap();
        register_host_and_catalog(&service).await;
        let (_, created) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;
        let session_id = created["id"].as_str().unwrap();
        let state_path =
            format!("/api/v1/runtime-hosts/reference-linux/sessions/{session_id}/state");

        let (status, error) = request_json(
            service,
            Method::PUT,
            &state_path,
            Some(json!({
                "state": "starting",
                "failure_reason": null,
                "preview_asset_path": "../outside.bmp"
            })),
        )
        .await;

        assert_eq!(status, 400);
        assert_eq!(error["code"], "invalid_asset_path");
    }

    #[tokio::test]
    async fn game_metadata_can_label_player_slots_for_new_sessions() {
        let service = app().await.unwrap();
        register_host_and_catalog(&service).await;
        let (update_status, _) = request_json(
            service.clone(),
            Method::PUT,
            "/api/v1/games/tmnt/metadata",
            Some(json!({
                "metadata": {
                    "player_count": 4,
                    "player_slots": [
                        {
                            "player_number": 2,
                            "label": "Donatello",
                            "position": "P2",
                            "character": "Donatello",
                            "artwork_path": "media/tmnt/p2.svg"
                        }
                    ]
                }
            })),
        )
        .await;
        assert_eq!(update_status, 200);

        let (_, created) = request_json(
            service,
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;

        assert_eq!(
            created["player_slots"][1]["presentation"]["label"],
            "Donatello"
        );
        assert_eq!(created["player_slots"][1]["presentation"]["position"], "P2");
        assert_eq!(
            created["player_slots"][1]["presentation"]["character"],
            "Donatello"
        );
        assert_eq!(
            created["player_slots"][1]["presentation"]["artwork_path"],
            "media/tmnt/p2.svg"
        );
    }

    #[tokio::test]
    async fn game_metadata_rejects_invalid_player_slot_metadata() {
        let service = app().await.unwrap();
        register_host_and_catalog(&service).await;

        let (status, error) = request_json(
            service,
            Method::PUT,
            "/api/v1/games/tmnt/metadata",
            Some(json!({
                "metadata": {
                    "player_slots": [
                        { "player_number": 2, "label": "P2" },
                        { "player_number": 2, "label": "Duplicate" }
                    ]
                }
            })),
        )
        .await;

        assert_eq!(status, 400);
        assert_eq!(error["code"], "duplicate_player_slot");
    }

    #[tokio::test]
    async fn game_metadata_rejects_unsafe_asset_paths() {
        let service = app().await.unwrap();
        register_host_and_catalog(&service).await;

        let (status, error) = request_json(
            service,
            Method::PUT,
            "/api/v1/games/tmnt/metadata",
            Some(json!({
                "metadata": {
                    "player_count": 4,
                    "marquee_path": "../secret.png"
                }
            })),
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(error["code"], "invalid_asset_path");
    }

    #[tokio::test]
    async fn asset_endpoint_serves_files_from_configured_root() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("control-plane.sqlite3");
        let asset_root = directory.path().join("assets");
        std::fs::create_dir_all(asset_root.join("media/tmnt")).unwrap();
        std::fs::write(asset_root.join("media/tmnt/marquee.png"), b"fake-png").unwrap();
        let service = app_with_database_tokens_ports_and_assets(
            &database,
            Duration::from_secs(15),
            "test-seat-token".to_owned(),
            "test-runtime-host-token".to_owned(),
            PortPoolConfig::default(),
            Some(asset_root),
        )
        .await
        .unwrap();

        let (status, content_type, body) = request_bytes(
            service,
            Method::GET,
            "/api/v1/assets/media/tmnt/marquee.png",
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(content_type.as_deref(), Some("image/png"));
        assert_eq!(body, b"fake-png");
    }

    #[tokio::test]
    async fn asset_endpoint_rejects_traversal_and_reports_missing_root() {
        let no_root = app().await.unwrap();
        let (missing_root_status, _, _) = request_bytes(
            no_root,
            Method::GET,
            "/api/v1/assets/media/tmnt/marquee.png",
        )
        .await;
        assert_eq!(missing_root_status, 404);

        let directory = tempfile::tempdir().unwrap();
        let service = app_with_database_tokens_ports_and_assets(
            directory.path().join("control-plane.sqlite3"),
            Duration::from_secs(15),
            "test-seat-token".to_owned(),
            "test-runtime-host-token".to_owned(),
            PortPoolConfig::default(),
            Some(PathBuf::from(directory.path())),
        )
        .await
        .unwrap();

        let (status, _, _) = request_bytes(
            service,
            Method::GET,
            "/api/v1/assets/media/%2e%2e/secret.png",
        )
        .await;

        assert_eq!(status, 400);
    }

    #[tokio::test]
    async fn catalog_publish_requires_registered_host() {
        let (status, error) = request_json(
            app().await.unwrap(),
            Method::PUT,
            "/api/v1/runtime-hosts/missing/catalog",
            Some(catalog()),
        )
        .await;
        assert_eq!(status, 404);
        assert_eq!(error["code"], "runtime_host_not_found");
    }

    async fn register_host_and_catalog(service: &axum::Router) {
        let (host_status, _) = request_json(
            service.clone(),
            Method::PUT,
            "/api/v1/runtime-hosts/reference-linux",
            Some(registration()),
        )
        .await;
        assert!(matches!(host_status, 200 | 201));
        let (catalog_status, _) = request_json(
            service.clone(),
            Method::PUT,
            "/api/v1/runtime-hosts/reference-linux/catalog",
            Some(catalog()),
        )
        .await;
        assert_eq!(catalog_status, 200);
    }

    fn session_request(seat_id: &str) -> Value {
        json!({
            "game_id": "tmnt",
            "seat_id": seat_id,
            "destination_address": "192.0.2.25"
        })
    }

    #[tokio::test]
    async fn session_allocation_returns_persisted_grant_and_isolates_ports() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("control-plane.sqlite3");
        let service = app_with_database(&database, Duration::from_secs(15))
            .await
            .unwrap();
        register_host_and_catalog(&service).await;

        let (first_status, first) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;
        assert_eq!(first_status, 201);
        assert_eq!(first["state"], "allocating");
        assert_eq!(first["runtime_host_id"], "reference-linux");
        assert_eq!(
            first["connection_grant"]["runtime_host_address"],
            "127.0.0.1"
        );
        assert_eq!(first["connection_grant"]["media_udp_port"], 41_000);
        assert_eq!(first["connection_grant"]["input_udp_port"], 42_000);
        assert!(first["connection_grant"]["token"].as_str().unwrap().len() >= 32);
        assert_eq!(first["player_slots"].as_array().unwrap().len(), 4);
        assert_eq!(first["player_slots"][0]["player_number"], 1);
        assert_eq!(first["player_slots"][0]["state"], "occupied");
        assert_eq!(first["player_slots"][0]["seat_id"], "seat-one");
        assert_eq!(
            first["player_slots"][0]["lease_expires_unix_ms"],
            first["connection_grant"]["expires_unix_ms"]
        );
        assert_eq!(first["player_slots"][1]["player_number"], 2);
        assert_eq!(first["player_slots"][1]["state"], "open");
        assert!(first["player_slots"][1]["seat_id"].is_null());

        let (busy_status, busy) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;
        assert_eq!(busy_status, 409);
        assert_eq!(busy["code"], "seat_session_conflict");

        let (second_status, second) = request_json(
            service,
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-two")),
        )
        .await;
        assert_eq!(second_status, 201);
        assert_eq!(second["connection_grant"]["media_udp_port"], 41_001);
        assert_eq!(second["connection_grant"]["input_udp_port"], 42_001);

        let reopened = app_with_database(&database, Duration::from_secs(15))
            .await
            .unwrap();
        let path = format!("/api/v1/sessions/{}", first["id"].as_str().unwrap());
        let (get_status, persisted) =
            request_json(reopened.clone(), Method::GET, &path, None).await;
        assert_eq!(get_status, 200);
        assert_eq!(persisted["connection_grant"], first["connection_grant"]);
        assert_eq!(persisted["player_slots"], first["player_slots"]);

        let (list_status, list) =
            request_json(reopened, Method::GET, "/api/v1/sessions", None).await;
        assert_eq!(list_status, 200);
        assert_eq!(list["sessions"].as_array().unwrap().len(), 2);
        assert_eq!(list["sessions"][0]["player_slots"], first["player_slots"]);
    }

    #[tokio::test]
    async fn session_allocation_rejects_invalid_unknown_and_unavailable_requests() {
        let service = app().await.unwrap();
        let (invalid_status, invalid) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(json!({
                "game_id": "tmnt",
                "seat_id": "seat-one",
                "destination_address": "not an ip"
            })),
        )
        .await;
        assert_eq!(invalid_status, 400);
        assert_eq!(invalid["code"], "invalid_destination_address");

        let (unknown_status, unknown) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;
        assert_eq!(unknown_status, 404);
        assert_eq!(unknown["code"], "catalog_game_not_found");

        register_host_and_catalog(&service).await;
        request_json(
            service.clone(),
            Method::POST,
            "/api/v1/runtime-hosts/reference-linux/heartbeat",
            Some(json!({ "sequence": 1, "active_session_count": 0 })),
        )
        .await;
        let (_, created) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;
        assert_eq!(created["state"], "allocating");

        let (missing_status, missing) =
            request_json(service, Method::GET, "/api/v1/sessions/missing", None).await;
        assert_eq!(missing_status, 404);
        assert_eq!(missing["code"], "session_not_found");

        let expiry_directory = tempfile::tempdir().unwrap();
        let expiring = app_with_database(
            expiry_directory.path().join("control-plane.sqlite3"),
            Duration::from_millis(100),
        )
        .await
        .unwrap();
        register_host_and_catalog(&expiring).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        let (unavailable_status, unavailable) = request_json(
            expiring,
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-two")),
        )
        .await;
        assert_eq!(unavailable_status, 503);
        assert_eq!(unavailable["code"], "catalog_game_unavailable");
    }

    #[tokio::test]
    async fn spectator_grants_allocate_distinct_media_ports_without_claiming_slots() {
        let service = app().await.unwrap();
        register_host_and_catalog(&service).await;
        let (_, created) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;
        let session_id = created["id"].as_str().unwrap();
        let state_path =
            format!("/api/v1/runtime-hosts/reference-linux/sessions/{session_id}/state");
        for state in ["starting", "ready", "active"] {
            request_json(
                service.clone(),
                Method::PUT,
                &state_path,
                Some(json!({ "state": state, "failure_reason": null })),
            )
            .await;
        }

        let (summary_status, summaries) = request_json(
            service.clone(),
            Method::GET,
            "/api/v1/active-sessions",
            None,
        )
        .await;
        assert_eq!(summary_status, 200);
        assert_eq!(summaries["sessions"][0]["id"], session_id);
        assert_eq!(
            summaries["sessions"][0]["preview_status"],
            "spectator_available"
        );
        assert!(summaries["sessions"][0]["connection_grant"].is_null());

        let spectator_path = format!("/api/v1/sessions/{session_id}/spectators");
        let (first_status, first) = request_json(
            service.clone(),
            Method::POST,
            &spectator_path,
            Some(json!({
                "seat_id": "seat-two",
                "destination_address": "192.0.2.26"
            })),
        )
        .await;
        assert_eq!(first_status, 201);
        assert_eq!(first["session_id"], session_id);
        assert_eq!(first["seat_id"], "seat-two");
        assert_eq!(first["runtime_host_id"], "reference-linux");
        assert_eq!(first["runtime_host_address"], "127.0.0.1");
        assert_eq!(first["media_udp_port"], 41_001);

        let (second_status, second) = request_json(
            service.clone(),
            Method::POST,
            &spectator_path,
            Some(json!({
                "seat_id": "seat-three",
                "destination_address": "192.0.2.27"
            })),
        )
        .await;
        assert_eq!(second_status, 201);
        assert_eq!(second["media_udp_port"], 41_002);

        let (next_session_status, next_session) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-four")),
        )
        .await;
        assert_eq!(next_session_status, 201);
        assert_eq!(next_session["connection_grant"]["media_udp_port"], 41_003);
        assert_eq!(next_session["connection_grant"]["input_udp_port"], 42_001);

        let (assignments_status, assignments) = request_json(
            service.clone(),
            Method::GET,
            "/api/v1/runtime-hosts/reference-linux/sessions",
            None,
        )
        .await;
        assert_eq!(assignments_status, 200);
        assert_eq!(
            assignments["sessions"][0]["spectator_media_ports"],
            json!([41_001, 41_002])
        );

        let session_path = format!("/api/v1/sessions/{session_id}");
        let (_, session) = request_json(service.clone(), Method::GET, &session_path, None).await;
        assert_eq!(session["player_slots"][0]["state"], "occupied");
        assert_eq!(session["player_slots"][1]["state"], "open");
        assert_eq!(session["active_spectator_count"], 2);

        let release_path = format!(
            "/api/v1/sessions/{session_id}/spectators/{}",
            first["id"].as_str().unwrap()
        );
        let (wrong_seat_status, wrong_seat) = request_json(
            service.clone(),
            Method::DELETE,
            &release_path,
            Some(json!({ "seat_id": "seat-three" })),
        )
        .await;
        assert_eq!(wrong_seat_status, 404);
        assert_eq!(wrong_seat["code"], "spectator_grant_not_found");

        let (release_status, released) = request_json(
            service.clone(),
            Method::DELETE,
            &release_path,
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(release_status, 200);
        assert_eq!(released["active_spectator_count"], 1);

        let (_, assignments_after_release) = request_json(
            service,
            Method::GET,
            "/api/v1/runtime-hosts/reference-linux/sessions",
            None,
        )
        .await;
        assert_eq!(
            assignments_after_release["sessions"][0]["spectator_media_ports"],
            json!([41_002])
        );
    }

    #[tokio::test]
    async fn session_port_pools_are_configurable_and_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let service = app_with_database_tokens_and_ports(
            directory.path().join("control-plane.sqlite3"),
            Duration::from_secs(15),
            "test-seat-token".to_owned(),
            "test-runtime-host-token".to_owned(),
            PortPoolConfig {
                media_port_start: 45_000,
                media_port_count: 2,
                input_port_start: 46_000,
                input_port_count: 2,
            },
        )
        .await
        .unwrap();
        register_host_and_catalog(&service).await;

        let (first_status, first) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;
        assert_eq!(first_status, 201);
        assert_eq!(first["connection_grant"]["media_udp_port"], 45_000);
        assert_eq!(first["connection_grant"]["input_udp_port"], 46_000);

        let (second_status, second) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-two")),
        )
        .await;
        assert_eq!(second_status, 201);
        assert_eq!(second["connection_grant"]["media_udp_port"], 45_001);
        assert_eq!(second["connection_grant"]["input_udp_port"], 46_001);

        let (exhausted_status, exhausted) = request_json(
            service,
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-three")),
        )
        .await;
        assert_eq!(exhausted_status, 503);
        assert_eq!(exhausted["code"], "session_ports_exhausted");
    }

    #[tokio::test]
    async fn runtime_host_drives_versioned_session_lifecycle_and_stop_is_idempotent() {
        let service = app().await.unwrap();
        register_host_and_catalog(&service).await;
        let (_, created) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;
        let session_id = created["id"].as_str().unwrap();

        let (assignments_status, assignments) = request_json(
            service.clone(),
            Method::GET,
            "/api/v1/runtime-hosts/reference-linux/sessions",
            None,
        )
        .await;
        assert_eq!(assignments_status, 200);
        assert_eq!(assignments["sessions"][0]["session_id"], session_id);
        assert_eq!(assignments["sessions"][0]["rom_name"], "tmnt");
        assert_eq!(assignments["sessions"][0]["state"], "allocating");
        assert_eq!(
            assignments["sessions"][0]["input_token"],
            created["connection_grant"]["token"]
        );

        let state_path =
            format!("/api/v1/runtime-hosts/reference-linux/sessions/{session_id}/state");
        for expected in ["starting", "ready", "active"] {
            let (status, session) = request_json(
                service.clone(),
                Method::PUT,
                &state_path,
                Some(json!({ "state": expected, "failure_reason": null })),
            )
            .await;
            assert_eq!(status, 200);
            assert_eq!(session["state"], expected);
        }
        let (retry_status, retried) = request_json(
            service.clone(),
            Method::PUT,
            &state_path,
            Some(json!({ "state": "active", "failure_reason": null })),
        )
        .await;
        assert_eq!(retry_status, 200);
        assert_eq!(retried["state"], "active");

        let (invalid_status, invalid) = request_json(
            service.clone(),
            Method::PUT,
            &state_path,
            Some(json!({ "state": "ready", "failure_reason": null })),
        )
        .await;
        assert_eq!(invalid_status, 409);
        assert_eq!(invalid["code"], "invalid_session_transition");

        let stop_path = format!("/api/v1/sessions/{session_id}/stop");
        let (stop_status, stopping) =
            request_json(service.clone(), Method::POST, &stop_path, None).await;
        assert_eq!(stop_status, 200);
        assert_eq!(stopping["state"], "stopping");
        let (_, stopping_retry) =
            request_json(service.clone(), Method::POST, &stop_path, None).await;
        assert_eq!(stopping_retry["state"], "stopping");

        let (_, stopped) = request_json(
            service.clone(),
            Method::PUT,
            &state_path,
            Some(json!({ "state": "stopped", "failure_reason": null })),
        )
        .await;
        assert_eq!(stopped["state"], "stopped");
        let (_, assignments_after_stop) = request_json(
            service,
            Method::GET,
            "/api/v1/runtime-hosts/reference-linux/sessions",
            None,
        )
        .await;
        assert!(
            assignments_after_stop["sessions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn player_slot_reservation_is_atomic_and_lease_based() {
        let service = app().await.unwrap();
        register_host_and_catalog(&service).await;
        let (_, created) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;
        let session_id = created["id"].as_str().unwrap();
        let state_path =
            format!("/api/v1/runtime-hosts/reference-linux/sessions/{session_id}/state");
        for state in ["starting", "ready", "active"] {
            request_json(
                service.clone(),
                Method::PUT,
                &state_path,
                Some(json!({ "state": state, "failure_reason": null })),
            )
            .await;
        }

        let reserve_p2 = format!("/api/v1/sessions/{session_id}/player-slots/2/reserve");
        let (reserve_status, reserved) = request_json(
            service.clone(),
            Method::POST,
            &reserve_p2,
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(reserve_status, 200);
        assert_eq!(reserved["player_slots"][1]["player_number"], 2);
        assert_eq!(reserved["player_slots"][1]["state"], "reserved");
        assert_eq!(reserved["player_slots"][1]["seat_id"], "seat-two");
        assert!(reserved["player_slots"][1]["lease_expires_unix_ms"].is_number());
        assert!(
            reserved["connection_grant"]["expires_unix_ms"]
                .as_u64()
                .unwrap()
                >= reserved["player_slots"][1]["lease_expires_unix_ms"]
                    .as_u64()
                    .unwrap()
        );

        let (retry_status, retry) = request_json(
            service.clone(),
            Method::POST,
            &reserve_p2,
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(retry_status, 200);
        assert_eq!(retry["player_slots"][1]["seat_id"], "seat-two");

        let connect_p2 = format!("/api/v1/sessions/{session_id}/player-slots/2/connect");
        let (wrong_connect_status, wrong_connect) = request_json(
            service.clone(),
            Method::POST,
            &connect_p2,
            Some(json!({ "seat_id": "seat-three" })),
        )
        .await;
        assert_eq!(wrong_connect_status, 409);
        assert_eq!(wrong_connect["code"], "player_slot_unavailable");

        let (connect_status, connected) = request_json(
            service.clone(),
            Method::POST,
            &connect_p2,
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(connect_status, 200);
        assert_eq!(connected["player_slots"][1]["state"], "occupied");
        assert_eq!(connected["player_slots"][1]["seat_id"], "seat-two");
        assert!(connected["player_slots"][1]["lease_expires_unix_ms"].is_null());

        let (connect_retry_status, connect_retry) = request_json(
            service.clone(),
            Method::POST,
            &connect_p2,
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(connect_retry_status, 200);
        assert_eq!(connect_retry["player_slots"][1]["state"], "occupied");

        let disconnect_p2 = format!("/api/v1/sessions/{session_id}/player-slots/2/disconnect");
        let (wrong_disconnect_status, wrong_disconnect) = request_json(
            service.clone(),
            Method::POST,
            &disconnect_p2,
            Some(json!({ "seat_id": "seat-three" })),
        )
        .await;
        assert_eq!(wrong_disconnect_status, 409);
        assert_eq!(wrong_disconnect["code"], "player_slot_unavailable");

        let (disconnect_status, disconnected) = request_json(
            service.clone(),
            Method::POST,
            &disconnect_p2,
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(disconnect_status, 200);
        assert_eq!(disconnected["player_slots"][1]["state"], "disconnected");
        assert_eq!(disconnected["player_slots"][1]["seat_id"], "seat-two");
        assert!(disconnected["player_slots"][1]["lease_expires_unix_ms"].is_number());

        let (reconnect_status, reconnected) = request_json(
            service.clone(),
            Method::POST,
            &connect_p2,
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(reconnect_status, 200);
        assert_eq!(reconnected["player_slots"][1]["state"], "occupied");
        assert!(reconnected["player_slots"][1]["lease_expires_unix_ms"].is_null());

        let release_p2 = format!("/api/v1/sessions/{session_id}/player-slots/2/release");
        let (wrong_release_status, wrong_release) = request_json(
            service.clone(),
            Method::POST,
            &release_p2,
            Some(json!({ "seat_id": "seat-three" })),
        )
        .await;
        assert_eq!(wrong_release_status, 409);
        assert_eq!(wrong_release["code"], "player_slot_unavailable");

        let (release_status, released) = request_json(
            service.clone(),
            Method::POST,
            &release_p2,
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(release_status, 200);
        assert_eq!(released["player_slots"][1]["state"], "open");
        assert!(released["player_slots"][1]["seat_id"].is_null());

        let (release_retry_status, release_retry) = request_json(
            service.clone(),
            Method::POST,
            &release_p2,
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(release_retry_status, 200);
        assert_eq!(release_retry["player_slots"][1]["state"], "open");

        let (reserve_again_status, reserved_again) = request_json(
            service.clone(),
            Method::POST,
            &reserve_p2,
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(reserve_again_status, 200);
        assert_eq!(reserved_again["player_slots"][1]["seat_id"], "seat-two");

        let (conflict_status, conflict) = request_json(
            service.clone(),
            Method::POST,
            &reserve_p2,
            Some(json!({ "seat_id": "seat-three" })),
        )
        .await;
        assert_eq!(conflict_status, 409);
        assert_eq!(conflict["code"], "player_slot_unavailable");

        let reserve_p3 = format!("/api/v1/sessions/{session_id}/player-slots/3/reserve");
        let (busy_status, busy) = request_json(
            service,
            Method::POST,
            &reserve_p3,
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(busy_status, 409);
        assert_eq!(busy["code"], "seat_session_conflict");
    }

    #[tokio::test]
    async fn player_slot_reservation_rejects_missing_unstarted_and_unknown_slots() {
        let service = app().await.unwrap();
        register_host_and_catalog(&service).await;
        let (_, created) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;
        let session_id = created["id"].as_str().unwrap();

        let allocating_path = format!("/api/v1/sessions/{session_id}/player-slots/2/reserve");
        let (allocating_status, allocating) = request_json(
            service.clone(),
            Method::POST,
            &allocating_path,
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(allocating_status, 409);
        assert_eq!(allocating["code"], "player_slot_unavailable");

        let state_path =
            format!("/api/v1/runtime-hosts/reference-linux/sessions/{session_id}/state");
        for state in ["starting", "ready"] {
            request_json(
                service.clone(),
                Method::PUT,
                &state_path,
                Some(json!({ "state": state, "failure_reason": null })),
            )
            .await;
        }
        let missing_slot_path = format!("/api/v1/sessions/{session_id}/player-slots/9/reserve");
        let (slot_status, slot_error) = request_json(
            service.clone(),
            Method::POST,
            &missing_slot_path,
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(slot_status, 404);
        assert_eq!(slot_error["code"], "player_slot_not_found");

        let (session_status, session_error) = request_json(
            service,
            Method::POST,
            "/api/v1/sessions/missing/player-slots/1/reserve",
            Some(json!({ "seat_id": "seat-two" })),
        )
        .await;
        assert_eq!(session_status, 404);
        assert_eq!(session_error["code"], "session_not_found");
    }

    #[tokio::test]
    async fn host_expiry_marks_assigned_sessions_runtime_lost() {
        let directory = tempfile::tempdir().unwrap();
        let service = app_with_database(
            directory.path().join("control-plane.sqlite3"),
            Duration::from_millis(100),
        )
        .await
        .unwrap();
        register_host_and_catalog(&service).await;
        let (_, created) = request_json(
            service.clone(),
            Method::POST,
            "/api/v1/sessions",
            Some(session_request("seat-one")),
        )
        .await;
        let session_id = created["id"].as_str().unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;

        request_json(
            service.clone(),
            Method::GET,
            "/api/v1/runtime-hosts/reference-linux",
            None,
        )
        .await;
        let session_path = format!("/api/v1/sessions/{session_id}");
        let (_, lost) = request_json(service, Method::GET, &session_path, None).await;
        assert_eq!(lost["state"], "runtime_lost");
        assert_eq!(lost["failure_reason"], "runtime host heartbeat expired");
    }

    #[tokio::test]
    async fn unknown_routes_return_not_found() {
        let response = app()
            .await
            .unwrap()
            .oneshot(Request::get("/api/v2").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
    }
}
