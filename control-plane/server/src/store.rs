use std::{collections::BTreeMap, error::Error, fmt, path::Path};

use control_protocol::{
    CatalogGame, ConnectionGrant, CreateSessionRequest, GameAvailability, GameRuntimeProfile,
    PlayerSlot, PlayerSlotState, RegisterRuntimeHost, RuntimeHost, RuntimeHostCapabilities,
    RuntimeHostCatalog, RuntimeHostHeartbeat, RuntimeHostStatus, RuntimeSessionAssignment, Session,
    SessionState,
};
use tokio_rusqlite::{Connection, params, rusqlite::OptionalExtension};

const SCHEMA: &str = "
    PRAGMA foreign_keys = ON;
    CREATE TABLE IF NOT EXISTS runtime_hosts (
        id TEXT PRIMARY KEY NOT NULL,
        display_name TEXT NOT NULL,
        agent_version TEXT NOT NULL,
        capabilities_json TEXT NOT NULL,
        status TEXT NOT NULL,
        last_seen_unix_ms INTEGER NOT NULL,
        heartbeat_sequence INTEGER NOT NULL,
        active_session_count INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS runtime_hosts_last_seen
        ON runtime_hosts(last_seen_unix_ms);
    CREATE TABLE IF NOT EXISTS games (
        id TEXT PRIMARY KEY NOT NULL,
        display_name TEXT NOT NULL,
        rom_name TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS runtime_profiles (
        runtime_host_id TEXT NOT NULL,
        game_id TEXT NOT NULL,
        profile_json TEXT NOT NULL,
        PRIMARY KEY (runtime_host_id, game_id),
        FOREIGN KEY (runtime_host_id) REFERENCES runtime_hosts(id) ON DELETE CASCADE,
        FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE
    );
    CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY NOT NULL,
        game_id TEXT NOT NULL,
        seat_id TEXT NOT NULL,
        destination_address TEXT NOT NULL,
        runtime_host_id TEXT NOT NULL,
        runtime_host_address TEXT NOT NULL,
        runtime_profile_json TEXT NOT NULL,
        state TEXT NOT NULL,
        grant_token TEXT NOT NULL UNIQUE,
        grant_expires_unix_ms INTEGER NOT NULL,
        media_udp_port INTEGER NOT NULL,
        input_udp_port INTEGER NOT NULL,
        player_slots_json TEXT NOT NULL DEFAULT '[]',
        created_unix_ms INTEGER NOT NULL,
        updated_unix_ms INTEGER NOT NULL,
        failure_reason TEXT,
        FOREIGN KEY (game_id) REFERENCES games(id),
        FOREIGN KEY (runtime_host_id) REFERENCES runtime_hosts(id)
    );
    CREATE INDEX IF NOT EXISTS sessions_runtime_state
        ON sessions(runtime_host_id, state);
    CREATE INDEX IF NOT EXISTS sessions_seat_state
        ON sessions(seat_id, state);
    CREATE TABLE IF NOT EXISTS session_events (
        sequence INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id TEXT NOT NULL,
        state TEXT NOT NULL,
        occurred_unix_ms INTEGER NOT NULL,
        detail TEXT,
        FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
    );
";

type AssignmentRow = (
    String,
    String,
    String,
    String,
    i64,
    i64,
    String,
    String,
    String,
);

#[derive(Clone)]
pub struct RuntimeHostStore {
    connection: Connection,
}

impl RuntimeHostStore {
    pub async fn in_memory() -> Result<Self, StoreError> {
        let connection = Connection::open_in_memory()
            .await
            .map_err(StoreError::database)?;
        Self::initialize(connection).await
    }

    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let connection = Connection::open(path).await.map_err(StoreError::database)?;
        Self::initialize(connection).await
    }

    async fn initialize(connection: Connection) -> Result<Self, StoreError> {
        connection
            .call(|connection| connection.execute_batch(SCHEMA))
            .await
            .map_err(StoreError::database)?;
        connection
            .call(|connection| -> tokio_rusqlite::rusqlite::Result<()> {
                let has_player_slots = connection
                    .prepare("PRAGMA table_info(sessions)")?
                    .query_map([], |row| row.get::<_, String>(1))?
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .any(|name| name == "player_slots_json");
                if !has_player_slots {
                    connection.execute(
                        "ALTER TABLE sessions
                         ADD COLUMN player_slots_json TEXT NOT NULL DEFAULT '[]'",
                        [],
                    )?;
                }
                Ok(())
            })
            .await
            .map_err(StoreError::database)?;
        Ok(Self { connection })
    }

    pub async fn ping(&self) -> Result<(), StoreError> {
        self.connection
            .call(|connection| connection.query_row("SELECT 1", [], |_| Ok(())))
            .await
            .map_err(StoreError::database)
    }

    pub async fn upsert_registration(
        &self,
        host_id: String,
        registration: RegisterRuntimeHost,
        now_unix_ms: u64,
    ) -> Result<(bool, RuntimeHost), StoreError> {
        let capabilities_json =
            serde_json::to_string(&registration.capabilities).map_err(StoreError::serialization)?;
        let now = to_sql_integer(now_unix_ms, "last_seen_unix_ms")?;
        let display_name = registration.display_name.clone();
        let agent_version = registration.agent_version.clone();
        let stored_id = host_id.clone();
        let stored_capabilities = capabilities_json.clone();

        let (created, sequence, active_session_count) = self
            .connection
            .call(
                move |connection| -> tokio_rusqlite::rusqlite::Result<(bool, i64, i64)> {
                    let previous = connection
                        .query_row(
                            "SELECT heartbeat_sequence, active_session_count
                         FROM runtime_hosts WHERE id = ?1",
                            [&stored_id],
                            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                        )
                        .optional()?;
                    let (sequence, active_session_count) = previous.unwrap_or((0, 0));
                    connection.execute(
                        "INSERT INTO runtime_hosts (
                        id, display_name, agent_version, capabilities_json, status,
                        last_seen_unix_ms, heartbeat_sequence, active_session_count
                     ) VALUES (?1, ?2, ?3, ?4, 'online', ?5, ?6, ?7)
                     ON CONFLICT(id) DO UPDATE SET
                        display_name = excluded.display_name,
                        agent_version = excluded.agent_version,
                        capabilities_json = excluded.capabilities_json,
                        status = 'online',
                        last_seen_unix_ms = excluded.last_seen_unix_ms",
                        params![
                            stored_id,
                            display_name,
                            agent_version,
                            stored_capabilities,
                            now,
                            sequence,
                            active_session_count
                        ],
                    )?;
                    Ok((previous.is_none(), sequence, active_session_count))
                },
            )
            .await
            .map_err(StoreError::database)?;

        Ok((
            created,
            RuntimeHost {
                id: host_id,
                display_name: registration.display_name,
                agent_version: registration.agent_version,
                capabilities: registration.capabilities,
                status: RuntimeHostStatus::Online,
                last_seen_unix_ms: now_unix_ms,
                heartbeat_sequence: from_sql_integer(sequence, "heartbeat_sequence")?,
                active_session_count: u32::try_from(active_session_count).map_err(|_| {
                    StoreError::data("active_session_count is outside the supported range")
                })?,
            },
        ))
    }

    pub async fn heartbeat(
        &self,
        host_id: String,
        heartbeat: RuntimeHostHeartbeat,
        now_unix_ms: u64,
    ) -> Result<RuntimeHost, StoreError> {
        let now = to_sql_integer(now_unix_ms, "last_seen_unix_ms")?;
        let sequence = to_sql_integer(heartbeat.sequence, "heartbeat_sequence")?;
        let active_session_count = i64::from(heartbeat.active_session_count);
        let stored = self
            .connection
            .call(
                move |connection| -> tokio_rusqlite::rusqlite::Result<
                    Result<StoredHost, HeartbeatRejection>,
                > {
                    let previous = connection
                        .query_row(
                            "SELECT id, display_name, agent_version, capabilities_json,
                                status, last_seen_unix_ms, heartbeat_sequence,
                                active_session_count
                         FROM runtime_hosts WHERE id = ?1",
                            [&host_id],
                            StoredHost::from_row,
                        )
                        .optional()?;
                    let Some(mut host) = previous else {
                        return Ok(Err(HeartbeatRejection::NotFound));
                    };
                    if sequence < host.heartbeat_sequence {
                        return Ok(Err(HeartbeatRejection::Stale));
                    }
                    if sequence == host.heartbeat_sequence
                        && active_session_count != host.active_session_count
                    {
                        return Ok(Err(HeartbeatRejection::SequenceConflict));
                    }

                    connection.execute(
                        "UPDATE runtime_hosts
                     SET status = 'online', last_seen_unix_ms = ?2,
                         heartbeat_sequence = ?3, active_session_count = ?4
                     WHERE id = ?1",
                        params![host_id, now, sequence, active_session_count],
                    )?;
                    host.status = "online".to_owned();
                    host.last_seen_unix_ms = now;
                    host.heartbeat_sequence = sequence;
                    host.active_session_count = active_session_count;
                    Ok(Ok(host))
                },
            )
            .await
            .map_err(StoreError::database)?
            .map_err(StoreError::from_heartbeat_rejection)?;
        stored.into_runtime_host()
    }

    pub async fn get(
        &self,
        host_id: String,
        now_unix_ms: u64,
        offline_after_ms: u64,
    ) -> Result<RuntimeHost, StoreError> {
        let cutoff = to_sql_integer(
            now_unix_ms.saturating_sub(offline_after_ms),
            "offline cutoff",
        )?;
        let now = to_sql_integer(now_unix_ms, "current timestamp")?;
        let stored = self
            .connection
            .call(move |connection| {
                mark_expired_offline(connection, cutoff, now)?;
                connection
                    .query_row(
                        "SELECT id, display_name, agent_version, capabilities_json,
                                status, last_seen_unix_ms, heartbeat_sequence,
                                active_session_count
                         FROM runtime_hosts WHERE id = ?1",
                        [&host_id],
                        StoredHost::from_row,
                    )
                    .optional()
            })
            .await
            .map_err(StoreError::database)?
            .ok_or(StoreError::NotFound)?;
        stored.into_runtime_host()
    }

    pub async fn list(
        &self,
        now_unix_ms: u64,
        offline_after_ms: u64,
    ) -> Result<Vec<RuntimeHost>, StoreError> {
        let cutoff = to_sql_integer(
            now_unix_ms.saturating_sub(offline_after_ms),
            "offline cutoff",
        )?;
        let now = to_sql_integer(now_unix_ms, "current timestamp")?;
        let stored = self
            .connection
            .call(move |connection| {
                mark_expired_offline(connection, cutoff, now)?;
                let mut statement = connection.prepare(
                    "SELECT id, display_name, agent_version, capabilities_json,
                            status, last_seen_unix_ms, heartbeat_sequence,
                            active_session_count
                     FROM runtime_hosts ORDER BY id",
                )?;
                statement
                    .query_map([], StoredHost::from_row)?
                    .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(StoreError::database)?;
        stored
            .into_iter()
            .map(StoredHost::into_runtime_host)
            .collect()
    }

    pub async fn replace_host_catalog(
        &self,
        host_id: String,
        catalog: RuntimeHostCatalog,
    ) -> Result<(), StoreError> {
        let games = catalog
            .games
            .into_iter()
            .map(|game| {
                serde_json::to_string(&game.profile)
                    .map(|profile_json| (game.id, game.display_name, game.rom_name, profile_json))
                    .map_err(StoreError::serialization)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.connection
            .call(
                move |connection| -> tokio_rusqlite::rusqlite::Result<Result<(), ()>> {
                    let host_exists = connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM runtime_hosts WHERE id = ?1)",
                        [&host_id],
                        |row| row.get::<_, bool>(0),
                    )?;
                    if !host_exists {
                        return Ok(Err(()));
                    }
                    let transaction = connection.transaction()?;
                    transaction.execute(
                        "DELETE FROM runtime_profiles WHERE runtime_host_id = ?1",
                        [&host_id],
                    )?;
                    for (game_id, display_name, rom_name, profile_json) in games {
                        transaction.execute(
                            "INSERT INTO games (id, display_name, rom_name)
                         VALUES (?1, ?2, ?3)
                         ON CONFLICT(id) DO UPDATE SET
                            display_name = excluded.display_name,
                            rom_name = excluded.rom_name",
                            params![game_id, display_name, rom_name],
                        )?;
                        transaction.execute(
                            "INSERT INTO runtime_profiles
                            (runtime_host_id, game_id, profile_json)
                         VALUES (?1, ?2, ?3)",
                            params![host_id, game_id, profile_json],
                        )?;
                    }
                    transaction.commit()?;
                    Ok(Ok(()))
                },
            )
            .await
            .map_err(StoreError::database)?
            .map_err(|()| StoreError::NotFound)
    }

    pub async fn list_catalog(
        &self,
        now_unix_ms: u64,
        offline_after_ms: u64,
    ) -> Result<Vec<CatalogGame>, StoreError> {
        let cutoff = to_sql_integer(
            now_unix_ms.saturating_sub(offline_after_ms),
            "offline cutoff",
        )?;
        let now = to_sql_integer(now_unix_ms, "current timestamp")?;
        let rows = self
            .connection
            .call(move |connection| {
                mark_expired_offline(connection, cutoff, now)?;
                let mut statement = connection.prepare(
                    "SELECT g.id, g.display_name, g.rom_name, rp.runtime_host_id,
                            h.status, rp.profile_json
                     FROM games g
                     JOIN runtime_profiles rp ON rp.game_id = g.id
                     JOIN runtime_hosts h ON h.id = rp.runtime_host_id
                     ORDER BY g.id, rp.runtime_host_id",
                )?;
                statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(StoreError::database)?;

        let mut games = BTreeMap::<String, CatalogGame>::new();
        for (game_id, display_name, rom_name, host_id, status, profile_json) in rows {
            let runtime_host_status = match status.as_str() {
                "online" => RuntimeHostStatus::Online,
                "offline" => RuntimeHostStatus::Offline,
                _ => return Err(StoreError::data("runtime host has an unknown status")),
            };
            let profile = serde_json::from_str::<GameRuntimeProfile>(&profile_json)
                .map_err(StoreError::serialization)?;
            games
                .entry(game_id.clone())
                .or_insert_with(|| CatalogGame {
                    id: game_id,
                    display_name,
                    rom_name,
                    availability: Vec::new(),
                })
                .availability
                .push(GameAvailability {
                    runtime_host_id: host_id,
                    runtime_host_status,
                    profile,
                });
        }
        Ok(games.into_values().collect())
    }

    pub async fn get_catalog_game(
        &self,
        game_id: String,
        now_unix_ms: u64,
        offline_after_ms: u64,
    ) -> Result<CatalogGame, StoreError> {
        self.list_catalog(now_unix_ms, offline_after_ms)
            .await?
            .into_iter()
            .find(|game| game.id == game_id)
            .ok_or(StoreError::GameNotFound)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn allocate_session(
        &self,
        session_id: String,
        grant_token: String,
        request: CreateSessionRequest,
        now_unix_ms: u64,
        grant_expires_unix_ms: u64,
        offline_after_ms: u64,
        media_port_start: u16,
        input_port_start: u16,
        port_count: u16,
    ) -> Result<Session, StoreError> {
        let now = to_sql_integer(now_unix_ms, "session timestamp")?;
        let grant_expires = to_sql_integer(grant_expires_unix_ms, "grant expiry")?;
        let cutoff = to_sql_integer(
            now_unix_ms.saturating_sub(offline_after_ms),
            "offline cutoff",
        )?;
        let stored = self
            .connection
            .call(move |connection| -> tokio_rusqlite::rusqlite::Result<Result<StoredSession, AllocationRejection>> {
                let transaction = connection.transaction()?;
                mark_expired_offline(&transaction, cutoff, now)?;

                let seat_busy = transaction.query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM sessions
                        WHERE seat_id = ?1 AND state NOT IN
                            ('stopped', 'allocation_failed', 'launch_failed', 'runtime_lost', 'terminated')
                    )",
                    [&request.seat_id],
                    |row| row.get::<_, bool>(0),
                )?;
                if seat_busy {
                    return Ok(Err(AllocationRejection::SeatBusy));
                }

                let game_exists = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM games WHERE id = ?1)",
                    [&request.game_id],
                    |row| row.get::<_, bool>(0),
                )?;
                if !game_exists {
                    return Ok(Err(AllocationRejection::GameNotFound));
                }

                let selected = transaction
                    .query_row(
                        "SELECT h.id, h.capabilities_json, rp.profile_json
                         FROM runtime_hosts h
                         JOIN runtime_profiles rp ON rp.runtime_host_id = h.id
                         WHERE rp.game_id = ?1 AND h.status = 'online'
                         ORDER BY (
                            SELECT COUNT(*) FROM sessions s
                            WHERE s.runtime_host_id = h.id AND s.state NOT IN
                                ('stopped', 'allocation_failed', 'launch_failed', 'runtime_lost', 'terminated')
                         ), h.id
                         LIMIT 1",
                        [&request.game_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
                    )
                    .optional()?;
                let Some((runtime_host_id, capabilities_json, runtime_profile_json)) = selected else {
                    return Ok(Err(AllocationRejection::GameUnavailable));
                };
                let capabilities: RuntimeHostCapabilities = match serde_json::from_str(&capabilities_json) {
                    Ok(value) => value,
                    Err(_) => return Ok(Err(AllocationRejection::InvalidHostCapabilities)),
                };
                if capabilities.data_plane_address.trim().is_empty() {
                    return Ok(Err(AllocationRejection::MissingDataPlaneAddress));
                }
                let runtime_profile: GameRuntimeProfile = match serde_json::from_str(&runtime_profile_json) {
                    Ok(value) => value,
                    Err(_) => return Ok(Err(AllocationRejection::InvalidRuntimeProfile)),
                };
                let player_slots_json = match serde_json::to_string(&initial_player_slots(
                    runtime_profile.max_players,
                    &request.seat_id,
                    grant_expires_unix_ms,
                )) {
                    Ok(value) => value,
                    Err(_) => return Ok(Err(AllocationRejection::InvalidRuntimeProfile)),
                };

                let mut selected_ports = None;
                for offset in 0..port_count {
                    let Some(media_port) = media_port_start.checked_add(offset) else { break };
                    let Some(input_port) = input_port_start.checked_add(offset) else { break };
                    let in_use = transaction.query_row(
                        "SELECT EXISTS(
                            SELECT 1 FROM sessions
                            WHERE runtime_host_id = ?1
                              AND state NOT IN ('stopped', 'allocation_failed', 'launch_failed', 'runtime_lost', 'terminated')
                              AND (media_udp_port = ?2 OR input_udp_port = ?3)
                        )",
                        params![runtime_host_id, i64::from(media_port), i64::from(input_port)],
                        |row| row.get::<_, bool>(0),
                    )?;
                    if !in_use {
                        selected_ports = Some((media_port, input_port));
                        break;
                    }
                }
                let Some((media_udp_port, input_udp_port)) = selected_ports else {
                    return Ok(Err(AllocationRejection::PortsExhausted));
                };

                transaction.execute(
                    "INSERT INTO sessions (
                        id, game_id, seat_id, destination_address, runtime_host_id,
                        runtime_host_address, runtime_profile_json, state, grant_token,
                        grant_expires_unix_ms, media_udp_port, input_udp_port, player_slots_json,
                        created_unix_ms, updated_unix_ms, failure_reason
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'allocating', ?8, ?9, ?10, ?11, ?12, ?13, ?13, NULL)",
                    params![
                        session_id,
                        request.game_id,
                        request.seat_id,
                        request.destination_address,
                        runtime_host_id,
                        capabilities.data_plane_address,
                        runtime_profile_json,
                        grant_token,
                        grant_expires,
                        i64::from(media_udp_port),
                        i64::from(input_udp_port),
                        player_slots_json,
                        now,
                    ],
                )?;
                transaction.execute(
                    "INSERT INTO session_events (session_id, state, occurred_unix_ms, detail)
                     VALUES (?1, 'requested', ?2, NULL), (?1, 'allocating', ?2, NULL)",
                    params![session_id, now],
                )?;
                let stored = transaction.query_row(
                    &session_select_sql("WHERE id = ?1"),
                    [&session_id],
                    StoredSession::from_row,
                )?;
                transaction.commit()?;
                Ok(Ok(stored))
            })
            .await
            .map_err(StoreError::database)?
            .map_err(StoreError::from_allocation_rejection)?;
        stored.into_session()
    }

    pub async fn get_session(&self, session_id: String) -> Result<Session, StoreError> {
        let stored = self
            .connection
            .call(move |connection| {
                connection
                    .query_row(
                        &session_select_sql("WHERE id = ?1"),
                        [&session_id],
                        StoredSession::from_row,
                    )
                    .optional()
            })
            .await
            .map_err(StoreError::database)?
            .ok_or(StoreError::SessionNotFound)?;
        stored.into_session()
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>, StoreError> {
        let stored = self
            .connection
            .call(move |connection| {
                let mut statement =
                    connection.prepare(&session_select_sql("ORDER BY created_unix_ms, id"))?;
                statement
                    .query_map([], StoredSession::from_row)?
                    .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(StoreError::database)?;
        stored
            .into_iter()
            .map(StoredSession::into_session)
            .collect()
    }

    pub async fn list_runtime_assignments(
        &self,
        runtime_host_id: String,
    ) -> Result<Vec<RuntimeSessionAssignment>, StoreError> {
        let rows = self
            .connection
            .call(
                move |connection| -> tokio_rusqlite::rusqlite::Result<Option<Vec<AssignmentRow>>> {
                let host_exists = connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM runtime_hosts WHERE id = ?1)",
                    [&runtime_host_id],
                    |row| row.get::<_, bool>(0),
                )?;
                if !host_exists {
                    return Ok(None);
                }
                let mut statement = connection.prepare(
                    "SELECT s.id, s.game_id, g.rom_name, s.destination_address,
                            s.media_udp_port, s.input_udp_port, s.grant_token,
                            s.runtime_profile_json, s.state
                     FROM sessions s
                     JOIN games g ON g.id = s.game_id
                     WHERE s.runtime_host_id = ?1
                       AND s.state NOT IN
                           ('stopped', 'allocation_failed', 'launch_failed', 'runtime_lost', 'terminated')
                     ORDER BY s.created_unix_ms, s.id",
                )?;
                let sessions = statement
                    .query_map([runtime_host_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                            row.get::<_, i64>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                            row.get::<_, String>(8)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                    Ok(Some(sessions))
                },
            )
            .await
            .map_err(StoreError::database)?
            .ok_or(StoreError::NotFound)?;
        rows.into_iter()
            .map(
                |(
                    session_id,
                    game_id,
                    rom_name,
                    destination_address,
                    media,
                    input,
                    input_token,
                    profile,
                    state,
                )| {
                    Ok(RuntimeSessionAssignment {
                        session_id,
                        game_id,
                        rom_name,
                        destination_address,
                        media_udp_port: u16::try_from(media).map_err(|_| {
                            StoreError::data("media UDP port is outside the supported range")
                        })?,
                        input_udp_port: u16::try_from(input).map_err(|_| {
                            StoreError::data("input UDP port is outside the supported range")
                        })?,
                        input_token,
                        runtime_profile: serde_json::from_str(&profile)
                            .map_err(StoreError::serialization)?,
                        state: parse_session_state(&state)?,
                    })
                },
            )
            .collect()
    }

    pub async fn update_session_state(
        &self,
        runtime_host_id: String,
        session_id: String,
        next_state: SessionState,
        failure_reason: Option<String>,
        now_unix_ms: u64,
    ) -> Result<Session, StoreError> {
        let now = to_sql_integer(now_unix_ms, "session timestamp")?;
        let next_state_name = session_state_name(next_state);
        let stored = self
            .connection
            .call(move |connection| -> tokio_rusqlite::rusqlite::Result<Result<StoredSession, StateUpdateRejection>> {
                let transaction = connection.transaction()?;
                let current = transaction
                    .query_row(
                        "SELECT runtime_host_id, state, failure_reason FROM sessions WHERE id = ?1",
                        [&session_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?)),
                    )
                    .optional()?;
                let Some((assigned_host_id, current_state_name, current_failure)) = current else {
                    return Ok(Err(StateUpdateRejection::SessionNotFound));
                };
                if assigned_host_id != runtime_host_id {
                    return Ok(Err(StateUpdateRejection::WrongRuntimeHost));
                }
                let current_state = match parse_session_state(&current_state_name) {
                    Ok(value) => value,
                    Err(_) => return Ok(Err(StateUpdateRejection::InvalidStoredState)),
                };
                if current_state == next_state {
                    if current_failure != failure_reason {
                        return Ok(Err(StateUpdateRejection::ConflictingRetry));
                    }
                } else {
                    if !valid_runtime_transition(current_state, next_state) {
                        return Ok(Err(StateUpdateRejection::InvalidTransition));
                    }
                    if is_failure_state(next_state)
                        && failure_reason.as_deref().is_none_or(|reason| reason.trim().is_empty())
                    {
                        return Ok(Err(StateUpdateRejection::MissingFailureReason));
                    }
                    transaction.execute(
                        "UPDATE sessions SET state = ?2, updated_unix_ms = ?3, failure_reason = ?4
                         WHERE id = ?1",
                        params![session_id, next_state_name, now, failure_reason],
                    )?;
                    transaction.execute(
                        "INSERT INTO session_events (session_id, state, occurred_unix_ms, detail)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![session_id, next_state_name, now, failure_reason],
                    )?;
                }
                let stored = transaction.query_row(
                    &session_select_sql("WHERE id = ?1"),
                    [&session_id],
                    StoredSession::from_row,
                )?;
                transaction.commit()?;
                Ok(Ok(stored))
            })
            .await
            .map_err(StoreError::database)?
            .map_err(StoreError::from_state_update_rejection)?;
        stored.into_session()
    }

    pub async fn request_session_stop(
        &self,
        session_id: String,
        now_unix_ms: u64,
    ) -> Result<Session, StoreError> {
        let now = to_sql_integer(now_unix_ms, "session timestamp")?;
        let stored = self
            .connection
            .call(
                move |connection| -> tokio_rusqlite::rusqlite::Result<Option<StoredSession>> {
                    let transaction = connection.transaction()?;
                    let state = transaction
                        .query_row(
                            "SELECT state FROM sessions WHERE id = ?1",
                            [&session_id],
                            |row| row.get::<_, String>(0),
                        )
                        .optional()?;
                    let Some(state) = state else {
                        return Ok(None);
                    };
                    let parsed = parse_session_state(&state).map_err(|error| {
                        tokio_rusqlite::rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                    })?;
                    if !parsed.is_terminal() && parsed != SessionState::Stopping {
                        transaction.execute(
                            "UPDATE sessions SET state = 'stopping', updated_unix_ms = ?2,
                         failure_reason = NULL WHERE id = ?1",
                            params![session_id, now],
                        )?;
                        transaction.execute(
                        "INSERT INTO session_events (session_id, state, occurred_unix_ms, detail)
                         VALUES (?1, 'stopping', ?2, 'stop requested')",
                        params![session_id, now],
                    )?;
                    }
                    let stored = transaction.query_row(
                        &session_select_sql("WHERE id = ?1"),
                        [&session_id],
                        StoredSession::from_row,
                    )?;
                    transaction.commit()?;
                    Ok(Some(stored))
                },
            )
            .await
            .map_err(StoreError::database)?
            .ok_or(StoreError::SessionNotFound)?;
        stored.into_session()
    }

    pub async fn reserve_player_slot(
        &self,
        session_id: String,
        player_number: u32,
        seat_id: String,
        now_unix_ms: u64,
        lease_duration_ms: u64,
    ) -> Result<Session, StoreError> {
        let now = to_sql_integer(now_unix_ms, "slot reservation timestamp")?;
        let lease_expires = to_sql_integer(
            now_unix_ms.saturating_add(lease_duration_ms),
            "slot reservation expiry",
        )?;
        let stored = self
            .connection
            .call(move |connection| -> tokio_rusqlite::rusqlite::Result<Result<StoredSession, SlotReservationRejection>> {
                let transaction = connection.transaction()?;
                let current = transaction
                    .query_row(
                        "SELECT state, player_slots_json FROM sessions WHERE id = ?1",
                        [&session_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()?;
                let Some((state_name, player_slots_json)) = current else {
                    return Ok(Err(SlotReservationRejection::SessionNotFound));
                };
                let state = match parse_session_state(&state_name) {
                    Ok(value) => value,
                    Err(_) => return Ok(Err(SlotReservationRejection::InvalidStoredState)),
                };
                if !matches!(state, SessionState::Ready | SessionState::Active) {
                    return Ok(Err(SlotReservationRejection::Unavailable));
                }

                let mut slots: Vec<PlayerSlot> = match serde_json::from_str(&player_slots_json) {
                    Ok(value) => value,
                    Err(_) => return Ok(Err(SlotReservationRejection::InvalidStoredSlots)),
                };
                expire_slot_leases(&mut slots, now_unix_ms);

                let already_claims_another_slot = slots.iter().any(|slot| {
                    slot.player_number != player_number
                        && slot.seat_id.as_deref() == Some(seat_id.as_str())
                        && !matches!(slot.state, PlayerSlotState::Open)
                });
                if already_claims_another_slot {
                    return Ok(Err(SlotReservationRejection::SeatBusy));
                }

                let seat_busy_elsewhere = transaction
                    .prepare(
                        "SELECT id, player_slots_json FROM sessions
                         WHERE id != ?1
                           AND state NOT IN
                            ('stopped', 'allocation_failed', 'launch_failed', 'runtime_lost', 'terminated')",
                    )?
                    .query_map([&session_id], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .any(|(_, slots_json)| {
                        serde_json::from_str::<Vec<PlayerSlot>>(&slots_json)
                            .map(|slots| {
                                slots.iter().any(|slot| {
                                    slot.seat_id.as_deref() == Some(seat_id.as_str())
                                        && !matches!(slot.state, PlayerSlotState::Open)
                                })
                            })
                            .unwrap_or(false)
                    });
                if seat_busy_elsewhere {
                    return Ok(Err(SlotReservationRejection::SeatBusy));
                }

                let Some(slot) = slots
                    .iter_mut()
                    .find(|slot| slot.player_number == player_number)
                else {
                    return Ok(Err(SlotReservationRejection::SlotNotFound));
                };
                match slot.state {
                    PlayerSlotState::Open => {
                        slot.state = PlayerSlotState::Reserved;
                        slot.seat_id = Some(seat_id.clone());
                        slot.lease_expires_unix_ms = Some(now_unix_ms.saturating_add(lease_duration_ms));
                    }
                    PlayerSlotState::Reserved if slot.seat_id.as_deref() == Some(seat_id.as_str()) => {
                        slot.lease_expires_unix_ms = Some(now_unix_ms.saturating_add(lease_duration_ms));
                    }
                    PlayerSlotState::Occupied | PlayerSlotState::Disconnected
                        if slot.seat_id.as_deref() == Some(seat_id.as_str()) => {}
                    _ => return Ok(Err(SlotReservationRejection::Unavailable)),
                }
                let updated_slots = match serde_json::to_string(&slots) {
                    Ok(value) => value,
                    Err(_) => return Ok(Err(SlotReservationRejection::InvalidStoredSlots)),
                };
                transaction.execute(
                    "UPDATE sessions
                     SET player_slots_json = ?2,
                         grant_expires_unix_ms = max(grant_expires_unix_ms, ?3),
                         updated_unix_ms = ?4
                     WHERE id = ?1",
                    params![session_id, updated_slots, lease_expires, now],
                )?;
                transaction.execute(
                    "INSERT INTO session_events (session_id, state, occurred_unix_ms, detail)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        session_id,
                        state_name,
                        now,
                        format!("player {player_number} reserved by {seat_id} until {lease_expires}")
                    ],
                )?;
                let stored = transaction.query_row(
                    &session_select_sql("WHERE id = ?1"),
                    [&session_id],
                    StoredSession::from_row,
                )?;
                transaction.commit()?;
                Ok(Ok(stored))
            })
            .await
            .map_err(StoreError::database)?
            .map_err(StoreError::from_slot_reservation_rejection)?;
        stored.into_session()
    }

    pub async fn release_player_slot(
        &self,
        session_id: String,
        player_number: u32,
        seat_id: String,
        now_unix_ms: u64,
    ) -> Result<Session, StoreError> {
        let now = to_sql_integer(now_unix_ms, "slot release timestamp")?;
        let stored = self
            .connection
            .call(
                move |connection| -> tokio_rusqlite::rusqlite::Result<
                    Result<StoredSession, SlotReservationRejection>,
                > {
                    let transaction = connection.transaction()?;
                    let current = transaction
                        .query_row(
                            "SELECT state, player_slots_json FROM sessions WHERE id = ?1",
                            [&session_id],
                            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                        )
                        .optional()?;
                    let Some((state_name, player_slots_json)) = current else {
                        return Ok(Err(SlotReservationRejection::SessionNotFound));
                    };
                    let state = match parse_session_state(&state_name) {
                        Ok(value) => value,
                        Err(_) => return Ok(Err(SlotReservationRejection::InvalidStoredState)),
                    };
                    if state.is_terminal() {
                        return Ok(Err(SlotReservationRejection::Unavailable));
                    }

                    let mut slots: Vec<PlayerSlot> = match serde_json::from_str(&player_slots_json)
                    {
                        Ok(value) => value,
                        Err(_) => return Ok(Err(SlotReservationRejection::InvalidStoredSlots)),
                    };
                    expire_slot_leases(&mut slots, now_unix_ms);

                    let Some(slot) = slots
                        .iter_mut()
                        .find(|slot| slot.player_number == player_number)
                    else {
                        return Ok(Err(SlotReservationRejection::SlotNotFound));
                    };

                    if !matches!(slot.state, PlayerSlotState::Open) {
                        if slot.seat_id.as_deref() != Some(seat_id.as_str()) {
                            return Ok(Err(SlotReservationRejection::Unavailable));
                        }
                        slot.state = PlayerSlotState::Open;
                        slot.seat_id = None;
                        slot.lease_expires_unix_ms = None;
                    }

                    let updated_slots = match serde_json::to_string(&slots) {
                        Ok(value) => value,
                        Err(_) => return Ok(Err(SlotReservationRejection::InvalidStoredSlots)),
                    };
                    transaction.execute(
                        "UPDATE sessions
                     SET player_slots_json = ?2, updated_unix_ms = ?3
                     WHERE id = ?1",
                        params![session_id, updated_slots, now],
                    )?;
                    transaction.execute(
                        "INSERT INTO session_events (session_id, state, occurred_unix_ms, detail)
                     VALUES (?1, ?2, ?3, ?4)",
                        params![
                            session_id,
                            state_name,
                            now,
                            format!("player {player_number} released by {seat_id}")
                        ],
                    )?;
                    let stored = transaction.query_row(
                        &session_select_sql("WHERE id = ?1"),
                        [&session_id],
                        StoredSession::from_row,
                    )?;
                    transaction.commit()?;
                    Ok(Ok(stored))
                },
            )
            .await
            .map_err(StoreError::database)?
            .map_err(StoreError::from_slot_reservation_rejection)?;
        stored.into_session()
    }
}

fn session_select_sql(suffix: &str) -> String {
    format!(
        "SELECT id, game_id, seat_id, destination_address, runtime_host_id,
                runtime_host_address, runtime_profile_json, state, grant_token,
                grant_expires_unix_ms, media_udp_port, input_udp_port,
                player_slots_json, created_unix_ms, updated_unix_ms, failure_reason
         FROM sessions {suffix}"
    )
}

fn initial_player_slots(
    max_players: u32,
    seat_id: &str,
    lease_expires_unix_ms: u64,
) -> Vec<PlayerSlot> {
    let slot_count = max_players.max(1);
    (1..=slot_count)
        .map(|player_number| {
            if player_number == 1 {
                PlayerSlot {
                    player_number,
                    state: PlayerSlotState::Occupied,
                    seat_id: Some(seat_id.to_owned()),
                    lease_expires_unix_ms: Some(lease_expires_unix_ms),
                }
            } else {
                PlayerSlot {
                    player_number,
                    state: PlayerSlotState::Open,
                    seat_id: None,
                    lease_expires_unix_ms: None,
                }
            }
        })
        .collect()
}

fn expire_slot_leases(slots: &mut [PlayerSlot], now_unix_ms: u64) {
    for slot in slots {
        if matches!(
            slot.state,
            PlayerSlotState::Reserved | PlayerSlotState::Disconnected
        ) && slot
            .lease_expires_unix_ms
            .is_some_and(|expires| expires <= now_unix_ms)
        {
            slot.state = PlayerSlotState::Open;
            slot.seat_id = None;
            slot.lease_expires_unix_ms = None;
        }
    }
}

fn mark_expired_offline(
    connection: &tokio_rusqlite::rusqlite::Connection,
    cutoff: i64,
    now: i64,
) -> tokio_rusqlite::rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO session_events (session_id, state, occurred_unix_ms, detail)
         SELECT s.id, 'runtime_lost', ?2, 'runtime host heartbeat expired'
         FROM sessions s
         JOIN runtime_hosts h ON h.id = s.runtime_host_id
         WHERE h.status = 'online' AND h.last_seen_unix_ms < ?1
           AND s.state NOT IN
               ('stopped', 'allocation_failed', 'launch_failed', 'runtime_lost', 'terminated')",
        params![cutoff, now],
    )?;
    connection.execute(
        "UPDATE sessions
         SET state = 'runtime_lost', updated_unix_ms = ?2,
             failure_reason = 'runtime host heartbeat expired'
         WHERE runtime_host_id IN (
             SELECT id FROM runtime_hosts
             WHERE status = 'online' AND last_seen_unix_ms < ?1
         ) AND state NOT IN
             ('stopped', 'allocation_failed', 'launch_failed', 'runtime_lost', 'terminated')",
        params![cutoff, now],
    )?;
    connection.execute(
        "UPDATE runtime_hosts SET status = 'offline'
         WHERE status = 'online' AND last_seen_unix_ms < ?1",
        [cutoff],
    )?;
    Ok(())
}

struct StoredHost {
    id: String,
    display_name: String,
    agent_version: String,
    capabilities_json: String,
    status: String,
    last_seen_unix_ms: i64,
    heartbeat_sequence: i64,
    active_session_count: i64,
}

impl StoredHost {
    fn from_row(row: &tokio_rusqlite::rusqlite::Row<'_>) -> tokio_rusqlite::rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            display_name: row.get(1)?,
            agent_version: row.get(2)?,
            capabilities_json: row.get(3)?,
            status: row.get(4)?,
            last_seen_unix_ms: row.get(5)?,
            heartbeat_sequence: row.get(6)?,
            active_session_count: row.get(7)?,
        })
    }

    fn into_runtime_host(self) -> Result<RuntimeHost, StoreError> {
        let status = match self.status.as_str() {
            "online" => RuntimeHostStatus::Online,
            "offline" => RuntimeHostStatus::Offline,
            _ => return Err(StoreError::data("runtime host has an unknown status")),
        };
        let capabilities = serde_json::from_str::<RuntimeHostCapabilities>(&self.capabilities_json)
            .map_err(StoreError::serialization)?;
        Ok(RuntimeHost {
            id: self.id,
            display_name: self.display_name,
            agent_version: self.agent_version,
            capabilities,
            status,
            last_seen_unix_ms: from_sql_integer(self.last_seen_unix_ms, "last_seen_unix_ms")?,
            heartbeat_sequence: from_sql_integer(self.heartbeat_sequence, "heartbeat_sequence")?,
            active_session_count: u32::try_from(self.active_session_count).map_err(|_| {
                StoreError::data("active_session_count is outside the supported range")
            })?,
        })
    }
}

