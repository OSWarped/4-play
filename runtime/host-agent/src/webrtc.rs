use std::{
    env, fs, io,
    path::PathBuf,
    process::{Child, Command, Stdio},
};

use control_protocol::MediaTransport;

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:18080";
const DEFAULT_ANSWER_FILE: &str = "/tmp/4play-webrtc-answer.sdp";

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
    let mut transports = vec![MediaTransport::UdpMpegTs];
    if env_flag("FOURPLAY_WEBRTC_EXPERIMENTAL") && gstreamer_webrtc_prerequisites_present() {
        transports.push(MediaTransport::WebRtc);
    }
    transports
}

fn gstreamer_webrtc_prerequisites_present() -> bool {
    ["webrtcbin", "vp8enc", "rtpopuspay"].iter().all(|element| {
        Command::new("gst-inspect-1.0")
            .arg(element)
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

fn default_script_path() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent()
                .map(|parent| parent.join("../tools/webrtc-gst-test-pattern.py"))
        })
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("/home/blake/src/4-play/tools/webrtc-gst-test-pattern.py"))
}

fn env_flag(name: &str) -> bool {
    env_flag_value(env::var(name).ok().as_deref())
}

fn env_flag_value(value: Option<&str>) -> bool {
    value
        .map(|value| matches!(value, "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::env_flag_value;

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
}
