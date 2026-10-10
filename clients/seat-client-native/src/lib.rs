use std::path::PathBuf;

use control_protocol::{
    CatalogGame, CatalogGameList, CreateSessionRequest, CreateSpectatorGrantRequest, PlayerSlot,
    PlayerSlotState, ReservePlayerSlotRequest, Session, SessionSummary, SessionSummaryList,
    SpectatorGrant,
};
use input_protocol::SessionToken;
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

#[derive(Debug)]
pub enum NativeClientError<ApiError> {
    Api(ApiError),
    Config(SeatClientConfigError),
    RuntimePlan(RuntimePlanError),
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

    fn controller(api: FakeApi) -> NativeSeatController<FakeApi> {
        NativeSeatController::new(
            SeatClientConfig {
                control_plane_url: "http://192.0.2.68:8080".to_owned(),
                seat_id: "windows-seat-1".to_owned(),
                destination_address: "192.0.2.10".parse().unwrap(),
                seat_api_token: "seat-token".to_owned(),
                ffplay_path: PathBuf::from("ffplay"),
            },
            api,
        )
        .unwrap()
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
