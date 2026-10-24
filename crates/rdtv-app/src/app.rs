//! Application state machine: screens, modals, and the single active session.

use crate::{
    audio_out::AudioOut,
    gfx::Gfx,
    keys::{In, Remote},
    osk::{Mode as OskMode, Osk, OskAction},
    session_view::ActiveSession,
    ui::{Frame, Nav, UiIn, UiState},
};
use rdtv_core::{
    profile::ServerProfile,
    reconnect::{FailureKind, ReconnectPlan},
    session::{self, SessionEvent, SessionRequest},
    store::{KeyboardLayout, Store},
    transport::Target,
};
use rdtv_media::{caps, ndl::HwCodec, pipeline::PipelineEvent};
use std::{
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    FirstRun,
    Home,
    Settings,
    Profiles,
    ProfileEdit(Box<ServerProfile>, bool /* is new */),
    PeerSettings(String),
    Diagnostics,
    About,
    Help,
    Session,
}

/// Where locally typed text goes.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldTarget {
    HomeTarget,
    Password,
    TwoFactor,
    Search,
    ProfileName,
    ProfileIdServer,
    ProfileRelay,
    ProfileApi,
    ProfileKey,
    ProfileImport,
    PeerAlias(String),
    AccountUser,
    AccountPassword,
    AccountCode,
}

pub struct Editor {
    pub target: FieldTarget,
    pub value: String,
    pub secret: bool,
    pub osk: Osk,
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    SwitchSession(Target, Option<String>),
    DeleteProfile(String),
    ClearHistory,
    ForgetPassword(String),
    Quit,
    SignOut,
}

pub enum Modal {
    Password {
        wrong: bool,
        value: String,
        remember: bool,
    },
    TwoFactor {
        wrong: bool,
        value: String,
    },
    Connecting {
        phase: session::Phase,
        since: Instant,
    },
    Reconnecting {
        plan: ReconnectPlan,
        next_at: Instant,
        attempt_running: bool,
        reason: String,
    },
    Failed {
        title: String,
        text: String,
        kind: FailureKind,
        retry: bool,
    },
    Confirm {
        text: String,
        action: ConfirmAction,
    },
    Notice {
        title: String,
        text: String,
    },
    /// After the app returned from the background where the default policy disconnected.
    ReconnectPrompt,
    PeerMenu(String),
    Account,
}

pub enum AppMsg {
    Session(u64, SessionEvent),
    Pipeline(u64, PipelineEvent),
}

pub struct App {
    pub store: Store,
    pub screen: Screen,
    pub prev_screen: Screen,
    pub modal: Option<Modal>,
    pub editor: Option<Editor>,
    pub ui: UiState,
    pub rt: rdtv_core::hbb_common::tokio::runtime::Runtime,
    pub tx: mpsc::Sender<AppMsg>,
    rx: mpsc::Receiver<AppMsg>,
    pub active: Option<ActiveSession>,
    pub platform: caps::Platform,
    pub failed_codecs: Vec<HwCodec>,
    pub home_target: String,
    pub search: String,
    pub quit: bool,
    pub start: Instant,
    pub log_path: std::path::PathBuf,
    pub focus_lost_at: Option<Instant>,
    pub audio_out: Option<AudioOut>,
    pub audio_error: Option<String>,
    pub sdl: sdl2::Sdl,
    pub profile_form: crate::screens::ProfileForm,
    pub account_form: crate::screens::AccountForm,
    pub last_session: Option<(String, Target)>,
    /// In-memory salted hash of the last successful login (for automatic reconnect).
    pub last_session_hash: Option<Vec<u8>>,
    pub toast: Option<(String, Instant)>,
    /// Experimental hardware picture placement (exported video window).
    pub hwplane: Option<crate::hwplane::HwPlane>,
    pub hwplane_error: Option<String>,
    pub updates: crate::updates::UpdateState,
    pub lan: LanScan,
}

/// Leaving the app: stop input at once; disconnect when focus has not come back after this
/// grace period (a transient system overlay does not count as leaving).
const BACKGROUND_GRACE: Duration = Duration::from_millis(2500);

