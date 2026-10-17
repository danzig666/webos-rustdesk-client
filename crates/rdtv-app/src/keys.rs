//! Platform input decoding.
//!
//! webOS remote buttons arrive from the SDL-webOS fork as keyboard events with fork-specific
//! scancodes (SDL2 2.30.x fork, `include/SDL_scancode.h`): HOME 384, CH_UP 480,
//! CH_DOWN 481, BACK 482, RED 486, GREEN 487, YELLOW 488, BLUE 489, EXIT 505. The `sdl2`
//! crate has no variants for them, so raw events are inspected before conversion.
//! On a desktop build (simulator) F1..F4 stand in for the colour keys and F9 for Back.

use sdl2::{
    event::Event,
    keyboard::{Keycode, Mod, Scancode},
    mouse::MouseButton as SdlButton,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remote {
    Up,
    Down,
    Left,
    Right,
    Ok,
    Back,
    Red,
    Green,
    Yellow,
    Blue,
    Exit,
    Home,
    ChUp,
    ChDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtrButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyMods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub gui: bool,
    pub altgr: bool,
    pub caps: bool,
}

impl KeyMods {
    fn from_sdl(m: Mod) -> KeyMods {
        KeyMods {
            ctrl: m.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD),
            alt: m.intersects(Mod::LALTMOD),
            altgr: m.intersects(Mod::RALTMOD | Mod::MODEMOD),
            shift: m.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD),
            gui: m.intersects(Mod::LGUIMOD | Mod::RGUIMOD),
            caps: m.intersects(Mod::CAPSMOD),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum In {
    /// TV remote buttons (and arrows/Enter, which a remote and a keyboard share).
    Remote {
        key: Remote,
        down: bool,
        repeat: bool,
    },
    /// A physical keyboard key that is not a shared navigation key.
    Key {
        key: Keycode,
        scancode: Option<Scancode>,
        mods: KeyMods,
        down: bool,
        repeat: bool,
    },
    Text(String),
    PointerMove {
        x: i32,
        y: i32,
    },
    PointerButton {
        button: PtrButton,
        down: bool,
        x: i32,
        y: i32,
    },
    Wheel {
        dx: i32,
        dy: i32,
    },
    Focus(bool),
    Quit,
}

const WEBOS_HOME: i32 = 384;
const WEBOS_CH_UP: i32 = 480;
const WEBOS_CH_DOWN: i32 = 481;
const WEBOS_BACK: i32 = 482;
const WEBOS_RED: i32 = 486;
const WEBOS_GREEN: i32 = 487;
const WEBOS_YELLOW: i32 = 488;
const WEBOS_BLUE: i32 = 489;
const WEBOS_EXIT: i32 = 505;

pub fn webos_remote_from_scancode(sc: i32) -> Option<Remote> {
    Some(match sc {
        WEBOS_HOME => Remote::Home,
        WEBOS_CH_UP => Remote::ChUp,
        WEBOS_CH_DOWN => Remote::ChDown,
        WEBOS_BACK => Remote::Back,
        WEBOS_RED => Remote::Red,
        WEBOS_GREEN => Remote::Green,
        WEBOS_YELLOW => Remote::Yellow,
        WEBOS_BLUE => Remote::Blue,
        WEBOS_EXIT => Remote::Exit,
        _ => return None,
    })
}

pub fn is_webos() -> bool {
    cfg!(target_arch = "arm")
}

/// Poll all pending SDL events and translate them.
/// Which `SDL_KeyboardEvent` layout the loaded libSDL2 uses.
///
/// LG's own SDL2 (in the TV's /usr/lib) inserts `Uint32 inputSource` after `windowID` in
/// keyboard, text-editing and text-input events (openlgtv/buildroot-nc4
/// `package/sdl2/webos/001-webos-input-structs.patch`). The bundled webosbrew backport uses
/// upstream's layout. If the system library is loaded anyway (e.g. a launcher
/// `LD_LIBRARY_PATH` winning over our rpath), decoding with the wrong layout shifts every
/// key code: remote buttons stop working and digits turn into garbage. The layout is
/// therefore chosen from the version of the library actually loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAbi {
    Upstream,
    Lg,
}

static KEY_ABI: std::sync::OnceLock<KeyAbi> = std::sync::OnceLock::new();

/// Detect (once) and log the loaded SDL version and keyboard event layout.
pub fn init_key_abi() -> (KeyAbi, String) {
    let mut v = sdl2::sys::SDL_version {
        major: 0,
        minor: 0,
        patch: 0,
    };
    unsafe { sdl2::sys::SDL_GetVersion(&mut v) };
    let forced = std::env::var("RDTV_SDL_ABI").ok();
    let abi = match forced.as_deref() {
        Some("lg") => KeyAbi::Lg,
        Some("upstream") => KeyAbi::Upstream,
        // LG's firmware SDL2 builds are 2.0.x (2.0.10 on webOS 5/6, 2.0.14 on 7+); the
        // bundled backport is 2.30.12.
        _ if is_webos() && v.major == 2 && v.minor == 0 => KeyAbi::Lg,
        _ => KeyAbi::Upstream,
    };
    let _ = KEY_ABI.set(abi);
    let desc = format!(
        "SDL {}.{}.{} ({:?} keyboard event layout)",
        v.major, v.minor, v.patch, abi
    );
    (abi, desc)
}

fn key_abi() -> KeyAbi {
    *KEY_ABI.get().unwrap_or(&KeyAbi::Upstream)
}

/// The last few raw key events, shown in Diagnostics so a keyboard's behaviour on a real TV
/// (which keys arrive, with which codes and modifiers) can be reported.
static KEY_LOG: std::sync::Mutex<std::collections::VecDeque<String>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());
const KEY_LOG_LEN: usize = 10;

