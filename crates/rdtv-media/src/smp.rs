//! LG Starfish media pipeline (`libplayerAPIs`) hardware video decoding into the app's exported
//! punch-through window.
//!
//! NDL DirectMedia always presents full-screen. Starfish takes the exported window id in its
//! Load payload (`option.windowId`), after which the compositor shows the picture wherever
//! `SDL_webOSSetExportedWindow` puts it: this is how Cobalt (YouTube) shrinks its video. The
//! payload follows mariotaku/ss4s `modules/webos/smp` (`MakeLoadPayload`, webOS 5 resource),
//! the C++ calling details plx-native `src/starfish.c`; see `csrc/smp_shim.c` and
//! docs/SOURCES.md.

use crate::ndl::{self, HwCodec};
use anyhow::{anyhow, bail, Result};
use std::{
    ffi::{c_char, c_int, c_uint, c_ulonglong, CStr, CString},
    time::{Duration, Instant},
};

#[repr(C)]
struct RawSmp {
    _p: [u8; 0],
}

extern "C" {
    fn rdtv_smp_available() -> *const c_char;
    fn rdtv_smp_load(payload: *const c_char) -> *mut RawSmp;
    fn rdtv_smp_load_completed(p: *const RawSmp) -> c_int;
    fn rdtv_smp_failed(p: *const RawSmp) -> c_int;
    fn rdtv_smp_feed(p: *mut RawSmp, data: *const u8, size: c_uint, pts_ns: c_ulonglong) -> c_char;
    fn rdtv_smp_unload(p: *mut RawSmp);
}

/// `Ok` when the Starfish library and the symbols this backend needs are present.
pub fn available() -> Result<()> {
    let e = unsafe { rdtv_smp_available() };
    if e.is_null() {
        Ok(())
    } else {
        Err(anyhow!(unsafe { CStr::from_ptr(e) }
            .to_string_lossy()
            .into_owned()))
    }
}

fn codec_name(c: HwCodec) -> &'static str {
    match c {
        HwCodec::H264 => "H264",
        HwCodec::H265 => "H265",
        HwCodec::Vp9 => "VP9",
        HwCodec::Av1 => "AV1",
    }
}

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

pub(crate) fn load_payload(codec: HwCodec, w: i32, h: i32, app_id: &str, window: &str) -> String {
    format!(
        concat!(
            r#"{{"args":[{{"mediaTransportType":"BUFFERSTREAM","option":{{"#,
            r#""appId":{app},"#,
            r#""externalStreamingInfo":{{"contents":{{"codec":{{"video":"{codec}"}},"#,
            r#""esInfo":{{"pauseAtDecodeTime":true,"ptsToDecode":0,"seperatedPTS":true}},"#,
            r#""format":"RAW","provider":"Chrome"}},"#,
            r#""streamQualityInfo":true,"audioSync":false,"streamQualityInfoCorruptedFrame":true,"#,
            r#""streamQualityInfoNonFlushable":true,"restartStreaming":false,"#,
            r#""bufferingCtrInfo":{{"bufferMaxLevel":0,"bufferMinLevel":0,"preBufferByte":0,"#,
            r#""qBufferLevelAudio":0,"qBufferLevelVideo":0,"#,
            r#""srcBufferLevelAudio":{{"minimum":1,"maximum":32768}},"#,
            r#""srcBufferLevelVideo":{{"minimum":1,"maximum":1048576}}}}}},"#,
            r#""transmission":{{"contentsType":"WEBRTC"}},"#,
            r#""needAudio":false,"queryPosition":false,"lowDelayMode":true,"#,
            r#""adaptiveStreaming":{{"audioOnly":false,"maxWidth":{w},"maxHeight":{h},"maxFrameRate":60.0}},"#,
            r#""windowId":{win}}}}}]}}"#
        ),
        app = json_str(app_id),
        codec = codec_name(codec),
        w = w,
        h = h,
        win = json_str(window),
    )
}

/// One loaded Starfish pipeline (one codec, one stream size), bound to the exported window.
pub struct SmpPlayer {
    raw: *mut RawSmp,
    pub codec: HwCodec,
    pub width: i32,
    pub height: i32,
    loaded_at: Instant,
    feeding: bool,
    refused_since: Option<Instant>,
    frames: u64,
}

