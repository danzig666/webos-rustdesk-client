//! Updates through the project's Homebrew Channel catalog.
//!
//! Every release publishes a webOS Homebrew Channel repository (`apps.json`, the format of
//! repo.webosbrew.org) on the `homebrew-repo` branch. Once the catalog is added to the Homebrew
//! Channel, it lists RustCouch and offers "Update" when a newer version is out. The channel
//! does not notify by itself, so the app checks the catalog's manifest (one small HTTPS request
//! per start, switchable off) and shows a notice on Home.
//!
//! Installing in the app (as in the author's ytnative): the manifest names the package and
//! its SHA-256, and the app hands both to Homebrew Channel's service
//! (`luna://org.webosbrew.hbchannel.service/install`), which downloads, verifies and installs
//! it; an app may not install packages itself, and installing closes it.
//!
//! The Homebrew Channel's own launch parameter `{"launchMode":"addRepository","url":…}` opens its
//! settings with the repository URL filled in (webosbrew/webos-homebrew-channel
//! `frontend/views/MainView.js`), so adding the catalog needs no typing on the remote.

use std::sync::{Arc, Mutex};

pub const NEEDS_HBCHANNEL: &str = "Homebrew Channel is needed to install updates";

/// A release from the catalog manifest.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub ipk_url: String,
    pub sha256: String,
}

pub fn parse_manifest(json: &str) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    Some(Release {
        version: v["version"].as_str()?.to_string(),
        ipk_url: v["ipkUrl"].as_str()?.to_string(),
        sha256: v["ipkHash"]["sha256"].as_str()?.to_string(),
    })
}

/// Progress of an install handed to Homebrew Channel.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    Idle,
    /// Downloading (percent, when Homebrew Channel says).
    Downloading(Option<u32>),
    Installing,
    Installed,
    Failed(String),
}

/// One reply of Homebrew Channel's install, read into a state (None: nothing to show).
pub fn read_install_reply(reply: &str) -> Option<Install> {
    let v: serde_json::Value = serde_json::from_str(reply).ok()?;
    if v["returnValue"] == serde_json::Value::Bool(false) || v.get("errorText").is_some() {
        let t = v["errorText"].as_str().unwrap_or("the installer refused");
        // No Homebrew Channel on this TV: the bus answers for the missing service.
        if t.contains("does not exist") || t.contains("Unknown service") {
            return Some(Install::Failed(NEEDS_HBCHANNEL.into()));
        }
        return Some(Install::Failed(t.to_string()));
    }
    if v["finished"] == serde_json::Value::Bool(true) {
        return Some(Install::Installed);
    }
    match v["statusText"].as_str() {
        Some(t) if t.starts_with("Download") => Some(Install::Downloading(
            v["progress"].as_f64().map(|p| p.clamp(0.0, 100.0) as u32),
        )),
        Some(t) if t.starts_with("Verif") || t.starts_with("Install") => Some(Install::Installing),
        _ => None,
    }
}

pub const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/danzig666/webos-rustdesk-client/homebrew-repo/apps.json";
const MANIFEST_URL: &str = "https://raw.githubusercontent.com/danzig666/webos-rustdesk-client/homebrew-repo/io.github.danzig666.remotedesktv.manifest.json";
const HBCHANNEL_ID: &str = "org.webosbrew.hbchannel";

/// The version this build was released as (set by CI from the release tag).
pub fn current_version() -> &'static str {
    option_env!("RDTV_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

fn parse(v: &str) -> Option<Vec<u32>> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(['-', '+']).next()?;
    core.split('.').map(|p| p.parse().ok()).collect()
}

/// `latest` is a newer release than `current` (numeric dotted versions).
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse(latest), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[derive(Default)]
pub struct UpdateState {
    slot: Option<Arc<Mutex<Option<Result<Release, String>>>>>,
    /// Newest version in the catalog, once known.
    pub latest: Option<String>,
    pub release: Option<Release>,
    pub error: Option<String>,
    install: Option<Arc<Mutex<Install>>>,
    /// The "update available" question was shown this run.
    pub prompted: bool,
}

impl UpdateState {
    pub fn start(&mut self, rt: &rdtv_core::hbb_common::tokio::runtime::Runtime) {
        let slot = Arc::new(Mutex::new(None));
        self.slot = Some(slot.clone());
        rt.spawn(async move {
            let r = match rdtv_core::http::request("GET", MANIFEST_URL, None, None).await {
                Ok(resp) if resp.status == 200 => {
                    parse_manifest(&resp.body).ok_or_else(|| "catalog manifest is incomplete".to_owned())
                }
                Ok(resp) => Err(format!("catalog returned HTTP {}", resp.status)),
                Err(e) => Err(e.to_string()),
            };
            if let Ok(mut g) = slot.lock() {
                *g = Some(r);
            }
        });
    }

