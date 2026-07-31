mod encoder;
mod media_bridge;
mod session;

use encoder::{EncoderConfig, EncoderProcess};
use media_bridge::{MediaBridge, MediaBridgeConfig};
use session::{Session, SessionConfig};
use std::env;
use std::process;
use std::time::Duration;

#[derive(Debug)]
struct RuntimeArgs {
    session_id: u32,
    rom: String,
    width: u32,
    height: u32,
    fps: f64,
    destination_ip: String,
    udp_port: u16,
    duration_seconds: u64,
}

fn print_usage(program: &str) {
    eprintln!(
        "Usage:
  {program} \\
    --session-id <id> \\
    --rom <name> \\
    --width <pixels> \\
    --height <pixels> \\
    --fps <rate> \\
    --udp-port <port> \\
    [--destination-ip <address>] \\
    [--duration <seconds>]"
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
    let mut duration_seconds = 70_u64;

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

            "--duration" => {
                duration_seconds =
                    parse_value(require_value(&mut args, "--duration"), "--duration");
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

    RuntimeArgs {
        session_id: required(session_id, "--session-id", &program),
        rom: required(rom, "--rom", &program),
        width: required(width, "--width", &program),
        height: required(height, "--height", &program),
        fps: required(fps, "--fps", &program),
        destination_ip,
        udp_port: required(udp_port, "--udp-port", &program),
        duration_seconds,
    }
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
    };

    let (encoder, inputs) = EncoderProcess::spawn(&encoder_config)?;

    let mut bridge = MediaBridge::new(MediaBridgeConfig {
        video_path: session.video_path(),
        audio_path: session.audio_path(),
        width: session.config.width,
        height: session.config.height,
    });

    bridge.start(inputs.video, inputs.audio);

    println!(
        "Live bridge ready for session {}. Launch MAME.",
        session.config.id
    );

    bridge.monitor_for(Duration::from_secs(args.duration_seconds));

    bridge.stop()?;
    encoder.wait()?;

    Ok(())
}
