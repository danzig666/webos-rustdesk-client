//! Account sign-in and address book against a RustDesk API server.
//!
//! Endpoints and payloads follow upstream RustDesk 1.5.0 (`flutter/lib/models/user_model.dart`,
//! `flutter/lib/common/hbbs/hbbs.dart`, `flutter/lib/models/ab_model.dart`):
//! * `POST /api/login` `{username,password,id,uuid,autoLogin,type:"account",deviceInfo}`;
//!   the reply is `{type:"access_token",access_token,user}` or a challenge
//!   (`email_check` / `tfa_check` with `secret`), answered with
//!   `type:"email_code"` + `verificationCode` (+ `tfaCode` for TOTP).
//! * `POST /api/currentUser` (Bearer) to validate a stored token, `POST /api/logout`.
//! * Address book: Pro servers expose `POST /api/ab/personal` (guid) and
//!   `POST /api/ab/peers?current=N&pageSize=100&ab=<guid>`; older/compatible servers expose
//!   the legacy `GET /api/ab` whose `data` is a JSON string `{tags,peers}`.
//! A plain OSS hbbs/hbbr has no API server: these calls then fail with a clear message and
//! ID/password connections keep working (accounts are never faked).

use crate::http;
use hbb_common::{anyhow::bail, ResultType};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AbPeer {
    pub id: String,
    pub alias: String,
    pub hostname: String,
    pub platform: String,
    pub username: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum LoginOutcome {
    Token { token: String, user: String },
    /// The server wants an emailed or TOTP code.
    NeedCode { secret: String, tfa: bool },
}

pub struct Client {
    pub api: String,
    pub my_id: String,
    pub uuid: String,
    pub device_name: String,
}

fn err_of(v: &Value) -> Option<String> {
    v.get("error").and_then(|e| e.as_str()).map(|s| s.to_owned())
}

impl Client {
    fn url(&self, path: &str) -> ResultType<String> {
        if self.api.trim().is_empty() {
            bail!("This server profile has no API server, so accounts are not available. ID/password connections still work.");
        }
        Ok(format!("{}{}", self.api.trim_end_matches('/'), path))
    }

    fn device_info(&self) -> Value {
        json!({ "os": "webOS", "type": "client", "name": self.device_name })
    }

    fn parse_login(&self, status: u16, body: &str) -> ResultType<LoginOutcome> {
        let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        if let Some(e) = err_of(&v) {
            bail!("{e}");
        }
        if status == 404 {
            bail!("The server does not provide account sign-in (HTTP 404).");
        }
        if status != 200 {
            bail!("Sign-in failed (HTTP {status})");
        }
        match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "access_token" => {
                let token = v.get("access_token").and_then(|t| t.as_str()).unwrap_or("").to_owned();
                if token.is_empty() {
                    bail!("Server returned an empty token");
                }
                let user = v
                    .get("user")
                    .and_then(|u| u.get("display_name").and_then(|x| x.as_str()).filter(|s| !s.is_empty()).or_else(|| u.get("name").and_then(|x| x.as_str())))
                    .unwrap_or("")
                    .to_owned();
                Ok(LoginOutcome::Token { token, user })
            }
            t @ ("email_check" | "tfa_check") => Ok(LoginOutcome::NeedCode {
                secret: v.get("secret").and_then(|s| s.as_str()).unwrap_or("").to_owned(),
                tfa: t == "tfa_check",
            }),
            other => bail!("Unsupported sign-in method from server: {other}"),
        }
    }

    pub async fn login(&self, username: &str, password: &str) -> ResultType<LoginOutcome> {
        let body = json!({
            "username": username, "password": password, "id": self.my_id, "uuid": self.uuid,
            "autoLogin": true, "type": "account", "deviceInfo": self.device_info(),
        });
        let r = http::request("POST", &self.url("/api/login")?, None, Some(&body.to_string())).await?;
        self.parse_login(r.status, &r.body)
    }

    pub async fn login_code(&self, username: &str, code: &str, secret: &str, tfa: bool) -> ResultType<LoginOutcome> {
        let mut body = json!({
            "username": username, "id": self.my_id, "uuid": self.uuid, "autoLogin": true,
            "type": "email_code", "verificationCode": code, "secret": secret, "deviceInfo": self.device_info(),
        });
        if tfa {
            body["tfaCode"] = json!(code);
        }
        let r = http::request("POST", &self.url("/api/login")?, None, Some(&body.to_string())).await?;
        self.parse_login(r.status, &r.body)
    }

    /// Returns the user name, or an error (401/400 means the token is no longer valid).
    pub async fn current_user(&self, token: &str) -> ResultType<String> {
        let body = json!({ "id": self.my_id, "uuid": self.uuid });
        let r = http::request("POST", &self.url("/api/currentUser")?, Some(token), Some(&body.to_string())).await?;
        if r.status == 401 || r.status == 400 {
            bail!("signed out by the server");
        }
        let v: Value = serde_json::from_str(&r.body).unwrap_or(Value::Null);
        if let Some(e) = err_of(&v) {
            bail!("{e}");
        }
        Ok(v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_owned())
    }

    pub async fn logout(&self, token: &str) {
        let body = json!({ "id": self.my_id, "uuid": self.uuid });
        if let Ok(u) = self.url("/api/logout") {
            let _ = http::request("POST", &u, Some(token), Some(&body.to_string())).await;
        }
    }

    pub async fn address_book(&self, token: &str) -> ResultType<Vec<AbPeer>> {
        // Pro / current API first.
        let r = http::request("POST", &self.url("/api/ab/personal")?, Some(token), Some("{}")).await?;
        if r.status == 401 {
            bail!("signed out by the server");
        }
        if r.status == 200 {
            let v: Value = serde_json::from_str(&r.body).unwrap_or(Value::Null);
            if let Some(e) = err_of(&v) {
                bail!("{e}");
            }
            if let Some(guid) = v.get("guid").and_then(|g| g.as_str()) {
                return self.ab_peers(token, guid).await;
            }
        }
        // Legacy address book.
        let r = http::request("GET", &self.url("/api/ab")?, Some(token), None).await?;
        if r.status == 404 {
            bail!("The server does not provide an address book.");
        }
        if r.status == 401 {
            bail!("signed out by the server");
        }
        if r.body.trim().eq_ignore_ascii_case("null") || r.body.trim().is_empty() {
            return Ok(Vec::new());
        }
        let v: Value = serde_json::from_str(&r.body)?;
        if let Some(e) = err_of(&v) {
            bail!("{e}");
        }
        let data = v.get("data").and_then(|d| d.as_str()).unwrap_or("null");
        Ok(parse_legacy_ab(data))
    }

    async fn ab_peers(&self, token: &str, guid: &str) -> ResultType<Vec<AbPeer>> {
        let mut out = Vec::new();
        let mut current = 0;
        loop {
            current += 1;
            let mut u = url::Url::parse(&self.url("/api/ab/peers")?)?;
            u.query_pairs_mut()
                .append_pair("current", &current.to_string())
                .append_pair("pageSize", "100")
                .append_pair("ab", guid);
            let r = http::request("POST", u.as_str(), Some(token), Some("{}")).await?;
            if r.status != 200 {
                bail!("address book request failed (HTTP {})", r.status);
            }
            let v: Value = serde_json::from_str(&r.body)?;
            if let Some(e) = err_of(&v) {
                bail!("{e}");
            }
            let total = v.get("total").and_then(|t| t.as_u64()).unwrap_or(0) as usize;
            let page: Vec<AbPeer> = v
                .get("data")
                .and_then(|d| d.as_array())
                .map(|a| a.iter().filter_map(peer_from).collect())
                .unwrap_or_default();
            let n = page.len();
            out.extend(page);
            if n == 0 || out.len() >= total || current > 50 {
                break;
            }
        }
        Ok(out)
    }
}

