use std::{env, error::Error, net::SocketAddr, path::PathBuf, time::Duration};

use control_plane_server::PortPoolConfig;
use tokio::net::TcpListener;

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:8080";
const BIND_ENVIRONMENT_VARIABLE: &str = "FOURPLAY_CONTROL_PLANE_BIND";
const DEFAULT_DATABASE_PATH: &str = "data/control-plane.sqlite3";
const DEFAULT_OFFLINE_SECONDS: u64 = 15;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
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
    let listener = TcpListener::bind(bind_address).await?;
    let app = control_plane_server::app_with_database_tokens_and_ports(
        &database_path,
        Duration::from_secs(offline_seconds),
        seat_api_token,
        runtime_host_api_token,
        port_pools,
    )
    .await?;

    println!(
        "4-Play control plane listening on http://{bind_address} database={} host_offline_seconds={offline_seconds} media_ports={}:{} input_ports={}:{}",
        database_path.display(),
        port_pools.media_port_start,
        port_pools.media_port_count,
        port_pools.input_port_start,
        port_pools.input_port_count
    );

    axum::serve(listener, app)
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
