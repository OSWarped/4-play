use std::{
    env, fs, io,
    path::PathBuf,
    process::{Child, Command, Stdio},
};

use control_protocol::MediaTransport;

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:18080";
const DEFAULT_ANSWER_FILE: &str = "/tmp/4play-webrtc-answer.sdp";
const DEFAULT_SESSION_BIND_ADDRESS: &str = "127.0.0.1";
const DEFAULT_SESSION_PORT_START: u16 = 43_000;
const DEFAULT_SESSION_PORT_COUNT: u16 = 100;
const DEFAULT_INPUT_PORT_START: u16 = 42_000;
const SESSION_PUBLISHER_GST_ELEMENTS: &[&str] = &[
    "webrtcbin",
    "appsrc",
    "videoconvert",
    "audioconvert",
    "audioresample",
    "vp8enc",
    "rtpvp8pay",
    "opusenc",
    "rtpopuspay",
];

#[derive(Debug, Clone)]
pub struct WebRtcTestPublisherConfig {
    pub enabled: bool,
    pub python_path: PathBuf,
    pub script_path: PathBuf,
    pub bind_address: String,
    pub answer_file: PathBuf,
    pub browser_host_ip: Option<String>,
}

impl WebRtcTestPublisherConfig {
    pub fn from_environment() -> Self {
        Self {
            enabled: env_flag("FOURPLAY_WEBRTC_TEST_PATTERN_SUPERVISED"),
            python_path: env::var_os("FOURPLAY_PYTHON_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/usr/bin/python3")),
            script_path: env::var_os("FOURPLAY_WEBRTC_TEST_PATTERN_SCRIPT")
                .map(PathBuf::from)
                .unwrap_or_else(default_script_path),
            bind_address: env::var("FOURPLAY_WEBRTC_TEST_PATTERN_BIND")
                .unwrap_or_else(|_| DEFAULT_BIND_ADDRESS.to_owned()),
            answer_file: env::var_os("FOURPLAY_WEBRTC_TEST_PATTERN_ANSWER_FILE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_ANSWER_FILE)),
            browser_host_ip: env::var("FOURPLAY_WEBRTC_BROWSER_HOST_IP")
                .ok()
                .filter(|value| !value.trim().is_empty()),
        }
    }

    pub fn publisher_url(&self) -> String {
        format!("http://{}/offer", self.bind_address)
    }
}

#[derive(Debug, Clone)]
pub struct WebRtcSessionPublisherConfig {
    pub enabled: bool,
    pub python_path: PathBuf,
    pub script_path: PathBuf,
    pub bind_address: String,
    pub port_start: u16,
    pub port_count: u16,
    pub input_port_start: u16,
    pub answer_directory: PathBuf,
    pub browser_host_ip: Option<String>,
}

impl WebRtcSessionPublisherConfig {
    pub fn from_environment() -> Self {
        Self {
            enabled: env_flag("FOURPLAY_WEBRTC_SESSION_PUBLISHERS"),
            python_path: env::var_os("FOURPLAY_PYTHON_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/usr/bin/python3")),
            script_path: env::var_os("FOURPLAY_WEBRTC_SESSION_PUBLISHER_SCRIPT")
                .map(PathBuf::from)
                .unwrap_or_else(default_raw_session_script_path),
            bind_address: env::var("FOURPLAY_WEBRTC_SESSION_BIND_ADDRESS")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_SESSION_BIND_ADDRESS.to_owned()),
            port_start: env_u16(
                "FOURPLAY_WEBRTC_SESSION_PORT_START",
                DEFAULT_SESSION_PORT_START,
            ),
            port_count: env_u16(
                "FOURPLAY_WEBRTC_SESSION_PORT_COUNT",
                DEFAULT_SESSION_PORT_COUNT,
            )
            .max(1),
            input_port_start: env_u16("FOURPLAY_INPUT_PORT_START", DEFAULT_INPUT_PORT_START),
            answer_directory: env::var_os("FOURPLAY_WEBRTC_SESSION_ANSWER_DIRECTORY")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/tmp/4play/webrtc-answers")),
            browser_host_ip: env::var("FOURPLAY_WEBRTC_BROWSER_HOST_IP")
                .ok()
                .filter(|value| !value.trim().is_empty()),
        }
    }

    pub fn prerequisites_present(&self) -> bool {
        gstreamer_webrtc_prerequisites_present()
    }

    pub fn can_advertise(&self) -> bool {
        self.enabled && self.prerequisites_present() && self.script_path.is_file()
    }

    pub fn signal_port_for_input(&self, input_port: u16) -> Option<u16> {
        let offset = input_port.checked_sub(self.input_port_start)?;
        if offset >= self.port_count {
            return None;
        }
        self.port_start.checked_add(offset)
    }

    pub fn bind_address_for_input(&self, input_port: u16) -> Option<String> {
        self.signal_port_for_input(input_port)
            .map(|port| format!("{}:{port}", self.bind_address))
    }

    pub fn control_plane_template(&self) -> String {
        format!(
            "http://{}:{{webrtc_signal_port}}/sessions/{{session_id}}/offer",
            self.bind_address
        )
    }
}

