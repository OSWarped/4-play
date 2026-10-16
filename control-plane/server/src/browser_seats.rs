//! Opt-in, direct-LAN browser seats. The shared bearer never leaves the server.
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
}

/// Empty settings disable automatic enrollment. No forwarded headers are trusted.
pub fn with_browser_seats(
    app: Router,
    secret: String,
    networks: &str,
    origins: &str,
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
            .route("/client-v2/bootstrap", post(bootstrap))
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
    let html = browser_html()
        .replace("data-client-version=\"2\"", "data-client-version=\"2\" data-client-layout=\"grid\"")
        .replace("<title>4-Play Seat Client</title>", "<title>4-Play Arcade | V3</title>")
        .replace("</head>", "<link rel=\"stylesheet\" href=\"/client-v3/styles.css\" /></head>")
        .replace("<script src=\"/client/app.js\"", "<script src=\"/client-v3/experience.js\" defer></script>\n<script src=\"/client/app.js\"");
    protected_response(([("content-type", "text/html; charset=utf-8")], html).into_response())
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
impl IntoResponse for AccessDenied {
    fn into_response(self) -> Response {
        fail(self.0, self.1)
    }
}

impl Seats {
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
    let (ip, origin) = match state.check(&headers, peer, true) {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };
    let now = super::unix_time_ms();
    let id = state.identity(&headers, now).unwrap_or_else(Uuid::new_v4);
    let mut response =
        Json(json!({ "seat_id":format!("browser-{id}"), "destination_address":ip.to_string() }))
            .into_response();
    response.headers_mut().insert(
        "set-cookie",
        state
            .issue(id, now, origin.starts_with("https:"))
            .parse()
            .unwrap(),
    );
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
    let (ip, _) = match state.check(request.headers(), peer, request.method() != Method::GET) {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };
    let Some(id) = state.identity(request.headers(), super::unix_time_ms()) else {
        return fail(
            StatusCode::UNAUTHORIZED,
            "Browser seat expired. Reload the client to reconnect.",
        );
    };
    let seat = format!("browser-{id}");
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
    parts.extensions.insert(ConnectInfo(peer));
    parts.headers.remove("cookie");
    parts.headers.remove("content-length");
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
    async fn echo(request: ExtractRequest) -> Json<Value> {
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
