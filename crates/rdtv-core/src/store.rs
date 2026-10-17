//! Persistent application state.
//!
//! Layout (all under the app's private data directory, files mode 0600):
//! * `state.json`   - profiles, per-profile peers/recents/settings, app settings. No secrets.
//! * `secrets.json` - remembered password hashes and account tokens, keyed by
//!   `(profile id, peer id)` / profile id. Never exported unless explicitly requested.
//!
//! Isolation rule: everything about a peer is keyed by the profile id first. The same peer
//! ID under two profiles is two unrelated records.
//!
//! Remembered passwords are stored as upstream does: the salted hash
//! `sha256(password || salt)` that the host's challenge is then applied to. It is not the
//! plaintext, but anyone with filesystem/root access to the TV can replay it against that
//! host. That is documented in docs/SECURITY.md; there is no reversible "encryption".

use crate::profile::ServerProfile;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

pub const STATE_VERSION: u32 = 1;
const MAX_RECENTS: usize = 30;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ViewMode {
    #[default]
    Fit,
    Original,
    Zoom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OskPresentation {
    /// Shrink the remote picture to fit above the keyboard (default).
    #[default]
    Shrink,
    /// Keyboard covers the lower part of the picture.
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DpadMode {
    #[default]
    Mouse,
    Arrows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum KeyboardLayout {
    #[default]
    EnUs,
    Hu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Quality {
    /// Upstream "balanced" (the upstream default).
    #[default]
    Balanced,
    /// Upstream "best".
    Best,
    /// Upstream "low" (fast).
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CodecPreference {
    /// Let the host choose among what this TV can decode (hardware first).
    #[default]
    Auto,
    H265,
    H264,
    Vp9,
    Vp8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    pub label: String,
    /// Key names understood by `input::parse_chord`, e.g. `["Ctrl","Shift","Esc"]`.
    pub keys: Vec<String>,
}

pub fn default_shortcuts(mac: bool) -> Vec<Shortcut> {
    let s = |label: &str, keys: &[&str]| Shortcut {
        label: label.to_owned(),
        keys: keys.iter().map(|k| k.to_string()).collect(),
    };
    if mac {
        vec![
            s("App switcher (Cmd+Tab)", &["Meta", "Tab"]),
            s("Force quit (Cmd+Opt+Esc)", &["Meta", "Alt", "Esc"]),
            s("Show desktop (F11)", &["F11"]),
            s("Copy (Cmd+C)", &["Meta", "C"]),
            s("Paste (Cmd+V)", &["Meta", "V"]),
        ]
    } else {
        vec![
            s("Switch window (Alt+Tab)", &["Alt", "Tab"]),
            s("Task Manager (Ctrl+Shift+Esc)", &["Ctrl", "Shift", "Esc"]),
            s("Show desktop (Win+D)", &["Meta", "D"]),
            s("Copy (Ctrl+C)", &["Ctrl", "C"]),
            s("Paste (Ctrl+V)", &["Ctrl", "V"]),
        ]
    }
}

/// Per-computer settings, scoped to a server profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PeerSettings {
    /// Selected monitor index and its name when last seen (for visible fallback).
    pub monitor: i32,
    pub monitor_name: String,
    pub view: ViewMode,
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub keyboard_layout: KeyboardLayout,
    pub osk_presentation: OskPresentation,
    pub dpad_mode: DpadMode,
    pub shortcuts: Option<Vec<Shortcut>>,
    pub quality: Quality,
    /// 0 = upstream default.
    pub fps: u32,
    pub codec: CodecPreference,
    pub muted: bool,
    pub lock_on_disconnect: bool,
    pub show_remote_cursor: bool,
}

impl Default for PeerSettings {
    fn default() -> Self {
        PeerSettings {
            monitor: 0,
            monitor_name: String::new(),
            view: ViewMode::Fit,
            zoom: 1.0,
            pan_x: 0.5,
            pan_y: 0.5,
            keyboard_layout: KeyboardLayout::EnUs,
            osk_presentation: OskPresentation::Shrink,
            dpad_mode: DpadMode::Mouse,
            shortcuts: None,
            quality: Quality::Balanced,
            fps: 0,
            codec: CodecPreference::Auto,
            muted: false,
            lock_on_disconnect: false,
            show_remote_cursor: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PeerRecord {
    pub alias: String,
    pub favorite: bool,
    /// Unix seconds.
    pub last_connected: i64,
    /// Informational, from the last PeerInfo: hostname / platform.
    pub hostname: String,
    pub platform: String,
    pub settings: PeerSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProfileData {
    pub peers: BTreeMap<String, PeerRecord>,
    pub recents: Vec<String>,
    /// Address book cached from the profile's API server (no secrets).
    pub address_book: Vec<crate::account::AbPeer>,
    pub account_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub first_run_done: bool,
    pub keep_connected_in_background: bool,
    pub ui_scale: f32,
    pub reduced_motion: bool,
    pub escape_chord_enabled: bool,
    pub prefer_hardware_decoding: bool,
    /// Experimental: drive the hardware video plane through an exported window so zoom,
    /// pan and keyboard-shrink also work with hardware decoding.
    pub hw_viewport_experimental: bool,
    pub audio_enabled_default: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            first_run_done: false,
            keep_connected_in_background: false,
            ui_scale: 1.0,
            reduced_motion: false,
            escape_chord_enabled: true,
            prefer_hardware_decoding: true,
            hw_viewport_experimental: false,
            audio_enabled_default: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub version: u32,
    pub profiles: Vec<ServerProfile>,
    pub active_profile: String,
    pub data: BTreeMap<String, ProfileData>,
    pub settings: AppSettings,
    /// Stable local client ID announced to hosts as `my_id` (not a secret).
    pub client_id: String,
}

impl Default for State {
    fn default() -> Self {
        State {
            version: STATE_VERSION,
            profiles: vec![ServerProfile::public()],
            active_profile: "public".into(),
            data: BTreeMap::new(),
            settings: AppSettings::default(),
            client_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Secrets {
    /// profile id -> peer id -> base64(sha256(password || salt))
    pub passwords: BTreeMap<String, BTreeMap<String, String>>,
    /// profile id -> API access token
    pub tokens: BTreeMap<String, String>,
}

pub struct Store {
    dir: PathBuf,
    pub state: State,
    pub secrets: Secrets,
}

fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        log::warn!("{} is too large; ignoring", path.display());
        return None;
    }
    std::fs::read(path).ok()
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

impl Store {
    pub fn open(dir: impl Into<PathBuf>) -> Store {
        let dir = dir.into();
        let _ = std::fs::create_dir_all(&dir);
        let mut state: State = read_bounded(&dir.join("state.json"))
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let secrets: Secrets = read_bounded(&dir.join("secrets.json"))
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self::migrate(&mut state);
        Store { dir, state, secrets }
    }

    fn migrate(state: &mut State) {
        if !state.profiles.iter().any(|p| p.id == "public") {
            state.profiles.insert(0, ServerProfile::public());
        }
        if !state.profiles.iter().any(|p| p.id == state.active_profile) {
            state.active_profile = state.profiles[0].id.clone();
        }
        if state.client_id.is_empty() {
            state.client_id = generate_client_id();
        }
        if state.settings.ui_scale < 0.75 || state.settings.ui_scale > 1.5 {
            state.settings.ui_scale = 1.0;
        }
        state.version = STATE_VERSION;
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn save(&self) {
        match serde_json::to_vec_pretty(&self.state) {
            Ok(b) => {
                if let Err(e) = write_private(&self.dir.join("state.json"), &b) {
                    log::error!("saving state failed: {e}");
                }
            }
            Err(e) => log::error!("serializing state failed: {e}"),
        }
        match serde_json::to_vec(&self.secrets) {
            Ok(b) => {
                if let Err(e) = write_private(&self.dir.join("secrets.json"), &b) {
                    log::error!("saving secrets failed: {e}");
                }
            }
            Err(e) => log::error!("serializing secrets failed: {e}"),
        }
    }

    pub fn active_profile(&self) -> &ServerProfile {
        self.state
            .profiles
            .iter()
            .find(|p| p.id == self.state.active_profile)
            .unwrap_or(&self.state.profiles[0])
    }

    pub fn profile_data(&self, profile: &str) -> ProfileData {
        self.state.data.get(profile).cloned().unwrap_or_default()
    }

    pub fn profile_data_mut(&mut self, profile: &str) -> &mut ProfileData {
        self.state.data.entry(profile.to_owned()).or_default()
    }

    pub fn peer(&self, profile: &str, peer: &str) -> PeerRecord {
        self.state
            .data
            .get(profile)
            .and_then(|d| d.peers.get(peer))
            .cloned()
            .unwrap_or_default()
    }

    pub fn peer_mut(&mut self, profile: &str, peer: &str) -> &mut PeerRecord {
        self.profile_data_mut(profile)
            .peers
            .entry(peer.to_owned())
            .or_default()
    }

    pub fn note_connected(&mut self, profile: &str, peer: &str, hostname: &str, platform: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        {
            let rec = self.peer_mut(profile, peer);
            rec.last_connected = now;
            if !hostname.is_empty() {
                rec.hostname = hostname.to_owned();
            }
            if !platform.is_empty() {
                rec.platform = platform.to_owned();
            }
        }
        let data = self.profile_data_mut(profile);
        data.recents.retain(|p| p != peer);
        data.recents.insert(0, peer.to_owned());
        data.recents.truncate(MAX_RECENTS);
    }

    pub fn remembered_password(&self, profile: &str, peer: &str) -> Option<Vec<u8>> {
        use hbb_common::base64::{engine::general_purpose::STANDARD, Engine};
        let s = self.secrets.passwords.get(profile)?.get(peer)?;
        STANDARD.decode(s).ok().filter(|v| v.len() == 32)
    }

    pub fn remember_password(&mut self, profile: &str, peer: &str, salted_hash: &[u8]) {
        use hbb_common::base64::{engine::general_purpose::STANDARD, Engine};
        self.secrets
            .passwords
            .entry(profile.to_owned())
            .or_default()
            .insert(peer.to_owned(), STANDARD.encode(salted_hash));
    }

    pub fn forget_password(&mut self, profile: &str, peer: &str) {
        if let Some(m) = self.secrets.passwords.get_mut(profile) {
            m.remove(peer);
        }
    }

    pub fn clear_history(&mut self, profile: &str) {
        let data = self.profile_data_mut(profile);
        data.recents.clear();
        data.peers.retain(|_, r| r.favorite);
    }

    pub fn token(&self, profile: &str) -> Option<&str> {
        self.secrets
            .tokens
            .get(profile)
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
    }

    pub fn set_token(&mut self, profile: &str, token: Option<String>) {
        match token {
            Some(t) => {
                self.secrets.tokens.insert(profile.to_owned(), t);
            }
            None => {
                self.secrets.tokens.remove(profile);
                let d = self.profile_data_mut(profile);
                d.address_book.clear();
                d.account_name.clear();
            }
        }
    }

    pub fn delete_profile(&mut self, profile: &str) -> bool {
        if profile == "public" {
            return false;
        }
        self.state.profiles.retain(|p| p.id != profile);
        self.state.data.remove(profile);
        self.secrets.passwords.remove(profile);
        self.secrets.tokens.remove(profile);
        if self.state.active_profile == profile {
            self.state.active_profile = "public".into();
        }
        true
    }

    /// Settings export. Secrets are excluded unless `include_secrets` is set explicitly.
    pub fn export(&self, include_secrets: bool) -> String {
        let mut v = serde_json::json!({ "remotedesk_tv_export": 1, "state": self.state });
        if include_secrets {
            v["secrets"] = serde_json::to_value(&self.secrets).unwrap_or_default();
        }
        serde_json::to_string_pretty(&v).unwrap_or_default()
    }

    pub fn new_profile_id(&self) -> String {
        loop {
            let id = format!("p{}", hbb_common::rand::random::<u32>() % 1_000_000);
            if !self.state.profiles.iter().any(|p| p.id == id) {
                return id;
            }
        }
    }
}

/// A 9-digit local identifier, in the shape of RustDesk IDs. Only used as `my_id` in the
/// login request (hosts show it in their connection-manager window).
fn generate_client_id() -> String {
    let n: u32 = 100_000_000 + hbb_common::rand::random::<u32>() % 900_000_000;
    n.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_isolation_and_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path());
        let mut p2 = ServerProfile::self_hosted("home", "Home");
        p2.id_server = "rd.home".into();
        s.state.profiles.push(p2);
        s.remember_password("public", "123456789", &[1u8; 32]);
        s.peer_mut("public", "123456789").settings.muted = true;
        s.note_connected("public", "123456789", "pc", "Windows");
        s.save();

        let s = Store::open(dir.path());
        assert_eq!(s.remembered_password("public", "123456789"), Some(vec![1u8; 32]));
        // Same peer ID on another profile shares nothing.
        assert_eq!(s.remembered_password("home", "123456789"), None);
        assert!(!s.peer("home", "123456789").settings.muted);
        assert!(s.peer("public", "123456789").settings.muted);
        assert_eq!(s.profile_data("public").recents, vec!["123456789".to_string()]);
        assert!(s.profile_data("home").recents.is_empty());
        assert_eq!(s.state.client_id.len(), 9);
    }

    #[test]
    fn export_redacts_secrets_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path());
        s.remember_password("public", "1", &[7u8; 32]);
        s.set_token("public", Some("tok".into()));
        let e = s.export(false);
        assert!(!e.contains("tok"));
        assert!(!e.contains("BwcH"));
        assert!(s.export(true).contains("tok"));
    }

    #[test]
    fn forget_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path());
        s.state.profiles.push(ServerProfile::self_hosted("x", "X"));
        s.remember_password("x", "1", &[1u8; 32]);
        s.forget_password("x", "1");
        assert_eq!(s.remembered_password("x", "1"), None);
        s.remember_password("x", "1", &[1u8; 32]);
        assert!(s.delete_profile("x"));
        assert!(!s.delete_profile("public"));
        assert!(s.secrets.passwords.get("x").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path());
        s.save();
        let m = std::fs::metadata(dir.path().join("secrets.json")).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o600);
    }
}
