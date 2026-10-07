use std::fs::File;
use std::io::{self, Read};
use std::net::UdpSocket;
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::str::FromStr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

// Wall-clock timestamps are assigned by FFmpeg in the raw-video demuxer's time
// base. Using the game's frame rate as that time base makes it too coarse for
// an exact-60-Hz source: adjacent frames can land on the same timestamp and be
// dropped. A 90 kHz clock preserves the capture arrival time, after which the
// fps filter emits a stable stream at the game's native frame rate.
const VIDEO_TIMESTAMP_CLOCK_HZ: u32 = 90_000;
const MPEGTS_PACKET_PAYLOAD_SIZE: usize = 1_316;
const SPECTATOR_PORT_RELOAD_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    Aac,
    Opus,
}

impl AudioCodec {
    pub fn name(self) -> &'static str {
        match self {
            Self::Aac => "aac",
            Self::Opus => "opus",
        }
    }
}

impl FromStr for AudioCodec {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "aac" => Ok(Self::Aac),
            "opus" => Ok(Self::Opus),
            _ => Err(format!("expected 'aac' or 'opus', got '{value}'")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub destination_ip: String,
    pub udp_port: u16,
    pub spectator_udp_ports: Vec<u16>,
    pub spectator_ports_file: Option<PathBuf>,
    pub audio_codec: AudioCodec,
    pub audio_thread_queue_size: usize,
}

pub struct EncoderInputs {
    pub video: File,
    pub audio: File,
}

pub struct EncoderProcess {
    child: Option<Child>,
    fanout: Option<MediaFanout>,
}

impl EncoderProcess {
    pub fn spawn(config: &EncoderConfig) -> io::Result<(Self, EncoderInputs)> {
        let (video_read_fd, video_writer) = create_pipe()?;
        let (audio_read_fd, audio_writer) = create_pipe()?;

        let video_size = format!("{}x{}", config.width, config.height);
        let fps = format!("{:.6}", config.fps);
        let mut command = Command::new("ffmpeg");

        command
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("info")
            .arg("-thread_queue_size")
            .arg("64")
            .arg("-probesize")
            .arg("32")
            .arg("-analyzeduration")
            .arg("1")
            .arg("-fpsprobesize")
            .arg("0")
            .arg("-use_wallclock_as_timestamps")
            .arg("1")
            .arg("-f")
            .arg("rawvideo")
            .arg("-pixel_format")
            .arg("bgr0")
            .arg("-video_size")
            .arg(video_size)
            .arg("-framerate")
            .arg(VIDEO_TIMESTAMP_CLOCK_HZ.to_string())
            .arg("-i")
            .arg("pipe:3")
            .arg("-thread_queue_size")
            .arg(config.audio_thread_queue_size.to_string())
            .arg("-probesize")
            .arg("32")
            .arg("-analyzeduration")
            .arg("1")
            .arg("-use_wallclock_as_timestamps")
            .arg("1")
            .arg("-f")
            .arg("s16le")
            .arg("-ar")
            .arg("48000")
            .arg("-ac")
            .arg("2")
            .arg("-i")
            .arg("pipe:4")
            .arg("-map")
            .arg("0:v:0")
            .arg("-map")
            .arg("1:a:0")
            .arg("-filter:v")
            .arg(format!("fps=fps={fps}:start_time=0:round=near"))
            .arg("-fps_mode:v")
            .arg("passthrough")
            .arg("-c:v")
            .arg("libx264")
            .arg("-preset")
            .arg("ultrafast")
            .arg("-tune")
            .arg("zerolatency")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg("-g")
            .arg("15")
            .arg("-keyint_min")
            .arg("15")
            .arg("-sc_threshold")
            .arg("0")
            .arg("-bf")
            .arg("0")
            .arg("-refs")
            .arg("1")
            .arg("-c:a");

        match config.audio_codec {
            AudioCodec::Aac => {
                command.arg("aac").arg("-b:a").arg("128k");
            }
            AudioCodec::Opus => {
                command
                    .arg("libopus")
                    .arg("-b:a")
                    .arg("128k")
                    .arg("-application")
                    .arg("lowdelay")
                    .arg("-frame_duration")
                    .arg("5")
                    .arg("-vbr")
                    .arg("off");
            }
        }

        command
            .arg("-fflags")
            .arg("nobuffer")
            .arg("-flags")
            .arg("low_delay")
            .arg("-flush_packets")
            .arg("1")
            .arg("-muxdelay")
            .arg("0")
            .arg("-muxpreload")
            .arg("0")
            .arg("-f")
            .arg("mpegts")
            .arg("pipe:1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        unsafe {
            command.pre_exec(move || {
                install_child_descriptor(video_read_fd, 3)?;
                install_child_descriptor(audio_read_fd, 4)?;

                if video_read_fd != 3 && video_read_fd != 4 {
                    libc::close(video_read_fd);
                }

                if audio_read_fd != 3 && audio_read_fd != 4 {
                    libc::close(audio_read_fd);
                }

                Ok(())
            });
        }

        let child_result = command.spawn();

        unsafe {
            libc::close(video_read_fd);
            libc::close(audio_read_fd);
        }

        let mut child = child_result?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("FFmpeg stdout was not piped"))?;
        let fanout = MediaFanout::start(
            stdout,
            config.destination_ip.clone(),
            config.udp_port,
            config.spectator_udp_ports.clone(),
            config.spectator_ports_file.clone(),
        )?;

