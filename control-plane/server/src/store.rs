use std::{collections::BTreeMap, error::Error, fmt, path::Path};

use control_protocol::{
    CatalogGame, GameAvailability, GameRuntimeProfile, RegisterRuntimeHost, RuntimeHost,
    RuntimeHostCapabilities, RuntimeHostCatalog, RuntimeHostHeartbeat, RuntimeHostStatus,
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
";

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
        let stored = self
            .connection
            .call(move |connection| {
                mark_expired_offline(connection, cutoff)?;
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
        let stored = self
            .connection
            .call(move |connection| {
                mark_expired_offline(connection, cutoff)?;
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
        let rows = self
            .connection
            .call(move |connection| {
                mark_expired_offline(connection, cutoff)?;
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
}

fn mark_expired_offline(
    connection: &tokio_rusqlite::rusqlite::Connection,
    cutoff: i64,
) -> tokio_rusqlite::rusqlite::Result<()> {
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

#[derive(Debug)]
pub enum StoreError {
    NotFound,
    GameNotFound,
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
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(formatter, "runtime host was not found"),
            Self::GameNotFound => write!(formatter, "catalog game was not found"),
            Self::StaleHeartbeat => write!(formatter, "heartbeat sequence is stale"),
            Self::HeartbeatSequenceConflict => write!(formatter, "heartbeat sequence conflicts"),
            Self::Database(message) => write!(formatter, "database error: {message}"),
            Self::Serialization(message) => write!(formatter, "serialization error: {message}"),
            Self::InvalidData(message) => write!(formatter, "invalid stored data: {message}"),
        }
    }
}

impl Error for StoreError {}