    /// Pick up a finished check (call once per frame; cheap).
    pub fn poll(&mut self) {
        let Some(slot) = &self.slot else { return };
        let r = slot.lock().ok().and_then(|mut g| g.take());
        if let Some(r) = r {
            self.slot = None;
            match r {
                Ok(r) => {
                    log::info!("catalog: latest {}, running {}", r.version, current_version());
                    self.latest = Some(r.version.clone());
                    self.release = Some(r);
                    self.error = None;
                }
                Err(e) => {
                    log::info!("update check failed: {e}");
                    self.error = Some(e);
                }
            }
        }
    }

    pub fn available(&self) -> Option<&str> {
        self.latest.as_deref().filter(|l| is_newer(l, current_version()))
    }

    pub fn checking(&self) -> bool {
        self.slot.is_some()
    }

    pub fn install_state(&self) -> Install {
        self.install
            .as_ref()
            .and_then(|s| s.lock().ok().map(|g| g.clone()))
            .unwrap_or(Install::Idle)
    }

    /// Hand the newer release to Homebrew Channel, which downloads, verifies (SHA-256) and
    /// installs it. Installing closes the app.
    pub fn install(&mut self) {
        let Some(rel) = self
            .release
            .clone()
            .filter(|r| is_newer(&r.version, current_version()))
        else {
            return;
        };
        if matches!(
            self.install_state(),
            Install::Downloading(_) | Install::Installing | Install::Installed
        ) {
            return;
        }
        let state = Arc::new(Mutex::new(Install::Downloading(None)));
        self.install = Some(state.clone());
        log::info!("update: installing {} through Homebrew Channel", rel.version);
        let payload = serde_json::json!({
            "ipkUrl": rel.ipk_url, "ipkHash": rel.sha256, "id": rdtv_media::APP_ID, "subscribe": true,
        })
        .to_string();
        let sent = match crate::luna::Luna::wide(rdtv_media::APP_ID) {
            Ok(luna) => {
                let st = state.clone();
                luna.subscribe(
                    "luna://org.webosbrew.hbchannel.service/install",
                    &payload,
                    move |reply| {
                        if let Some(s) = read_install_reply(reply) {
                            if let Install::Failed(e) = &s {
                                log::warn!("update: install failed: {e}");
                            }
                            if let Ok(mut g) = st.lock() {
                                // A late reply never turns a failure back into progress.
                                if !matches!(*g, Install::Failed(_) | Install::Installed) {
                                    *g = s;
                                }
                            }
                        }
                    },
                )
            }
            Err(e) => {
                log::warn!("update: no bus: {e}");
                false
            }
        };
        if !sent {
            if let Ok(mut g) = state.lock() {
                *g = Install::Failed(NEEDS_HBCHANNEL.into());
            }
        }
    }
}

/// Launch the Homebrew Channel, optionally straight into "add repository" with our catalog.
pub fn open_hbchannel(add_catalog: bool) -> Result<(), String> {
    let params = if add_catalog {
        serde_json::json!({ "launchMode": "addRepository", "url": CATALOG_URL })
    } else {
        serde_json::json!({})
    };
    let payload = serde_json::json!({ "id": HBCHANNEL_ID, "params": params }).to_string();
    let mut last = String::new();
    // luna-send-pub is the public-bus client apps may use; luna-send exists on rooted TVs.
    for tool in ["luna-send-pub", "luna-send"] {
        match std::process::Command::new(tool)
            .args(["-n", "1", "luna://com.webos.applicationManager/launch", &payload])
            .output()
        {
            Ok(o)
                if o.status.success()
                    && !String::from_utf8_lossy(&o.stdout).contains("\"returnValue\":false") =>
            {
                return Ok(())
            }
            Ok(o) => {
                last = format!(
                    "{tool}: {}",
                    String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .chars()
                        .take(200)
                        .collect::<String>()
                )
            }
            Err(e) => last = format!("{tool}: {e}"),
        }
    }
    Err(format!("could not open the Homebrew Channel ({last})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_and_install_replies() {
        let r =
            parse_manifest(r#"{"version":"0.11.2","ipkUrl":"https://x/a.ipk","ipkHash":{"sha256":"ab"}}"#)
                .unwrap();
        assert_eq!((r.version.as_str(), r.sha256.as_str()), ("0.11.2", "ab"));
        assert!(parse_manifest(r#"{"version":"0.11.2"}"#).is_none());
        assert_eq!(
            read_install_reply(r#"{"statusText":"Downloading…","progress":41.7}"#),
            Some(Install::Downloading(Some(41)))
        );
        assert_eq!(
            read_install_reply(r#"{"statusText":"Installing…"}"#),
            Some(Install::Installing)
        );
        assert_eq!(
            read_install_reply(r#"{"statusText":"Finished.","finished":true}"#),
            Some(Install::Installed)
        );
        assert_eq!(
            read_install_reply(
                r#"{"returnValue":false,"errorText":"Service does not exist: org.webosbrew.hbchannel.service."}"#
            ),
            Some(Install::Failed(NEEDS_HBCHANNEL.into()))
        );
        assert_eq!(read_install_reply(r#"{"returnValue":true}"#), None);
    }

    #[test]
    fn version_comparison() {
        assert!(is_newer("0.3.1", "0.3.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.3.0"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
        assert!(!is_newer("garbage", "0.3.0"));
    }
}
