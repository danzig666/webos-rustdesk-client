//! Server profiles. Each profile is an isolation scope: favourites, history, remembered
//! passwords, account tokens, address-book cache and per-computer settings live under the
//! profile id, never globally (see `store`).

use hbb_common::config::{RENDEZVOUS_PORT, RENDEZVOUS_SERVERS, RS_PUB_KEY};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileKind {
    /// The public RustDesk service built into upstream (`RENDEZVOUS_SERVERS`, `RS_PUB_KEY`).
    Public,
    /// A self-hosted RustDesk server (hbbs/hbbr, OSS or Pro).
    SelfHosted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerProfile {
    /// Stable local identifier, used as the storage scope. Never shown as a secret.
    pub id: String,
    pub name: String,
    pub kind: ProfileKind,
    /// ID/rendezvous server, `host` or `host:port` (default port 21116).
    #[serde(default)]
    pub id_server: String,
    /// Relay server, `host` or `host:port` (default port 21117). Optional: the rendezvous
    /// server normally tells the client which relay to use.
    #[serde(default)]
    pub relay_server: String,
    /// API server (accounts / address book), e.g. `https://rd.example.com`. Optional.
    #[serde(default)]
    pub api_server: String,
    /// Server public key (base64), as shown by the server's `id_ed25519.pub`.
    #[serde(default)]
    pub key: String,
}

impl ServerProfile {
    pub fn public() -> Self {
        ServerProfile {
            id: "public".into(),
            name: "RustDesk public server".into(),
            kind: ProfileKind::Public,
            id_server: String::new(),
            relay_server: String::new(),
            api_server: String::new(),
            key: String::new(),
        }
    }

    pub fn self_hosted(id: impl Into<String>, name: impl Into<String>) -> Self {
        ServerProfile {
            id: id.into(),
            name: name.into(),
            kind: ProfileKind::SelfHosted,
            id_server: String::new(),
            relay_server: String::new(),
            api_server: String::new(),
            key: String::new(),
        }
    }

    /// Rendezvous servers to try, in order.
    pub fn rendezvous_servers(&self) -> Vec<String> {
        match self.kind {
            ProfileKind::Public => RENDEZVOUS_SERVERS.iter().map(|s| s.to_string()).collect(),
            ProfileKind::SelfHosted => {
                let s = self.id_server.trim();
                if s.is_empty() {
                    vec![]
                } else {
                    vec![s.to_owned()]
                }
            }
        }
    }

    /// Key used for `licence_key` and peer-id signature verification. Mirrors upstream
    /// `get_key`: the configured key, or `RS_PUB_KEY` when none is configured.
    pub fn effective_key(&self) -> String {
        let k = self.key.trim();
        if k.is_empty() || self.kind == ProfileKind::Public {
            RS_PUB_KEY.to_owned()
        } else {
            k.to_owned()
        }
    }

    /// API server, mirroring upstream `get_api_server_`: explicit value, else derived from
    /// the custom ID server (port 21114), else the public console.
    pub fn effective_api_server(&self) -> String {
        let api = self.api_server.trim().trim_end_matches('/');
        if !api.is_empty() {
            return api.to_owned();
        }
        match self.kind {
            ProfileKind::Public => "https://admin.rustdesk.com".to_owned(),
            ProfileKind::SelfHosted => {
                let s0 = self.id_server.trim();
                if s0.is_empty() {
                    return String::new();
                }
                let s = hbb_common::socket_client::increase_port(s0, -2);
                if s == s0 {
                    format!("http://{}:{}", s, RENDEZVOUS_PORT - 2)
                } else {
                    format!("http://{s}")
                }
            }
        }
    }

    /// Validation used by the profile editor.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Name is required".into());
        }
        if self.kind == ProfileKind::SelfHosted {
            if self.id_server.trim().is_empty() {
                return Err("ID server is required for a self-hosted profile".into());
            }
            let k = self.key.trim();
            if !k.is_empty() && crate::transport::get_rs_pk(k).is_none() {
                return Err("Key must be the 44-character base64 public key".into());
            }
        }
        Ok(())
    }

    /// Import the upstream "server configuration string" (the reversed base64 JSON produced
    /// by RustDesk's "Export server config"): `{"host":..,"relay":..,"api":..,"key":..}`.
    pub fn from_upstream_config_string(id: &str, s: &str) -> Option<Self> {
        use hbb_common::base64::{engine::general_purpose::STANDARD, Engine};
        let s = s.trim();
        if s.is_empty() || s.len() > 4096 {
            return None;
        }
        let reversed: String = s.chars().rev().collect();
        let mut padded = reversed.clone();
        while !padded.len().is_multiple_of(4) {
            padded.push('=');
        }
        let bytes = STANDARD.decode(padded.as_bytes()).ok()?;
        let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
        let host = get("host");
        if host.is_empty() {
            return None;
        }
        let mut p = ServerProfile::self_hosted(id, host.clone());
        p.id_server = host;
        p.relay_server = get("relay");
        p.api_server = get("api");
        p.key = get("key");
        Some(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_profile_defaults() {
        let p = ServerProfile::public();
        assert_eq!(p.effective_key(), RS_PUB_KEY);
        assert!(!p.rendezvous_servers().is_empty());
        assert_eq!(p.effective_api_server(), "https://admin.rustdesk.com");
    }

    #[test]
    fn self_hosted_api_derivation() {
        let mut p = ServerProfile::self_hosted("a", "Home");
        p.id_server = "rd.example.com".into();
        assert_eq!(p.effective_api_server(), "http://rd.example.com:21114");
        p.id_server = "rd.example.com:21116".into();
        assert_eq!(p.effective_api_server(), "http://rd.example.com:21114");
        p.api_server = "https://api.example.com/".into();
        assert_eq!(p.effective_api_server(), "https://api.example.com");
        assert_eq!(p.effective_key(), RS_PUB_KEY);
        p.key = "OeVuKk5nlHiXp+APNn0Y3pC1Iwpwn44JGqrQCsWqmBw=".into();
        assert!(p.validate().is_ok());
        p.key = "nope".into();
        assert!(p.validate().is_err());
    }

    #[test]
    fn upstream_config_string_import() {
        use hbb_common::base64::{engine::general_purpose::STANDARD, Engine};
        let json = r#"{"host":"rd.example.com","relay":"","api":"","key":"abc"}"#;
        let enc = STANDARD.encode(json).trim_end_matches('=').to_string();
        let s: String = enc.chars().rev().collect();
        let p = ServerProfile::from_upstream_config_string("x", &s).unwrap();
        assert_eq!(p.id_server, "rd.example.com");
        assert_eq!(p.key, "abc");
        assert!(ServerProfile::from_upstream_config_string("x", "garbage").is_none());
    }
}