struct StoredSession {
    id: String,
    game_id: String,
    seat_id: String,
    destination_address: String,
    runtime_host_id: String,
    runtime_host_address: String,
    runtime_profile_json: String,
    state: String,
    grant_token: String,
    grant_expires_unix_ms: i64,
    media_udp_port: i64,
    input_udp_port: i64,
    player_slots_json: String,
    created_unix_ms: i64,
    updated_unix_ms: i64,
    failure_reason: Option<String>,
}

impl StoredSession {
    fn from_row(row: &tokio_rusqlite::rusqlite::Row<'_>) -> tokio_rusqlite::rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            game_id: row.get(1)?,
            seat_id: row.get(2)?,
            destination_address: row.get(3)?,
            runtime_host_id: row.get(4)?,
            runtime_host_address: row.get(5)?,
            runtime_profile_json: row.get(6)?,
            state: row.get(7)?,
            grant_token: row.get(8)?,
            grant_expires_unix_ms: row.get(9)?,
            media_udp_port: row.get(10)?,
            input_udp_port: row.get(11)?,
            player_slots_json: row.get(12)?,
            created_unix_ms: row.get(13)?,
            updated_unix_ms: row.get(14)?,
            failure_reason: row.get(15)?,
        })
    }

    fn into_session(self) -> Result<Session, StoreError> {
        let runtime_profile =
            serde_json::from_str::<GameRuntimeProfile>(&self.runtime_profile_json)
                .map_err(StoreError::serialization)?;
        let player_slots = serde_json::from_str::<Vec<PlayerSlot>>(&self.player_slots_json)
            .map_err(StoreError::serialization)?;
        Ok(Session {
            id: self.id,
            game_id: self.game_id,
            seat_id: self.seat_id,
            destination_address: self.destination_address,
            runtime_host_id: self.runtime_host_id.clone(),
            runtime_profile,
            state: parse_session_state(&self.state)?,
            connection_grant: ConnectionGrant {
                token: self.grant_token,
                expires_unix_ms: from_sql_integer(
                    self.grant_expires_unix_ms,
                    "grant_expires_unix_ms",
                )?,
                runtime_host_id: self.runtime_host_id,
                runtime_host_address: self.runtime_host_address,
                media_udp_port: u16::try_from(self.media_udp_port).map_err(|_| {
                    StoreError::data("media UDP port is outside the supported range")
                })?,
                input_udp_port: u16::try_from(self.input_udp_port).map_err(|_| {
                    StoreError::data("input UDP port is outside the supported range")
                })?,
            },
            player_slots,
            created_unix_ms: from_sql_integer(self.created_unix_ms, "created_unix_ms")?,
            updated_unix_ms: from_sql_integer(self.updated_unix_ms, "updated_unix_ms")?,
            failure_reason: self.failure_reason,
        })
    }
}