        println!(
            "Encoder started: PID={} primary_destination={}:{} spectator_ports={:?} spectator_ports_file={} video_fps={fps} video_timestamp_clock_hz={} audio_codec={} audio_thread_queue={}",
            child.id(),
            config.destination_ip,
            config.udp_port,
            config.spectator_udp_ports,
            config
                .spectator_ports_file
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".to_owned()),
            VIDEO_TIMESTAMP_CLOCK_HZ,
            config.audio_codec.name(),
            config.audio_thread_queue_size
        );

        Ok((
            Self {
                child: Some(child),
                fanout: Some(fanout),
            },
            EncoderInputs {
                video: video_writer,
                audio: audio_writer,
            },
        ))
    }

    pub fn wait(&mut self) -> io::Result<()> {
        let status = self
            .child
            .take()
            .ok_or_else(|| io::Error::other("FFmpeg process was already reaped"))?
            .wait()?;
        if let Some(mut fanout) = self.fanout.take() {
            fanout.stop()?;
        }

        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "FFmpeg exited with status {status}"
            )))
        }
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match self.child.as_mut() {
            Some(child) => child.try_wait(),
            None => Ok(None),
        }
    }
}

impl Drop for EncoderProcess {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };

        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
        if let Some(mut fanout) = self.fanout.take() {
            let _ = fanout.stop();
        }
    }
}

struct MediaFanout {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<io::Result<()>>>,
}

impl MediaFanout {
    fn start(
        mut source: impl Read + Send + 'static,
        destination_ip: String,
        primary_port: u16,
        initial_spectator_ports: Vec<u16>,
        spectator_ports_file: Option<PathBuf>,
    ) -> io::Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = Arc::clone(&running);
        let handle = thread::spawn(move || {
            let mut buffer = [0_u8; MPEGTS_PACKET_PAYLOAD_SIZE];
            let mut destinations = fanout_destinations(
                &destination_ip,
                primary_port,
                &initial_spectator_ports,
                spectator_ports_file.as_deref(),
            )?;
            let mut last_reload = Instant::now();

            while thread_running.load(Ordering::Acquire) {
                if last_reload.elapsed() >= SPECTATOR_PORT_RELOAD_INTERVAL {
                    destinations = fanout_destinations(
                        &destination_ip,
                        primary_port,
                        &initial_spectator_ports,
                        spectator_ports_file.as_deref(),
                    )?;
                    last_reload = Instant::now();
                }
                let bytes_read = source.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                for destination in &destinations {
                    socket.send_to(&buffer[..bytes_read], destination)?;
                }
            }
            Ok(())
        });
        Ok(Self {
            running,
            handle: Some(handle),
        })
    }

    fn stop(&mut self) -> io::Result<()> {
        self.running.store(false, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            match handle.join() {
                Ok(result) => result,
                Err(_) => Err(io::Error::other("media fan-out thread panicked")),
            }
        } else {
            Ok(())
        }
    }
}

