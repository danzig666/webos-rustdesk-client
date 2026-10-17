//! Full on-screen keyboard.
//!
//! * Layouts: English (US) QWERTY and Hungarian QWERTZ (with AltGr level), switchable with
//!   one key. The layout only decides which characters the local keys produce; text is
//!   delivered to the host as text (see `rdtv_core::input`), so the host's own keyboard
//!   layout is never changed.
//! * Modifiers: select once = one-shot (applies to the next key), select again = locked,
//!   a third time = off. Modifiers travel with each key event, so nothing stays held on the
//!   host while the user navigates between keys.
//! * Compact layout (letters, digits, editing, arrows) and expanded layout (adds Esc,
//!   F1-F12, Print Screen/Scroll Lock/Pause, Insert/Home/End/Page Up/Page Down) plus an
//!   optional numeric keypad.
//! * In remote mode a customizable shortcut row (saved per computer) and a separate
//!   Ctrl+Alt+Del host action are shown.

use crate::{
    gfx::{size, theme, H, W},
    ui::{idx, Frame},
};
use rdtv_core::{
    input::{parse_chord, Chord, ChordKey, Mods},
    message_proto::ControlKey,
    store::{KeyboardLayout, Shortcut},
};
use sdl2::{pixels::Color, rect::Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Editing a local field (password, ID, server address...): nothing goes to the host.
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModState {
    #[default]
    Off,
    OneShot,
    Locked,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OskAction {
    /// Text for the focused local field or the host.
    Text(String),
    /// Key or chord for the host (remote mode).
    Chord(Chord),
    /// Local editing.
    Backspace,
    Submit,
    CtrlAltDel,
    Close,
    LayoutChanged(KeyboardLayout),
    ShortcutsChanged(Vec<Shortcut>),
}

#[derive(Clone, Copy)]
enum K {
    /// (normal, shift, altgr)
    Ch(char, char, Option<char>),
    Ctrl(&'static str, ControlKey),
    Mod(usize),
    Space,
    Backspace,
    Enter,
    Layout,
    Expand,
    Numpad,
    Close,
}

const M_CTRL: usize = 0;
const M_ALT: usize = 1;
const M_ALTGR: usize = 2;
const M_SHIFT: usize = 3;
const M_WIN: usize = 4;
const MOD_NAMES: [&str; 5] = ["Ctrl", "Alt", "AltGr", "Shift", "Win"];

fn ch(n: char, s: char) -> (K, f32) {
    (K::Ch(n, s, None), 1.0)
}
fn cha(n: char, s: char, a: char) -> (K, f32) {
    (K::Ch(n, s, Some(a)), 1.0)
}

fn rows(layout: KeyboardLayout, mac: bool) -> Vec<Vec<(K, f32)>> {
    let win = if mac { "Cmd" } else { "Win" };
    let _ = win;
    match layout {
        KeyboardLayout::EnUs => vec![
            vec![
                ch('`', '~'),
                ch('1', '!'),
                ch('2', '@'),
                ch('3', '#'),
                ch('4', '$'),
                ch('5', '%'),
                ch('6', '^'),
                ch('7', '&'),
                ch('8', '*'),
                ch('9', '('),
                ch('0', ')'),
                ch('-', '_'),
                ch('=', '+'),
                (K::Backspace, 2.0),
            ],
            vec![
                (K::Ctrl("Tab", ControlKey::Tab), 1.5),
                ch('q', 'Q'),
                ch('w', 'W'),
                ch('e', 'E'),
                ch('r', 'R'),
                ch('t', 'T'),
                ch('y', 'Y'),
                ch('u', 'U'),
                ch('i', 'I'),
                ch('o', 'O'),
                ch('p', 'P'),
                ch('[', '{'),
                ch(']', '}'),
                (K::Ch('\\', '|', None), 1.5),
            ],
            vec![
                (K::Ctrl("Caps", ControlKey::CapsLock), 1.8),
                ch('a', 'A'),
                ch('s', 'S'),
                ch('d', 'D'),
                ch('f', 'F'),
                ch('g', 'G'),
                ch('h', 'H'),
                ch('j', 'J'),
                ch('k', 'K'),
                ch('l', 'L'),
                ch(';', ':'),
                ch('\'', '"'),
                (K::Enter, 2.2),
            ],
            vec![
                (K::Mod(M_SHIFT), 2.3),
                ch('z', 'Z'),
                ch('x', 'X'),
                ch('c', 'C'),
                ch('v', 'V'),
                ch('b', 'B'),
                ch('n', 'N'),
                ch('m', 'M'),
                ch(',', '<'),
                ch('.', '>'),
                ch('/', '?'),
                (K::Ctrl("↑", ControlKey::UpArrow), 1.0),
                (K::Mod(M_SHIFT), 1.7),
            ],
        ],
        KeyboardLayout::Hu => vec![
            vec![
                cha('0', '§', '¬'),
                cha('1', '\'', '~'),
                cha('2', '"', 'ˇ'),
                cha('3', '+', '^'),
                cha('4', '!', '˘'),
                cha('5', '%', '°'),
                cha('6', '/', '˛'),
                cha('7', '=', '`'),
                cha('8', '(', '˙'),
                cha('9', ')', '´'),
                ch('ö', 'Ö'),
                ch('ü', 'Ü'),
                ch('ó', 'Ó'),
                (K::Backspace, 2.0),
            ],
            vec![
                (K::Ctrl("Tab", ControlKey::Tab), 1.5),
                cha('q', 'Q', '\\'),
                cha('w', 'W', '|'),
                cha('e', 'E', 'Ä'),
                ch('r', 'R'),
                ch('t', 'T'),
                cha('z', 'Z', '€'),
                cha('u', 'U', '€'),
                cha('i', 'I', 'Í'),
                ch('o', 'O'),
                ch('p', 'P'),
                cha('ő', 'Ő', '÷'),
                cha('ú', 'Ú', '×'),
                (K::Ch('ű', 'Ű', Some('¤')), 1.5),
            ],
            vec![
                (K::Ctrl("Caps", ControlKey::CapsLock), 1.8),
                cha('a', 'A', 'ä'),
                cha('s', 'S', 'đ'),
                cha('d', 'D', 'Đ'),
                cha('f', 'F', '['),
                cha('g', 'G', ']'),
                ch('h', 'H'),
                cha('j', 'J', 'í'),
                cha('k', 'K', 'ł'),
                cha('l', 'L', 'Ł'),
                cha('é', 'É', '$'),
                cha('á', 'Á', 'ß'),
                (K::Enter, 2.2),
            ],
            vec![
                (K::Mod(M_SHIFT), 1.3),
                cha('í', 'Í', '<'),
                cha('y', 'Y', '>'),
                cha('x', 'X', '#'),
                cha('c', 'C', '&'),
                cha('v', 'V', '@'),
                cha('b', 'B', '{'),
                cha('n', 'N', '}'),
                ch('m', 'M'),
                cha(',', '?', ';'),
                cha('.', ':', '>'),
                cha('-', '_', '*'),
                (K::Ctrl("↑", ControlKey::UpArrow), 1.0),
                (K::Mod(M_SHIFT), 1.7),
            ],
        ],
    }
}

fn bottom_row(mode: Mode) -> Vec<(K, f32)> {
    let mut v = vec![
        (K::Mod(M_CTRL), 1.3),
        (K::Mod(M_WIN), 1.3),
        (K::Mod(M_ALT), 1.3),
        (K::Space, 4.6),
        (K::Mod(M_ALTGR), 1.3),
        (K::Ctrl("←", ControlKey::LeftArrow), 1.0),
        (K::Ctrl("↓", ControlKey::DownArrow), 1.0),
        (K::Ctrl("→", ControlKey::RightArrow), 1.0),
        (K::Layout, 1.3),
    ];
    if mode == Mode::Remote {
        v.push((K::Expand, 1.4));
        v.push((K::Numpad, 1.2));
    }
    v.push((K::Close, 1.4));
    v
}

fn fn_row() -> Vec<(K, f32)> {
    let mut v = vec![(K::Ctrl("Esc", ControlKey::Escape), 1.2)];
    let f = [
        ("F1", ControlKey::F1),
        ("F2", ControlKey::F2),
        ("F3", ControlKey::F3),
        ("F4", ControlKey::F4),
        ("F5", ControlKey::F5),
        ("F6", ControlKey::F6),
        ("F7", ControlKey::F7),
        ("F8", ControlKey::F8),
        ("F9", ControlKey::F9),
        ("F10", ControlKey::F10),
        ("F11", ControlKey::F11),
        ("F12", ControlKey::F12),
    ];
    for (n, k) in f {
        v.push((K::Ctrl(n, k), 1.0));
    }
    v.push((K::Ctrl("PrtSc", ControlKey::Snapshot), 1.1));
    v.push((K::Ctrl("ScrLk", ControlKey::Scroll), 1.1));
    v.push((K::Ctrl("Pause", ControlKey::Pause), 1.1));
    v
}

fn nav_cluster() -> Vec<Vec<(K, f32)>> {
    vec![
        vec![
            (K::Ctrl("Ins", ControlKey::Insert), 1.0),
            (K::Ctrl("Home", ControlKey::Home), 1.0),
            (K::Ctrl("PgUp", ControlKey::PageUp), 1.0),
        ],
        vec![
            (K::Ctrl("Del", ControlKey::Delete), 1.0),
            (K::Ctrl("End", ControlKey::End), 1.0),
            (K::Ctrl("PgDn", ControlKey::PageDown), 1.0),
        ],
    ]
}

fn numpad() -> Vec<Vec<(K, f32)>> {
    let n = |l: &'static str, k| (K::Ctrl(l, k), 1.0);
    vec![
        vec![
            n("Num", ControlKey::NumLock),
            n("/", ControlKey::Divide),
            n("*", ControlKey::Multiply),
            n("-", ControlKey::Subtract),
        ],
        vec![
            n("7", ControlKey::Numpad7),
            n("8", ControlKey::Numpad8),
            n("9", ControlKey::Numpad9),
            n("+", ControlKey::Add),
        ],
        vec![
            n("4", ControlKey::Numpad4),
            n("5", ControlKey::Numpad5),
            n("6", ControlKey::Numpad6),
            n("Ent", ControlKey::NumpadEnter),
        ],
        vec![
            n("1", ControlKey::Numpad1),
            n("2", ControlKey::Numpad2),
            n("3", ControlKey::Numpad3),
            n("0", ControlKey::Numpad0),
        ],
    ]
}

pub struct Osk {
    pub mode: Mode,
    pub layout: KeyboardLayout,
    pub expanded: bool,
    pub numpad: bool,
    pub mods: [ModState; 5],
    pub caps: bool,
    pub mac: bool,
    pub shortcuts: Vec<Shortcut>,
    last_chord: Option<Vec<String>>,
    remove_mode: bool,
}

impl Osk {
    pub fn new(mode: Mode, layout: KeyboardLayout) -> Osk {
        Osk {
            mode,
            layout,
            expanded: false,
            numpad: false,
            mods: [ModState::Off; 5],
            caps: false,
            mac: false,
            shortcuts: Vec::new(),
            last_chord: None,
            remove_mode: false,
        }
    }

    /// Height the keyboard occupies at the bottom of the screen.
    pub fn height(&self) -> i32 {
        let rows = 5 + self.expanded as i32 + (self.mode == Mode::Remote) as i32;
        rows * 74 + 36
    }

    pub fn reset_modifiers(&mut self) {
        self.mods = [ModState::Off; 5];
    }

    fn mods_now(&self) -> Mods {
        let on = |i: usize| self.mods[i] != ModState::Off;
        Mods {
            ctrl: on(M_CTRL),
            alt: on(M_ALT),
            altgr: false,
            shift: on(M_SHIFT),
            meta: on(M_WIN),
        }
    }

    fn consume_one_shots(&mut self) {
        for m in self.mods.iter_mut() {
            if *m == ModState::OneShot {
                *m = ModState::Off;
            }
        }
    }

    fn mod_names_now(&self) -> Vec<String> {
        let mut v = Vec::new();
        for (i, m) in self.mods.iter().enumerate() {
            if *m != ModState::Off && i != M_ALTGR {
                v.push(match i {
                    M_WIN => "Meta".to_owned(),
                    _ => MOD_NAMES[i].to_owned(),
                });
            }
        }
        v
    }

    fn key_label(&self, k: &K) -> String {
        let shift = self.mods[M_SHIFT] != ModState::Off;
        let altgr = self.mods[M_ALTGR] != ModState::Off;
        match k {
            K::Ch(n, s, a) => {
                if altgr {
                    if let Some(a) = a {
                        return a.to_string();
                    }
                }
                let upper = shift ^ (self.caps && n.is_alphabetic());
                if upper {
                    s.to_string()
                } else {
                    n.to_string()
                }
            }
            K::Ctrl(l, _) => l.to_string(),
            K::Mod(i) => {
                if *i == M_WIN && self.mac {
                    "Cmd".into()
                } else if *i == M_ALT && self.mac {
                    "Option".into()
                } else {
                    MOD_NAMES[*i].into()
                }
            }
            K::Space => "Space".into(),
            K::Backspace => "⌫ Backspace".into(),
            K::Enter => {
                if self.mode == Mode::Local {
                    "Done".into()
                } else {
                    "Enter ⏎".into()
                }
            }
            K::Layout => match self.layout {
                KeyboardLayout::EnUs => "EN→HU".into(),
                KeyboardLayout::Hu => "HU→EN".into(),
            },
            K::Expand => if self.expanded { "Less" } else { "More" }.into(),
            K::Numpad => if self.numpad { "No 123" } else { "123" }.into(),
            K::Close => "Close ⌄".into(),
        }
    }

    fn press(&mut self, k: K, out: &mut Vec<OskAction>) {
        let hotkey = self.mods[M_CTRL] != ModState::Off
            || self.mods[M_ALT] != ModState::Off
            || self.mods[M_WIN] != ModState::Off;
        match k {
            K::Mod(i) => {
                self.mods[i] = match self.mods[i] {
                    ModState::Off => ModState::OneShot,
                    ModState::OneShot => ModState::Locked,
                    ModState::Locked => ModState::Off,
                };
                // Shift on both sides of the layout is one modifier.
            }
            K::Ch(n, s, a) => {
                let shift = self.mods[M_SHIFT] != ModState::Off;
                let altgr = self.mods[M_ALTGR] != ModState::Off;
                if self.mode == Mode::Remote && hotkey && !altgr {
                    // Shortcut: base character plus modifiers (e.g. Ctrl+C, Win+D).
                    let base = if n.is_ascii() { n } else { s.to_ascii_lowercase() };
                    let chord = Chord {
                        mods: self.mods_now(),
                        key: ChordKey::Char(base.to_ascii_lowercase()),
                    };
                    let mut names = self.mod_names_now();
                    names.push(base.to_ascii_uppercase().to_string());
                    self.last_chord = Some(names);
                    out.push(OskAction::Chord(chord));
                } else {
                    let c = if altgr {
                        a.unwrap_or(n)
                    } else {
                        let upper = shift ^ (self.caps && n.is_alphabetic());
                        if upper {
                            s
                        } else {
                            n
                        }
                    };
                    out.push(OskAction::Text(c.to_string()));
                }
                self.consume_one_shots();
            }
            K::Ctrl(name, ck) => {
                if ck == ControlKey::CapsLock {
                    self.caps = !self.caps;
                }
                if self.mode == Mode::Remote {
                    let mut names = self.mod_names_now();
                    names.push(name.to_owned());
                    self.last_chord = Some(names);
                    out.push(OskAction::Chord(Chord {
                        mods: self.mods_now(),
                        key: ChordKey::Control(ck),
                    }));
                } else {
                    match ck {
                        ControlKey::Delete => out.push(OskAction::Backspace),
                        ControlKey::Tab => out.push(OskAction::Text("\t".into())),
                        _ => {}
                    }
                }
                self.consume_one_shots();
            }
            K::Space => {
                if self.mode == Mode::Remote && hotkey {
                    out.push(OskAction::Chord(Chord {
                        mods: self.mods_now(),
                        key: ChordKey::Control(ControlKey::Space),
                    }));
                } else {
                    out.push(OskAction::Text(" ".into()));
                }
                self.consume_one_shots();
            }
            K::Backspace => {
                if self.mode == Mode::Remote {
                    out.push(OskAction::Chord(Chord {
                        mods: self.mods_now(),
                        key: ChordKey::Control(ControlKey::Backspace),
                    }));
                } else {
                    out.push(OskAction::Backspace);
                }
                self.consume_one_shots();
            }
            K::Enter => {
                if self.mode == Mode::Remote {
                    out.push(OskAction::Chord(Chord {
                        mods: self.mods_now(),
                        key: ChordKey::Control(ControlKey::Return),
                    }));
                } else {
                    out.push(OskAction::Submit);
                }
                self.consume_one_shots();
            }
            K::Layout => {
                self.layout = match self.layout {
                    KeyboardLayout::EnUs => KeyboardLayout::Hu,
                    KeyboardLayout::Hu => KeyboardLayout::EnUs,
                };
                out.push(OskAction::LayoutChanged(self.layout));
            }
            K::Expand => self.expanded = !self.expanded,
            K::Numpad => self.numpad = !self.numpad,
            K::Close => out.push(OskAction::Close),
        }
    }

    /// Draw and handle the keyboard; returns the actions triggered this frame.
    pub fn draw(&mut self, f: &mut Frame) -> Vec<OskAction> {
        let mut out = Vec::new();
        let h = self.height();
        let top = H - h;
        f.g.fill(
            Rect::new(0, top, W as u32, h as u32),
            Color::RGBA(14, 22, 40, 245),
        );
        f.g.fill(Rect::new(0, top, W as u32, 3), theme::ACCENT);
        let key_h = 64;
        let gap = 10;
        let mut y = top + 18;
        let side_w = if self.numpad { 4 * 86 + 30 } else { 0 } + if self.expanded { 3 * 104 + 30 } else { 0 };
        let main_w = W - 60 - side_w;
        let mut kid = 0usize;

        // Shortcut row (remote only).
        if self.mode == Mode::Remote {
            let mut x = 30;
            let cad = Rect::new(x, y, 250, key_h as u32);
            if f.key_button(idx("osk-cad", 0), cad, "Ctrl+Alt+Del", None, "") {
                out.push(OskAction::CtrlAltDel);
            }
            x += 260;
            let mut remove: Option<usize> = None;
            for (i, s) in self.shortcuts.iter().enumerate() {
                let w = (f.g.text_width(&s.label, size::SMALL, true) + 40).min(360);
                if x + w > main_w - 260 {
                    break;
                }
                let r = Rect::new(x, y, w as u32, key_h as u32);
                let label = if self.remove_mode {
                    format!("✕ {}", s.label)
                } else {
                    s.label.clone()
                };
                let (foc, act) = f.register(idx("osk-sc", i), r);
                f.g.round(
                    r,
                    12,
                    if self.remove_mode {
                        theme::DANGER
                    } else if foc {
                        theme::PANEL_HI
                    } else {
                        theme::ROW
                    },
                );
                if foc {
                    f.focus_ring(r, 12);
                }
                f.g.text_centered(&label, r, size::SMALL, true, theme::TEXT);
                if act {
                    if self.remove_mode {
                        remove = Some(i);
                    } else if let Some(c) = parse_chord(&s.keys) {
                        out.push(OskAction::Chord(c));
                    }
                }
                x += w + gap;
            }
            if let Some(i) = remove {
                self.shortcuts.remove(i);
                out.push(OskAction::ShortcutsChanged(self.shortcuts.clone()));
            }
            let save_r = Rect::new(main_w - 250, y, 120, key_h as u32);
            if f.button(idx("osk-sc-save", 0), save_r, "+ Save", false) {
                if let Some(keys) = self.last_chord.clone() {
                    let label = keys.join("+");
                    if self.shortcuts.len() < 12 {
                        self.shortcuts.push(Shortcut { label, keys });
                        out.push(OskAction::ShortcutsChanged(self.shortcuts.clone()));
                    }
                }
            }
            let rm_r = Rect::new(main_w - 120, y, 150, key_h as u32);
            if f.button(
                idx("osk-sc-rm", 0),
                rm_r,
                if self.remove_mode { "Done" } else { "Remove…" },
                false,
            ) {
                self.remove_mode = !self.remove_mode;
            }
            y += key_h + gap;
        }

        let mut all_rows: Vec<Vec<(K, f32)>> = Vec::new();
        if self.expanded {
            all_rows.push(fn_row());
        }
        all_rows.extend(rows(self.layout, self.mac));
        all_rows.push(bottom_row(self.mode));
        let rows_top = y;
        for row in &all_rows {
            let units: f32 = row.iter().map(|(_, w)| *w).sum();
            let unit = (main_w as f32 - gap as f32 * (row.len() as f32 - 1.0)) / units;
            let mut x: f32 = 30.0;
            for (k, wu) in row {
                let w = unit * wu + gap as f32 * (wu - 1.0).max(0.0);
                let r = Rect::new(x.round() as i32, y, w.round() as u32, key_h as u32);
                let label = self.key_label(k);
                let id = if matches!(k, K::Enter) {
                    crate::ui::id("osk-enter")
                } else {
                    idx("osk-key", kid)
                };
                kid += 1;
                let (foc, act) = f.register(id, r);
                let (bg, state_txt) = match k {
                    K::Mod(i) => match self.mods[*i] {
                        ModState::Off => (
                            if foc {
                                theme::PANEL_HI
                            } else {
                                Color::RGB(40, 55, 85)
                            },
                            "",
                        ),
                        ModState::OneShot => (theme::ACCENT, "once"),
                        ModState::Locked => (Color::RGB(0, 150, 120), "locked"),
                    },
                    K::Ctrl(_, ControlKey::CapsLock) if self.caps => (Color::RGB(0, 150, 120), "on"),
                    K::Close => (Color::RGB(70, 40, 50), ""),
                    K::Ch(..) | K::Space => (if foc { theme::PANEL_HI } else { theme::ROW }, ""),
                    _ => (
                        if foc {
                            theme::PANEL_HI
                        } else {
                            Color::RGB(40, 55, 85)
                        },
                        "",
                    ),
                };
                f.g.round(r, 10, bg);
                if foc {
                    f.focus_ring(r, 10);
                }
                let px = if label.chars().count() > 2 {
                    size::SMALL
                } else {
                    size::LARGE
                };
                if state_txt.is_empty() {
                    f.g.text_centered(&label, r, px, true, theme::TEXT);
                } else {
                    let top_r = Rect::new(r.x(), r.y() - 8, r.width(), r.height());
                    f.g.text_centered(&label, top_r, size::SMALL, true, theme::TEXT);
                    let st_r = Rect::new(r.x(), r.y() + 22, r.width(), r.height() - 22);
                    f.g.text_centered(state_txt, st_r, 18.0, false, theme::TEXT);
                }
                if act {
                    self.press(*k, &mut out);
                }
                x += w + gap as f32;
            }
            y += key_h + gap;
        }

        // Side panels.
        let mut sx = 30 + main_w + 30;
        if self.expanded {
            let mut yy = rows_top + if self.expanded { key_h + gap } else { 0 };
            for row in nav_cluster() {
                let mut x = sx;
                for (k, _) in row {
                    let r = Rect::new(x, yy, 94, key_h as u32);
                    let id = idx("osk-key", kid);
                    kid += 1;
                    let label = self.key_label(&k);
                    let (foc, act) = f.register(id, r);
                    f.g.round(
                        r,
                        10,
                        if foc {
                            theme::PANEL_HI
                        } else {
                            Color::RGB(40, 55, 85)
                        },
                    );
                    if foc {
                        f.focus_ring(r, 10);
                    }
                    f.g.text_centered(&label, r, size::SMALL, true, theme::TEXT);
                    if act {
                        self.press(k, &mut out);
                    }
                    x += 104;
                }
                yy += key_h + gap;
            }
            sx += 3 * 104 + 30;
        }
        if self.numpad {
            let mut yy = rows_top;
            for row in numpad() {
                let mut x = sx;
                for (k, _) in row {
                    let r = Rect::new(x, yy, 76, key_h as u32);
                    let id = idx("osk-key", kid);
                    kid += 1;
                    let label = self.key_label(&k);
                    let (foc, act) = f.register(id, r);
                    f.g.round(r, 10, if foc { theme::PANEL_HI } else { theme::ROW });
                    if foc {
                        f.focus_ring(r, 10);
                    }
                    f.g.text_centered(&label, r, size::BODY, true, theme::TEXT);
                    if act {
                        self.press(k, &mut out);
                    }
                    x += 86;
                }
                yy += key_h + gap;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press_char(o: &mut Osk, n: char) -> Vec<OskAction> {
        let rows = rows(o.layout, false);
        let k = rows
            .iter()
            .flatten()
            .find(|(k, _)| matches!(k, K::Ch(c, _, _) if *c == n))
            .unwrap()
            .0;
        let mut out = Vec::new();
        o.press(k, &mut out);
        out
    }

    #[test]
    fn hungarian_characters_and_shift_one_shot() {
        let mut o = Osk::new(Mode::Remote, KeyboardLayout::Hu);
        for c in ['á', 'é', 'í', 'ó', 'ö', 'ő', 'ú', 'ü', 'ű'] {
            assert_eq!(press_char(&mut o, c), vec![OskAction::Text(c.to_string())]);
        }
        o.press(K::Mod(M_SHIFT), &mut Vec::new());
        assert_eq!(press_char(&mut o, 'ő'), vec![OskAction::Text("Ő".into())]);
        // One-shot consumed.
        assert_eq!(press_char(&mut o, 'ű'), vec![OskAction::Text("ű".into())]);
        // Locked shift persists.
        o.press(K::Mod(M_SHIFT), &mut Vec::new());
        o.press(K::Mod(M_SHIFT), &mut Vec::new());
        assert_eq!(o.mods[M_SHIFT], ModState::Locked);
        assert_eq!(press_char(&mut o, 'á'), vec![OskAction::Text("Á".into())]);
        assert_eq!(press_char(&mut o, 'é'), vec![OskAction::Text("É".into())]);
    }

    #[test]
    fn altgr_level_and_qwertz() {
        let mut o = Osk::new(Mode::Remote, KeyboardLayout::Hu);
        o.press(K::Mod(M_ALTGR), &mut Vec::new());
        assert_eq!(press_char(&mut o, 'v'), vec![OskAction::Text("@".into())]);
        assert_eq!(press_char(&mut o, 'z'), vec![OskAction::Text("z".into())]);
    }

    #[test]
    fn ctrl_shift_esc_chord() {
        let mut o = Osk::new(Mode::Remote, KeyboardLayout::EnUs);
        o.press(K::Mod(M_CTRL), &mut Vec::new());
        o.press(K::Mod(M_SHIFT), &mut Vec::new());
        let mut out = Vec::new();
        o.press(K::Ctrl("Esc", ControlKey::Escape), &mut out);
        match &out[0] {
            OskAction::Chord(c) => {
                assert!(c.mods.ctrl && c.mods.shift);
                assert_eq!(c.key, ChordKey::Control(ControlKey::Escape));
            }
            other => panic!("{other:?}"),
        }
        assert!(o.mods.iter().all(|m| *m == ModState::Off), "one-shots consumed");
    }

    #[test]
    fn ctrl_c_is_a_shortcut_not_text() {
        let mut o = Osk::new(Mode::Remote, KeyboardLayout::EnUs);
        o.press(K::Mod(M_CTRL), &mut Vec::new());
        let out = press_char(&mut o, 'c');
        assert!(matches!(&out[0], OskAction::Chord(c) if c.key == ChordKey::Char('c') && c.mods.ctrl));
    }

    #[test]
    fn local_mode_never_produces_chords() {
        let mut o = Osk::new(Mode::Local, KeyboardLayout::EnUs);
        o.press(K::Mod(M_CTRL), &mut Vec::new());
        let mut out = Vec::new();
        o.press(K::Enter, &mut out);
        o.press(K::Backspace, &mut out);
        assert_eq!(out, vec![OskAction::Submit, OskAction::Backspace]);
    }
}
