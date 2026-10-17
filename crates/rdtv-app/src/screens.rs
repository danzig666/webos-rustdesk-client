//! Screens and dialogs (everything except the live session view).

use crate::{
    app::{App, ConfirmAction, FieldTarget, Modal, Screen},
    gfx::{size, theme, H, W},
    ui::{id, idx, Frame},
};
use rdtv_core::{
    account::{self, LoginOutcome},
    profile::{ProfileKind, ServerProfile},
    reconnect::FailureKind,
    session::Phase,
    store::{CodecPreference, DpadMode, KeyboardLayout, OskPresentation, Quality},
    transport::Target,
};
use sdl2::{pixels::Color, rect::Rect};
use std::time::Instant;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const UPSTREAM: &str = "RustDesk 1.5.0 (fada664df7a294d1d1a9ca3e7cd3637069122f17)";

#[derive(Default, Clone)]
pub struct ProfileForm {
    pub name: String,
    pub id_server: String,
    pub relay: String,
    pub api: String,
    pub key: String,
    pub import: String,
    pub error: String,
}

#[derive(Default, Clone)]
pub struct AccountForm {
    pub user: String,
    pub password: String,
    pub code: String,
    pub challenge: Option<(String, bool)>,
    pub busy: bool,
    pub status: String,
    pub result: Option<std::sync::Arc<std::sync::Mutex<Option<AccountResult>>>>,
    pub home_tab: usize,
}

#[derive(Debug, Clone)]
pub enum AccountResult {
    Login(Result<LoginOutcome, String>),
    Book(Result<(String, Vec<account::AbPeer>), String>),
}

fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect::new(x, y, w.max(1) as u32, h.max(1) as u32)
}

fn header(f: &mut Frame, title: &str, subtitle: &str) {
    f.g.text(title, 80, 50, size::TITLE, true, theme::TEXT);
    if !subtitle.is_empty() {
        f.g.text(subtitle, 82, 112, size::BODY, false, theme::TEXT_DIM);
    }
}

fn footer(f: &mut Frame, s: &str) {
    f.g.text(s, 80, H - 60, size::SMALL, false, theme::TEXT_DIM);
}

pub fn draw_screen(app: &mut App, f: &mut Frame) {
    poll_account(app);
    match app.screen.clone() {
        Screen::FirstRun => first_run(app, f),
        Screen::Home => home(app, f),
        Screen::Settings => settings(app, f),
        Screen::Profiles => profiles(app, f),
        Screen::ProfileEdit(p, is_new) => profile_edit(app, f, *p, is_new),
        Screen::PeerSettings(peer) => peer_settings(app, f, &peer),
        Screen::Diagnostics => diagnostics(app, f),
        Screen::About => about(app, f),
        Screen::Help => help(app, f),
        Screen::Session => {}
    }
}

// ----- first run --------------------------------------------------------------------------

fn first_run(app: &mut App, f: &mut Frame) {
    header(
        f,
        "Welcome to RemoteDesk TV",
        "A community RustDesk-compatible client for controlling your computer from the TV.",
    );
    let text = "Install the regular RustDesk app on your computer and note its ID and password. \
This TV only connects out to your computer; nothing on the TV can be controlled remotely.\n\n\
Buttons during a session: point with the Magic Remote and press OK to click. Red = right click, \
Green = next monitor, Yellow = switch the arrow buttons between mouse and arrow keys, \
Blue = keyboard, Back = session menu. Every action is also available from the on-screen menu.\n\n\
Choose which RustDesk server your computer uses. You can add more servers later in Settings.";
    f.panel(r(80, 170, 1760, 560));
    f.g.paragraph(text, 120, 200, 1680, size::BODY, theme::TEXT);
    if f.button(
        id("fr-public"),
        r(120, 780, 760, 90),
        "Use the RustDesk public server",
        true,
    ) {
        app.store.state.active_profile = "public".into();
        app.store.state.settings.first_run_done = true;
        app.store.save();
        app.go(Screen::Home);
    }
    if f.button(
        id("fr-self"),
        r(920, 780, 760, 90),
        "Add my self-hosted server…",
        false,
    ) {
        app.store.state.settings.first_run_done = true;
        app.store.save();
        let pid = app.store.new_profile_id();
        open_profile_editor(app, ServerProfile::self_hosted(pid, "My server"), true);
    }
    footer(f, "Use the arrow buttons and OK, or point with the Magic Remote.");
}

// ----- home -------------------------------------------------------------------------------

struct HomeItem {
    id: String,
    title: String,
    subtitle: String,
    favorite: bool,
}

