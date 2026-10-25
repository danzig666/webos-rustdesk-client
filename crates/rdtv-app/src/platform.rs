//! Process-level platform setup: data directory, logging, webOS SDL hints.

use std::{io::Write, path::PathBuf, sync::Mutex};

/// Private, persistent data directory. On webOS the launcher sets `HOME` to the app's own
/// directory; settings live in `$HOME/data` (falls back to /tmp, non-persistent).
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("RDTV_DATA_DIR") {
        return PathBuf::from(d);
    }
    let base = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/remotedesk-tv"));
    let dir = if crate::keys::is_webos() {
        base.join("data")
    } else {
        base.join(".local/share/remotedesk-tv")
    };
    if std::fs::create_dir_all(&dir).is_ok() {
        dir
    } else {
        let t = PathBuf::from("/tmp/remotedesk-tv");
        let _ = std::fs::create_dir_all(&t);
        t
    }
}

struct FileLogger {
    file: Mutex<Option<std::fs::File>>,
    level: log::LevelFilter,
}

impl log::Log for FileLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= self.level
    }
    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        // Third-party crates log at info level per packet in places; keep ours and warnings.
        let target = r.target();
        if r.level() > log::Level::Warn && !(target.starts_with("rdtv") || target.starts_with("remotedesk")) {
            return;
        }
        let line = format!(
            "{} {:5} {}: {}\n",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            r.level(),
            target,
            r.args()
        );
        eprint!("{line}");
        if let Ok(mut g) = self.file.lock() {
            if let Some(f) = g.as_mut() {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }
    fn flush(&self) {}
}

/// Log to stderr and `<data>/logs/remotedesk-tv.log` (previous run kept as `.1`).
/// Logs never contain passwords, tokens or typed text.
pub fn init_logging(dir: &std::path::Path) -> PathBuf {
    let logs = dir.join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let path = logs.join("remotedesk-tv.log");
    let _ = std::fs::rename(&path, logs.join("remotedesk-tv.log.1"));
    let file = std::fs::File::create(&path).ok();
    let level = if std::env::var("RDTV_DEBUG").is_ok() {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };
    let logger = Box::leak(Box::new(FileLogger {
        file: Mutex::new(file),
        level,
    }));
    let _ = log::set_logger(logger);
    log::set_max_level(level);
    path
}

/// webOS SDL hints. Must be set before the window is created (they are latched then).
/// * Back/Exit are delivered to the app instead of the system (without both, a long Back
///   press makes webOS terminate the app; reported by punktfunk docs/NOTES.md).
/// * Home stays with the system so users can always leave the app.
pub fn set_sdl_hints() {
    use sdl2::hint::set;
    set("SDL_WEBOS_ACCESS_POLICY_KEYS_BACK", "true");
    set("SDL_WEBOS_ACCESS_POLICY_KEYS_EXIT", "true");
    set("SDL_WEBOS_CURSOR_SLEEP_TIME", "30000");
    set("SDL_TOUCH_MOUSE_EVENTS", "0");
    set("SDL_RENDER_SCALE_QUALITY", "1");
    set("SDL_VIDEO_EXTERNAL_CONTEXT", "0");
}

/// Hostname-like label sent to hosts as `my_name`.
pub fn client_name() -> String {
    if crate::keys::is_webos() {
        "LG TV (RustCouch)".to_owned()
    } else {
        "RustCouch (desktop)".to_owned()
    }
}

/// Make sure hbb_common's own config (socks proxy lookup etc.) stays inside our data dir.
pub fn confine_upstream_config(dir: &std::path::Path) {
    let up = dir.join("upstream");
    let _ = std::fs::create_dir_all(&up);
    if let Ok(mut d) = rdtv_core::hbb_common::config::APP_DIR.write() {
        *d = up.to_string_lossy().into_owned();
    }
    if let Ok(mut n) = rdtv_core::hbb_common::config::APP_NAME.write() {
        *n = "RemoteDeskTV".to_owned();
    }
}
