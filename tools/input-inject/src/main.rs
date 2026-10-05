use input_protocol::{ControllerState, button};
use std::env;
use std::net::{SocketAddr, UdpSocket};
use std::process;
use std::thread;
use std::time::{Duration, Instant};

const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(20);
const DEFAULT_HOLD: Duration = Duration::from_millis(250);

#[derive(Debug, PartialEq, Eq)]
struct Config {
    destination: SocketAddr,
    buttons: u16,
    axis_x: i16,
    axis_y: i16,
    hold: Duration,
}

fn main() {
    let program = env::args()
        .next()
        .unwrap_or_else(|| "input-inject".to_string());
    let config = parse_args(env::args().skip(1)).unwrap_or_else(|error| {
        eprintln!("{error}");
        print_usage(&program);
        process::exit(2);
    });

    if let Err(error) = run(&config) {
        eprintln!("Input injection failed: {error}");
        process::exit(1);
    }
}

fn print_usage(program: &str) {
    eprintln!(
        "Usage: {program} <runtime-address:input-port> \\
  [--buttons <action1,...|coin|start|none>] \\
  [--axis-x <-1|0|1>] [--axis-y <-1|0|1>] [--hold-ms <milliseconds>]"
    );
}

fn parse_args<I>(mut args: I) -> Result<Config, String>
where
    I: Iterator<Item = String>,
{
    let destination = args
        .next()
        .ok_or_else(|| "Missing runtime address.".to_string())?
        .parse::<SocketAddr>()
        .map_err(|error| format!("Invalid runtime address: {error}"))?;

    let mut buttons = 0;
    let mut axis_x = 0;
    let mut axis_y = 0;
    let mut hold = DEFAULT_HOLD;

    while let Some(option) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("Missing value for {option}."))?;
        match option.as_str() {
            "--buttons" => buttons = parse_buttons(&value)?,
            "--axis-x" => axis_x = parse_axis(&value, "--axis-x")?,
            "--axis-y" => axis_y = parse_axis(&value, "--axis-y")?,
            "--hold-ms" => {
                let milliseconds = value
                    .parse::<u64>()
                    .map_err(|error| format!("Invalid --hold-ms value: {error}"))?;
                if !(20..=10_000).contains(&milliseconds) {
                    return Err("--hold-ms must be between 20 and 10000.".to_string());
                }
                hold = Duration::from_millis(milliseconds);
            }
            _ => return Err(format!("Unknown option: {option}")),
        }
    }

    Ok(Config {
        destination,
        buttons,
        axis_x,
        axis_y,
        hold,
    })
}

fn parse_axis(value: &str, option: &str) -> Result<i16, String> {
    match value {
        "-1" => Ok(-i16::MAX),
        "0" => Ok(0),
        "1" => Ok(i16::MAX),
        _ => Err(format!("{option} must be -1, 0, or 1.")),
    }
}

fn parse_buttons(value: &str) -> Result<u16, String> {
    if value.eq_ignore_ascii_case("none") || value.is_empty() {
        return Ok(0);
    }

    value.split(',').try_fold(0, |mask, name| {
        let button = match name.to_ascii_lowercase().as_str() {
            "action1" | "south" => button::SOUTH,
            "action2" | "east" => button::EAST,
            "action3" | "north" => button::NORTH,
            "action4" | "west" => button::WEST,
            "action5" | "left-shoulder" => button::LEFT_SHOULDER,
            "action6" | "right-shoulder" => button::RIGHT_SHOULDER,
            "coin" => button::COIN,
            "start" => button::START,
            unknown => return Err(format!("Unknown button: {unknown}")),
        };
        Ok(mask | button)
    })
}

fn run(config: &Config) -> std::io::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect(config.destination)?;

    let mut sequence = 1_u32;
    let pressed = ControllerState {
        sequence,
        buttons: config.buttons,
        axis_x: config.axis_x,
        axis_y: config.axis_y,
        flags: 0,
        player_slot: 1,
    };
    let started = Instant::now();

    loop {
        socket.send(
            &ControllerState {
                sequence,
                ..pressed
            }
            .encode(),
        )?;
        sequence = sequence.wrapping_add(1);
        if started.elapsed() >= config.hold {
            break;
        }
        thread::sleep(HEARTBEAT_INTERVAL.min(config.hold.saturating_sub(started.elapsed())));
    }

    for _ in 0..3 {
        socket.send(
            &ControllerState {
                sequence,
                player_slot: 1,
                ..ControllerState::default()
            }
            .encode(),
        )?;
        sequence = sequence.wrapping_add(1);
        thread::sleep(HEARTBEAT_INTERVAL);
    }

    println!(
        "Injected buttons=0x{:04x} axis=({}, {}) for {} ms to {}",
        config.buttons,
        config.axis_x.signum(),
        config.axis_y.signum(),
        config.hold.as_millis(),
        config.destination
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combined_arcade_buttons_and_axes() {
        let config = parse_args(
            [
                "127.0.0.1:42001",
                "--buttons",
                "action1,action6,start",
                "--axis-x",
                "-1",
                "--axis-y",
                "1",
                "--hold-ms",
                "500",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .unwrap();

        assert_eq!(config.destination, "127.0.0.1:42001".parse().unwrap());
        assert_eq!(
            config.buttons,
            button::SOUTH | button::RIGHT_SHOULDER | button::START
        );
        assert_eq!(config.axis_x, -i16::MAX);
        assert_eq!(config.axis_y, i16::MAX);
        assert_eq!(config.hold, Duration::from_millis(500));
    }

    #[test]
    fn rejects_unknown_buttons() {
        assert!(parse_buttons("action7").is_err());
    }
}
