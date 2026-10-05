mod encoder;
mod mame;
mod media_bridge;
mod network_input;
mod session;
mod terminal_input;
mod virtual_controller;

use encoder::{AudioCodec, EncoderConfig, EncoderProcess};
use input_protocol::SessionToken;
use mame::{MameConfig, MameProcess};
use media_bridge::{MediaBridge, MediaBridgeConfig};
use network_input::run_network_input;
use session::{Session, SessionConfig};
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use signal_hook::flag;
use std::env;
use std::io;
use std::path::PathBuf;
use std::process::{self, ExitStatus};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;
use std::time::Instant;
use terminal_input::run_terminal_input;
use virtual_controller::{VirtualController, mame_device_id_match};

#[derive(Debug)]
struct RuntimeArgs {
    session_id: String,
    rom: String,
    width: u32,
    height: u32,
    fps: f64,
    destination_ip: String,
    udp_port: u16,
    terminal_input: bool,
    input_port: Option<u16>,
    input_token: Option<SessionToken>,
    players: u8,
    debug_input: bool,
    autosave: bool,
    audio_codec: AudioCodec,
    audio_block_ms: usize,
    audio_thread_queue_size: usize,
    mame_path: PathBuf,
    mame_ini_path: PathBuf,
    status_file: Option<PathBuf>,
}

#[derive(Debug)]
enum ChildFailure {
    Mame(ExitStatus),
    Encoder(ExitStatus),
}

impl std::fmt::Display for ChildFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mame(status) => {
                write!(formatter, "MAME exited unexpectedly with status {status}")
            }
            Self::Encoder(status) => {
                write!(formatter, "FFmpeg exited unexpectedly with status {status}")
            }
        }
    }
}

fn print_usage(program: &str) {
    eprintln!(
        "Usage:
  {program} \
    --session-id <id> \
    --rom <name> \
    --width <pixels> \
    --height <pixels> \
    --fps <rate> \
    --udp-port <port> \
    [--destination-ip <address>] \
    [--terminal-input | --input-port <port>] \
    [--input-token <uuid>] \
    [--players <count>] \
    [--debug-input] \
    [--autosave] \
    [--audio-codec <aac|opus>] \
    [--audio-block-ms <milliseconds>] \
    [--audio-thread-queue-size <packets>] \
    [--mame-path <path>] \
    [--mame-ini-path <path>] \
    [--status-file <path>]"
    );
}

fn require_value<I>(args: &mut I, option: &str) -> String
where
    I: Iterator<Item = String>,
{
    args.next().unwrap_or_else(|| {
        eprintln!("Missing value for {option}");
        process::exit(2);
    })
}

fn parse_value<T>(value: String, option: &str) -> T
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value.parse::<T>().unwrap_or_else(|error| {
        eprintln!("Invalid value for {option}: {error}");
        process::exit(2);
    })
}

fn required<T>(value: Option<T>, option: &str, program: &str) -> T {
    value.unwrap_or_else(|| {
        eprintln!("Missing required option: {option}");
        print_usage(program);
        process::exit(2);
    })
}