/// LAN discovery state for the Home "Nearby" tab.
#[derive(Default)]
pub struct LanScan {
    slot: Option<std::sync::Arc<std::sync::Mutex<Option<Result<Vec<rdtv_core::lan::LanPeer>, String>>>>>,
    pub started: Option<Instant>,
    pub peers: Vec<rdtv_core::lan::LanPeer>,
    pub error: Option<String>,
    /// At least one scan has finished.
    pub done: bool,
}

impl LanScan {
    pub fn scanning(&self) -> bool {
        self.slot.is_some()
    }
}

impl App {
    pub fn new(sdl: sdl2::Sdl, store: Store, log_path: std::path::PathBuf) -> App {
        let rt = rdtv_core::hbb_common::tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("rdtv-net")
            .enable_all()
            .build()
            .expect("tokio runtime");
        let (tx, rx) = mpsc::channel();
        let platform = caps::detect();
        log::info!(
            "platform: webOS release {:?}, NDL: {}",
            platform.webos_release,
            platform.ndl_error.as_deref().unwrap_or("available")
        );
        let screen = if store.state.settings.first_run_done {
            Screen::Home
        } else {
            Screen::FirstRun
        };
        let mut ui = UiState::default();
        if screen == Screen::Home {
            ui.want_focus = Some(crate::ui::id("home-target"));
        }
        let mut app = App {
            store,
            screen: screen.clone(),
            prev_screen: screen,
            modal: None,
            editor: None,
            ui,
            rt,
            tx,
            rx,
            active: None,
            platform,
            failed_codecs: Vec::new(),
            home_target: String::new(),
            search: String::new(),
            quit: false,
            start: Instant::now(),
            log_path,
            focus_lost_at: None,
            audio_out: None,
            audio_error: None,
            sdl,
            profile_form: Default::default(),
            account_form: Default::default(),
            last_session: None,
            last_session_hash: None,
            toast: None,
            hwplane: None,
            hwplane_error: None,
            updates: Default::default(),
            lan: Default::default(),
        };
        if app.store.state.settings.check_updates {
            app.updates.start(&app.rt);
        }
        app
    }

    pub fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    pub fn go(&mut self, s: Screen) {
        if self.screen != s {
            if s == Screen::Home {
                self.ui.want_focus = Some(crate::ui::id("home-target"));
            }
            self.prev_screen = std::mem::replace(&mut self.screen, s);
            self.ui.reset = true;
        }
    }

    /// Start a LAN discovery scan unless one is running.
    pub fn start_lan_scan(&mut self) {
        if self.lan.slot.is_some() {
            return;
        }
        let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
        self.lan.slot = Some(slot.clone());
        self.lan.started = Some(Instant::now());
        self.rt.spawn_blocking(move || {
            let r =
                rdtv_core::lan::discover(std::time::Duration::from_millis(2500)).map_err(|e| e.to_string());
            if let Ok(mut g) = slot.lock() {
                *g = Some(r);
            }
        });
    }

    fn poll_lan(&mut self) {
        let Some(slot) = &self.lan.slot else { return };
        let Some(r) = slot.lock().ok().and_then(|mut g| g.take()) else {
            return;
        };
        self.lan.slot = None;
        match r {
            Ok(peers) => {
                log::info!("LAN discovery: {} computer(s)", peers.len());
                // Remember each computer's MAC (Wake-on-LAN) and name for its saved entry.
                let pid = self.store.state.active_profile.clone();
                let mut changed = false;
                for p in &peers {
                    if p.mac.is_empty() && p.hostname.is_empty() {
                        continue;
                    }
                    let r = self.store.peer_mut(&pid, &p.id);
                    if !p.mac.is_empty() && r.mac != p.mac {
                        r.mac = p.mac.clone();
                        changed = true;
                    }
                    if r.hostname.is_empty() && !p.hostname.is_empty() {
                        r.hostname = p.hostname.clone();
                        changed = true;
                    }
                    if r.platform.is_empty() && !p.platform.is_empty() {
                        r.platform = p.platform.clone();
                        changed = true;
                    }
                }
                if changed {
                    self.store.save();
                }
                self.lan.peers = peers;
                self.lan.error = None;
            }
            Err(e) => {
                log::warn!("LAN discovery failed: {e}");
                self.lan.error = Some(e);
            }
        }
        self.lan.done = true;
    }

