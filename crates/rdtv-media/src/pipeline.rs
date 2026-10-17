//! Video decode pipeline: one thread, off the UI and network threads.
//!
//! * Hardware first: H.264/H.265/VP9/AV1 go to NDL when allowed and not marked failed.
//! * Software fallback: VP8/VP9 through libvpx; pictures land in a single-slot mailbox the
//!   renderer takes the newest picture from (presentation may drop, decoding never does,
//!   because VP8/VP9 frames are interdependent).
//! * Keyframe gating: after a (re)load, reset or a dropped compressed frame, frames are
//!   discarded until a keyframe arrives and a refresh is requested from the host (the
//!   upstream recovery mechanism, `Misc.refresh_video[_display]`).
//! * The network side never blocks: frames go through a bounded queue; overflow drops the
//!   frame, flags a resync and asks for a keyframe (upstream `VIDEO_QUEUE_SIZE` behaviour).

use crate::{frame::YuvFrame, ndl, smp, vpx::VpxDecoder};
use rdtv_core::message_proto::{video_frame, EncodedVideoFrame, VideoFrame};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub const QUEUE_LEN: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Codec {
    Vp8,
    Vp9,
    H264,
    H265,
    Av1,
}

impl Codec {
    pub fn name(self) -> &'static str {
        match self {
            Codec::Vp8 => "VP8",
            Codec::Vp9 => "VP9",
            Codec::H264 => "H.264",
            Codec::H265 => "H.265",
            Codec::Av1 => "AV1",
        }
    }
    pub fn hw(self) -> Option<ndl::HwCodec> {
        match self {
            Codec::Vp8 => None,
            Codec::Vp9 => Some(ndl::HwCodec::Vp9),
            Codec::H264 => Some(ndl::HwCodec::H264),
            Codec::H265 => Some(ndl::HwCodec::H265),
            Codec::Av1 => Some(ndl::HwCodec::Av1),
        }
    }
}

fn split(vf: VideoFrame) -> Option<(Codec, Vec<EncodedVideoFrame>)> {
    match vf.union {
        Some(video_frame::Union::Vp9s(f)) => Some((Codec::Vp9, f.frames)),
        Some(video_frame::Union::Vp8s(f)) => Some((Codec::Vp8, f.frames)),
        Some(video_frame::Union::H264s(f)) => Some((Codec::H264, f.frames)),
        Some(video_frame::Union::H265s(f)) => Some((Codec::H265, f.frames)),
        Some(video_frame::Union::Av1s(f)) => Some((Codec::Av1, f.frames)),
        _ => None,
    }
}

pub fn contains_key_frame(vf: &VideoFrame) -> bool {
    match &vf.union {
        Some(video_frame::Union::Vp9s(f))
        | Some(video_frame::Union::Vp8s(f))
        | Some(video_frame::Union::H264s(f))
        | Some(video_frame::Union::H265s(f))
        | Some(video_frame::Union::Av1s(f)) => f.frames.iter().any(|x| x.key),
        _ => false,
    }
}

#[derive(Debug, Clone)]
pub enum PipelineEvent {
    /// Started presenting with this backend.
    Presenting {
        codec: Codec,
        hardware: bool,
        width: i32,
        height: i32,
    },
    /// Ask the host for a keyframe on this display.
    NeedKeyframe(i32),
    /// This codec cannot be decoded here (any more): re-advertise without it.
    CodecFailed { codec: Codec, reason: String },
}

#[derive(Debug, Clone, Copy)]
pub struct PipelineConfig {
    /// Hardware decoding allowed (user setting and NDL present).
    pub hardware: bool,
    /// VP9 to hardware (else libvpx).
    pub vp9_hardware: bool,
}

#[derive(Default)]
pub struct Stats {
    pub received: AtomicU64,
    pub decoded: AtomicU64,
    pub dropped: AtomicU64,
}

enum Msg {
    Frame(VideoFrame),
    StreamSize(i32, i32),
    Reset,
    Config(PipelineConfig),
    Stop,
}

/// The newest decoded software picture.
pub type Mailbox = Arc<Mutex<Option<YuvFrame>>>;

