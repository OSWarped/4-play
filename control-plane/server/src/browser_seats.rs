//! Opt-in, direct-LAN browser seats. The shared bearer never leaves the server.
use crate::wan_testers::{GateError, WanGate, WanTesterConfig};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{ConnectInfo, State},
    http::{HeaderMap, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hmac::{Hmac, Mac};
use ipnet::IpNet;
use serde_json::{Value, json};
use sha2::Sha256;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tower::ServiceExt;
use uuid::Uuid;

const COOKIE: &str = "fourplay_browser_seat";
const TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const MAX_BODY: usize = 300 * 1024;
type Signature = Hmac<Sha256>;

#[derive(Clone)]
struct Seats {
    app: Router,
    secret: Arc<String>,
    networks: Vec<IpNet>,
    origins: Vec<String>,
    wan: Option<WanGate>,
}

/// Empty settings disable automatic enrollment. No forwarded headers are trusted.
pub fn with_browser_seats(
    app: Router,
    secret: String,
    networks: &str,
    origins: &str,
) -> Result<Router, String> {
    browser_router(app, secret, networks, origins, None)
}

pub async fn with_wan_browser_seats(
    app: Router,
    secret: String,
    networks: &str,
    origins: &str,
    config: Option<WanTesterConfig>,
) -> Result<Router, String> {
    let wan = match config {
        Some(config) => Some(WanGate::open(config).await?),
        None => None,
    };
    browser_router(app, secret, networks, origins, wan)
}

fn browser_router(
    app: Router,
    secret: String,
    networks: &str,
    origins: &str,
    wan: Option<WanGate>,
) -> Result<Router, String> {
    let networks = networks
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            s.trim()
                .parse::<IpNet>()
                .map_err(|_| "invalid FOURPLAY_BROWSER_SEAT_NETWORKS CIDR".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let origins = origins
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            let url = reqwest::Url::parse(s.trim()).map_err(|_| "invalid browser seat origin")?;
            if !["http", "https"].contains(&url.scheme())
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(
                    "browser seat origins must be exact http(s) origins without paths".to_owned(),
                );
            }
            Ok(url.origin().ascii_serialization())
        })
        .collect::<Result<Vec<_>, String>>()?;
    let state = Seats {
        app: app.clone(),
        secret: Arc::new(secret),
        networks,
        origins,
        wan,
    };
    Ok(app.merge(
        Router::new()
            .route("/client-v2", get(index))
            .route("/client-v2/", get(index))
            .route("/client-v2/experience.js", get(script))
            .route("/client-v2/styles.css", get(styles))
            .route("/client-v3", get(grid_index))
            .route("/client-v3/", get(grid_index))
            .route("/client-v3/experience.js", get(grid_script))
            .route("/client-v3/styles.css", get(grid_styles))
            .route("/client-v4", get(minimal_index))
            .route("/client-v4/", get(minimal_index))
            .route("/client-v4/experience.js", get(minimal_script))
            .route("/client-v4/styles.css", get(minimal_styles))
            .route("/client-v2/bootstrap", post(bootstrap))
            .route("/client-v2/webrtc-config", get(media_config))
            .route(
                "/client-v2/tester-access",
                get(tester_page).post(tester_enroll),
            )
            .route("/client-v2/tester-access.js", get(tester_script))
            .route(
                "/client-v2/api/{*path}",
                get(proxy).post(proxy).delete(proxy),
            )
            .with_state(state),
    ))
}

async fn index() -> Response {
    protected_response(
        (
            [("content-type", "text/html; charset=utf-8")],
            browser_html(),
        )
            .into_response(),
    )
}
fn browser_html() -> String {
    include_str!("../static/client/index.html")
        .replace("<body>", "<body data-client-version=\"2\">")
        .replace("</head>", "<link rel=\"stylesheet\" href=\"/client-v2/styles.css\" /></head>")
        .replace("<script src=\"/client/app.js\"", "<script src=\"/client-v2/experience.js\" defer></script>\n<script src=\"/client/app.js\"")
}
async fn grid_index() -> Response {
    protected_response(
        ([("content-type", "text/html; charset=utf-8")], grid_html()).into_response(),
    )
}
fn grid_html() -> String {
    browser_html()
        .replace("data-client-version=\"2\"", "data-client-version=\"2\" data-client-layout=\"grid\"")
        .replace("<title>4-Play Seat Client</title>", "<title>4-Play Arcade | V3</title>")
        .replace("</head>", "<link rel=\"stylesheet\" href=\"/client-v3/styles.css\" /></head>")
        .replace("<script src=\"/client/app.js\"", "<script src=\"/client-v3/experience.js\" defer></script>\n<script src=\"/client/app.js\"")
}
async fn minimal_index() -> Response {
    let html = grid_html()
        .replace("data-client-layout=\"grid\"", "data-client-layout=\"grid\" data-client-skin=\"minimal\"")
        .replace("4-Play Arcade | V3", "4-Play Arcade | V4")
        .replace("</head>", "<link rel=\"stylesheet\" href=\"/client-v4/styles.css\" /></head>")
        .replace("<script src=\"/client/app.js\"", "<script src=\"/client-v4/experience.js\" defer></script>\n<script src=\"/client/app.js\"");
    protected_response(([("content-type", "text/html; charset=utf-8")], html).into_response())
}
async fn minimal_script() -> Response {
    super::static_response(
        "text/javascript; charset=utf-8",
        include_str!("../static/client-v4/experience.js"),
    )
}
async fn minimal_styles() -> Response {
    super::static_response(
        "text/css; charset=utf-8",
        include_str!("../static/client-v4/styles.css"),
    )
}
async fn grid_script() -> Response {
    super::static_response(
        "text/javascript; charset=utf-8",
        include_str!("../static/client-v3/experience.js"),
    )
}
async fn grid_styles() -> Response {
    super::static_response(
        "text/css; charset=utf-8",
        include_str!("../static/client-v3/styles.css"),
    )
}
async fn script() -> Response {
    super::static_response(
        "text/javascript; charset=utf-8",
        include_str!("../static/client-v2/experience.js"),
    )
}
async fn styles() -> Response {
    super::static_response(
        "text/css; charset=utf-8",
        include_str!("../static/client-v2/styles.css"),
    )
}