fn parse_session_state(state: &str) -> Result<SessionState, StoreError> {
    match state {
        "requested" => Ok(SessionState::Requested),
        "allocating" => Ok(SessionState::Allocating),
        "starting" => Ok(SessionState::Starting),
        "ready" => Ok(SessionState::Ready),
        "active" => Ok(SessionState::Active),
        "stopping" => Ok(SessionState::Stopping),
        "stopped" => Ok(SessionState::Stopped),
        "allocation_failed" => Ok(SessionState::AllocationFailed),
        "launch_failed" => Ok(SessionState::LaunchFailed),
        "runtime_lost" => Ok(SessionState::RuntimeLost),
        "unhealthy" => Ok(SessionState::Unhealthy),
        "terminated" => Ok(SessionState::Terminated),
        _ => Err(StoreError::data("session has an unknown state")),
    }
}

fn session_state_name(state: SessionState) -> &'static str {
    match state {
        SessionState::Requested => "requested",
        SessionState::Allocating => "allocating",
        SessionState::Starting => "starting",
        SessionState::Ready => "ready",
        SessionState::Active => "active",
        SessionState::Stopping => "stopping",
        SessionState::Stopped => "stopped",
        SessionState::AllocationFailed => "allocation_failed",
        SessionState::LaunchFailed => "launch_failed",
        SessionState::RuntimeLost => "runtime_lost",
        SessionState::Unhealthy => "unhealthy",
        SessionState::Terminated => "terminated",
    }
}

