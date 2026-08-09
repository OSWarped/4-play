use std::fs::File;
use std::io;
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

#[derive(Debug, Clone)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub destination_ip: String,
    pub udp_port: u16,
}

pub struct EncoderInputs {
    pub video: File,
    pub audio: File,
}

pub struct EncoderProcess {
    child: Child,
}

impl EncoderProcess {
    pub fn spawn(config: &EncoderConfig) -> io::Result<(Self, EncoderInputs)> {
        let (video_read_fd, video_writer) = create_pipe()?;
        let (audio_read_fd, audio_writer) = create_pipe()?;

        let video_size = format!("{}x{}", config.width, config.height);
        let fps = format!("{:.6}", config.fps);
        let destination = format!(
            "udp://{}:{}?pkt_size=1316",
            config.destination_ip, config.udp_port
        );

        let mut command = Command::new("ffmpeg");

        command
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("info")
            .arg("-thread_queue_size")
            .arg("64")
            .arg("-f")
            .arg("rawvideo")
            .arg("-pixel_format")
            .arg("bgr0")
            .arg("-video_size")
            .arg(video_size)
            .arg("-framerate")
            .arg(fps)
            .arg("-i")
            .arg("pipe:3")
            .arg("-thread_queue_size")
            .arg("64")
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
            .arg("-c:a")
            .arg("aac")
            .arg("-b:a")
            .arg("128k")
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
            .arg(destination)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
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

        let child = child_result?;

        println!(
            "Encoder started: PID={} destination={}:{}",
            child.id(),
            config.destination_ip,
            config.udp_port
        );

        Ok((
            Self { child },
            EncoderInputs {
                video: video_writer,
                audio: audio_writer,
            },
        ))
    }

    pub fn wait(mut self) -> io::Result<()> {
        let status = self.child.wait()?;

        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "FFmpeg exited with status {status}"
            )))
        }
    }

    pub fn terminate(&mut self) -> io::Result<()> {
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
        }

        Ok(())
    }
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