fn fail(status: StatusCode, message: &str) -> Response {
    protected_response(
        (
            status,
            Json(json!({"code":"browser_seat_access", "message":message})),
        )
            .into_response(),
    )
}
fn protected_response(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert("cache-control", "no-store".parse().unwrap());
    headers.insert("x-frame-options", "DENY".parse().unwrap());
    headers.insert("referrer-policy", "same-origin".parse().unwrap());
    response
}

struct AccessDenied(StatusCode, &'static str);
fn gate_response(error: GateError) -> Response {
    let mut response = protected_response(
        (error.0, Json(json!({"code":error.1,"message":error.2}))).into_response(),
    );
    if error.0 == StatusCode::TOO_MANY_REQUESTS {
        response
            .headers_mut()
            .insert("retry-after", "60".parse().unwrap());
    }
    response
}

async fn media_config(
    State(state): State<Seats>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let (_, _, wan_id) = match state.authorize(&headers, peer, false).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if let Some(id) = wan_id {
        let Some(media) = state.wan.as_ref().and_then(|w| w.media.as_ref()) else {
            return gate_response(GateError(
                StatusCode::SERVICE_UNAVAILABLE,
                "wan_media_unavailable",
                "Internet media is not configured yet.",
            ));
        };
        return protected_response(
            Json(media.browser_config(id, super::unix_time_ms())).into_response(),
        );
    }
    if state.identity(&headers, super::unix_time_ms()).is_none() {
        return fail(
            StatusCode::UNAUTHORIZED,
            "Browser seat expired. Reload the client.",
        );
    }
    protected_response(Json(json!({"iceServers":[]})).into_response())
}

async fn tester_page(
    State(state): State<Seats>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request<Body>,
) -> Response {
    let Some(wan) = state.wan.as_ref() else {
        return fail(StatusCode::NOT_FOUND, "Not found.");
    };
    if request.uri().query().is_some() {
        return fail(
            StatusCode::BAD_REQUEST,
            "Invitation codes must not be placed in URLs.",
        );
    }
    if let Err(error) = wan.transport(request.headers(), peer) {
        return gate_response(error);
    }
    let mut response = protected_response(
        (
            [("content-type", "text/html; charset=utf-8")],
            include_str!("../static/client-v2/tester-access.html"),
        )
            .into_response(),
    );
    response.headers_mut().insert("content-security-policy", "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'".parse().unwrap());
    response
        .headers_mut()
        .insert("referrer-policy", "no-referrer".parse().unwrap());
    response
}

async fn tester_script() -> Response {
    super::static_response(
        "text/javascript; charset=utf-8",
        include_str!("../static/client-v2/tester-access.js"),
    )
}

async fn tester_enroll(
    State(state): State<Seats>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request<Body>,
) -> Response {
    let Some(wan) = state.wan.as_ref() else {
        return fail(StatusCode::NOT_FOUND, "Not found.");
    };
    if request.uri().query().is_some() {
        return fail(
            StatusCode::BAD_REQUEST,
            "Invitation codes must not be placed in URLs.",
        );
    }
    let ip = match wan.request(request.headers(), peer, true) {
        Ok(ip) => ip,
        Err(error) => return gate_response(error),
    };
    if let Err(error) = wan.take_attempt(ip) {
        return gate_response(error);
    }
    if request
        .headers()
        .get("content-type")
        .and_then(|s| s.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or("").trim())
        != Some("application/json")
    {
        return fail(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Expected JSON.");
    }
    let now = super::unix_time_ms();
    // Retrying a form submission with an already valid cookie must not consume a
    // second browser enrollment. It does not refresh/extend that cookie.
    if wan.identity(request.headers(), now).await.is_ok() {
        return protected_response(Json(json!({"ok":true})).into_response());
    }
    let bytes = match to_bytes(request.into_body(), 1024).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::PAYLOAD_TOO_LARGE, "Request too large."),
    };
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Enrollment {
        invitation: String,
    }
    let body: Enrollment = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "Expected an invitation code."),
    };
    let admission = match wan.access.redeem(&body.invitation, now).await {
        Ok(v) => v,
        Err(crate::tester_access::AccessError::Capacity) => {
            return gate_response(GateError(
                StatusCode::SERVICE_UNAVAILABLE,
                "tester_capacity",
                "All tester seats are currently in use.",
            ));
        }
        Err(crate::tester_access::AccessError::Storage) => {
            return gate_response(GateError(
                StatusCode::SERVICE_UNAVAILABLE,
                "tester_storage_unavailable",
                "Tester access is temporarily unavailable.",
            ));
        }
        Err(_) => {
            return gate_response(GateError(
                StatusCode::UNAUTHORIZED,
                "tester_invitation_denied",
                "Invitation is invalid, expired, revoked, or already used.",
            ));
        }
    };
    let mut response = protected_response(Json(json!({"ok":true})).into_response());
    response.headers_mut().insert(
        "set-cookie",
        format!(
            "{}={}; Path=/client-v2; HttpOnly; Secure; SameSite=Strict; Max-Age={}",
            crate::wan_testers::COOKIE,
            admission.credential,
            ((admission.expires_ms - now) / 1000).max(1)
        )
        .parse()
        .unwrap(),
    );
    response
}