fn valid_runtime_transition(current: SessionState, next: SessionState) -> bool {
    matches!(
        (current, next),
        (SessionState::Allocating, SessionState::Starting)
            | (SessionState::Allocating, SessionState::Stopping)
            | (SessionState::Allocating, SessionState::AllocationFailed)
            | (SessionState::Starting, SessionState::Ready)
            | (SessionState::Starting, SessionState::Active)
            | (SessionState::Starting, SessionState::Stopping)
            | (SessionState::Starting, SessionState::LaunchFailed)
            | (SessionState::Starting, SessionState::RuntimeLost)
            | (SessionState::Ready, SessionState::Active)
            | (SessionState::Ready, SessionState::Stopping)
            | (SessionState::Ready, SessionState::RuntimeLost)
            | (SessionState::Active, SessionState::Stopping)
            | (SessionState::Active, SessionState::RuntimeLost)
            | (SessionState::Active, SessionState::Unhealthy)
            | (SessionState::Unhealthy, SessionState::Active)
            | (SessionState::Unhealthy, SessionState::Stopping)
            | (SessionState::Unhealthy, SessionState::RuntimeLost)
            | (SessionState::Unhealthy, SessionState::Terminated)
            | (SessionState::Stopping, SessionState::Stopped)
            | (SessionState::Stopping, SessionState::RuntimeLost)
            | (SessionState::Stopping, SessionState::Terminated)
    )
}

