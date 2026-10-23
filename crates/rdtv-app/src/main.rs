//! RemoteDesk TV: RustDesk-compatible remote desktop client for LG webOS TVs.

mod app;
mod audio_out;
mod gfx;
mod hwplane;
mod keys;
mod osk;
mod platform;
mod screens;
mod session_view;
mod ui;
mod updates;
mod view;

use std::time::{Duration, Instant};

/// Loaded SDL version and keyboard layout, shown in Diagnostics.
pub static SDL_DESC: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn main() {
    if std::env::args().any(|a| a == "--self-test") {
        std::process::exit(self_test());
    }
    if let Err(e) = run() {
        log::error!("fatal: {e:#}");
        eprintln!("fatal: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let data = platform::data_dir();
    let log_path = platform::init_logging(&data);
    log::info!(
        "RemoteDesk TV {} starting (args: {} launch params)",
        updates::current_version(),
        std::env::args().count().saturating_sub(1)
    );
    platform::confine_upstream_config(&data);
    std::panic::set_hook(Box::new(|info| {
        log::error!("panic: {info}");
    }));

    platform::set_sdl_hints();
    let sdl = sdl2::init().map_err(anyhow::Error::msg)?;
    let video = sdl.video().map_err(anyhow::Error::msg)?;
    log::info!("SDL video driver: {}", video.current_video_driver());
    let (_, sdl_desc) = keys::init_key_abi();
    log::info!("{sdl_desc}");
    let _ = SDL_DESC.set(sdl_desc);
    let (w, h) = match video.current_display_mode(0) {
        Ok(m) if m.w > 0 && m.h > 0 => (m.w as u32, m.h as u32),
        _ => (1920, 1080),
    };
    let mut wb = video.window("RemoteDesk TV", w, h);
    if keys::is_webos() || std::env::var("RDTV_FULLSCREEN").is_ok() {
        wb.fullscreen();
    }
    if keys::is_webos() {
        // Exported video windows need an OpenGL window; creating it so up front avoids
        // SDL-webOS recreating the window (and invalidating the renderer) later.
        wb.opengl();
    } else {
        wb.resizable();
    }
    let window = wb.build()?;
    let canvas = match window.clone().into_canvas().accelerated().present_vsync().build() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("accelerated renderer unavailable ({e}); using software rendering");
            window.into_canvas().software().build()?
        }
    };
    log::info!("renderer: {:?}", canvas.info().name);
    let mut g = gfx::Gfx::new(canvas)?;

    let store = rdtv_core::store::Store::open(&data);
    g.ui_scale = store.state.settings.ui_scale;
    let mut app = app::App::new(sdl.clone(), store, log_path);
    let mut inputs = Vec::new();
    let frame = Duration::from_millis(16);
    while !app.quit {
        let t0 = Instant::now();
        inputs.clear();
        keys::poll(&mut inputs);
        app.update(std::mem::take(&mut inputs));
        g.ui_scale = app.store.state.settings.ui_scale;
        app.draw(&mut g);
        g.canvas.present();
        // present_vsync paces us; this guards renderers without vsync.
        let el = t0.elapsed();
        if el < frame / 2 {
            std::thread::sleep(frame / 2 - el);
        }
    }
    app.store.save();
    rdtv_media::ndl::quit();
    log::info!("exit");
    Ok(())
}

/// Non-graphical self test (used by CI under qemu-arm and over SSH on a TV): exercises the
/// statically linked decoders, crypto, storage and the NDL probe. Exit code 0 = all passed.
fn self_test() -> i32 {
    let mut ok = true;
    let mut check = |name: &str, r: Result<String, String>| match &r {
        Ok(d) => println!("PASS {name}: {d}"),
        Err(e) => {
            println!("FAIL {name}: {e}");
            ok = false;
        }
    };
    check(
        "libvpx vp9",
        rdtv_media::vpx::VpxDecoder::new(true)
            .map(|_| "decoder created".into())
            .map_err(|e| e.to_string()),
    );
    check(
        "libvpx vp8",
        rdtv_media::vpx::VpxDecoder::new(false)
            .map(|_| "decoder created".into())
            .map_err(|e| e.to_string()),
    );
    check(
        "libopus",
        rdtv_media::opus::OpusDecoder::new(48000, 2)
            .map(|_| "decoder created".into())
            .map_err(|e| e.to_string()),
    );
    check("sodium", {
        use rdtv_core::hbb_common::sodiumoxide::crypto::box_;
        let (pk, _) = box_::gen_keypair();
        Ok(format!("keypair {} bytes", pk.0.len()))
    });
    check(
        "server key",
        rdtv_core::transport::get_rs_pk(rdtv_core::hbb_common::config::RS_PUB_KEY)
            .map(|_| "public server key decodes".into())
            .ok_or_else(|| "bad key".into()),
    );
    let dir = std::env::temp_dir().join(format!("rdtv-selftest-{}", std::process::id()));
    check("store", {
        let s = rdtv_core::store::Store::open(&dir);
        s.save();
        let r = if dir.join("state.json").exists() {
            Ok(format!("client id {}", s.state.client_id.len()))
        } else {
            Err("state not written".into())
        };
        let _ = std::fs::remove_dir_all(&dir);
        r
    });
    let p = rdtv_media::caps::detect();
    println!("INFO webOS release: {:?}", p.webos_release);
    println!(
        "INFO NDL: {}",
        p.ndl_error.clone().unwrap_or_else(|| "available".into())
    );
    let caps = rdtv_media::caps::decode_caps(&p, rdtv_media::caps::HwPolicy { hardware: true }, &[]);
    println!("INFO advertised codecs: {caps:?}");
    if ok {
        0
    } else {
        1
    }
}
