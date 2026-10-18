//! Coturn REST credentials: only short-lived derived credentials reach browsers.
use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha1::Sha1;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct WanMedia {
    host: String,
    secret: String,
}
impl WanMedia {
    pub fn from_environment() -> Result<Option<Self>, String> {
        match (
            std::env::var("FOURPLAY_WAN_TURN_HOST").ok(),
            std::env::var("FOURPLAY_WAN_TURN_SECRET").ok(),
        ) {
            (None, None) => Ok(None),
            (Some(host), Some(secret)) => Ok(Some(Self::new(host, secret)?)),
            _ => Err(
                "Set both FOURPLAY_WAN_TURN_HOST and FOURPLAY_WAN_TURN_SECRET, or neither".into(),
            ),
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
        ],"iceTransportPolicy":"all","expires_unix_ms":expiry*1000})
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
}
