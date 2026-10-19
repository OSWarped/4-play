//! Coturn REST credentials: only short-lived derived credentials reach browsers.
use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha1::Sha1;
use uuid::Uuid;

fn credential_secret(name: &str, directory: Option<&str>) -> Result<String, String> {
    use std::io::Read;
    let error = || "TURN systemd credential is missing, unsafe or invalid".to_string();
    if name != "4play-turn.conf" {
        return Err(error());
    }
    let directory = std::path::Path::new(directory.ok_or_else(error)?);
    if !directory.is_absolute() {
        return Err(error());
    }
    let path = directory.join(name);
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| error())?;
    if !metadata.is_file() || metadata.len() > 65536 {
        return Err(error());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o037 != 0 || metadata.nlink() > 1 {
            return Err(error());
        }
    }
    let file = std::fs::File::open(path).map_err(|_| error())?;
    #[cfg(unix)]
    if !private_credential_permissions(&file).map_err(|_| error())? {
        return Err(error());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata().map_err(|_| error())?;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
            return Err(error());
        }
    }
    let mut text = String::new();
    file.take(65537)
        .read_to_string(&mut text)
        .map_err(|_| error())?;
    if text.len() > 65536 {
        return Err(error());
    }
    let values: Vec<_> = text
        .lines()
        .filter_map(|line| line.strip_prefix("static-auth-secret="))
        .collect();
    if values.len() != 1
        || values[0].len() != 64
        || !values[0].bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(error());
    }
    Ok(values[0].into())
}

#[cfg(unix)]
fn private_credential_permissions(file: &std::fs::File) -> std::io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    let mode = file.metadata()?.permissions().mode() & 0o777;
    if mode & 0o077 == 0 {
        return Ok(true);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::{fd::AsRawFd, unix::fs::MetadataExt};
        // systemd prefers a root-owned 0400 file with a named-user read ACL.
        // Its ACL mask appears as group-read in st_mode; do not accept ordinary 0440.
        let mut acl = [0u8; 64];
        // SAFETY: valid borrowed descriptor and writable bounded buffer; no ownership transfer.
        let size = unsafe {
            libc::fgetxattr(
                file.as_raw_fd(),
                c"system.posix_acl_access".as_ptr(),
                acl.as_mut_ptr().cast(),
                acl.len(),
            )
        };
        // SAFETY: geteuid has no pointer or lifetime requirements.
        let uid = unsafe { libc::geteuid() };
        let owner = file.metadata()?.uid();
        Ok(mode == 0o440
            && (owner == 0 || owner == uid)
            && size == 44
            && private_read_acl(&acl[..44], uid))
    }
    #[cfg(not(target_os = "linux"))]
    Ok(false)
}

#[cfg(target_os = "linux")]
fn private_read_acl(acl: &[u8], uid: u32) -> bool {
    if acl.len() != 44 || acl[..4] != 2u32.to_le_bytes() {
        return false;
    }
    let expected = [
        (1u16, 4u16, u32::MAX),
        (2, 4, uid),
        (4, 0, u32::MAX),
        (16, 4, u32::MAX),
        (32, 0, u32::MAX),
    ];
    acl[4..]
        .chunks_exact(8)
        .zip(expected)
        .all(|(entry, (tag, permission, id))| {
            entry[..2] == tag.to_le_bytes()
                && entry[2..4] == permission.to_le_bytes()
                && entry[4..] == id.to_le_bytes()
        })
}

fn secret_source(
    plain: Option<String>,
    credential: Option<String>,
    directory: Option<String>,
) -> Result<Option<String>, String> {
    match (plain, credential) {
        (Some(_), Some(_)) => Err("Configure only one TURN secret source".into()),
        (Some(secret), None) => Ok(Some(secret)),
        (None, Some(name)) => credential_secret(&name, directory.as_deref()).map(Some),
        (None, None) => Ok(None),
    }
}

