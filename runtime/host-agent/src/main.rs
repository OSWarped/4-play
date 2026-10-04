use std::{env, error::Error};

use runtime_host_agent::{
    AgentConfig, RuntimeHostAgent, catalog::discover_catalog, configured_mame_path,
    discover_capabilities, runtime::RuntimeAdapterConfig,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = AgentConfig::from_environment().map_err(std::io::Error::other)?;
    let capabilities = discover_capabilities();
    let mame_path = configured_mame_path()
        .ok_or_else(|| std::io::Error::other("MAME is required for catalog discovery"))?;
    let catalog_path = env::var("FOURPLAY_CATALOG_PATH")
        .unwrap_or_else(|_| "catalog/test-catalog.json".to_owned());
    let mame_ini_path = env::var("FOURPLAY_MAME_INI_PATH")
        .ok()
        .or_else(|| cfg!(unix).then(|| "/opt/4play/config/mame".to_owned()));
    let catalog = discover_catalog(&catalog_path, &mame_path, mame_ini_path.as_deref())
        .map_err(std::io::Error::other)?;
    let runtime_adapter = RuntimeAdapterConfig::from_environment(
        &mame_path,
        mame_ini_path.as_deref().unwrap_or("/opt/4play/config/mame"),
    );
    let agent = RuntimeHostAgent::new(config, capabilities)
        .with_catalog(catalog)
        .with_runtime_adapter(runtime_adapter);

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