pub struct VideoPipeline {
    tx: SyncSender<Msg>,
    resync: Arc<AtomicBool>,
    pub mailbox: Mailbox,
    pub stats: Arc<Stats>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl VideoPipeline {
    pub fn start(
        config: PipelineConfig,
        on_event: Arc<dyn Fn(PipelineEvent) + Send + Sync>,
    ) -> VideoPipeline {
        let (tx, rx) = sync_channel(QUEUE_LEN);
        let resync = Arc::new(AtomicBool::new(true));
        let mailbox: Mailbox = Arc::new(Mutex::new(None));
        let stats = Arc::new(Stats::default());
        let w = Worker {
            config,
            rx,
            resync: resync.clone(),
            mailbox: mailbox.clone(),
            stats: stats.clone(),
            on_event,
            hw: None,
            sw: None,
            size: (0, 0),
            display: 0,
            failed: Vec::new(),
            last_key_request: None,
            presenting: None,
            consecutive_errors: 0,
        };
        let thread = std::thread::Builder::new()
            .name("rdtv-video".into())
            .spawn(move || w.run())
            .ok();
        VideoPipeline {
            tx,
            resync,
            mailbox,
            stats,
            thread,
        }
    }

    /// Called from the network runtime; never blocks.
    pub fn push(&self, vf: VideoFrame) {
        self.stats.received.fetch_add(1, Ordering::Relaxed);
        match self.tx.try_send(Msg::Frame(vf)) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.stats.dropped.fetch_add(1, Ordering::Relaxed);
                self.resync.store(true, Ordering::SeqCst);
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }

    /// The selected monitor's pixel size (needed to load the hardware decoder).
    pub fn set_stream_size(&self, w: i32, h: i32) {
        let _ = self.tx.try_send(Msg::StreamSize(w, h));
    }

    /// Drop decoder state (monitor switch, reconnect) and wait for a keyframe.
    pub fn reset(&self) {
        self.resync.store(true, Ordering::SeqCst);
        let _ = self.tx.try_send(Msg::Reset);
    }

    pub fn set_config(&self, c: PipelineConfig) {
        let _ = self.tx.try_send(Msg::Config(c));
    }
}

impl Drop for VideoPipeline {
    fn drop(&mut self) {
        let _ = self.tx.try_send(Msg::Stop);
        if let Some(t) = self.thread.take() {
            // The worker only blocks in NDL/libvpx calls; give it a bounded time.
            let deadline = Instant::now() + Duration::from_secs(3);
            while !t.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if t.is_finished() {
                let _ = t.join();
            } else {
                log::error!("video thread did not stop within 3 s; detaching it");
            }
        }
    }
}

/// The hardware decoder in use: Starfish into the app's exported window when the app
/// created one (the picture can then be placed and scaled), else NDL full-screen.
enum HwPlayer {
    Smp(smp::SmpPlayer),
    Ndl(ndl::NdlPlayer),
}

impl HwPlayer {
    fn load(codec: ndl::HwCodec, w: i32, h: i32) -> anyhow::Result<HwPlayer> {
        if ndl::target_window().is_some() {
            match smp::SmpPlayer::load(codec, w, h) {
                Ok(p) => {
                    ndl::set_window_state(
                        true,
                        format!("Starfish decoder in exported window ({})", codec.name()),
                    );
                    return Ok(HwPlayer::Smp(p));
                }
                Err(e) => {
                    log::warn!("Starfish unavailable, using NDL: {e:#}");
                    ndl::set_window_state(false, format!("Starfish failed ({e:#}); NDL full-screen"));
                }
            }
        } else {
            ndl::set_window_state(false, "no exported window; NDL full-screen".into());
        }
        ndl::NdlPlayer::load(codec, w, h).map(HwPlayer::Ndl)
    }
    fn codec(&self) -> ndl::HwCodec {
        match self {
            HwPlayer::Smp(p) => p.codec,
            HwPlayer::Ndl(p) => p.codec,
        }
    }
    fn width(&self) -> i32 {
        match self {
            HwPlayer::Smp(p) => p.width,
            HwPlayer::Ndl(p) => p.width,
        }
    }
    fn height(&self) -> i32 {
        match self {
            HwPlayer::Smp(p) => p.height,
            HwPlayer::Ndl(p) => p.height,
        }
    }
    fn play(&mut self, data: &[u8]) -> anyhow::Result<bool> {
        match self {
            HwPlayer::Smp(p) => p.play(data),
            HwPlayer::Ndl(p) => p.play(data),
        }
    }
    fn is_dead(&self) -> bool {
        match self {
            HwPlayer::Smp(p) => p.is_dead(),
            HwPlayer::Ndl(p) => p.is_dead(),
        }
    }
    fn presenting(&self) -> bool {
        match self {
            HwPlayer::Smp(p) => p.presenting(),
            HwPlayer::Ndl(p) => p.presenting(),
        }
    }
}

struct Worker {
    config: PipelineConfig,
    rx: Receiver<Msg>,
    resync: Arc<AtomicBool>,
    mailbox: Mailbox,
    stats: Arc<Stats>,
    on_event: Arc<dyn Fn(PipelineEvent) + Send + Sync>,
    hw: Option<HwPlayer>,
    sw: Option<VpxDecoder>,
    size: (i32, i32),
    display: i32,
    failed: Vec<Codec>,
    last_key_request: Option<Instant>,
    presenting: Option<(Codec, bool)>,
    consecutive_errors: u32,
}

impl Worker {
    fn run(mut self) {
        while let Ok(msg) = self.rx.recv() {
            match msg {
                Msg::Frame(vf) => self.frame(vf),
                Msg::StreamSize(w, h) => {
                    if (w, h) != self.size {
                        self.size = (w, h);
                        // A hardware pipeline is loaded for one size; reload on change.
                        if self.hw.is_some() {
                            self.hw = None;
                            self.presenting = None;
                            self.resync.store(true, Ordering::SeqCst);
                        }
                    }
                }
                Msg::Reset => {
                    self.hw = None;
                    self.sw = None;
                    self.presenting = None;
                    self.resync.store(true, Ordering::SeqCst);
                    if let Ok(mut m) = self.mailbox.lock() {
                        *m = None;
                    }
                }
                Msg::Config(c) => {
                    self.config = c;
                    self.hw = None;
                    self.sw = None;
                    self.presenting = None;
                    self.resync.store(true, Ordering::SeqCst);
                }
                Msg::Stop => break,
            }
        }
        self.hw = None;
    }

