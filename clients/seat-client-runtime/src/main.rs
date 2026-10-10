use std::{
    env, io,
    net::{SocketAddr, UdpSocket},
    path::PathBuf,
    process::{Child, Command, Stdio},
    str::FromStr,
    thread,
    time::Duration,
};

use input_protocol::{AuthenticatedControllerState, ControllerState, FLAG_STOP, SessionToken};
use seat_client_core::media_receiver_plan;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeConfig {
    command: RuntimeCommand,
    ffplay_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeCommand {
    Media { port: u16 },
    Input(InputCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InputCommand {
    destination: SocketAddr,
    token: SessionToken,
    player: u8,
    seconds: u64,
    stop: bool,
}

fn main() {
    let program = env::args()
        .next()
        .unwrap_or_else(|| "seat-client-runtime".to_owned());
    match parse_args(env::args().skip(1)) {
        Ok(config) => {
            if let Err(error) = run(config) {
                eprintln!("Error: {error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{error}\n\n{}", usage(&program));
            std::process::exit(2);
        }
    }
}

fn run(config: RuntimeConfig) -> io::Result<()> {
    match config.command {
        RuntimeCommand::Media { port } => {
            let plan = media_receiver_plan(port);
            println!("Starting 4-Play media receiver on UDP port {port}.");
            println!("Receiver URL: {}", plan.receiver_url);
            println!("Press Ctrl+C to stop the receiver.");
            let mut child = spawn_ffplay(&config.ffplay_path, &plan.ffplay_args)?;
            loop {
                if let Some(status) = child.try_wait()? {
                    println!("ffplay exited with {status}.");
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(250));
            }
        }
        RuntimeCommand::Input(command) => run_input(command),
    }
}

fn run_input(command: InputCommand) -> io::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let interval = Duration::from_millis(16);
    let ticks = command.seconds.saturating_mul(1000).div_ceil(16);
    println!(
        "Sending neutral 4-Play input to {} as player {} for {} second(s).",
        command.destination, command.player, command.seconds
    );
    for sequence in 1..=ticks {
        send_controller_state(
            &socket,
            command.destination,
            command.token,
            ControllerState {
                sequence: sequence.try_into().unwrap_or(u32::MAX),
                player_slot: command.player,
                ..ControllerState::default()
            },
        )?;
        thread::sleep(interval);
    }
    if command.stop {
        send_controller_state(
            &socket,
            command.destination,
            command.token,
            ControllerState {
                sequence: ticks.saturating_add(1).try_into().unwrap_or(u32::MAX),
                flags: FLAG_STOP,
                player_slot: command.player,
                ..ControllerState::default()
            },
        )?;
        println!("Sent stop packet.");
    }
    Ok(())
}

fn send_controller_state(
    socket: &UdpSocket,
    destination: SocketAddr,
    token: SessionToken,
    state: ControllerState,
) -> io::Result<()> {
    socket.send_to(
        &AuthenticatedControllerState { token, state }.encode(),
        destination,
    )?;
    Ok(())
}

fn spawn_ffplay(path: &PathBuf, args: &[String]) -> io::Result<Child> {
    Command::new(path).args(args).stdin(Stdio::null()).spawn()
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<RuntimeConfig, String> {
    let mut args = args.into_iter().peekable();
    let mut ffplay_path = env::var("FOURPLAY_FFPLAY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("ffplay"));
    let Some(command) = args.next() else {
        return Err("missing command".to_owned());
    };
    let command = match command.as_str() {
        "media" => {
            let mut port = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--port" => {
                        port = Some(parse_port(value(&mut args, &option)?)?);
                    }
                    "--ffplay-path" => {
                        ffplay_path = PathBuf::from(value(&mut args, &option)?);
                    }
                    "--help" | "-h" => return Err("help requested".to_owned()),
                    other => return Err(format!("unknown option {other}")),
                }
            }
            RuntimeCommand::Media {
                port: port.ok_or_else(|| "missing --port <udp-port>".to_owned())?,
            }
        }
        "input" => {
            let mut destination = None;
            let mut token = None;
            let mut player = None;
            let mut seconds = 1;
            let mut stop = false;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--destination" => {
                        destination = Some(parse_socket_addr(value(&mut args, &option)?)?);
                    }
                    "--token" => {
                        token = Some(parse_token(value(&mut args, &option)?)?);
                    }
                    "--player" => {
                        player = Some(parse_player(value(&mut args, &option)?)?);
                    }
                    "--seconds" => {
                        seconds = parse_seconds(value(&mut args, &option)?)?;
                    }
                    "--stop" => {
                        stop = true;
                    }
                    "--help" | "-h" => return Err("help requested".to_owned()),
                    other => return Err(format!("unknown option {other}")),
                }
            }
            RuntimeCommand::Input(InputCommand {
                destination: destination
                    .ok_or_else(|| "missing --destination <host:port>".to_owned())?,
                token: token.ok_or_else(|| "missing --token <session-token>".to_owned())?,
                player: player.ok_or_else(|| "missing --player <player-number>".to_owned())?,
                seconds,
                stop,
            })
        }
        "--help" | "-h" => return Err("help requested".to_owned()),
        other => return Err(format!("unknown command {other}")),
    };
    Ok(RuntimeConfig {
        command,
        ffplay_path,
    })
}