fn home(app: &mut App, f: &mut Frame) {
    let profile = app.store.active_profile().clone();
    f.g.text("RemoteDesk", 80, 44, size::TITLE, true, theme::TEXT);
    let w = f.g.text_width("RemoteDesk", size::TITLE, true);
    f.g.text(" TV", 80 + w, 44, size::TITLE, true, theme::ACCENT2);
    // Header buttons.
    let srv = format!("Server: {}", profile.name);
    let sw = (f.g.text_width(&srv, size::BODY, true) + 60).min(620);
    if f.button(id("home-server"), r(W - 80 - sw - 340, 40, sw, 70), &srv, false) {
        app.go(Screen::Profiles);
    }
    if f.button(
        id("home-settings"),
        r(W - 80 - 320, 40, 160, 70),
        "Settings",
        false,
    ) {
        app.go(Screen::Settings);
    }
    if f.button(id("home-help"), r(W - 80 - 150, 40, 150, 70), "Help", false) {
        app.go(Screen::Help);
    }

    // Connect panel.
    f.panel(r(80, 150, 640, 800));
    f.g.text("Connect to a computer", 120, 180, size::LARGE, true, theme::TEXT);
    let target = app.home_target.clone();
    if f.field(
        id("home-target"),
        r(120, 250, 560, 110),
        "Computer ID or address",
        &target,
        false,
        "e.g. 123 456 789",
        false,
    ) {
        app.open_editor(FieldTarget::HomeTarget, target.clone(), false);
    }
    if f.button(id("home-connect"), r(120, 390, 560, 90), "Connect", true) {
        if app.home_target.trim().is_empty() {
            app.open_editor(FieldTarget::HomeTarget, String::new(), false);
        } else {
            let t = app.home_target.clone();
            app.request_connect(&t, None);
        }
    }
    f.g.paragraph(
        "Use the ID shown by RustDesk on the computer, or its IP address when direct IP access is enabled there.",
        120, 510, 560, size::SMALL, theme::TEXT_DIM,
    );
    let pdata = app.store.profile_data(&profile.id);
    let acct = if app.store.token(&profile.id).is_some() {
        format!(
            "Signed in{}",
            if pdata.account_name.is_empty() {
                String::new()
            } else {
                format!(" as {}", pdata.account_name)
            }
        )
    } else {
        "Not signed in".to_owned()
    };
    f.g.text_fit(&acct, 120, 700, size::BODY, false, theme::TEXT, 560);
    if f.button(
        id("home-account"),
        r(120, 750, 560, 80),
        "Account & address book",
        false,
    ) {
        app.modal = Some(Modal::Account);
    }
    if let Some((_, t)) = &app.last_session {
        let label = format!("Reconnect to {}", t.as_str());
        if f.button(id("home-last"), r(120, 850, 560, 80), &label, false) {
            let t = t.as_str().to_owned();
            app.request_connect(&t, None);
        }
    }

    // Lists.
    let tabs = ["Favorites", "Recent", "Address book"];
    let tab = app.account_form.home_tab.min(2);
    let mut tx = 780;
    for (i, name) in tabs.iter().enumerate() {
        let tw = f.g.text_width(name, size::BODY, true) + 48;
        let rr = r(tx, 150, tw, 70);
        let (foc, act) = f.register(idx("home-tab", i), rr);
        f.g.round(
            rr,
            35,
            if i == tab {
                theme::ACCENT
            } else if foc {
                theme::PANEL_HI
            } else {
                theme::ROW
            },
        );
        if foc {
            f.focus_ring(rr, 35);
        }
        f.g.text_centered(name, rr, size::BODY, true, theme::TEXT);
        if act || (foc && i != tab && f.st.pointer.is_none()) {
            app.account_form.home_tab = i;
        }
        tx += tw + 16;
    }
    let search = app.search.clone();
    if f.field(
        id("home-search"),
        r(1520, 140, 320, 90),
        "Search",
        &search,
        false,
        "Name or ID",
        false,
    ) {
        app.open_editor(FieldTarget::Search, search.clone(), false);
    }
    let q = app.search.to_lowercase();
    let mut items: Vec<HomeItem> = Vec::new();
    let saved = |id: &str| app.store.remembered_password(&profile.id, id).is_some();
    match tab {
        0 | 1 => {
            let ids: Vec<String> = if tab == 0 {
                pdata
                    .peers
                    .iter()
                    .filter(|(_, p)| p.favorite)
                    .map(|(k, _)| k.clone())
                    .collect()
            } else {
                pdata.recents.clone()
            };
            for pid in ids {
                let rec = pdata.peers.get(&pid).cloned().unwrap_or_default();
                let title = if rec.alias.is_empty() {
                    pid.clone()
                } else {
                    rec.alias.clone()
                };
                let mut sub = vec![format!("ID {pid}")];
                if !rec.hostname.is_empty() {
                    sub.push(rec.hostname.clone());
                }
                if !rec.platform.is_empty() {
                    sub.push(rec.platform.clone());
                }
                if saved(&pid) {
                    sub.push("password saved".into());
                }
                items.push(HomeItem {
                    id: pid,
                    title,
                    subtitle: sub.join(" · "),
                    favorite: rec.favorite,
                });
            }
        }
        _ => {
            for p in &pdata.address_book {
                let title = if p.alias.is_empty() {
                    if p.hostname.is_empty() {
                        p.id.clone()
                    } else {
                        p.hostname.clone()
                    }
                } else {
                    p.alias.clone()
                };
                let mut sub = vec![format!("ID {}", p.id)];
                if !p.platform.is_empty() {
                    sub.push(p.platform.clone());
                }
                if !p.tags.is_empty() {
                    sub.push(p.tags.join(", "));
                }
                let fav = pdata.peers.get(&p.id).is_some_and(|r| r.favorite);
                items.push(HomeItem {
                    id: p.id.clone(),
                    title,
                    subtitle: sub.join(" · "),
                    favorite: fav,
                });
            }
        }
    }
    if !q.is_empty() {
        items.retain(|i| {
            i.title.to_lowercase().contains(&q)
                || i.id.to_lowercase().contains(&q)
                || i.subtitle.to_lowercase().contains(&q)
        });
    }
    let list_y = 250;
    let row_h = 104;
    let gap = 14;
    let visible = 6;
    let focused = items
        .iter()
        .enumerate()
        .position(|(i, _)| f.is_focused(idx("home-row", i)) || f.is_focused(idx("home-opt", i)));
    let first = f.list_scroll(id("home-list"), focused, visible, items.len());
    if items.is_empty() {
        let msg = match tab {
            0 => "No favorites yet. Open ⋯ next to a recent computer to add it.",
            1 => "No recent connections on this server profile yet.",
            _ => {
                if app.store.token(&profile.id).is_some() {
                    "The address book is empty or not loaded. Open Account & address book to refresh."
                } else {
                    "Sign in (Account & address book) to see your address book, if your server provides one."
                }
            }
        };
        f.g.paragraph(msg, 790, list_y + 40, 1040, size::BODY, theme::TEXT_DIM);
    }
    for (i, it) in items.iter().enumerate().skip(first).take(visible) {
        let y = list_y + (i - first) as i32 * (row_h + gap);
        let badge = if it.favorite {
            Some(("★ Favorite", Color::RGB(120, 90, 10)))
        } else {
            None
        };
        if f.row(
            idx("home-row", i),
            r(780, y, 940, row_h),
            &it.title,
            &it.subtitle,
            badge,
        ) {
            let pid = it.id.clone();
            app.request_connect(&pid, None);
        }
        if f.button(idx("home-opt", i), r(1736, y, 104, row_h), "⋯", false) {
            app.modal = Some(Modal::PeerMenu(it.id.clone()));
        }
    }
    if items.len() > visible {
        f.g.text(
            &format!(
                "{}-{} of {}",
                first + 1,
                (first + visible).min(items.len()),
                items.len()
            ),
            790,
            list_y + visible as i32 * (row_h + gap) + 4,
            size::SMALL,
            false,
            theme::TEXT_DIM,
        );
    }
    footer(f, "0-9: type an ID · OK: connect · ⋯: options · Back: exit");
}

// ----- settings ---------------------------------------------------------------------------

fn settings(app: &mut App, f: &mut Frame) {
    header(f, "Settings", "");
    let s = app.store.state.settings.clone();
    let profile = app.store.active_profile().clone();
    let scale_label = format!("{:.0}%", s.ui_scale * 100.0);
    let rows: Vec<(&str, String, u8)> = vec![
        (
            "set-profiles",
            format!("Server profiles (active: {})", profile.name),
            0,
        ),
        ("set-account", "Account & address book (this server)".into(), 0),
        (
            "set-bg",
            "Keep connected in background".into(),
            if s.keep_connected_in_background { 2 } else { 1 },
        ),
        (
            "set-hw",
            "Hardware video decoding (recommended)".into(),
            if s.prefer_hardware_decoding { 2 } else { 1 },
        ),
        (
            "set-place",
            "Hardware picture placement: shrink/zoom with hardware decoding (experimental)".into(),
            if s.hw_picture_placement { 2 } else { 1 },
        ),
        (
            "set-chord",
            "Keyboard shortcut Ctrl+Alt+Shift+Space opens session menu".into(),
            if s.escape_chord_enabled { 2 } else { 1 },
        ),
        (
            "set-motion",
            "Reduced motion".into(),
            if s.reduced_motion { 2 } else { 1 },
        ),
        ("set-scale", format!("Interface size: {scale_label}"), 0),
        ("set-clear", "Clear history for this server".into(), 0),
        (
            "set-export",
            "Export settings (passwords and tokens excluded)".into(),
            0,
        ),
        ("set-diag", "Diagnostics".into(), 0),
        ("set-help", "Buttons & help".into(), 0),
        ("set-about", "About & licenses".into(), 0),
        ("set-first", "Show the welcome guide again".into(), 0),
    ];
    let visible = 7;
    let focused = rows.iter().position(|(k, _, _)| f.is_focused(id(k)));
    let first = f.list_scroll(id("set-list"), focused, visible, rows.len());
    let mut act = None;
    for (i, (k, label, kind)) in rows.iter().enumerate().skip(first).take(visible) {
        let rr = r(80, 170 + (i - first) as i32 * 112, 1760, 96);
        let a = match kind {
            0 => f.choice(id(k), rr, label, ""),
            n => f.toggle(id(k), rr, label, *n == 2),
        };
        if a {
            act = Some(*k);
        }
    }
    footer(f, "Back: return");
    let Some(k) = act else { return };
    let st = &mut app.store.state.settings;
    match k {
        "set-profiles" => app.go(Screen::Profiles),
        "set-account" => app.modal = Some(Modal::Account),
        "set-bg" => st.keep_connected_in_background = !st.keep_connected_in_background,
        "set-hw" => {
            st.prefer_hardware_decoding = !st.prefer_hardware_decoding;
            app.failed_codecs.clear();
        }
        "set-place" => {
            st.hw_picture_placement = !st.hw_picture_placement;
            app.hwplane_error = None;
        }
        "set-chord" => st.escape_chord_enabled = !st.escape_chord_enabled,
        "set-motion" => st.reduced_motion = !st.reduced_motion,
        "set-scale" => {
            st.ui_scale = match (st.ui_scale * 100.0).round() as i32 {
                85 => 1.0,
                100 => 1.15,
                115 => 1.3,
                _ => 0.85,
            };
        }
        "set-clear" => {
            app.modal = Some(Modal::Confirm {
                text: format!(
                    "Clear recent computers for \"{}\"? Favorites are kept.",
                    profile.name
                ),
                action: ConfirmAction::ClearHistory,
            })
        }
        "set-export" => {
            let path = app.store.dir().join("settings-export.json");
            match std::fs::write(&path, app.store.export(false)) {
                Ok(()) => app.toast(format!("Exported to {}", path.display())),
                Err(e) => app.toast(format!("Export failed: {e}")),
            }
        }
        "set-diag" => app.go(Screen::Diagnostics),
        "set-help" => app.go(Screen::Help),
        "set-about" => app.go(Screen::About),
        "set-first" => app.go(Screen::FirstRun),
        _ => {}
    }
    app.store.save();
}