fn log_key(k: &RawKey) {
    if k.repeat {
        return;
    }
    // Never `Keycode::name()`: for webOS remote keys SDL builds the name from an xkb keysym
    // outside Unicode, and the crate's UTF-8 unwrap then panics (0.2.0 crashed on Back).
    let name = if (0..512).contains(&k.scancode) {
        extern "C" {
            // Declared with a plain int: webOS scancodes are not variants of the bindgen enum.
            fn SDL_GetScancodeName(scancode: std::ffi::c_int) -> *const std::ffi::c_char;
        }
        let p = unsafe { SDL_GetScancodeName(k.scancode) };
        if p.is_null() {
            String::new()
        } else {
            unsafe { std::ffi::CStr::from_ptr(p) }
                .to_string_lossy()
                .into_owned()
        }
    } else {
        String::new()
    };
    let name = if name.is_empty() {
        format!("key 0x{:x}", k.sym)
    } else {
        format!("{name} (0x{:x})", k.sym)
    };
    let line = format!(
        "{}{} sc{} m{:x}",
        if k.down { "+" } else { "-" },
        name,
        k.scancode,
        k.modstate
    );
    if let Ok(mut l) = KEY_LOG.lock() {
        if l.len() >= KEY_LOG_LEN {
            l.pop_front();
        }
        l.push_back(line);
    }
}

/// Recent raw key events, oldest first.
pub fn recent_keys() -> Vec<String> {
    KEY_LOG
        .lock()
        .map(|l| l.iter().cloned().collect())
        .unwrap_or_default()
}

struct RawKey {
    down: bool,
    repeat: bool,
    scancode: i32,
    sym: i32,
    modstate: u16,
}

/// Read a keyboard event from raw event bytes using the detected layout.
fn decode_key(raw: &sdl2::sys::SDL_Event, down: bool) -> RawKey {
    let p = raw as *const sdl2::sys::SDL_Event as *const u8;
    // Upstream: state@12 repeat@13 keysym{scancode@16 sym@20 mod@24}; LG: everything +4.
    let off = match key_abi() {
        KeyAbi::Upstream => 0,
        KeyAbi::Lg => 4,
    };
    // SAFETY: SDL_Event is a 56-byte union; all offsets read are below 32.
    unsafe {
        RawKey {
            down,
            repeat: *p.add(13 + off) != 0,
            scancode: std::ptr::read_unaligned(p.add(16 + off) as *const i32),
            sym: std::ptr::read_unaligned(p.add(20 + off) as *const i32),
            modstate: std::ptr::read_unaligned(p.add(24 + off) as *const u16),
        }
    }
}

/// Poll all pending SDL events and translate them.
pub fn poll(out: &mut Vec<In>) {
    use sdl2::sys::SDL_EventType as T;
    loop {
        let mut raw = std::mem::MaybeUninit::<sdl2::sys::SDL_Event>::uninit();
        // SAFETY: SDL_PollEvent fully initializes the event when it returns 1.
        if unsafe { sdl2::sys::SDL_PollEvent(raw.as_mut_ptr()) } == 0 {
            break;
        }
        let raw = unsafe { raw.assume_init() };
        let ty = unsafe { raw.type_ };
        if ty == T::SDL_KEYDOWN as u32 || ty == T::SDL_KEYUP as u32 {
            let k = decode_key(&raw, ty == T::SDL_KEYDOWN as u32);
            log::trace!("key sc={} sym={} down={}", k.scancode, k.sym, k.down);
            log_key(&k);
            if let Some(r) = webos_remote_from_scancode(k.scancode) {
                out.push(In::Remote {
                    key: r,
                    down: k.down,
                    repeat: k.repeat,
                });
                continue;
            }
            if let Some(key) = Keycode::from_i32(k.sym) {
                translate_key(
                    key,
                    Scancode::from_i32(k.scancode),
                    Mod::from_bits_truncate(k.modstate),
                    k.down,
                    k.repeat,
                    out,
                );
            }
            continue;
        }
        if (ty == T::SDL_TEXTINPUT as u32 || ty == T::SDL_TEXTEDITING as u32) && key_abi() == KeyAbi::Lg {
            continue; // text payload offset differs under LG's layout; text comes from keys
        }
        translate(Event::from_ll(raw), out);
    }
}

