use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError, bounded};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const AUDIO_SAMPLE_RATE: usize = 48_000;
const AUDIO_CHANNELS: usize = 2;
const AUDIO_BYTES_PER_SAMPLE: usize = 2;
const VIDEO_QUEUE_CAPACITY: usize = 2;
const AUDIO_QUEUE_CAPACITY: usize = 4;

#[derive(Debug, Clone)]
pub struct MediaBridgeConfig {
    pub video_path: PathBuf,
    pub audio_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub audio_block_ms: usize,
    pub preview_image_path: Option<PathBuf>,
    pub preview_interval_ms: u64,
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
    pub preview_frames_written: AtomicU64,

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
        let bridge_started = Instant::now();
        self.running.store(true, Ordering::Release);

        let (video_sender, video_receiver) = bounded::<Vec<u8>>(VIDEO_QUEUE_CAPACITY);
        let latest_preview_frame = Arc::new(Mutex::new(None));

        let (audio_sender, audio_receiver) = bounded::<Vec<u8>>(AUDIO_QUEUE_CAPACITY);

        self.start_video_reader(
            video_sender,
            video_receiver.clone(),
            Arc::clone(&latest_preview_frame),
        );

        self.start_audio_reader(audio_sender, audio_receiver.clone());

        self.start_video_writer(video_receiver, video_output);
        self.start_audio_writer(audio_receiver, audio_output);
        self.start_preview_writer(latest_preview_frame);
        self.start_metrics_monitor(bridge_started);
    }

    pub fn is_ready(&self) -> bool {
        self.metrics.video_frames.load(Ordering::Acquire) > 0
            && self.metrics.audio_blocks.load(Ordering::Acquire) > 0
    }

    fn start_video_reader(
        &mut self,
        sender: Sender<Vec<u8>>,
        drop_receiver: Receiver<Vec<u8>>,
        latest_preview_frame: Arc<Mutex<Option<Vec<u8>>>>,
    ) {
        let video_path = self.config.video_path.clone();

        let frame_bytes = self.config.width as usize * self.config.height as usize * 4;

        let metrics = Arc::clone(&self.metrics);
        let running = Arc::clone(&self.running);

        self.handles.push(thread::spawn(move || {
            println!("Video reader waiting on {}", video_path.display());

            let mut source = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&video_path)?;

            println!("Video reader connected: {} bytes per frame", frame_bytes);

            while running.load(Ordering::Acquire) {
                let mut frame = vec![0_u8; frame_bytes];

                match read_exact_while_running(&mut source, &mut frame, &running)? {
                    true => {
                        record_first_video(&metrics)?;

                        metrics.video_frames.fetch_add(1, Ordering::Relaxed);

                        metrics
                            .video_bytes
                            .fetch_add(frame_bytes as u64, Ordering::Relaxed);

                        if let Ok(mut latest) = latest_preview_frame.lock() {
                            *latest = Some(frame.clone());
                        }

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
                    false => break,
                }
            }

            drop(sender);
            Ok(())
        }));
    }

    fn start_audio_reader(&mut self, sender: Sender<Vec<u8>>, drop_receiver: Receiver<Vec<u8>>) {
        let audio_path = self.config.audio_path.clone();

        let audio_block_ms = self.config.audio_block_ms;
        let block_samples = AUDIO_SAMPLE_RATE * audio_block_ms / 1_000;

        let block_bytes = block_samples * AUDIO_CHANNELS * AUDIO_BYTES_PER_SAMPLE;

        let metrics = Arc::clone(&self.metrics);
        let running = Arc::clone(&self.running);

        self.handles.push(thread::spawn(move || {
            println!("Audio reader waiting on {}", audio_path.display());

            let mut source = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&audio_path)?;

            println!(
                "Audio reader connected: {} bytes per {} ms block",
                block_bytes, audio_block_ms
            );

            while running.load(Ordering::Acquire) {
                let mut block = vec![0_u8; block_bytes];

                match read_exact_while_running(&mut source, &mut block, &running)? {
                    true => {
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
                    false => break,
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

    fn start_preview_writer(&mut self, latest_preview_frame: Arc<Mutex<Option<Vec<u8>>>>) {
        let Some(preview_path) = self.config.preview_image_path.clone() else {
            return;
        };
        let width = self.config.width;
        let height = self.config.height;
        let interval = Duration::from_millis(self.config.preview_interval_ms.max(100));
        let metrics = Arc::clone(&self.metrics);
        let running = Arc::clone(&self.running);

        self.handles.push(thread::spawn(move || {
            println!(
                "Preview writer enabled: {} every {} ms",
                preview_path.display(),
                interval.as_millis()
            );
            while running.load(Ordering::Acquire) {
                thread::sleep(interval);
                let frame = latest_preview_frame
                    .lock()
                    .map_err(|_| io::Error::other("preview frame lock poisoned"))?
                    .clone();
                let Some(frame) = frame else {
                    continue;
                };
                write_bgr0_bmp_atomic(&preview_path, width, height, &frame)?;
                metrics
                    .preview_frames_written
                    .fetch_add(1, Ordering::Relaxed);
            }
            Ok(())
        }));
    }

    fn start_metrics_monitor(&mut self, bridge_started: Instant) {
        let metrics = Arc::clone(&self.metrics);
        let running = Arc::clone(&self.running);

        self.handles.push(thread::spawn(move || {
            while running.load(Ordering::Acquire) {
                thread::sleep(Duration::from_secs(1));

                let frames = metrics.video_frames.load(Ordering::Relaxed);

                let audio_blocks = metrics.audio_blocks.load(Ordering::Relaxed);

                let audio_samples = metrics.audio_samples.load(Ordering::Relaxed);

                let dropped_video = metrics.video_frames_dropped.load(Ordering::Relaxed);

                let dropped_audio = metrics.audio_blocks_dropped.load(Ordering::Relaxed);

                let video_queue = metrics.video_queue_depth.load(Ordering::Relaxed);

                let audio_queue = metrics.audio_queue_depth.load(Ordering::Relaxed);

                let preview_frames = metrics.preview_frames_written.load(Ordering::Relaxed);

                let first_video = metrics.first_video_at.lock().ok().and_then(|value| *value);

                let first_audio = metrics.first_audio_at.lock().ok().and_then(|value| *value);

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

                let video_start_ms = elapsed_ms(bridge_started, first_video);
                let audio_start_ms = elapsed_ms(bridge_started, first_audio);
                let startup_offset_ms = startup_offset_ms(first_video, first_audio);

                println!(
                    "video_frames={frames:<6} video_fps={video_fps:<6.2} \
                 audio_blocks={audio_blocks:<6} \
                 audio_seconds={audio_seconds:<6.2} \
                 video_start_ms={video_start_ms:<8.3} \
                 audio_start_ms={audio_start_ms:<8.3} \
                 offset_ms={startup_offset_ms:+.3} \
                 dropped_v={dropped_video:<5} \
                 dropped_a={dropped_audio:<5} \
                 vq={video_queue} aq={audio_queue} preview_frames={preview_frames}"
                );
            }

            Ok(())
        }));
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

fn read_exact_while_running(
    source: &mut File,
    buffer: &mut [u8],
    running: &AtomicBool,
) -> io::Result<bool> {
    let mut offset = 0;
    while offset < buffer.len() {
        if !running.load(Ordering::Acquire) {
            return Ok(false);
        }
        match source.read(&mut buffer[offset..]) {
            Ok(0) => thread::sleep(Duration::from_millis(2)),
            Ok(count) => offset += count,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(true)
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

fn write_bgr0_bmp_atomic(path: &Path, width: u32, height: u32, frame: &[u8]) -> io::Result<()> {
    let expected = width as usize * height as usize * 4;
    if frame.len() != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "preview frame has unexpected size",
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.to_path_buf();
    tmp.set_extension("tmp");
    let bmp = encode_bgr0_as_bmp(width, height, frame)?;
    std::fs::write(&tmp, bmp)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn encode_bgr0_as_bmp(width: u32, height: u32, frame: &[u8]) -> io::Result<Vec<u8>> {
    let row_bytes = width as usize * 3;
    let row_stride = row_bytes.div_ceil(4) * 4;
    let pixel_bytes = row_stride
        .checked_mul(height as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "preview image is too large"))?;
    let file_size = 54usize
        .checked_add(pixel_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "preview image is too large"))?;
    let file_size_u32 = u32::try_from(file_size)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "preview image is too large"))?;
    let pixel_bytes_u32 = u32::try_from(pixel_bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "preview image is too large"))?;
    let height_i32 = i32::try_from(height)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "preview image is too tall"))?;

    let mut bmp = Vec::with_capacity(file_size);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&file_size_u32.to_le_bytes());
    bmp.extend_from_slice(&[0, 0, 0, 0]);
    bmp.extend_from_slice(&(54u32).to_le_bytes());
    bmp.extend_from_slice(&(40u32).to_le_bytes());
    bmp.extend_from_slice(&(width as i32).to_le_bytes());
    bmp.extend_from_slice(&(-height_i32).to_le_bytes());
    bmp.extend_from_slice(&(1u16).to_le_bytes());
    bmp.extend_from_slice(&(24u16).to_le_bytes());
    bmp.extend_from_slice(&(0u32).to_le_bytes());
    bmp.extend_from_slice(&pixel_bytes_u32.to_le_bytes());
    bmp.extend_from_slice(&(2_835i32).to_le_bytes());
    bmp.extend_from_slice(&(2_835i32).to_le_bytes());
    bmp.extend_from_slice(&(0u32).to_le_bytes());
    bmp.extend_from_slice(&(0u32).to_le_bytes());

    let padding = row_stride - row_bytes;
    for row in 0..height as usize {
        let row_start = row * width as usize * 4;
        for column in 0..width as usize {
            let pixel_start = row_start + column * 4;
            bmp.extend_from_slice(&frame[pixel_start..pixel_start + 3]);
        }
        bmp.extend(std::iter::repeat_n(0, padding));
    }
    Ok(bmp)
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

fn elapsed_ms(started: Instant, event: Option<Instant>) -> f64 {
    event
        .map(|event| event.duration_since(started).as_secs_f64() * 1_000.0)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::encode_bgr0_as_bmp;

    #[test]
    fn encodes_bgr0_frame_as_top_down_bmp() {
        let frame = [
            0, 0, 255, 0, // red
            0, 255, 0, 0, // green
            255, 0, 0, 0, // blue
            255, 255, 255, 0, // white
        ];

        let bmp = encode_bgr0_as_bmp(2, 2, &frame).unwrap();

        assert_eq!(&bmp[0..2], b"BM");
        assert_eq!(i32::from_le_bytes(bmp[22..26].try_into().unwrap()), -2);
        assert_eq!(&bmp[54..57], &[0, 0, 255]);
        assert_eq!(&bmp[57..60], &[0, 255, 0]);
    }
}