// ----- profiles ---------------------------------------------------------------------------

pub fn open_profile_editor(app: &mut App, p: ServerProfile, is_new: bool) {
    app.profile_form = ProfileForm {
        name: p.name.clone(),
        id_server: p.id_server.clone(),
        relay: p.relay_server.clone(),
        api: p.api_server.clone(),
        key: p.key.clone(),
        import: String::new(),
        error: String::new(),
    };
    app.go(Screen::ProfileEdit(Box::new(p), is_new));
}

fn profiles(app: &mut App, f: &mut Frame) {
    header(
        f,
        "Server profiles",
        "Each profile keeps its own favorites, history, passwords and account.",
    );
    let list = app.store.state.profiles.clone();
    let active = app.store.state.active_profile.clone();
    let mut y = 180;
    for (i, p) in list.iter().enumerate() {
        let sub = match p.kind {
            ProfileKind::Public => "RustDesk public server".to_owned(),
            ProfileKind::SelfHosted => format!(
                "ID server {}{}",
                p.id_server,
                if p.key.is_empty() {
                    " · no key set"
                } else {
                    " · key set"
                }
            ),
        };
        let badge = if p.id == active {
            Some(("Active", theme::ACCENT))
        } else {
            None
        };
        if f.row(idx("prof-row", i), r(80, y, 1460, 100), &p.name, &sub, badge) {
            let pid = p.id.clone();
            app.switch_profile(&pid);
        }
        if p.kind == ProfileKind::SelfHosted
            && f.button(idx("prof-edit", i), r(1560, y, 280, 100), "Edit", false)
        {
            open_profile_editor(app, p.clone(), false);
        }
        y += 116;
        if y > H - 260 {
            break;
        }
    }
    if f.button(
        id("prof-add"),
        r(80, H - 200, 700, 90),
        "+ Add self-hosted server",
        true,
    ) {
        let pid = app.store.new_profile_id();
        open_profile_editor(app, ServerProfile::self_hosted(pid, "My server"), true);
    }
    footer(f, "OK on a profile: make it active · Back: return");
}

fn profile_edit(app: &mut App, f: &mut Frame, p: ServerProfile, is_new: bool) {
    header(
        f,
        if is_new {
            "Add self-hosted server"
        } else {
            "Edit server"
        },
        "Values are the same as in RustDesk's ID/Relay server settings.",
    );
    let pf = app.profile_form.clone();
    let fields: [(&str, &str, String, FieldTarget, &str); 6] = [
        (
            "pf-name",
            "Name",
            pf.name.clone(),
            FieldTarget::ProfileName,
            "e.g. Home server",
        ),
        (
            "pf-id",
            "ID server (host or host:port)",
            pf.id_server.clone(),
            FieldTarget::ProfileIdServer,
            "rd.example.com",
        ),
        (
            "pf-relay",
            "Relay server (optional)",
            pf.relay.clone(),
            FieldTarget::ProfileRelay,
            "usually not needed",
        ),
        (
            "pf-api",
            "API server (optional, for accounts)",
            pf.api.clone(),
            FieldTarget::ProfileApi,
            "https://rd.example.com",
        ),
        (
            "pf-key",
            "Key (public key of the server)",
            pf.key.clone(),
            FieldTarget::ProfileKey,
            "44-character key from id_ed25519.pub",
        ),
        (
            "pf-import",
            "Or paste a RustDesk server config string",
            pf.import.clone(),
            FieldTarget::ProfileImport,
            "optional",
        ),
    ];
    for (i, (k, label, val, target, ph)) in fields.into_iter().enumerate() {
        let col = (i % 2) as i32;
        let row = (i / 2) as i32;
        let rr = r(80 + col * 900, 170 + row * 140, 860, 110);
        if f.field(id(k), rr, label, &val, false, ph, false) {
            app.open_editor(target, val, false);
        }
    }
    f.g.paragraph(
        "Trust: the key lets this TV verify that computers on this server are who they claim to be. Without a key, connections still work but are not verified end to end.",
        80, 600, 1760, size::SMALL, theme::TEXT_DIM,
    );
    if !pf.error.is_empty() {
        f.g.text_fit(&pf.error, 80, 690, size::BODY, true, theme::DANGER, 1760);
    }
    let mut save = false;
    if f.button(id("pf-save"), r(80, 760, 400, 90), "Save", true) {
        save = true;
    }
    if f.button(id("pf-cancel"), r(500, 760, 300, 90), "Cancel", false) {
        app.go(Screen::Profiles);
    }
    if !is_new && f.button(id("pf-delete"), r(820, 760, 360, 90), "Delete profile", false) {
        app.modal = Some(Modal::Confirm {
            text: format!(
                "Delete \"{}\" and all its favorites, history and saved passwords?",
                p.name
            ),
            action: ConfirmAction::DeleteProfile(p.id.clone()),
        });
    }
    if !save {
        return;
    }
    let mut np = p.clone();
    if !pf.import.trim().is_empty() {
        match ServerProfile::from_upstream_config_string(&p.id, &pf.import) {
            Some(imp) => {
                np = imp;
                if !pf.name.trim().is_empty() && pf.name != "My server" {
                    np.name = pf.name.clone();
                }
            }
            None => {
                app.profile_form.error = "The config string could not be read.".into();
                return;
            }
        }
    } else {
        np.name = pf.name.trim().to_owned();
        np.id_server = pf.id_server.trim().to_owned();
        np.relay_server = pf.relay.trim().to_owned();
        np.api_server = pf.api.trim().to_owned();
        np.key = pf.key.trim().to_owned();
    }
    if let Err(e) = np.validate() {
        app.profile_form.error = e;
        return;
    }
    let changed_active = app.store.state.active_profile == np.id && app.active.is_some();
    if let Some(slot) = app.store.state.profiles.iter_mut().find(|x| x.id == np.id) {
        *slot = np.clone();
    } else {
        app.store.state.profiles.push(np.clone());
    }
    if changed_active {
        app.end_session(false, "server profile changed");
    }
    if is_new {
        app.store.state.active_profile = np.id.clone();
    }
    app.store.save();
    app.go(if is_new { Screen::Home } else { Screen::Profiles });
}