    pub fn toast(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), Instant::now()));
    }

    pub fn hw_policy(&self) -> caps::HwPolicy {
        caps::HwPolicy {
            hardware: self.store.state.settings.prefer_hardware_decoding,
        }
    }

    pub fn open_editor(&mut self, target: FieldTarget, value: String, secret: bool) {
        let layout = KeyboardLayout::EnUs;
        self.editor = Some(Editor {
            target,
            value,
            secret,
            osk: Osk::new(OskMode::Local, layout),
        });
        self.ui.want_focus = Some(crate::ui::id("osk-enter"));
    }

    fn apply_edit(&mut self, target: &FieldTarget, value: String) {
        match target {
            FieldTarget::HomeTarget => self.home_target = value,
            FieldTarget::Search => self.search = value,
            FieldTarget::Password => {
                if let Some(Modal::Password { value: v, .. }) = self.modal.as_mut() {
                    *v = value;
                }
            }
            FieldTarget::TwoFactor => {
                if let Some(Modal::TwoFactor { value: v, .. }) = self.modal.as_mut() {
                    *v = value;
                }
            }
            FieldTarget::ProfileName => self.profile_form.name = value,
            FieldTarget::ProfileIdServer => self.profile_form.id_server = value,
            FieldTarget::ProfileRelay => self.profile_form.relay = value,
            FieldTarget::ProfileApi => self.profile_form.api = value,
            FieldTarget::ProfileKey => self.profile_form.key = value,
            FieldTarget::ProfileImport => self.profile_form.import = value,
            FieldTarget::PeerAlias(peer) => {
                let pid = self.store.state.active_profile.clone();
                self.store.peer_mut(&pid, peer).alias = value;
                self.store.save();
            }
            FieldTarget::AccountUser => self.account_form.user = value,
            FieldTarget::AccountPassword => self.account_form.password = value,
            FieldTarget::AccountCode => self.account_form.code = value,
        }
    }

    // ----- sessions --------------------------------------------------------------------

    /// Connect, ending any current session first (with confirmation when it is live).
    pub fn request_connect(&mut self, target_text: &str, password: Option<String>) {
        let Some(target) = Target::parse(target_text) else {
            self.modal = Some(Modal::Notice {
                title: "Invalid ID or address".into(),
                text: "Enter a RustDesk ID (digits) or an IP address / host:port the remote computer allows for direct access.".into(),
            });
            return;
        };
        if self.active.as_ref().is_some_and(|a| a.connected) {
            self.modal = Some(Modal::Confirm {
                text: format!("End the current session and connect to {}?", target.as_str()),
                action: ConfirmAction::SwitchSession(target, password),
            });
            return;
        }
        self.connect(target, password);
    }

    pub fn connect(&mut self, target: Target, password: Option<String>) {
        self.end_session(false, "Switching computer");
        let profile = self.store.active_profile().clone();
        let peer_id = target.as_str().to_owned();
        let rec = self.store.peer(&profile.id, &peer_id);
        let mut remembered = self.store.remembered_password(&profile.id, &peer_id);
        if remembered.is_none()
            && self
                .last_session
                .as_ref()
                .is_some_and(|(p, t)| *p == profile.id && *t == target)
        {
            remembered = self.last_session_hash.clone();
        } else if self
            .last_session
            .as_ref()
            .is_none_or(|(p, t)| *p != profile.id || *t != target)
        {
            self.last_session_hash = None;
        }
        let caps = caps::decode_caps(&self.platform, self.hw_policy(), &self.failed_codecs);
        let req = SessionRequest {
            profile: profile.clone(),
            target: target.clone(),
            password,
            remembered_hash: remembered,
            remember: false,
            settings: rec.settings.clone(),
            caps,
            audio_enabled: true,
            client_id: self.store.state.client_id.clone(),
            client_name: crate::platform::client_name(),
            force_relay: false,
        };
        let ring = Arc::new(rdtv_media::audio::AudioRing::new(250));
        ring.set_muted(rec.settings.muted);
        if self.audio_out.is_none() {
            match AudioOut::open(&self.sdl, ring.clone()) {
                Ok(a) => self.audio_out = Some(a),
                Err(e) => {
                    log::warn!("audio unavailable: {e}");
                    self.audio_error = Some(e);
                }
            }
        } else {
            // Re-open so the callback reads the new ring.
            self.audio_out = AudioOut::open(&self.sdl, ring.clone()).ok();
        }
        self.prepare_hwplane();
        let a = ActiveSession::start(self, req, ring, rec.settings);
        self.active = Some(a);
        self.last_session = Some((profile.id.clone(), target));
        self.modal = Some(Modal::Connecting {
            phase: session::Phase::Connecting,
            since: Instant::now(),
        });
    }

    /// Create (once) or drop the exported video window the Starfish decoder presents into.
    fn prepare_hwplane(&mut self) {
        let s = &self.store.state.settings;
        let wanted = crate::keys::is_webos()
            && s.hw_picture_placement
            && s.prefer_hardware_decoding
            && self.platform.ndl_available;
        if !wanted {
            self.hwplane = None;
            rdtv_media::ndl::set_target_window(None);
            return;
        }
        if self.hwplane.is_none() && self.hwplane_error.is_none() {
            match crate::hwplane::HwPlane::create() {
                Ok(p) => self.hwplane = Some(p),
                Err(e) => {
                    log::warn!("hardware picture placement unavailable: {e}");
                    self.hwplane_error = Some(e);
                }
            }
        }
        match self.hwplane.as_mut() {
            Some(p) => rdtv_media::ndl::set_target_window(Some(p.id())),
            None => rdtv_media::ndl::set_target_window(None),
        }
    }

    /// End the active session. `intentional` sessions are never retried.
    pub fn end_session(&mut self, lock: bool, why: &str) {
        if let Some(a) = self.active.take() {
            log::info!("ending session {} ({why})", a.handle.generation);
            a.stop(lock);
        }
        if matches!(
            self.modal,
            Some(Modal::Reconnecting { .. }) | Some(Modal::Connecting { .. })
        ) {
            self.modal = None;
        }
        if self.screen == Screen::Session {
            self.go(Screen::Home);
        }
    }

    fn start_reconnect_attempt(&mut self) {
        let Some((pid, target)) = self.last_session.clone() else {
            return;
        };
        if self.store.state.active_profile != pid {
            return;
        }
        // Reuse connect() but keep the reconnect modal.
        let plan_modal = self.modal.take();
        self.connect(target, None);
        if let Some(Modal::Reconnecting {
            plan,
            next_at,
            reason,
            ..
        }) = plan_modal
        {
            self.modal = Some(Modal::Reconnecting {
                plan,
                next_at,
                attempt_running: true,
                reason,
            });
        }
    }

    fn handle_session_event(&mut self, gen: u64, ev: SessionEvent) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if active.handle.generation != gen {
            return; // stale session
        }
        let pid = active.profile_id.clone();
        let peer_id = active.peer_id.clone();
        match ev {
            SessionEvent::Phase(p) => {
                if let Some(Modal::Connecting { phase, .. }) = self.modal.as_mut() {
                    *phase = p;
                }
            }
            SessionEvent::PasswordRequired { wrong } => {
                self.modal = Some(Modal::Password {
                    wrong,
                    value: String::new(),
                    // Remembering is the default; switch it off in the dialog for a one-off login.
                    remember: true,
                });
                self.ui.want_focus = Some(crate::ui::id("pw-field"));
            }
            SessionEvent::TwoFactorRequired { wrong } => {
                self.modal = Some(Modal::TwoFactor {
                    wrong,
                    value: String::new(),
                });
            }
            SessionEvent::RememberHash(h) => {
                self.store.remember_password(&pid, &peer_id, &h);
                self.store.save();
            }
            SessionEvent::ForgetHash => {
                self.store.forget_password(&pid, &peer_id);
                self.store.save();
            }
            SessionEvent::Connected {
                peer,
                route,
                secured,
                session_hash,
            } => {
                self.last_session_hash = session_hash;
                active.on_connected(peer.clone(), route, secured);
                self.store
                    .note_connected(&pid, &peer_id, &peer.hostname, &peer.platform);
                self.store.save();
                self.modal = None;
                self.go(Screen::Session);
            }
            SessionEvent::Ended(end) => {
                let was_reconnecting = matches!(self.modal, Some(Modal::Reconnecting { .. }));
                log::info!(
                    "session {gen} ended: {} ({:?}, intentional={})",
                    end.message,
                    end.kind,
                    end.intentional
                );
                if let Some(a) = self.active.take() {
                    a.stop(false);
                }
                if end.intentional {
                    if self.screen == Screen::Session {
                        self.go(Screen::Home);
                    }
                    return;
                }
                if self.screen == Screen::Session {
                    self.go(Screen::Home);
                }
                let retryable = end.kind.retryable() && (end.was_connected || was_reconnecting);
                if retryable {
                    let now = Instant::now();
                    let mut plan = match self.modal.take() {
                        Some(Modal::Reconnecting { plan, .. }) => plan,
                        _ => ReconnectPlan::start(now),
                    };
                    if let Some(next) = plan.next_attempt_at(now) {
                        self.modal = Some(Modal::Reconnecting {
                            plan,
                            next_at: next,
                            attempt_running: false,
                            reason: end.message.clone(),
                        });
                        return;
                    }
                }
                self.modal = Some(Modal::Failed {
                    title: end.kind.title().to_owned(),
                    text: friendly_error(&end.message, end.kind),
                    kind: end.kind,
                    retry: !matches!(
                        end.kind,
                        FailureKind::Auth | FailureKind::Key | FailureKind::IdNotFound
                    ),
                });
            }
            other => active.on_event(other, &mut self.store),
        }
    }

    fn handle_pipeline_event(&mut self, gen: u64, ev: PipelineEvent) {
        let policy = self.hw_policy();
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if active.handle.generation != gen {
            return;
        }
        match ev {
            PipelineEvent::CodecFailed { codec, reason } => {
                if let Some(hw) = codec.hw() {
                    if !self.failed_codecs.contains(&hw) {
                        self.failed_codecs.push(hw);
                    }
                }
                let caps = caps::decode_caps(&self.platform, policy, &self.failed_codecs);
                active.on_codec_failed(
                    codec,
                    &reason,
                    caps,
                    caps::vp9_in_hardware(&self.platform, policy, &self.failed_codecs),
                );
                self.toast(format!(
                    "{} decoding unavailable ({reason}); switching codec",
                    codec.name()
                ));
            }
            other => active.on_pipeline_event(other),
        }
    }

    // ----- frame -------------------------------------------------------------------------

    pub fn update(&mut self, inputs: Vec<In>) {
        self.updates.poll();
        self.poll_lan();
        while let Ok(m) = self.rx.try_recv() {
            match m {
                AppMsg::Session(g, e) => self.handle_session_event(g, e),
                AppMsg::Pipeline(g, e) => self.handle_pipeline_event(g, e),
            }
        }
        for i in inputs {
            self.handle_input(i);
        }
        // Background grace expiry.
        if let Some(t) = self.focus_lost_at {
            if t.elapsed() > BACKGROUND_GRACE && self.active.as_ref().is_some_and(|a| a.connected) {
                if self.store.state.settings.keep_connected_in_background {
                    if let Some(a) = self.active.as_mut() {
                        a.enter_background();
                    }
                } else {
                    self.end_session(
                        self.active
                            .as_ref()
                            .is_some_and(|a| a.settings.lock_on_disconnect),
                        "app left the foreground",
                    );
                    self.modal = Some(Modal::ReconnectPrompt);
                }
                self.focus_lost_at = None;
            }
        }
        // Reconnect schedule.
        let due = matches!(&self.modal, Some(Modal::Reconnecting { next_at, attempt_running: false, .. }) if Instant::now() >= *next_at);
        if due {
            self.start_reconnect_attempt();
        }
        if let Some(a) = self.active.as_mut() {
            a.tick();
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(5))
        {
            self.toast = None;
        }
    }

    fn handle_input(&mut self, i: In) {
        match &i {
            In::Quit => {
                self.end_session(false, "quit");
                self.quit = true;
                return;
            }
            In::Focus(false) => {
                log::info!("focus lost");
                if let Some(a) = self.active.as_mut() {
                    a.pause_input();
                }
                self.focus_lost_at = Some(Instant::now());
                return;
            }
            In::Focus(true) => {
                log::info!("focus gained");
                self.focus_lost_at = None;
                if let Some(a) = self.active.as_mut() {
                    a.resume_from_background();
                }
                return;
            }
            _ => {}
        }
        // Local text editor owns all input while open.
        if self.editor.is_some() {
            self.editor_input(i);
            return;
        }
        // An active session view gets first look when no local overlay is open.
        if self.screen == Screen::Session && self.modal.is_none() {
            if let Some(a) = self.active.as_mut() {
                if a.handle_input(&i, &mut self.ui, &mut self.store) {
                    return;
                }
            }
        }
        self.nav_input(i);
    }

    /// Generic local navigation for screens and modals.
    fn nav_input(&mut self, i: In) {
        match i {
            In::Remote { key, down: true, .. } => match key {
                Remote::Up => self.ui.push(UiIn::Nav(Nav::Up)),
                Remote::Down => self.ui.push(UiIn::Nav(Nav::Down)),
                Remote::Left => self.ui.push(UiIn::Nav(Nav::Left)),
                Remote::Right => self.ui.push(UiIn::Nav(Nav::Right)),
                Remote::Ok => self.ui.push(UiIn::Activate),
                Remote::Back => self.back(),
                Remote::ChUp => self.ui.push(UiIn::Wheel(5)),
                Remote::ChDown => self.ui.push(UiIn::Wheel(-5)),
                _ => {}
            },
            In::PointerMove { x, y } => self.ui.push(UiIn::PointerMove(x, y)),
            In::PointerButton {
                button: crate::keys::PtrButton::Left,
                down: false,
                x,
                y,
            } => self.ui.push(UiIn::Click(x, y)),
            In::Wheel { dy, .. } => self.ui.push(UiIn::Wheel(dy)),
            In::Key {
                key,
                down: true,
                mods,
                ..
            } => {
                use sdl2::keyboard::Keycode;
                match key {
                    Keycode::Escape | Keycode::AcBack => self.back(),
                    Keycode::Tab if mods.shift => self.ui.push(UiIn::Nav(Nav::Up)),
                    Keycode::Tab => self.ui.push(UiIn::Nav(Nav::Down)),
                    other => {
                        if let Some(d) =
                            crate::keys::digit_of(other).filter(|_| !(mods.ctrl || mods.alt || mods.gui))
                        {
                            self.digit_entry(d);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// A number button pressed outside a text editor: start editing the relevant field
    /// with that digit (the ID field on Home, the password/code field in dialogs, or the
    /// focused field), so IDs, passwords and codes can be typed on the remote's number pad.
    fn digit_entry(&mut self, d: char) {
        use crate::ui::id;
        let focused = self.ui.focused;
        let is = |name: &str| focused == Some(id(name));
        let target = match &self.modal {
            Some(Modal::Password { value, .. }) => Some((FieldTarget::Password, value.clone(), true)),
            Some(Modal::TwoFactor { value, .. }) => Some((FieldTarget::TwoFactor, value.clone(), false)),
            Some(Modal::Account) => {
                let f = &self.account_form;
                if is("acc-user") {
                    Some((FieldTarget::AccountUser, f.user.clone(), false))
                } else if is("acc-pass") {
                    Some((FieldTarget::AccountPassword, f.password.clone(), true))
                } else if f.challenge.is_some() {
                    Some((FieldTarget::AccountCode, f.code.clone(), false))
                } else {
                    None
                }
            }
            Some(_) => None,
            None => match &self.screen {
                Screen::Home if is("home-search") => Some((FieldTarget::Search, self.search.clone(), false)),
                Screen::Home => Some((FieldTarget::HomeTarget, self.home_target.clone(), false)),
                Screen::ProfileEdit(..) => {
                    let p = &self.profile_form;
                    [
                        ("pf-name", FieldTarget::ProfileName, &p.name),
                        ("pf-id", FieldTarget::ProfileIdServer, &p.id_server),
                        ("pf-relay", FieldTarget::ProfileRelay, &p.relay),
                        ("pf-api", FieldTarget::ProfileApi, &p.api),
                        ("pf-key", FieldTarget::ProfileKey, &p.key),
                        ("pf-import", FieldTarget::ProfileImport, &p.import),
                    ]
                    .into_iter()
                    .find(|(n, _, _)| is(n))
                    .map(|(_, t, v)| (t, v.clone(), false))
                }
                _ => None,
            },
        };
        if let Some((target, value, secret)) = target {
            self.open_editor(target, value, secret);
            if let Some(ed) = self.editor.as_mut() {
                ed.value.push(d);
            }
        }
    }

    fn editor_input(&mut self, i: In) {
        match i {
            In::Remote {
                key: Remote::Back,
                down: true,
                ..
            } => {
                self.editor = None;
            }
            In::Key {
                key,
                down: true,
                mods,
                ..
            } => {
                use sdl2::keyboard::Keycode;
                let Some(ed) = self.editor.as_mut() else { return };
                match key {
                    Keycode::Backspace => {
                        ed.value.pop();
                    }
                    Keycode::Escape => self.editor = None,
                    Keycode::Return | Keycode::KpEnter => self.finish_editor(true),
                    _ => {
                        if let Some(c) = crate::session_view::keycode_char(key, &mods) {
                            if ed.value.chars().count() < 512 {
                                ed.value.push(c);
                            }
                        }
                    }
                }
            }
            In::Remote {
                key: Remote::Ok,
                down: true,
                ..
            } if false => {}
            other => self.nav_input(other),
        }
    }

    pub fn editor_actions(&mut self, acts: Vec<OskAction>) {
        for a in acts {
            let Some(ed) = self.editor.as_mut() else { return };
            match a {
                OskAction::Text(t) => {
                    if ed.value.chars().count() < 512 && t != "\t" {
                        ed.value.push_str(&t);
                    }
                }
                OskAction::Backspace => {
                    ed.value.pop();
                }
                OskAction::Submit => self.finish_editor(true),
                OskAction::Close => self.finish_editor(false),
                _ => {}
            }
        }
    }

    pub fn finish_editor(&mut self, submit: bool) {
        let Some(ed) = self.editor.take() else { return };
        let target = ed.target.clone();
        self.apply_edit(&target, ed.value.clone());
        if submit {
            match target {
                FieldTarget::HomeTarget => {
                    let t = self.home_target.clone();
                    self.request_connect(&t, None);
                }
                FieldTarget::Password => self.submit_password(),
                FieldTarget::TwoFactor => self.submit_2fa(),
                _ => {}
            }
        }
    }

    pub fn submit_password(&mut self) {
        if let Some(Modal::Password { value, remember, .. }) = self.modal.take() {
            if let Some(a) = self.active.as_ref() {
                a.handle.send(session::SessionCommand::Password {
                    password: value,
                    remember,
                });
                self.modal = Some(Modal::Connecting {
                    phase: session::Phase::LoggingIn,
                    since: Instant::now(),
                });
            }
        }
    }

    pub fn submit_2fa(&mut self) {
        if let Some(Modal::TwoFactor { value, .. }) = self.modal.take() {
            if let Some(a) = self.active.as_ref() {
                a.handle.send(session::SessionCommand::TwoFactor { code: value });
                self.modal = Some(Modal::Connecting {
                    phase: session::Phase::LoggingIn,
                    since: Instant::now(),
                });
            }
        }
    }

    /// Short Back: close the topmost local panel first; never sends anything remote.
    pub fn back(&mut self) {
        if self.editor.is_some() {
            self.editor = None;
            return;
        }
        if let Some(m) = self.modal.take() {
            match m {
                Modal::Connecting { .. } | Modal::Password { .. } | Modal::TwoFactor { .. } => {
                    self.end_session(false, "cancelled");
                }
                Modal::Reconnecting { .. } => {
                    self.end_session(false, "reconnect cancelled");
                }
                _ => {}
            }
            return;
        }
        match self.screen {
            Screen::Home => {
                self.modal = Some(Modal::Confirm {
                    text: "Exit RemoteDesk TV?".into(),
                    action: ConfirmAction::Quit,
                });
            }
            Screen::Session => {
                if let Some(a) = self.active.as_mut() {
                    a.toggle_toolbar();
                }
            }
            Screen::ProfileEdit(..) => self.go(Screen::Profiles),
            Screen::Profiles | Screen::Diagnostics | Screen::About | Screen::Help => {
                self.go(Screen::Settings)
            }
            Screen::PeerSettings(_) | Screen::Settings => {
                if self.active.as_ref().is_some_and(|a| a.connected) {
                    self.go(Screen::Session)
                } else {
                    self.go(Screen::Home)
                }
            }
            Screen::FirstRun => {}
        }
    }

    pub fn do_confirm(&mut self, a: ConfirmAction) {
        match a {
            ConfirmAction::SwitchSession(t, p) => self.connect(t, p),
            ConfirmAction::DeleteProfile(id) => {
                if self.active.as_ref().is_some_and(|a| a.profile_id == id) {
                    self.end_session(false, "profile deleted");
                }
                self.store.delete_profile(&id);
                self.store.save();
                self.go(Screen::Profiles);
            }
            ConfirmAction::ClearHistory => {
                let pid = self.store.state.active_profile.clone();
                self.store.clear_history(&pid);
                self.store.save();
                self.toast("History cleared for this server profile");
            }
            ConfirmAction::ForgetPassword(peer) => {
                let pid = self.store.state.active_profile.clone();
                self.store.forget_password(&pid, &peer);
                self.store.save();
                self.toast("Saved password removed");
            }
            ConfirmAction::Quit => {
                self.end_session(false, "quit");
                self.quit = true;
            }
            ConfirmAction::SignOut => crate::screens::sign_out(self),
        }
    }

    /// Switch the active server profile (ends any session on the old profile first).
    pub fn switch_profile(&mut self, id: &str) {
        if self.store.state.active_profile == id {
            return;
        }
        if self.active.is_some() {
            self.end_session(false, "server profile switched");
        }
        self.store.state.active_profile = id.to_owned();
        self.store.save();
        self.toast(format!("Server: {}", self.store.active_profile().name));
    }

    pub fn draw(&mut self, g: &mut Gfx) {
        let now = self.now_ms();
        let mut ui = std::mem::take(&mut self.ui);
        {
            let mut f = Frame::begin(g, &mut ui, now);
            let overlay = self.modal.is_some() || self.editor.is_some();
            if self.screen == Screen::Session {
                crate::session_view::draw(self, &mut f, overlay);
            } else {
                f.g.clear(crate::gfx::theme::BG);
                f.set_inert(overlay);
                crate::screens::draw_screen(self, &mut f);
            }
            if self.modal.is_some() {
                f.set_inert(self.editor.is_some());
                crate::screens::draw_modal(self, &mut f);
            }
            if self.editor.is_some() {
                f.set_inert(false);
                crate::screens::draw_editor(self, &mut f);
            }
            crate::screens::draw_toast(self, &mut f);
            f.end();
        }
        // Focus requests raised while drawing landed on the placeholder state.
        let during = std::mem::replace(&mut self.ui, ui);
        if during.reset {
            self.ui.reset = true;
        }
        if during.want_focus.is_some() {
            self.ui.want_focus = during.want_focus;
        }
    }
}

pub fn friendly_error(msg: &str, kind: FailureKind) -> String {
    match kind {
        FailureKind::IdNotFound => {
            "The server does not know this ID. Check the ID and that the right server profile is selected."
                .into()
        }
        FailureKind::PeerOffline => "The remote computer is offline or not connected to this server.".into(),
        FailureKind::Key => {
            format!("The server rejected the key ({msg}). Check the key in the server profile.")
        }
        FailureKind::Auth => format!("{msg}. Check the password."),
        FailureKind::Server => {
            format!("Could not reach the server: {msg}. Check the network and the ID server address.")
        }
        FailureKind::ConnectionLost => format!("The connection was lost: {msg}."),
        FailureKind::Codec => format!("The video stream could not be decoded: {msg}."),
        FailureKind::Platform => format!("A TV playback component failed: {msg}."),
        FailureKind::Refused => format!("The remote computer refused: {msg}."),
        FailureKind::Other => msg.to_owned(),
    }
}
