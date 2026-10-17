//! RemoteDesk TV: RustDesk-compatible remote desktop client for LG webOS TVs.

mod app;
mod audio_out;
mod gfx;
mod keys;
mod osk;
mod platform;
mod screens;
mod session_view;
mod ui;
mod view;

use std::time::{Duration, Instant};

fn main() {
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
        env!("CARGO_PKG_VERSION"),
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
    let (w, h) = match video.current_display_mode(0) {
        Ok(m) if m.w > 0 && m.h > 0 => (m.w as u32, m.h as u32),
        _ => (1920, 1080),
    };
    let mut wb = video.window("RemoteDesk TV", w, h);
    if keys::is_webos() || std::env::var("RDTV_FULLSCREEN").is_ok() {
        wb.fullscreen();
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
