use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
};

use control_protocol::RuntimeSessionAssignment;

#[derive(Debug, Clone)]
pub struct RuntimeAdapterConfig {
    pub session_runtime_path: PathBuf,
    pub mame_path: PathBuf,
    pub mame_ini_path: PathBuf,
    pub state_directory: PathBuf,
    pub preview_directory: PathBuf,
    pub preview_interval_ms: u64,
}

impl RuntimeAdapterConfig {
    pub fn from_environment(
        mame_path: impl Into<PathBuf>,
        mame_ini_path: impl Into<PathBuf>,
    ) -> Self {
        let session_runtime_path = std::env::var_os("FOURPLAY_SESSION_RUNTIME_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(default_session_runtime_path);
        let state_directory = std::env::var_os("FOURPLAY_RUNTIME_STATE_DIRECTORY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp/4play/host-agent"));
        let preview_directory = std::env::var_os("FOURPLAY_PREVIEW_DIRECTORY")
            .map(PathBuf::from)
            .unwrap_or_else(|| state_directory.join("previews"));
        let preview_interval_ms = std::env::var("FOURPLAY_PREVIEW_INTERVAL_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1_000);
        Self {
            session_runtime_path,
            mame_path: mame_path.into(),
            mame_ini_path: mame_ini_path.into(),
            state_directory,
            preview_directory,
            preview_interval_ms,
        }
    }
}

fn default_session_runtime_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("session-runtime")))
        .unwrap_or_else(|| PathBuf::from("session-runtime"))
}

pub struct RuntimeSupervisor {
    config: RuntimeAdapterConfig,
    processes: HashMap<String, ManagedRuntime>,
}

struct ManagedRuntime {
    child: Option<Child>,
    pid: u32,
    status_file: PathBuf,
    pid_file: PathBuf,
    spectator_ports_file: PathBuf,
    preview_image_file: PathBuf,
}

#[derive(Debug)]
pub enum RuntimeObservation {
    Starting,
    Active,
    Exited(String),
    Missing,
}

impl RuntimeSupervisor {
    pub fn new(config: RuntimeAdapterConfig) -> Self {
        Self {
            config,
            processes: HashMap::new(),
        }
    }

    pub fn active_count(&self) -> u32 {
        self.processes.len().try_into().unwrap_or(u32::MAX)
    }

