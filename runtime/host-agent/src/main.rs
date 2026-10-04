use std::error::Error;

use runtime_host_agent::{AgentConfig, RuntimeHostAgent, discover_capabilities};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = AgentConfig::from_environment().map_err(std::io::Error::other)?;
    let capabilities = discover_capabilities();
    let agent = RuntimeHostAgent::new(config, capabilities);

    println!(
        "4-Play runtime host agent starting: id={} control_plane={}",
        agent.config().host_id,
        agent.config().control_plane_url
    );
    println!(
        "Capabilities: os={} arch={} cpus={} memory_bytes={} encoders={:?} adapters={:?}",
        agent.registration().capabilities.operating_system,
        agent.registration().capabilities.architecture,
        agent.registration().capabilities.logical_cpu_count,
        agent.registration().capabilities.memory_bytes,
        agent.registration().capabilities.encoder_names,
        agent.registration().capabilities.emulator_adapters,
    );

    agent.run_until(shutdown_signal()).await;
    println!("Runtime host agent stopped");
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
