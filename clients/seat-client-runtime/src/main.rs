use std::{
    env, io,
    net::{SocketAddr, UdpSocket},
    path::PathBuf,
    process::{Child, Command, Stdio},
    str::FromStr,
    thread,
    time::Duration,
};

use input_protocol::{
    AuthenticatedControllerState, ControllerState, FLAG_STOP, SessionToken, button,
};
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
    buttons: u16,
    axis_x: i16,
    axis_y: i16,
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
        "Sending 4-Play input to {} as player {} for {} second(s): buttons=0x{:04x} axis=({}, {}).",
        command.destination,
        command.player,
        command.seconds,
        command.buttons,
        command.axis_x,
        command.axis_y
    );
    for sequence in 1..=ticks {
        send_input_packet(&socket, command.packet(sequence, 0))?;
        thread::sleep(interval);
    }
    if command.stop {
        send_input_packet(&socket, command.packet(ticks.saturating_add(1), FLAG_STOP))?;
        println!("Sent stop packet.");
    }
    Ok(())
}

impl InputCommand {
    fn packet(&self, sequence: u64, flags: u8) -> InputPacket {
        InputPacket {
            destination: self.destination,
            token: self.token,
            state: ControllerState {
                sequence: sequence.try_into().unwrap_or(u32::MAX),
                buttons: self.buttons,
                axis_x: self.axis_x,
                axis_y: self.axis_y,
                flags,
                player_slot: self.player,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InputPacket {
    destination: SocketAddr,
    token: SessionToken,
    state: ControllerState,
}

impl InputPacket {
    fn encode(self) -> [u8; input_protocol::AUTHENTICATED_PACKET_SIZE] {
        AuthenticatedControllerState {
            token: self.token,
            state: self.state,
        }
        .encode()
    }
}

fn send_input_packet(socket: &UdpSocket, packet: InputPacket) -> io::Result<()> {
    socket.send_to(&packet.encode(), packet.destination)?;
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
            let mut buttons = 0;
            let mut axis_x = 0;
            let mut axis_y = 0;
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
                    "--buttons" => {
                        buttons = parse_button_mask(value(&mut args, &option)?)?;
                    }
                    "--axis-x" => {
                        axis_x = parse_axis(value(&mut args, &option)?, "axis-x")?;
                    }
                    "--axis-y" => {
                        axis_y = parse_axis(value(&mut args, &option)?, "axis-y")?;
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
                buttons,
                axis_x,
                axis_y,
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

fn parse_axis(value: String, field: &str) -> Result<i16, String> {
    value
        .parse::<i16>()
        .map_err(|_| format!("invalid {field} value {value}"))
        .and_then(|axis| {
            if (-1..=1).contains(&axis) {
                Ok(axis)
            } else {
                Err(format!("{field} must be -1, 0, or 1"))
            }
        })
}

fn parse_button_mask(value: String) -> Result<u16, String> {
    let mut buttons = 0;
    for name in value
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        buttons |= match name.to_ascii_lowercase().as_str() {
            "action1" | "b1" | "south" | "attack" => button::ACTION_1,
            "action2" | "b2" | "east" | "jump" => button::ACTION_2,
            "action3" | "b3" | "north" => button::ACTION_3,
            "action4" | "b4" | "west" => button::ACTION_4,
            "action5" | "b5" | "tl" | "left-shoulder" => button::ACTION_5,
            "action6" | "b6" | "tr" | "right-shoulder" => button::ACTION_6,
            "coin" | "select" => button::COIN,
            "start" => button::START,
            other => return Err(format!("unknown button {other}")),
        };
    }
    Ok(buttons)
}

fn value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{option} requires a value"))
}

fn usage(program: &str) -> String {
    format!(
        "Usage:\n  {program} media --port <udp-port> [--ffplay-path <path>]\n  {program} input --destination <host:port> --token <uuid> --player <number> [--seconds <seconds>] [--buttons <names>] [--axis-x <-1|0|1>] [--axis-y <-1|0|1>] [--stop]\n\nInput buttons:\n  action1/b1/attack, action2/b2/jump, action3/b3, action4/b4, action5/b5, action6/b6, coin/select, start\n\nEnvironment:\n  FOURPLAY_FFPLAY_PATH    Default ffplay executable path."
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
            "--buttons".to_owned(),
            "attack,jump,start".to_owned(),
            "--axis-x".to_owned(),
            "1".to_owned(),
            "--axis-y".to_owned(),
            "-1".to_owned(),
            "--stop".to_owned(),
        ])
        .unwrap();

        match config.command {
            RuntimeCommand::Input(command) => {
                assert_eq!(command.destination, "192.0.2.68:42000".parse().unwrap());
                assert_eq!(command.player, 2);
                assert_eq!(command.seconds, 3);
                assert_eq!(
                    command.buttons,
                    button::ACTION_1 | button::ACTION_2 | button::START
                );
                assert_eq!(command.axis_x, 1);
                assert_eq!(command.axis_y, -1);
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
    fn rejects_unknown_button_name() {
        assert!(
            parse_args([
                "input".to_owned(),
                "--destination".to_owned(),
                "192.0.2.68:42000".to_owned(),
                "--token".to_owned(),
                "00112233-4455-6677-8899-aabbccddeeff".to_owned(),
                "--player".to_owned(),
                "1".to_owned(),
                "--buttons".to_owned(),
                "turbo".to_owned(),
            ])
            .unwrap_err()
            .contains("unknown button")
        );
    }

    #[test]
    fn builds_authenticated_input_packet() {
        let command = InputCommand {
            destination: "192.0.2.68:42000".parse().unwrap(),
            token: "00112233-4455-6677-8899-aabbccddeeff".parse().unwrap(),
            player: 3,
            seconds: 1,
            buttons: button::ACTION_1 | button::START,
            axis_x: -1,
            axis_y: 1,
            stop: false,
        };

        let packet = command.packet(42, 0);
        assert_eq!(packet.destination, "192.0.2.68:42000".parse().unwrap());
        assert_eq!(
            AuthenticatedControllerState::decode(&packet.encode())
                .unwrap()
                .state,
            ControllerState {
                sequence: 42,
                buttons: button::ACTION_1 | button::START,
                axis_x: -1,
                axis_y: 1,
                flags: 0,
                player_slot: 3,
            }
        );
    }

    #[test]
    fn usage_mentions_media_command() {
        assert!(usage("seat-client-runtime").contains("media --port"));
        assert!(usage("seat-client-runtime").contains("input --destination"));
    }
}