fn fanout_destinations(
    destination_ip: &str,
    primary_port: u16,
    initial_spectator_ports: &[u16],
    spectator_ports_file: Option<&Path>,
) -> io::Result<Vec<String>> {
    let mut ports = std::collections::BTreeSet::new();
    ports.insert(primary_port);
    ports.extend(initial_spectator_ports.iter().copied());
    if let Some(path) = spectator_ports_file
        && let Ok(contents) = std::fs::read_to_string(path)
    {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let port = line.parse::<u16>().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid spectator media port '{line}': {error}"),
                )
            })?;
            ports.insert(port);
        }
    }
    Ok(ports
        .into_iter()
        .map(|port| format!("{destination_ip}:{port}"))
        .collect())
}

fn create_pipe() -> io::Result<(RawFd, File)> {
    let mut descriptors = [0; 2];

    let result = unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC) };

    if result != 0 {
        return Err(io::Error::last_os_error());
    }

    let read_fd = descriptors[0];
    let write_fd = descriptors[1];

    let writer = unsafe { File::from_raw_fd(write_fd) };

    Ok((read_fd, writer))
}

fn install_child_descriptor(source: RawFd, destination: RawFd) -> io::Result<()> {
    if source != destination {
        let result = unsafe { libc::dup2(source, destination) };

        if result == -1 {
            return Err(io::Error::last_os_error());
        }
    }

    // dup2 clears FD_CLOEXEC when it duplicates to a different descriptor,
    // but dup2(fd, fd) does nothing. Clear it explicitly in both cases.
    let flags = unsafe { libc::fcntl(destination, libc::F_GETFD) };

    if flags == -1 {
        return Err(io::Error::last_os_error());
    }

    let result = unsafe { libc::fcntl(destination, libc::F_SETFD, flags & !libc::FD_CLOEXEC) };

    if result == -1 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AudioCodec, EncoderConfig, fanout_destinations};

    #[test]
    fn parses_supported_audio_codecs() {
        assert_eq!("aac".parse(), Ok(AudioCodec::Aac));
        assert_eq!("OPUS".parse(), Ok(AudioCodec::Opus));
        assert!("mp3".parse::<AudioCodec>().is_err());
    }

    #[test]
    fn builds_single_fanout_destination_for_primary_media() {
        let config = test_config(vec![]);

        assert_eq!(
            fanout_destinations(
                &config.destination_ip,
                config.udp_port,
                &config.spectator_udp_ports,
                None,
            )
            .unwrap(),
            vec!["192.0.2.25:41000"]
        );
    }

    #[test]
    fn builds_fanout_destinations_for_spectator_media() {
        let config = test_config(vec![41001, 41002]);

        assert_eq!(
            fanout_destinations(
                &config.destination_ip,
                config.udp_port,
                &config.spectator_udp_ports,
                None,
            )
            .unwrap(),
            vec!["192.0.2.25:41000", "192.0.2.25:41001", "192.0.2.25:41002"]
        );
    }

    fn test_config(spectator_udp_ports: Vec<u16>) -> EncoderConfig {
        EncoderConfig {
            width: 320,
            height: 224,
            fps: 60.0,
            destination_ip: "192.0.2.25".to_owned(),
            udp_port: 41000,
            spectator_udp_ports,
            spectator_ports_file: None,
            audio_codec: AudioCodec::Aac,
            audio_thread_queue_size: 64,
        }
    }
}