// The library is thread-safe for one caller at a time; the player lives on the video thread.
unsafe impl Send for SmpPlayer {}

const WAIT_FOR_LOAD: Duration = Duration::from_millis(1500);
const DEAD_AFTER_REFUSED: Duration = Duration::from_secs(3);

impl SmpPlayer {
    pub fn load(codec: HwCodec, width: i32, height: i32) -> Result<SmpPlayer> {
        available()?;
        if width <= 0 || height <= 0 || width > 8192 || height > 8192 {
            bail!("invalid stream size {width}x{height}");
        }
        let window = ndl::target_window().ok_or_else(|| anyhow!("no exported window"))?;
        let window = window.to_string_lossy().into_owned();
        let app_id = std::env::var("APPID").unwrap_or_else(|_| crate::APP_ID.to_owned());
        let payload = load_payload(codec, width, height, &app_id, &window);
        log::info!("Starfish Load {payload}");
        let c = CString::new(payload)?;
        let raw = unsafe { rdtv_smp_load(c.as_ptr()) };
        if raw.is_null() {
            bail!("Starfish Load({} {width}x{height}) refused", codec.name());
        }
        Ok(SmpPlayer {
            raw,
            codec,
            width,
            height,
            loaded_at: Instant::now(),
            feeding: false,
            refused_since: None,
            frames: 0,
        })
    }

    pub fn is_dead(&self) -> bool {
        (unsafe { rdtv_smp_failed(self.raw) != 0 })
            || self
                .refused_since
                .is_some_and(|t| t.elapsed() > DEAD_AFTER_REFUSED)
    }

    pub fn presenting(&self) -> bool {
        self.frames > 0
    }

    /// Feed one access unit. `Ok(false)` = not ready yet, retry with the next keyframe.
    pub fn play(&mut self, data: &[u8]) -> Result<bool> {
        if !self.feeding {
            let completed = unsafe { rdtv_smp_load_completed(self.raw) } != 0;
            if !completed && self.loaded_at.elapsed() < WAIT_FOR_LOAD {
                return Ok(false);
            }
            if !completed {
                log::warn!(
                    "Starfish: no LOADCOMPLETED after {:?}, feeding anyway",
                    self.loaded_at.elapsed()
                );
            }
            self.feeding = true;
        }
        let len = c_uint::try_from(data.len()).map_err(|_| anyhow!("frame too large"))?;
        let pts = self.loaded_at.elapsed().as_nanos() as c_ulonglong;
        // A full buffer drains within a frame or two; give it a bounded moment.
        for _ in 0..40 {
            match unsafe { rdtv_smp_feed(self.raw, data.as_ptr(), len, pts) } as u8 {
                b'O' => {
                    self.refused_since = None;
                    self.frames += 1;
                    return Ok(true);
                }
                b'B' => std::thread::sleep(Duration::from_millis(3)),
                _ => break,
            }
        }
        self.refused_since.get_or_insert_with(Instant::now);
        bail!("Starfish refused frame")
    }
}

impl Drop for SmpPlayer {
    fn drop(&mut self) {
        unsafe { rdtv_smp_unload(self.raw) };
        log::info!(
            "Starfish unloaded {} after {} frames",
            self.codec.name(),
            self.frames
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_carries_window_and_codec() {
        let p = load_payload(HwCodec::H265, 1920, 1080, "app.id", "_Window_Id_1");
        assert!(p.contains(r#""windowId":"_Window_Id_1""#));
        assert!(p.contains(r#""codec":{"video":"H265"}"#));
        assert!(p.contains(r#""lowDelayMode":true"#));
        // Balanced braces/brackets (valid JSON shape).
        let opens = p.matches('{').count();
        assert_eq!(opens, p.matches('}').count());
        assert_eq!(p.matches('[').count(), p.matches(']').count());
    }

    #[test]
    fn missing_library_is_a_capability_failure() {
        if std::path::Path::new("/usr/lib/libplayerAPIs.so.1").exists() {
            return;
        }
        assert!(available().is_err());
    }
}