fn is_failure_state(state: SessionState) -> bool {
    matches!(
        state,
        SessionState::AllocationFailed
            | SessionState::LaunchFailed
            | SessionState::RuntimeLost
            | SessionState::Unhealthy
            | SessionState::Terminated
    )
}

fn to_sql_integer(value: u64, field: &str) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::data(format!("{field} is too large for SQLite")))
}

fn from_sql_integer(value: i64, field: &str) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::data(format!("{field} must not be negative")))
}

enum HeartbeatRejection {
    NotFound,
    Stale,
    SequenceConflict,
}

enum AllocationRejection {
    GameNotFound,
    GameUnavailable,
    SeatBusy,
    PortsExhausted,
    InvalidHostCapabilities,
    InvalidRuntimeProfile,
    MissingDataPlaneAddress,
}

enum StateUpdateRejection {
    SessionNotFound,
    WrongRuntimeHost,
    InvalidStoredState,
    InvalidTransition,
    MissingFailureReason,
    ConflictingRetry,
}

enum SlotReservationRejection {
    SessionNotFound,
    SlotNotFound,
    SeatBusy,
    Unavailable,
    InvalidStoredState,
    InvalidStoredSlots,
}

#[derive(Debug)]
pub enum StoreError {
    NotFound,
    GameNotFound,
    GameUnavailable,
    SessionNotFound,
    SeatBusy,
    PortsExhausted,
    PlayerSlotNotFound,
    PlayerSlotUnavailable,
    MissingDataPlaneAddress,
    WrongRuntimeHost,
    InvalidSessionTransition,
    MissingFailureReason,
    ConflictingStateRetry,
    StaleHeartbeat,
    HeartbeatSequenceConflict,
    Database(String),
    Serialization(String),
    InvalidData(String),
}

