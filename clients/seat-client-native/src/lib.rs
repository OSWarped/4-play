use std::{
    fs, io,
    net::{AddrParseError, IpAddr, UdpSocket},
    path::PathBuf,
    process::{Child, Command, Stdio},
};

use control_protocol::{
    CatalogGame, CatalogGameList, CreateSessionRequest, CreateSpectatorGrantRequest, PlayerSlot,
    PlayerSlotState, ReservePlayerSlotRequest, Session, SessionState, SessionSummary,
    SessionSummaryList, SpectatorGrant,
};
use input_protocol::{
    AuthenticatedControllerState, ControllerState, FLAG_STOP, SessionToken, button,
};
use seat_client_core::{
    InputForwardingPlan, MediaReceiverPlan, PublicRuntimeHandoff, RuntimePlanError,
    SeatClientConfig, SeatClientConfigError, public_player_handoff, public_spectator_handoff,
    spectator_runtime_plan,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePlayerLaunchPlan {
    pub public_handoff: PublicRuntimeHandoff,
    pub media_process: ProcessSpec,
    pub input: NativeInputForwardingPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSpectatorLaunchPlan {
    pub grant_id: String,
    pub public_handoff: PublicRuntimeHandoff,
    pub media_process: ProcessSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeInputForwardingPlan {
    pub destination: String,
    pub token: SessionToken,
    pub player_number: u32,
}

impl From<InputForwardingPlan> for NativeInputForwardingPlan {
    fn from(plan: InputForwardingPlan) -> Self {
        Self {
            destination: plan.destination,
            token: plan.token,
            player_number: plan.player_number,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeRuntimeStatus {
    pub mode: NativeRuntimeMode,
    pub session_id: String,
    pub game_id: String,
    pub media_udp_port: u16,
    pub input_destination: Option<String>,
    pub player_number: Option<u32>,
    pub media_running: bool,
    pub input_running: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeRuntimeMode {
    Player,
    Spectator,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NativeSeatSnapshot {
    pub games: Vec<CatalogGame>,
    pub active_sessions: Vec<SessionSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeRuntime {
    Player(NativePlayerLaunchPlan),
    Spectator(NativeSpectatorLaunchPlan),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinableSlot {
    pub player_number: u32,
    pub state: PlayerSlotState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeSeatViewModel {
    pub games: Vec<NativeGameCard>,
    pub active_sessions: Vec<NativeSessionCard>,
    pub current_runtime: Option<NativeRuntimeStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeGameCard {
    pub id: String,
    pub display_name: String,
    pub genre: Option<String>,
    pub release_year: Option<u16>,
    pub manufacturer: Option<String>,
    pub player_count: Option<u32>,
    pub screenshot_path: Option<String>,
    pub marquee_path: Option<String>,
    pub logo_path: Option<String>,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeSessionCard {
    pub id: String,
    pub game_id: String,
    pub display_name: String,
    pub runtime_host_id: String,
    pub state: SessionState,
    pub max_players: u32,
    pub joinable_slots: Vec<NativeJoinableSlot>,
    pub player_slots: Vec<PlayerSlot>,
    pub can_spectate: bool,
    pub active_spectator_count: u32,
    pub preview_asset_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeJoinableSlot {
    pub player_number: u32,
    pub state: PlayerSlotState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeControllerInput {
    pub buttons: u16,
    pub axis_x: i16,
    pub axis_y: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeInputButton {
    Action1,
    Action2,
    Action3,
    Action4,
    Action5,
    Action6,
    Coin,
    Start,
}

impl NativeInputButton {
    pub fn mask(self) -> u16 {
        match self {
            Self::Action1 => button::ACTION_1,
            Self::Action2 => button::ACTION_2,
            Self::Action3 => button::ACTION_3,
            Self::Action4 => button::ACTION_4,
            Self::Action5 => button::ACTION_5,
            Self::Action6 => button::ACTION_6,
            Self::Coin => button::COIN,
            Self::Start => button::START,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeControllerStateTracker {
    state: NativeControllerInput,
}

impl NativeControllerStateTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> NativeControllerInput {
        self.state
    }

    pub fn set_button(
        &mut self,
        button: NativeInputButton,
        pressed: bool,
    ) -> NativeControllerInput {
        if pressed {
            self.state.buttons |= button.mask();
        } else {
            self.state.buttons &= !button.mask();
        }
        self.state
    }

    pub fn set_axis(&mut self, axis_x: i16, axis_y: i16) -> NativeControllerInput {
        self.state.axis_x = clamp_axis(axis_x);
        self.state.axis_y = clamp_axis(axis_y);
        self.state
    }

    pub fn neutralize(&mut self) -> NativeControllerInput {
        self.state = NativeControllerInput::default();
        self.state
    }
}

#[derive(Debug)]
pub enum NativeClientError<ApiError> {
    Api(ApiError),
    Config(SeatClientConfigError),
    RuntimePlan(RuntimePlanError),
}

#[derive(Debug)]
pub enum NativeRuntimeSupervisorError<MediaError, InputError> {
    Media(MediaError),
    Input(InputError),
    NoPlayerRuntime,
}

#[derive(Debug)]
pub enum NativeSeatAppError<ApiError, MediaError, InputError> {
    Controller(NativeClientError<ApiError>),
    Runtime(NativeRuntimeSupervisorError<MediaError, InputError>),
    NoRuntime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeSeatConfigFile {
    pub control_plane_url: String,
    pub seat_id: String,
    pub destination_address: String,
    pub ffplay_path: PathBuf,
}

impl NativeSeatConfigFile {
    pub fn from_config(config: &SeatClientConfig) -> Self {
        Self {
            control_plane_url: config.control_plane_url.clone(),
            seat_id: config.seat_id.clone(),
            destination_address: config.destination_address.to_string(),
            ffplay_path: config.ffplay_path.clone(),
        }
    }

    pub fn into_config(
        self,
        seat_api_token: impl Into<String>,
    ) -> Result<SeatClientConfig, NativeSeatConfigError> {
        let config = SeatClientConfig {
            control_plane_url: self.control_plane_url,
            seat_id: self.seat_id,
            destination_address: self
                .destination_address
                .parse::<IpAddr>()
                .map_err(NativeSeatConfigError::Address)?,
            seat_api_token: seat_api_token.into(),
            ffplay_path: self.ffplay_path,
        };
        config.validate().map_err(NativeSeatConfigError::Config)?;
        Ok(config)
    }

    pub fn validate_without_token(&self) -> Result<(), NativeSeatConfigError> {
        self.clone().into_config("__runtime_token_placeholder")?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum NativeSeatConfigError {
    Io(io::Error),
    Json(serde_json::Error),
    Address(AddrParseError),
    Config(SeatClientConfigError),
}

impl std::fmt::Display for NativeSeatConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "seat config I/O error: {error}"),
            Self::Json(error) => write!(formatter, "seat config JSON error: {error}"),
            Self::Address(error) => write!(formatter, "seat destination address error: {error}"),
            Self::Config(error) => write!(formatter, "seat config error: {error}"),
        }
    }
}

impl std::error::Error for NativeSeatConfigError {}

pub struct NativeSeatConfigStore {
    path: PathBuf,
}

impl NativeSeatConfigStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn load_with_token(
        &self,
        seat_api_token: impl Into<String>,
    ) -> Result<SeatClientConfig, NativeSeatConfigError> {
        let text = fs::read_to_string(&self.path).map_err(NativeSeatConfigError::Io)?;
        serde_json::from_str::<NativeSeatConfigFile>(&text)
            .map_err(NativeSeatConfigError::Json)?
            .into_config(seat_api_token)
    }

    pub fn save(&self, config: &SeatClientConfig) -> Result<(), NativeSeatConfigError> {
        config.validate().map_err(NativeSeatConfigError::Config)?;
        self.save_file(&NativeSeatConfigFile::from_config(config))
    }

    pub fn save_file(&self, config: &NativeSeatConfigFile) -> Result<(), NativeSeatConfigError> {
        config.validate_without_token()?;
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(NativeSeatConfigError::Io)?;
        }
        let text = serde_json::to_string_pretty(config).map_err(NativeSeatConfigError::Json)?;
        fs::write(&self.path, text).map_err(NativeSeatConfigError::Io)
    }
}

pub struct NativeSeatUiSession<A, M, I>
where
    A: ControlPlaneApi,
    M: MediaProcessSupervisor,
    I: InputPacketSender,
{
    app: NativeSeatApp<A, M, I>,
    input: NativeControllerStateTracker,
}

impl<A, M, I> NativeSeatUiSession<A, M, I>
where
    A: ControlPlaneApi,
    M: MediaProcessSupervisor,
    I: InputPacketSender,
{
    pub fn new(app: NativeSeatApp<A, M, I>) -> Self {
        Self {
            app,
            input: NativeControllerStateTracker::new(),
        }
    }

    pub fn input_state(&self) -> NativeControllerInput {
        self.input.state()
    }

    pub fn handle_command(
        &mut self,
        command: NativeUiCommand,
    ) -> Result<NativeUiResponse, NativeSeatAppError<A::Error, M::Error, I::Error>> {
        match command {
            NativeUiCommand::Refresh => self.refresh_view_model(),
            NativeUiCommand::StartGame { game_id } => self
                .app
                .start_game(&game_id)
                .map(NativeUiResponse::RuntimeStatus),
            NativeUiCommand::JoinSession {
                session_id,
                player_number,
            } => self
                .app
                .join_session(&session_id, player_number)
                .map(NativeUiResponse::RuntimeStatus),
            NativeUiCommand::RejoinSession {
                session_id,
                player_number,
            } => self
                .app
                .rejoin_session(&session_id, player_number)
                .map(NativeUiResponse::RuntimeStatus),
            NativeUiCommand::SpectateSession { session_id } => self
                .app
                .spectate_session(&session_id)
                .map(NativeUiResponse::RuntimeStatus),
            NativeUiCommand::JoinFromSpectator {
                session_id,
                player_number,
            } => self
                .app
                .join_from_spectator(&session_id, player_number)
                .map(NativeUiResponse::RuntimeStatus),
            NativeUiCommand::LeavePlayerSlot {
                session_id,
                player_number,
            } => {
                self.app.leave_player_slot(&session_id, player_number)?;
                Ok(NativeUiResponse::Ack)
            }
            NativeUiCommand::StopRuntime => {
                self.app.stop_runtime()?;
                Ok(NativeUiResponse::Ack)
            }
            NativeUiCommand::SetButton { button, pressed } => {
                let state = self.input.set_button(button, pressed);
                self.app.send_controller_state(state)?;
                Ok(NativeUiResponse::InputState(state))
            }
            NativeUiCommand::SetAxis { axis_x, axis_y } => {
                let state = self.input.set_axis(axis_x, axis_y);
                self.app.send_controller_state(state)?;
                Ok(NativeUiResponse::InputState(state))
            }
            NativeUiCommand::NeutralizeInput => {
                let state = self.input.neutralize();
                self.app.send_controller_state(state)?;
                Ok(NativeUiResponse::InputState(state))
            }
            NativeUiCommand::SendStopInput => {
                self.input.neutralize();
                self.app.send_player_stop()?;
                Ok(NativeUiResponse::InputState(self.input.state()))
            }
        }
    }

    fn refresh_view_model(
        &mut self,
    ) -> Result<NativeUiResponse, NativeSeatAppError<A::Error, M::Error, I::Error>> {
        let snapshot = self.app.refresh()?;
        let status = self.app.runtime_status()?;
        Ok(NativeUiResponse::ViewModel(build_native_seat_view_model(
            snapshot,
            status,
            self.app.seat_id(),
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NativeUiCommand {
    Refresh,
    StartGame {
        game_id: String,
    },
    JoinSession {
        session_id: String,
        player_number: u32,
    },
    RejoinSession {
        session_id: String,
        player_number: u32,
    },
    SpectateSession {
        session_id: String,
    },
    JoinFromSpectator {
        session_id: String,
        player_number: u32,
    },
    LeavePlayerSlot {
        session_id: String,
        player_number: u32,
    },
    StopRuntime,
    SetButton {
        button: NativeInputButton,
        pressed: bool,
    },
    SetAxis {
        axis_x: i16,
        axis_y: i16,
    },
    NeutralizeInput,
    SendStopInput,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum NativeUiResponse {
    ViewModel(NativeSeatViewModel),
    RuntimeStatus(NativeRuntimeStatus),
    InputState(NativeControllerInput),
    Ack,
}

pub struct NativeSeatApp<A, M, I>
where
    A: ControlPlaneApi,
    M: MediaProcessSupervisor,
    I: InputPacketSender,
{
    controller: NativeSeatController<A>,
    supervisor: NativeRuntimeSupervisor<M, I>,
}

impl<A, M, I> NativeSeatApp<A, M, I>
where
    A: ControlPlaneApi,
    M: MediaProcessSupervisor,
    I: InputPacketSender,
{
    pub fn new(
        config: SeatClientConfig,
        api: A,
        media: M,
        input: I,
    ) -> Result<Self, NativeClientError<A::Error>> {
        Ok(Self {
            controller: NativeSeatController::new(config, api)?,
            supervisor: NativeRuntimeSupervisor::new(media, input),
        })
    }

    pub fn refresh(
        &mut self,
    ) -> Result<NativeSeatSnapshot, NativeSeatAppError<A::Error, M::Error, I::Error>> {
        self.controller
            .refresh()
            .map_err(NativeSeatAppError::Controller)
    }

    pub fn seat_id(&self) -> &str {
        self.controller.seat_id()
    }

    pub fn start_game(
        &mut self,
        game_id: &str,
    ) -> Result<NativeRuntimeStatus, NativeSeatAppError<A::Error, M::Error, I::Error>> {
        let launch = self
            .controller
            .start_game(game_id)
            .map_err(NativeSeatAppError::Controller)?;
        self.supervisor
            .start_player(launch)
            .map_err(NativeSeatAppError::Runtime)?;
        self.runtime_status()?.ok_or(NativeSeatAppError::NoRuntime)
    }

    pub fn join_session(
        &mut self,
        session_id: &str,
        player_number: u32,
    ) -> Result<NativeRuntimeStatus, NativeSeatAppError<A::Error, M::Error, I::Error>> {
        let launch = self
            .controller
            .join_session(session_id, player_number)
            .map_err(NativeSeatAppError::Controller)?;
        self.supervisor
            .start_player(launch)
            .map_err(NativeSeatAppError::Runtime)?;
        self.runtime_status()?.ok_or(NativeSeatAppError::NoRuntime)
    }

    pub fn rejoin_session(
        &mut self,
        session_id: &str,
        player_number: u32,
    ) -> Result<NativeRuntimeStatus, NativeSeatAppError<A::Error, M::Error, I::Error>> {
        let launch = self
            .controller
            .rejoin_session(session_id, player_number)
            .map_err(NativeSeatAppError::Controller)?;
        self.supervisor
            .start_player(launch)
            .map_err(NativeSeatAppError::Runtime)?;
        self.runtime_status()?.ok_or(NativeSeatAppError::NoRuntime)
    }

    pub fn spectate_session(
        &mut self,
        session_id: &str,
    ) -> Result<NativeRuntimeStatus, NativeSeatAppError<A::Error, M::Error, I::Error>> {
        let launch = self
            .controller
            .spectate_session(session_id)
            .map_err(NativeSeatAppError::Controller)?;
        self.supervisor
            .start_spectator(launch)
            .map_err(NativeSeatAppError::Runtime)?;
        self.runtime_status()?.ok_or(NativeSeatAppError::NoRuntime)
    }

    pub fn join_from_spectator(
        &mut self,
        session_id: &str,
        player_number: u32,
    ) -> Result<NativeRuntimeStatus, NativeSeatAppError<A::Error, M::Error, I::Error>> {
        let launch = self
            .controller
            .join_from_spectator(session_id, player_number)
            .map_err(NativeSeatAppError::Controller)?;
        self.supervisor
            .start_player(launch)
            .map_err(NativeSeatAppError::Runtime)?;
        self.runtime_status()?.ok_or(NativeSeatAppError::NoRuntime)
    }

    pub fn leave_player_slot(
        &mut self,
        session_id: &str,
        player_number: u32,
    ) -> Result<(), NativeSeatAppError<A::Error, M::Error, I::Error>> {
        self.controller
            .leave_player_slot(session_id, player_number)
            .map_err(NativeSeatAppError::Controller)?;
        self.stop_runtime()
    }

    pub fn stop_runtime(&mut self) -> Result<(), NativeSeatAppError<A::Error, M::Error, I::Error>> {
        self.supervisor.stop().map_err(NativeSeatAppError::Runtime)
    }

    pub fn runtime_status(
        &mut self,
    ) -> Result<Option<NativeRuntimeStatus>, NativeSeatAppError<A::Error, M::Error, I::Error>> {
        self.supervisor
            .status()
            .map_err(NativeSeatAppError::Runtime)
    }

    pub fn send_player_input(
        &mut self,
        buttons: u16,
        axis_x: i16,
        axis_y: i16,
    ) -> Result<(), NativeSeatAppError<A::Error, M::Error, I::Error>> {
        self.supervisor
            .send_player_input(buttons, axis_x, axis_y)
            .map_err(NativeSeatAppError::Runtime)
    }

    pub fn send_controller_state(
        &mut self,
        state: NativeControllerInput,
    ) -> Result<(), NativeSeatAppError<A::Error, M::Error, I::Error>> {
        self.send_player_input(state.buttons, state.axis_x, state.axis_y)
    }

    pub fn send_player_stop(
        &mut self,
    ) -> Result<(), NativeSeatAppError<A::Error, M::Error, I::Error>> {
        self.supervisor
            .send_player_stop()
            .map_err(NativeSeatAppError::Runtime)
    }
}

fn clamp_axis(value: i16) -> i16 {
    value.clamp(-1, 1)
}

pub trait MediaProcessSupervisor {
    type Error;
    type Handle;

    fn spawn_media(&mut self, spec: &ProcessSpec) -> Result<Self::Handle, Self::Error>;
    fn stop_media(&mut self, handle: &mut Self::Handle) -> Result<(), Self::Error>;
    fn media_running(&mut self, handle: &mut Self::Handle) -> Result<bool, Self::Error>;
}

pub trait InputPacketSender {
    type Error;

    fn send_input(
        &mut self,
        destination: &str,
        token: SessionToken,
        state: ControllerState,
    ) -> Result<(), Self::Error>;
}

pub struct ChildMediaProcessSupervisor;

impl MediaProcessSupervisor for ChildMediaProcessSupervisor {
    type Error = io::Error;
    type Handle = Child;

    fn spawn_media(&mut self, spec: &ProcessSpec) -> Result<Self::Handle, Self::Error> {
        Command::new(&spec.program)
            .args(&spec.args)
            .stdin(Stdio::null())
            .spawn()
    }

    fn stop_media(&mut self, handle: &mut Self::Handle) -> Result<(), Self::Error> {
        if handle.try_wait()?.is_none() {
            handle.kill()?;
        }
        handle.wait()?;
        Ok(())
    }

    fn media_running(&mut self, handle: &mut Self::Handle) -> Result<bool, Self::Error> {
        Ok(handle.try_wait()?.is_none())
    }
}

pub struct UdpInputPacketSender {
    socket: UdpSocket,
}

impl UdpInputPacketSender {
    pub fn bind_any() -> io::Result<Self> {
        Ok(Self {
            socket: UdpSocket::bind("0.0.0.0:0")?,
        })
    }
}

impl InputPacketSender for UdpInputPacketSender {
    type Error = io::Error;

    fn send_input(
        &mut self,
        destination: &str,
        token: SessionToken,
        state: ControllerState,
    ) -> Result<(), Self::Error> {
        self.socket.send_to(
            &AuthenticatedControllerState { token, state }.encode(),
            destination,
        )?;
        Ok(())
    }
}

pub struct NativeRuntimeSupervisor<M, I>
where
    M: MediaProcessSupervisor,
    I: InputPacketSender,
{
    media: M,
    input: I,
    current: Option<RunningNativeRuntime<M::Handle>>,
    sequence: u32,
}

enum RunningNativeRuntime<Handle> {
    Player {
        launch: NativePlayerLaunchPlan,
        media: Handle,
    },
    Spectator {
        launch: NativeSpectatorLaunchPlan,
        media: Handle,
    },
}

impl<M, I> NativeRuntimeSupervisor<M, I>
where
    M: MediaProcessSupervisor,
    I: InputPacketSender,
{
    pub fn new(media: M, input: I) -> Self {
        Self {
            media,
            input,
            current: None,
            sequence: 0,
        }
    }

    pub fn start_player(
        &mut self,
        launch: NativePlayerLaunchPlan,
    ) -> Result<(), NativeRuntimeSupervisorError<M::Error, I::Error>> {
        self.stop()?;
        let media = self
            .media
            .spawn_media(&launch.media_process)
            .map_err(NativeRuntimeSupervisorError::Media)?;
        self.current = Some(RunningNativeRuntime::Player { launch, media });
        self.sequence = 0;
        Ok(())
    }

    pub fn start_spectator(
        &mut self,
        launch: NativeSpectatorLaunchPlan,
    ) -> Result<(), NativeRuntimeSupervisorError<M::Error, I::Error>> {
        self.stop()?;
        let media = self
            .media
            .spawn_media(&launch.media_process)
            .map_err(NativeRuntimeSupervisorError::Media)?;
        self.current = Some(RunningNativeRuntime::Spectator { launch, media });
        self.sequence = 0;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), NativeRuntimeSupervisorError<M::Error, I::Error>> {
        if let Some(mut current) = self.current.take() {
            let media = match &mut current {
                RunningNativeRuntime::Player { media, .. }
                | RunningNativeRuntime::Spectator { media, .. } => media,
            };
            self.media
                .stop_media(media)
                .map_err(NativeRuntimeSupervisorError::Media)?;
        }
        Ok(())
    }

    pub fn status(
        &mut self,
    ) -> Result<Option<NativeRuntimeStatus>, NativeRuntimeSupervisorError<M::Error, I::Error>> {
        let Some(current) = self.current.as_mut() else {
            return Ok(None);
        };
        match current {
            RunningNativeRuntime::Player { launch, media } => {
                let mut status = launch.safe_status();
                status.media_running = self
                    .media
                    .media_running(media)
                    .map_err(NativeRuntimeSupervisorError::Media)?;
                status.input_running = true;
                Ok(Some(status))
            }
            RunningNativeRuntime::Spectator { launch, media } => {
                let mut status = launch.safe_status();
                status.media_running = self
                    .media
                    .media_running(media)
                    .map_err(NativeRuntimeSupervisorError::Media)?;
                Ok(Some(status))
            }
        }
    }

    pub fn send_player_input(
        &mut self,
        buttons: u16,
        axis_x: i16,
        axis_y: i16,
    ) -> Result<(), NativeRuntimeSupervisorError<M::Error, I::Error>> {
        self.send_player_input_with_flags(buttons, axis_x, axis_y, 0)
    }

    pub fn send_player_stop(
        &mut self,
    ) -> Result<(), NativeRuntimeSupervisorError<M::Error, I::Error>> {
        self.send_player_input_with_flags(0, 0, 0, FLAG_STOP)
    }

    fn send_player_input_with_flags(
        &mut self,
        buttons: u16,
        axis_x: i16,
        axis_y: i16,
        flags: u8,
    ) -> Result<(), NativeRuntimeSupervisorError<M::Error, I::Error>> {
        let Some(RunningNativeRuntime::Player { launch, .. }) = self.current.as_ref() else {
            return Err(NativeRuntimeSupervisorError::NoPlayerRuntime);
        };
        self.sequence = self.sequence.wrapping_add(1);
        let state = ControllerState {
            sequence: self.sequence,
            buttons,
            axis_x,
            axis_y,
            flags,
            player_slot: launch.input.player_number.try_into().unwrap_or(u8::MAX),
        };
        self.input
            .send_input(&launch.input.destination, launch.input.token, state)
            .map_err(NativeRuntimeSupervisorError::Input)
    }
}

pub trait ControlPlaneApi {
    type Error;

    fn list_games(&mut self) -> Result<CatalogGameList, Self::Error>;
    fn list_active_sessions(&mut self) -> Result<SessionSummaryList, Self::Error>;
    fn get_session(&mut self, session_id: &str) -> Result<Session, Self::Error>;
    fn create_session(&mut self, request: CreateSessionRequest) -> Result<Session, Self::Error>;
    fn reserve_player_slot(
        &mut self,
        session_id: &str,
        player_number: u32,
        request: ReservePlayerSlotRequest,
    ) -> Result<Session, Self::Error>;
    fn connect_player_slot(
        &mut self,
        session_id: &str,
        player_number: u32,
        request: ReservePlayerSlotRequest,
    ) -> Result<Session, Self::Error>;
    fn disconnect_player_slot(
        &mut self,
        session_id: &str,
        player_number: u32,
        request: ReservePlayerSlotRequest,
    ) -> Result<Session, Self::Error>;
    fn create_spectator_grant(
        &mut self,
        session_id: &str,
        request: CreateSpectatorGrantRequest,
    ) -> Result<SpectatorGrant, Self::Error>;
    fn release_spectator_grant(
        &mut self,
        session_id: &str,
        grant_id: &str,
        request: ReservePlayerSlotRequest,
    ) -> Result<Session, Self::Error>;
}

pub struct NativeSeatController<A> {
    config: SeatClientConfig,
    api: A,
    current_runtime: Option<NativeRuntime>,
}

impl<A: ControlPlaneApi> NativeSeatController<A> {
    pub fn new(config: SeatClientConfig, api: A) -> Result<Self, NativeClientError<A::Error>> {
        config.validate().map_err(NativeClientError::Config)?;
        Ok(Self {
            config,
            api,
            current_runtime: None,
        })
    }

    pub fn refresh(&mut self) -> Result<NativeSeatSnapshot, NativeClientError<A::Error>> {
        let games = self.api.list_games().map_err(NativeClientError::Api)?.games;
        let active_sessions = self
            .api
            .list_active_sessions()
            .map_err(NativeClientError::Api)?
            .sessions;
        Ok(NativeSeatSnapshot {
            games,
            active_sessions,
        })
    }

    pub fn current_runtime(&self) -> Option<&NativeRuntime> {
        self.current_runtime.as_ref()
    }

    pub fn seat_id(&self) -> &str {
        &self.config.seat_id
    }

    pub fn start_game(
        &mut self,
        game_id: &str,
    ) -> Result<NativePlayerLaunchPlan, NativeClientError<A::Error>> {
        let session = self
            .api
            .create_session(CreateSessionRequest {
                game_id: game_id.to_owned(),
                seat_id: self.config.seat_id.clone(),
                destination_address: self.config.destination_address.to_string(),
            })
            .map_err(NativeClientError::Api)?;
        self.player_launch_from_session(&session, 1)
    }

    pub fn join_session(
        &mut self,
        session_id: &str,
        player_number: u32,
    ) -> Result<NativePlayerLaunchPlan, NativeClientError<A::Error>> {
        let request = self.slot_request();
        self.api
            .reserve_player_slot(session_id, player_number, request.clone())
            .map_err(NativeClientError::Api)?;
        let session = self
            .api
            .connect_player_slot(session_id, player_number, request)
            .map_err(NativeClientError::Api)?;
        self.player_launch_from_session(&session, player_number)
    }

    pub fn rejoin_session(
        &mut self,
        session_id: &str,
        player_number: u32,
    ) -> Result<NativePlayerLaunchPlan, NativeClientError<A::Error>> {
        let session = self
            .api
            .connect_player_slot(session_id, player_number, self.slot_request())
            .map_err(NativeClientError::Api)?;
        self.player_launch_from_session(&session, player_number)
    }

    pub fn leave_player_slot(
        &mut self,
        session_id: &str,
        player_number: u32,
    ) -> Result<Session, NativeClientError<A::Error>> {
        let session = self
            .api
            .disconnect_player_slot(session_id, player_number, self.slot_request())
            .map_err(NativeClientError::Api)?;
        if matches!(
            self.current_runtime.as_ref(),
            Some(NativeRuntime::Player(plan))
                if plan.public_handoff.session_id == session_id
                    && plan.input.player_number == player_number
        ) {
            self.current_runtime = None;
        }
        Ok(session)
    }

    pub fn spectate_session(
        &mut self,
        session_id: &str,
    ) -> Result<NativeSpectatorLaunchPlan, NativeClientError<A::Error>> {
        let grant = self
            .api
            .create_spectator_grant(
                session_id,
                CreateSpectatorGrantRequest {
                    seat_id: self.config.seat_id.clone(),
                    destination_address: self.config.destination_address.to_string(),
                },
            )
            .map_err(NativeClientError::Api)?;
        let session = self
            .api
            .get_session(session_id)
            .map_err(NativeClientError::Api)?;
        let launch =
            plan_native_spectator_launch(&session, &grant, self.config.ffplay_path.clone());
        self.current_runtime = Some(NativeRuntime::Spectator(launch.clone()));
        Ok(launch)
    }

    pub fn join_from_spectator(
        &mut self,
        session_id: &str,
        player_number: u32,
    ) -> Result<NativePlayerLaunchPlan, NativeClientError<A::Error>> {
        if let Some(NativeRuntime::Spectator(plan)) = self.current_runtime.as_ref()
            && plan.public_handoff.session_id == session_id
        {
            self.api
                .release_spectator_grant(session_id, &plan.grant_id, self.slot_request())
                .map_err(NativeClientError::Api)?;
        }
        self.join_session(session_id, player_number)
    }

    fn player_launch_from_session(
        &mut self,
        session: &Session,
        player_number: u32,
    ) -> Result<NativePlayerLaunchPlan, NativeClientError<A::Error>> {
        let launch =
            plan_native_player_launch(session, player_number, self.config.ffplay_path.clone())
                .map_err(NativeClientError::RuntimePlan)?;
        self.current_runtime = Some(NativeRuntime::Player(launch.clone()));
        Ok(launch)
    }

    fn slot_request(&self) -> ReservePlayerSlotRequest {
        ReservePlayerSlotRequest {
            seat_id: self.config.seat_id.clone(),
        }
    }
}

pub struct BlockingControlPlaneClient {
    client: reqwest::blocking::Client,
    control_plane_url: String,
    api_token: String,
}

impl BlockingControlPlaneClient {
    pub fn new(control_plane_url: impl Into<String>, api_token: impl Into<String>) -> Self {
        Self {
            client: reqwest::blocking::Client::new(),
            control_plane_url: control_plane_url.into().trim_end_matches('/').to_owned(),
            api_token: api_token.into(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.control_plane_url, path)
    }

    fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, reqwest::Error> {
        self.client
            .get(self.url(path))
            .bearer_auth(&self.api_token)
            .send()?
            .error_for_status()?
            .json()
    }

    fn post<B: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, reqwest::Error> {
        self.client
            .post(self.url(path))
            .bearer_auth(&self.api_token)
            .json(body)
            .send()?
            .error_for_status()?
            .json()
    }

    fn delete<B: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, reqwest::Error> {
        self.client
            .delete(self.url(path))
            .bearer_auth(&self.api_token)
            .json(body)
            .send()?
            .error_for_status()?
            .json()
    }
}

impl ControlPlaneApi for BlockingControlPlaneClient {
    type Error = reqwest::Error;

    fn list_games(&mut self) -> Result<CatalogGameList, Self::Error> {
        self.get("/api/v1/games")
    }

    fn list_active_sessions(&mut self) -> Result<SessionSummaryList, Self::Error> {
        self.get("/api/v1/active-sessions")
    }

    fn get_session(&mut self, session_id: &str) -> Result<Session, Self::Error> {
        self.get(&format!("/api/v1/sessions/{session_id}"))
    }

    fn create_session(&mut self, request: CreateSessionRequest) -> Result<Session, Self::Error> {
        self.post("/api/v1/sessions", &request)
    }

    fn reserve_player_slot(
        &mut self,
        session_id: &str,
        player_number: u32,
        request: ReservePlayerSlotRequest,
    ) -> Result<Session, Self::Error> {
        self.post(
            &format!("/api/v1/sessions/{session_id}/player-slots/{player_number}/reserve"),
            &request,
        )
    }

    fn connect_player_slot(
        &mut self,
        session_id: &str,
        player_number: u32,
        request: ReservePlayerSlotRequest,
    ) -> Result<Session, Self::Error> {
        self.post(
            &format!("/api/v1/sessions/{session_id}/player-slots/{player_number}/connect"),
            &request,
        )
    }

    fn disconnect_player_slot(
        &mut self,
        session_id: &str,
        player_number: u32,
        request: ReservePlayerSlotRequest,
    ) -> Result<Session, Self::Error> {
        self.post(
            &format!("/api/v1/sessions/{session_id}/player-slots/{player_number}/disconnect"),
            &request,
        )
    }

    fn create_spectator_grant(
        &mut self,
        session_id: &str,
        request: CreateSpectatorGrantRequest,
    ) -> Result<SpectatorGrant, Self::Error> {
        self.post(
            &format!("/api/v1/sessions/{session_id}/spectators"),
            &request,
        )
    }

    fn release_spectator_grant(
        &mut self,
        session_id: &str,
        grant_id: &str,
        request: ReservePlayerSlotRequest,
    ) -> Result<Session, Self::Error> {
        self.delete(
            &format!("/api/v1/sessions/{session_id}/spectators/{grant_id}"),
            &request,
        )
    }
}

pub fn joinable_slots_for_seat(slots: &[PlayerSlot], seat_id: &str) -> Vec<JoinableSlot> {
    slots
        .iter()
        .filter(|slot| {
            matches!(slot.state, PlayerSlotState::Open)
                || (matches!(slot.state, PlayerSlotState::Disconnected)
                    && slot.seat_id.as_deref() == Some(seat_id))
        })
        .map(|slot| JoinableSlot {
            player_number: slot.player_number,
            state: slot.state,
        })
        .collect()
}

pub fn build_native_seat_view_model(
    snapshot: NativeSeatSnapshot,
    current_runtime: Option<NativeRuntimeStatus>,
    seat_id: &str,
) -> NativeSeatViewModel {
    let games = snapshot
        .games
        .iter()
        .map(native_game_card)
        .collect::<Vec<_>>();
    let active_sessions = snapshot
        .active_sessions
        .iter()
        .map(|session| native_session_card(session, &snapshot.games, seat_id))
        .collect::<Vec<_>>();
    NativeSeatViewModel {
        games,
        active_sessions,
        current_runtime,
    }
}

fn native_game_card(game: &CatalogGame) -> NativeGameCard {
    NativeGameCard {
        id: game.id.clone(),
        display_name: game.display_name.clone(),
        genre: game.metadata.genre.clone(),
        release_year: game.metadata.release_year,
        manufacturer: game.metadata.manufacturer.clone(),
        player_count: game.metadata.player_count.or_else(|| {
            game.availability
                .first()
                .map(|available| available.profile.max_players)
        }),
        screenshot_path: game.metadata.screenshot_path.clone(),
        marquee_path: game.metadata.marquee_path.clone(),
        logo_path: game.metadata.logo_path.clone(),
        available: game.availability.iter().any(|availability| {
            availability.runtime_host_status == control_protocol::RuntimeHostStatus::Online
        }),
    }
}

fn native_session_card(
    session: &SessionSummary,
    games: &[CatalogGame],
    seat_id: &str,
) -> NativeSessionCard {
    let display_name = games
        .iter()
        .find(|game| game.id == session.game_id)
        .map(|game| game.display_name.clone())
        .unwrap_or_else(|| session.game_id.clone());
    NativeSessionCard {
        id: session.id.clone(),
        game_id: session.game_id.clone(),
        display_name,
        runtime_host_id: session.runtime_host_id.clone(),
        state: session.state,
        max_players: session.runtime_profile.max_players,
        joinable_slots: joinable_slots_for_seat(&session.player_slots, seat_id)
            .into_iter()
            .map(|slot| NativeJoinableSlot {
                player_number: slot.player_number,
                state: slot.state,
            })
            .collect(),
        player_slots: session.player_slots.clone(),
        can_spectate: !session.state.is_terminal(),
        active_spectator_count: session.active_spectator_count,
        preview_asset_path: session.preview_asset_path.clone(),
    }
}

pub fn plan_native_player_launch(
    session: &Session,
    player_number: u32,
    ffplay_path: PathBuf,
) -> Result<NativePlayerLaunchPlan, RuntimePlanError> {
    let private_plan = seat_client_core::player_runtime_plan(session, player_number)?;
    Ok(NativePlayerLaunchPlan {
        public_handoff: public_player_handoff(session, player_number)?,
        media_process: media_process_spec(&private_plan.media, ffplay_path),
        input: private_plan.input.into(),
    })
}

pub fn plan_native_spectator_launch(
    session: &Session,
    grant: &SpectatorGrant,
    ffplay_path: PathBuf,
) -> NativeSpectatorLaunchPlan {
    let private_plan = spectator_runtime_plan(grant);
    NativeSpectatorLaunchPlan {
        grant_id: grant.id.clone(),
        public_handoff: public_spectator_handoff(session, grant),
        media_process: media_process_spec(&private_plan.media, ffplay_path),
    }
}

pub fn media_process_spec(plan: &MediaReceiverPlan, ffplay_path: PathBuf) -> ProcessSpec {
    ProcessSpec {
        program: ffplay_path,
        args: plan.ffplay_args.clone(),
    }
}

impl NativePlayerLaunchPlan {
    pub fn safe_status(&self) -> NativeRuntimeStatus {
        NativeRuntimeStatus {
            mode: NativeRuntimeMode::Player,
            session_id: self.public_handoff.session_id.clone(),
            game_id: self.public_handoff.game_id.clone(),
            media_udp_port: self.public_handoff.media.udp_port,
            input_destination: Some(self.input.destination.clone()),
            player_number: Some(self.input.player_number),
            media_running: false,
            input_running: false,
        }
    }
}

impl NativeSpectatorLaunchPlan {
    pub fn safe_status(&self) -> NativeRuntimeStatus {
        NativeRuntimeStatus {
            mode: NativeRuntimeMode::Spectator,
            session_id: self.public_handoff.session_id.clone(),
            game_id: self.public_handoff.game_id.clone(),
            media_udp_port: self.public_handoff.media.udp_port,
            input_destination: None,
            player_number: None,
            media_running: false,
            input_running: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use control_protocol::{
        ConnectionGrant, GameAvailability, GameMetadata, GameRuntimeProfile,
        PlayerSlotPresentation, PreviewStatus, RuntimeHostStatus, SessionState,
    };

    #[test]
    fn player_launch_plan_keeps_input_token_out_of_process_args_and_status() {
        let launch = plan_native_player_launch(&session(), 1, PathBuf::from("ffplay.exe")).unwrap();

        assert_eq!(launch.media_process.program, PathBuf::from("ffplay.exe"));
        assert!(
            launch
                .media_process
                .args
                .iter()
                .any(|arg| arg == "low_delay")
        );
        assert_eq!(launch.input.destination, "192.0.2.68:42000");

        let token = "00112233-4455-6677-8899-aabbccddeeff";
        let process_json = serde_json::to_string(&launch.media_process).unwrap();
        let status_json = serde_json::to_string(&launch.safe_status()).unwrap();
        let public_json = serde_json::to_string(&launch.public_handoff).unwrap();

        assert!(!process_json.contains(token));
        assert!(!status_json.contains(token));
        assert!(!public_json.contains(token));
        assert!(!status_json.contains("token"));
    }

    #[test]
    fn spectator_launch_plan_has_media_only_status() {
        let launch =
            plan_native_spectator_launch(&session(), &spectator_grant(), PathBuf::from("ffplay"));
        let status = launch.safe_status();

        assert_eq!(status.mode, NativeRuntimeMode::Spectator);
        assert_eq!(status.media_udp_port, 41_002);
        assert_eq!(status.input_destination, None);
        assert_eq!(status.player_number, None);
    }

    #[test]
    fn controller_refresh_lists_games_and_active_sessions() {
        let mut controller = controller(FakeApi::default());
        let snapshot = controller.refresh().unwrap();

        assert_eq!(snapshot.games[0].id, "tmnt");
        assert_eq!(snapshot.active_sessions[0].id, "session-1");
    }

    #[test]
    fn controller_start_game_creates_player_runtime() {
        let mut controller = controller(FakeApi::default());
        let launch = controller.start_game("tmnt").unwrap();

        assert_eq!(
            launch.public_handoff.mode,
            seat_client_core::RuntimeHandoffMode::Player
        );
        assert_eq!(launch.input.destination, "192.0.2.68:42000");
        assert!(matches!(
            controller.current_runtime(),
            Some(NativeRuntime::Player(plan)) if plan.public_handoff.session_id == "session-1"
        ));
        assert_eq!(controller.api.calls, vec!["create_session:tmnt".to_owned()]);
    }

    #[test]
    fn controller_spectate_then_join_releases_spectator_and_starts_player_runtime() {
        let mut controller = controller(FakeApi::default());

        let spectator = controller.spectate_session("session-1").unwrap();
        assert_eq!(spectator.grant_id, "grant-1");
        assert!(matches!(
            controller.current_runtime(),
            Some(NativeRuntime::Spectator(plan)) if plan.grant_id == "grant-1"
        ));

        let player = controller.join_from_spectator("session-1", 2).unwrap();
        assert_eq!(player.input.player_number, 2);
        assert!(matches!(
            controller.current_runtime(),
            Some(NativeRuntime::Player(plan)) if plan.input.player_number == 2
        ));
        assert_eq!(
            controller.api.calls,
            vec![
                "create_spectator:session-1".to_owned(),
                "get_session:session-1".to_owned(),
                "release_spectator:session-1:grant-1".to_owned(),
                "reserve:session-1:2".to_owned(),
                "connect:session-1:2".to_owned(),
            ]
        );
    }

    #[test]
    fn joinable_slots_include_open_and_same_seat_disconnected_slots() {
        let slots = vec![
            PlayerSlot {
                player_number: 1,
                state: PlayerSlotState::Occupied,
                seat_id: Some("seat-a".to_owned()),
                lease_expires_unix_ms: None,
                presentation: PlayerSlotPresentation::default(),
            },
            PlayerSlot {
                player_number: 2,
                state: PlayerSlotState::Open,
                seat_id: None,
                lease_expires_unix_ms: None,
                presentation: PlayerSlotPresentation::default(),
            },
            PlayerSlot {
                player_number: 3,
                state: PlayerSlotState::Disconnected,
                seat_id: Some("windows-seat-1".to_owned()),
                lease_expires_unix_ms: None,
                presentation: PlayerSlotPresentation::default(),
            },
            PlayerSlot {
                player_number: 4,
                state: PlayerSlotState::Disconnected,
                seat_id: Some("other-seat".to_owned()),
                lease_expires_unix_ms: None,
                presentation: PlayerSlotPresentation::default(),
            },
        ];

        assert_eq!(
            joinable_slots_for_seat(&slots, "windows-seat-1"),
            vec![
                JoinableSlot {
                    player_number: 2,
                    state: PlayerSlotState::Open,
                },
                JoinableSlot {
                    player_number: 3,
                    state: PlayerSlotState::Disconnected,
                },
            ]
        );
    }

    #[test]
    fn runtime_supervisor_starts_player_media_and_sends_input_without_process_token() {
        let launch = plan_native_player_launch(&session(), 2, PathBuf::from("ffplay")).unwrap();
        let token = launch.input.token;
        let mut supervisor = NativeRuntimeSupervisor::new(
            FakeMediaSupervisor::default(),
            FakeInputSender::default(),
        );

        supervisor.start_player(launch).unwrap();
        supervisor
            .send_player_input(input_protocol::button::ACTION_1, -1, 0)
            .unwrap();

        let status = supervisor.status().unwrap().unwrap();
        assert_eq!(status.mode, NativeRuntimeMode::Player);
        assert_eq!(status.player_number, Some(2));
        assert!(status.media_running);
        assert!(status.input_running);

        let spawned = &supervisor.media.spawned[0];
        assert_eq!(spawned.program, PathBuf::from("ffplay"));
        let process_json = serde_json::to_string(spawned).unwrap();
        assert!(!process_json.contains("00112233-4455-6677-8899-aabbccddeeff"));
        assert_eq!(supervisor.input.sent.len(), 1);
        assert_eq!(supervisor.input.sent[0].destination, "192.0.2.68:42000");
        assert_eq!(supervisor.input.sent[0].token, token);
        assert_eq!(supervisor.input.sent[0].state.sequence, 1);
        assert_eq!(
            supervisor.input.sent[0].state.buttons,
            input_protocol::button::ACTION_1
        );
        assert_eq!(supervisor.input.sent[0].state.axis_x, -1);
        assert_eq!(supervisor.input.sent[0].state.player_slot, 2);
    }

    #[test]
    fn runtime_supervisor_can_start_spectator_without_input() {
        let launch =
            plan_native_spectator_launch(&session(), &spectator_grant(), PathBuf::from("ffplay"));
        let mut supervisor = NativeRuntimeSupervisor::new(
            FakeMediaSupervisor::default(),
            FakeInputSender::default(),
        );

        supervisor.start_spectator(launch).unwrap();

        let status = supervisor.status().unwrap().unwrap();
        assert_eq!(status.mode, NativeRuntimeMode::Spectator);
        assert_eq!(status.media_udp_port, 41_002);
        assert!(status.media_running);
        assert!(!status.input_running);
        assert!(matches!(
            supervisor.send_player_input(0, 0, 0),
            Err(NativeRuntimeSupervisorError::NoPlayerRuntime)
        ));
        assert!(supervisor.input.sent.is_empty());
    }

    #[test]
    fn runtime_supervisor_stops_previous_runtime_before_switching() {
        let player = plan_native_player_launch(&session(), 1, PathBuf::from("ffplay")).unwrap();
        let spectator =
            plan_native_spectator_launch(&session(), &spectator_grant(), PathBuf::from("ffplay"));
        let mut supervisor = NativeRuntimeSupervisor::new(
            FakeMediaSupervisor::default(),
            FakeInputSender::default(),
        );

        supervisor.start_player(player).unwrap();
        supervisor.start_spectator(spectator).unwrap();

        assert_eq!(supervisor.media.spawned.len(), 2);
        assert_eq!(supervisor.media.stopped, vec![1]);
        assert_eq!(
            supervisor.status().unwrap().unwrap().mode,
            NativeRuntimeMode::Spectator
        );
    }

    #[test]
    fn runtime_supervisor_sends_stop_flag_to_player_runtime() {
        let launch = plan_native_player_launch(&session(), 1, PathBuf::from("ffplay")).unwrap();
        let mut supervisor = NativeRuntimeSupervisor::new(
            FakeMediaSupervisor::default(),
            FakeInputSender::default(),
        );

        supervisor.start_player(launch).unwrap();
        supervisor.send_player_stop().unwrap();

        assert_eq!(supervisor.input.sent[0].state.flags, FLAG_STOP);
        assert_eq!(supervisor.input.sent[0].state.buttons, 0);
        assert_eq!(supervisor.input.sent[0].state.axis_x, 0);
        assert_eq!(supervisor.input.sent[0].state.axis_y, 0);
    }

    #[test]
    fn native_seat_app_start_game_launches_runtime_and_exposes_safe_status() {
        let mut app = app(FakeApi::default());

        let status = app.start_game("tmnt").unwrap();

        assert_eq!(status.mode, NativeRuntimeMode::Player);
        assert_eq!(status.session_id, "session-1");
        assert_eq!(status.player_number, Some(1));
        assert_eq!(
            status.input_destination,
            Some("192.0.2.68:42000".to_owned())
        );
        assert!(status.media_running);
        assert!(status.input_running);
        assert_eq!(app.controller.api.calls, vec!["create_session:tmnt"]);
        assert_eq!(app.supervisor.media.spawned.len(), 1);

        let status_json = serde_json::to_string(&status).unwrap();
        assert!(!status_json.contains("00112233-4455-6677-8899-aabbccddeeff"));
        assert!(!status_json.contains("token"));
    }

    #[test]
    fn native_seat_app_spectate_then_join_switches_local_runtime_and_input() {
        let mut app = app(FakeApi::default());

        let spectator = app.spectate_session("session-1").unwrap();
        assert_eq!(spectator.mode, NativeRuntimeMode::Spectator);
        assert_eq!(spectator.player_number, None);
        assert_eq!(app.supervisor.media.spawned.len(), 1);

        let player = app.join_from_spectator("session-1", 2).unwrap();
        assert_eq!(player.mode, NativeRuntimeMode::Player);
        assert_eq!(player.player_number, Some(2));
        assert_eq!(app.supervisor.media.spawned.len(), 2);
        assert_eq!(app.supervisor.media.stopped, vec![1]);
        assert_eq!(
            app.controller.api.calls,
            vec![
                "create_spectator:session-1".to_owned(),
                "get_session:session-1".to_owned(),
                "release_spectator:session-1:grant-1".to_owned(),
                "reserve:session-1:2".to_owned(),
                "connect:session-1:2".to_owned(),
            ]
        );

        app.send_player_input(input_protocol::button::START, 0, 0)
            .unwrap();
        assert_eq!(app.supervisor.input.sent[0].state.player_slot, 2);
        assert_eq!(
            app.supervisor.input.sent[0].state.buttons,
            input_protocol::button::START
        );
    }

    #[test]
    fn native_seat_app_leave_player_slot_disconnects_and_stops_runtime() {
        let mut app = app(FakeApi::default());

        app.start_game("tmnt").unwrap();
        app.leave_player_slot("session-1", 1).unwrap();

        assert_eq!(app.supervisor.media.stopped, vec![1]);
        assert!(app.runtime_status().unwrap().is_none());
        assert_eq!(
            app.controller.api.calls,
            vec!["create_session:tmnt", "disconnect:session-1:1"]
        );
    }

    #[test]
    fn native_seat_config_store_round_trips_config() {
        let store = NativeSeatConfigStore::new(temp_config_path("round-trip"));
        let config = seat_config();

        store.save(&config).unwrap();
        let loaded = store.load_with_token("seat-token").unwrap();

        assert_eq!(loaded, config);
        let text = std::fs::read_to_string(store.path()).unwrap();
        assert!(text.contains("\"control_plane_url\""));
        assert!(!text.contains("seat-token"));
        assert!(!text.contains("seat_api_token"));
        let _ = std::fs::remove_file(store.path());
    }

    #[test]
    fn native_seat_config_file_rejects_invalid_destination_address() {
        let config = NativeSeatConfigFile {
            control_plane_url: "http://192.0.2.68:8080".to_owned(),
            seat_id: "windows-seat-1".to_owned(),
            destination_address: "not-an-ip".to_owned(),
            ffplay_path: PathBuf::from("ffplay"),
        };

        assert!(matches!(
            config.into_config("seat-token"),
            Err(NativeSeatConfigError::Address(_))
        ));
    }

    #[test]
    fn native_seat_config_file_requires_runtime_token_overlay() {
        let config = NativeSeatConfigFile {
            control_plane_url: "http://192.0.2.68:8080".to_owned(),
            seat_id: "windows-seat-1".to_owned(),
            destination_address: "192.0.2.10".to_owned(),
            ffplay_path: PathBuf::from("ffplay"),
        };

        assert!(matches!(
            config.into_config(""),
            Err(NativeSeatConfigError::Config(
                SeatClientConfigError::MissingSeatApiToken
            ))
        ));
    }

    #[test]
    fn native_seat_view_model_builds_ready_to_render_cards_without_tokens() {
        let mut summary = session_summary();
        summary.player_slots.push(PlayerSlot {
            player_number: 2,
            state: PlayerSlotState::Open,
            seat_id: None,
            lease_expires_unix_ms: None,
            presentation: PlayerSlotPresentation::default(),
        });
        summary.active_spectator_count = 1;
        summary.preview_asset_path = Some("previews/session-1.jpg".to_owned());
        let runtime = NativeRuntimeStatus {
            mode: NativeRuntimeMode::Spectator,
            session_id: "session-1".to_owned(),
            game_id: "tmnt".to_owned(),
            media_udp_port: 41_002,
            input_destination: None,
            player_number: None,
            media_running: true,
            input_running: false,
        };

        let view_model = build_native_seat_view_model(
            NativeSeatSnapshot {
                games: vec![catalog_game()],
                active_sessions: vec![summary],
            },
            Some(runtime),
            "windows-seat-2",
        );

        assert_eq!(view_model.games[0].id, "tmnt");
        assert_eq!(
            view_model.active_sessions[0].display_name,
            "Teenage Mutant Ninja Turtles"
        );
        assert_eq!(view_model.active_sessions[0].joinable_slots.len(), 1);
        assert_eq!(
            view_model.active_sessions[0].joinable_slots[0],
            NativeJoinableSlot {
                player_number: 2,
                state: PlayerSlotState::Open,
            }
        );
        assert!(view_model.active_sessions[0].can_spectate);
        assert_eq!(view_model.active_sessions[0].active_spectator_count, 1);

        let json = serde_json::to_string(&view_model).unwrap();
        assert!(!json.contains("00112233-4455-6677-8899-aabbccddeeff"));
        assert!(!json.contains("token"));
    }

    #[test]
    fn native_seat_view_model_marks_terminal_sessions_not_spectatable() {
        let mut summary = session_summary();
        summary.state = SessionState::Stopped;

        let view_model = build_native_seat_view_model(
            NativeSeatSnapshot {
                games: vec![catalog_game()],
                active_sessions: vec![summary],
            },
            None,
            "windows-seat-1",
        );

        assert!(!view_model.active_sessions[0].can_spectate);
    }

    #[test]
    fn native_controller_tracker_preserves_simultaneous_buttons_and_axes() {
        let mut tracker = NativeControllerStateTracker::new();

        tracker.set_button(NativeInputButton::Action1, true);
        let state = tracker.set_button(NativeInputButton::Action2, true);
        assert_eq!(
            state.buttons,
            input_protocol::button::ACTION_1 | input_protocol::button::ACTION_2
        );

        let state = tracker.set_axis(-10, 2);
        assert_eq!(state.axis_x, -1);
        assert_eq!(state.axis_y, 1);
        assert_eq!(
            state.buttons,
            input_protocol::button::ACTION_1 | input_protocol::button::ACTION_2
        );

        let state = tracker.set_button(NativeInputButton::Action1, false);
        assert_eq!(state.buttons, input_protocol::button::ACTION_2);

        let state = tracker.neutralize();
        assert_eq!(state, NativeControllerInput::default());
    }

    #[test]
    fn native_seat_app_sends_controller_tracker_state() {
        let mut app = app(FakeApi::default());
        let mut tracker = NativeControllerStateTracker::new();

        app.start_game("tmnt").unwrap();
        tracker.set_button(NativeInputButton::Start, true);
        tracker.set_axis(1, 0);
        app.send_controller_state(tracker.state()).unwrap();

        assert_eq!(app.supervisor.input.sent[0].state.player_slot, 1);
        assert_eq!(
            app.supervisor.input.sent[0].state.buttons,
            input_protocol::button::START
        );
        assert_eq!(app.supervisor.input.sent[0].state.axis_x, 1);
    }

    #[test]
    fn native_ui_session_refresh_returns_view_model() {
        let mut ui = NativeSeatUiSession::new(app(FakeApi::default()));

        let response = ui.handle_command(NativeUiCommand::Refresh).unwrap();

        match response {
            NativeUiResponse::ViewModel(view_model) => {
                assert_eq!(view_model.games[0].id, "tmnt");
                assert_eq!(view_model.active_sessions[0].id, "session-1");
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn native_ui_session_start_and_input_commands_drive_runtime() {
        let mut ui = NativeSeatUiSession::new(app(FakeApi::default()));

        let response = ui
            .handle_command(NativeUiCommand::StartGame {
                game_id: "tmnt".to_owned(),
            })
            .unwrap();
        assert!(matches!(
            response,
            NativeUiResponse::RuntimeStatus(NativeRuntimeStatus {
                mode: NativeRuntimeMode::Player,
                ..
            })
        ));

        let response = ui
            .handle_command(NativeUiCommand::SetButton {
                button: NativeInputButton::Action1,
                pressed: true,
            })
            .unwrap();
        assert_eq!(
            response,
            NativeUiResponse::InputState(NativeControllerInput {
                buttons: input_protocol::button::ACTION_1,
                axis_x: 0,
                axis_y: 0,
            })
        );
        assert_eq!(
            ui.app.supervisor.input.sent[0].state.buttons,
            input_protocol::button::ACTION_1
        );

        let response = ui
            .handle_command(NativeUiCommand::SetAxis {
                axis_x: 1,
                axis_y: -1,
            })
            .unwrap();
        assert_eq!(
            response,
            NativeUiResponse::InputState(NativeControllerInput {
                buttons: input_protocol::button::ACTION_1,
                axis_x: 1,
                axis_y: -1,
            })
        );
        assert_eq!(ui.app.supervisor.input.sent[1].state.axis_y, -1);
    }

    #[test]
    fn native_ui_command_serializes_with_stable_names() {
        let command = NativeUiCommand::JoinFromSpectator {
            session_id: "session-1".to_owned(),
            player_number: 2,
        };

        assert_eq!(
            serde_json::to_value(command).unwrap(),
            serde_json::json!({
                "type": "join_from_spectator",
                "session_id": "session-1",
                "player_number": 2
            })
        );
    }

    fn controller(api: FakeApi) -> NativeSeatController<FakeApi> {
        NativeSeatController::new(seat_config(), api).unwrap()
    }

    fn app(api: FakeApi) -> NativeSeatApp<FakeApi, FakeMediaSupervisor, FakeInputSender> {
        NativeSeatApp::new(
            seat_config(),
            api,
            FakeMediaSupervisor::default(),
            FakeInputSender::default(),
        )
        .unwrap()
    }

    fn seat_config() -> SeatClientConfig {
        SeatClientConfig {
            control_plane_url: "http://192.0.2.68:8080".to_owned(),
            seat_id: "windows-seat-1".to_owned(),
            destination_address: "192.0.2.10".parse().unwrap(),
            seat_api_token: "seat-token".to_owned(),
            ffplay_path: PathBuf::from("ffplay"),
        }
    }

    fn temp_config_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "4play-seat-client-native-{label}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[derive(Debug, Clone, Default)]
    struct FakeMediaSupervisor {
        spawned: Vec<ProcessSpec>,
        stopped: Vec<u32>,
        next_handle: u32,
    }

    impl MediaProcessSupervisor for FakeMediaSupervisor {
        type Error = String;
        type Handle = u32;

        fn spawn_media(&mut self, spec: &ProcessSpec) -> Result<Self::Handle, Self::Error> {
            self.spawned.push(spec.clone());
            self.next_handle += 1;
            Ok(self.next_handle)
        }

        fn stop_media(&mut self, handle: &mut Self::Handle) -> Result<(), Self::Error> {
            self.stopped.push(*handle);
            Ok(())
        }

        fn media_running(&mut self, _handle: &mut Self::Handle) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

    #[derive(Debug, Clone, Default)]
    struct FakeInputSender {
        sent: Vec<SentInput>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SentInput {
        destination: String,
        token: SessionToken,
        state: ControllerState,
    }

    impl InputPacketSender for FakeInputSender {
        type Error = String;

        fn send_input(
            &mut self,
            destination: &str,
            token: SessionToken,
            state: ControllerState,
        ) -> Result<(), Self::Error> {
            self.sent.push(SentInput {
                destination: destination.to_owned(),
                token,
                state,
            });
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct FakeApi {
        calls: Vec<String>,
        session: Session,
        spectator_grant: SpectatorGrant,
    }

    impl Default for FakeApi {
        fn default() -> Self {
            Self {
                calls: Vec::new(),
                session: session(),
                spectator_grant: spectator_grant(),
            }
        }
    }

    impl ControlPlaneApi for FakeApi {
        type Error = String;

        fn list_games(&mut self) -> Result<CatalogGameList, Self::Error> {
            self.calls.push("list_games".to_owned());
            Ok(CatalogGameList {
                games: vec![catalog_game()],
            })
        }

        fn list_active_sessions(&mut self) -> Result<SessionSummaryList, Self::Error> {
            self.calls.push("list_active_sessions".to_owned());
            Ok(SessionSummaryList {
                sessions: vec![session_summary()],
            })
        }

        fn get_session(&mut self, session_id: &str) -> Result<Session, Self::Error> {
            self.calls.push(format!("get_session:{session_id}"));
            Ok(self.session.clone())
        }

        fn create_session(
            &mut self,
            request: CreateSessionRequest,
        ) -> Result<Session, Self::Error> {
            self.calls
                .push(format!("create_session:{}", request.game_id));
            Ok(self.session.clone())
        }

        fn reserve_player_slot(
            &mut self,
            session_id: &str,
            player_number: u32,
            _request: ReservePlayerSlotRequest,
        ) -> Result<Session, Self::Error> {
            self.calls
                .push(format!("reserve:{session_id}:{player_number}"));
            Ok(self.session.clone())
        }

        fn connect_player_slot(
            &mut self,
            session_id: &str,
            player_number: u32,
            _request: ReservePlayerSlotRequest,
        ) -> Result<Session, Self::Error> {
            self.calls
                .push(format!("connect:{session_id}:{player_number}"));
            Ok(self.session.clone())
        }

        fn disconnect_player_slot(
            &mut self,
            session_id: &str,
            player_number: u32,
            _request: ReservePlayerSlotRequest,
        ) -> Result<Session, Self::Error> {
            self.calls
                .push(format!("disconnect:{session_id}:{player_number}"));
            Ok(self.session.clone())
        }

        fn create_spectator_grant(
            &mut self,
            session_id: &str,
            _request: CreateSpectatorGrantRequest,
        ) -> Result<SpectatorGrant, Self::Error> {
            self.calls.push(format!("create_spectator:{session_id}"));
            Ok(self.spectator_grant.clone())
        }

        fn release_spectator_grant(
            &mut self,
            session_id: &str,
            grant_id: &str,
            _request: ReservePlayerSlotRequest,
        ) -> Result<Session, Self::Error> {
            self.calls
                .push(format!("release_spectator:{session_id}:{grant_id}"));
            Ok(self.session.clone())
        }
    }

    fn session() -> Session {
        Session {
            id: "session-1".to_owned(),
            game_id: "tmnt".to_owned(),
            seat_id: "windows-seat-1".to_owned(),
            destination_address: "192.0.2.10".to_owned(),
            runtime_host_id: "reference-linux".to_owned(),
            runtime_profile: GameRuntimeProfile {
                width: 320,
                height: 224,
                refresh_hz: 60.0,
                rotation_degrees: 0,
                max_players: 4,
                buttons_per_player: 2,
                supports_save_state: true,
            },
            state: SessionState::Active,
            connection_grant: connection_grant(),
            player_slots: vec![PlayerSlot {
                player_number: 1,
                state: PlayerSlotState::Occupied,
                seat_id: Some("windows-seat-1".to_owned()),
                lease_expires_unix_ms: None,
                presentation: PlayerSlotPresentation::default(),
            }],
            active_spectator_count: 0,
            created_unix_ms: 1,
            updated_unix_ms: 2,
            failure_reason: None,
        }
    }

    fn connection_grant() -> ConnectionGrant {
        ConnectionGrant {
            token: "00112233-4455-6677-8899-aabbccddeeff".to_owned(),
            expires_unix_ms: 999,
            runtime_host_id: "reference-linux".to_owned(),
            runtime_host_address: "192.0.2.68".to_owned(),
            media_udp_port: 41_000,
            input_udp_port: 42_000,
        }
    }

    fn spectator_grant() -> SpectatorGrant {
        SpectatorGrant {
            id: "grant-1".to_owned(),
            session_id: "session-1".to_owned(),
            seat_id: "windows-seat-2".to_owned(),
            destination_address: "192.0.2.10".to_owned(),
            media_udp_port: 41_002,
            runtime_host_id: "reference-linux".to_owned(),
            runtime_host_address: "192.0.2.68".to_owned(),
            expires_unix_ms: 999,
        }
    }

    fn catalog_game() -> CatalogGame {
        CatalogGame {
            id: "tmnt".to_owned(),
            display_name: "Teenage Mutant Ninja Turtles".to_owned(),
            rom_name: "tmnt".to_owned(),
            metadata: GameMetadata::default(),
            availability: vec![GameAvailability {
                runtime_host_id: "reference-linux".to_owned(),
                runtime_host_status: RuntimeHostStatus::Online,
                profile: runtime_profile(),
            }],
        }
    }

    fn session_summary() -> SessionSummary {
        SessionSummary {
            id: "session-1".to_owned(),
            game_id: "tmnt".to_owned(),
            runtime_host_id: "reference-linux".to_owned(),
            runtime_profile: runtime_profile(),
            state: SessionState::Active,
            player_slots: session().player_slots,
            active_spectator_count: 0,
            preview_status: PreviewStatus::Unavailable,
            preview_asset_path: None,
            preview_updated_unix_ms: None,
            updated_unix_ms: 2,
        }
    }

    fn runtime_profile() -> GameRuntimeProfile {
        GameRuntimeProfile {
            width: 320,
            height: 224,
            refresh_hz: 60.0,
            rotation_degrees: 0,
            max_players: 4,
            buttons_per_player: 2,
            supports_save_state: true,
        }
    }
}