    fn request_key(&mut self) {
        let now = Instant::now();
        if self
            .last_key_request
            .is_none_or(|t| now.duration_since(t) > Duration::from_millis(1000))
        {
            self.last_key_request = Some(now);
            (self.on_event)(PipelineEvent::NeedKeyframe(self.display));
        }
    }

    fn fail(&mut self, codec: Codec, reason: String) {
        log::warn!("{} unusable: {reason}", codec.name());
        if !self.failed.contains(&codec) {
            self.failed.push(codec);
            (self.on_event)(PipelineEvent::CodecFailed { codec, reason });
        }
        self.hw = None;
        self.presenting = None;
        self.resync.store(true, Ordering::SeqCst);
    }

    fn use_hardware(&self, codec: Codec) -> bool {
        self.config.hardware
            && codec.hw().is_some()
            && (codec != Codec::Vp9 || self.config.vp9_hardware)
            && !self.failed.contains(&codec)
    }

    fn frame(&mut self, vf: VideoFrame) {
        self.display = vf.display;
        if self.resync.load(Ordering::SeqCst) {
            if !contains_key_frame(&vf) {
                self.request_key();
                return;
            }
            self.resync.store(false, Ordering::SeqCst);
        }
        let Some((codec, frames)) = split(vf) else {
            return;
        };
        if self.use_hardware(codec) {
            self.decode_hw(codec, frames);
        } else if matches!(codec, Codec::Vp8 | Codec::Vp9) {
            self.decode_sw(codec, frames);
        } else {
            // H.264/H.265/AV1 without a working hardware decoder: no software path.
            self.fail(codec, "no decoder available on this TV".into());
        }
    }

    fn decode_hw(&mut self, codec: Codec, frames: Vec<EncodedVideoFrame>) {
        let hwc = codec.hw().expect("hardware codec");
        let needs_load = self.hw.as_ref().is_none_or(|p| p.codec() != hwc);
        if needs_load {
            self.hw = None;
            self.sw = None;
            if let Ok(mut m) = self.mailbox.lock() {
                *m = None;
            }
            let (w, h) = if self.size.0 > 0 { self.size } else { (1920, 1080) };
            match HwPlayer::load(hwc, w, h) {
                Ok(p) => self.hw = Some(p),
                Err(e) => return self.fail(codec, e.to_string()),
            }
        }
        for f in frames {
            let Some(p) = self.hw.as_mut() else { return };
            match p.play(&f.data) {
                Ok(true) => {
                    self.stats.decoded.fetch_add(1, Ordering::Relaxed);
                    self.consecutive_errors = 0;
                    if self.presenting != Some((codec, true)) {
                        self.presenting = Some((codec, true));
                        (self.on_event)(PipelineEvent::Presenting {
                            codec,
                            hardware: true,
                            width: p.width(),
                            height: p.height(),
                        });
                    }
                }
                Ok(false) => {
                    // Not ready: wait for the next keyframe to start cleanly.
                    self.resync.store(true, Ordering::SeqCst);
                    self.request_key();
                    return;
                }
                Err(e) => {
                    self.consecutive_errors += 1;
                    if p.is_dead() || (!p.presenting() && self.consecutive_errors > 30) {
                        return self.fail(codec, e.to_string());
                    }
                    self.resync.store(true, Ordering::SeqCst);
                    self.request_key();
                    return;
                }
            }
        }
        if self.hw.as_ref().is_some_and(|p| p.is_dead()) {
            self.fail(codec, "hardware decoder reported a fatal error".into());
        }
    }

