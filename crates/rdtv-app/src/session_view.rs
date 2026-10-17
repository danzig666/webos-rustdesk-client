//! The remote desktop session: video, input routing, toolbar, on-screen keyboard.
//!
//! Remote-button contract (owner decision) while the desktop has input focus:
//! Magic Remote pointer = absolute pointing; OK = left click (D-pad mouse mode) or Enter
//! (arrow mode); D-pad = virtual mouse or arrow keys; Yellow = toggle mouse/arrow mode;
//! Green = next monitor; Red = right click; Blue = keyboard; short Back = close the top
//! local panel, else show/hide the toolbar (never Escape, never disconnect); wheel =
//! scroll. Home/long Back stay with the TV. Overlays own their input: colour shortcuts
//! and clicks never leak through an open menu or keyboard to the remote desktop.

use crate::{
    app::{App, AppMsg, Screen},
    gfx::{size, theme, Gfx, H, W},
    keys::{physical_to_remote, In, KeyMods, PtrButton, Remote, RemoteKey},
    osk::{Mode as OskMode, Osk, OskAction},
    ui::{id, idx, Frame, Nav, UiIn, UiState},
    view::{self, Monitor, RectF, ViewState},
};
use rdtv_core::{
    input::{self, HeldInput, MouseButton, Mods},
    message_proto::{AudioFormat, AudioFrame, ControlKey, VideoFrame},
    session::{self, DecodeCaps, DisplayDesc, MediaSink, PeerSummary, SessionCommand, SessionEvent, SessionHandle},
    store::{default_shortcuts, DpadMode, OskPresentation, PeerSettings, Quality, Store, ViewMode},
    transport::Route,
};
use rdtv_media::{
    audio::AudioRing,
    pipeline::{Codec, PipelineConfig, PipelineEvent, VideoPipeline},
};
use sdl2::{
    keyboard::Keycode,
    pixels::{Color, PixelFormatEnum},
    rect::Rect,
    render::Texture,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const TOOLBAR_HIDE: Duration = Duration::from_secs(5);
const TOOLBAR_H: i32 = 104;
const KEYBOARD_ACTIVE: Duration = Duration::from_secs(10);

struct Sink {
    pipe: Arc<VideoPipeline>,
    ring: Arc<AudioRing>,
    paused: Arc<AtomicBool>,
}

impl MediaSink for Sink {
    fn video(&self, _g: u64, f: VideoFrame) {
        if !self.paused.load(Ordering::Relaxed) {
            self.pipe.push(f);
        }
    }
    fn audio_format(&self, _g: u64, f: AudioFormat) {
        self.ring.set_format(f.sample_rate, f.channels);
    }
    fn audio(&self, _g: u64, f: AudioFrame) {
        if !self.paused.load(Ordering::Relaxed) {
            self.ring.push_packet(&f.data);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    None,
    Monitors,
    View,
    Keys,
    Quality,
    Info,
}

struct CursorTex {
    tex: Texture<'static>,
    hotx: i32,
    hoty: i32,
    w: u32,
    h: u32,
}

pub struct ActiveSession {
    pub handle: SessionHandle,
    pub profile_id: String,
    pub peer_id: String,
    pub connected: bool,
    pub peer: Option<PeerSummary>,
    pub route: Option<Route>,
    pub secured: bool,
    pub displays: Vec<DisplayDesc>,
    pub current: Option<DisplayDesc>,
    pipe: Arc<VideoPipeline>,
    ring: Arc<AudioRing>,
    paused: Arc<AtomicBool>,
    video_tex: Option<(Texture<'static>, u32, u32)>,
    pub presenting: Option<(Codec, bool, i32, i32)>,
    cursors: HashMap<u64, CursorTex>,
    cursor_id: u64,
    pending_cursor: Option<session::CursorImage>,
    remote_cursor: Option<(i32, i32)>,
    pub settings: PeerSettings,
    pub view: ViewState,
    held: HeldInput,
    pub toolbar: bool,
    toolbar_activity: Instant,
    pub menu: Menu,
    pub osk: Option<Osk>,
    view_before_osk: Option<ViewState>,
    vmouse: (i32, i32),
    dpad: [bool; 4],
    dpad_since: Option<Instant>,
    last_pointer: Option<Instant>,
    pointer_ui: (i32, i32),
    drag_lock: bool,
    ok_down: bool,
    keyboard_active_until: Option<Instant>,
    banner: Option<(String, Instant)>,
    win_alone: bool,
    background: bool,
    last_move_sent: (i32, i32),
    fps_count: (Instant, u64, f32),
    pub notice: Option<(String, Instant)>,
    pub uac: bool,
    px_per_logical: f32,
}

pub fn keycode_char(key: Keycode, mods: &KeyMods) -> Option<char> {
    let n = key.into_i32();
    if !(0x20..0x7f).contains(&n) {
        // Non-ASCII keycodes (e.g. a Hungarian keyboard's á) are the character itself.
        if n > 0xff && (n & (1 << 30)) == 0 {
            if let Some(c) = char::from_u32(n as u32) {
                return Some(if mods.shift ^ mods.caps { c.to_uppercase().next().unwrap_or(c) } else { c });
            }
        }
        return None;
    }
    let c = n as u8 as char;
    if !mods.shift {
        return Some(if mods.caps && c.is_ascii_alphabetic() { c.to_ascii_uppercase() } else { c });
    }
    Some(match c {
        'a'..='z' => if mods.caps { c } else { c.to_ascii_uppercase() },
        '1' => '!', '2' => '@', '3' => '#', '4' => '$', '5' => '%', '6' => '^', '7' => '&', '8' => '*', '9' => '(', '0' => ')',
        '-' => '_', '=' => '+', '[' => '{', ']' => '}', '\\' => '|', ';' => ':', '\'' => '"', ',' => '<', '.' => '>', '/' => '?', '`' => '~',
        other => other,
    })
}

impl ActiveSession {
    pub fn start(app: &mut App, req: session::SessionRequest, ring: Arc<AudioRing>, settings: PeerSettings) -> ActiveSession {
        let gen_cell = Arc::new(AtomicU64::new(0));
        let tx = app.tx.clone();
        let g2 = gen_cell.clone();
        let pipe = Arc::new(VideoPipeline::start(
            PipelineConfig {
                hardware: app.platform.ndl_available && app.store.state.settings.prefer_hardware_decoding,
                vp9_hardware: rdtv_media::caps::vp9_in_hardware(&app.platform, app.hw_policy(), &app.failed_codecs),
            },
            Arc::new(move |e| {
                let _ = tx.send(AppMsg::Pipeline(g2.load(Ordering::SeqCst), e));
            }),
        ));
        let paused = Arc::new(AtomicBool::new(false));
        let sink = Arc::new(Sink { pipe: pipe.clone(), ring: ring.clone(), paused: paused.clone() });
        let tx = app.tx.clone();
        let profile_id = req.profile.id.clone();
        let peer_id = req.target.as_str().to_owned();
        let handle = session::start(app.rt.handle(), req, sink, Arc::new(move |g, e| {
            let _ = tx.send(AppMsg::Session(g, e));
        }));
        gen_cell.store(handle.generation, Ordering::SeqCst);
        let view = ViewState {
            mode: settings.view,
            zoom: settings.zoom.max(1.0),
            pan_x: settings.pan_x,
            pan_y: settings.pan_y,
        };
        ActiveSession {
            handle,
            profile_id,
            peer_id,
            connected: false,
            peer: None,
            route: None,
            secured: false,
            displays: Vec::new(),
            current: None,
            pipe,
            ring,
            paused,
            video_tex: None,
            presenting: None,
            cursors: HashMap::new(),
            cursor_id: 0,
            pending_cursor: None,
            remote_cursor: None,
            settings,
            view,
            held: HeldInput::default(),
            toolbar: true,
            toolbar_activity: Instant::now(),
            menu: Menu::None,
            osk: None,
            view_before_osk: None,
            vmouse: (0, 0),
            dpad: [false; 4],
            dpad_since: None,
            last_pointer: None,
            pointer_ui: (W / 2, H / 2),
            drag_lock: false,
            ok_down: false,
            keyboard_active_until: None,
            banner: None,
            win_alone: false,
            background: false,
            last_move_sent: (i32::MIN, i32::MIN),
            fps_count: (Instant::now(), 0, 0.0),
            notice: None,
            uac: false,
            px_per_logical: 1.0,
        }
    }

    pub fn stop(self, lock: bool) {
        let mut s = self;
        if s.connected {
            let rel = s.held.release_all();
            if !rel.is_empty() {
                s.handle.send(SessionCommand::SendMany(rel));
            }
        }
        s.handle.send(SessionCommand::Disconnect { lock: lock && s.connected });
        s.pipe.reset();
        s.ring.clear();
    }

    fn send(&self, m: rdtv_core::message_proto::Message) {
        self.handle.send_msg(m);
    }

    fn monitor(&self) -> Option<Monitor> {
        self.current.as_ref().map(|d| Monitor { x: d.x, y: d.y, w: d.width.max(1), h: d.height.max(1) })
    }

    pub fn on_connected(&mut self, peer: PeerSummary, route: Route, secured: bool) {
        self.connected = true;
        self.displays = peer.displays.clone();
        let idx = if (peer.current_display as usize) < self.displays.len() { peer.current_display as usize } else { 0 };
        let want = self.settings.monitor;
        let pick = if want >= 0 && (want as usize) < self.displays.len() { want as usize } else { idx };
        if want as usize >= self.displays.len() && !self.settings.monitor_name.is_empty() {
            self.notice = Some((format!("Saved monitor \"{}\" is not available; showing monitor {}", self.settings.monitor_name, idx + 1), Instant::now()));
        }
        self.current = self.displays.get(pick).cloned().or_else(|| self.displays.first().cloned());
        if let Some(d) = &self.current {
            self.pipe.set_stream_size(d.width, d.height);
            self.vmouse = (d.x + d.width / 2, d.y + d.height / 2);
        }
        if self.settings.shortcuts.is_none() {
            self.settings.shortcuts = Some(default_shortcuts(peer.platform.eq_ignore_ascii_case("Mac OS")));
        }
        self.peer = Some(peer);
        self.route = Some(route);
        self.secured = secured;
        self.toolbar = true;
        self.toolbar_activity = Instant::now();
    }

    pub fn on_event(&mut self, ev: SessionEvent, store: &mut Store) {
        match ev {
            SessionEvent::Displays(d) => {
                self.displays = d;
                if let Some(cur) = &self.current {
                    if !self.displays.iter().any(|x| x.index == cur.index) {
                        self.notice = Some(("The selected monitor disappeared; switched to the first monitor".into(), Instant::now()));
                        self.switch_monitor(0, store);
                    }
                }
            }
            SessionEvent::SwitchDisplay(d) => {
                if d.width > 0 && d.height > 0 {
                    self.pipe.set_stream_size(d.width, d.height);
                }
                self.pipe.reset();
                self.video_tex = None;
                let name = if d.name.is_empty() { format!("Monitor {}", d.index + 1) } else { d.name.clone() };
                self.banner = Some((format!("Monitor {} of {} · {}", d.index + 1, self.displays.len().max(1), name), Instant::now()));
                self.current = Some(d);
                if let Some(m) = self.monitor() {
                    self.vmouse = view::clamp_host(m, self.vmouse.0, self.vmouse.1);
                }
            }
            SessionEvent::Cursor(c) => {
                if self.cursors.len() > 64 {
                    self.cursors.clear();
                }
                // Texture creation happens lazily at draw time (needs the texture creator).
                self.pending_cursor = Some(c);
            }
            SessionEvent::CursorId(i) => self.cursor_id = i,
            SessionEvent::CursorPosition { x, y } => self.remote_cursor = Some((x, y)),
            SessionEvent::Permission { name, enabled } => {
                self.notice = Some((format!("Host {} permission: {}", name, if enabled { "allowed" } else { "denied" }), Instant::now()));
            }
            SessionEvent::Notice { title, text } => {
                self.notice = Some((format!("{title}: {text}"), Instant::now()));
            }
            SessionEvent::Uac(on) => self.uac = on,
            _ => {}
        }
    }

    pub fn on_pipeline_event(&mut self, ev: PipelineEvent) {
        match ev {
            PipelineEvent::Presenting { codec, hardware, width, height } => {
                log::info!("presenting {} {}x{} ({})", codec.name(), width, height, if hardware { "hardware" } else { "software" });
                self.presenting = Some((codec, hardware, width, height));
                if hardware {
                    self.video_tex = None;
                }
            }
            PipelineEvent::NeedKeyframe(d) => {
                self.handle.send(SessionCommand::RefreshVideo(d));
            }
            PipelineEvent::CodecFailed { .. } => {}
        }
    }

    pub fn on_codec_failed(&mut self, codec: Codec, reason: &str, caps: DecodeCaps, vp9_hw: bool) {
        log::warn!("codec {} failed: {reason}; re-advertising {:?}", codec.name(), caps);
        self.pipe.set_config(PipelineConfig { hardware: caps.h264 || caps.h265 || caps.av1 || vp9_hw, vp9_hardware: vp9_hw });
        self.handle.send(SessionCommand::UpdateDecoding(caps));
        if let Some(d) = &self.current {
            self.handle.send(SessionCommand::RefreshVideo(d.index));
        }
    }

    // ----- lifecycle ---------------------------------------------------------------------

    pub fn pause_input(&mut self) {
        let rel = self.held.release_all();
        if !rel.is_empty() && self.connected {
            self.handle.send(SessionCommand::SendMany(rel));
        }
        self.dpad = [false; 4];
        self.dpad_since = None;
        self.drag_lock = false;
        self.ok_down = false;
        if let Some(o) = self.osk.as_mut() {
            o.reset_modifiers();
        }
    }

    pub fn enter_background(&mut self) {
        log::info!("keeping the session in the background: input paused, audio muted, video stopped");
        self.background = true;
        self.paused.store(true, Ordering::SeqCst);
        self.ring.set_muted(true);
        self.pipe.reset();
        self.video_tex = None;
    }

    pub fn resume_from_background(&mut self) {
        if self.background {
            self.background = false;
            self.paused.store(false, Ordering::SeqCst);
            self.ring.set_muted(self.settings.muted);
            self.pipe.reset();
            if let Some(d) = &self.current {
                self.handle.send(SessionCommand::RefreshVideo(d.index));
            }
        }
    }

    // ----- helpers -----------------------------------------------------------------------

    fn save_settings(&self, store: &mut Store) {
        let rec = store.peer_mut(&self.profile_id, &self.peer_id);
        let mut s = self.settings.clone();
        s.view = self.view.mode;
        s.zoom = self.view.zoom;
        s.pan_x = self.view.pan_x;
        s.pan_y = self.view.pan_y;
        rec.settings = s;
        store.save();
    }

    pub fn toggle_toolbar(&mut self) {
        self.toolbar = !self.toolbar;
        self.menu = Menu::None;
        self.toolbar_activity = Instant::now();
    }

    fn touch_toolbar(&mut self) {
        self.toolbar_activity = Instant::now();
    }

    pub fn switch_monitor(&mut self, index: usize, store: &mut Store) {
        if let Some(d) = self.displays.get(index).cloned() {
            self.handle.send(SessionCommand::SwitchDisplay(d.index));
            self.settings.monitor = d.index;
            self.settings.monitor_name = d.name.clone();
            self.save_settings(store);
            let name = if d.name.is_empty() { format!("Monitor {}", d.index + 1) } else { d.name.clone() };
            self.banner = Some((format!("Switching to monitor {} of {} · {}", d.index + 1, self.displays.len(), name), Instant::now()));
        }
    }

    fn next_monitor(&mut self, store: &mut Store) {
        let n = self.displays.len();
        if n <= 1 {
            self.banner = Some(("Only one monitor".into(), Instant::now()));
            return;
        }
        let cur = self.current.as_ref().map(|d| d.index as usize).unwrap_or(0);
        self.switch_monitor((cur + 1) % n, store);
    }

    fn hardware(&self) -> bool {
        self.presenting.is_some_and(|p| p.1)
    }

    fn stream_size(&self) -> (f32, f32) {
        if let Some((_, w, h)) = &self.video_tex {
            return (*w as f32, *h as f32);
        }
        if let Some((_, _, w, h)) = self.presenting {
            return (w as f32, h as f32);
        }
        self.current.as_ref().map(|d| (d.width as f32, d.height as f32)).unwrap_or((1920.0, 1080.0))
    }

    /// The area the picture may use and `V` within it.
    pub fn view_rect(&self) -> RectF {
        let osk_h = self.osk.as_ref().map(|o| o.height()).unwrap_or(0);
        let shrink = self.osk.is_some() && self.settings.osk_presentation == OskPresentation::Shrink && !self.hardware();
        let area = if shrink { RectF::new(0.0, 0.0, W as f32, (H - osk_h) as f32) } else { RectF::new(0.0, 0.0, W as f32, H as f32) };
        let (sw, sh) = self.stream_size();
        let st = if self.hardware() { ViewState::default() } else { self.view };
        view::view_rect(area, sw, sh, &st, self.px_per_logical)
    }

    fn pointer_host(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        let m = self.monitor()?;
        if self.osk.as_ref().is_some_and(|o| y >= H - o.height()) {
            return None;
        }
        if self.toolbar && y < TOOLBAR_H + 20 {
            return None;
        }
        view::to_host(self.view_rect(), m, x as f32, y as f32)
    }

    fn mods_none() -> Mods {
        Mods::default()
    }

    fn click_pos(&self) -> (i32, i32) {
        self.vmouse
    }

    fn send_button(&mut self, b: MouseButton, down: bool) {
        let (x, y) = self.click_pos();
        self.held.note_button(b, down, x, y);
        self.send(input::mouse_button(b, down, x, y, Self::mods_none()));
    }

    fn click(&mut self, b: MouseButton) {
        self.send_button(b, true);
        self.send_button(b, false);
    }

    fn move_to(&mut self, x: i32, y: i32) {
        self.vmouse = (x, y);
        self.held.note_pos(x, y);
        if (x, y) != self.last_move_sent {
            self.last_move_sent = (x, y);
            self.send(input::mouse_move(x, y));
        }
    }

    fn open_keyboard(&mut self) {
        let mut o = Osk::new(OskMode::Remote, self.settings.keyboard_layout);
        o.mac = self.peer.as_ref().is_some_and(|p| p.platform.eq_ignore_ascii_case("Mac OS"));
        o.shortcuts = self.settings.shortcuts.clone().unwrap_or_default();
        self.view_before_osk = Some(self.view);
        self.osk = Some(o);
        self.toolbar = false;
        self.menu = Menu::None;
        // Keyboard owns the D-pad while open; release anything the D-pad was holding.
        self.dpad = [false; 4];
        if self.hardware() && self.settings.osk_presentation == OskPresentation::Shrink {
            self.notice = Some(("With hardware decoding the TV shows the picture full-screen, so the keyboard overlays it (shrink needs software decoding).".into(), Instant::now()));
        }
    }

    fn close_keyboard(&mut self) {
        self.osk = None;
        if let Some(v) = self.view_before_osk.take() {
            self.view = v;
        }
    }

    pub fn tick(&mut self) {
        if self.toolbar && self.menu == Menu::None && self.osk.is_none() && self.toolbar_activity.elapsed() > TOOLBAR_HIDE && self.connected {
            self.toolbar = false;
        }
        // D-pad virtual mouse movement with acceleration.
        if self.settings.dpad_mode == DpadMode::Mouse && self.osk.is_none() && self.menu == Menu::None && !self.toolbar && self.dpad.iter().any(|d| *d) {
            let held_for = self.dpad_since.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0);
            let speed_ui = (4.0 + held_for * 30.0).min(30.0);
            let v = self.view_rect();
            if let Some(m) = self.monitor() {
                let sx = m.w as f32 / v.w.max(1.0);
                let sy = m.h as f32 / v.h.max(1.0);
                let dx = (self.dpad[3] as i32 - self.dpad[2] as i32) as f32 * speed_ui * sx;
                let dy = (self.dpad[1] as i32 - self.dpad[0] as i32) as f32 * speed_ui * sy;
                let (nx, ny) = view::clamp_host(m, self.vmouse.0 + dx.round() as i32, self.vmouse.1 + dy.round() as i32);
                self.move_to(nx, ny);
                if self.view.mode != ViewMode::Fit && !self.hardware() {
                    let area = RectF::new(0.0, 0.0, W as f32, H as f32);
                    let (px, py) = view::pan_to_include(&self.view, v, area, m, nx, ny);
                    self.view.pan_x = px;
                    self.view.pan_y = py;
                }
            }
        }
        let decoded = self.pipe.stats.decoded.load(Ordering::Relaxed);
        if self.fps_count.0.elapsed() >= Duration::from_secs(1) {
            let el = self.fps_count.0.elapsed().as_secs_f32();
            self.fps_count.2 = (decoded - self.fps_count.1) as f32 / el;
            self.fps_count = (Instant::now(), decoded, self.fps_count.2);
        }
    }

    // ----- input -------------------------------------------------------------------------

    /// Returns true when consumed.
    pub fn handle_input(&mut self, i: &In, ui: &mut UiState, store: &mut Store) -> bool {
        if !self.connected || self.background {
            return false;
        }
        // Physical escape chord: Ctrl+Alt+Shift+Space opens local controls.
        if let In::Key { key: Keycode::Space, mods, down: true, .. } = i {
            if store.state.settings.escape_chord_enabled && mods.ctrl && mods.alt && mods.shift {
                self.toolbar = true;
                self.menu = Menu::None;
                self.touch_toolbar();
                return true;
            }
        }
        let local_overlay = self.osk.is_some() || self.toolbar || self.menu != Menu::None;
        match i {
            In::Remote { key, down, repeat } => {
                let (key, down, repeat) = (*key, *down, *repeat);
                if key == Remote::Back {
                    if down && !repeat {
                        if self.menu != Menu::None {
                            self.menu = Menu::None;
                            self.touch_toolbar();
                        } else if self.osk.is_some() {
                            self.close_keyboard();
                        } else {
                            self.toggle_toolbar();
                        }
                    }
                    return true;
                }
                if key == Remote::Blue {
                    if down && !repeat && self.menu == Menu::None {
                        if self.osk.is_some() {
                            self.close_keyboard();
                        } else {
                            self.open_keyboard();
                        }
                    }
                    return true;
                }
                if local_overlay {
                    // Overlay owns D-pad/OK; colour shortcuts are blocked while a menu or
                    // the keyboard is open, but work over the bare toolbar.
                    if self.toolbar && self.osk.is_none() && self.menu == Menu::None && matches!(key, Remote::Red | Remote::Green | Remote::Yellow) {
                        self.toolbar = false;
                        return self.handle_input(i, ui, store);
                    }
                    if down {
                        self.touch_toolbar();
                        match key {
                            Remote::Up => ui.push(UiIn::Nav(Nav::Up)),
                            Remote::Down => ui.push(UiIn::Nav(Nav::Down)),
                            Remote::Left => ui.push(UiIn::Nav(Nav::Left)),
                            Remote::Right => ui.push(UiIn::Nav(Nav::Right)),
                            Remote::Ok if !repeat => ui.push(UiIn::Activate),
                            _ => {}
                        }
                    }
                    return true;
                }
                let kb_active = self.keyboard_active_until.is_some_and(|t| Instant::now() < t);
                match key {
                    Remote::Red if down && !repeat => self.click(MouseButton::Right),
                    Remote::Green if down && !repeat => self.next_monitor(store),
                    Remote::Yellow if down && !repeat => {
                        self.settings.dpad_mode = match self.settings.dpad_mode {
                            DpadMode::Mouse => DpadMode::Arrows,
                            DpadMode::Arrows => DpadMode::Mouse,
                        };
                        self.dpad = [false; 4];
                        let rel = self.held.release_all();
                        if !rel.is_empty() {
                            self.handle.send(SessionCommand::SendMany(rel));
                        }
                        self.banner = Some((
                            match self.settings.dpad_mode {
                                DpadMode::Mouse => "D-pad: Mouse mode".into(),
                                DpadMode::Arrows => "D-pad: Arrow-key mode".into(),
                            },
                            Instant::now(),
                        ));
                        self.save_settings(store);
                    }
                    Remote::ChUp if down => self.send(input::mouse_wheel(0, 3)),
                    Remote::ChDown if down => self.send(input::mouse_wheel(0, -3)),
                    Remote::Up | Remote::Down | Remote::Left | Remote::Right | Remote::Ok => {
                        let arrows = self.settings.dpad_mode == DpadMode::Arrows || kb_active;
                        if arrows {
                            let ck = match key {
                                Remote::Up => ControlKey::UpArrow,
                                Remote::Down => ControlKey::DownArrow,
                                Remote::Left => ControlKey::LeftArrow,
                                Remote::Right => ControlKey::RightArrow,
                                _ => ControlKey::Return,
                            };
                            if down && repeat {
                                self.send(input::control_key(ck, true, false, Mods::default()));
                            } else {
                                self.held.note_control_key(ck, down);
                                self.send(input::control_key(ck, down, false, Mods::default()));
                            }
                        } else {
                            let d = match key {
                                Remote::Up => Some(0),
                                Remote::Down => Some(1),
                                Remote::Left => Some(2),
                                Remote::Right => Some(3),
                                _ => None,
                            };
                            if let Some(d) = d {
                                if down && !self.dpad.iter().any(|x| *x) {
                                    self.dpad_since = Some(Instant::now());
                                }
                                self.dpad[d] = down;
                            } else if !repeat {
                                // OK: left click; with drag-lock the button stays down
                                // until the next OK.
                                if self.drag_lock {
                                    if down {
                                        let held = self.held.is_button_down(MouseButton::Left);
                                        self.send_button(MouseButton::Left, !held);
                                        if held {
                                            self.drag_lock = false;
                                            self.banner = Some(("Drag lock released".into(), Instant::now()));
                                        }
                                    }
                                } else if down != self.ok_down {
                                    self.ok_down = down;
                                    self.send_button(MouseButton::Left, down);
                                }
                            }
                        }
                    }
                    _ => {}
                }
                true
            }
            In::PointerMove { x, y } => {
                self.pointer_ui = (*x, *y);
                self.last_pointer = Some(Instant::now());
                if local_overlay {
                    let over_ui = (self.toolbar && *y < TOOLBAR_H + 20)
                        || self.menu != Menu::None
                        || self.osk.as_ref().is_some_and(|o| *y >= H - o.height());
                    if over_ui {
                        ui.push(UiIn::PointerMove(*x, *y));
                        self.touch_toolbar();
                        return true;
                    }
                }
                if let Some((hx, hy)) = self.pointer_host(*x, *y) {
                    self.move_to(hx, hy);
                }
                true
            }
            In::PointerButton { button, down, x, y } => {
                let over_ui = (self.toolbar && *y < TOOLBAR_H + 20)
                    || self.menu != Menu::None
                    || self.osk.as_ref().is_some_and(|o| *y >= H - o.height());
                if over_ui {
                    if !*down && *button == PtrButton::Left {
                        ui.push(UiIn::Click(*x, *y));
                    }
                    self.touch_toolbar();
                    return true;
                }
                // Releases always go through (no stuck buttons), presses only on the picture.
                let b = match button {
                    PtrButton::Left => MouseButton::Left,
                    PtrButton::Right => MouseButton::Right,
                    PtrButton::Middle => MouseButton::Middle,
                };
                if *down {
                    if let Some((hx, hy)) = self.pointer_host(*x, *y) {
                        self.move_to(hx, hy);
                        self.send_button(b, true);
                    }
                } else if self.held.is_button_down(b) {
                    if let Some((hx, hy)) = self.pointer_host(*x, *y) {
                        self.move_to(hx, hy);
                    }
                    if !(self.drag_lock && b == MouseButton::Left) {
                        self.send_button(b, false);
                    }
                }
                true
            }
            In::Wheel { dx, dy } => {
                if self.menu != Menu::None || (self.osk.is_some() && self.pointer_ui.1 >= H - self.osk.as_ref().map(|o| o.height()).unwrap_or(0)) {
                    ui.push(UiIn::Wheel(*dy));
                } else {
                    self.send(input::mouse_wheel(*dx, *dy));
                }
                true
            }
            In::Key { key, mods, down, repeat, .. } => {
                self.keyboard_active_until = Some(Instant::now() + KEYBOARD_ACTIVE);
                if self.menu != Menu::None {
                    return false;
                }
                self.physical_key(*key, mods, *down, *repeat);
                true
            }
            In::Text(_) => true,
            _ => false,
        }
    }

    fn physical_key(&mut self, key: Keycode, mods: &KeyMods, down: bool, repeat: bool) {
        let m = Mods { ctrl: mods.ctrl, alt: mods.alt, shift: mods.shift, meta: mods.gui, altgr: false };
        // Bare modifiers are not sent on their own (legacy mode attaches modifiers to each
        // key), except a lone Win/Command press, which opens the Start menu.
        match key {
            Keycode::LGui | Keycode::RGui => {
                if down && !repeat {
                    self.win_alone = true;
                } else if !down && self.win_alone {
                    self.win_alone = false;
                    self.send(input::control_key(ControlKey::Meta, true, true, Mods::default()));
                }
                return;
            }
            Keycode::LCtrl | Keycode::RCtrl | Keycode::LShift | Keycode::RShift | Keycode::LAlt | Keycode::RAlt => return,
            _ => {}
        }
        if down {
            self.win_alone = false;
        }
        if !m.any_hotkey() {
            if let Some(c) = keycode_char(key, mods) {
                if down {
                    self.send(input::text(&c.to_string()));
                }
                return;
            }
        }
        match physical_to_remote(key, mods) {
            Some(RemoteKey::Control(ck)) => {
                if repeat && down {
                    self.send(input::control_key(ck, true, false, m));
                } else {
                    self.held.note_control_key(ck, down);
                    self.send(input::control_key(ck, down, false, m));
                }
            }
            Some(RemoteKey::Char(c)) => {
                if down {
                    self.send(input::chr_key(c, true, true, m));
                }
            }
            None => {}
        }
    }

    pub fn osk_actions(&mut self, acts: Vec<OskAction>, store: &mut Store) {
        let linux = self.peer.as_ref().is_some_and(|p| p.platform == "Linux");
        let _ = linux;
        for a in acts {
            match a {
                OskAction::Text(t) => self.send(input::text(&t)),
                OskAction::Chord(c) => {
                    for m in input::chord_messages(&c) {
                        self.send(m);
                    }
                }
                OskAction::CtrlAltDel => self.send(input::ctrl_alt_del()),
                OskAction::Close => self.close_keyboard(),
                OskAction::LayoutChanged(l) => {
                    self.settings.keyboard_layout = l;
                    self.save_settings(store);
                }
                OskAction::ShortcutsChanged(s) => {
                    self.settings.shortcuts = Some(s);
                    self.save_settings(store);
                }
                OskAction::Backspace | OskAction::Submit => {}
            }
        }
    }

    fn upload_video(&mut self, g: &mut Gfx) {
        if self.hardware() {
            return;
        }
        let pic = self.pipe.mailbox.lock().ok().and_then(|mut m| m.take());
        let Some(pic) = pic else { return };
        let needs_new = self.video_tex.as_ref().is_none_or(|(_, w, h)| *w != pic.width || *h != pic.height);
        if needs_new {
            match g.texture_creator().create_texture_streaming(PixelFormatEnum::IYUV, pic.width, pic.height) {
                Ok(t) => self.video_tex = Some((t, pic.width, pic.height)),
                Err(e) => {
                    log::error!("video texture: {e}");
                    return;
                }
            }
        }
        if let Some((t, _, _)) = self.video_tex.as_mut() {
            let _ = t.update_yuv(None, &pic.y, pic.y_stride, &pic.u, pic.uv_stride, &pic.v, pic.uv_stride);
        }
    }

    fn upload_cursor(&mut self, g: &mut Gfx) {
        if let Some(c) = self.pending_cursor.take() {
            if let Ok(mut t) = g.texture_creator().create_texture_static(PixelFormatEnum::ABGR8888, c.width as u32, c.height as u32) {
                let _ = t.update(None, &c.rgba, (c.width * 4) as usize);
                t.set_blend_mode(sdl2::render::BlendMode::Blend);
                self.cursors.insert(c.id, CursorTex { tex: t, hotx: c.hotx, hoty: c.hoty, w: c.width as u32, h: c.height as u32 });
                self.cursor_id = c.id;
            }
        }
    }
}


/// Draw the session screen and its overlays.
pub fn draw(app: &mut App, f: &mut Frame, overlay: bool) {
    let Some(a) = app.active.as_mut() else {
        f.g.clear(theme::BG);
        return;
    };
    a.px_per_logical = f.g.raster_scale;
    a.upload_video(f.g);
    a.upload_cursor(f.g);
    let hw = a.hardware();
    if hw {
        // The decoded picture is on the hardware plane below: punch through.
        f.g.canvas.set_blend_mode(sdl2::render::BlendMode::None);
        f.g.clear(Color::RGBA(0, 0, 0, 0));
        f.g.canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    } else {
        f.g.clear(Color::RGB(0, 0, 0));
        let v = a.view_rect();
        if let Some((t, _, _)) = a.video_tex.as_ref() {
            let _ = f.g.canvas.copy(t, None, v.to_sdl());
        } else if a.connected {
            f.g.text_centered("Waiting for the first picture…", Rect::new(0, 0, W as u32, H as u32), size::LARGE, false, theme::TEXT_DIM);
        }
    }
    // Remote cursor for D-pad mouse use (the Magic Remote pointer is drawn by the TV).
    let pointer_recent = a.last_pointer.is_some_and(|t| t.elapsed() < Duration::from_secs(3));
    if a.settings.dpad_mode == DpadMode::Mouse && !pointer_recent && a.osk.is_none() {
        let v = a.view_rect();
        if let (Some(m), Some(c)) = (a.monitor(), a.cursors.get(&a.cursor_id)) {
            let (ux, uy) = view::from_host(v, m, a.vmouse.0, a.vmouse.1);
            let scale = (v.w / m.w as f32).clamp(0.5, 2.0);
            let dst = Rect::new(
                (ux - c.hotx as f32 * scale) as i32,
                (uy - c.hoty as f32 * scale) as i32,
                ((c.w as f32 * scale) as u32).max(1),
                ((c.h as f32 * scale) as u32).max(1),
            );
            let _ = f.g.canvas.copy(&c.tex, None, dst);
        } else if let Some(m) = a.monitor() {
            let v = a.view_rect();
            let (ux, uy) = view::from_host(v, m, a.vmouse.0, a.vmouse.1);
            f.g.round(Rect::new(ux as i32 - 8, uy as i32 - 8, 16, 16), 8, theme::ACCENT2);
        }
    }
    let inert = overlay;
    f.set_inert(inert);
    // Banner (monitor switch / mode change).
    if let Some((txt, t)) = a.banner.clone() {
        if t.elapsed() < Duration::from_millis(2500) {
            let w = f.g.text_width(&txt, size::LARGE, true) + 60;
            let r = Rect::new((W - w) / 2, H / 2 - 50, w as u32, 100);
            f.g.round(r, 24, Color::RGBA(10, 18, 34, 220));
            f.g.text_centered(&txt, r, size::LARGE, true, theme::TEXT);
        } else {
            a.banner = None;
        }
    }
    if let Some((txt, t)) = a.notice.clone() {
        if t.elapsed() < Duration::from_secs(7) {
            let w = (f.g.text_width(&txt, size::BODY, false) + 60).min(W - 120);
            let r = Rect::new((W - w) / 2, H - 140 - a.osk.as_ref().map(|o| o.height()).unwrap_or(0), w as u32, 70);
            f.g.round(r, 18, Color::RGBA(60, 40, 10, 230));
            f.g.text_fit(&txt, r.x() + 30, r.y() + 18, size::BODY, false, theme::TEXT, w - 60);
        } else {
            a.notice = None;
        }
    }
    if a.toolbar {
        draw_toolbar(app, f);
    }
    let Some(a) = app.active.as_mut() else { return };
    if a.menu != Menu::None {
        draw_menu(app, f);
    }
    let Some(a) = app.active.as_mut() else { return };
    if let Some(mut o) = a.osk.take() {
        let acts = o.draw(f);
        a.osk = Some(o);
        a.osk_actions(acts, &mut app.store);
    }
    let Some(a) = app.active.as_ref() else { return };
    // Persistent tiny status chip when the toolbar is hidden.
    if !a.toolbar && a.osk.is_none() {
        let mode = match a.settings.dpad_mode {
            DpadMode::Mouse => "Mouse",
            DpadMode::Arrows => "Arrows",
        };
        let txt = format!("{mode} · Back: menu");
        let w = f.g.text_width(&txt, size::SMALL, false) + 30;
        let r = Rect::new(W - w - 20, 16, w as u32, 40);
        f.g.round(r, 20, Color::RGBA(10, 18, 34, 150));
        f.g.text_centered(&txt, r, size::SMALL, false, Color::RGBA(236, 241, 250, 200));
    }
}


fn draw_toolbar(app: &mut App, f: &mut Frame) {
    let Some(a) = app.active.as_mut() else { return };
    f.g.fill(Rect::new(0, 0, W as u32, TOOLBAR_H as u32), Color::RGBA(10, 18, 34, 235));
    let host = a.peer.as_ref().map(|p| if p.hostname.is_empty() { a.peer_id.clone() } else { p.hostname.clone() }).unwrap_or_default();
    f.g.text_fit(&host, 30, 14, size::BODY, true, theme::TEXT, 270);
    let status = format!(
        "{}{}",
        a.route.map(|r| r.label()).unwrap_or(""),
        if a.secured { " · encrypted" } else { "" }
    );
    f.g.text_fit(&status, 30, 56, size::SMALL, false, theme::TEXT_DIM, 270);
    let y = 18;
    let h = 68u32;
    let mut x = 310;
    let mon_label = a
        .current
        .as_ref()
        .map(|d| format!("Monitor {}/{}", d.index + 1, a.displays.len().max(1)))
        .unwrap_or_else(|| "Monitor".into());
    let mode_label = match a.settings.dpad_mode {
        DpadMode::Mouse => "Mouse",
        DpadMode::Arrows => "Arrows",
    };
    let items: Vec<(&str, String, Option<Color>, &str)> = vec![
        ("tb-kbd", "Keyboard".into(), Some(theme::BLUE_KEY), "Blue"),
        ("tb-mon", mon_label, Some(theme::GREEN_KEY), "Green"),
        ("tb-mode", mode_label.into(), Some(theme::YELLOW_KEY), "Yellow"),
        ("tb-view", "View".into(), None, ""),
        ("tb-keys", "Keys".into(), None, ""),
        ("tb-audio", if a.settings.muted { "Unmute".into() } else { "Mute".into() }, None, ""),
        ("tb-quality", "Quality".into(), None, ""),
        ("tb-info", "Info".into(), None, ""),
        ("tb-disc", "Disconnect".into(), None, ""),
    ];
    let mut clicked = None;
    for (key, label, col, kn) in items.iter() {
        let _ = kn;
        let w = f.g.text_width(label, size::BODY, true) + if col.is_some() { 66 } else { 36 };
        let r = Rect::new(x, y, w as u32, h);
        if f.key_button(id(key), r, label, *col, "") {
            clicked = Some(*key);
        }
        x += w + 10;
    }
    let Some(c) = clicked else { return };
    let store = &mut app.store;
    let a = app.active.as_mut().unwrap();
    a.touch_toolbar();
    match c {
        "tb-kbd" => a.open_keyboard(),
        "tb-mon" => a.menu = Menu::Monitors,
        "tb-mode" => {
            a.settings.dpad_mode = match a.settings.dpad_mode {
                DpadMode::Mouse => DpadMode::Arrows,
                DpadMode::Arrows => DpadMode::Mouse,
            };
            a.save_settings(store);
        }
        "tb-view" => a.menu = Menu::View,
        "tb-keys" => a.menu = Menu::Keys,
        "tb-audio" => {
            a.settings.muted = !a.settings.muted;
            a.ring.set_muted(a.settings.muted);
            a.save_settings(store);
        }
        "tb-quality" => a.menu = Menu::Quality,
        "tb-info" => a.menu = Menu::Info,
        "tb-disc" => {
            let lock = a.settings.lock_on_disconnect;
            app.end_session(lock, "user disconnected");
        }
        _ => {}
    }
}

fn draw_menu(app: &mut App, f: &mut Frame) {
    let Some(a) = app.active.as_mut() else { return };
    let menu = a.menu;
    let w = 760;
    let x = (W - w) / 2;
    let top = TOOLBAR_H + 30;
    let row_h = 76;
    let gap = 12;
    enum Act {
        Monitor(usize),
        View(ViewMode),
        ZoomIn,
        ZoomOut,
        Cad,
        Lock,
        Chord(&'static [&'static str]),
        DoubleClick,
        DragLock,
        Refresh,
        Release,
        Quality(Quality),
        Fps(u32),
        OskPresentation,
        LockOnDisconnect,
        PeerSettings,
        Close,
    }
    let mut rows: Vec<(String, Act)> = Vec::new();
    let title;
    match menu {
        Menu::Monitors => {
            title = "Monitors".to_owned();
            for (i, d) in a.displays.iter().enumerate() {
                let cur = a.current.as_ref().is_some_and(|c| c.index == d.index);
                let name = if d.name.is_empty() { format!("Monitor {}", i + 1) } else { d.name.clone() };
                rows.push((format!("{}{} · {}x{}", if cur { "● " } else { "" }, name, d.width, d.height), Act::Monitor(i)));
            }
        }
        Menu::View => {
            title = "View (local only; the remote resolution never changes)".to_owned();
            let hw = a.hardware();
            rows.push(("Fit to screen".into(), Act::View(ViewMode::Fit)));
            if hw {
                rows.push(("1:1 and zoom need software decoding (hardware plane is full-screen)".into(), Act::Close));
            } else {
                rows.push(("Original size (1:1 pixels)".into(), Act::View(ViewMode::Original)));
                rows.push((format!("Zoom in (now {:.1}x)", if a.view.mode == ViewMode::Zoom { a.view.zoom } else { 1.0 }), Act::ZoomIn));
                rows.push(("Zoom out".into(), Act::ZoomOut));
            }
            rows.push((
                format!("Keyboard: {}", match a.settings.osk_presentation {
                    OskPresentation::Shrink => "shrink picture above keyboard",
                    OskPresentation::Overlay => "overlay lower picture",
                }),
                Act::OskPresentation,
            ));
        }
        Menu::Keys => {
            title = "Keys & actions".to_owned();
            rows.push(("Send Ctrl+Alt+Del".into(), Act::Cad));
            rows.push(("Lock remote computer".into(), Act::Lock));
            rows.push(("Escape".into(), Act::Chord(&["Esc"])));
            rows.push(("Windows / Super key".into(), Act::Chord(&["Meta"])));
            rows.push(("Alt+Tab".into(), Act::Chord(&["Alt", "Tab"])));
            rows.push(("Alt+F4".into(), Act::Chord(&["Alt", "F4"])));
            rows.push(("Double-click".into(), Act::DoubleClick));
            rows.push((if a.drag_lock { "Drag lock: on (OK releases)".into() } else { "Drag lock (hold left button)".into() }, Act::DragLock));
            rows.push(("Refresh video".into(), Act::Refresh));
            rows.push(("Release stuck keys and buttons".into(), Act::Release));
            rows.push((format!("Lock on disconnect: {}", if a.settings.lock_on_disconnect { "on" } else { "off" }), Act::LockOnDisconnect));
            rows.push(("Computer settings…".into(), Act::PeerSettings));
        }
        Menu::Quality => {
            title = "Picture quality".to_owned();
            rows.push((format!("{}Balanced (default)", if a.settings.quality == Quality::Balanced { "● " } else { "" }), Act::Quality(Quality::Balanced)));
            rows.push((format!("{}Best (sharpest text)", if a.settings.quality == Quality::Best { "● " } else { "" }), Act::Quality(Quality::Best)));
            rows.push((format!("{}Low (fast)", if a.settings.quality == Quality::Low { "● " } else { "" }), Act::Quality(Quality::Low)));
            for fps in [30u32, 60] {
                rows.push((format!("{}Frame rate {fps}", if a.settings.fps == fps { "● " } else { "" }), Act::Fps(fps)));
            }
        }
        Menu::Info => {
            title = "Session info".to_owned();
            let p = a.presenting.map(|(c, hw, w, h)| format!("{} {}x{} · {}", c.name(), w, h, if hw { "hardware decoder" } else { "software decoder" })).unwrap_or_else(|| "no picture yet".into());
            let st = &a.pipe.stats;
            rows.push((format!("Video: {p}"), Act::Close));
            rows.push((format!("Decoded {:.0} fps · received {} · dropped {}", a.fps_count.2, st.received.load(Ordering::Relaxed), st.dropped.load(Ordering::Relaxed)), Act::Close));
            rows.push((format!("Audio queue {} ms{}", a.ring.queued_ms(), if a.settings.muted { " (muted)" } else { "" }), Act::Close));
            let peer = a.peer.as_ref().map(|p| format!("{} · {} · RustDesk {}", p.hostname, p.platform, p.version)).unwrap_or_default();
            rows.push((format!("Host: {peer}"), Act::Close));
            rows.push((format!("Route: {} · {}", a.route.map(|r| r.label()).unwrap_or("?"), if a.secured { "end-to-end key negotiated" } else { "not end-to-end encrypted (no verified peer key)" }), Act::Close));
            if a.uac {
                rows.push(("The remote side shows an elevated (UAC) window".into(), Act::Close));
            }
        }
        Menu::None => return,
    }
    let n = rows.len() as i32;
    let h = 100 + n * (row_h + gap);
    let panel = Rect::new(x, top, w as u32, h.min(H - top - 20) as u32);
    f.g.round(panel, 24, Color::RGBA(22, 33, 56, 248));
    f.g.text_fit(&title, x + 30, top + 26, size::LARGE, true, theme::TEXT, w - 60);
    let visible = ((H - top - 140) / (row_h + gap)).max(1) as usize;
    let focused_index = rows.iter().enumerate().position(|(i, _)| f.is_focused(idx("menu-row", i)));
    let first = f.list_scroll(id("menu-list"), focused_index, visible, rows.len());
    let mut picked = None;
    for (i, (label, _)) in rows.iter().enumerate().skip(first).take(visible) {
        let ry = top + 90 + (i - first) as i32 * (row_h + gap);
        let r = Rect::new(x + 24, ry, (w - 48) as u32, row_h as u32);
        let (foc, act) = f.register(idx("menu-row", i), r);
        f.g.round(r, 14, if foc { theme::PANEL_HI } else { theme::ROW });
        if foc {
            f.focus_ring(r, 14);
        }
        f.g.text_fit(label, r.x() + 24, r.y() + 20, size::BODY, false, theme::TEXT, w - 100);
        if act {
            picked = Some(i);
        }
    }
    let Some(i) = picked else { return };
    let act = rows.swap_remove(i).1;
    let store = &mut app.store;
    let a = app.active.as_mut().unwrap();
    a.touch_toolbar();
    match act {
        Act::Monitor(i) => {
            a.switch_monitor(i, store);
            a.menu = Menu::None;
        }
        Act::View(m) => {
            a.view.mode = m;
            if m == ViewMode::Fit {
                a.view.zoom = 1.0;
            }
            a.save_settings(store);
        }
        Act::ZoomIn => {
            if a.view.mode != ViewMode::Zoom {
                a.view.mode = ViewMode::Zoom;
                a.view.zoom = 1.0;
            }
            a.view.zoom = (a.view.zoom * 1.5).min(view::MAX_ZOOM);
            if let Some(m) = a.monitor() {
                a.view.pan_x = (a.vmouse.0 - m.x) as f32 / m.w as f32;
                a.view.pan_y = (a.vmouse.1 - m.y) as f32 / m.h as f32;
            }
            a.save_settings(store);
        }
        Act::ZoomOut => {
            a.view.zoom = (a.view.zoom / 1.5).max(1.0);
            if a.view.zoom <= 1.0 {
                a.view.mode = ViewMode::Fit;
            }
            a.save_settings(store);
        }
        Act::Cad => {
            a.send(input::ctrl_alt_del());
            a.menu = Menu::None;
        }
        Act::Lock => {
            a.send(input::lock_screen());
            a.notice = Some(("Lock request sent to the remote computer".into(), Instant::now()));
            a.menu = Menu::None;
        }
        Act::Chord(keys) => {
            let ks: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
            if let Some(c) = input::parse_chord(&ks) {
                for m in input::chord_messages(&c) {
                    a.send(m);
                }
            }
            a.menu = Menu::None;
        }
        Act::DoubleClick => {
            a.click(MouseButton::Left);
            a.click(MouseButton::Left);
            a.menu = Menu::None;
        }
        Act::DragLock => {
            a.drag_lock = !a.drag_lock;
            if !a.drag_lock && a.held.is_button_down(MouseButton::Left) {
                a.send_button(MouseButton::Left, false);
            }
            a.menu = Menu::None;
            a.toolbar = false;
        }
        Act::Refresh => {
            a.pipe.reset();
            if let Some(d) = &a.current {
                a.handle.send(SessionCommand::RefreshVideo(d.index));
            }
            a.menu = Menu::None;
        }
        Act::Release => {
            let rel = a.held.release_all();
            a.handle.send(SessionCommand::SendMany(rel));
            for ck in [ControlKey::Control, ControlKey::Shift, ControlKey::Alt, ControlKey::Meta, ControlKey::RAlt] {
                a.send(input::control_key(ck, false, false, Mods::default()));
            }
            for b in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
                let (x, y) = a.vmouse;
                a.send(input::mouse_button(b, false, x, y, Mods::default()));
            }
            a.drag_lock = false;
            a.notice = Some(("Released all keys and buttons".into(), Instant::now()));
            a.menu = Menu::None;
        }
        Act::Quality(q) => {
            a.settings.quality = q;
            a.handle.send(SessionCommand::SetQuality(q));
            a.save_settings(store);
        }
        Act::Fps(fps) => {
            a.settings.fps = fps;
            a.handle.send(SessionCommand::SetFps(fps));
            a.save_settings(store);
        }
        Act::OskPresentation => {
            a.settings.osk_presentation = match a.settings.osk_presentation {
                OskPresentation::Shrink => OskPresentation::Overlay,
                OskPresentation::Overlay => OskPresentation::Shrink,
            };
            a.save_settings(store);
        }
        Act::LockOnDisconnect => {
            a.settings.lock_on_disconnect = !a.settings.lock_on_disconnect;
            a.save_settings(store);
        }
        Act::PeerSettings => {
            let peer = a.peer_id.clone();
            a.menu = Menu::None;
            app.go(Screen::PeerSettings(peer));
        }
        Act::Close => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycode_chars() {
        let none = KeyMods::default();
        let shift = KeyMods { shift: true, ..Default::default() };
        assert_eq!(keycode_char(Keycode::A, &none), Some('a'));
        assert_eq!(keycode_char(Keycode::A, &shift), Some('A'));
        assert_eq!(keycode_char(Keycode::Num1, &shift), Some('!'));
        assert_eq!(keycode_char(Keycode::F1, &none), None);
    }
}
