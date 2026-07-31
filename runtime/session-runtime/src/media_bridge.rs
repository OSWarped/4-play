use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError, bounded};
use std::fs::{File, OpenOptions};
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

const VIDEO_QUEUE_CAPACITY: usize = 2;
const AUDIO_QUEUE_CAPACITY: usize = 4;

#[derive(Debug, Clone)]
pub struct MediaBridgeConfig {
    pub video_path: PathBuf,
    pub audio_path: PathBuf,
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

    pub video_frames_dropped: AtomicU64,
    pub audio_blocks_dropped: AtomicU64,

    pub video_queue_depth: AtomicU64,
    pub audio_queue_depth: AtomicU64,

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

    pub fn start(&mut self, video_output: File, audio_output: File) {
        self.running.store(true, Ordering::Release);

        let (video_sender, video_receiver) = bounded::<Vec<u8>>(VIDEO_QUEUE_CAPACITY);

        let (audio_sender, audio_receiver) = bounded::<Vec<u8>>(AUDIO_QUEUE_CAPACITY);

        self.start_video_reader(video_sender, video_receiver.clone());

        self.start_audio_reader(audio_sender, audio_receiver.clone());

        self.start_video_writer(video_receiver, video_output);
        self.start_audio_writer(audio_receiver, audio_output);
    }

