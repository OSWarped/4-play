use std::fmt;
use std::fs;
use std::io;
use std::os::unix::fs::FileTypeExt;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Created,
    Preparing,
    Prepared,
    LaunchingEncoder,
    LaunchingEmulator,
    Running,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub id: u32,
    pub rom: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub destination_ip: String,
    pub udp_port: u16,
}

#[derive(Debug, Default)]
pub struct SessionMetrics {
    pub video_frames_received: u64,
    pub audio_samples_received: u64,
    pub video_frames_dropped: u64,
    pub video_queue_depth: usize,
    pub audio_queue_depth_ms: u64,
}

#[derive(Debug)]
pub struct Session {
    pub config: SessionConfig,
    pub state: SessionState,
    pub working_directory: PathBuf,
    pub metrics: SessionMetrics,
}

impl Session {
    pub fn new(config: SessionConfig) -> Self {
        let working_directory = PathBuf::from(format!("/tmp/4play/session-{}", config.id));

        Self {
            config,
            state: SessionState::Created,
            working_directory,
            metrics: SessionMetrics::default(),
        }
    }

    pub fn prepare(&mut self) -> io::Result<()> {
        self.state = SessionState::Preparing;

        fs::create_dir_all(self.working_directory.join("cfg"))?;
        fs::create_dir_all(self.working_directory.join("nvram"))?;
        fs::create_dir_all(self.working_directory.join("state"))?;
        fs::create_dir_all(self.working_directory.join("snap"))?;
        fs::create_dir_all(self.working_directory.join("diff"))?;

        self.state = SessionState::Prepared;

        Ok(())
    }

    pub fn video_path(&self) -> PathBuf {
        self.working_directory.join("video.raw")
    }

    pub fn audio_path(&self) -> PathBuf {
        self.working_directory.join("audio.pcm")
    }

    pub fn create_media_endpoints(&self) -> io::Result<()> {
        let video_path = self.video_path();
        let audio_path = self.audio_path();

        for path in [&video_path, &audio_path] {
            if path.exists() {
                fs::remove_file(path)?;
            }

            let status = Command::new("mkfifo").arg(path).status()?;

            if !status.success() {
                return Err(io::Error::other(format!(
                    "mkfifo failed for {}",
                    path.display()
                )));
            }

            let metadata = fs::metadata(path)?;
            if !metadata.file_type().is_fifo() {
                return Err(io::Error::other(format!(
                    "{} was created but is not a FIFO",
                    path.display()
                )));
            }
        }

        Ok(())
    }
}

impl fmt::Display for Session {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "4-Play Session Runtime")?;
        writeln!(formatter)?;
        writeln!(formatter, "Session ID:   {}", self.config.id)?;
        writeln!(formatter, "ROM:          {}", self.config.rom)?;
        writeln!(
            formatter,
            "Video:        {}x{} @ {:.4} Hz",
            self.config.width, self.config.height, self.config.fps
        )?;
        writeln!(formatter, "Audio:        48000 Hz stereo S16LE")?;
        writeln!(
            formatter,
            "Destination:  {}:{}",
            self.config.destination_ip, self.config.udp_port
        )?;
        writeln!(
            formatter,
            "Working dir:  {}",
            self.working_directory.display()
        )?;
        writeln!(formatter, "Video path:   {}", self.video_path().display())?;
        writeln!(formatter, "Audio path:   {}", self.audio_path().display())?;
        writeln!(formatter, "State:        {:?}", self.state)?;
        writeln!(
            formatter,
            "Metrics:      video={} audio={} dropped={} vq={} aq={}ms",
            self.metrics.video_frames_received,
            self.metrics.audio_samples_received,
            self.metrics.video_frames_dropped,
            self.metrics.video_queue_depth,
            self.metrics.audio_queue_depth_ms
        )
    }
}