impl StoreError {
    fn database(error: impl fmt::Display) -> Self {
        Self::Database(error.to_string())
    }

    fn serialization(error: impl fmt::Display) -> Self {
        Self::Serialization(error.to_string())
    }

    fn data(message: impl Into<String>) -> Self {
        Self::InvalidData(message.into())
    }

    fn from_heartbeat_rejection(rejection: HeartbeatRejection) -> Self {
        match rejection {
            HeartbeatRejection::NotFound => Self::NotFound,
            HeartbeatRejection::Stale => Self::StaleHeartbeat,
            HeartbeatRejection::SequenceConflict => Self::HeartbeatSequenceConflict,
        }
    }

    fn from_allocation_rejection(rejection: AllocationRejection) -> Self {
        match rejection {
            AllocationRejection::GameNotFound => Self::GameNotFound,
            AllocationRejection::GameUnavailable => Self::GameUnavailable,
            AllocationRejection::SeatBusy => Self::SeatBusy,
            AllocationRejection::PortsExhausted => Self::PortsExhausted,
            AllocationRejection::InvalidHostCapabilities => {
                Self::data("runtime host capabilities could not be decoded")
            }
            AllocationRejection::InvalidRuntimeProfile => {
                Self::data("runtime profile could not be decoded")
            }
            AllocationRejection::MissingDataPlaneAddress => Self::MissingDataPlaneAddress,
        }
    }