fn translate_key(
    key: Keycode,
    scancode: Option<Scancode>,
    keymod: Mod,
    down: bool,
    repeat: bool,
    out: &mut Vec<In>,
) {
    let shared = match key {
        Keycode::Up => Some(Remote::Up),
        Keycode::Down => Some(Remote::Down),
        Keycode::Left => Some(Remote::Left),
        Keycode::Right => Some(Remote::Right),
        Keycode::Return | Keycode::KpEnter => Some(Remote::Ok),
        // The remote's Back button has its own webOS scancode (handled before this); on the
        // TV an AC Back key comes from a keyboard's Esc, which belongs to the computer.
        Keycode::AcBack if is_webos() => {
            return out.push(In::Key {
                key: Keycode::Escape,
                scancode: Some(Scancode::Escape),
                mods: KeyMods::from_sdl(keymod),
                down,
                repeat,
            })
        }
        Keycode::AcBack => Some(Remote::Back),
        _ if !is_webos() => match key {
            Keycode::F1 => Some(Remote::Red),
            Keycode::F2 => Some(Remote::Green),
            Keycode::F3 => Some(Remote::Yellow),
            Keycode::F4 => Some(Remote::Blue),
            Keycode::F9 => Some(Remote::Back),
            _ => None,
        },
        _ => None,
    };
    let mods = KeyMods::from_sdl(keymod);
    match shared {
        // Arrows/Enter with modifiers are keyboard shortcuts, not remote navigation.
        Some(r)
            if !(mods.ctrl || mods.alt || mods.gui || mods.shift)
                || !matches!(
                    r,
                    Remote::Up | Remote::Down | Remote::Left | Remote::Right | Remote::Ok
                ) =>
        {
            out.push(In::Remote { key: r, down, repeat })
        }
        _ => out.push(In::Key {
            key,
            scancode,
            mods,
            down,
            repeat,
        }),
    }
}

fn translate(ev: Event, out: &mut Vec<In>) {
    match ev {
        Event::Quit { .. } => out.push(In::Quit),
        Event::TextInput { text, .. } => out.push(In::Text(text)),
        Event::MouseMotion { x, y, .. } => out.push(In::PointerMove { x, y }),
        Event::MouseButtonDown { mouse_btn, x, y, .. } | Event::MouseButtonUp { mouse_btn, x, y, .. } => {
            let down = matches!(ev, Event::MouseButtonDown { .. });
            let button = match mouse_btn {
                SdlButton::Left => PtrButton::Left,
                SdlButton::Right => PtrButton::Right,
                SdlButton::Middle => PtrButton::Middle,
                _ => return,
            };
            out.push(In::PointerButton { button, down, x, y });
        }
        Event::MouseWheel { x, y, .. } => out.push(In::Wheel { dx: x, dy: y }),
        Event::Window { win_event, .. } => match win_event {
            sdl2::event::WindowEvent::FocusGained | sdl2::event::WindowEvent::Shown => {
                out.push(In::Focus(true))
            }
            sdl2::event::WindowEvent::FocusLost | sdl2::event::WindowEvent::Hidden => {
                out.push(In::Focus(false))
            }
            _ => {}
        },
        Event::AppWillEnterBackground { .. } | Event::AppDidEnterBackground { .. } => {
            out.push(In::Focus(false))
        }
        Event::AppDidEnterForeground { .. } => out.push(In::Focus(true)),
        _ => {}
    }
}

/// The digit of a number key. The Magic Remote's number buttons arrive through the
/// SDL-webOS fork's default XKB mapping as ordinary digit keys (the fork maps only the
/// colour/navigation/media buttons specially); some remotes and USB keypads report keypad
/// codes instead, so both are accepted.
pub fn digit_of(key: Keycode) -> Option<char> {
    Some(match key {
        Keycode::Num0 | Keycode::Kp0 => '0',
        Keycode::Num1 | Keycode::Kp1 => '1',
        Keycode::Num2 | Keycode::Kp2 => '2',
        Keycode::Num3 | Keycode::Kp3 => '3',
        Keycode::Num4 | Keycode::Kp4 => '4',
        Keycode::Num5 | Keycode::Kp5 => '5',
        Keycode::Num6 | Keycode::Kp6 => '6',
        Keycode::Num7 | Keycode::Kp7 => '7',
        Keycode::Num8 | Keycode::Kp8 => '8',
        Keycode::Num9 | Keycode::Kp9 => '9',
        _ => return None,
    })
}