fn parse_args() -> RuntimeArgs {
    let mut args = env::args();

    let program = args.next().unwrap_or_else(|| "session-runtime".to_string());

    let mut session_id = None;
    let mut rom = None;
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    let mut udp_port = None;
    let mut destination_ip = String::from("192.168.20.10");
    let mut terminal_input = false;
    let mut input_port = None;
    let mut input_token = None;
    let mut players = 1;
    let mut debug_input = false;
    let mut autosave = false;
    let mut audio_codec = AudioCodec::Aac;
    let mut audio_block_ms = 20;
    let mut audio_thread_queue_size = 64;
    let mut mame_path = PathBuf::from("/home/blake/src/mame-4play/mame");
    let mut mame_ini_path = PathBuf::from("/opt/4play/config/mame");
    let mut status_file = None;

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--session-id" => {
                session_id = Some(parse_value(
                    require_value(&mut args, "--session-id"),
                    "--session-id",
                ));
            }
            "--rom" => {
                rom = Some(require_value(&mut args, "--rom"));
            }
            "--width" => {
                width = Some(parse_value(require_value(&mut args, "--width"), "--width"));
            }
            "--height" => {
                height = Some(parse_value(
                    require_value(&mut args, "--height"),
                    "--height",
                ));
            }
            "--fps" => {
                fps = Some(parse_value(require_value(&mut args, "--fps"), "--fps"));
            }
            "--destination-ip" => {
                destination_ip = require_value(&mut args, "--destination-ip");
            }
            "--udp-port" => {
                udp_port = Some(parse_value(
                    require_value(&mut args, "--udp-port"),
                    "--udp-port",
                ));
            }
            "--terminal-input" => {
                terminal_input = true;
            }
            "--input-port" => {
                input_port = Some(parse_value(
                    require_value(&mut args, "--input-port"),
                    "--input-port",
                ));
            }
            "--input-token" => {
                input_token = Some(parse_value(
                    require_value(&mut args, "--input-token"),
                    "--input-token",
                ));
            }
            "--players" => {
                players = parse_value(require_value(&mut args, "--players"), "--players");
            }
            "--debug-input" => {
                debug_input = true;
            }
            "--autosave" => {
                autosave = true;
            }
            "--audio-codec" => {
                audio_codec =
                    parse_value(require_value(&mut args, "--audio-codec"), "--audio-codec");
            }
            "--audio-block-ms" => {
                audio_block_ms = parse_value(
                    require_value(&mut args, "--audio-block-ms"),
                    "--audio-block-ms",
                );
            }
            "--audio-thread-queue-size" => {
                audio_thread_queue_size = parse_value(
                    require_value(&mut args, "--audio-thread-queue-size"),
                    "--audio-thread-queue-size",
                );
            }
            "--mame-path" => {
                mame_path = PathBuf::from(require_value(&mut args, "--mame-path"));
            }
            "--mame-ini-path" => {
                mame_ini_path = PathBuf::from(require_value(&mut args, "--mame-ini-path"));
            }
            "--status-file" => {
                status_file = Some(PathBuf::from(require_value(&mut args, "--status-file")));
            }
            "--help" | "-h" => {
                print_usage(&program);
                process::exit(0);
            }
            unknown => {
                eprintln!("Unknown option: {unknown}");
                print_usage(&program);
                process::exit(2);
            }
        }
    }

    let parsed = RuntimeArgs {
        session_id: required(session_id, "--session-id", &program),
        rom: required(rom, "--rom", &program),
        width: required(width, "--width", &program),
        height: required(height, "--height", &program),
        fps: required(fps, "--fps", &program),
        destination_ip,
        udp_port: required(udp_port, "--udp-port", &program),
        terminal_input,
        input_port,
        input_token,
        players,
        debug_input,
        autosave,
        audio_codec,
        audio_block_ms,
        audio_thread_queue_size,
        mame_path,
        mame_ini_path,
        status_file,
    };

    if parsed.width == 0 || parsed.height == 0 {
        eprintln!("Width and height must be greater than zero.");
        process::exit(2);
    }

    if !parsed.fps.is_finite() || parsed.fps <= 0.0 {
        eprintln!("FPS must be a positive finite number.");
        process::exit(2);
    }

    if parsed.terminal_input && parsed.input_port.is_some() {
        eprintln!("--terminal-input and --input-port cannot be used together.");
        process::exit(2);
    }
    if parsed.input_token.is_some() && parsed.input_port.is_none() {
        eprintln!("--input-token requires --input-port.");
        process::exit(2);
    }
    if !(1..=8).contains(&parsed.players) {
        eprintln!("--players must be between 1 and 8.");
        process::exit(2);
    }

    if !(1..=100).contains(&parsed.audio_block_ms) {
        eprintln!("--audio-block-ms must be between 1 and 100.");
        process::exit(2);
    }

    if !(1..=1024).contains(&parsed.audio_thread_queue_size) {
        eprintln!("--audio-thread-queue-size must be between 1 and 1024.");
        process::exit(2);
    }

    parsed
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args();
    write_runtime_status(args.status_file.as_deref(), "starting")?;
    let status_file = args.status_file.clone();
    let result = run(args);
    if result.is_ok() {
        write_runtime_status(status_file.as_deref(), "stopped")?;
    }
    result
}