// ----- per-computer settings --------------------------------------------------------------

fn peer_settings(app: &mut App, f: &mut Frame, peer: &str) {
    let pid = app.store.state.active_profile.clone();
    let rec = app.store.peer(&pid, peer);
    let s = rec.settings.clone();
    let title = if rec.alias.is_empty() {
        peer.to_owned()
    } else {
        format!("{} ({peer})", rec.alias)
    };
    header(
        f,
        &format!("Computer settings: {title}"),
        "Saved for this computer on this server profile.",
    );
    let saved = app.store.remembered_password(&pid, peer).is_some();
    let rows: Vec<(&str, String, String, u8)> = vec![
        (
            "ps-alias",
            "Name".into(),
            if rec.alias.is_empty() {
                "(none)".into()
            } else {
                rec.alias.clone()
            },
            0,
        ),
        (
            "ps-fav",
            "Favorite".into(),
            String::new(),
            if rec.favorite { 2 } else { 1 },
        ),
        (
            "ps-quality",
            "Picture quality".into(),
            match s.quality {
                Quality::Balanced => "Balanced",
                Quality::Best => "Best",
                Quality::Low => "Low",
            }
            .into(),
            0,
        ),
        (
            "ps-fps",
            "Frame rate".into(),
            if s.fps == 0 {
                "Automatic".into()
            } else {
                format!("{} fps", s.fps)
            },
            0,
        ),
        (
            "ps-codec",
            "Preferred codec".into(),
            match s.codec {
                CodecPreference::Auto => "Automatic",
                CodecPreference::H265 => "H.265",
                CodecPreference::H264 => "H.264",
                CodecPreference::Vp9 => "VP9",
                CodecPreference::Vp8 => "VP8",
            }
            .into(),
            0,
        ),
        (
            "ps-lock",
            "Lock remote computer on disconnect".into(),
            String::new(),
            if s.lock_on_disconnect { 2 } else { 1 },
        ),
        (
            "ps-mute",
            "Mute audio".into(),
            String::new(),
            if s.muted { 2 } else { 1 },
        ),
        (
            "ps-layout",
            "Keyboard layout".into(),
            match s.keyboard_layout {
                KeyboardLayout::EnUs => "English (US)",
                KeyboardLayout::Hu => "Hungarian",
            }
            .into(),
            0,
        ),
        (
            "ps-osk",
            "Keyboard presentation".into(),
            match s.osk_presentation {
                OskPresentation::Shrink => "Shrink picture",
                OskPresentation::Overlay => "Overlay picture",
            }
            .into(),
            0,
        ),
        (
            "ps-dpad",
            "Arrow buttons".into(),
            match s.dpad_mode {
                DpadMode::Mouse => "Move the mouse",
                DpadMode::Arrows => "Arrow keys",
            }
            .into(),
            0,
        ),
        (
            "ps-cursor",
            "Show the computer's mouse cursor".into(),
            String::new(),
            if s.show_remote_cursor { 2 } else { 1 },
        ),
        (
            "ps-short",
            "Reset keyboard shortcuts to defaults".into(),
            String::new(),
            0,
        ),
        (
            "ps-forget",
            if saved {
                "Forget saved password".into()
            } else {
                "No saved password".into()
            },
            String::new(),
            0,
        ),
    ];
    let visible = 7;
    let focused = rows.iter().position(|(k, ..)| f.is_focused(id(k)));
    let first = f.list_scroll(id("ps-list"), focused, visible, rows.len());
    let mut act = None;
    for (i, (k, label, val, kind)) in rows.iter().enumerate().skip(first).take(visible) {
        let rr = r(80, 170 + (i - first) as i32 * 112, 1760, 96);
        let a = match kind {
            0 => f.choice(id(k), rr, label, val),
            n => f.toggle(id(k), rr, label, *n == 2),
        };
        if a {
            act = Some(*k);
        }
    }
    footer(
        f,
        "Changes apply to the next connection (quality and mute apply immediately). Back: return",
    );
    let Some(k) = act else { return };
    if k == "ps-alias" {
        app.open_editor(FieldTarget::PeerAlias(peer.to_owned()), rec.alias.clone(), false);
        return;
    }
    if k == "ps-forget" {
        if saved {
            app.modal = Some(Modal::Confirm {
                text: "Forget the saved password for this computer?".into(),
                action: ConfirmAction::ForgetPassword(peer.to_owned()),
            });
        }
        return;
    }
    {
        let rec = app.store.peer_mut(&pid, peer);
        let s = &mut rec.settings;
        match k {
            "ps-fav" => rec.favorite = !rec.favorite,
            "ps-quality" => {
                s.quality = match s.quality {
                    Quality::Balanced => Quality::Best,
                    Quality::Best => Quality::Low,
                    Quality::Low => Quality::Balanced,
                }
            }
            "ps-fps" => {
                s.fps = match s.fps {
                    0 => 30,
                    30 => 60,
                    _ => 0,
                }
            }
            "ps-codec" => {
                s.codec = match s.codec {
                    CodecPreference::Auto => CodecPreference::H265,
                    CodecPreference::H265 => CodecPreference::H264,
                    CodecPreference::H264 => CodecPreference::Vp9,
                    CodecPreference::Vp9 => CodecPreference::Vp8,
                    CodecPreference::Vp8 => CodecPreference::Auto,
                }
            }
            "ps-lock" => s.lock_on_disconnect = !s.lock_on_disconnect,
            "ps-mute" => s.muted = !s.muted,
            "ps-layout" => {
                s.keyboard_layout = match s.keyboard_layout {
                    KeyboardLayout::EnUs => KeyboardLayout::Hu,
                    KeyboardLayout::Hu => KeyboardLayout::EnUs,
                }
            }
            "ps-osk" => {
                s.osk_presentation = match s.osk_presentation {
                    OskPresentation::Shrink => OskPresentation::Overlay,
                    OskPresentation::Overlay => OskPresentation::Shrink,
                }
            }
            "ps-dpad" => {
                s.dpad_mode = match s.dpad_mode {
                    DpadMode::Mouse => DpadMode::Arrows,
                    DpadMode::Arrows => DpadMode::Mouse,
                }
            }
            "ps-cursor" => s.show_remote_cursor = !s.show_remote_cursor,
            "ps-short" => s.shortcuts = None,
            _ => {}
        }
    }
    let new = app.store.peer(&pid, peer).settings;
    app.store.save();
    if let Some(a) = app.active.as_mut() {
        if a.peer_id == peer && a.profile_id == pid {
            if a.settings.quality != new.quality {
                a.handle
                    .send(rdtv_core::session::SessionCommand::SetQuality(new.quality));
            }
            if a.settings.fps != new.fps && new.fps > 0 {
                a.handle.send(rdtv_core::session::SessionCommand::SetFps(new.fps));
            }
            let runtime_monitor = (a.settings.monitor, a.settings.monitor_name.clone());
            a.settings = new;
            a.settings.monitor = runtime_monitor.0;
            a.settings.monitor_name = runtime_monitor.1;
            if a.settings.shortcuts.is_none() {
                a.settings.shortcuts = Some(rdtv_core::store::default_shortcuts(
                    a.peer.as_ref().is_some_and(|p| p.platform == "Mac OS"),
                ));
            }
        }
    }
}

