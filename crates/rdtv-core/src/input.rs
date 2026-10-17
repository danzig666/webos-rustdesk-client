//! Construction of RustDesk input messages and tracking of held remote input.
//!
//! Key delivery uses upstream's *Legacy* keyboard mode (`KeyboardMode::Legacy`):
//! * non-character keys are `ControlKey`s;
//! * shortcuts with Ctrl/Alt/Meta send the base character as `chr` plus `modifiers`;
//! * text (including Hungarian `áéíóöőúüű`) is sent as `seq`, which the host types as text
//!   regardless of the host keyboard layout.
//!
//! Modifiers travel *with each key event* (`KeyEvent.modifiers`); the host presses them for
//! that event and releases them afterwards (`sync_modifiers`/`release_keys` in upstream
//! `src/server/input_service.rs`). So a locally latched modifier on the on-screen keyboard
//! never leaves a key held on the host while the user navigates.
//!
//! Mouse masks follow upstream `src/common.rs`: `mask = (buttons << 3) | type`.

use base::message_proto::*;
use hbb_common::protobuf::EnumOrUnknown;
use std::collections::BTreeSet;

pub const MOUSE_TYPE_MOVE: i32 = 0;
pub const MOUSE_TYPE_DOWN: i32 = 1;
pub const MOUSE_TYPE_UP: i32 = 2;
pub const MOUSE_TYPE_WHEEL: i32 = 3;
pub const MOUSE_BUTTON_LEFT: i32 = 0x01;
pub const MOUSE_BUTTON_RIGHT: i32 = 0x02;
pub const MOUSE_BUTTON_WHEEL: i32 = 0x04;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    fn bits(self) -> i32 {
        match self {
            MouseButton::Left => MOUSE_BUTTON_LEFT,
            MouseButton::Right => MOUSE_BUTTON_RIGHT,
            MouseButton::Middle => MOUSE_BUTTON_WHEEL,
        }
    }
}

/// Local modifier state attached to events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub altgr: bool,
}

impl Mods {
    pub fn any_hotkey(&self) -> bool {
        self.ctrl || self.alt || self.meta
    }

    fn to_proto(self) -> Vec<EnumOrUnknown<ControlKey>> {
        let mut v = Vec::new();
        if self.ctrl {
            v.push(ControlKey::Control.into());
        }
        if self.alt {
            v.push(ControlKey::Alt.into());
        }
        if self.altgr {
            v.push(ControlKey::RAlt.into());
        }
        if self.shift {
            v.push(ControlKey::Shift.into());
        }
        if self.meta {
            v.push(ControlKey::Meta.into());
        }
        v
    }
}

pub fn mouse_msg(mask: i32, x: i32, y: i32, mods: Mods) -> Message {
    let mut msg = Message::new();
    msg.set_mouse_event(MouseEvent {
        mask,
        x,
        y,
        modifiers: mods.to_proto(),
        ..Default::default()
    });
    msg
}

pub fn mouse_move(x: i32, y: i32) -> Message {
    mouse_msg(MOUSE_TYPE_MOVE, x, y, Mods::default())
}

pub fn mouse_button(btn: MouseButton, down: bool, x: i32, y: i32, mods: Mods) -> Message {
    let t = if down { MOUSE_TYPE_DOWN } else { MOUSE_TYPE_UP };
    mouse_msg((btn.bits() << 3) | t, x, y, mods)
}

/// Wheel: upstream sends the delta in x/y of a WHEEL event (positive y scrolls up).
pub fn mouse_wheel(dx: i32, dy: i32) -> Message {
    mouse_msg(MOUSE_TYPE_WHEEL, dx, dy, Mods::default())
}

pub fn control_key(key: ControlKey, down: bool, press: bool, mods: Mods) -> Message {
    let mut ev = KeyEvent::new();
    ev.set_control_key(key);
    ev.down = down;
    ev.press = press;
    ev.modifiers = mods.to_proto();
    ev.mode = KeyboardMode::Legacy.into();
    let mut msg = Message::new();
    msg.set_key_event(ev);
    msg
}

/// A character key carrying hotkey modifiers (e.g. Ctrl+C). `c` should be the unshifted
/// base character as it appears on a US keyboard.
pub fn chr_key(c: char, down: bool, press: bool, mods: Mods) -> Message {
    let mut ev = KeyEvent::new();
    ev.set_chr(c as u32);
    ev.down = down;
    ev.press = press;
    ev.modifiers = mods.to_proto();
    ev.mode = KeyboardMode::Legacy.into();
    let mut msg = Message::new();
    msg.set_key_event(ev);
    msg
}