fn run(args: RuntimeArgs) -> Result<(), Box<dyn std::error::Error>> {
    let runtime_status_file = args.status_file.clone();
    let shutdown_requested = install_shutdown_handlers()?;
    let controller_enabled = args.terminal_input || args.input_port.is_some();
    let controller_id = controller_id(&args.session_id);
    let controller_device_ids = if controller_enabled {
        (1..=args.players)
            .map(|player_number| {
                (
                    player_number,
                    mame_device_id_match(controller_id, player_number),
                )
            })
            .collect()
    } else {
        Vec::new()
    };

    let config = SessionConfig {
        id: args.session_id.clone(),
        rom: args.rom,
        width: args.width,
        height: args.height,
        fps: args.fps,
        destination_ip: args.destination_ip,
        udp_port: args.udp_port,
    };

    let mut session = Session::new(config);

    session.prepare()?;
    session.create_media_endpoints()?;

    println!("{session}");

    let encoder_config = EncoderConfig {
        width: session.config.width,
        height: session.config.height,
        fps: session.config.fps,
        destination_ip: session.config.destination_ip.clone(),
        udp_port: session.config.udp_port,
        audio_codec: args.audio_codec,
        audio_thread_queue_size: args.audio_thread_queue_size,
    };

    session.state = session::SessionState::LaunchingEncoder;
    let (mut encoder, inputs) = EncoderProcess::spawn(&encoder_config)?;

    let mut bridge = MediaBridge::new(MediaBridgeConfig {
        video_path: session.video_path(),
        audio_path: session.audio_path(),
        width: session.config.width,
        height: session.config.height,
        audio_block_ms: args.audio_block_ms,
    });

    bridge.start(inputs.video, inputs.audio);

    println!("Media bridge ready for session {}.", session.config.id);

    let mut controllers = controller_enabled
        .then(|| {
            (1..=args.players)
                .map(|player_number| VirtualController::create(controller_id, player_number))
                .collect::<io::Result<Vec<_>>>()
        })
        .transpose()?;

    let mame_config = MameConfig {
        binary: args.mame_path,
        ini_path: args.mame_ini_path,
        rom: session.config.rom.clone(),
        working_directory: session.working_directory.clone(),
        video_path: session.video_path(),
        audio_path: session.audio_path(),
        controller_device_ids,
        autosave: args.autosave,
    };

    session.state = session::SessionState::LaunchingEmulator;
    let mut mame = MameProcess::spawn(&mame_config)?;
    session.state = session::SessionState::Running;
    wait_for_runtime_ready(&bridge, &mut mame, &mut encoder)?;
    write_runtime_status(runtime_status_file.as_deref(), "active")?;
    let mut child_failure = None;

    let input_result = if let Some(controllers) = controllers.as_mut() {
        if args.terminal_input {
            run_terminal_input(&mut controllers[0], || {
                should_stop(
                    &shutdown_requested,
                    &mut mame,
                    &mut encoder,
                    &mut child_failure,
                )
            })
        } else {
            run_network_input(
                controllers,
                args.input_port.unwrap(),
                args.input_token,
                args.debug_input,
                || {
                    should_stop(
                        &shutdown_requested,
                        &mut mame,
                        &mut encoder,
                        &mut child_failure,
                    )
                },
            )
        }
    } else {
        wait_for_shutdown_or_child(
            &mut mame,
            &mut encoder,
            &shutdown_requested,
            &mut child_failure,
        )
    };

    if shutdown_requested.load(Ordering::Acquire) {
        println!(
            "Shutdown requested; stopping session {}.",
            session.config.id
        );
    }

    // Always attempt every cleanup stage. Preserve the first operational error
    // only after MAME, bridge threads, and FFmpeg have all been stopped/reaped.
    session.state = session::SessionState::Stopping;
    let mame_result = mame.terminate();
    let bridge_result = bridge.stop();
    let encoder_result = encoder.wait();

    input_result?;

    if let Some(failure) = child_failure {
        return Err(io::Error::other(failure.to_string()).into());
    }

    if let Some(status) = mame_result? {
        println!("MAME exited with status: {status}");
    }

    bridge_result?;
    encoder_result?;
    session.state = session::SessionState::Stopped;

    Ok(())
}

fn wait_for_runtime_ready(
    bridge: &MediaBridge,
    mame: &mut MameProcess,
    encoder: &mut EncoderProcess,
) -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    while !bridge.is_ready() {
        if let Some(status) = mame.try_wait()? {
            return Err(io::Error::other(format!(
                "MAME exited during startup with status {status}"
            ))
            .into());
        }
        if let Some(status) = encoder.try_wait()? {
            return Err(io::Error::other(format!(
                "FFmpeg exited during startup with status {status}"
            ))
            .into());
        }
        if started.elapsed() >= Duration::from_secs(10) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "session runtime did not receive video and audio within 10 seconds",
            )
            .into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn controller_id(session_id: &str) -> u32 {
    session_id.bytes().fold(2_166_136_261_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    })
}

fn write_runtime_status(path: Option<&std::path::Path>, status: &str) -> io::Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{status}\n"))
}

fn install_shutdown_handlers() -> io::Result<Arc<AtomicBool>> {
    let shutdown_requested = Arc::new(AtomicBool::new(false));

    flag::register(SIGINT, Arc::clone(&shutdown_requested))?;
    flag::register(SIGTERM, Arc::clone(&shutdown_requested))?;

    Ok(shutdown_requested)
}

fn should_stop(
    shutdown_requested: &AtomicBool,
    mame: &mut MameProcess,
    encoder: &mut EncoderProcess,
    child_failure: &mut Option<ChildFailure>,
) -> io::Result<bool> {
    if shutdown_requested.load(Ordering::Acquire) {
        return Ok(true);
    }

    if let Some(status) = mame.try_wait()? {
        *child_failure = Some(ChildFailure::Mame(status));
        return Ok(true);
    }

    if let Some(status) = encoder.try_wait()? {
        *child_failure = Some(ChildFailure::Encoder(status));
        return Ok(true);
    }

    Ok(false)
}

fn wait_for_shutdown_or_child(
    mame: &mut MameProcess,
    encoder: &mut EncoderProcess,
    shutdown_requested: &AtomicBool,
    child_failure: &mut Option<ChildFailure>,
) -> io::Result<()> {
    while !should_stop(shutdown_requested, mame, encoder, child_failure)? {
        thread::sleep(Duration::from_millis(25));
    }
    Ok(())
}