// ----- diagnostics / about / help ---------------------------------------------------------

pub fn diagnostics_text(app: &App) -> String {
    let caps = rdtv_media::caps::decode_caps(&app.platform, app.hw_policy(), &app.failed_codecs);
    let uid = unsafe { libc_getuid() };
    let route = if uid == 0 {
        "running as root (rooted TV / Homebrew Channel elevation)"
    } else {
        "unprivileged (Developer Mode or non-elevated Homebrew install)"
    };
    let mut s = String::new();
    s.push_str(&format!(
        "RemoteDesk TV {VERSION}\nUpstream baseline: {UPSTREAM}\n"
    ));
    s.push_str(&format!(
        "webOS release (os_info.json): {}\n",
        app.platform
            .webos_release
            .clone()
            .unwrap_or_else(|| "unknown / not webOS".into())
    ));
    s.push_str(&format!("Process: uid {uid}, {route}\n"));
    s.push_str(&format!(
        "Graphics/input library: {}\n",
        crate::SDL_DESC.get().map(|s| s.as_str()).unwrap_or("unknown")
    ));
    s.push_str(&format!(
        "NDL DirectMedia hardware decoder: {}\n",
        app.platform
            .ndl_error
            .clone()
            .unwrap_or_else(|| "available".into())
    ));
    s.push_str(&format!(
        "Hardware decoding setting: {}\n",
        if app.store.state.settings.prefer_hardware_decoding {
            "on"
        } else {
            "off"
        }
    ));
    let keys = crate::keys::recent_keys();
    s.push_str(&format!(
        "Last keys (+ press, - release; newest last): {}\n",
        if keys.is_empty() {
            "none yet".to_owned()
        } else {
            keys.join("  ")
        }
    ));
    s.push_str(&format!(
        "Hardware picture placement: {}\n",
        if !app.store.state.settings.hw_picture_placement {
            "off".to_owned()
        } else if let Some(e) = &app.hwplane_error {
            format!("unavailable ({e})")
        } else if let Some(p) = &app.hwplane {
            format!(
                "exported window {} · {} · zoom/1:1 crop: {}",
                p.id(),
                rdtv_media::ndl::window_status(),
                if p.can_crop() {
                    "available"
                } else {
                    "not available"
                }
            )
        } else {
            "not started yet".to_owned()
        }
    ));
    s.push_str(&format!(
        "Advertised codecs: H.265 {} · H.264 {} · AV1 {} · VP9 {} · VP8 {}\n",
        yn(caps.h265),
        yn(caps.h264),
        yn(caps.av1),
        yn(caps.vp9),
        yn(caps.vp8)
    ));
    if !app.failed_codecs.is_empty() {
        s.push_str(&format!(
            "Hardware codecs disabled after failures this run: {:?}\n",
            app.failed_codecs
        ));
    }
    s.push_str(&format!(
        "Audio output: {}\n",
        app.audio_error
            .clone()
            .unwrap_or_else(|| if app.audio_out.is_some() {
                "open".into()
            } else {
                "opens with the first session".into()
            })
    ));
    s.push_str(&format!(
        "Data directory: {}\nLog file: {}\n",
        app.store.dir().display(),
        app.log_path.display()
    ));
    s.push_str(&format!("Server profiles: {}\n", app.store.state.profiles.len()));
    if let Some(a) = &app.active {
        if let Some((c, hw, w, h)) = a.presenting {
            s.push_str(&format!(
                "Current video: {} {}x{} ({})\n",
                c.name(),
                w,
                h,
                if hw { "hardware" } else { "software" }
            ));
        }
    }
    s
}

fn yn(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

unsafe fn libc_getuid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    getuid()
}

fn diagnostics(app: &mut App, f: &mut Frame) {
    header(
        f,
        "Diagnostics",
        "No data leaves the TV. Passwords, tokens and typed text are never logged.",
    );
    let text = diagnostics_text(app);
    f.panel(r(80, 170, 1760, 650));
    f.g.paragraph(&text, 120, 200, 1680, size::BODY, theme::TEXT);
    if f.button(
        id("diag-write"),
        r(80, 850, 700, 90),
        "Save diagnostic report",
        true,
    ) {
        let path = app.store.dir().join("diagnostics.txt");
        let mut body = text.clone();
        if let Ok(log) = std::fs::read_to_string(&app.log_path) {
            body.push_str("\n--- log (tail) ---\n");
            let lines: Vec<&str> = log.lines().collect();
            for l in lines.iter().skip(lines.len().saturating_sub(300)) {
                body.push_str(l);
                body.push('\n');
            }
        }
        match std::fs::write(&path, body) {
            Ok(()) => app.toast(format!("Saved {}", path.display())),
            Err(e) => app.toast(format!("Could not save: {e}")),
        }
    }
}

fn about(_app: &mut App, f: &mut Frame) {
    header(f, "About RemoteDesk TV", &format!("Version {VERSION}"));
    let text = format!(
        "RemoteDesk TV is an independent, community-made RustDesk-compatible remote desktop client for LG webOS TVs. \
It is not made, endorsed or supported by RustDesk, Purslane Ltd., LG Electronics or any project it learned from.\n\n\
License: GNU AGPL-3.0. Source code: https://github.com/danzig666/webos-rustdesk-client\n\
Protocol implementation reuses {UPSTREAM} (hbb_common and base crates, AGPL-3.0) and code ported from its client.\n\n\
Third-party components: SDL2 webOS port by webosbrew (zlib), libvpx (BSD-3-Clause), libopus (BSD-3-Clause), \
DejaVu fonts (Bitstream Vera / public domain), fontdue (MIT/Apache-2.0), rustls, ring, tokio, protobuf and other Rust crates \
(see THIRD-PARTY-NOTICES.txt in the package). Hardware decoding uses the TV's own NDL DirectMedia library, which is not shipped with this app."
    );
    f.panel(r(80, 170, 1760, 720));
    f.g.paragraph(&text, 120, 200, 1680, size::BODY, theme::TEXT);
}

