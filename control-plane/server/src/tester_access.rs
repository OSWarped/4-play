//! Private WAN invitation/admission foundation. No public routes are enabled here.
//!
//! Codes and admission credentials are high-entropy bearer secrets, returned only
//! when issued and stored as domain-separated hashes. Authorization always checks
//! the current invitation row, so revocation also invalidates issued admissions.
use std::{fmt, fs::OpenOptions, path::Path};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio_rusqlite::{
    Connection, params,
    rusqlite::{OptionalExtension, TransactionBehavior},
};
use uuid::Uuid;

const HOUR: u64 = 3_600_000;
const MAX_INVITE_LIFETIME: u64 = 7 * 24 * HOUR;
const ADMISSION_LIFETIME: u64 = 8 * HOUR;
const MAX_SEATS_PER_INVITE: u32 = 8;
const MAX_ACTIVE_SEATS: i64 = 8;
const MAX_INVITATIONS: i64 = 1000;

const SCHEMA: &str = "
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS tester_invitations (
    id TEXT PRIMARY KEY NOT NULL,
    code_hash TEXT NOT NULL UNIQUE,
    created_ms INTEGER NOT NULL,
    expires_ms INTEGER NOT NULL,
    max_seats INTEGER NOT NULL CHECK(max_seats BETWEEN 1 AND 8),
    used_seats INTEGER NOT NULL DEFAULT 0,
    revoked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS tester_admissions (
    credential_hash TEXT PRIMARY KEY NOT NULL,
    invitation_id TEXT NOT NULL REFERENCES tester_invitations(id),
    seat_id TEXT NOT NULL UNIQUE,
    expires_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS tester_admissions_invitation ON tester_admissions(invitation_id);
";

#[derive(Debug, PartialEq, Eq)]
pub enum AccessError {
    Denied,
    Capacity,
    InvalidSettings,
    Storage,
}
impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Denied => "Tester credential is invalid, expired, revoked, or exhausted",
            Self::Capacity => "Tester capacity reached",
            Self::InvalidSettings => "Invalid tester access settings",
            Self::Storage => "Private tester access storage unavailable",
        })
    }
}
impl std::error::Error for AccessError {}

