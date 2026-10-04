use std::{env, fs, future::Future, path::Path, process::Command, time::Duration};

use control_protocol::{
    RegisterRuntimeHost, RuntimeHost, RuntimeHostCapabilities, RuntimeHostCatalog,
    RuntimeHostHeartbeat,
};
use reqwest::Client;
use tokio::time::{MissedTickBehavior, interval};

pub mod catalog;

pub const DEFAULT_CONTROL_PLANE_URL: &str = "http://127.0.0.1:8080";
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub control_plane_url: String,
    pub host_id: String,
    pub display_name: String,
    pub heartbeat_interval: Duration,
}

impl AgentConfig {
    pub fn from_environment() -> Result<Self, String> {
        let default_host_name = host_name().unwrap_or_else(|| "runtime-host".to_owned());
        let control_plane_url = env::var("FOURPLAY_CONTROL_PLANE_URL")
            .unwrap_or_else(|_| DEFAULT_CONTROL_PLANE_URL.to_owned());
        let host_id = env::var("FOURPLAY_RUNTIME_HOST_ID")
            .unwrap_or_else(|_| default_host_name.to_ascii_lowercase());
        let display_name =
            env::var("FOURPLAY_RUNTIME_HOST_NAME").unwrap_or_else(|_| default_host_name.clone());
        let heartbeat_interval = match env::var("FOURPLAY_HEARTBEAT_SECONDS") {
            Ok(value) => {
                let seconds = value
                    .parse::<u64>()
                    .map_err(|_| "FOURPLAY_HEARTBEAT_SECONDS must be an integer".to_owned())?;
                if seconds == 0 {
                    return Err("FOURPLAY_HEARTBEAT_SECONDS must be greater than zero".to_owned());
                }
                Duration::from_secs(seconds)
            }
            Err(_) => DEFAULT_HEARTBEAT_INTERVAL,
        };

        validate_host_id(&host_id)?;
        if display_name.trim().is_empty() {
            return Err("FOURPLAY_RUNTIME_HOST_NAME must not be empty".to_owned());
        }

        Ok(Self {
            control_plane_url: control_plane_url.trim_end_matches('/').to_owned(),
            host_id,
            display_name,
            heartbeat_interval,
        })
    }
}

#[derive(Clone)]
pub struct RuntimeHostAgent {
    client: Client,
    config: AgentConfig,
    registration: RegisterRuntimeHost,
    catalog: Option<RuntimeHostCatalog>,
}

impl RuntimeHostAgent {
    pub fn new(config: AgentConfig, capabilities: RuntimeHostCapabilities) -> Self {
        let registration = RegisterRuntimeHost {
            display_name: config.display_name.clone(),
            agent_version: env!("CARGO_PKG_VERSION").to_owned(),
            capabilities,
        };
        Self {
            client: Client::new(),
            config,
            registration,
            catalog: None,
        }
    }

    pub fn with_catalog(mut self, catalog: RuntimeHostCatalog) -> Self {
        self.catalog = Some(catalog);
        self
    }

    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    pub fn registration(&self) -> &RegisterRuntimeHost {
        &self.registration
    }