fn help(_app: &mut App, f: &mut Frame) {
    header(f, "Buttons & help", "");
    let rows: [(&str, Option<Color>, &str); 13] = [
        ("Magic Remote pointer", None, "Moves the remote mouse. OK / click = left click. Wheel = scroll."),
        ("Red", Some(theme::RED_KEY), "Right click at the mouse position."),
        ("Green", Some(theme::GREEN_KEY), "Next monitor (wraps around). The monitor name is shown briefly."),
        ("Yellow", Some(theme::YELLOW_KEY), "Arrow buttons: switch between Mouse mode and Arrow-key mode."),
        ("Blue", Some(theme::BLUE_KEY), "Open or close the on-screen keyboard."),
        ("Back (short)", None, "Closes the top menu or keyboard; otherwise shows or hides the session toolbar. Never disconnects."),
        ("Arrow buttons", None, "Mouse mode: move the pointer (hold to speed up), OK clicks. Arrow mode: send arrow keys, OK = Enter."),
        ("Number buttons 0-9", None, "Type digits: IDs on Home, passwords and codes in dialogs, digits on the computer."),
        ("CH + / CH −", None, "Scroll up / down."),
        ("Keyboard", None, "Select Ctrl/Alt/Shift/Win once for the next key, twice to lock. EN→HU switches layout. More = F-keys and navigation keys."),
        ("Toolbar", None, "View (fit, 1:1, zoom), Keys & actions (Ctrl+Alt+Del, lock, double-click, drag lock), quality, info, disconnect."),
        ("Leaving the app", None, "Disconnects by default; you get a Reconnect button when you return. Settings can keep it connected."),
        ("Connection loss", None, "Reconnects automatically for up to 30 seconds with a countdown and Cancel."),
    ];
    let mut y = 150;
    for (name, col, desc) in rows {
        let mut x = 80;
        if let Some(c) = col {
            f.g.round(r(x, y + 8, 34, 34), 17, c);
            x += 48;
        }
        f.g.text(name, x, y + 6, size::BODY, true, theme::TEXT);
        f.g.text_fit(desc, 560, y + 6, size::BODY, false, theme::TEXT_DIM, 1300);
        y += 66;
    }
}

// ----- account ----------------------------------------------------------------------------

fn account_client(app: &App) -> account::Client {
    let p = app.store.active_profile();
    account::Client {
        api: p.effective_api_server(),
        my_id: app.store.state.client_id.clone(),
        uuid: app.store.state.client_uuid.clone(),
        device_name: crate::platform::client_name(),
    }
}

fn start_account_task<F>(app: &mut App, fut: F)
where
    F: std::future::Future<Output = AccountResult> + Send + 'static,
{
    let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    app.account_form.result = Some(slot.clone());
    app.account_form.busy = true;
    app.rt.spawn(async move {
        let r = fut.await;
        if let Ok(mut g) = slot.lock() {
            *g = Some(r);
        }
    });
}

fn refresh_address_book(app: &mut App) {
    let Some(token) = app
        .store
        .token(&app.store.state.active_profile)
        .map(|s| s.to_owned())
    else {
        return;
    };
    let c = account_client(app);
    app.account_form.status = "Loading address book…".into();
    start_account_task(app, async move {
        let user = c.current_user(&token).await;
        let r = match user {
            Err(e) => Err(e.to_string()),
            Ok(u) => c
                .address_book(&token)
                .await
                .map(|b| (u, b))
                .map_err(|e| e.to_string()),
        };
        AccountResult::Book(r)
    });
}

fn poll_account(app: &mut App) {
    let Some(slot) = app.account_form.result.clone() else {
        return;
    };
    let res = slot.lock().ok().and_then(|mut g| g.take());
    let Some(res) = res else { return };
    app.account_form.result = None;
    app.account_form.busy = false;
    let pid = app.store.state.active_profile.clone();
    match res {
        AccountResult::Login(Ok(LoginOutcome::Token { token, user })) => {
            app.store.set_token(&pid, Some(token));
            app.store.profile_data_mut(&pid).account_name = user;
            app.store.save();
            app.account_form.password.clear();
            app.account_form.code.clear();
            app.account_form.challenge = None;
            app.account_form.status = "Signed in.".into();
            refresh_address_book(app);
        }
        AccountResult::Login(Ok(LoginOutcome::NeedCode { secret, tfa })) => {
            app.account_form.challenge = Some((secret, tfa));
            app.account_form.status = if tfa {
                "Enter the code from your authenticator app.".into()
            } else {
                "Enter the verification code sent by email.".into()
            };
        }
        AccountResult::Login(Err(e)) => {
            app.account_form.password.clear();
            app.account_form.status = format!("Sign-in failed: {e}");
        }
        AccountResult::Book(Ok((user, book))) => {
            let d = app.store.profile_data_mut(&pid);
            if !user.is_empty() {
                d.account_name = user;
            }
            d.address_book = book;
            let n = d.address_book.len();
            app.store.save();
            app.account_form.status = format!("Address book: {n} computers.");
        }
        AccountResult::Book(Err(e)) => {
            if e.contains("signed out") {
                app.store.set_token(&pid, None);
                app.store.save();
            }
            app.account_form.status = format!("Address book unavailable: {e}");
        }
    }
}

pub fn sign_out(app: &mut App) {
    let pid = app.store.state.active_profile.clone();
    if let Some(token) = app.store.token(&pid).map(|s| s.to_owned()) {
        let c = account_client(app);
        app.rt.spawn(async move { c.logout(&token).await });
    }
    app.store.set_token(&pid, None);
    app.store.save();
    app.account_form = AccountForm {
        home_tab: app.account_form.home_tab,
        ..Default::default()
    };
    app.toast("Signed out of this server profile");
}

fn account_modal(app: &mut App, f: &mut Frame) {
    let p = app.store.active_profile().clone();
    let api = p.effective_api_server();
    let panel = r(360, 120, 1200, 840);
    f.g.round(panel, 28, theme::PANEL);
    f.g.text_fit(
        &format!("Account · {}", p.name),
        400,
        150,
        size::TITLE,
        true,
        theme::TEXT,
        1120,
    );
    f.g.text_fit(
        &format!(
            "API server: {}",
            if api.is_empty() {
                "none configured".into()
            } else {
                api.clone()
            }
        ),
        400,
        215,
        size::SMALL,
        false,
        theme::TEXT_DIM,
        1120,
    );
    let signed_in = app.store.token(&p.id).is_some();
    let form = app.account_form.clone();
    let mut y = 270;
    if signed_in {
        let name = app.store.profile_data(&p.id).account_name;
        f.g.text_fit(
            &format!(
                "Signed in{}",
                if name.is_empty() {
                    String::new()
                } else {
                    format!(" as {name}")
                }
            ),
            400,
            y,
            size::LARGE,
            true,
            theme::TEXT,
            1120,
        );
        y += 80;
        if f.button(
            id("acc-refresh"),
            r(400, y, 540, 90),
            if form.busy {
                "Working…"
            } else {
                "Refresh address book"
            },
            true,
        ) && !form.busy
        {
            refresh_address_book(app);
        }
        if f.button(id("acc-signout"), r(980, y, 540, 90), "Sign out", false) {
            app.modal = Some(Modal::Confirm {
                text: "Sign out of this server profile? Its cached address book is removed.".into(),
                action: ConfirmAction::SignOut,
            });
            return;
        }
        y += 120;
    } else if let Some((secret, tfa)) = form.challenge.clone() {
        if f.field(
            id("acc-code"),
            r(400, y, 1120, 110),
            if tfa {
                "Authenticator code"
            } else {
                "Email verification code"
            },
            &form.code,
            false,
            "123456",
            false,
        ) {
            app.open_editor(FieldTarget::AccountCode, form.code.clone(), false);
        }
        y += 140;
        if f.button(
            id("acc-verify"),
            r(400, y, 540, 90),
            if form.busy { "Working…" } else { "Verify" },
            true,
        ) && !form.busy
        {
            let c = account_client(app);
            let (u, code) = (form.user.clone(), form.code.clone());
            start_account_task(app, async move {
                AccountResult::Login(
                    c.login_code(&u, &code, &secret, tfa)
                        .await
                        .map_err(|e| e.to_string()),
                )
            });
        }
        y += 120;
    } else {
        if f.field(
            id("acc-user"),
            r(400, y, 1120, 110),
            "User name or email",
            &form.user,
            false,
            "",
            false,
        ) {
            app.open_editor(FieldTarget::AccountUser, form.user.clone(), false);
        }
        y += 130;
        if f.field(
            id("acc-pass"),
            r(400, y, 1120, 110),
            "Password",
            &form.password,
            true,
            "",
            false,
        ) {
            app.open_editor(FieldTarget::AccountPassword, form.password.clone(), true);
        }
        y += 140;
        if f.button(
            id("acc-login"),
            r(400, y, 540, 90),
            if form.busy { "Working…" } else { "Sign in" },
            true,
        ) && !form.busy
        {
            let c = account_client(app);
            let (u, pw) = (form.user.clone(), form.password.clone());
            start_account_task(app, async move {
                AccountResult::Login(c.login(&u, &pw).await.map_err(|e| e.to_string()))
            });
        }
        y += 120;
    }
    f.g.paragraph(
        if form.status.is_empty() { "Accounts and address books need a server that provides them (RustDesk Pro or a compatible API server). The public server's console and plain open-source servers may not. ID and password connections work without signing in." } else { &form.status },
        400, y, 1120, size::SMALL, theme::TEXT_DIM,
    );
    if f.button(id("acc-close"), r(400, 860, 300, 80), "Close", false) {
        app.modal = None;
    }
}