impl IntoResponse for AccessDenied {
    fn into_response(self) -> Response {
        fail(self.0, self.1)
    }
}

impl Seats {
    async fn authorize(
        &self,
        headers: &HeaderMap,
        peer: SocketAddr,
        write: bool,
    ) -> Result<(IpAddr, String, Option<Uuid>), Response> {
        if let Some(wan) = self.wan.as_ref().filter(|wan| wan.matches_host(headers)) {
            let ip = wan.request(headers, peer, write).map_err(gate_response)?;
            let id = wan
                .identity(headers, super::unix_time_ms())
                .await
                .map_err(gate_response)?;
            return Ok((ip, wan.origin.clone(), Some(id)));
        }
        self.check(headers, peer, write)
            .map(|(ip, origin)| (ip, origin, None))
            .map_err(IntoResponse::into_response)
    }
    fn check(
        &self,
        headers: &HeaderMap,
        peer: SocketAddr,
        write: bool,
    ) -> Result<(IpAddr, String), AccessDenied> {
        if self.networks.is_empty() || self.origins.is_empty() {
            return Err(AccessDenied(
                StatusCode::SERVICE_UNAVAILABLE,
                "Automatic browser seats are not enabled by this site's operator.",
            ));
        }
        let ip = match peer.ip() {
            IpAddr::V6(ip) => ip
                .to_ipv4_mapped()
                .map(IpAddr::V4)
                .unwrap_or(IpAddr::V6(ip)),
            ip => ip,
        };
        if !self.networks.iter().any(|network| network.contains(&ip)) {
            return Err(AccessDenied(
                StatusCode::FORBIDDEN,
                "This network is not approved for browser seats.",
            ));
        }
        let host = headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let origin = self.origins.iter().find(|origin| {
            origin
                .split_once("://")
                .is_some_and(|(_, authority)| authority == host)
        });
        let Some(origin) = origin else {
            return Err(AccessDenied(
                StatusCode::FORBIDDEN,
                "Unapproved browser seat host.",
            ));
        };
        let supplied = headers.get("origin").and_then(|v| v.to_str().ok());
        if headers.get("x-4play-client").and_then(|v| v.to_str().ok()) != Some("v2")
            || supplied.is_some_and(|value| value != origin)
            || (write && supplied != Some(origin))
            || headers
                .get("sec-fetch-site")
                .is_some_and(|value| value != "same-origin" && value != "none")
        {
            return Err(AccessDenied(
                StatusCode::FORBIDDEN,
                "Browser requests must come from this site's own client.",
            ));
        }
        Ok((ip, origin.clone()))
    }

    fn mac(&self, value: &str) -> Signature {
        let mut mac =
            Signature::new_from_slice(self.secret.as_bytes()).expect("HMAC accepts any key size");
        mac.update(b"4play-browser-seat-v2\0");
        mac.update(value.as_bytes());
        mac
    }
    fn issue(&self, id: Uuid, now: u64, secure: bool) -> String {
        let value = format!("{id}.{}", now.saturating_add(TTL_MS));
        let signature: String = self
            .mac(&value)
            .finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!(
            "{COOKIE}={value}.{signature}; Path=/client-v2; HttpOnly; SameSite=Strict; Max-Age={}{}",
            TTL_MS / 1000,
            if secure { "; Secure" } else { "" }
        )
    }
    fn identity(&self, headers: &HeaderMap, now: u64) -> Option<Uuid> {
        let cookies = headers.get("cookie")?.to_str().ok()?;
        let value = cookies
            .split(';')
            .map(str::trim)
            .find_map(|s| s.strip_prefix(&format!("{COOKIE}=")))?;
        let (payload, signature) = value.rsplit_once('.')?;
        let (id, expires) = payload.split_once('.')?;
        let expires = expires.parse::<u64>().ok()?;
        if expires <= now || expires > now.saturating_add(TTL_MS) || signature.len() != 64 {
            return None;
        }
        let bytes = (0..64)
            .step_by(2)
            .map(|index| u8::from_str_radix(signature.get(index..index + 2)?, 16).ok())
            .collect::<Option<Vec<_>>>()?;
        self.mac(payload).verify_slice(&bytes).ok()?;
        Uuid::parse_str(id).ok()
    }
}

