//! Updates through the project's Homebrew Channel catalog.
//!
//! Every release publishes a webOS Homebrew Channel repository (`apps.json`, the format of
//! repo.webosbrew.org) on the `homebrew-repo` branch. Once the catalog is added to the Homebrew
//! Channel, it lists RustCouch and offers "Update" when a newer version is out. The channel
//! does not notify by itself, so the app checks the catalog's manifest (one small HTTPS request
//! per start, switchable off) and shows a notice on Home.
//!
//! The Homebrew Channel's own launch parameter `{"launchMode":"addRepository","url":…}` opens its
//! settings with the repository URL filled in (webosbrew/webos-homebrew-channel
//! `frontend/views/MainView.js`), so adding the catalog needs no typing on the remote.

use std::sync::{Arc, Mutex};

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
    slot: Option<Arc<Mutex<Option<Result<String, String>>>>>,
    /// Newest version in the catalog, once known.
    pub latest: Option<String>,
    pub error: Option<String>,
}

impl UpdateState {
    pub fn start(&mut self, rt: &rdtv_core::hbb_common::tokio::runtime::Runtime) {
        let slot = Arc::new(Mutex::new(None));
        self.slot = Some(slot.clone());
        rt.spawn(async move {
            let r = match rdtv_core::http::request("GET", MANIFEST_URL, None, None).await {
                Ok(resp) if resp.status == 200 => serde_json::from_str::<serde_json::Value>(&resp.body)
                    .ok()
                    .and_then(|v| v.get("version")?.as_str().map(str::to_owned))
                    .ok_or_else(|| "catalog manifest has no version".to_owned()),
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
                Ok(v) => {
                    log::info!("catalog: latest {v}, running {}", current_version());
                    self.latest = Some(v);
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
    fn version_comparison() {
        assert!(is_newer("0.3.1", "0.3.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.3.0"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
        assert!(!is_newer("garbage", "0.3.0"));
    }
}