// ----- modals -----------------------------------------------------------------------------

pub fn draw_modal(app: &mut App, f: &mut Frame) {
    f.scrim();
    let Some(m) = app.modal.take() else { return };
    let mut keep = true;
    let peer_label = app
        .active
        .as_ref()
        .map(|a| a.peer_id.clone())
        .or_else(|| app.last_session.as_ref().map(|(_, t)| t.as_str().to_owned()))
        .unwrap_or_default();
    let m = match m {
        Modal::Account => {
            app.modal = Some(Modal::Account);
            account_modal(app, f);
            return;
        }
        Modal::Connecting { phase, since } => {
            let panel = r(460, 300, 1000, 420);
            f.g.round(panel, 28, theme::PANEL);
            let txt = match phase {
                Phase::ConnectingServer => "Contacting the ID server…",
                Phase::Connecting => "Connecting to the computer…",
                Phase::LoggingIn => "Signing in…",
                Phase::WaitingApproval => "Waiting for the remote side to accept…",
                Phase::Connected => "Connected",
            };
            f.g.text_fit(&peer_label, 520, 340, size::TITLE, true, theme::TEXT, 880);
            f.g.text(txt, 520, 420, size::LARGE, false, theme::TEXT);
            let dots = ".".repeat(((since.elapsed().as_millis() / 400) % 4) as usize);
            f.g.text(
                &format!("{}s {dots}", since.elapsed().as_secs()),
                520,
                480,
                size::BODY,
                false,
                theme::TEXT_DIM,
            );
            if f.button(id("conn-cancel"), r(520, 590, 360, 90), "Cancel", false) {
                app.end_session(false, "cancelled");
                keep = false;
            }
            Modal::Connecting { phase, since }
        }
        Modal::Password {
            wrong,
            value,
            mut remember,
        } => {
            let panel = r(410, 200, 1100, 660);
            f.g.round(panel, 28, theme::PANEL);
            f.g.text_fit(
                &format!("Password for {peer_label}"),
                460,
                240,
                size::TITLE,
                true,
                theme::TEXT,
                1000,
            );
            f.g.text_fit(
                if wrong {
                    "Wrong password. Please try again."
                } else {
                    "Enter the RustDesk password shown on the computer, or wait for someone there to accept."
                },
                460,
                310,
                size::BODY,
                wrong,
                if wrong { theme::WARN } else { theme::TEXT_DIM },
                1000,
            );
            if f.field(
                id("pw-field"),
                r(460, 380, 1000, 110),
                "Password",
                &value,
                true,
                "",
                false,
            ) {
                app.open_editor(FieldTarget::Password, value.clone(), true);
            }
            if f.toggle(
                id("pw-remember"),
                r(460, 520, 1000, 90),
                "Remember password?",
                remember,
            ) {
                remember = !remember;
            }
            let mut submit = false;
            if f.button(id("pw-ok"), r(460, 650, 480, 90), "Connect", true) {
                submit = true;
            }
            if f.button(id("pw-cancel"), r(980, 650, 480, 90), "Cancel", false) {
                app.end_session(false, "cancelled at password");
                keep = false;
            }
            let m = Modal::Password {
                wrong,
                value,
                remember,
            };
            if submit && keep {
                app.modal = Some(m);
                if matches!(&app.modal, Some(Modal::Password { value, .. }) if value.is_empty()) {
                    let v = String::new();
                    app.open_editor(FieldTarget::Password, v, true);
                } else {
                    app.submit_password();
                }
                return;
            }
            m
        }
        Modal::TwoFactor { wrong, value } => {
            let panel = r(410, 250, 1100, 520);
            f.g.round(panel, 28, theme::PANEL);
            f.g.text("Two-factor code", 460, 290, size::TITLE, true, theme::TEXT);
            f.g.text(
                if wrong {
                    "Wrong code, try again."
                } else {
                    "Enter the 2FA code for the remote computer."
                },
                460,
                360,
                size::BODY,
                false,
                theme::TEXT_DIM,
            );
            if f.field(
                id("tfa-field"),
                r(460, 420, 1000, 110),
                "Code",
                &value,
                false,
                "123456",
                false,
            ) {
                app.open_editor(FieldTarget::TwoFactor, value.clone(), false);
            }
            let m = Modal::TwoFactor { wrong, value };
            if f.button(id("tfa-ok"), r(460, 570, 480, 90), "Verify", true) {
                app.modal = Some(m);
                app.submit_2fa();
                return;
            }
            if f.button(id("tfa-cancel"), r(980, 570, 480, 90), "Cancel", false) {
                app.end_session(false, "cancelled at 2fa");
                keep = false;
            }
            m
        }
        Modal::Reconnecting {
            plan,
            next_at,
            attempt_running,
            reason,
        } => {
            let panel = r(410, 280, 1100, 480);
            f.g.round(panel, 28, theme::PANEL);
            f.g.text("Connection lost", 460, 320, size::TITLE, true, theme::TEXT);
            f.g.text_fit(&reason, 460, 390, size::BODY, false, theme::TEXT_DIM, 1000);
            let now = Instant::now();
            let line = if attempt_running {
                format!("Reconnecting (attempt {})…", plan.attempts())
            } else {
                format!(
                    "Reconnecting in {} s",
                    next_at.saturating_duration_since(now).as_secs() + 1
                )
            };
            f.g.text(&line, 460, 460, size::LARGE, true, theme::ACCENT2);
            f.g.text(
                &format!("Giving up in {} s", plan.remaining(now).as_secs()),
                460,
                520,
                size::BODY,
                false,
                theme::TEXT_DIM,
            );
            if f.button(id("rc-cancel"), r(460, 620, 400, 90), "Cancel", true) {
                app.end_session(false, "reconnect cancelled");
                keep = false;
            }
            Modal::Reconnecting {
                plan,
                next_at,
                attempt_running,
                reason,
            }
        }
        Modal::Failed {
            title,
            text,
            kind,
            retry,
        } => {
            let panel = r(360, 260, 1200, 520);
            f.g.round(panel, 28, theme::PANEL);
            f.g.text_fit(&title, 410, 300, size::TITLE, true, theme::TEXT, 1100);
            f.g.paragraph(&text, 410, 380, 1100, size::BODY, theme::TEXT_DIM);
            let mut x = 410;
            if retry && f.button(id("fail-retry"), r(x, 650, 360, 90), "Retry", true) {
                if let Some((_, t)) = app.last_session.clone() {
                    app.modal = None;
                    app.connect(t, None);
                    return;
                }
            }
            if retry {
                x += 380;
            }
            if kind == FailureKind::Key
                && f.button(id("fail-profile"), r(x, 650, 400, 90), "Server settings", false)
            {
                app.go(Screen::Profiles);
                keep = false;
            }
            if f.button(id("fail-close"), r(1180, 650, 330, 90), "Close", false) {
                keep = false;
            }
            Modal::Failed {
                title,
                text,
                kind,
                retry,
            }
        }
        Modal::Confirm { text, action } => {
            let panel = r(460, 330, 1000, 380);
            f.g.round(panel, 28, theme::PANEL);
            f.g.paragraph(&text, 510, 380, 900, size::LARGE, theme::TEXT);
            if f.button(id("cf-yes"), r(510, 580, 420, 90), "Yes", true) {
                app.do_confirm(action.clone());
                if app.modal.is_none() {
                    return;
                }
                keep = false;
            }
            if f.button(id("cf-no"), r(990, 580, 420, 90), "No", false) {
                keep = false;
            }
            Modal::Confirm { text, action }
        }
        Modal::Notice { title, text } => {
            let panel = r(410, 330, 1100, 400);
            f.g.round(panel, 28, theme::PANEL);
            f.g.text_fit(&title, 460, 370, size::TITLE, true, theme::TEXT, 1000);
            f.g.paragraph(&text, 460, 440, 1000, size::BODY, theme::TEXT_DIM);
            if f.button(id("nt-ok"), r(460, 610, 360, 90), "OK", true) {
                keep = false;
            }
            Modal::Notice { title, text }
        }
        Modal::ReconnectPrompt => {
            let panel = r(410, 300, 1100, 440);
            f.g.round(panel, 28, theme::PANEL);
            f.g.text("Disconnected", 460, 340, size::TITLE, true, theme::TEXT);
            f.g.paragraph("The session ended when you left the app (you can change this in Settings: Keep connected in background).", 460, 410, 1000, size::BODY, theme::TEXT_DIM);
            if f.button(
                id("rp-yes"),
                r(460, 600, 560, 100),
                &format!("Reconnect to {peer_label}"),
                true,
            ) {
                if let Some((_, t)) = app.last_session.clone() {
                    app.modal = None;
                    app.connect(t, None);
                    return;
                }
            }
            if f.button(id("rp-no"), r(1060, 600, 400, 100), "Not now", false) {
                keep = false;
            }
            Modal::ReconnectPrompt
        }
        Modal::PeerMenu(peer) => {
            let pid = app.store.state.active_profile.clone();
            let rec = app.store.peer(&pid, &peer);
            let saved = app.store.remembered_password(&pid, &peer).is_some();
            let panel = r(560, 160, 800, 760);
            f.g.round(panel, 28, theme::PANEL);
            let title = if rec.alias.is_empty() {
                peer.clone()
            } else {
                rec.alias.clone()
            };
            f.g.text_fit(&title, 610, 200, size::TITLE, true, theme::TEXT, 700);
            let mut y = 280;
            let mut act = None;
            let items = [
                ("pm-connect", "Connect".to_owned()),
                (
                    "pm-fav",
                    if rec.favorite {
                        "Remove from favorites".into()
                    } else {
                        "Add to favorites".into()
                    },
                ),
                ("pm-rename", "Rename".into()),
                ("pm-settings", "Computer settings".into()),
                (
                    "pm-forget",
                    if saved {
                        "Forget saved password".into()
                    } else {
                        "No saved password".into()
                    },
                ),
                ("pm-remove", "Remove from recent".into()),
            ];
            for (k, label) in items.iter() {
                if f.button(id(k), r(610, y, 700, 86), label, *k == "pm-connect") {
                    act = Some(*k);
                }
                y += 100;
            }
            let m = Modal::PeerMenu(peer.clone());
            match act {
                Some("pm-connect") => {
                    app.modal = None;
                    app.request_connect(&peer, None);
                    return;
                }
                Some("pm-fav") => {
                    let r = app.store.peer_mut(&pid, &peer);
                    r.favorite = !r.favorite;
                    app.store.save();
                }
                Some("pm-rename") => {
                    app.modal = Some(m);
                    app.open_editor(FieldTarget::PeerAlias(peer.clone()), rec.alias.clone(), false);
                    return;
                }
                Some("pm-settings") => {
                    app.go(Screen::PeerSettings(peer.clone()));
                    keep = false;
                }
                Some("pm-forget") if saved => {
                    app.store.forget_password(&pid, &peer);
                    app.store.save();
                    app.toast("Saved password removed");
                }
                Some("pm-remove") => {
                    let d = app.store.profile_data_mut(&pid);
                    d.recents.retain(|p| p != &peer);
                    app.store.save();
                    keep = false;
                }
                _ => {}
            }
            m
        }
    };
    if keep && app.modal.is_none() {
        app.modal = Some(m);
    }
}

