mod encoder;
mod mame;
mod media_bridge;
mod network_input;
mod session;
mod terminal_input;
mod virtual_controller;

use encoder::{AudioCodec, EncoderConfig, EncoderProcess};
use mame::{MameConfig, MameProcess};
use media_bridge::{MediaBridge, MediaBridgeConfig};
use network_input::run_network_input;
use session::{Session, SessionConfig};
use std::env;
use std::path::PathBuf;
use std::process;
use terminal_input::run_terminal_input;
use virtual_controller::VirtualController;

#[derive(Debug)]
struct RuntimeArgs {
    session_id: u32,
    rom: String,
    width: u32,
    height: u32,
    fps: f64,
    destination_ip: String,
    udp_port: u16,
    terminal_input: bool,
    input_port: Option<u16>,
    audio_codec: AudioCodec,
    audio_block_ms: usize,
    audio_thread_queue_size: usize,
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
    [--audio-codec <aac|opus>] \
    [--audio-block-ms <milliseconds>] \
    [--audio-thread-queue-size <packets>]"
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
    let mut audio_codec = AudioCodec::Aac;
    let mut audio_block_ms = 20;
    let mut audio_thread_queue_size = 64;

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
        audio_codec,
        audio_block_ms,
        audio_thread_queue_size,
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

    let config = SessionConfig {
        id: args.session_id,
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

    let (encoder, inputs) = EncoderProcess::spawn(&encoder_config)?;

    let mut bridge = MediaBridge::new(MediaBridgeConfig {
        video_path: session.video_path(),
        audio_path: session.audio_path(),
        width: session.config.width,
        height: session.config.height,
        audio_block_ms: args.audio_block_ms,
    });

    bridge.start(inputs.video, inputs.audio);

    println!("Media bridge ready for session {}.", session.config.id);

    let mut controller = (args.terminal_input || args.input_port.is_some())
        .then(|| VirtualController::create(1))
        .transpose()?;

    let mame_config = MameConfig {
        binary: PathBuf::from("/home/blake/src/mame-4play/mame"),
        ini_path: PathBuf::from("/opt/4play/config/mame"),
        rom: session.config.rom.clone(),
        working_directory: session.working_directory.clone(),
        video_path: session.video_path(),
        audio_path: session.audio_path(),
    };

    let mut mame = MameProcess::spawn(&mame_config)?;

    if let Some(controller) = controller.as_mut() {
        let input_result = if args.terminal_input {
            run_terminal_input(controller)
        } else {
            run_network_input(controller, args.input_port.unwrap(), || {
                Ok(mame.try_wait()?.is_some())
            })
        };
        let terminate_result = mame.terminate();

        input_result?;
        terminate_result?;
    }

    let mame_status = mame.wait()?;

    println!("MAME exited with status: {mame_status}");

    bridge.stop()?;
    encoder.wait()?;

    Ok(())
}
