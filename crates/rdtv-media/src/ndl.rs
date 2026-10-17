//! LG webOS NDL DirectMedia (API 2, webOS 5+) hardware video decoding.
//!
//! The library is `dlopen`ed at runtime (never linked): a `DT_NEEDED` on
//! `libNDL_directmedia.so.1` would stop the app from starting where it is missing, and a
//! missing symbol must become a capability failure, not a crash.
//!
//! Struct layouts follow `webosbrew/webos-userland` (`include/libndl-media`). The
//! video-only load with a wall-clock PTS in milliseconds since load follows the approach
//! used by mariotaku/ss4s (`modules/webos/ndl/webos5`, used by Moonlight TV), and the
//! dlopen/state-callback handling follows punktfunk/client-webos
//! (`src/platform/webos/ndl`, MIT/Apache-2.0). See docs/SOURCES.md.
//!
//! Decoded pictures go to a hardware video plane underneath the UI's GL surface; the app
//! must keep the window mapped and clear the video area to transparent black.
//! NDL API 2 offers no destination rectangle: the picture is presented full-screen by the
//! TV (see docs/COMPATIBILITY.md for the viewport consequences).

use anyhow::{anyhow, bail, Result};
use std::{
    ffi::{c_char, c_int, c_longlong, c_uint, c_void, CStr, CString},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HwCodec {
    H264,
    H265,
    Vp9,
    Av1,
}

impl HwCodec {
    fn ndl_type(self) -> c_int {
        match self {
            HwCodec::H264 => 1,
            HwCodec::H265 => 2,
            HwCodec::Vp9 => 3,
            HwCodec::Av1 => 4,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            HwCodec::H264 => "H.264",
            HwCodec::H265 => "H.265",
            HwCodec::Vp9 => "VP9",
            HwCodec::Av1 => "AV1",
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VideoInfo {
    width: c_int,
    height: c_int,
    kind: c_int,
    unknown1: c_int,
}

#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct AudioUnion {
    bytes: [u8; 32],
}

#[repr(C)]
struct DataInfo {
    video: VideoInfo,
    audio: AudioUnion,
}

type LoadCb = Option<extern "C" fn(c_int, c_longlong, *const c_char)>;

struct Lib {
    init: unsafe extern "C" fn(*const c_char) -> c_int,
    quit: unsafe extern "C" fn() -> c_int,
    get_error: unsafe extern "C" fn() -> *const c_char,
    load: unsafe extern "C" fn(*mut DataInfo, LoadCb) -> c_int,
    unload: unsafe extern "C" fn() -> c_int,
    video_play: unsafe extern "C" fn(*mut c_void, c_uint, c_longlong) -> c_int,
    flush: unsafe extern "C" fn() -> c_int,
    render_len: unsafe extern "C" fn(*mut c_int) -> c_int,
}

unsafe impl Send for Lib {}
unsafe impl Sync for Lib {}

const LIB_NAME: &CStr = c"libNDL_directmedia.so.1";

unsafe fn sym<T: Copy>(h: *mut c_void, name: &CStr) -> Result<T> {
    let p = libc::dlsym(h, name.as_ptr());
    if p.is_null() {
        bail!("missing symbol {}", name.to_string_lossy());
    }
    Ok(std::mem::transmute_copy::<*mut c_void, T>(&p))
}

fn lib() -> Result<&'static Lib> {
    static LIB: OnceLock<std::result::Result<Lib, String>> = OnceLock::new();
    LIB.get_or_init(|| unsafe {
        let h = libc::dlopen(LIB_NAME.as_ptr(), libc::RTLD_LAZY | libc::RTLD_GLOBAL);
        if h.is_null() {
            let e = libc::dlerror();
            let msg = if e.is_null() {
                "dlopen failed".to_owned()
            } else {
                CStr::from_ptr(e).to_string_lossy().into_owned()
            };
            return Err(format!("NDL DirectMedia not available: {msg}"));
        }
        let r = (|| -> Result<Lib> {
            Ok(Lib {
                init: sym(h, c"NDL_DirectMediaInit")?,
                quit: sym(h, c"NDL_DirectMediaQuit")?,
                get_error: sym(h, c"NDL_DirectMediaGetError")?,
                load: sym(h, c"NDL_DirectMediaLoad")?,
                unload: sym(h, c"NDL_DirectMediaUnload")?,
                video_play: sym(h, c"NDL_DirectVideoPlay")?,
                flush: sym(h, c"NDL_DirectVideoFlushRenderBuffer")?,
                render_len: sym(h, c"NDL_DirectVideoGetRenderBufferLength")?,
            })
        })();
        r.map_err(|e| format!("NDL DirectMedia API 2 incomplete: {e}"))
    })
    .as_ref()
    .map_err(|e| anyhow!("{e}"))
}

fn last_error(l: &Lib) -> String {
    let p = unsafe { (l.get_error)() };
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Whether the NDL API 2 library and all required symbols are present.
pub fn available() -> Result<()> {
    lib().map(|_| ())
}

const STATE_LOADCOMPLETED: c_int = 0x16;
const STATE_ERROR: c_int = 0x12;
static LOAD_SEQ: AtomicU64 = AtomicU64::new(0);
static ERROR_SEQ: AtomicU64 = AtomicU64::new(0);
static INIT_DONE: AtomicBool = AtomicBool::new(false);
/// Serializes all NDL calls (the library is process-global and not documented as
/// thread-safe).
static FFI: Mutex<()> = Mutex::new(());

extern "C" fn on_load_state(state: c_int, num: c_longlong, detail: *const c_char) {
    match state {
        STATE_LOADCOMPLETED => {
            LOAD_SEQ.fetch_add(1, Ordering::SeqCst);
        }
        STATE_ERROR => {
            ERROR_SEQ.fetch_add(1, Ordering::SeqCst);
            let d = if detail.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(detail) }.to_string_lossy().into_owned()
            };
            log::error!("NDL fatal state 0x{state:x} num={num} {d}");
        }
        _ => log::trace!("NDL state 0x{state:x} num={num}"),
    }
}

fn app_id() -> String {
    std::env::var("APPID").unwrap_or_else(|_| crate::APP_ID.to_owned())
}

fn ensure_init(l: &Lib) -> Result<()> {
    if INIT_DONE.load(Ordering::SeqCst) {
        return Ok(());
    }
    let id = CString::new(app_id()).unwrap_or_default();
    let rc = unsafe { (l.init)(id.as_ptr()) };
    if rc != 0 {
        bail!("NDL_DirectMediaInit failed: {rc} {}", last_error(l));
    }
    INIT_DONE.store(true, Ordering::SeqCst);
    Ok(())
}

/// Exported (punch-through) window the decoded picture should be bound to, if the app
/// created one. NDL API 2 cannot use it (`NDL_DirectMediaSetWindowId` left the picture
/// full-screen on a C3, 0.1.3 test), so only the Starfish backend (`crate::smp`) binds it.
static TARGET_WINDOW: Mutex<Option<CString>> = Mutex::new(None);
static WINDOW_BOUND: AtomicBool = AtomicBool::new(false);
static WINDOW_GEN: AtomicU64 = AtomicU64::new(0);
static WINDOW_STATUS: Mutex<String> = Mutex::new(String::new());

pub fn set_target_window(id: Option<&str>) {
    let mut g = TARGET_WINDOW.lock().unwrap_or_else(|e| e.into_inner());
    *g = id.and_then(|s| CString::new(s).ok());
    if g.is_none() {
        WINDOW_BOUND.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn target_window() -> Option<CString> {
    TARGET_WINDOW.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Whether the current decoder presents into the app's exported window.
pub fn window_bound() -> bool {
    WINDOW_BOUND.load(Ordering::SeqCst)
}

/// Changes on every decoder load: the window placement must be sent again.
pub fn window_generation() -> u64 {
    WINDOW_GEN.load(Ordering::SeqCst)
}

pub fn window_status() -> String {
    WINDOW_STATUS.lock().map(|s| s.clone()).unwrap_or_default()
}

pub(crate) fn set_window_state(bound: bool, status: String) {
    log::info!("hardware picture window: {status}");
    WINDOW_BOUND.store(bound, Ordering::SeqCst);
    WINDOW_GEN.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut g) = WINDOW_STATUS.lock() {
        *g = status;
    }
}

/// Release the process-wide NDL instance (on app exit / background release).
pub fn quit() {
    if INIT_DONE.swap(false, Ordering::SeqCst) {
        if let Ok(l) = lib() {
            let _g = FFI.lock().unwrap_or_else(|e| e.into_inner());
            unsafe { (l.quit)() };
        }
    }
}

/// One loaded hardware decode pipeline (one codec, one stream size).
pub struct NdlPlayer {
    l: &'static Lib,
    pub codec: HwCodec,
    pub width: i32,
    pub height: i32,
    loaded_at: Instant,
    load_seq: u64,
    error_seq: u64,
    feeding: bool,
    refused_since: Option<Instant>,
    frames: u64,
}

const FEED_ANYWAY_AFTER: Duration = Duration::from_millis(300);
const DEAD_AFTER_REFUSED: Duration = Duration::from_secs(3);

impl NdlPlayer {
    pub fn load(codec: HwCodec, width: i32, height: i32) -> Result<NdlPlayer> {
        let l = lib()?;
        if width <= 0 || height <= 0 || width > 8192 || height > 8192 {
            bail!("invalid stream size {width}x{height}");
        }
        let _g = FFI.lock().unwrap_or_else(|e| e.into_inner());
        ensure_init(l)?;
        let mut info = DataInfo {
            video: VideoInfo {
                width,
                height,
                kind: codec.ndl_type(),
                unknown1: 0,
            },
            audio: AudioUnion { bytes: [0; 32] },
        };
        let load_seq = LOAD_SEQ.load(Ordering::SeqCst);
        let error_seq = ERROR_SEQ.load(Ordering::SeqCst);
        let rc = unsafe { (l.load)(&mut info, Some(on_load_state)) };
        if rc != 0 {
            bail!(
                "NDL_DirectMediaLoad({} {width}x{height}) failed: {rc} {}",
                codec.name(),
                last_error(l)
            );
        }
        log::info!("NDL loaded {} {width}x{height}", codec.name());
        Ok(NdlPlayer {
            l,
            codec,
            width,
            height,
            loaded_at: Instant::now(),
            load_seq,
            error_seq,
            feeding: false,
            refused_since: None,
            frames: 0,
        })
    }

    /// True once the pipeline reported a fatal error or refused every frame for a while.
    pub fn is_dead(&self) -> bool {
        ERROR_SEQ.load(Ordering::SeqCst) != self.error_seq
            || self
                .refused_since
                .is_some_and(|t| t.elapsed() > DEAD_AFTER_REFUSED)
    }

    /// Whether the decoder has accepted at least one frame.
    pub fn presenting(&self) -> bool {
        self.frames > 0
    }

    /// Feed one complete access unit. `Ok(false)` = not ready yet, retry with the next
    /// keyframe; frames are not queued here to keep latency bounded.
    pub fn play(&mut self, data: &[u8]) -> Result<bool> {
        if !self.feeding {
            let completed = LOAD_SEQ.load(Ordering::SeqCst) != self.load_seq;
            if !completed && self.loaded_at.elapsed() < FEED_ANYWAY_AFTER {
                return Ok(false);
            }
            if !completed {
                log::warn!(
                    "NDL: no LOADCOMPLETED after {:?}, feeding anyway",
                    self.loaded_at.elapsed()
                );
            }
            self.feeding = true;
        }
        let pts_ms = self.loaded_at.elapsed().as_millis() as c_longlong;
        let len = c_uint::try_from(data.len()).map_err(|_| anyhow!("frame too large"))?;
        let rc = {
            let _g = FFI.lock().unwrap_or_else(|e| e.into_inner());
            unsafe { (self.l.video_play)(data.as_ptr() as *mut c_void, len, pts_ms) }
        };
        if rc != 0 {
            self.refused_since.get_or_insert_with(Instant::now);
            hbb_log_throttle(&format!(
                "NDL_DirectVideoPlay refused frame: {rc} {}",
                last_error(self.l)
            ));
            bail!("hardware decoder refused frame ({rc})");
        }
        self.refused_since = None;
        self.frames += 1;
        Ok(true)
    }

    pub fn flush(&self) {
        let _g = FFI.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { (self.l.flush)() };
    }

    /// Frames queued for presentation (latency indicator), if the query works.
    pub fn queue_depth(&self) -> Option<i32> {
        let mut n: c_int = 0;
        let _g = FFI.lock().unwrap_or_else(|e| e.into_inner());
        (unsafe { (self.l.render_len)(&mut n) } == 0).then_some(n)
    }
}

fn hbb_log_throttle(msg: &str) {
    use std::sync::atomic::AtomicU64;
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if LAST.swap(now, Ordering::Relaxed) != now {
        log::warn!("{msg}");
    }
}

impl Drop for NdlPlayer {
    fn drop(&mut self) {
        let _g = FFI.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { (self.l.unload)() };
        log::info!("NDL unloaded {} after {} frames", self.codec.name(), self.frames);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_userland_header() {
        assert_eq!(std::mem::size_of::<VideoInfo>(), 16);
        assert_eq!(std::mem::offset_of!(DataInfo, audio), 16);
        assert_eq!(std::mem::size_of::<DataInfo>(), 48);
    }

    #[test]
    fn missing_library_is_a_capability_failure() {
        // On a development machine the library is absent: must be an error, not a crash.
        if std::path::Path::new("/usr/lib/libNDL_directmedia.so.1").exists() {
            return;
        }
        assert!(available().is_err());
        assert!(NdlPlayer::load(HwCodec::H264, 1920, 1080).is_err());
    }
}