fn parse_port(value: String) -> Result<u16, String> {
    value
        .parse::<u16>()
        .map_err(|_| format!("invalid UDP port {value}"))
        .and_then(|port| {
            if port == 0 {
                Err("UDP port must be greater than zero".to_owned())
            } else {
                Ok(port)
            }
        })
}

fn parse_socket_addr(value: String) -> Result<SocketAddr, String> {
    value
        .parse::<SocketAddr>()
        .map_err(|_| format!("invalid socket address {value}"))
}

fn parse_token(value: String) -> Result<SessionToken, String> {
    SessionToken::from_str(&value).map_err(|error| error.to_string())
}

fn parse_player(value: String) -> Result<u8, String> {
    value
        .parse::<u8>()
        .map_err(|_| format!("invalid player number {value}"))
        .and_then(|player| {
            if player == 0 {
                Err("player number must be greater than zero".to_owned())
            } else {
                Ok(player)
            }
        })
}

fn parse_seconds(value: String) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("invalid seconds value {value}"))
        .and_then(|seconds| {
            if seconds == 0 || seconds > 86_400 {
                Err("seconds must be between 1 and 86400".to_owned())
            } else {
                Ok(seconds)
            }
        })
}

fn value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{option} requires a value"))
}

fn usage(program: &str) -> String {
    format!(
        "Usage:\n  {program} media --port <udp-port> [--ffplay-path <path>]\n  {program} input --destination <host:port> --token <uuid> --player <number> [--seconds <seconds>] [--stop]\n\nEnvironment:\n  FOURPLAY_FFPLAY_PATH    Default ffplay executable path."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_media_command() {
        let config = parse_args([
            "media".to_owned(),
            "--port".to_owned(),
            "41000".to_owned(),
            "--ffplay-path".to_owned(),
            "ffplay-custom".to_owned(),
        ])
        .unwrap();

        assert_eq!(
            config,
            RuntimeConfig {
                command: RuntimeCommand::Media { port: 41_000 },
                ffplay_path: PathBuf::from("ffplay-custom"),
            }
        );
    }

    #[test]
    fn rejects_missing_port() {
        assert!(
            parse_args(["media".to_owned()])
                .unwrap_err()
                .contains("missing --port")
        );
    }

    #[test]
    fn rejects_zero_port() {
        assert!(
            parse_args(["media".to_owned(), "--port".to_owned(), "0".to_owned()])
                .unwrap_err()
                .contains("greater than zero")
        );
    }

    #[test]
    fn parses_input_command() {
        let config = parse_args([
            "input".to_owned(),
            "--destination".to_owned(),
            "192.0.2.68:42000".to_owned(),
            "--token".to_owned(),
            "00112233-4455-6677-8899-aabbccddeeff".to_owned(),
            "--player".to_owned(),
            "2".to_owned(),
            "--seconds".to_owned(),
            "3".to_owned(),
            "--stop".to_owned(),
        ])
        .unwrap();

        match config.command {
            RuntimeCommand::Input(command) => {
                assert_eq!(command.destination, "192.0.2.68:42000".parse().unwrap());
                assert_eq!(command.player, 2);
                assert_eq!(command.seconds, 3);
                assert!(command.stop);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_input_token() {
        assert!(
            parse_args([
                "input".to_owned(),
                "--destination".to_owned(),
                "192.0.2.68:42000".to_owned(),
                "--player".to_owned(),
                "1".to_owned(),
            ])
            .unwrap_err()
            .contains("missing --token")
        );
    }

    #[test]
    fn usage_mentions_media_command() {
        assert!(usage("seat-client-runtime").contains("media --port"));
        assert!(usage("seat-client-runtime").contains("input --destination"));
    }
}
