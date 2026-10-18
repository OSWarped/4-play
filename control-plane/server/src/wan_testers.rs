//! Opt-in admission gate for the co-located nginx edge, separate from LAN trust.
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::http::{HeaderMap, StatusCode};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

use crate::tester_access::{AccessError, TesterAccess};

pub(crate) const COOKIE: &str = "__Secure-fourplay_tester";
type Signature = Hmac<Sha256>;

/// All fields must be supplied together. Debug is intentionally not implemented.
pub struct WanTesterConfig {
    pub origin: String,
    pub edge_key: String,
    pub database: PathBuf,
}

impl WanTesterConfig {
    pub fn from_environment() -> Result<Option<Self>, String> {
        Self::from_settings(
            std::env::var("FOURPLAY_WAN_TESTER_ORIGIN").ok(),
            std::env::var("FOURPLAY_WAN_EDGE_KEY").ok(),
            std::env::var("FOURPLAY_WAN_TESTER_DATABASE").ok(),
        )
    }

    fn from_settings(
        origin: Option<String>,
        edge_key: Option<String>,
        database: Option<String>,
    ) -> Result<Option<Self>, String> {
        match (origin, edge_key, database) {
            (None, None, None) => Ok(None),
            (Some(origin), Some(edge_key), Some(database)) if !database.is_empty() => {
                Ok(Some(Self { origin, edge_key, database: database.into() }))
            }
            _ => Err("Set all three FOURPLAY_WAN_TESTER_ORIGIN, FOURPLAY_WAN_EDGE_KEY, and FOURPLAY_WAN_TESTER_DATABASE, or none".into()),
        }
    }
}

#[derive(Clone)]
pub(crate) struct WanGate {
    pub access: TesterAccess,
    pub origin: String,
    authority: String,
    pub media: Option<crate::wan_media::WanMedia>,
    edge_signature: Vec<u8>,
    attempts: Arc<Mutex<Attempts>>,
}

#[derive(Default)]
struct Attempts {
    window: Option<Instant>,
    total: u32,
    ips: HashMap<IpAddr, u32>,
}

impl Attempts {
    fn take(&mut self, ip: IpAddr, now: Instant) -> bool {
        if self
            .window
            .is_none_or(|start| now.saturating_duration_since(start) >= Duration::from_secs(60))
        {
            self.window = Some(now);
            self.total = 0;
            self.ips.clear();
        }
        // One bounded map, at most 60 entries, regardless of forged/new addresses.
        if self.total >= 60 || self.ips.get(&ip).copied().unwrap_or(0) >= 5 {
            return false;
        }
        self.total += 1;
        *self.ips.entry(ip).or_default() += 1;
        true
    }
}

#[derive(Debug)]
pub(crate) struct GateError(pub StatusCode, pub &'static str, pub &'static str);