    fn from_state_update_rejection(rejection: StateUpdateRejection) -> Self {
        match rejection {
            StateUpdateRejection::SessionNotFound => Self::SessionNotFound,
            StateUpdateRejection::WrongRuntimeHost => Self::WrongRuntimeHost,
            StateUpdateRejection::InvalidStoredState => Self::data("session has an unknown state"),
            StateUpdateRejection::InvalidTransition => Self::InvalidSessionTransition,
            StateUpdateRejection::MissingFailureReason => Self::MissingFailureReason,
            StateUpdateRejection::ConflictingRetry => Self::ConflictingStateRetry,
        }
    }

    fn from_slot_reservation_rejection(rejection: SlotReservationRejection) -> Self {
        match rejection {
            SlotReservationRejection::SessionNotFound => Self::SessionNotFound,
            SlotReservationRejection::SlotNotFound => Self::PlayerSlotNotFound,
            SlotReservationRejection::SeatBusy => Self::SeatBusy,
            SlotReservationRejection::Unavailable => Self::PlayerSlotUnavailable,
            SlotReservationRejection::InvalidStoredState => {
                Self::data("session has an unknown state")
            }
            SlotReservationRejection::InvalidStoredSlots => {
                Self::data("session player slots could not be decoded")
            }
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(formatter, "runtime host was not found"),
            Self::GameNotFound => write!(formatter, "catalog game was not found"),
            Self::GameUnavailable => write!(formatter, "catalog game has no online runtime host"),
            Self::SessionNotFound => write!(formatter, "session was not found"),
            Self::SeatBusy => write!(formatter, "seat already has a nonterminal session"),
            Self::PortsExhausted => {
                write!(formatter, "runtime host has no available session ports")
            }
            Self::PlayerSlotNotFound => write!(formatter, "player slot was not found"),
            Self::PlayerSlotUnavailable => write!(formatter, "player slot is not currently open"),
            Self::MissingDataPlaneAddress => {
                write!(formatter, "runtime host has no data-plane address")
            }
            Self::WrongRuntimeHost => write!(formatter, "session belongs to another runtime host"),
            Self::InvalidSessionTransition => {
                write!(formatter, "session state transition is invalid")
            }
            Self::MissingFailureReason => write!(formatter, "failure state requires a reason"),
            Self::ConflictingStateRetry => {
                write!(formatter, "state retry conflicts with stored details")
            }
            Self::StaleHeartbeat => write!(formatter, "heartbeat sequence is stale"),
            Self::HeartbeatSequenceConflict => write!(formatter, "heartbeat sequence conflicts"),
            Self::Database(message) => write!(formatter, "database error: {message}"),
            Self::Serialization(message) => write!(formatter, "serialization error: {message}"),
            Self::InvalidData(message) => write!(formatter, "invalid stored data: {message}"),
        }
    }
}

impl Error for StoreError {}