pub fn draw_editor(app: &mut App, f: &mut Frame) {
    let Some(mut ed) = app.editor.take() else { return };
    f.scrim();
    let label = match &ed.target {
        FieldTarget::HomeTarget => "Computer ID or address",
        FieldTarget::Password => "Password",
        FieldTarget::TwoFactor => "Two-factor code",
        FieldTarget::Search => "Search",
        FieldTarget::ProfileName => "Profile name",
        FieldTarget::ProfileIdServer => "ID server",
        FieldTarget::ProfileRelay => "Relay server",
        FieldTarget::ProfileApi => "API server",
        FieldTarget::ProfileKey => "Server key",
        FieldTarget::ProfileImport => "Server config string",
        FieldTarget::PeerAlias(_) => "Computer name",
        FieldTarget::AccountUser => "User name or email",
        FieldTarget::AccountPassword => "Account password",
        FieldTarget::AccountCode => "Verification code",
    };
    let osk_h = ed.osk.height();
    let fy = (H - osk_h - 170).max(40);
    f.set_inert(true);
    f.field(
        id("editor-field"),
        r(200, fy, W - 400, 120),
        label,
        &ed.value,
        ed.secret,
        "",
        true,
    );
    f.set_inert(false);
    f.g.text(
        "Done / Enter: confirm · Close or Back: cancel",
        200,
        fy + 130,
        size::SMALL,
        false,
        theme::TEXT_DIM,
    );
    let acts = ed.osk.draw(f);
    app.editor = Some(ed);
    app.editor_actions(acts);
}

pub fn draw_toast(app: &mut App, f: &mut Frame) {
    if let Some((t, _)) = &app.toast {
        let w = (f.g.text_width(t, size::BODY, false) + 60).min(W - 160);
        let rr = r((W - w) / 2, H - 150, w, 70);
        f.g.round(rr, 35, Color::RGBA(30, 45, 75, 240));
        f.g.text_fit(
            t,
            rr.x() + 30,
            rr.y() + 18,
            size::BODY,
            false,
            theme::TEXT,
            w - 60,
        );
    }
}

#[allow(dead_code)]
fn _unused(_: Target) {}