fn peer_from(v: &Value) -> Option<AbPeer> {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
    let id = s("id");
    if id.is_empty() {
        return None;
    }
    Some(AbPeer {
        id,
        alias: s("alias"),
        hostname: s("hostname"),
        platform: s("platform"),
        username: s("username"),
        tags: v
            .get("tags")
            .and_then(|t| t.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_owned())).collect())
            .unwrap_or_default(),
    })
}

/// Legacy `data` payload: `{"tags":[...],"peers":[{id,alias,hostname,platform,tags,hash}]}`.
/// Password hashes stored in the address book are deliberately not used here.
pub fn parse_legacy_ab(data: &str) -> Vec<AbPeer> {
    let v: Value = serde_json::from_str(data).unwrap_or(Value::Null);
    v.get("peers")
        .and_then(|p| p.as_array())
        .map(|a| a.iter().filter_map(peer_from).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c() -> Client {
        Client { api: "https://x".into(), my_id: "1".into(), uuid: "u".into(), device_name: "tv".into() }
    }

    #[test]
    fn login_responses() {
        let ok = c().parse_login(200, r#"{"type":"access_token","access_token":"T","user":{"name":"bob"}}"#).unwrap();
        assert!(matches!(ok, LoginOutcome::Token { ref token, ref user } if token == "T" && user == "bob"));
        let ch = c().parse_login(200, r#"{"type":"tfa_check","secret":"S"}"#).unwrap();
        assert!(matches!(ch, LoginOutcome::NeedCode { tfa: true, .. }));
        assert!(c().parse_login(200, r#"{"error":"Wrong credentials"}"#).is_err());
        assert!(c().parse_login(404, "").is_err());
    }

    #[test]
    fn legacy_address_book() {
        let peers = parse_legacy_ab(r#"{"tags":["a"],"peers":[{"id":"123","alias":"Office","tags":["a"],"hash":"secret"},{"alias":"no id"}]}"#);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].alias, "Office");
    }

    #[test]
    fn no_api_server_is_a_clear_error() {
        let cl = Client { api: String::new(), ..c() };
        assert!(cl.url("/api/login").is_err());
    }
}