    fn start_video_reader(&mut self, sender: Sender<Vec<u8>>, drop_receiver: Receiver<Vec<u8>>) {
        let video_path = self.config.video_path.clone();

        let frame_bytes = self.config.width as usize * self.config.height as usize * 4;

        let metrics = Arc::clone(&self.metrics);
        let running = Arc::clone(&self.running);

        self.handles.push(thread::spawn(move || {
            println!("Video reader waiting on {}", video_path.display());

            let mut source = OpenOptions::new().read(true).open(&video_path)?;

            println!("Video reader connected: {} bytes per frame", frame_bytes);

            while running.load(Ordering::Acquire) {
                let mut frame = vec![0_u8; frame_bytes];

                match source.read_exact(&mut frame) {
                    Ok(()) => {
                        record_first_video(&metrics)?;

                        metrics.video_frames.fetch_add(1, Ordering::Relaxed);

                        metrics
                            .video_bytes
                            .fetch_add(frame_bytes as u64, Ordering::Relaxed);

                        send_latest(
                            &sender,
                            &drop_receiver,
                            frame,
                            &metrics.video_frames_dropped,
                        );

                        metrics
                            .video_queue_depth
                            .store(sender.len() as u64, Ordering::Relaxed);
                    }
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                        break;
                    }
                    Err(error) => return Err(error),
                }
            }

            drop(sender);
            Ok(())
        }));
    }

    fn start_audio_reader(&mut self, sender: Sender<Vec<u8>>, drop_receiver: Receiver<Vec<u8>>) {
        let audio_path = self.config.audio_path.clone();

        let block_samples = AUDIO_SAMPLE_RATE * AUDIO_BLOCK_MS / 1_000;

        let block_bytes = block_samples * AUDIO_CHANNELS * AUDIO_BYTES_PER_SAMPLE;

        let metrics = Arc::clone(&self.metrics);
        let running = Arc::clone(&self.running);

        self.handles.push(thread::spawn(move || {
            println!("Audio reader waiting on {}", audio_path.display());

            let mut source = OpenOptions::new().read(true).open(&audio_path)?;

            println!(
                "Audio reader connected: {} bytes per {} ms block",
                block_bytes, AUDIO_BLOCK_MS
            );

            while running.load(Ordering::Acquire) {
                let mut block = vec![0_u8; block_bytes];

                match source.read_exact(&mut block) {
                    Ok(()) => {
                        record_first_audio(&metrics)?;

                        metrics.audio_blocks.fetch_add(1, Ordering::Relaxed);

                        metrics
                            .audio_samples
                            .fetch_add(block_samples as u64, Ordering::Relaxed);

                        metrics
                            .audio_bytes
                            .fetch_add(block_bytes as u64, Ordering::Relaxed);

                        send_latest(
                            &sender,
                            &drop_receiver,
                            block,
                            &metrics.audio_blocks_dropped,
                        );

                        metrics
                            .audio_queue_depth
                            .store(sender.len() as u64, Ordering::Relaxed);
                    }
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                        break;
                    }
                    Err(error) => return Err(error),
                }
            }

            drop(sender);
            Ok(())
        }));
    }

    fn start_video_writer(&mut self, receiver: Receiver<Vec<u8>>, mut output: File) {
        let metrics = Arc::clone(&self.metrics);

        self.handles.push(thread::spawn(move || {
            while let Ok(frame) = receiver.recv() {
                output.write_all(&frame)?;

                metrics
                    .video_queue_depth
                    .store(receiver.len() as u64, Ordering::Relaxed);
            }

            output.flush()?;
            Ok(())
        }));
    }

    fn start_audio_writer(&mut self, receiver: Receiver<Vec<u8>>, mut output: File) {
        let metrics = Arc::clone(&self.metrics);

        self.handles.push(thread::spawn(move || {
            while let Ok(block) = receiver.recv() {
                output.write_all(&block)?;

                metrics
                    .audio_queue_depth
                    .store(receiver.len() as u64, Ordering::Relaxed);
            }

            output.flush()?;
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

            let dropped_video = self.metrics.video_frames_dropped.load(Ordering::Relaxed);

            let dropped_audio = self.metrics.audio_blocks_dropped.load(Ordering::Relaxed);

            let video_queue = self.metrics.video_queue_depth.load(Ordering::Relaxed);

            let audio_queue = self.metrics.audio_queue_depth.load(Ordering::Relaxed);

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

            let startup_offset_ms = startup_offset_ms(first_video, first_audio);

            println!(
                "video_frames={frames:<6} video_fps={video_fps:<6.2} \
                 audio_blocks={audio_blocks:<6} \
                 audio_seconds={audio_seconds:<6.2} \
                 offset_ms={startup_offset_ms:+.3} \
                 dropped_v={dropped_video:<5} \
                 dropped_a={dropped_audio:<5} \
                 vq={video_queue} aq={audio_queue}"
            );
        }
    }

    pub fn stop(mut self) -> io::Result<()> {
        self.running.store(false, Ordering::Release);

        for handle in self.handles.drain(..) {
            match handle.join() {
                Ok(result) => result?,
                Err(_) => {
                    return Err(io::Error::other("media bridge thread panicked"));
                }
            }
        }

        Ok(())
    }
}

fn send_latest(
    sender: &Sender<Vec<u8>>,
    drop_receiver: &Receiver<Vec<u8>>,
    value: Vec<u8>,
    dropped_counter: &AtomicU64,
) {
    match sender.try_send(value) {
        Ok(()) => {}

        Err(TrySendError::Full(value)) => {
            match drop_receiver.try_recv() {
                Ok(_) => {
                    dropped_counter.fetch_add(1, Ordering::Relaxed);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => return,
            }

            if sender.try_send(value).is_err() {
                dropped_counter.fetch_add(1, Ordering::Relaxed);
            }
        }

        Err(TrySendError::Disconnected(_)) => {}
    }
}

fn record_first_video(metrics: &MediaBridgeMetrics) -> io::Result<()> {
    if metrics.video_frames.load(Ordering::Relaxed) == 0 {
        let mut first = metrics
            .first_video_at
            .lock()
            .map_err(|_| io::Error::other("video timestamp lock poisoned"))?;

        if first.is_none() {
            *first = Some(Instant::now());
        }
    }

    Ok(())
}

fn record_first_audio(metrics: &MediaBridgeMetrics) -> io::Result<()> {
    if metrics.audio_blocks.load(Ordering::Relaxed) == 0 {
        let mut first = metrics
            .first_audio_at
            .lock()
            .map_err(|_| io::Error::other("audio timestamp lock poisoned"))?;

        if first.is_none() {
            *first = Some(Instant::now());
        }
    }

    Ok(())
}

fn startup_offset_ms(video: Option<Instant>, audio: Option<Instant>) -> f64 {
    match (video, audio) {
        (Some(video), Some(audio)) if audio >= video => {
            audio.duration_since(video).as_secs_f64() * 1_000.0
        }

        (Some(video), Some(audio)) => -(video.duration_since(audio).as_secs_f64() * 1_000.0),

        _ => 0.0,
    }
}