// Deliberately no Debug/Serialize: these values must not enter status logs/API lists.
pub struct IssuedInvitation {
    pub id: Uuid,
    pub code: String,
}
pub struct IssuedAdmission {
    pub seat_id: Uuid,
    pub credential: String,
    pub expires_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct InvitationStatus {
    pub id: String,
    pub expires_ms: u64,
    pub max_seats: u32,
    pub used_seats: u32,
    pub revoked: bool,
}

#[derive(Clone)]
pub struct TesterAccess {
    connection: Connection,
}

fn secret() -> String {
    // Two independent v4 UUIDs provide 244 random bits without new dependencies.
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}
fn hash(domain: &[u8], value: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(value.as_bytes());
    format!("{:x}", digest.finalize())
}
fn valid_credential(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}
pub(crate) fn admission_fingerprint(credential: &str) -> String {
    hash(b"4play-tester-admission-v1\0", credential)
}
fn timestamp(value: u64) -> Result<i64, AccessError> {
    i64::try_from(value).map_err(|_| AccessError::InvalidSettings)
}

impl TesterAccess {
    /// Parent must already exist. On Unix, refuse symlinks, non-files, group/world
    /// access, and a group/world-writable parent; create new files with mode 0600.
    pub async fn open(path: &Path) -> Result<Self, AccessError> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent_meta = std::fs::metadata(parent).map_err(|_| AccessError::Storage)?;
        if !parent_meta.is_dir() {
            return Err(AccessError::Storage);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if parent_meta.permissions().mode() & 0o022 != 0 {
                return Err(AccessError::Storage);
            }
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(AccessError::Storage),
        }
        let metadata = std::fs::symlink_metadata(path).map_err(|_| AccessError::Storage)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(AccessError::Storage);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(AccessError::Storage);
            }
        }
        let connection = Connection::open(path)
            .await
            .map_err(|_| AccessError::Storage)?;
        connection
            .call::<_, _, tokio_rusqlite::rusqlite::Error>(|db| {
                db.busy_timeout(std::time::Duration::from_secs(5))?;
                db.execute_batch(SCHEMA)?;
                Ok(())
            })
            .await
            .map_err(|_| AccessError::Storage)?;
        Ok(Self { connection })
    }

    /// Limits are lifetime browser enrollments per invitation, not concurrent
    /// sessions. Clearing cookies does not recover spent seats; issue a new invite.
    pub async fn issue(
        &self,
        now: u64,
        expires: u64,
        max_seats: u32,
    ) -> Result<IssuedInvitation, AccessError> {
        if expires <= now
            || expires - now > MAX_INVITE_LIFETIME
            || !(1..=MAX_SEATS_PER_INVITE).contains(&max_seats)
        {
            return Err(AccessError::InvalidSettings);
        }
        let created = timestamp(now)?;
        let expiry = timestamp(expires)?;
        let id = Uuid::new_v4();
        let code = secret();
        let code_hash = hash(b"4play-tester-invite-v1\0", &code);
        let inserted = self.connection.call::<_, _, tokio_rusqlite::rusqlite::Error>(move |db| {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let count: i64 = tx.query_row("SELECT count(*) FROM tester_invitations", [], |r| r.get(0))?;
            if count >= MAX_INVITATIONS { return Ok(false); }
            tx.execute("INSERT INTO tester_invitations (id,code_hash,created_ms,expires_ms,max_seats) VALUES (?1,?2,?3,?4,?5)",
                       params![id.to_string(), code_hash, created, expiry, max_seats])?;
            tx.commit()?;
            Ok(true)
        }).await.map_err(|_| AccessError::Storage)?;
        if !inserted {
            return Err(AccessError::Capacity);
        }
        Ok(IssuedInvitation { id, code })
    }

    pub async fn redeem(&self, code: &str, now: u64) -> Result<IssuedAdmission, AccessError> {
        if !valid_credential(code) {
            return Err(AccessError::Denied);
        }
        let now = timestamp(now)?;
        let code_hash = hash(b"4play-tester-invite-v1\0", code);
        let credential = secret();
        let credential_hash = hash(b"4play-tester-admission-v1\0", &credential);
        let seat = Uuid::new_v4();
        let result = self.connection.call::<_, _, tokio_rusqlite::rusqlite::Error>(move |db| {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let invite = tx.query_row("SELECT id,expires_ms,max_seats,used_seats FROM tester_invitations WHERE code_hash=?1 AND revoked=0 AND expires_ms>?2 AND created_ms<=?2",
                params![code_hash, now], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, u32>(2)?, r.get::<_, u32>(3)?))).optional()?;
            let Some((id, expires, max_seats, used)) = invite else { return Ok(Err(AccessError::Denied)); };
            if used >= max_seats { return Ok(Err(AccessError::Denied)); }
            let active: i64 = tx.query_row("SELECT count(*) FROM tester_admissions a JOIN tester_invitations i ON i.id=a.invitation_id WHERE a.expires_ms>?1 AND i.expires_ms>?1 AND i.revoked=0", [now], |r| r.get(0))?;
            if active >= MAX_ACTIVE_SEATS { return Ok(Err(AccessError::Capacity)); }
            let expiry = expires.min(now.saturating_add(ADMISSION_LIFETIME as i64));
            tx.execute("UPDATE tester_invitations SET used_seats=used_seats+1 WHERE id=?1", [&id])?;
            tx.execute("INSERT INTO tester_admissions (credential_hash,invitation_id,seat_id,expires_ms) VALUES (?1,?2,?3,?4)", params![credential_hash, id, seat.to_string(), expiry])?;
            tx.commit()?;
            Ok(Ok(expiry as u64))
        }).await.map_err(|_| AccessError::Storage)??;
        Ok(IssuedAdmission {
            seat_id: seat,
            credential,
            expires_ms: result,
        })
    }

    pub async fn authenticate(&self, credential: &str, now: u64) -> Result<Uuid, AccessError> {
        if !valid_credential(credential) {
            return Err(AccessError::Denied);
        }
        let now = timestamp(now)?;
        let credential_hash = hash(b"4play-tester-admission-v1\0", credential);
        let seat = self.connection.call::<_, _, tokio_rusqlite::rusqlite::Error>(move |db| {
            db.query_row("SELECT a.seat_id FROM tester_admissions a JOIN tester_invitations i ON i.id=a.invitation_id WHERE a.credential_hash=?1 AND a.expires_ms>?2 AND i.expires_ms>?2 AND i.created_ms<=?2 AND i.revoked=0",
                params![credential_hash, now], |r| r.get::<_, String>(0)).optional()
        }).await.map_err(|_| AccessError::Storage)?.ok_or(AccessError::Denied)?;
        Uuid::parse_str(&seat).map_err(|_| AccessError::Storage)
    }

    pub async fn revoke(&self, id: Uuid) -> Result<bool, AccessError> {
        self.connection
            .call::<_, _, tokio_rusqlite::rusqlite::Error>(move |db| {
                Ok(db.execute(
                    "UPDATE tester_invitations SET revoked=1 WHERE id=?1 AND revoked=0",
                    [id.to_string()],
                )? == 1)
            })
            .await
            .map_err(|_| AccessError::Storage)
    }

    /// Safe operator status: no codes, hashes, admission credentials, or seat IDs.
    pub async fn list(&self) -> Result<Vec<InvitationStatus>, AccessError> {
        self.connection.call::<_, _, tokio_rusqlite::rusqlite::Error>(|db| {
            db.prepare("SELECT id,expires_ms,max_seats,used_seats,revoked FROM tester_invitations ORDER BY created_ms,id")?
                .query_map([], |r| Ok(InvitationStatus { id: r.get(0)?, expires_ms: r.get::<_, i64>(1)? as u64, max_seats: r.get(2)?, used_seats: r.get(3)?, revoked: r.get(4)? }))?
                .collect::<Result<Vec<_>, _>>()
        }).await.map_err(|_| AccessError::Storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn fixture() -> (tempfile::TempDir, TesterAccess) {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // tempfile follows this host's umask (0002 on the reference host).
            // Set up the same explicitly private directory as the operator CLI.
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let access = TesterAccess::open(&dir.path().join("test.sqlite3"))
            .await
            .unwrap();
        (dir, access)
    }

    #[tokio::test]
    async fn issue_redeem_and_persist_without_storing_secrets() {
        let (dir, access) = fixture().await;
        let invite = access.issue(1000, 1000 + HOUR, 1).await.unwrap();
        let admission = access.redeem(&invite.code, 2000).await.unwrap();
        assert_eq!(
            access
                .authenticate(&admission.credential, 3000)
                .await
                .unwrap(),
            admission.seat_id
        );
        assert_eq!(admission.expires_ms, 1000 + HOUR);
        let reopened = TesterAccess::open(&dir.path().join("test.sqlite3"))
            .await
            .unwrap();
        assert_eq!(
            reopened
                .authenticate(&admission.credential, 4000)
                .await
                .unwrap(),
            admission.seat_id
        );
        let bytes = std::fs::read(dir.path().join("test.sqlite3")).unwrap();
        for secret in [&invite.code, &admission.credential] {
            assert!(!String::from_utf8_lossy(&bytes).contains(secret));
        }
        let status = serde_json::to_string(&access.list().await.unwrap()).unwrap();
        assert!(!status.contains("hash"));
        assert!(!status.contains(&invite.code));
        assert!(!status.contains(&admission.credential));
    }

    #[tokio::test]
    async fn revoke_invalidates_admissions_and_redeem_after_reopen() {
        let (dir, access) = fixture().await;
        let invite = access.issue(1000, 1000 + HOUR, 2).await.unwrap();
        let admission = access.redeem(&invite.code, 2000).await.unwrap();
        assert!(access.revoke(invite.id).await.unwrap());
        assert!(!access.revoke(invite.id).await.unwrap());
        let access = TesterAccess::open(&dir.path().join("test.sqlite3"))
            .await
            .unwrap();
        assert_eq!(
            access.authenticate(&admission.credential, 3000).await,
            Err(AccessError::Denied)
        );
        assert!(matches!(
            access.redeem(&invite.code, 3000).await,
            Err(AccessError::Denied)
        ));
    }

    #[tokio::test]
    async fn expiry_and_credential_domains_fail_closed() {
        let (_dir, access) = fixture().await;
        let invite = access.issue(1000, 1000 + HOUR, 2).await.unwrap();
        assert!(matches!(
            access.redeem(&invite.code, 999).await,
            Err(AccessError::Denied)
        ));
        let admission = access.redeem(&invite.code, 2000).await.unwrap();
        assert_eq!(
            access.authenticate(&invite.code, 2000).await,
            Err(AccessError::Denied)
        );
        assert!(matches!(
            access.redeem(&admission.credential, 2000).await,
            Err(AccessError::Denied)
        ));
        assert_eq!(
            access
                .authenticate(&admission.credential, 1000 + HOUR)
                .await,
            Err(AccessError::Denied)
        );
        assert!(matches!(
            access.redeem(&invite.code, 1000 + HOUR).await,
            Err(AccessError::Denied)
        ));
        for bad in ["", "not-a-code", &"g".repeat(64), &"a".repeat(65)] {
            assert!(matches!(
                access.redeem(bad, 2000).await,
                Err(AccessError::Denied)
            ));
            assert_eq!(
                access.authenticate(bad, 2000).await,
                Err(AccessError::Denied)
            );
        }
    }

    #[tokio::test]
    async fn limits_are_atomic_across_independent_connections() {
        let (dir, first) = fixture().await;
        let second = TesterAccess::open(&dir.path().join("test.sqlite3"))
            .await
            .unwrap();
        let invite = first.issue(1000, 1000 + HOUR, 1).await.unwrap();
        let (a, b) = tokio::join!(
            first.redeem(&invite.code, 2000),
            second.redeem(&invite.code, 2000)
        );
        assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
        assert_eq!(first.list().await.unwrap()[0].used_seats, 1);
    }

    #[tokio::test]
    async fn global_capacity_and_admission_lifetime_are_bounded() {
        let (_dir, access) = fixture().await;
        let invite = access
            .issue(1000, 1000 + MAX_INVITE_LIFETIME, 8)
            .await
            .unwrap();
        for _ in 0..8 {
            let admission = access.redeem(&invite.code, 2000).await.unwrap();
            assert_eq!(admission.expires_ms, 2000 + ADMISSION_LIFETIME);
            assert_eq!(
                access
                    .authenticate(&admission.credential, 2000 + ADMISSION_LIFETIME)
                    .await,
                Err(AccessError::Denied)
            );
        }
        let another = access
            .issue(1000, 1000 + MAX_INVITE_LIFETIME, 1)
            .await
            .unwrap();
        assert!(matches!(
            access.redeem(&another.code, 2000).await,
            Err(AccessError::Capacity)
        ));
        access.revoke(invite.id).await.unwrap();
        assert!(access.redeem(&another.code, 2001).await.is_ok());
        assert!(matches!(
            access.redeem(&invite.code, 2001).await,
            Err(AccessError::Denied)
        ));
    }

    #[tokio::test]
    async fn invalid_operator_settings_are_rejected() {
        let (_dir, access) = fixture().await;
        for (expires, seats) in [
            (1000, 1),
            (999, 1),
            (1001, 0),
            (1001, 9),
            (1001 + MAX_INVITE_LIFETIME, 1),
        ] {
            assert!(matches!(
                access.issue(1000, expires, seats).await,
                Err(AccessError::InvalidSettings)
            ));
        }
        assert!(matches!(
            access.issue(u64::MAX - 1, u64::MAX, 1).await,
            Err(AccessError::InvalidSettings)
        ));
        assert!(access.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn renewal_reads_do_not_spend_or_extend_admission() {
        let (_dir, access) = fixture().await;
        let invite = access
            .issue(1000, 1000 + MAX_INVITE_LIFETIME, 1)
            .await
            .unwrap();
        let admission = access.redeem(&invite.code, 2000).await.unwrap();
        for now in [2001, 3000, admission.expires_ms - 1] {
            assert_eq!(
                access
                    .authenticate(&admission.credential, now)
                    .await
                    .unwrap(),
                admission.seat_id
            );
        }
        assert_eq!(access.list().await.unwrap()[0].used_seats, 1);
        assert_eq!(
            access
                .authenticate(&admission.credential, admission.expires_ms)
                .await,
            Err(AccessError::Denied)
        );
        assert!(matches!(
            access.redeem(&invite.code, admission.expires_ms).await,
            Err(AccessError::Denied)
        ));
    }

    #[tokio::test]
    async fn expired_admissions_free_global_capacity_but_not_spent_invites() {
        let (_dir, access) = fixture().await;
        let first = access
            .issue(1000, 1000 + MAX_INVITE_LIFETIME, 8)
            .await
            .unwrap();
        for _ in 0..8 {
            access.redeem(&first.code, 2000).await.unwrap();
        }
        let second = access
            .issue(1000, 1000 + MAX_INVITE_LIFETIME, 1)
            .await
            .unwrap();
        assert!(matches!(
            access.redeem(&second.code, 3000).await,
            Err(AccessError::Capacity)
        ));
        assert!(
            access
                .redeem(&second.code, 2000 + ADMISSION_LIFETIME)
                .await
                .is_ok()
        );
        assert!(matches!(
            access.redeem(&first.code, 2000 + ADMISSION_LIFETIME).await,
            Err(AccessError::Denied)
        ));
    }

    #[tokio::test]
    async fn global_capacity_is_atomic_across_connections() {
        let (dir, first) = fixture().await;
        let second = TesterAccess::open(&dir.path().join("test.sqlite3"))
            .await
            .unwrap();
        let invite = first.issue(1000, 1000 + HOUR, 7).await.unwrap();
        for _ in 0..7 {
            first.redeem(&invite.code, 2000).await.unwrap();
        }
        let a = first.issue(1000, 1000 + HOUR, 1).await.unwrap();
        let b = first.issue(1000, 1000 + HOUR, 1).await.unwrap();
        let (a, b) = tokio::join!(first.redeem(&a.code, 2000), second.redeem(&b.code, 2000));
        assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    }

    #[tokio::test]
    async fn invitation_storage_is_bounded() {
        let (_dir, access) = fixture().await;
        access.connection.call::<_, _, tokio_rusqlite::rusqlite::Error>(|db| {
            db.execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<1000) INSERT INTO tester_invitations (id,code_hash,created_ms,expires_ms,max_seats) SELECT 'fixture-'||x, 'hash-'||x, 0, 1, 1 FROM n;")?;
            Ok(())
        }).await.unwrap();
        assert!(matches!(
            access.issue(1000, 2000, 1).await,
            Err(AccessError::Capacity)
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn private_files_required_and_symlinks_refused() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (dir, _access) = fixture().await;
        let file = dir.path().join("test.sqlite3");
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let alias = dir.path().join("alias.sqlite3");
        symlink(&file, &alias).unwrap();
        assert!(matches!(
            TesterAccess::open(&alias).await,
            Err(AccessError::Storage)
        ));
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            TesterAccess::open(&file).await,
            Err(AccessError::Storage)
        ));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(matches!(
            TesterAccess::open(&dir.path().join("new.sqlite3")).await,
            Err(AccessError::Storage)
        ));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}
