use std::{
    collections::HashSet,
    env, fs, io,
    net::{SocketAddr, UdpSocket},
    path::PathBuf,
    process::{Child, Command, Stdio},
    str::FromStr,
    thread,
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use input_protocol::{
    AuthenticatedControllerState, ControllerState, FLAG_STOP, SessionToken, button,
};
use seat_client_core::{media_receiver_plan, public_player_handoff, public_spectator_handoff};

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeConfig {
    command: RuntimeCommand,
    ffplay_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeCommand {
    Media { port: u16 },
    Input(InputCommand),
    Keyboard(KeyboardCommand),
    PublicPlayerHandoff(PublicPlayerHandoffCommand),
    PublicSpectatorHandoff(PublicSpectatorHandoffCommand),
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyboardCommand {
    destination: SocketAddr,
    token: SessionToken,
    player: u8,
    debug: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicPlayerHandoffCommand {
    session_json: PathBuf,
    player: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicSpectatorHandoffCommand {
    session_json: PathBuf,
    spectator_grant_json: PathBuf,
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
        RuntimeCommand::Keyboard(command) => run_keyboard(command),
        RuntimeCommand::PublicPlayerHandoff(command) => run_public_player_handoff(command),
        RuntimeCommand::PublicSpectatorHandoff(command) => run_public_spectator_handoff(command),
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

fn run_keyboard(command: KeyboardCommand) -> io::Result<()> {
    let _raw_mode = RawModeGuard::enable()?;
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let interval = Duration::from_millis(16);
    let mut held = HashSet::new();
    let mut sequence = 0_u32;
    println!(
        "Forwarding keyboard input to {} as player {}. Press Esc to stop.",
        command.destination, command.player
    );
    println!("W/A/S/D move; J/K/L high attacks; M/,/. low attacks; 1 coin; 2 start");

    loop {
        if event::poll(interval)?
            && let Event::Key(key) = event::read()?
        {
            if key.code == KeyCode::Esc && key.kind == KeyEventKind::Press {
                sequence = sequence.wrapping_add(1);
                send_input_packet(
                    &socket,
                    keyboard_packet(
                        command.destination,
                        command.token,
                        &held,
                        sequence,
                        FLAG_STOP,
                        command.player,
                    ),
                )?;
                println!("Stopped keyboard input forwarding.");
                return Ok(());
            }
            update_held_keys(&mut held, key);
        }

        sequence = sequence.wrapping_add(1);
        let packet = keyboard_packet(
            command.destination,
            command.token,
            &held,
            sequence,
            0,
            command.player,
        );
        if command.debug {
            println!("{}", debug_state(&held, packet.state));
        }
        send_input_packet(&socket, packet)?;
    }
}

struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

fn keyboard_packet(
    destination: SocketAddr,
    token: SessionToken,
    held: &HashSet<KeyCode>,
    sequence: u32,
    flags: u8,
    player_slot: u8,
) -> InputPacket {
    InputPacket {
        destination,
        token,
        state: state_from_keys(held, sequence, flags, player_slot),
    }
}

fn update_held_keys(held: &mut HashSet<KeyCode>, key: KeyEvent) -> bool {
    let code = normalize_key(key.code);
    if !is_control_key(code) {
        return false;
    }
    match key.kind {
        KeyEventKind::Press => held.insert(code),
        KeyEventKind::Release => held.remove(&code),
        KeyEventKind::Repeat => false,
    }
}

fn normalize_key(key: KeyCode) -> KeyCode {
    match key {
        KeyCode::Char(character) => KeyCode::Char(character.to_ascii_lowercase()),
        other => other,
    }
}

fn is_control_key(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::Char('w' | 'a' | 's' | 'd' | 'j' | 'k' | 'l' | 'm' | ',' | '.' | '1' | '2')
    )
}

fn state_from_keys(
    held: &HashSet<KeyCode>,
    sequence: u32,
    flags: u8,
    player_slot: u8,
) -> ControllerState {
    let is_held = |character| held.contains(&KeyCode::Char(character));
    let axis_x = (i16::from(is_held('d')) - i16::from(is_held('a'))) * i16::MAX;
    let axis_y = (i16::from(is_held('s')) - i16::from(is_held('w'))) * i16::MAX;
    let mut buttons = 0;
    for (key, mask) in [
        ('j', button::ACTION_1),
        ('k', button::ACTION_2),
        ('l', button::ACTION_3),
        ('m', button::ACTION_4),
        (',', button::ACTION_5),
        ('.', button::ACTION_6),
        ('1', button::COIN),
        ('2', button::START),
    ] {
        if is_held(key) {
            buttons |= mask;
        }
    }
    ControllerState {
        sequence,
        buttons,
        axis_x,
        axis_y,
        flags,
        player_slot,
    }
}

fn debug_state(held: &HashSet<KeyCode>, state: ControllerState) -> String {
    format!(
        "input seq={} axis=({}, {}) buttons=0x{:04x} keys={}",
        state.sequence,
        state.axis_x.signum(),
        state.axis_y.signum(),
        state.buttons,
        debug_keys(held)
    )
}

fn debug_keys(held: &HashSet<KeyCode>) -> String {
    let keys = ['w', 'a', 's', 'd', 'j', 'k', 'l', 'm', ',', '.', '1', '2']
        .into_iter()
        .filter(|character| held.contains(&KeyCode::Char(*character)))
        .collect::<String>();
    if keys.is_empty() {
        "none".to_owned()
    } else {
        keys
    }
}

fn spawn_ffplay(path: &PathBuf, args: &[String]) -> io::Result<Child> {
    Command::new(path).args(args).stdin(Stdio::null()).spawn()
}

fn run_public_player_handoff(command: PublicPlayerHandoffCommand) -> io::Result<()> {
    let session = read_json_file::<control_protocol::Session>(&command.session_json)?;
    let handoff = public_player_handoff(&session, command.player)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    print_json(&handoff)
}

fn run_public_spectator_handoff(command: PublicSpectatorHandoffCommand) -> io::Result<()> {
    let session = read_json_file::<control_protocol::Session>(&command.session_json)?;
    let grant = read_json_file::<control_protocol::SpectatorGrant>(&command.spectator_grant_json)?;
    let handoff = public_spectator_handoff(&session, &grant);
    print_json(&handoff)
}

fn read_json_file<T: serde::de::DeserializeOwned>(path: &PathBuf) -> io::Result<T> {
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn print_json<T: serde::Serialize>(value: &T) -> io::Result<()> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    println!("{json}");
    Ok(())
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
        "keyboard" => {
            let mut destination = None;
            let mut token = None;
            let mut player = None;
            let mut debug = false;
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
                    "--debug-input" => {
                        debug = true;
                    }
                    "--help" | "-h" => return Err("help requested".to_owned()),
                    other => return Err(format!("unknown option {other}")),
                }
            }
            RuntimeCommand::Keyboard(KeyboardCommand {
                destination: destination
                    .ok_or_else(|| "missing --destination <host:port>".to_owned())?,
                token: token.ok_or_else(|| "missing --token <session-token>".to_owned())?,
                player: player.ok_or_else(|| "missing --player <player-number>".to_owned())?,
                debug,
            })
        }
        "handoff-player" => {
            let mut session_json = None;
            let mut player = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--session-json" => {
                        session_json = Some(PathBuf::from(value(&mut args, &option)?));
                    }
                    "--player" => {
                        player = Some(parse_player_u32(value(&mut args, &option)?)?);
                    }
                    "--help" | "-h" => return Err("help requested".to_owned()),
                    other => return Err(format!("unknown option {other}")),
                }
            }
            RuntimeCommand::PublicPlayerHandoff(PublicPlayerHandoffCommand {
                session_json: session_json
                    .ok_or_else(|| "missing --session-json <path>".to_owned())?,
                player: player.ok_or_else(|| "missing --player <player-number>".to_owned())?,
            })
        }
        "handoff-spectator" => {
            let mut session_json = None;
            let mut spectator_grant_json = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--session-json" => {
                        session_json = Some(PathBuf::from(value(&mut args, &option)?));
                    }
                    "--spectator-grant-json" => {
                        spectator_grant_json = Some(PathBuf::from(value(&mut args, &option)?));
                    }
                    "--help" | "-h" => return Err("help requested".to_owned()),
                    other => return Err(format!("unknown option {other}")),
                }
            }
            RuntimeCommand::PublicSpectatorHandoff(PublicSpectatorHandoffCommand {
                session_json: session_json
                    .ok_or_else(|| "missing --session-json <path>".to_owned())?,
                spectator_grant_json: spectator_grant_json
                    .ok_or_else(|| "missing --spectator-grant-json <path>".to_owned())?,
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

fn parse_player_u32(value: String) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("invalid player number {value}"))
        .and_then(|player| {
            if player == 0 || player > u32::from(u8::MAX) {
                Err("player number must be between 1 and 255".to_owned())
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
        "Usage:\n  {program} media --port <udp-port> [--ffplay-path <path>]\n  {program} input --destination <host:port> --token <uuid> --player <number> [--seconds <seconds>] [--buttons <names>] [--axis-x <-1|0|1>] [--axis-y <-1|0|1>] [--stop]\n  {program} keyboard --destination <host:port> --token <uuid> --player <number> [--debug-input]\n  {program} handoff-player --session-json <path> --player <number>\n  {program} handoff-spectator --session-json <path> --spectator-grant-json <path>\n\nInput buttons:\n  action1/b1/attack, action2/b2/jump, action3/b3, action4/b4, action5/b5, action6/b6, coin/select, start\n\nKeyboard map:\n  W/A/S/D move; J/K/L high attacks; M/,/. low attacks; 1 coin; 2 start; Esc stops.\n\nEnvironment:\n  FOURPLAY_FFPLAY_PATH    Default ffplay executable path."
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
    fn parses_keyboard_command() {
        let config = parse_args([
            "keyboard".to_owned(),
            "--destination".to_owned(),
            "192.0.2.68:42000".to_owned(),
            "--token".to_owned(),
            "00112233-4455-6677-8899-aabbccddeeff".to_owned(),
            "--player".to_owned(),
            "4".to_owned(),
            "--debug-input".to_owned(),
        ])
        .unwrap();

        match config.command {
            RuntimeCommand::Keyboard(command) => {
                assert_eq!(command.destination, "192.0.2.68:42000".parse().unwrap());
                assert_eq!(command.player, 4);
                assert!(command.debug);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_public_player_handoff_command() {
        let config = parse_args([
            "handoff-player".to_owned(),
            "--session-json".to_owned(),
            "session.json".to_owned(),
            "--player".to_owned(),
            "2".to_owned(),
        ])
        .unwrap();

        match config.command {
            RuntimeCommand::PublicPlayerHandoff(command) => {
                assert_eq!(command.session_json, PathBuf::from("session.json"));
                assert_eq!(command.player, 2);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_public_spectator_handoff_command() {
        let config = parse_args([
            "handoff-spectator".to_owned(),
            "--session-json".to_owned(),
            "session.json".to_owned(),
            "--spectator-grant-json".to_owned(),
            "grant.json".to_owned(),
        ])
        .unwrap();

        match config.command {
            RuntimeCommand::PublicSpectatorHandoff(command) => {
                assert_eq!(command.session_json, PathBuf::from("session.json"));
                assert_eq!(command.spectator_grant_json, PathBuf::from("grant.json"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
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
    fn simultaneous_keys_produce_combined_state() {
        let held = HashSet::from([
            KeyCode::Char('w'),
            KeyCode::Char('d'),
            KeyCode::Char('j'),
            KeyCode::Char('k'),
        ]);
        let state = state_from_keys(&held, 7, 0, 1);
        assert_eq!(state.axis_x, i16::MAX);
        assert_eq!(state.axis_y, -i16::MAX);
        assert_eq!(state.buttons, button::ACTION_1 | button::ACTION_2);
    }

    #[test]
    fn debug_state_lists_axis_buttons_and_keys() {
        let held = HashSet::from([KeyCode::Char('d'), KeyCode::Char('j')]);
        let state = state_from_keys(&held, 42, 0, 1);

        assert_eq!(
            debug_state(&held, state),
            "input seq=42 axis=(1, 0) buttons=0x0001 keys=dj"
        );
    }

    #[test]
    fn usage_mentions_media_command() {
        assert!(usage("seat-client-runtime").contains("media --port"));
        assert!(usage("seat-client-runtime").contains("input --destination"));
        assert!(usage("seat-client-runtime").contains("handoff-player"));
    }
}
