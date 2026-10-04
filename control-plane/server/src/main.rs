use std::{env, error::Error, net::SocketAddr, path::PathBuf, time::Duration};

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
    let listener = TcpListener::bind(bind_address).await?;
    let app = control_plane_server::app_with_database(
        &database_path,
        Duration::from_secs(offline_seconds),
    )
    .await?;

    println!(
        "4-Play control plane listening on http://{bind_address} database={} host_offline_seconds={offline_seconds}",
        database_path.display()
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

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
