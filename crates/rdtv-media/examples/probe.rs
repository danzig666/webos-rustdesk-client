//! Headless interoperability probe: connect to a stock RustDesk host, log in, decode video
//! with the real pipeline, send input, report what was negotiated.
//!
//!   cargo run -p rdtv-media --example probe -- <id-or-address> <password> [seconds]

use rdtv_core::{
    hbb_common::tokio,
    input,
    profile::ServerProfile,
    session::{self, MediaSink, SessionCommand, SessionEvent, SessionRequest},
    store::PeerSettings,
    transport::Target,
};
use rdtv_media::{
    audio::AudioRing,
    caps,
    pipeline::{PipelineConfig, PipelineEvent, VideoPipeline},
};
use std::sync::{atomic::Ordering, Arc, Mutex};

struct Sink {
    video: Arc<VideoPipeline>,
    audio: Arc<AudioRing>,
}

impl MediaSink for Sink {
    fn video(&self, _g: u64, f: rdtv_core::message_proto::VideoFrame) {
        self.video.push(f);
    }
    fn audio_format(&self, _g: u64, f: rdtv_core::message_proto::AudioFormat) {
        println!("audio format: {} Hz x{}", f.sample_rate, f.channels);
        self.audio.set_format(f.sample_rate, f.channels);
    }
    fn audio(&self, _g: u64, f: rdtv_core::message_proto::AudioFrame) {
        self.audio.push_packet(&f.data);
    }
}

fn main() {
    let _ = rdtv_core::hbb_common::env_logger::builder()
        .filter_level(rdtv_core::hbb_common::log::LevelFilter::Info)
        .try_init();
    let args: Vec<String> = std::env::args().collect();
    let target = Target::parse(args.get(1).map(|s| s.as_str()).unwrap_or("127.0.0.1")).expect("target");
    let password = args.get(2).cloned().unwrap_or_default();
    let secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(8);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let platform = caps::detect();
    let policy = caps::HwPolicy { hardware: true };
    let dcaps = caps::decode_caps(&platform, policy, &[]);
    println!("platform: {:?}, advertising {:?}", platform, dcaps);

    let handle_slot: Arc<Mutex<Option<session::SessionHandle>>> = Arc::new(Mutex::new(None));
    let hs = handle_slot.clone();
    let pipe = Arc::new(VideoPipeline::start(
        PipelineConfig {
            hardware: platform.ndl_available,
            vp9_hardware: caps::vp9_in_hardware(&platform, policy, &[]),
        },
        Arc::new(move |e| {
            println!("pipeline: {e:?}");
            if let PipelineEvent::NeedKeyframe(d) = e {
                if let Some(h) = hs.lock().unwrap().as_ref() {
                    h.send(SessionCommand::RefreshVideo(d));
                }
            }
        }),
    ));
    let audio = Arc::new(AudioRing::new(200));
    let sink = Arc::new(Sink {
        video: pipe.clone(),
        audio: audio.clone(),
    });
    let connected = Arc::new(Mutex::new(None));
    let c2 = connected.clone();
    let p2 = pipe.clone();
    let req = SessionRequest {
        profile: ServerProfile::public(),
        target,
        password: Some(password),
        remembered_hash: None,
        remember: true,
        settings: PeerSettings::default(),
        caps: dcaps,
        audio_enabled: true,
        client_id: "123123123".into(),
        client_name: "RemoteDesk TV probe".into(),
        force_relay: false,
    };
    let h = session::start(
        rt.handle(),
        req,
        sink,
        Arc::new(move |gen, e| match &e {
            SessionEvent::Connected {
                peer, route, secured, ..
            } => {
                println!("CONNECTED gen={gen} host={} platform={} version={} route={} secured={secured} displays={:?}",
                    peer.hostname, peer.platform, peer.version, route.label(),
                    peer.displays.iter().map(|d| (d.index, d.x, d.y, d.width, d.height, d.name.clone())).collect::<Vec<_>>());
                if let Some(d) = peer.displays.get(peer.current_display as usize) {
                    p2.set_stream_size(d.width, d.height);
                }
                *c2.lock().unwrap() = Some(peer.clone());
            }
            SessionEvent::Cursor(c) => println!("cursor {}x{} id={}", c.width, c.height, c.id),
            other => println!("event: {other:?}"),
        }),
    );
    *handle_slot.lock().unwrap() = Some(h.clone());
    let t0 = std::time::Instant::now();
    let mut sent_input = false;
    while t0.elapsed().as_secs() < secs {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if !sent_input && connected.lock().unwrap().is_some() && t0.elapsed().as_secs() >= 3 {
            sent_input = true;
            let mut msgs = vec![
                input::mouse_move(1200, 650),
                input::mouse_button(input::MouseButton::Left, true, 1200, 650, Default::default()),
                input::mouse_button(input::MouseButton::Left, false, 1200, 650, Default::default()),
            ];
            msgs.push(input::text(
                "echo 'Hello áéíóöőúüű ÁÉÍÓÖŐÚÜŰ' > /tmp/rdtv_typed.txt",
            ));
            msgs.push(input::control_key(
                rdtv_core::message_proto::ControlKey::Return,
                true,
                true,
                Default::default(),
            ));
            h.send(SessionCommand::SendMany(msgs));
            println!("sent input");
        }
    }
    let s = &pipe.stats;
    println!(
        "STATS received={} decoded={} dropped={} audio_queued_ms={}",
        s.received.load(Ordering::Relaxed),
        s.decoded.load(Ordering::Relaxed),
        s.dropped.load(Ordering::Relaxed),
        audio.queued_ms()
    );
    if let Some(f) = pipe.mailbox.lock().unwrap().as_ref() {
        println!("last picture {}x{}", f.width, f.height);
    }
    h.send(SessionCommand::Disconnect { lock: false });
    std::thread::sleep(std::time::Duration::from_millis(500));
}