pub struct WebRtcTestPublisherSupervisor {
    config: WebRtcTestPublisherConfig,
    child: Option<Child>,
}

impl WebRtcTestPublisherSupervisor {
    pub fn new(config: WebRtcTestPublisherConfig) -> Self {
        Self {
            config,
            child: None,
        }
    }

    pub fn ensure_started(&mut self) -> io::Result<()> {
        if !self.config.enabled {
            return Ok(());
        }
        if let Some(child) = self.child.as_mut() {
            if child.try_wait()?.is_none() {
                return Ok(());
            }
            self.child = None;
        }
        if !self.config.script_path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "WebRTC test-pattern publisher script was not found: {}",
                    self.config.script_path.display()
                ),
            ));
        }
        if let Some(parent) = self.config.answer_file.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let mut command = Command::new(&self.config.python_path);
        command
            .arg(&self.config.script_path)
            .arg("--http-signal")
            .arg(&self.config.bind_address)
            .arg("--answer-file")
            .arg(&self.config.answer_file)
            .stdin(Stdio::null());
        if let Some(browser_host_ip) = &self.config.browser_host_ip {
            command.env("FOURPLAY_WEBRTC_BROWSER_HOST_IP", browser_host_ip);
        }
        let child = command.spawn()?;
        println!(
            "Started WebRTC test-pattern publisher pid={} url={}",
            child.id(),
            self.config.publisher_url()
        );
        self.child = Some(child);
        Ok(())
    }

    pub fn stop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for WebRtcTestPublisherSupervisor {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn discover_media_transports() -> Vec<MediaTransport> {
    let session_publishers = WebRtcSessionPublisherConfig::from_environment();
    media_transports_for(
        session_publishers.enabled,
        session_publishers.can_advertise(),
    )
}

fn media_transports_for(
    session_publishers_enabled: bool,
    prerequisites_present: bool,
) -> Vec<MediaTransport> {
    let mut transports = vec![MediaTransport::UdpMpegTs];
    if session_publishers_enabled && prerequisites_present {
        transports.push(MediaTransport::WebRtc);
    }
    transports
}

fn gstreamer_webrtc_prerequisites_present() -> bool {
    SESSION_PUBLISHER_GST_ELEMENTS.iter().all(|element| {
        Command::new("gst-inspect-1.0")
            .arg(element)
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

fn default_script_path() -> PathBuf {
    let from_current_exe = env::current_exe()
        .ok()
        .and_then(|path| find_script_from_ancestor(&path, "webrtc-gst-test-pattern.py"));
    from_current_exe
        .unwrap_or_else(|| PathBuf::from("/home/blake/src/4-play/tools/webrtc-gst-test-pattern.py"))
}

fn default_raw_session_script_path() -> PathBuf {
    let from_current_exe = env::current_exe()
        .ok()
        .and_then(|path| find_script_from_ancestor(&path, "webrtc-gst-raw-session-publisher.py"));
    from_current_exe.unwrap_or_else(|| {
        PathBuf::from("/home/blake/src/4-play/tools/webrtc-gst-raw-session-publisher.py")
    })
}

fn find_script_from_ancestor(start: &std::path::Path, script_name: &str) -> Option<PathBuf> {
    start.ancestors().find_map(|ancestor| {
        let candidate = ancestor.join("tools").join(script_name);
        candidate.is_file().then_some(candidate)
    })
}

fn env_flag(name: &str) -> bool {
    env_flag_value(env::var(name).ok().as_deref())
}

fn env_u16(name: &str, default: u16) -> u16 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(default)
}

fn env_flag_value(value: Option<&str>) -> bool {
    value
        .map(|value| matches!(value, "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use control_protocol::MediaTransport;

    use super::{
        SESSION_PUBLISHER_GST_ELEMENTS, WebRtcSessionPublisherConfig, env_flag_value,
        find_script_from_ancestor, media_transports_for,
    };

    #[test]
    fn env_flag_accepts_common_truthy_values() {
        for value in [
            Some("1"),
            Some("true"),
            Some("TRUE"),
            Some("yes"),
            Some("YES"),
            Some("on"),
            Some("ON"),
        ] {
            assert!(env_flag_value(value));
        }
    }

    #[test]
    fn env_flag_rejects_missing_or_false_values() {
        for value in [None, Some("0"), Some("false"), Some("off"), Some("")] {
            assert!(!env_flag_value(value));
        }
    }

    #[test]
    fn web_rtc_transport_is_advertised_only_for_session_publishers() {
        assert_eq!(
            media_transports_for(false, true),
            vec![MediaTransport::UdpMpegTs]
        );
        assert_eq!(
            media_transports_for(true, false),
            vec![MediaTransport::UdpMpegTs]
        );
        assert_eq!(
            media_transports_for(true, true),
            vec![MediaTransport::UdpMpegTs, MediaTransport::WebRtc]
        );
    }

    #[test]
    fn session_publisher_prerequisites_cover_raw_media_pipeline() {
        for required in [
            "webrtcbin",
            "appsrc",
            "vp8enc",
            "rtpvp8pay",
            "opusenc",
            "rtpopuspay",
        ] {
            assert!(
                SESSION_PUBLISHER_GST_ELEMENTS.contains(&required),
                "missing required GStreamer element {required}"
            );
        }
    }

    #[test]
    fn finds_script_from_repo_ancestor() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("4play-webrtc-test-{unique}"));
        let nested = root.join("target/release");
        let tools = root.join("tools");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&tools).unwrap();
        fs::write(
            tools.join("webrtc-gst-test-pattern.py"),
            "#!/usr/bin/env python3\n",
        )
        .unwrap();

        let found = find_script_from_ancestor(
            &nested.join("runtime-host-agent"),
            "webrtc-gst-test-pattern.py",
        )
        .unwrap();
        assert_eq!(found, tools.join("webrtc-gst-test-pattern.py"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn session_publisher_derives_ports_from_input_ports() {
        let config = WebRtcSessionPublisherConfig {
            enabled: true,
            python_path: PathBuf::from("python3"),
            script_path: PathBuf::from("publisher.py"),
            bind_address: "127.0.0.1".to_owned(),
            port_start: 43_000,
            port_count: 10,
            input_port_start: 42_000,
            answer_directory: PathBuf::from("/tmp/answers"),
            browser_host_ip: None,
        };

        assert_eq!(config.signal_port_for_input(42_000), Some(43_000));
        assert_eq!(config.signal_port_for_input(42_003), Some(43_003));
        assert_eq!(config.signal_port_for_input(42_010), None);
        assert_eq!(
            config.control_plane_template(),
            "http://127.0.0.1:{webrtc_signal_port}/sessions/{session_id}/offer"
        );
    }
}
