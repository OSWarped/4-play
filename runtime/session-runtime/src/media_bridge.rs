use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const AUDIO_SAMPLE_RATE: usize = 48_000;
const AUDIO_CHANNELS: usize = 2;
const AUDIO_BYTES_PER_SAMPLE: usize = 2;
const AUDIO_BLOCK_MS: usize = 20;

#[derive(Debug, Clone)]
pub struct MediaBridgeConfig {
    pub video_path: PathBuf,
    pub audio_path: PathBuf,
    pub video_capture_path: PathBuf,
    pub audio_capture_path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Default)]
pub struct MediaBridgeMetrics {
    pub video_frames: AtomicU64,
    pub audio_blocks: AtomicU64,
    pub audio_samples: AtomicU64,
    pub video_bytes: AtomicU64,
    pub audio_bytes: AtomicU64,
    pub first_video_at: Mutex<Option<Instant>>,
    pub first_audio_at: Mutex<Option<Instant>>,
}

pub struct MediaBridge {
    config: MediaBridgeConfig,
    metrics: Arc<MediaBridgeMetrics>,
    running: Arc<AtomicBool>,
    handles: Vec<JoinHandle<io::Result<()>>>,
}

impl MediaBridge {
    pub fn new(config: MediaBridgeConfig) -> Self {
        Self {
            config,
            metrics: Arc::new(MediaBridgeMetrics::default()),
            running: Arc::new(AtomicBool::new(false)),
            handles: Vec::new(),
        }
    }

    pub fn start(&mut self) {
        self.running.store(true, Ordering::Release);

        let video_path = self.config.video_path.clone();
        let video_capture_path = self.config.video_capture_path.clone();
        let video_frame_bytes = self.config.width as usize * self.config.height as usize * 4;

        let video_metrics = Arc::clone(&self.metrics);
        let video_running = Arc::clone(&self.running);

        self.handles.push(thread::spawn(move || {
            println!("Video reader waiting on {}", video_path.display());

            let mut source = OpenOptions::new().read(true).open(&video_path)?;
            let mut capture = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&video_capture_path)?;

            let mut frame = vec![0_u8; video_frame_bytes];

            println!(
                "Video reader connected: {} bytes per frame",
                video_frame_bytes
            );
            println!("Video capture output: {}", video_capture_path.display());

            while video_running.load(Ordering::Acquire) {
                match source.read_exact(&mut frame) {
                    Ok(()) => {
                        capture.write_all(&frame)?;

                        if video_metrics.video_frames.load(Ordering::Relaxed) == 0 {
                            let mut first = video_metrics
                                .first_video_at
                                .lock()
                                .map_err(|_| io::Error::other("video timestamp lock poisoned"))?;

                            *first = Some(Instant::now());
                        }

                        video_metrics.video_frames.fetch_add(1, Ordering::Relaxed);

                        video_metrics
                            .video_bytes
                            .fetch_add(video_frame_bytes as u64, Ordering::Relaxed);
                    }
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                        break;
                    }
                    Err(error) => return Err(error),
                }
            }

            capture.flush()?;

            Ok(())
        }));

        let audio_path = self.config.audio_path.clone();
        let audio_capture_path = self.config.audio_capture_path.clone();

        let audio_block_samples = AUDIO_SAMPLE_RATE * AUDIO_BLOCK_MS / 1_000;

        let audio_block_bytes = audio_block_samples * AUDIO_CHANNELS * AUDIO_BYTES_PER_SAMPLE;

        let audio_metrics = Arc::clone(&self.metrics);
        let audio_running = Arc::clone(&self.running);

        self.handles.push(thread::spawn(move || {
            println!("Audio reader waiting on {}", audio_path.display());

            let mut source = OpenOptions::new().read(true).open(&audio_path)?;
            let mut capture = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&audio_capture_path)?;

            let mut block = vec![0_u8; audio_block_bytes];

            println!(
                "Audio reader connected: {} bytes per {} ms block",
                audio_block_bytes, AUDIO_BLOCK_MS
            );
            println!("Audio capture output: {}", audio_capture_path.display());

            while audio_running.load(Ordering::Acquire) {
                match source.read_exact(&mut block) {
                    Ok(()) => {
                        capture.write_all(&block)?;

                        if audio_metrics.audio_blocks.load(Ordering::Relaxed) == 0 {
                            let mut first = audio_metrics
                                .first_audio_at
                                .lock()
                                .map_err(|_| io::Error::other("audio timestamp lock poisoned"))?;

                            *first = Some(Instant::now());
                        }

                        audio_metrics.audio_blocks.fetch_add(1, Ordering::Relaxed);

                        audio_metrics
                            .audio_samples
                            .fetch_add(audio_block_samples as u64, Ordering::Relaxed);

                        audio_metrics
                            .audio_bytes
                            .fetch_add(audio_block_bytes as u64, Ordering::Relaxed);
                    }
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                        break;
                    }
                    Err(error) => return Err(error),
                }
            }

            capture.flush()?;

            Ok(())
        }));
    }

    pub fn monitor_for(&self, duration: Duration) {
        let monitor_started = Instant::now();

        while monitor_started.elapsed() < duration {
            thread::sleep(Duration::from_secs(1));

            let frames = self.metrics.video_frames.load(Ordering::Relaxed);
            let audio_blocks = self.metrics.audio_blocks.load(Ordering::Relaxed);
            let audio_samples = self.metrics.audio_samples.load(Ordering::Relaxed);

            let first_video = self
                .metrics
                .first_video_at
                .lock()
                .ok()
                .and_then(|value| *value);

            let first_audio = self
                .metrics
                .first_audio_at
                .lock()
                .ok()
                .and_then(|value| *value);

            let video_fps = first_video
                .map(|started| {
                    let elapsed = started.elapsed().as_secs_f64();

                    if elapsed > 0.0 {
                        frames as f64 / elapsed
                    } else {
                        0.0
                    }
                })
                .unwrap_or(0.0);

            let audio_seconds = audio_samples as f64 / AUDIO_SAMPLE_RATE as f64;

            let startup_offset_ms = match (first_video, first_audio) {
                (Some(video), Some(audio)) if audio >= video => {
                    audio.duration_since(video).as_secs_f64() * 1_000.0
                }
                (Some(video), Some(audio)) => {
                    -(video.duration_since(audio).as_secs_f64() * 1_000.0)
                }
                _ => 0.0,
            };

            println!(
                "video_frames={frames:<6} video_fps={video_fps:<6.2} \
                 audio_blocks={audio_blocks:<6} \
                 audio_seconds={audio_seconds:<6.2} \
                 startup_offset_ms={startup_offset_ms:+.3}"
            );
        }
    }

    pub fn stop(mut self) -> io::Result<()> {
        self.running.store(false, Ordering::Release);

        for handle in self.handles.drain(..) {
            match handle.join() {
                Ok(result) => result?,
                Err(_) => {
                    return Err(io::Error::other("media reader thread panicked"));
                }
            }
        }

        Ok(())
    }
}