    pub fn ensure_started(&mut self, assignment: &RuntimeSessionAssignment) -> io::Result<()> {
        if self.processes.contains_key(&assignment.session_id) {
            return Ok(());
        }
        fs::create_dir_all(&self.config.state_directory)?;
        let status_file = self.status_file(&assignment.session_id);
        let pid_file = self.pid_file(&assignment.session_id);
        let spectator_ports_file = self.spectator_ports_file(&assignment.session_id);
        let preview_image_file = self.preview_image_file(&assignment.session_id);
        write_spectator_ports(&spectator_ports_file, &assignment.spectator_media_ports)?;
        if let Some(pid) = read_live_pid(&pid_file, &assignment.session_id)? {
            self.processes.insert(
                assignment.session_id.clone(),
                ManagedRuntime {
                    child: None,
                    pid,
                    status_file,
                    pid_file,
                    spectator_ports_file,
                    preview_image_file,
                },
            );
            return Ok(());
        }
        remove_if_present(&status_file)?;
        remove_if_present(&pid_file)?;
        remove_if_present(&preview_image_file)?;
        let log_path = self
            .config
            .state_directory
            .join(format!("{}.log", assignment.session_id));
        let stdout = File::create(&log_path)?;
        let stderr = stdout.try_clone()?;
        let mut command = Command::new(&self.config.session_runtime_path);
        command
            .arg("--session-id")
            .arg(&assignment.session_id)
            .arg("--rom")
            .arg(&assignment.rom_name)
            .arg("--width")
            .arg(assignment.runtime_profile.width.to_string())
            .arg("--height")
            .arg(assignment.runtime_profile.height.to_string())
            .arg("--fps")
            .arg(assignment.runtime_profile.refresh_hz.to_string())
            .arg("--players")
            .arg(assignment.runtime_profile.max_players.to_string())
            .arg("--destination-ip")
            .arg(&assignment.destination_address)
            .arg("--udp-port")
            .arg(assignment.media_udp_port.to_string());
        for spectator_port in &assignment.spectator_media_ports {
            command
                .arg("--spectator-udp-port")
                .arg(spectator_port.to_string());
        }
        command
            .arg("--spectator-ports-file")
            .arg(&spectator_ports_file)
            .arg("--input-port")
            .arg(assignment.input_udp_port.to_string())
            .arg("--input-token")
            .arg(&assignment.input_token)
            .arg("--mame-path")
            .arg(&self.config.mame_path)
            .arg("--mame-ini-path")
            .arg(&self.config.mame_ini_path)
            .arg("--status-file")
            .arg(&status_file)
            .arg("--preview-image-path")
            .arg(&preview_image_file)
            .arg("--preview-interval-ms")
            .arg(self.config.preview_interval_ms.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        if assignment.runtime_profile.supports_save_state {
            command.arg("--autosave");
        }
        if input_debug_enabled() {
            command.arg("--debug-input");
        }
        let child = command.spawn()?;
        let pid = child.id();
        fs::write(&pid_file, format!("{pid}\n"))?;
        self.processes.insert(
            assignment.session_id.clone(),
            ManagedRuntime {
                child: Some(child),
                pid,
                status_file,
                pid_file,
                spectator_ports_file,
                preview_image_file,
            },
        );
        Ok(())
    }

    pub fn refresh_spectator_ports(
        &mut self,
        assignment: &RuntimeSessionAssignment,
    ) -> io::Result<()> {
        if let Some(process) = self.processes.get(&assignment.session_id) {
            write_spectator_ports(
                &process.spectator_ports_file,
                &assignment.spectator_media_ports,
            )?;
        }
        Ok(())
    }

    pub fn observe(&mut self, session_id: &str) -> io::Result<RuntimeObservation> {
        let Some(process) = self.processes.get_mut(session_id) else {
            return Ok(RuntimeObservation::Missing);
        };
        let exit = if let Some(child) = process.child.as_mut() {
            child.try_wait()?
        } else if process_matches_session(process.pid, session_id) {
            None
        } else {
            Some(synthetic_exit_status())
        };
        if let Some(status) = exit {
            let description = format_exit_status(status);
            cleanup_process_files(process);
            self.processes.remove(session_id);
            return Ok(RuntimeObservation::Exited(description));
        }
        let status = fs::read_to_string(&process.status_file).unwrap_or_default();
        if status.trim() == "active" {
            Ok(RuntimeObservation::Active)
        } else {
            Ok(RuntimeObservation::Starting)
        }
    }

    pub fn request_stop(&mut self, session_id: &str) -> io::Result<()> {
        let Some(process) = self.processes.get(session_id) else {
            return Ok(());
        };
        if process.child.is_some() || process_matches_session(process.pid, session_id) {
            signal_terminate(process.pid)
        } else {
            Ok(())
        }
    }

    pub fn forget(&mut self, session_id: &str) {
        if let Some(mut process) = self.processes.remove(session_id) {
            if let Some(child) = process.child.as_mut() {
                let _ = child.try_wait();
            }
            cleanup_process_files(&process);
        }
    }

    pub fn stop_unassigned(&mut self, assigned: &HashSet<String>) {
        let stale = self
            .processes
            .keys()
            .filter(|id| !assigned.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        for session_id in stale {
            let _ = self.request_stop(&session_id);
            self.forget(&session_id);
        }
    }

    pub fn stop_all(&mut self) {
        let sessions = self.processes.keys().cloned().collect::<Vec<_>>();
        for session_id in sessions {
            let _ = self.request_stop(&session_id);
        }
    }

    fn status_file(&self, session_id: &str) -> PathBuf {
        self.config
            .state_directory
            .join(format!("{session_id}.status"))
    }

    fn pid_file(&self, session_id: &str) -> PathBuf {
        self.config
            .state_directory
            .join(format!("{session_id}.pid"))
    }

    fn spectator_ports_file(&self, session_id: &str) -> PathBuf {
        self.config
            .state_directory
            .join(format!("{session_id}.spectator-ports"))
    }

    fn preview_image_file(&self, session_id: &str) -> PathBuf {
        self.config
            .preview_directory
            .join(format!("{session_id}.bmp"))
    }
}

fn input_debug_enabled() -> bool {
    input_debug_value_enabled(std::env::var("FOURPLAY_INPUT_DEBUG").ok().as_deref())
}

fn input_debug_value_enabled(value: Option<&str>) -> bool {
    value
        .map(|value| matches!(value, "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"))
        .unwrap_or(false)
}

fn read_live_pid(path: &Path, session_id: &str) -> io::Result<Option<u32>> {
    let Some(contents) = fs::read_to_string(path).ok() else {
        return Ok(None);
    };
    let pid = contents
        .trim()
        .parse::<u32>()
        .map_err(|_| io::Error::other(format!("invalid runtime PID file: {}", path.display())))?;
    Ok(process_matches_session(pid, session_id).then_some(pid))
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn cleanup_process_files(process: &ManagedRuntime) {
    let _ = remove_if_present(&process.pid_file);
    let _ = remove_if_present(&process.status_file);
    let _ = remove_if_present(&process.spectator_ports_file);
    let _ = remove_if_present(&process.preview_image_file);
}

fn write_spectator_ports(path: &Path, ports: &[u16]) -> io::Result<()> {
    let body = ports
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        path,
        if body.is_empty() {
            body
        } else {
            format!("{body}\n")
        },
    )
}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(unix)]
fn process_matches_session(pid: u32, session_id: &str) -> bool {
    if !process_exists(pid) {
        return false;
    }
    let Ok(command_line) = fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let arguments = command_line
        .split(|byte| *byte == 0)
        .filter(|argument| !argument.is_empty())
        .collect::<Vec<_>>();
    arguments
        .windows(2)
        .any(|window| window[0] == b"--session-id" && window[1] == session_id.as_bytes())
}

#[cfg(not(unix))]
fn process_matches_session(_pid: u32, _session_id: &str) -> bool {
    false
}

#[cfg(unix)]
fn signal_terminate(pid: u32) -> io::Result<()> {
    let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    if result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn signal_terminate(_pid: u32) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "runtime process signaling is currently supported only on Unix hosts",
    ))
}

#[cfg(unix)]
fn synthetic_exit_status() -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(1 << 8)
}

#[cfg(windows)]
fn synthetic_exit_status() -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(1)
}

fn format_exit_status(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("session runtime exited with code {code}"),
        None => format!("session runtime exited: {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::input_debug_value_enabled;

    #[test]
    fn input_debug_accepts_common_truthy_values() {
        for value in [
            Some("1"),
            Some("true"),
            Some("TRUE"),
            Some("yes"),
            Some("YES"),
            Some("on"),
            Some("ON"),
        ] {
            assert!(input_debug_value_enabled(value));
        }
    }

    #[test]
    fn input_debug_rejects_missing_or_false_values() {
        for value in [None, Some("0"), Some("false"), Some("off"), Some("")] {
            assert!(!input_debug_value_enabled(value));
        }
    }
}
