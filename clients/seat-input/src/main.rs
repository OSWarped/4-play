use control_protocol::{
    CatalogGame, CatalogGameList, CreateSessionRequest, PlayerSlot, PlayerSlotState,
    ReservePlayerSlotRequest, RuntimeHostStatus, Session, SessionList, SessionState,
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use input_protocol::{
    AuthenticatedControllerState, ControllerState, FLAG_STOP, SessionToken, button,
};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use std::collections::HashSet;
use std::env;
use std::io::{self, Write};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::process::{self, Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(50);
const SESSION_POLL_INTERVAL: Duration = Duration::from_millis(100);
const SESSION_START_TIMEOUT: Duration = Duration::from_secs(30);

enum Mode {
    Direct(SocketAddr),
    Orchestrated(SeatConfig),
}

struct SeatConfig {
    control_plane_url: String,
    seat_id: String,
    destination_address: IpAddr,
    game_id: Option<String>,
    ffplay_path: String,
    no_media: bool,
    play_for: Option<Duration>,
    api_token: String,
    debug_input: bool,
}

enum BrowseSelection {
    StartGame(String),
    JoinSlot(PlayTarget),
    Quit,
}

struct PlayTarget {
    session_id: String,
    player_number: u8,
    stop_session_on_exit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputExit {
    UserRequested,
    ExternallyStopped,
    Completed,
}

struct RawMode;

impl RawMode {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if let Err(error) = disable_raw_mode() {
            eprintln!("Failed to restore terminal: {error}");
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match parse_mode() {
        Mode::Direct(destination) => run_direct(destination),
        Mode::Orchestrated(config) => run_orchestrated(config),
    }
}

fn run_direct(destination: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    println!("4-Play direct seat input -> {destination}");
    println!("Press Esc to disconnect and stop the development session.");
    run_controller(destination, None, 1, true, false, || false).map(|_| ())
}

fn run_orchestrated(config: SeatConfig) -> Result<(), Box<dyn std::error::Error>> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", config.api_token))?,
    );
    let client = Client::builder()
        .timeout(Duration::from_secs(3))
        .default_headers(headers)
        .build()?;
    let one_shot = config.game_id.is_some();
    let mut selected_game = config.game_id.clone();

    loop {
        let games = fetch_available_games(&client, &config.control_plane_url)?;
        let sessions = fetch_active_sessions(&client, &config.control_plane_url)?;
        let target = if let Some(game_id) = selected_game.take() {
            let game_id = validate_requested_game(&games, game_id)?;
            let session = create_session(&client, &config, &game_id)?;
            println!("Session requested: {}", session.id);
            PlayTarget {
                session_id: session.id,
                player_number: 1,
                stop_session_on_exit: true,
            }
        } else {
            match select_browse_action(&sessions, &games)? {
                BrowseSelection::StartGame(game_id) => {
                    let session = create_session(&client, &config, &game_id)?;
                    println!("Session requested: {}", session.id);
                    PlayTarget {
                        session_id: session.id,
                        player_number: 1,
                        stop_session_on_exit: true,
                    }
                }
                BrowseSelection::JoinSlot(target) => {
                    let session = reserve_player_slot(
                        &client,
                        &config.control_plane_url,
                        &target.session_id,
                        target.player_number,
                        &config.seat_id,
                    )?;
                    println!(
                        "Reserved player {} in session {}.",
                        target.player_number, session.id
                    );
                    let session = connect_player_slot(
                        &client,
                        &config.control_plane_url,
                        &target.session_id,
                        target.player_number,
                        &config.seat_id,
                    )?;
                    println!(
                        "Connected player {} in session {}.",
                        target.player_number, session.id
                    );
                    target
                }
                BrowseSelection::Quit => {
                    println!("Seat client stopped.");
                    return Ok(());
                }
            }
        };

        let active = match wait_for_active(&client, &config.control_plane_url, &target.session_id) {
            Ok(session) => session,
            Err(error) => {
                eprintln!("Session did not start: {error}");
                if one_shot {
                    return Err(error);
                }
                continue;
            }
        };
        if active.connection_grant.expires_unix_ms <= unix_time_ms() {
            request_stop(&client, &config.control_plane_url, &active.id)?;
            return Err("connection grant expired before the session became active".into());
        }

        let input_destination = format!(
            "{}:{}",
            active.connection_grant.runtime_host_address, active.connection_grant.input_udp_port
        )
        .parse::<SocketAddr>()?;
        let input_token = active.connection_grant.token.parse::<SessionToken>()?;
        let mut media = if config.no_media {
            None
        } else {
            match spawn_ffplay(&config.ffplay_path, active.connection_grant.media_udp_port) {
                Ok(child) => Some(child),
                Err(error) => {
                    let _ = request_stop(&client, &config.control_plane_url, &active.id);
                    return Err(error.into());
                }
            }
        };

        println!(
            "Playing {} as player {} through host {}. Press Esc to return to browsing.",
            active.game_id, target.player_number, active.runtime_host_id
        );
        let runtime_ended = Arc::new(AtomicBool::new(false));
        let monitor = spawn_session_monitor(
            client.clone(),
            config.control_plane_url.clone(),
            active.id.clone(),
            Arc::clone(&runtime_ended),
        );
        let mut stopped = || {
            runtime_ended.load(Ordering::Acquire)
                || media
                    .as_mut()
                    .and_then(|child| child.try_wait().ok().flatten())
                    .is_some()
        };
        let input_exit = if let Some(duration) = config.play_for {
            run_automated_controller(
                input_destination,
                input_token,
                target.player_number,
                duration,
                &mut stopped,
            )
        } else {
            run_controller(
                input_destination,
                Some(input_token),
                target.player_number,
                target.stop_session_on_exit,
                config.debug_input,
                &mut stopped,
            )
        }?;

        if target.stop_session_on_exit && !runtime_ended.load(Ordering::Acquire) {
            request_stop(&client, &config.control_plane_url, &active.id)?;
        } else if !target.stop_session_on_exit {
            match input_exit {
                InputExit::UserRequested | InputExit::Completed => {
                    release_player_slot(
                        &client,
                        &config.control_plane_url,
                        &active.id,
                        target.player_number,
                        &config.seat_id,
                    )?;
                }
                InputExit::ExternallyStopped => {
                    disconnect_player_slot(
                        &client,
                        &config.control_plane_url,
                        &active.id,
                        target.player_number,
                        &config.seat_id,
                    )?;
                }
            }
        }
        runtime_ended.store(true, Ordering::Release);
        let _ = monitor.join();
        stop_media(&mut media);

        if target.stop_session_on_exit {
            let final_session = wait_for_terminal(&client, &config.control_plane_url, &active.id)?;
            println!(
                "Session {} ended in state {:?}; returning to browsing.",
                final_session.id, final_session.state
            );
        } else {
            println!(
                "Left session {} as player {}; returning to browsing.",
                active.id, target.player_number
            );
        }
        if one_shot {
            return Ok(());
        }
    }
}

fn fetch_available_games(
    client: &Client,
    control_plane_url: &str,
) -> Result<Vec<CatalogGame>, Box<dyn std::error::Error>> {
    let catalog = client
        .get(format!("{control_plane_url}/api/v1/games"))
        .send()?
        .error_for_status()?
        .json::<CatalogGameList>()?;
    Ok(catalog
        .games
        .into_iter()
        .filter(|game| {
            game.availability
                .iter()
                .any(|entry| entry.runtime_host_status == RuntimeHostStatus::Online)
        })
        .collect())
}

fn fetch_active_sessions(
    client: &Client,
    control_plane_url: &str,
) -> Result<Vec<Session>, Box<dyn std::error::Error>> {
    let sessions = client
        .get(format!("{control_plane_url}/api/v1/sessions"))
        .send()?
        .error_for_status()?
        .json::<SessionList>()?;
    Ok(sessions
        .sessions
        .into_iter()
        .filter(|session| !session.state.is_terminal())
        .collect())
}

fn print_active_sessions(sessions: &[Session], games: &[CatalogGame]) {
    if sessions.is_empty() {
        return;
    }

    println!("\nActive sessions:");
    for (index, session) in sessions.iter().enumerate() {
        let game_name = games
            .iter()
            .find(|game| game.id == session.game_id)
            .map(|game| game.display_name.as_str())
            .unwrap_or(&session.game_id);
        println!(
            "  {}. {} ({}) on {} [{:?}]",
            index + 1,
            game_name,
            session.game_id,
            session.runtime_host_id,
            session.state
        );
        println!("     {}", describe_player_slots(&session.player_slots));
    }
    println!("  Reserve an open slot with j<session-number>.<player-number>, for example j1.2.");
}

fn describe_player_slots(slots: &[PlayerSlot]) -> String {
    if slots.is_empty() {
        return "player slots unavailable".to_owned();
    }
    slots
        .iter()
        .map(|slot| {
            let label = format!("P{}", slot.player_number);
            match slot.state {
                PlayerSlotState::Open => format!("{label} open"),
                PlayerSlotState::Reserved => describe_claimed_slot(&label, "reserved", slot),
                PlayerSlotState::Occupied => describe_claimed_slot(&label, "occupied", slot),
                PlayerSlotState::Disconnected => {
                    describe_claimed_slot(&label, "disconnected", slot)
                }
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn describe_claimed_slot(label: &str, state: &str, slot: &PlayerSlot) -> String {
    match slot.seat_id.as_deref() {
        Some(seat_id) => format!("{label} {state} by {seat_id}"),
        None => format!("{label} {state}"),
    }
}

fn select_browse_action(
    sessions: &[Session],
    games: &[CatalogGame],
) -> Result<BrowseSelection, Box<dyn std::error::Error>> {
    if games.is_empty() {
        return Err("no games are currently available on an online runtime host".into());
    }

    print_active_sessions(sessions, games);
    println!("\nAvailable games:");
    for (index, game) in games.iter().enumerate() {
        println!("  {}. {} ({})", index + 1, game.display_name, game.id);
    }
    print!("Choose a game number, j<session>.<player> to reserve, or q to quit: ");
    io::stdout().flush()?;
    let mut selection = String::new();
    io::stdin().read_line(&mut selection)?;
    let selection = selection.trim();
    if selection.eq_ignore_ascii_case("q") {
        return Ok(BrowseSelection::Quit);
    }
    if let Some((session_index, player_number)) = parse_reservation_selection(selection) {
        let Some(session) = sessions.get(session_index.saturating_sub(1)) else {
            return Err("session selection is outside the displayed range".into());
        };
        if !session.player_slots.iter().any(|slot| {
            slot.player_number == player_number && matches!(slot.state, PlayerSlotState::Open)
        }) {
            return Err("selected player slot is not open".into());
        }
        let player_number = u8::try_from(player_number)
            .map_err(|_| "selected player number is outside the supported range")?;
        return Ok(BrowseSelection::JoinSlot(PlayTarget {
            session_id: session.id.clone(),
            player_number,
            stop_session_on_exit: false,
        }));
    }
    let index = selection
        .parse::<usize>()
        .map_err(|_| "selection must be a game number, reservation, or q")?;
    games
        .get(index.saturating_sub(1))
        .map(|game| BrowseSelection::StartGame(game.id.clone()))
        .ok_or_else(|| "game selection is outside the displayed range".into())
}

fn validate_requested_game(
    games: &[CatalogGame],
    game_id: String,
) -> Result<String, Box<dyn std::error::Error>> {
    if games.iter().any(|game| game.id == game_id) {
        Ok(game_id)
    } else {
        Err(format!("game '{game_id}' is not currently available").into())
    }
}

fn parse_reservation_selection(selection: &str) -> Option<(usize, u32)> {
    let selection = selection.trim();
    let rest = selection
        .strip_prefix('j')
        .or_else(|| selection.strip_prefix('J'))?;
    let (session, player) = rest.split_once('.')?;
    let session_index = session.parse::<usize>().ok()?;
    let player_number = player.parse::<u32>().ok()?;
    if session_index == 0 || player_number == 0 {
        return None;
    }
    Some((session_index, player_number))
}

fn create_session(
    client: &Client,
    config: &SeatConfig,
    game_id: &str,
) -> Result<Session, Box<dyn std::error::Error>> {
    Ok(client
        .post(format!("{}/api/v1/sessions", config.control_plane_url))
        .json(&CreateSessionRequest {
            game_id: game_id.to_owned(),
            seat_id: config.seat_id.clone(),
            destination_address: config.destination_address.to_string(),
        })
        .send()?
        .error_for_status()?
        .json()?)
}

fn reserve_player_slot(
    client: &Client,
    control_plane_url: &str,
    session_id: &str,
    player_number: u8,
    seat_id: &str,
) -> Result<Session, Box<dyn std::error::Error>> {
    Ok(client
        .post(format!(
            "{control_plane_url}/api/v1/sessions/{session_id}/player-slots/{player_number}/reserve"
        ))
        .json(&ReservePlayerSlotRequest {
            seat_id: seat_id.to_owned(),
        })
        .send()?
        .error_for_status()?
        .json()?)
}

fn release_player_slot(
    client: &Client,
    control_plane_url: &str,
    session_id: &str,
    player_number: u8,
    seat_id: &str,
) -> Result<Session, Box<dyn std::error::Error>> {
    Ok(client
        .post(format!(
            "{control_plane_url}/api/v1/sessions/{session_id}/player-slots/{player_number}/release"
        ))
        .json(&ReservePlayerSlotRequest {
            seat_id: seat_id.to_owned(),
        })
        .send()?
        .error_for_status()?
        .json()?)
}

fn disconnect_player_slot(
    client: &Client,
    control_plane_url: &str,
    session_id: &str,
    player_number: u8,
    seat_id: &str,
) -> Result<Session, Box<dyn std::error::Error>> {
    Ok(client
        .post(format!(
            "{control_plane_url}/api/v1/sessions/{session_id}/player-slots/{player_number}/disconnect"
        ))
        .json(&ReservePlayerSlotRequest {
            seat_id: seat_id.to_owned(),
        })
        .send()?
        .error_for_status()?
        .json()?)
}

fn connect_player_slot(
    client: &Client,
    control_plane_url: &str,
    session_id: &str,
    player_number: u8,
    seat_id: &str,
) -> Result<Session, Box<dyn std::error::Error>> {
    Ok(client
        .post(format!(
            "{control_plane_url}/api/v1/sessions/{session_id}/player-slots/{player_number}/connect"
        ))
        .json(&ReservePlayerSlotRequest {
            seat_id: seat_id.to_owned(),
        })
        .send()?
        .error_for_status()?
        .json()?)
}

fn get_session(
    client: &Client,
    control_plane_url: &str,
    session_id: &str,
) -> Result<Session, reqwest::Error> {
    client
        .get(format!("{control_plane_url}/api/v1/sessions/{session_id}"))
        .send()?
        .error_for_status()?
        .json()
}

fn wait_for_active(
    client: &Client,
    control_plane_url: &str,
    session_id: &str,
) -> Result<Session, Box<dyn std::error::Error>> {
    let started = Instant::now();
    loop {
        let session = get_session(client, control_plane_url, session_id)?;
        if session.state == SessionState::Active {
            return Ok(session);
        }
        if session.state.is_terminal() {
            return Err(format!(
                "session entered {:?}: {}",
                session.state,
                session
                    .failure_reason
                    .as_deref()
                    .unwrap_or("no reason reported")
            )
            .into());
        }
        if started.elapsed() >= SESSION_START_TIMEOUT {
            let _ = request_stop(client, control_plane_url, session_id);
            return Err("session start timed out after 30 seconds".into());
        }
        thread::sleep(SESSION_POLL_INTERVAL);
    }
}

fn wait_for_terminal(
    client: &Client,
    control_plane_url: &str,
    session_id: &str,
) -> Result<Session, Box<dyn std::error::Error>> {
    let started = Instant::now();
    loop {
        let session = get_session(client, control_plane_url, session_id)?;
        if session.state.is_terminal() {
            return Ok(session);
        }
        if started.elapsed() >= SESSION_START_TIMEOUT {
            return Err("session stop timed out after 30 seconds".into());
        }
        thread::sleep(SESSION_POLL_INTERVAL);
    }
}

fn request_stop(
    client: &Client,
    control_plane_url: &str,
    session_id: &str,
) -> Result<Session, reqwest::Error> {
    client
        .post(format!(
            "{control_plane_url}/api/v1/sessions/{session_id}/stop"
        ))
        .send()?
        .error_for_status()?
        .json()
}

fn spawn_session_monitor(
    client: Client,
    control_plane_url: String,
    session_id: String,
    ended: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !ended.load(Ordering::Acquire) {
            match get_session(&client, &control_plane_url, &session_id) {
                Ok(session) if session.state.is_terminal() => {
                    ended.store(true, Ordering::Release);
                    break;
                }
                Ok(_) => {}
                Err(error) => eprintln!("Session monitor request failed: {error}"),
            }
            thread::sleep(Duration::from_millis(250));
        }
    })
}

fn spawn_ffplay(path: &str, port: u16) -> io::Result<Child> {
    let source = format!("udp://0.0.0.0:{port}?fifo_size=1000000&overrun_nonfatal=1");
    Command::new(path)
        .args([
            "-f",
            "mpegts",
            "-fflags",
            "nobuffer",
            "-flags",
            "low_delay",
            "-framedrop",
            "-probesize",
            "32768",
            "-analyzeduration",
            "0",
            &source,
        ])
        .stdin(Stdio::null())
        .spawn()
}

fn stop_media(media: &mut Option<Child>) {
    let Some(mut child) = media.take() else {
        return;
    };
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
}

fn run_controller<F>(
    destination: SocketAddr,
    input_token: Option<SessionToken>,
    player_slot: u8,
    send_stop_flag: bool,
    debug_input: bool,
    mut externally_stopped: F,
) -> Result<InputExit, Box<dyn std::error::Error>>
where
    F: FnMut() -> bool,
{
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect(destination)?;
    println!("4-Play seat input -> {destination}");
    println!("W/A/S/D move; J/K/L high attacks; M/,/. low attacks; 1 coin; 2 start");

    let _raw_mode = RawMode::enter()?;
    let mut held = HashSet::new();
    let mut sequence = 0_u32;
    let mut last_send = Instant::now() - HEARTBEAT_INTERVAL;
    let mut last_debug_state = None;

    loop {
        let wait = HEARTBEAT_INTERVAL.saturating_sub(last_send.elapsed());
        let mut changed = false;
        if event::poll(wait)?
            && let Event::Key(key) = event::read()?
        {
            if key.code == KeyCode::Esc && key.kind == KeyEventKind::Press {
                send_state(
                    &socket,
                    &held,
                    &mut sequence,
                    if send_stop_flag { FLAG_STOP } else { 0 },
                    player_slot,
                    input_token,
                )?;
                return Ok(InputExit::UserRequested);
            }
            changed = update_held_keys(&mut held, key);
        }
        if externally_stopped() {
            return Ok(InputExit::ExternallyStopped);
        }
        if changed || last_send.elapsed() >= HEARTBEAT_INTERVAL {
            let state = send_state(&socket, &held, &mut sequence, 0, player_slot, input_token)?;
            let debug_snapshot = DebugInputState::from(state);
            if debug_input && Some(debug_snapshot) != last_debug_state {
                println!("{}", debug_state(&held, state));
                last_debug_state = Some(debug_snapshot);
            }
            last_send = Instant::now();
        }
    }
}

fn run_automated_controller<F>(
    destination: SocketAddr,
    input_token: SessionToken,
    player_slot: u8,
    duration: Duration,
    mut externally_stopped: F,
) -> Result<InputExit, Box<dyn std::error::Error>>
where
    F: FnMut() -> bool,
{
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect(destination)?;
    let held = HashSet::new();
    let mut sequence = 0_u32;
    let started = Instant::now();
    while started.elapsed() < duration {
        if externally_stopped() {
            return Ok(InputExit::ExternallyStopped);
        }
        send_state(
            &socket,
            &held,
            &mut sequence,
            0,
            player_slot,
            Some(input_token),
        )?;
        thread::sleep(HEARTBEAT_INTERVAL);
    }
    Ok(InputExit::Completed)
}

fn parse_mode() -> Mode {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "seat-input".into());
    let values = args.collect::<Vec<_>>();
    if values.len() == 1
        && let Ok(destination) = values[0].parse::<SocketAddr>()
    {
        return Mode::Direct(destination);
    }

    let mut control_plane_url = None;
    let mut seat_id = env::var("FOURPLAY_SEAT_ID").unwrap_or_else(|_| "seat-dev".to_owned());
    let mut destination_address = env::var("FOURPLAY_SEAT_ADDRESS").ok();
    let mut game_id = None;
    let mut ffplay_path = env::var("FOURPLAY_FFPLAY_PATH").unwrap_or_else(|_| "ffplay".to_owned());
    let mut no_media = false;
    let mut play_for = None;
    let mut api_token = env::var("FOURPLAY_SEAT_API_TOKEN").ok();
    let mut debug_input = false;
    let mut index = 0;
    while index < values.len() {
        let option = &values[index];
        let value = |index: &mut usize| {
            *index += 1;
            values
                .get(*index)
                .cloned()
                .unwrap_or_else(|| usage(&program))
        };
        match option.as_str() {
            "--control-plane" => control_plane_url = Some(value(&mut index)),
            "--seat-id" => seat_id = value(&mut index),
            "--destination-ip" => destination_address = Some(value(&mut index)),
            "--game" => game_id = Some(value(&mut index)),
            "--ffplay-path" => ffplay_path = value(&mut index),
            "--no-media" => no_media = true,
            "--debug-input" => debug_input = true,
            "--api-token" => api_token = Some(value(&mut index)),
            "--play-for-ms" => {
                let milliseconds = value(&mut index).parse::<u64>().unwrap_or_else(|error| {
                    eprintln!("Invalid --play-for-ms value: {error}");
                    process::exit(2);
                });
                play_for = Some(Duration::from_millis(milliseconds));
            }
            "--help" | "-h" => usage(&program),
            _ => usage(&program),
        }
        index += 1;
    }
    let control_plane_url = control_plane_url.unwrap_or_else(|| usage(&program));
    let destination_address = destination_address
        .unwrap_or_else(|| usage(&program))
        .parse::<IpAddr>()
        .unwrap_or_else(|error| {
            eprintln!("Invalid seat destination address: {error}");
            process::exit(2);
        });
    let api_token = api_token.unwrap_or_else(|| usage(&program));
    if api_token.len() < 16 {
        eprintln!("Seat API token must contain at least 16 characters");
        process::exit(2);
    }
    Mode::Orchestrated(SeatConfig {
        control_plane_url: control_plane_url.trim_end_matches('/').to_owned(),
        seat_id,
        destination_address,
        game_id,
        ffplay_path,
        no_media,
        play_for,
        api_token,
        debug_input,
    })
}

fn usage(program: &str) -> ! {
    eprintln!(
        "Usage:\n  {program} <runtime-address:input-port>\n  {program} --control-plane <url> --destination-ip <seat-ip> [--seat-id <id>] [--api-token <token>] [--game <id>] [--ffplay-path <path>] [--no-media] [--debug-input] [--play-for-ms <milliseconds>]"
    );
    process::exit(2)
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

fn send_state(
    socket: &UdpSocket,
    held: &HashSet<KeyCode>,
    sequence: &mut u32,
    flags: u8,
    player_slot: u8,
    input_token: Option<SessionToken>,
) -> io::Result<ControllerState> {
    *sequence = sequence.wrapping_add(1);
    let state = state_from_keys(held, *sequence, flags, player_slot);
    if let Some(token) = input_token {
        socket.send(&AuthenticatedControllerState { token, state }.encode())?;
    } else {
        socket.send(&state.encode())?;
    }
    Ok(state)
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

#[derive(Clone, Copy, PartialEq, Eq)]
struct DebugInputState {
    buttons: u16,
    axis_x: i16,
    axis_y: i16,
    flags: u8,
}

impl From<ControllerState> for DebugInputState {
    fn from(state: ControllerState) -> Self {
        Self {
            buttons: state.buttons,
            axis_x: state.axis_x,
            axis_y: state.axis_y,
            flags: state.flags,
        }
    }
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

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn debug_state_names_neutral_keys_as_none() {
        let held = HashSet::new();
        let state = state_from_keys(&held, 43, 0, 1);

        assert_eq!(
            debug_state(&held, state),
            "input seq=43 axis=(0, 0) buttons=0x0000 keys=none"
        );
    }

    #[test]
    fn active_session_slots_are_described_for_browsing() {
        let slots = vec![
            PlayerSlot {
                player_number: 1,
                state: PlayerSlotState::Occupied,
                seat_id: Some("windows-seat-1".to_owned()),
                lease_expires_unix_ms: Some(123),
            },
            PlayerSlot {
                player_number: 2,
                state: PlayerSlotState::Open,
                seat_id: None,
                lease_expires_unix_ms: None,
            },
        ];

        assert_eq!(
            describe_player_slots(&slots),
            "P1 occupied by windows-seat-1; P2 open"
        );
    }

    #[test]
    fn missing_player_slots_are_reported_as_unavailable() {
        assert_eq!(describe_player_slots(&[]), "player slots unavailable");
    }

    #[test]
    fn state_from_keys_preserves_player_slot() {
        let state = state_from_keys(&HashSet::new(), 44, 0, 2);

        assert_eq!(state.player_slot, 2);
    }

    #[test]
    fn reservation_selection_uses_session_and_player_numbers() {
        assert_eq!(parse_reservation_selection("j1.2"), Some((1, 2)));
        assert_eq!(parse_reservation_selection("J12.4"), Some((12, 4)));
    }

    #[test]
    fn reservation_selection_rejects_invalid_values() {
        assert_eq!(parse_reservation_selection("1"), None);
        assert_eq!(parse_reservation_selection("j0.1"), None);
        assert_eq!(parse_reservation_selection("j1.0"), None);
        assert_eq!(parse_reservation_selection("j1"), None);
        assert_eq!(parse_reservation_selection("jone.two"), None);
    }
}
