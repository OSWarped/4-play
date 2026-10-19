use std::{env, error::Error, net::SocketAddr, path::PathBuf, time::Duration};

use control_plane_server::PortPoolConfig;
use tokio::net::TcpListener;

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:8080";
const BIND_ENVIRONMENT_VARIABLE: &str = "FOURPLAY_CONTROL_PLANE_BIND";
const DEFAULT_DATABASE_PATH: &str = "data/control-plane.sqlite3";
const DEFAULT_OFFLINE_SECONDS: u64 = 15;
const DEFAULT_PREVIEW_STALE_MS: u64 = 5_000;
const WEBRTC_TEST_PATTERN_PUBLISHER_ENVIRONMENT_VARIABLE: &str =
    "FOURPLAY_WEBRTC_TEST_PATTERN_PUBLISHER_URL";
const WEBRTC_SESSION_PUBLISHER_ENVIRONMENT_VARIABLE: &str =
    "FOURPLAY_WEBRTC_SESSION_PUBLISHER_URL_TEMPLATE";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = env::args_os().collect();
    if arguments
        .iter()
        .any(|value| value == "--check-wan-turn-credential")
    {
        if arguments.len() != 2 {
            return Err("The TURN credential check accepts no additional arguments".into());
        }
        control_plane_server::validate_wan_turn_systemd_credential()?;
        println!(
            "PASS: Rust TURN systemd credential validated; no secret printed, listener or database opened"
        );
        return Ok(());
    }
    let bind_address = env::var(BIND_ENVIRONMENT_VARIABLE)
        .unwrap_or_else(|_| DEFAULT_BIND_ADDRESS.to_owned())
        .parse::<SocketAddr>()?;
    let database_path = PathBuf::from(
        env::var("FOURPLAY_CONTROL_PLANE_DATABASE")
            .unwrap_or_else(|_| DEFAULT_DATABASE_PATH.to_owned()),
    );
    if let Some(parent) = database_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let offline_seconds = env::var("FOURPLAY_RUNTIME_HOST_OFFLINE_SECONDS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(DEFAULT_OFFLINE_SECONDS);
    if offline_seconds == 0 {
        return Err("FOURPLAY_RUNTIME_HOST_OFFLINE_SECONDS must be greater than zero".into());
    }
    let seat_api_token = required_secret("FOURPLAY_SEAT_API_TOKEN")?;
    let runtime_host_api_token = required_secret("FOURPLAY_RUNTIME_HOST_API_TOKEN")?;
    let port_pools = port_pool_config()?;
    let asset_root = env::var("FOURPLAY_ASSET_ROOT").ok().map(PathBuf::from);
    let preview_stale_ms = env::var("FOURPLAY_PREVIEW_STALE_MS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(DEFAULT_PREVIEW_STALE_MS);
    if preview_stale_ms == 0 {
        return Err("FOURPLAY_PREVIEW_STALE_MS must be greater than zero".into());
    }
    let webrtc_test_pattern_publisher_url =
        env::var(WEBRTC_TEST_PATTERN_PUBLISHER_ENVIRONMENT_VARIABLE).ok();
    let webrtc_session_publisher_url_template =
        env::var(WEBRTC_SESSION_PUBLISHER_ENVIRONMENT_VARIABLE).ok();
    let listener = TcpListener::bind(bind_address).await?;
    let app = control_plane_server::app_with_database_tokens_ports_assets_preview_and_webrtc(
        &database_path,
        Duration::from_secs(offline_seconds),
        seat_api_token.clone(),
        runtime_host_api_token,
        port_pools,
        asset_root.clone(),
        Duration::from_millis(preview_stale_ms),
        webrtc_test_pattern_publisher_url.clone(),
        webrtc_session_publisher_url_template.clone(),
    )
    .await?;
    let app = control_plane_server::with_thegamesdb(
        app,
        env::var("FOURPLAY_THEGAMESDB_API_KEY").ok(),
        PathBuf::from(
            env::var("FOURPLAY_THEGAMESDB_CACHE").unwrap_or_else(|_| "data/thegamesdb".to_owned()),
        ),
    )?;
    let import_config = if env::var("FOURPLAY_GAME_IMPORT_ENABLED").as_deref() == Ok("1") {
        let setting = |name: &str| {
            env::var(name)
                .map_err(|_| std::io::Error::other(format!("{name} is required for game imports")))
        };
        Some(control_plane_server::GameImportConfig {
            python: env::var("FOURPLAY_GAME_IMPORT_PYTHON")
                .unwrap_or_else(|_| "/usr/bin/python3".into()),
            worker: setting("FOURPLAY_GAME_IMPORT_WORKER")?,
            mame: setting("FOURPLAY_MAME_PATH")?,
            ini: setting("FOURPLAY_MAME_INI_PATH")?,
            rom_root: setting("FOURPLAY_GAME_IMPORT_ROM_ROOT")?,
            sample_root: setting("FOURPLAY_GAME_IMPORT_SAMPLE_ROOT")?,
            stage_root: setting("FOURPLAY_GAME_IMPORT_STAGE_ROOT")?,
            manifest: setting("FOURPLAY_CATALOG_PATH")?,
            host_id: setting("FOURPLAY_RUNTIME_HOST_ID")?,
            chdman: env::var("FOURPLAY_GAME_IMPORT_CHDMAN").ok(),
        })
    } else {
        None
    };
    let app = control_plane_server::with_game_imports(app, import_config);
    let app = control_plane_server::with_wan_browser_seats(
        app,
        seat_api_token,
        &env::var("FOURPLAY_BROWSER_SEAT_NETWORKS").unwrap_or_default(),
        &env::var("FOURPLAY_BROWSER_SEAT_ORIGINS").unwrap_or_default(),
        control_plane_server::WanTesterConfig::from_environment().map_err(std::io::Error::other)?,
    )
    .await
    .map_err(std::io::Error::other)?;

    println!(
        "4-Play control plane listening on http://{bind_address} database={} host_offline_seconds={offline_seconds} media_ports={}:{} input_ports={}:{} asset_root={} preview_stale_ms={preview_stale_ms} webrtc_test_pattern_publisher={} webrtc_session_publisher_template={}",
        database_path.display(),
        port_pools.media_port_start,
        port_pools.media_port_count,
        port_pools.input_port_start,
        port_pools.input_port_count,
        asset_root
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "disabled".to_owned()),
        webrtc_test_pattern_publisher_url
            .as_deref()
            .unwrap_or("disabled"),
        webrtc_session_publisher_url_template
            .as_deref()
            .unwrap_or("disabled")
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

fn required_secret(name: &str) -> Result<String, Box<dyn Error>> {
    let value = env::var(name).map_err(|_| format!("{name} must be set"))?;
    if value.len() < 16 {
        return Err(format!("{name} must contain at least 16 characters").into());
    }
    Ok(value)
}

fn port_pool_config() -> Result<PortPoolConfig, Box<dyn Error>> {
    let defaults = PortPoolConfig::default();
    let config = PortPoolConfig {
        media_port_start: optional_u16("FOURPLAY_MEDIA_PORT_START")?
            .unwrap_or(defaults.media_port_start),
        media_port_count: optional_u16("FOURPLAY_MEDIA_PORT_COUNT")?
            .unwrap_or(defaults.media_port_count),
        input_port_start: optional_u16("FOURPLAY_INPUT_PORT_START")?
            .unwrap_or(defaults.input_port_start),
        input_port_count: optional_u16("FOURPLAY_INPUT_PORT_COUNT")?
            .unwrap_or(defaults.input_port_count),
    };
    validate_port_pool(
        "FOURPLAY_MEDIA",
        config.media_port_start,
        config.media_port_count,
    )?;
    validate_port_pool(
        "FOURPLAY_INPUT",
        config.input_port_start,
        config.input_port_count,
    )?;
    Ok(config)
}

fn optional_u16(name: &str) -> Result<Option<u16>, Box<dyn Error>> {
    env::var(name)
        .ok()
        .map(|value| {
            value
                .parse::<u16>()
                .map_err(|error| format!("{name} must be a UDP port/count value: {error}").into())
        })
        .transpose()
}

fn validate_port_pool(label: &str, start: u16, count: u16) -> Result<(), Box<dyn Error>> {
    if count == 0 {
        return Err(format!("{label}_PORT_COUNT must be greater than zero").into());
    }
    if u32::from(start) + u32::from(count) > 65_536 {
        return Err(format!("{label}_PORT_START + {label}_PORT_COUNT exceeds 65536").into());
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
