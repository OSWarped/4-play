use std::{env, error::Error, net::SocketAddr};

use tokio::net::TcpListener;

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:8080";
const BIND_ENVIRONMENT_VARIABLE: &str = "FOURPLAY_CONTROL_PLANE_BIND";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let bind_address = env::var(BIND_ENVIRONMENT_VARIABLE)
        .unwrap_or_else(|_| DEFAULT_BIND_ADDRESS.to_owned())
        .parse::<SocketAddr>()?;
    let listener = TcpListener::bind(bind_address).await?;

    println!("4-Play control plane listening on http://{bind_address}");

    axum::serve(listener, control_plane_server::app())
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