async fn bootstrap(
    State(state): State<Seats>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let (ip, origin, wan_id) = match state.authorize(&headers, peer, true).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let now = super::unix_time_ms();
    let id = wan_id
        .or_else(|| state.identity(&headers, now))
        .unwrap_or_else(Uuid::new_v4);
    let mut response =
        Json(json!({ "seat_id":format!("browser-{id}"), "destination_address":ip.to_string() }))
            .into_response();
    if wan_id.is_none() {
        response.headers_mut().insert(
            "set-cookie",
            state
                .issue(id, now, origin.starts_with("https:"))
                .parse()
                .unwrap(),
        );
    }
    protected_response(response)
}

fn allowed(method: &Method, path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    if method == Method::GET
        && (matches!(
            path,
            "games" | "active-sessions" | "experimental/webrtc/status"
        ) || path.starts_with("assets/"))
    {
        return true;
    }
    if method == Method::POST && path == "sessions" {
        return true;
    }
    if parts.len() < 2 || parts[0] != "sessions" || Uuid::parse_str(parts[1]).is_err() {
        return false;
    }
    match (method.as_str(), &parts[2..]) {
        ("GET", [])
        | ("POST", ["stop"])
        | ("POST", ["spectators"])
        | ("POST", ["webrtc", "offer"]) => true,
        ("DELETE", ["spectators", grant]) => Uuid::parse_str(grant).is_ok(),
        ("POST", ["player-slots", player, action]) => {
            player.parse::<u32>().is_ok_and(|n| (1..=4).contains(&n))
                && ["reserve", "connect", "disconnect", "release"].contains(action)
        }
        ("POST", ["player-slots", player, "webrtc-input", "lease"]) => {
            player.parse::<u32>().is_ok_and(|n| (1..=4).contains(&n))
        }
        _ => false,
    }
}

fn scrub(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("token");
            for value in map.values_mut() {
                scrub(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                scrub(value);
            }
        }
        _ => (),
    }
}