/// Text typed as text on the host.
pub fn text(seq: &str) -> Message {
    let mut ev = KeyEvent::new();
    ev.set_seq(seq.to_owned());
    ev.press = true;
    ev.mode = KeyboardMode::Legacy.into();
    let mut msg = Message::new();
    msg.set_key_event(ev);
    msg
}

/// The genuine upstream privileged host action (handled by the host's
/// `ControlKey::CtrlAltDel` path, subject to its permissions).
pub fn ctrl_alt_del() -> Message {
    control_key(ControlKey::CtrlAltDel, true, true, Mods::default())
}

pub fn lock_screen() -> Message {
    control_key(ControlKey::LockScreen, true, true, Mods::default())
}

/// Named keys used by the on-screen keyboard, shortcuts and physical-key mapping.
pub fn control_key_by_name(name: &str) -> Option<ControlKey> {
    use ControlKey::*;
    Some(match name {
        "Esc" | "Escape" => Escape,
        "Tab" => Tab,
        "Enter" | "Return" => Return,
        "Backspace" => Backspace,
        "Delete" | "Del" => Delete,
        "Insert" | "Ins" => Insert,
        "Home" => Home,
        "End" => End,
        "PageUp" | "PgUp" => PageUp,
        "PageDown" | "PgDn" => PageDown,
        "Left" => LeftArrow,
        "Right" => RightArrow,
        "Up" => UpArrow,
        "Down" => DownArrow,
        "Space" => Space,
        "CapsLock" | "Caps" => CapsLock,
        "NumLock" => NumLock,
        "ScrollLock" => Scroll,
        "PrintScreen" | "PrtSc" => Snapshot,
        "Pause" => Pause,
        "Menu" | "Apps" => Apps,
        "F1" => F1,
        "F2" => F2,
        "F3" => F3,
        "F4" => F4,
        "F5" => F5,
        "F6" => F6,
        "F7" => F7,
        "F8" => F8,
        "F9" => F9,
        "F10" => F10,
        "F11" => F11,
        "F12" => F12,
        "Num0" => Numpad0,
        "Num1" => Numpad1,
        "Num2" => Numpad2,
        "Num3" => Numpad3,
        "Num4" => Numpad4,
        "Num5" => Numpad5,
        "Num6" => Numpad6,
        "Num7" => Numpad7,
        "Num8" => Numpad8,
        "Num9" => Numpad9,
        "Num*" => Multiply,
        "Num+" => Add,
        "Num-" => Subtract,
        "Num." => Decimal,
        "Num/" => Divide,
        "NumEnter" => NumpadEnter,
        _ => return None,
    })
}

/// A parsed shortcut: modifiers plus one target key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChordKey {
    Control(ControlKey),
    Char(char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub mods: Mods,
    pub key: ChordKey,
}

pub fn parse_chord(keys: &[String]) -> Option<Chord> {
    let mut mods = Mods::default();
    let mut target = None;
    for k in keys {
        match k.as_str() {
            "Ctrl" | "Control" => mods.ctrl = true,
            "Alt" | "Option" => mods.alt = true,
            "AltGr" => mods.altgr = true,
            "Shift" => mods.shift = true,
            "Meta" | "Win" | "Super" | "Cmd" | "Command" => mods.meta = true,
            other => {
                if target.is_some() {
                    return None;
                }
                target = Some(if let Some(ck) = control_key_by_name(other) {
                    ChordKey::Control(ck)
                } else {
                    let mut it = other.chars();
                    let c = it.next()?;
                    if it.next().is_some() {
                        return None;
                    }
                    ChordKey::Char(c.to_ascii_lowercase())
                });
            }
        }
    }
    let key = match target {
        Some(k) => k,
        // A bare modifier press (e.g. just "Meta" to open the Start menu).
        None if mods.meta => {
            mods.meta = false;
            ChordKey::Control(ControlKey::Meta)
        }
        None => return None,
    };
    Some(Chord { mods, key })
}

/// Messages to click a chord once (press semantics: host presses modifiers, the key down+up,
/// then releases the modifiers).
pub fn chord_messages(chord: &Chord) -> Vec<Message> {
    match &chord.key {
        ChordKey::Control(ck) => vec![control_key(*ck, true, true, chord.mods)],
        ChordKey::Char(c) => vec![chr_key(*c, true, true, chord.mods)],
    }
}