/// Map a physical keyboard key to the remote key it should send.
pub enum RemoteKey {
    Control(rdtv_core::message_proto::ControlKey),
    Char(char),
}

pub fn physical_to_remote(key: Keycode, mods: &KeyMods) -> Option<RemoteKey> {
    use rdtv_core::message_proto::ControlKey as C;
    let ck = match key {
        Keycode::Escape => C::Escape,
        Keycode::Tab => C::Tab,
        Keycode::Backspace => C::Backspace,
        Keycode::Delete => C::Delete,
        Keycode::Insert => C::Insert,
        Keycode::Home => C::Home,
        Keycode::End => C::End,
        Keycode::PageUp => C::PageUp,
        Keycode::PageDown => C::PageDown,
        Keycode::Up => C::UpArrow,
        Keycode::Down => C::DownArrow,
        Keycode::Left => C::LeftArrow,
        Keycode::Right => C::RightArrow,
        Keycode::Return => C::Return,
        Keycode::KpEnter => C::NumpadEnter,
        Keycode::Space if mods.ctrl || mods.alt || mods.gui => C::Space,
        Keycode::CapsLock => C::CapsLock,
        Keycode::NumLockClear => C::NumLock,
        Keycode::ScrollLock => C::Scroll,
        Keycode::PrintScreen => C::Snapshot,
        Keycode::Pause => C::Pause,
        Keycode::Application | Keycode::Menu => C::Apps,
        Keycode::F1 => C::F1,
        Keycode::F2 => C::F2,
        Keycode::F3 => C::F3,
        Keycode::F4 => C::F4,
        Keycode::F5 => C::F5,
        Keycode::F6 => C::F6,
        Keycode::F7 => C::F7,
        Keycode::F8 => C::F8,
        Keycode::F9 => C::F9,
        Keycode::F10 => C::F10,
        Keycode::F11 => C::F11,
        Keycode::F12 => C::F12,
        Keycode::LCtrl | Keycode::RCtrl => C::Control,
        Keycode::LShift => C::Shift,
        Keycode::RShift => C::RShift,
        Keycode::LAlt => C::Alt,
        Keycode::RAlt => C::RAlt,
        Keycode::LGui | Keycode::RGui => C::Meta,
        _ => {
            // Character keys only matter here when a hotkey modifier is held; plain text
            // arrives through TextInput.
            if mods.ctrl || mods.alt || mods.gui {
                let n = key.into_i32();
                if (0x20..0x7f).contains(&n) {
                    return Some(RemoteKey::Char((n as u8 as char).to_ascii_lowercase()));
                }
            }
            return None;
        }
    };
    Some(RemoteKey::Control(ck))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_log_survives_webos_remote_keys() {
        // 0.2.0 panicked here on the TV's Back key (name built from a non-Unicode keysym).
        for (sc, sym) in [(482, 0x4000_01e2), (482, 0x20_0001), (41, 27), (9999, -5)] {
            log_key(&RawKey {
                down: true,
                repeat: false,
                scancode: sc,
                sym,
                modstate: 0,
            });
        }
        let keys = recent_keys();
        assert!(keys.iter().any(|k| k.contains("sc482")));
        assert!(keys.iter().any(|k| k.contains("Escape")));
    }

    #[test]
    fn webos_colour_and_back_scancodes() {
        assert_eq!(webos_remote_from_scancode(486), Some(Remote::Red));
        assert_eq!(webos_remote_from_scancode(487), Some(Remote::Green));
        assert_eq!(webos_remote_from_scancode(488), Some(Remote::Yellow));
        assert_eq!(webos_remote_from_scancode(489), Some(Remote::Blue));
        assert_eq!(webos_remote_from_scancode(482), Some(Remote::Back));
        assert_eq!(webos_remote_from_scancode(4), None);
    }

    #[test]
    fn remote_number_buttons_are_digits() {
        assert_eq!(digit_of(Keycode::Num7), Some('7'));
        assert_eq!(digit_of(Keycode::Kp0), Some('0'));
        assert_eq!(digit_of(Keycode::A), None);
    }

    #[test]
    fn hotkey_letters_are_sent_as_chars() {
        let m = KeyMods {
            ctrl: true,
            ..Default::default()
        };
        assert!(matches!(
            physical_to_remote(Keycode::C, &m),
            Some(RemoteKey::Char('c'))
        ));
        assert!(physical_to_remote(Keycode::C, &KeyMods::default()).is_none());
        assert!(matches!(
            physical_to_remote(Keycode::F5, &KeyMods::default()),
            Some(RemoteKey::Control(_))
        ));
    }
}