    fn decode_sw(&mut self, codec: Codec, frames: Vec<EncodedVideoFrame>) {
        let vp9 = codec == Codec::Vp9;
        if self.sw.as_ref().is_none_or(|d| d.vp9 != vp9) {
            self.hw = None;
            match VpxDecoder::new(vp9) {
                Ok(d) => self.sw = Some(d),
                Err(e) => return self.fail(codec, e.to_string()),
            }
        }
        for f in frames {
            let Some(d) = self.sw.as_mut() else { return };
            match d.decode(&f.data) {
                Ok(Some(pic)) => {
                    self.stats.decoded.fetch_add(1, Ordering::Relaxed);
                    self.consecutive_errors = 0;
                    let (w, h) = (pic.width as i32, pic.height as i32);
                    if let Ok(mut m) = self.mailbox.lock() {
                        *m = Some(pic);
                    }
                    if self.presenting != Some((codec, false)) {
                        self.presenting = Some((codec, false));
                        (self.on_event)(PipelineEvent::Presenting {
                            codec,
                            hardware: false,
                            width: w,
                            height: h,
                        });
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    log::warn!("software decode error: {e}");
                    self.consecutive_errors += 1;
                    self.sw = None;
                    if self.consecutive_errors > 10 {
                        return self.fail(codec, e.to_string());
                    }
                    self.resync.store(true, Ordering::SeqCst);
                    self.request_key();
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdtv_core::message_proto::EncodedVideoFrames;
    use std::sync::Mutex as StdMutex;

    fn vf(codec: Codec, key: bool) -> VideoFrame {
        let frames = EncodedVideoFrames {
            frames: vec![EncodedVideoFrame {
                data: vec![0u8; 16].into(),
                key,
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut v = VideoFrame::new();
        match codec {
            Codec::H265 => v.set_h265s(frames),
            Codec::Vp9 => v.set_vp9s(frames),
            _ => v.set_h264s(frames),
        }
        v
    }

    type Events = Arc<StdMutex<Vec<PipelineEvent>>>;
    type Callback = Arc<dyn Fn(PipelineEvent) + Send + Sync>;

    fn collect() -> (Events, Callback) {
        let ev = Arc::new(StdMutex::new(Vec::new()));
        let e2 = ev.clone();
        (ev, Arc::new(move |e| e2.lock().unwrap().push(e)))
    }

    #[test]
    fn waits_for_keyframe_and_requests_one() {
        let (ev, cb) = collect();
        let p = VideoPipeline::start(
            PipelineConfig {
                hardware: false,
                vp9_hardware: false,
            },
            cb,
        );
        p.push(vf(Codec::Vp9, false));
        std::thread::sleep(Duration::from_millis(100));
        let evs = ev.lock().unwrap().clone();
        assert!(matches!(evs.first(), Some(PipelineEvent::NeedKeyframe(_))));
    }

    #[test]
    fn hevc_without_hardware_reports_codec_failure() {
        let (ev, cb) = collect();
        let p = VideoPipeline::start(
            PipelineConfig {
                hardware: true,
                vp9_hardware: true,
            },
            cb,
        );
        p.set_stream_size(1920, 1080);
        p.push(vf(Codec::H265, true));
        std::thread::sleep(Duration::from_millis(200));
        let evs = ev.lock().unwrap().clone();
        assert!(evs.iter().any(|e| matches!(
            e,
            PipelineEvent::CodecFailed {
                codec: Codec::H265,
                ..
            }
        )));
    }
}