/// Tracks remote input that is currently held so it can be released on focus loss,
/// modal entry, mode switches and disconnect. Releases are only meaningful while the
/// transport is alive; after a loss the tracker is simply cleared and never replayed.
#[derive(Debug, Default)]
pub struct HeldInput {
    buttons: BTreeSet<MouseButton>,
    keys: BTreeSet<i32>,
    last_pos: (i32, i32),
}

impl HeldInput {
    pub fn note_button(&mut self, b: MouseButton, down: bool, x: i32, y: i32) {
        self.last_pos = (x, y);
        if down {
            self.buttons.insert(b);
        } else {
            self.buttons.remove(&b);
        }
    }

    pub fn note_pos(&mut self, x: i32, y: i32) {
        self.last_pos = (x, y);
    }

    pub fn note_control_key(&mut self, k: ControlKey, down: bool) {
        if down {
            self.keys.insert(k as i32);
        } else {
            self.keys.remove(&(k as i32));
        }
    }

    pub fn is_button_down(&self, b: MouseButton) -> bool {
        self.buttons.contains(&b)
    }

    pub fn anything_held(&self) -> bool {
        !self.buttons.is_empty() || !self.keys.is_empty()
    }

    /// Release messages for everything held, and clear the state.
    pub fn release_all(&mut self) -> Vec<Message> {
        let (x, y) = self.last_pos;
        let mut out: Vec<Message> = self
            .buttons
            .iter()
            .map(|b| mouse_button(*b, false, x, y, Mods::default()))
            .collect();
        for k in self.keys.iter() {
            if let Some(ck) = control_key_from_i32(*k) {
                out.push(control_key(ck, false, false, Mods::default()));
            }
        }
        self.buttons.clear();
        self.keys.clear();
        out
    }

    /// Forget held state without sending anything (transport already gone).
    pub fn clear(&mut self) {
        self.buttons.clear();
        self.keys.clear();
    }
}

fn control_key_from_i32(v: i32) -> Option<ControlKey> {
    use hbb_common::protobuf::Enum;
    ControlKey::from_i32(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn mouse_mask_encoding() {
        let m = mouse_button(MouseButton::Right, true, 1, 2, Mods::default());
        assert_eq!(m.mouse_event().mask, (MOUSE_BUTTON_RIGHT << 3) | MOUSE_TYPE_DOWN);
        let m = mouse_button(MouseButton::Left, false, 1, 2, Mods::default());
        assert_eq!(m.mouse_event().mask, (MOUSE_BUTTON_LEFT << 3) | MOUSE_TYPE_UP);
        assert_eq!(mouse_wheel(0, -1).mouse_event().mask, MOUSE_TYPE_WHEEL);
    }

    #[test]
    fn chords() {
        let c = parse_chord(&s(&["Ctrl", "Shift", "Esc"])).unwrap();
        assert!(c.mods.ctrl && c.mods.shift && !c.mods.alt);
        assert_eq!(c.key, ChordKey::Control(ControlKey::Escape));
        let msgs = chord_messages(&c);
        let ke = msgs[0].key_event();
        assert!(ke.press);
        assert_eq!(ke.modifiers.len(), 2);

        let c = parse_chord(&s(&["Ctrl", "C"])).unwrap();
        assert_eq!(c.key, ChordKey::Char('c'));
        let c = parse_chord(&s(&["Meta"])).unwrap();
        assert_eq!(c.key, ChordKey::Control(ControlKey::Meta));
        assert!(parse_chord(&s(&["Ctrl"])).is_none());
        assert!(parse_chord(&s(&["A", "B"])).is_none());
    }

    #[test]
    fn hungarian_text_is_sent_as_text() {
        let m = text("áéíóöőúüű ÁÉÍÓÖŐÚÜŰ");
        assert_eq!(m.key_event().seq(), "áéíóöőúüű ÁÉÍÓÖŐÚÜŰ");
        assert_eq!(
            m.key_event().mode.enum_value_or(KeyboardMode::Map),
            KeyboardMode::Legacy
        );
    }

    #[test]
    fn held_input_released_once() {
        let mut h = HeldInput::default();
        h.note_button(MouseButton::Left, true, 10, 20);
        h.note_control_key(ControlKey::Shift, true);
        assert!(h.anything_held());
        let rel = h.release_all();
        assert_eq!(rel.len(), 2);
        assert!(!h.anything_held());
        assert!(h.release_all().is_empty());
    }

    #[test]
    fn privileged_actions_use_upstream_control_keys() {
        assert_eq!(ctrl_alt_del().key_event().control_key(), ControlKey::CtrlAltDel);
        assert_eq!(lock_screen().key_event().control_key(), ControlKey::LockScreen);
    }
}