async fn proxy(
    State(state): State<Seats>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request<Body>,
) -> Response {
    let (ip, _, wan_id) = match state
        .authorize(request.headers(), peer, request.method() != Method::GET)
        .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(id) = wan_id.or_else(|| state.identity(request.headers(), super::unix_time_ms()))
    else {
        return fail(
            StatusCode::UNAUTHORIZED,
            "Browser seat expired. Reload the client to reconnect.",
        );
    };
    let seat = format!("browser-{id}");
    let wan_admission = if wan_id.is_some() {
        crate::wan_testers::admission_fingerprint(request.headers())
    } else {
        None
    };
    let path = request
        .uri()
        .path()
        .strip_prefix("/client-v2/api/")
        .unwrap_or("");
    if !allowed(request.method(), path) || request.uri().query().is_some() {
        return fail(
            StatusCode::FORBIDDEN,
            "This operation is not available to browser seats.",
        );
    }
    // Read/stop only your own session. Join/spectate use existing slot/grant ownership checks.
    let parts: Vec<_> = path.split('/').collect();
    if parts.first() == Some(&"sessions")
        && (request.method() == Method::GET || parts.last() == Some(&"stop"))
    {
        let lookup = Request::builder()
            .uri(format!("/api/v1/sessions/{}", parts[1]))
            .header("authorization", format!("Bearer {}", state.secret))
            .body(Body::empty())
            .unwrap();
        let response = state.app.clone().oneshot(lookup).await.unwrap();
        if !response.status().is_success() {
            return protected_response(response);
        }
        let bytes = match to_bytes(response.into_body(), MAX_BODY).await {
            Ok(bytes) => bytes,
            Err(_) => return fail(StatusCode::BAD_GATEWAY, "Session lookup failed."),
        };
        let session: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let owns = session["seat_id"] == seat;
        let plays = session["player_slots"]
            .as_array()
            .is_some_and(|slots| slots.iter().any(|slot| slot["seat_id"] == seat));
        if !owns && (parts.last() == Some(&"stop") || !plays) {
            return fail(
                StatusCode::FORBIDDEN,
                "This browser does not own that session.",
            );
        }
    }
    let target = format!("/api/v1/{path}");
    let (mut parts, body) = request.into_parts();
    let body = if parts.method == Method::GET {
        Body::empty()
    } else {
        let bytes = match to_bytes(body, MAX_BODY).await {
            Ok(bytes) => bytes,
            Err(_) => return fail(StatusCode::PAYLOAD_TOO_LARGE, "Browser request too large."),
        };
        let mut value: Value = match serde_json::from_slice(&bytes) {
            Ok(Value::Object(map)) => Value::Object(map),
            _ => return fail(StatusCode::BAD_REQUEST, "Expected a JSON object."),
        };
        value["seat_id"] = json!(seat);
        if target == "/api/v1/sessions" || target.ends_with("/spectators") {
            value["destination_address"] = json!(ip.to_string());
        }
        Body::from(serde_json::to_vec(&value).unwrap())
    };
    parts.uri = target.parse().unwrap();
    // Do not carry the outer wildcard's routing parameters into the inner API.
    // Preserve only the real socket peer, used for WebRTC ICE address handling.
    parts.extensions.clear();
    parts
        .extensions
        .insert(ConnectInfo(SocketAddr::new(ip, peer.port())));
    if let Some(admission_hash) = wan_admission {
        parts
            .extensions
            .insert(super::WanMediaRequest { admission_hash });
    }
    // Forward only server-built headers, never the edge key, cookie, or a
    // browser-supplied private publisher/input/forwarding header.
    parts.headers = HeaderMap::new();
    parts.headers.insert(
        "authorization",
        format!("Bearer {}", state.secret).parse().unwrap(),
    );
    parts
        .headers
        .insert("content-type", "application/json".parse().unwrap());
    let response = state
        .app
        .clone()
        .oneshot(Request::from_parts(parts, body))
        .await
        .unwrap();
    if !response
        .headers()
        .get("content-type")
        .is_some_and(|value| value.to_str().unwrap_or("").starts_with("application/json"))
    {
        return protected_response(response);
    }
    let (parts, body) = response.into_parts();
    let bytes = match to_bytes(body, 8 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return fail(StatusCode::BAD_GATEWAY, "Browser response too large."),
    };
    let mut value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return fail(StatusCode::BAD_GATEWAY, "Invalid browser response."),
    };
    scrub(&mut value);
    let mut response = Response::from_parts(parts, Body::from(serde_json::to_vec(&value).unwrap()));
    response.headers_mut().remove("content-length");
    protected_response(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Request as ExtractRequest;

    const SECRET: &str = "server-only-test-seat-secret";
    const ORIGIN: &str = "http://192.168.20.68:8080";
    const WAN_ORIGIN: &str = "https://arcade.example.test";
    const EDGE_KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    async fn wan_fixture() -> (
        tempfile::TempDir,
        crate::tester_access::TesterAccess,
        Router,
    ) {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let database = dir.path().join("testers.db");
        let access = crate::tester_access::TesterAccess::open(&database)
            .await
            .unwrap();
        let app = with_wan_browser_seats(
            Router::new().route("/api/v1/{*path}", get(echo).post(echo).delete(echo)),
            SECRET.into(),
            "192.168.20.0/24",
            ORIGIN,
            Some(WanTesterConfig {
                origin: WAN_ORIGIN.into(),
                edge_key: EDGE_KEY.into(),
                database,
            }),
        )
        .await
        .unwrap();
        (dir, access, app)
    }
    fn wan_request(method: Method, path: &str, cookie: Option<&str>) -> Request<Body> {
        let mut r = request(method, path, cookie, "127.0.0.1", WAN_ORIGIN);
        r.headers_mut()
            .insert("host", "arcade.example.test".parse().unwrap());
        r.headers_mut()
            .insert("x-4play-edge-key", EDGE_KEY.parse().unwrap());
        r.headers_mut()
            .insert("x-4play-client-ip", "192.0.2.123".parse().unwrap());
        r
    }
    async fn wan_enroll(app: Router, code: &str) -> Response {
        let r = wan_request(Method::POST, "/client-v2/tester-access", None);
        let (parts, _) = r.into_parts();
        app.oneshot(Request::from_parts(
            parts,
            Body::from(json!({"invitation":code}).to_string()),
        ))
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn wan_enrollment_cookie_scope_revocation_and_scoped_proxy() {
        let (_dir, access, app) = wan_fixture().await;
        let now = super::super::unix_time_ms();
        let invite = access.issue(now, now + 3_600_000, 1).await.unwrap();
        let unauth = app
            .clone()
            .oneshot(wan_request(Method::POST, "/client-v2/bootstrap", None))
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(body(unauth).await["code"], "tester_access_required");
        let response = wan_enroll(app.clone(), &invite.code).await;
        assert_eq!(response.status(), StatusCode::OK);
        let full_cookie = response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(full_cookie.contains("Path=/client-v2; HttpOnly; Secure; SameSite=Strict"));
        let cookie = full_cookie.split(';').next().unwrap();
        let response_body = body(response).await;
        assert_eq!(response_body, json!({"ok":true}));
        let bootstrap = app
            .clone()
            .oneshot(wan_request(
                Method::POST,
                "/client-v2/bootstrap",
                Some(cookie),
            ))
            .await
            .unwrap();
        assert_eq!(bootstrap.status(), StatusCode::OK);
        assert!(bootstrap.headers().get("set-cookie").is_none());
        let identity = body(bootstrap).await;
        assert_eq!(identity["destination_address"], "192.0.2.123");
        let response = app
            .clone()
            .oneshot(wan_request(
                Method::POST,
                "/client-v2/api/sessions",
                Some(cookie),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value = body(response).await;
        assert_eq!(value["peer"], "192.0.2.123");
        assert_eq!(value["echo"]["seat_id"], identity["seat_id"]);
        assert!(value["connection_grant"].get("token").is_none());
        for path in [
            "/client-v2/api/runtime-hosts",
            "/client-v2/api/games/tmnt/metadata",
            "/client-v2/api/sessions/cleanup",
        ] {
            let r = app
                .clone()
                .oneshot(wan_request(Method::POST, path, Some(cookie)))
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::FORBIDDEN);
        }
        // Existing cookie cannot consume a new enrollment by retrying the form.
        let r = app
            .clone()
            .oneshot(wan_request(
                Method::POST,
                "/client-v2/tester-access",
                Some(cookie),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(access.list().await.unwrap()[0].used_seats, 1);
        access.revoke(invite.id).await.unwrap();
        let r = app
            .oneshot(wan_request(
                Method::GET,
                "/client-v2/api/games",
                Some(cookie),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wan_forged_edges_origins_and_lan_cookie_cannot_enroll() {
        let (_dir, access, app) = wan_fixture().await;
        for variant in 0..8 {
            let mut r = wan_request(Method::POST, "/client-v2/tester-access", None);
            match variant {
                0 => {
                    r.headers_mut().remove("x-4play-edge-key");
                }
                1 => {
                    r.headers_mut()
                        .insert("x-4play-edge-key", "b".repeat(64).parse().unwrap());
                }
                2 => {
                    r.extensions_mut().insert(ConnectInfo(
                        "192.168.20.10:1234".parse::<SocketAddr>().unwrap(),
                    ));
                }
                3 => {
                    r.headers_mut()
                        .insert("origin", "https://evil.example".parse().unwrap());
                }
                4 => {
                    r.headers_mut().remove("origin");
                }
                5 => {
                    r.headers_mut()
                        .insert("x-4play-client-ip", "127.0.0.1".parse().unwrap());
                }
                6 => {
                    r.headers_mut()
                        .append("x-4play-client-ip", "192.0.2.5".parse().unwrap());
                }
                _ => {
                    r.headers_mut()
                        .insert("sec-fetch-site", "cross-site".parse().unwrap());
                }
            }
            let response = app.clone().oneshot(r).await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "variant {variant}"
            );
        }
        assert!(access.list().await.unwrap().is_empty());
        // A valid LAN cookie is not a public admission cookie, and real LAN
        // enrollment remains independent of this new opt-in policy.
        let (cookie, _) = enroll(app.clone()).await;
        let r = app
            .oneshot(wan_request(
                Method::POST,
                "/client-v2/bootstrap",
                Some(&cookie),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wan_login_limits_and_secrets_in_urls_are_rejected() {
        let (_dir, _access, app) = wan_fixture().await;
        for _ in 0..5 {
            let r = wan_enroll(app.clone(), &"f".repeat(64)).await;
            assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        }
        let r = wan_enroll(app.clone(), &"f".repeat(64)).await;
        assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(r.headers()["retry-after"], "60");
        for method in [Method::GET, Method::POST] {
            let r = app
                .clone()
                .oneshot(wan_request(
                    method,
                    "/client-v2/tester-access?invitation=secret",
                    None,
                ))
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        }
        let r = app
            .clone()
            .oneshot(wan_request(Method::GET, "/client-v2/tester-access", None))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers().get("content-security-policy").is_some());
        let r = fixture()
            .oneshot(wan_request(Method::GET, "/client-v2/tester-access", None))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }
    async fn echo(request: ExtractRequest) -> Json<Value> {
        assert!(!request.headers().contains_key("x-4play-edge-key"));
        assert!(!request.headers().contains_key("cookie"));
        assert_eq!(
            request.headers()["authorization"],
            format!("Bearer {SECRET}")
        );
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|p| p.0.ip().to_string());
        let bytes = to_bytes(request.into_body(), MAX_BODY).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Json(
            json!({"echo":body,"peer":peer,"connection_grant":{"token":"private-runtime-token", "webrtc_signaling":{"offer_path":"/api/v1/sessions/test/webrtc/offer"}}}),
        )
    }
    fn fixture() -> Router {
        with_browser_seats(
            Router::new().route("/api/v1/{*path}", get(echo).post(echo).delete(echo)),
            SECRET.into(),
            "192.168.20.0/24,192.168.0.0/24",
            ORIGIN,
        )
        .unwrap()
    }
    fn request(
        method: Method,
        path: &str,
        cookie: Option<&str>,
        ip: &str,
        origin: &str,
    ) -> Request<Body> {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "192.168.20.68:8080")
            .header("origin", origin)
            .header("x-4play-client", "v2")
            .header("content-type", "application/json");
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        let mut request = request
            .body(Body::from(
                r#"{"game_id":"tmnt","seat_id":"someone-else","destination_address":"8.8.8.8"}"#,
            ))
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(
            format!("{ip}:12345").parse::<SocketAddr>().unwrap(),
        ));
        request
    }
    async fn body(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"text":String::from_utf8_lossy(&bytes)}))
    }
    async fn enroll(app: Router) -> (String, String) {
        let response = app
            .oneshot(request(
                Method::POST,
                "/client-v2/bootstrap",
                None,
                "192.168.20.10",
                ORIGIN,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(cookie.contains("HttpOnly; SameSite=Strict"));
        assert!(cookie.contains("Path=/client-v2"));
        let value = body(response).await;
        assert!(!value.to_string().contains(SECRET));
        (
            cookie.split(';').next().unwrap().to_owned(),
            value["seat_id"].as_str().unwrap().into(),
        )
    }

    #[tokio::test]
    async fn cookie_identity_is_automatic_stable_and_server_scoped() {
        let app = fixture();
        let (cookie, seat) = enroll(app.clone()).await;
        let renewed = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/client-v2/bootstrap",
                Some(&cookie),
                "192.168.0.186",
                ORIGIN,
            ))
            .await
            .unwrap();
        assert_eq!(body(renewed).await["seat_id"], seat);
        // A fresh app process with the same server secret recognizes the cookie.
        let response = fixture()
            .oneshot(request(
                Method::POST,
                "/client-v2/api/sessions",
                Some(&cookie),
                "192.168.0.186",
                ORIGIN,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let value = body(response).await;
        assert_eq!(value["echo"]["seat_id"], seat);
        assert_eq!(value["echo"]["destination_address"], "192.168.0.186");
        assert_eq!(value["peer"], "192.168.0.186");
        assert!(value["connection_grant"]["token"].is_null());
        assert!(!value.to_string().contains("private-runtime-token"));
        let (other, other_seat) = enroll(app).await;
        assert_ne!(cookie, other);
        assert_ne!(seat, other_seat);
    }

    #[tokio::test]
    async fn v2_assets_are_separate_and_never_embed_server_credentials() {
        let app = fixture();
        for path in [
            "/client-v2",
            "/client-v2/",
            "/client-v2/experience.js",
            "/client-v2/styles.css",
            "/client-v3",
            "/client-v3/",
            "/client-v3/experience.js",
            "/client-v3/styles.css",
            "/client-v4",
            "/client-v4/",
            "/client-v4/experience.js",
            "/client-v4/styles.css",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            let text = String::from_utf8(bytes.to_vec()).unwrap();
            assert!(!text.contains(SECRET));
            if path.ends_with("v2") || path.ends_with("v2/") {
                assert!(text.contains("data-client-version=\"2\""));
                assert!(
                    text.find("/client-v2/experience.js").unwrap()
                        < text.find("/client/app.js").unwrap()
                );
            }
            if path.ends_with("v3") || path.ends_with("v3/") {
                assert!(text.contains("data-client-layout=\"grid\""));
                assert!(
                    text.find("/client-v2/experience.js").unwrap()
                        < text.find("/client-v3/experience.js").unwrap()
                );
                assert!(
                    text.find("/client-v3/experience.js").unwrap()
                        < text.find("/client/app.js").unwrap()
                );
            }
            if path.ends_with("v4") || path.ends_with("v4/") {
                assert!(text.contains("data-client-skin=\"minimal\""));
                assert!(
                    text.find("/client-v3/experience.js").unwrap()
                        < text.find("/client-v4/experience.js").unwrap()
                );
                assert!(
                    text.find("/client-v4/experience.js").unwrap()
                        < text.find("/client/app.js").unwrap()
                );
            }
        }
    }

    #[tokio::test]
    async fn enrollment_fails_closed_for_network_host_origin_and_missing_opt_in() {
        for (ip, origin) in [
            ("192.168.10.9", ORIGIN),
            ("192.168.20.10", "http://evil.test"),
            ("192.168.20.10", "null"),
        ] {
            let mut req = request(Method::POST, "/client-v2/bootstrap", None, ip, origin);
            req.headers_mut()
                .insert("x-forwarded-for", "192.168.20.10".parse().unwrap());
            assert_eq!(
                fixture().oneshot(req).await.unwrap().status(),
                StatusCode::FORBIDDEN
            );
        }
        for header in ["host", "origin", "x-4play-client"] {
            let mut req = request(
                Method::POST,
                "/client-v2/bootstrap",
                None,
                "192.168.20.10",
                ORIGIN,
            );
            req.headers_mut().remove(header);
            assert_eq!(
                fixture().oneshot(req).await.unwrap().status(),
                StatusCode::FORBIDDEN
            );
        }
        let disabled = with_browser_seats(Router::new(), SECRET.into(), "", "").unwrap();
        assert_eq!(
            disabled
                .oneshot(request(
                    Method::POST,
                    "/client-v2/bootstrap",
                    None,
                    "192.168.20.10",
                    ORIGIN
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn browser_cookies_cannot_mutate_admin_metadata_or_read_global_grants() {
        let app = fixture();
        let (cookie, _) = enroll(app.clone()).await;
        for (method, path) in [
            (Method::POST, "games/tmnt/metadata"),
            (Method::POST, "runtime-hosts/reference-linux/heartbeat"),
            (Method::GET, "sessions"),
            (Method::GET, "admin/game-imports"),
            (Method::POST, "sessions/cleanup-stale"),
            (Method::GET, "admin/metadata-providers/thegamesdb/search"),
            (Method::GET, "games?token=x"),
            (Method::POST, "experimental/webrtc/test-pattern/offer"),
        ] {
            let response = app
                .clone()
                .oneshot(request(
                    method,
                    &format!("/client-v2/api/{path}"),
                    Some(&cookie),
                    "192.168.20.10",
                    ORIGIN,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        }
        let mut tampered = cookie.clone();
        tampered.push('a');
        let response = app
            .oneshot(request(
                Method::GET,
                "/client-v2/api/games",
                Some(&tampered),
                "192.168.20.10",
                ORIGIN,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        // Original APIs still require bearer authentication, not a V2 cookie.
        let original = crate::app().await.unwrap();
        assert_eq!(
            original
                .oneshot(request(
                    Method::GET,
                    "/api/v1/games",
                    Some(&cookie),
                    "192.168.20.10",
                    ORIGIN
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn real_session_ownership_is_isolated_between_automatic_seats() {
        let mut state = crate::AppState::in_memory(crate::DEFAULT_OFFLINE_AFTER)
            .await
            .unwrap();
        state.seat_api_token = SECRET.into();
        let inner = crate::app_with_state(state);
        for (path, value) in [
            (
                "/api/v1/runtime-hosts/linux",
                json!({"display_name":"Linux", "agent_version":"test", "capabilities":{
                "data_plane_address":"192.168.20.68", "operating_system":"linux", "architecture":"x86_64",
                "logical_cpu_count":4, "memory_bytes":1000000, "encoder_names":["libx264"], "emulator_adapters":["mame"]}}),
            ),
            (
                "/api/v1/runtime-hosts/linux/catalog",
                json!({"games":[{"id":"tmnt", "display_name":"TMNT", "rom_name":"tmnt", "profile":{
                "width":320,"height":224,"refresh_hz":60.0,"rotation_degrees":0,"max_players":4,"buttons_per_player":2,"supports_save_state":true}}]}),
            ),
        ] {
            let response = inner
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::PUT)
                        .uri(path)
                        .header("authorization", format!("Bearer {SECRET}"))
                        .header("content-type", "application/json")
                        .body(Body::from(value.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(
                response.status().is_success(),
                "{path}: {}",
                body(response).await
            );
        }
        let app =
            with_browser_seats(inner.clone(), SECRET.into(), "192.168.20.0/24", ORIGIN).unwrap();
        let (cookie1, seat1) = enroll(app.clone()).await;
        let (cookie2, seat2) = enroll(app.clone()).await;
        let response = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/client-v2/api/sessions",
                Some(&cookie1),
                "192.168.20.10",
                ORIGIN,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let session = body(response).await;
        assert_eq!(session["seat_id"], seat1);
        assert_eq!(session["destination_address"], "192.168.20.10");
        assert!(session["connection_grant"]["token"].is_null());
        let id = session["id"].as_str().unwrap();
        for state in ["starting", "ready", "active"] {
            let response = inner
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::PUT)
                        .uri(format!("/api/v1/runtime-hosts/linux/sessions/{id}/state"))
                        .header("authorization", format!("Bearer {SECRET}"))
                        .header("content-type", "application/json")
                        .body(Body::from(json!({"state":state}).to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(
                response.status().is_success(),
                "state transition: {}",
                body(response).await
            );
        }
        let path = format!("/client-v2/api/sessions/{id}");
        for (cookie, expected) in [
            (&cookie1, StatusCode::OK),
            (&cookie2, StatusCode::FORBIDDEN),
        ] {
            assert_eq!(
                app.clone()
                    .oneshot(request(
                        Method::GET,
                        &path,
                        Some(cookie),
                        "192.168.20.10",
                        ORIGIN
                    ))
                    .await
                    .unwrap()
                    .status(),
                expected
            );
        }
        assert_eq!(
            app.clone()
                .oneshot(request(
                    Method::POST,
                    &format!("{path}/stop"),
                    Some(&cookie2),
                    "192.168.20.10",
                    ORIGIN
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let response = app
            .clone()
            .oneshot(request(
                Method::POST,
                &format!("{path}/player-slots/2/reserve"),
                Some(&cookie2),
                "192.168.20.10",
                ORIGIN,
            ))
            .await
            .unwrap();
        assert!(response.status().is_success());
        let joined = body(response).await;
        assert_eq!(joined["player_slots"][1]["seat_id"], seat2);
        assert!(joined["connection_grant"]["token"].is_null());
        // Seat 1 cannot use body seat_id spoofing to release seat 2's reservation.
        let response = app
            .clone()
            .oneshot(request(
                Method::POST,
                &format!("{path}/player-slots/2/release"),
                Some(&cookie1),
                "192.168.20.10",
                ORIGIN,
            ))
            .await
            .unwrap();
        assert!(!response.status().is_success());
    }

    #[test]
    fn cookies_expire_reject_forgery_and_keys_rotate() {
        let state = Seats {
            app: Router::new(),
            secret: Arc::new(SECRET.into()),
            networks: vec![],
            origins: vec![],
            wan: None,
        };
        let id = Uuid::new_v4();
        let issued = state.issue(id, 100, true);
        assert!(issued.ends_with("; Secure"));
        let mut headers = HeaderMap::new();
        headers.insert("cookie", issued.parse().unwrap());
        assert_eq!(state.identity(&headers, 101), Some(id));
        assert_eq!(state.identity(&headers, 100 + TTL_MS), None);
        let changed = Seats {
            secret: Arc::new("different-key".into()),
            ..state.clone()
        };
        assert_eq!(changed.identity(&headers, 101), None);
        assert!(with_browser_seats(Router::new(), SECRET.into(), "bad-cidr", ORIGIN).is_err());
        assert!(
            with_browser_seats(
                Router::new(),
                SECRET.into(),
                "192.168.0.0/16",
                "http://server/path"
            )
            .is_err()
        );
    }
}