impl WanGate {
    pub async fn open(config: WanTesterConfig) -> Result<Self, String> {
        let url = reqwest::Url::parse(&config.origin).map_err(|_| "Invalid WAN tester origin")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || config.origin != url.origin().ascii_serialization()
            || config.edge_key.len() != 64
            || !config.edge_key.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(
                "WAN access requires an exact HTTPS origin and a separate random 64-hex edge key"
                    .into(),
            );
        }
        // Unlike the operator CLI, server startup must never create a new empty
        // invitation database on a typo or accidentally reset admission state.
        if !config.database.is_file() {
            return Err("Private WAN invitation database must already exist".into());
        }
        let access = TesterAccess::open(&config.database)
            .await
            .map_err(|e| e.to_string())?;
        let mut mac =
            Signature::new_from_slice(config.edge_key.as_bytes()).expect("HMAC accepts key");
        mac.update(b"4play-wan-edge-v1");
        Ok(Self {
            access,
            origin: config.origin.clone(),
            authority: config.origin[8..].into(),
            media: crate::wan_media::WanMedia::from_environment()?,
            edge_signature: mac.finalize().into_bytes().to_vec(),
            attempts: Arc::new(Mutex::new(Attempts::default())),
        })
    }

    pub fn matches_host(&self, headers: &HeaderMap) -> bool {
        headers
            .get("host")
            .is_some_and(|h| h == self.authority.as_str())
    }

    pub fn transport(&self, headers: &HeaderMap, peer: SocketAddr) -> Result<IpAddr, GateError> {
        // Do not trust a whole LAN CIDR, forwarded header chain, or every local
        // request. The nginx edge must overwrite all three headers below.
        if peer.ip() != "127.0.0.1".parse::<IpAddr>().unwrap() || !self.matches_host(headers) {
            return Err(forbidden());
        }
        let key = single_header(headers, "x-4play-edge-key").ok_or_else(forbidden)?;
        if key.len() != 64 {
            return Err(forbidden());
        }
        let mut mac = Signature::new_from_slice(key.as_bytes()).expect("HMAC accepts key");
        mac.update(b"4play-wan-edge-v1");
        mac.verify_slice(&self.edge_signature)
            .map_err(|_| forbidden())?;
        let ip = single_header(headers, "x-4play-client-ip")
            .and_then(|s| s.parse::<IpAddr>().ok())
            .ok_or_else(forbidden)?;
        let ip = match ip {
            IpAddr::V6(v) => v.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
            _ => ip,
        };
        if ip.is_loopback()
            || ip.is_unspecified()
            || ip.is_multicast()
            || ip == IpAddr::from([255, 255, 255, 255])
        {
            return Err(forbidden());
        }
        Ok(ip)
    }

    pub fn request(
        &self,
        headers: &HeaderMap,
        peer: SocketAddr,
        write: bool,
    ) -> Result<IpAddr, GateError> {
        let ip = self.transport(headers, peer)?;
        let origin = single_header(headers, "origin");
        if single_header(headers, "x-4play-client") != Some("v2")
            || origin.is_some_and(|o| o != self.origin)
            || (write && origin != Some(self.origin.as_str()))
            || headers
                .get("sec-fetch-site")
                .is_some_and(|h| h != "same-origin" && h != "none")
        {
            return Err(forbidden());
        }
        Ok(ip)
    }

    pub async fn identity(&self, headers: &HeaderMap, now: u64) -> Result<Uuid, GateError> {
        let credential = credential(headers).ok_or_else(required)?;
        self.access
            .authenticate(credential, now)
            .await
            .map_err(|e| match e {
                AccessError::Storage => GateError(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "tester_storage_unavailable",
                    "Tester access is temporarily unavailable.",
                ),
                _ => required(),
            })
    }

    pub fn take_attempt(&self, ip: IpAddr) -> Result<(), GateError> {
        if self
            .attempts
            .lock()
            .map_err(|_| forbidden())?
            .take(ip, Instant::now())
        {
            Ok(())
        } else {
            Err(GateError(
                StatusCode::TOO_MANY_REQUESTS,
                "tester_rate_limited",
                "Too many attempts. Please wait a minute.",
            ))
        }
    }
}

fn single_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        None
    } else {
        Some(value)
    }
}

fn credential(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers
        .get_all("cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|v| v.trim().split_once('='))
        .filter_map(|(name, value)| (name == COOKIE).then_some(value));
    let value = values.next()?;
    if values.next().is_some() {
        None
    } else {
        Some(value)
    }
}
pub(crate) fn admission_fingerprint(headers: &HeaderMap) -> Option<String> {
    credential(headers).map(crate::tester_access::admission_fingerprint)
}

fn forbidden() -> GateError {
    GateError(
        StatusCode::FORBIDDEN,
        "tester_edge_denied",
        "Unapproved tester request.",
    )
}
fn required() -> GateError {
    GateError(
        StatusCode::UNAUTHORIZED,
        "tester_access_required",
        "Enter your tester invitation to continue.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_are_all_or_nothing() {
        assert!(
            WanTesterConfig::from_settings(None, None, None)
                .unwrap()
                .is_none()
        );
        assert!(
            WanTesterConfig::from_settings(Some("https://arcade.test".into()), None, None).is_err()
        );
    }
    #[test]
    fn rate_limiter_bounds_ip_and_global_attempts_and_resets() {
        let mut attempts = Attempts::default();
        let now = Instant::now();
        let ip = "192.0.2.1".parse().unwrap();
        for _ in 0..5 {
            assert!(attempts.take(ip, now));
        }
        assert!(!attempts.take(ip, now));
        for index in 2..57 {
            assert!(attempts.take(IpAddr::from([192, 0, 2, index]), now));
        }
        assert!(!attempts.take(IpAddr::from([192, 0, 2, 100]), now));
        assert_eq!(attempts.ips.len(), 56);
        assert!(attempts.take(ip, now + Duration::from_secs(60)));
    }
    #[test]
    fn duplicate_credentials_are_rejected() {
        let mut headers = HeaderMap::new();
        headers.append("cookie", format!("{COOKIE}=one").parse().unwrap());
        headers.append("cookie", format!("{COOKIE}=two").parse().unwrap());
        assert!(credential(&headers).is_none());
    }
}