    pub async fn register(&self) -> Result<RuntimeHost, reqwest::Error> {
        self.client
            .put(self.runtime_host_url())
            .json(&self.registration)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    pub async fn heartbeat(
        &self,
        sequence: u64,
        active_session_count: u32,
    ) -> Result<RuntimeHost, reqwest::Error> {
        self.client
            .post(format!("{}/heartbeat", self.runtime_host_url()))
            .json(&RuntimeHostHeartbeat {
                sequence,
                active_session_count,
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    pub async fn publish_catalog(&self) -> Result<Option<RuntimeHostCatalog>, reqwest::Error> {
        let Some(catalog) = &self.catalog else {
            return Ok(None);
        };
        self.client
            .put(format!("{}/catalog", self.runtime_host_url()))
            .json(catalog)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map(Some)
    }

    pub async fn run_until<F>(&self, shutdown: F)
    where
        F: Future<Output = ()>,
    {
        tokio::pin!(shutdown);
        let mut timer = interval(self.config.heartbeat_interval);
        timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut next_sequence = None;

        loop {
            tokio::select! {
                () = &mut shutdown => break,
                _ = timer.tick() => {
                    if let Some(sequence) = next_sequence {
                        match self.heartbeat(sequence, 0).await {
                            Ok(host) => {
                                println!(
                                    "Heartbeat accepted: host={} sequence={} active_sessions={}",
                                    host.id, host.heartbeat_sequence, host.active_session_count
                                );
                                next_sequence = Some(sequence.saturating_add(1));
                            }
                            Err(error) => {
                                eprintln!("Heartbeat failed: {error}; registration will be refreshed");
                                next_sequence = None;
                            }
                        }
                    } else {
                        match self.register().await {
                            Ok(host) => {
                                println!(
                                    "Runtime host registered: id={} control_plane={}",
                                    host.id, self.config.control_plane_url
                                );
                                match self.publish_catalog().await {
                                    Ok(Some(catalog)) => println!(
                                        "Runtime catalog published: host={} games={}",
                                        host.id,
                                        catalog.games.len()
                                    ),
                                    Ok(None) => {}
                                    Err(error) => {
                                        eprintln!("Runtime catalog publication failed: {error}; retrying");
                                        continue;
                                    }
                                }
                                next_sequence = Some(host.heartbeat_sequence.saturating_add(1));
                            }
                            Err(error) => {
                                eprintln!("Runtime host registration failed: {error}; retrying");
                            }
                        }
                    }
                }
            }
        }
    }

    fn runtime_host_url(&self) -> String {
        format!(
            "{}/api/v1/runtime-hosts/{}",
            self.config.control_plane_url, self.config.host_id
        )
    }
}

pub fn discover_capabilities() -> RuntimeHostCapabilities {
    RuntimeHostCapabilities {
        operating_system: env::consts::OS.to_owned(),
        architecture: env::consts::ARCH.to_owned(),
        logical_cpu_count: std::thread::available_parallelism()
            .map(|count| count.get().try_into().unwrap_or(u32::MAX))
            .unwrap_or(1),
        memory_bytes: total_memory_bytes().unwrap_or(0),
        encoder_names: discover_h264_encoders(),
        emulator_adapters: discover_emulator_adapters(),
    }
}

fn validate_host_id(host_id: &str) -> Result<(), String> {
    let valid = !host_id.is_empty()
        && host_id.len() <= 64
        && host_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(
            "FOURPLAY_RUNTIME_HOST_ID must contain 1-64 ASCII letters, digits, dots, dashes, or underscores"
                .to_owned(),
        )
    }
}

fn host_name() -> Option<String> {
    env::var("HOSTNAME")
        .or_else(|_| env::var("COMPUTERNAME"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            fs::read_to_string("/etc/hostname")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
}

fn total_memory_bytes() -> Option<u64> {
    let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
    parse_mem_total_bytes(&meminfo)
}

fn parse_mem_total_bytes(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kibibytes = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    kibibytes.checked_mul(1024)
}

fn discover_h264_encoders() -> Vec<String> {
    let Ok(output) = Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    parse_available_encoders(&listing)
}

fn parse_available_encoders(listing: &str) -> Vec<String> {
    const PREFERRED_ENCODERS: [&str; 3] = ["h264_qsv", "h264_vaapi", "libx264"];
    let available = listing
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .collect::<Vec<_>>();
    PREFERRED_ENCODERS
        .iter()
        .filter(|encoder| available.contains(encoder))
        .map(|encoder| (*encoder).to_owned())
        .collect()
}

fn discover_emulator_adapters() -> Vec<String> {
    if configured_mame_path().is_some() {
        vec!["mame".to_owned()]
    } else {
        Vec::new()
    }
}

pub fn configured_mame_path() -> Option<String> {
    let configured_path = env::var("FOURPLAY_MAME_PATH").ok();
    let candidates = configured_path
        .as_deref()
        .into_iter()
        .chain(["/usr/games/mame", "mame"]);

    for candidate in candidates {
        let path_exists = candidate == "mame" || Path::new(candidate).exists();
        if path_exists
            && Command::new(candidate)
                .arg("-version")
                .output()
                .is_ok_and(|output| output.status.success())
        {
            return Some(candidate.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use control_protocol::{
        CatalogGameList, DiscoveredGame, GameRuntimeProfile, RuntimeHostCapabilities,
        RuntimeHostCatalog, RuntimeHostList,
    };
    use tokio::net::TcpListener;

    use super::{AgentConfig, RuntimeHostAgent, parse_available_encoders, parse_mem_total_bytes};

    #[test]
    fn parses_linux_total_memory() {
        assert_eq!(
            parse_mem_total_bytes("MemTotal:       16384000 kB\nMemFree: 1 kB\n"),
            Some(16_777_216_000)
        );
        assert_eq!(parse_mem_total_bytes("MemFree: 1 kB\n"), None);
    }

    #[test]
    fn reports_only_preferred_available_h264_encoders() {
        let listing = "\n V....D h264_vaapi           VAAPI H.264 encoder\n V..... libx264              libx264 H.264\n V..... mpeg4                MPEG-4\n";
        assert_eq!(
            parse_available_encoders(listing),
            vec!["h264_vaapi".to_owned(), "libx264".to_owned()]
        );
    }

    #[tokio::test]
    async fn registers_and_advances_from_the_server_sequence() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, control_plane_server::app().await.unwrap())
                .await
                .unwrap();
        });
        let agent = RuntimeHostAgent::new(
            AgentConfig {
                control_plane_url: format!("http://{address}"),
                host_id: "test-linux".to_owned(),
                display_name: "Test Linux".to_owned(),
                heartbeat_interval: Duration::from_millis(10),
            },
            RuntimeHostCapabilities {
                operating_system: "linux".to_owned(),
                architecture: "x86_64".to_owned(),
                logical_cpu_count: 4,
                memory_bytes: 1024,
                encoder_names: vec!["libx264".to_owned()],
                emulator_adapters: vec!["mame".to_owned()],
            },
        )
        .with_catalog(RuntimeHostCatalog {
            games: vec![DiscoveredGame {
                id: "tmnt".to_owned(),
                display_name: "Teenage Mutant Ninja Turtles".to_owned(),
                rom_name: "tmnt".to_owned(),
                profile: GameRuntimeProfile {
                    width: 320,
                    height: 224,
                    refresh_hz: 60.0,
                    rotation_degrees: 0,
                    max_players: 4,
                    buttons_per_player: 2,
                    supports_save_state: true,
                },
            }],
        });

        let registered = agent.register().await.unwrap();
        assert_eq!(registered.id, "test-linux");
        assert_eq!(
            agent.publish_catalog().await.unwrap().unwrap().games.len(),
            1
        );
        agent.heartbeat(5, 2).await.unwrap();
        let refreshed = agent.register().await.unwrap();
        assert_eq!(refreshed.heartbeat_sequence, 5);
        let heartbeat = agent
            .heartbeat(refreshed.heartbeat_sequence + 1, 1)
            .await
            .unwrap();
        assert_eq!(heartbeat.heartbeat_sequence, 6);

        let hosts = reqwest::get(format!("http://{address}/api/v1/runtime-hosts"))
            .await
            .unwrap()
            .json::<RuntimeHostList>()
            .await
            .unwrap();
        assert_eq!(hosts.hosts.len(), 1);
        assert_eq!(hosts.hosts[0].active_session_count, 1);
        let games = reqwest::get(format!("http://{address}/api/v1/games"))
            .await
            .unwrap()
            .json::<CatalogGameList>()
            .await
            .unwrap();
        assert_eq!(games.games.len(), 1);
        assert_eq!(games.games[0].availability[0].profile.width, 320);

        server.abort();
    }
}