#[derive(Clone)]
pub(crate) struct WanMedia {
    host: String,
    secret: String,
}
impl WanMedia {
    pub fn from_environment() -> Result<Option<Self>, String> {
        let secret = secret_source(
            std::env::var("FOURPLAY_WAN_TURN_SECRET").ok(),
            std::env::var("FOURPLAY_WAN_TURN_CREDENTIAL").ok(),
            std::env::var("CREDENTIALS_DIRECTORY").ok(),
        )?;
        match (std::env::var("FOURPLAY_WAN_TURN_HOST").ok(), secret) {
            (None, None) => Ok(None),
            (Some(host), Some(secret)) => Ok(Some(Self::new(host, secret)?)),
            _ => Err("Set TURN host and exactly one secret source, or neither".into()),
        }
    }
    fn new(host: String, secret: String) -> Result<Self, String> {
        if host.len() > 253
            || host.is_empty()
            || host.split('.').any(|s| {
                s.is_empty()
                    || s.len() > 63
                    || s.starts_with('-')
                    || s.ends_with('-')
                    || !s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
            || secret.len() != 64
            || !secret.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(
                "WAN TURN requires a DNS hostname and a random 64-hex shared secret".into(),
            );
        }
        Ok(Self { host, secret })
    }
    pub fn browser_config(&self, seat: Uuid, now_ms: u64) -> Value {
        let expiry = now_ms / 1000 + 3600;
        let username = format!("{expiry}:browser-{seat}");
        let mut mac =
            Hmac::<Sha1>::new_from_slice(self.secret.as_bytes()).expect("HMAC accepts key");
        mac.update(username.as_bytes());
        let credential = STANDARD.encode(mac.finalize().into_bytes());
        json!({"iceServers":[
            {"urls":format!("stun:{}:3478",self.host)},
            {"urls":[format!("turn:{}:3478?transport=udp",self.host),format!("turn:{}:3478?transport=tcp",self.host)],"username":username,"credential":credential}
        ],"iceTransportPolicy":"all","expires_unix_ms":expiry*1000,
          "credential_renew_after_ms":45*60*1000})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn turn_credentials_are_bounded_scoped_and_not_the_shared_secret() {
        let config = WanMedia::new("arcade.example.test".into(), "a".repeat(64)).unwrap();
        let seat = Uuid::nil();
        let json = config.browser_config(seat, 100_000);
        assert_eq!(json["expires_unix_ms"], 3_700_000);
        assert_eq!(json["credential_renew_after_ms"], 2_700_000);
        assert_eq!(
            json["iceServers"][1]["username"],
            format!("3700:browser-{seat}")
        );
        // Independently checked against Python's stdlib HMAC-SHA1 implementation.
        assert_eq!(
            json["iceServers"][1]["credential"],
            "LB/4ImZ0zn4FTMLgmN9TjUzkXVY="
        );
        assert!(!json.to_string().contains(&"a".repeat(64)));
        assert_ne!(json, config.browser_config(Uuid::new_v4(), 100_000));
    }
    #[test]
    fn unsafe_configuration_is_rejected() {
        for host in [
            "", "a/b", "a:3478", "a@b", "a..b", "-invalid", "invalid-", "a\nb",
        ] {
            assert!(WanMedia::new(host.into(), "a".repeat(64)).is_err());
        }
        assert!(WanMedia::new("turn.test".into(), "short".into()).is_err());
    }

    #[test]
    fn systemd_credential_is_bounded_and_exactly_one_secret_is_required() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("4play-turn.conf");
        std::fs::write(
            &path,
            format!("realm=test\nstatic-auth-secret={}\n", "a".repeat(64)),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let root = directory.path().to_str();
        assert_eq!(
            credential_secret("4play-turn.conf", root).unwrap(),
            "a".repeat(64)
        );
        assert!(
            secret_source(
                Some("a".repeat(64)),
                Some("4play-turn.conf".into()),
                root.map(String::from)
            )
            .is_err()
        );
        for text in [
            "no secret".into(),
            "static-auth-secret=short".into(),
            format!(
                "static-auth-secret={}\nstatic-auth-secret={}\n",
                "a".repeat(64),
                "b".repeat(64)
            ),
            "x".repeat(65537),
        ] {
            std::fs::write(&path, text).unwrap();
            let error = credential_secret("4play-turn.conf", root).unwrap_err();
            assert!(!error.contains(&"a".repeat(64)));
        }
        assert!(credential_secret("../4play-turn.conf", root).is_err());
        assert!(credential_secret("4play-turn.conf", None).is_err());
        assert!(credential_secret("4play-turn.conf", Some("relative")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn systemd_credential_rejects_symlinks_and_readable_by_other_users() {
        use std::os::unix::{fs::PermissionsExt, fs::symlink};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("4play-turn.conf");
        std::fs::write(&path, format!("static-auth-secret={}\n", "a".repeat(64))).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(credential_secret("4play-turn.conf", directory.path().to_str()).is_err());
        std::fs::rename(&path, directory.path().join("target")).unwrap();
        symlink(directory.path().join("target"), &path).unwrap();
        assert!(credential_secret("4play-turn.conf", directory.path().to_str()).is_err());
        std::fs::remove_file(&path).unwrap();
        let target = directory.path().join("target");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::hard_link(target, &path).unwrap();
        assert!(credential_secret("4play-turn.conf", directory.path().to_str()).is_err());
    }
}
