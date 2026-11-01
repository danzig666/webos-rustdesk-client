//! Exclusive capture of USB/Bluetooth keyboards and mice during a session (evdev
//! `EVIOCGRAB`).
//!
//! webOS acts on some keyboard keys itself before the app sees them (F2 settings, F11/F12
//! volume, Esc as Back, …). Opening the keyboard's `/dev/input/event*` node and grabbing it
//! makes this app the only reader: the TV no longer sees the keys, and we forward every key
//! to the computer by its physical position. The same technique lets chiaki-lg (a webOS
//! PlayStation Remote Play port) keep controller buttons away from the TV, without root.
//!
//! Only devices that look like real keyboards are taken (letter keys present, USB or
//! Bluetooth bus, not an LG remote), so the Magic Remote keeps working. The grab is held
//! only while a session is in the foreground with no local menu open; leaving the session,
//! opening the toolbar or quitting releases it (the kernel also drops it when the process
//! exits). When the nodes cannot be opened or grabbed, nothing changes: keys keep arriving
//! through SDL as before.
//!
//! Mice are taken the same way (relative X/Y and a left button, same bus and name rules):
//! the TV then stops moving its large pointer for them, and the app moves only the
//! computer's cursor, so a mouse feels like on a PC while the Magic Remote keeps the TV
//! pointer.

use std::{
    fs::File,
    io::Read,
    os::fd::AsRawFd,
    time::{Duration, Instant},
};

const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const REL_HWHEEL: u16 = 0x06;
const REL_WHEEL: u16 = 0x08;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const BUS_USB: u16 = 0x03;
const BUS_BLUETOOTH: u16 = 0x05;
const RESCAN: Duration = Duration::from_secs(2);

// Linux `_IOC(dir, 'E', nr, size)` request numbers (asm-generic layout, as on ARM).
const fn ioc(dir: u32, nr: u32, size: u32) -> u32 {
    (dir << 30) | (size << 16) | ((b'E' as u32) << 8) | nr
}
const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;
const EVIOCGID: u32 = ioc(IOC_READ, 0x02, 8);
const EVIOCGRAB: u32 = ioc(IOC_WRITE, 0x90, 4);
const fn eviocgname(len: u32) -> u32 {
    ioc(IOC_READ, 0x06, len)
}
const fn eviocgbit(ev: u32, len: u32) -> u32 {
    ioc(IOC_READ, 0x20 + ev, len)
}

/// One key from a grabbed keyboard: Linux evdev key code, pressed, released or held
/// (`repeat`: the kernel's auto-repeat while held, sent on as further presses).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvKey {
    pub code: u16,
    pub down: bool,
    pub repeat: bool,
}

/// Mouse buttons a grabbed mouse reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvButton {
    Left,
    Right,
    Middle,
}

/// Input from a grabbed device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrabEvent {
    Key(EvKey),
    /// Relative mouse movement (consecutive reports merged).
    Motion(i32, i32),
    Button(EvButton, bool),
    /// Wheel clicks: horizontal (right +), vertical (up +), like SDL's wheel events.
    Wheel(i32, i32),
}

struct Dev {
    path: String,
    name: String,
    /// Bus and vendor:product, for Diagnostics.
    id: String,
    mouse: bool,
    file: File,
}

#[derive(Default)]
pub struct InputGrab {
    devs: Vec<Dev>,
    /// What is wanted (keyboards, mice); a change releases everything and rescans.
    wanted: (bool, bool),
    last_scan: Option<Instant>,
    /// Nodes looked at and rejected (not a keyboard, or open/grab failed), to skip on rescans.
    skipped: Vec<String>,
    /// For Diagnostics: what is grabbed, or why nothing is.
    pub status: String,
}

fn ioctl(fd: i32, req: u32, arg: *mut libc::c_void) -> bool {
    // SAFETY: every request used here reads into or writes from a buffer of the size its
    // number encodes, owned by the caller for the duration of the call.
    unsafe { libc::ioctl(fd, req as _, arg) >= 0 }
}

fn bit(bits: &[u8], n: usize) -> bool {
    bits.get(n / 8).is_some_and(|b| b & (1 << (n % 8)) != 0)
}

/// A full keyboard: letters, space, Enter and Shift (the Magic Remote and IR receivers lack
/// letter keys), on a USB or Bluetooth bus, and not one of the TV's own input devices by
/// name. On an LG C3 the remote's Back arrives through "IoT keypad", and the Bluetooth stack
/// has a "Bluetooth-audio-source" device; both claim a full key set.
pub fn looks_like_keyboard(name: &str, bus: u16, key_bits: &[u8]) -> bool {
    let lname = name.to_ascii_lowercase();
    let remote = [
        "lge", "magic", "remote", "rcu", "iot", "lot ", "keypad", "audio", "virtual", "uinput",
    ]
    .iter()
    .any(|w| lname.contains(w));
    let keys = [30usize, 44, 57, 28, 42] // KEY_A, KEY_Z, KEY_SPACE, KEY_ENTER, KEY_LEFTSHIFT
        .iter()
        .all(|&k| bit(key_bits, k));
    keys && !remote && matches!(bus, BUS_USB | BUS_BLUETOOTH)
}

/// A mouse: relative X and Y and a left button, on a USB or Bluetooth bus, not one of the
/// TV's own devices by name (the Magic Remote's pointer must stay with the TV).
pub fn looks_like_mouse(name: &str, bus: u16, key_bits: &[u8], rel_bits: &[u8]) -> bool {
    let lname = name.to_ascii_lowercase();
    let tv_own = [
        "lge", "magic", "remote", "rcu", "iot", "lot ", "keypad", "audio", "virtual", "uinput",
    ]
    .iter()
    .any(|w| lname.contains(w));
    bit(rel_bits, REL_X as usize)
        && bit(rel_bits, REL_Y as usize)
        && bit(key_bits, BTN_LEFT as usize)
        && !tv_own
        && matches!(bus, BUS_USB | BUS_BLUETOOTH)
}

/// Decode `input_event`s from raw bytes (`event_size` is the platform struct size): key
/// presses, releases and auto-repeats only. The computer does not repeat keys it is sent as
/// held, so repeats are forwarded as the RustDesk desktop client does.
pub fn decode_events(buf: &[u8], event_size: usize) -> Vec<GrabEvent> {
    let mut out = Vec::new();
    // struct input_event { struct timeval time; __u16 type; __u16 code; __s32 value; }
    let off = event_size - 8;
    for ev in buf.chunks_exact(event_size) {
        let ty = u16::from_ne_bytes([ev[off], ev[off + 1]]);
        let code = u16::from_ne_bytes([ev[off + 2], ev[off + 3]]);
        let value = i32::from_ne_bytes([ev[off + 4], ev[off + 5], ev[off + 6], ev[off + 7]]);
        match (ty, code) {
            (EV_KEY, BTN_LEFT | BTN_RIGHT | BTN_MIDDLE) if value == 0 || value == 1 => {
                let b = match code {
                    BTN_LEFT => EvButton::Left,
                    BTN_RIGHT => EvButton::Right,
                    _ => EvButton::Middle,
                };
                out.push(GrabEvent::Button(b, value == 1));
            }
            (EV_KEY, _) if (0..=2).contains(&value) => out.push(GrabEvent::Key(EvKey {
                code,
                down: value != 0,
                repeat: value == 2,
            })),
            (EV_REL, REL_X | REL_Y) => {
                let (dx, dy) = if code == REL_X { (value, 0) } else { (0, value) };
                if let Some(GrabEvent::Motion(x, y)) = out.last_mut() {
                    *x += dx;
                    *y += dy;
                } else {
                    out.push(GrabEvent::Motion(dx, dy));
                }
            }
            (EV_REL, REL_WHEEL) => out.push(GrabEvent::Wheel(0, value)),
            (EV_REL, REL_HWHEEL) => out.push(GrabEvent::Wheel(value, 0)),
            _ => {}
        }
    }
    out
}

impl InputGrab {
    /// Hold or release the grab of keyboards and of mice; call once per frame.
    pub fn set_active(&mut self, keyboards: bool, mice: bool) {
        let on = keyboards || mice;
        if self.wanted != (keyboards, mice) {
            self.wanted = (keyboards, mice);
            self.release();
        }
        if !on {
            self.release();
            return;
        }
        if self.last_scan.is_some_and(|t| t.elapsed() < RESCAN) {
            return;
        }
        self.last_scan = Some(Instant::now());
        self.scan();
    }

    fn release(&mut self) {
        for d in self.devs.drain(..) {
            ioctl(d.file.as_raw_fd(), EVIOCGRAB, std::ptr::null_mut());
            log::info!("input released: {} ({})", d.name, d.path);
        }
        self.last_scan = None;
        self.skipped.clear();
    }

    pub fn active(&self) -> bool {
        !self.devs.is_empty()
    }

    /// A mouse is currently grabbed.
    pub fn has_mouse(&self) -> bool {
        self.devs.iter().any(|d| d.mouse)
    }

    fn scan(&mut self) {
        let entries = match std::fs::read_dir("/dev/input") {
            Ok(e) => e,
            Err(e) => {
                self.status = format!("cannot list /dev/input ({e})");
                return;
            }
        };
        let mut paths: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path().to_string_lossy().into_owned())
            .filter(|p| p.rsplit('/').next().is_some_and(|n| n.starts_with("event")))
            .collect();
        paths.sort();
        let mut errors = Vec::new();
        for path in paths {
            if self.devs.iter().any(|d| d.path == path) || self.skipped.contains(&path) {
                continue;
            }
            match open_device(&path, self.wanted) {
                Ok(Some(dev)) => {
                    log::info!("input grabbed: {} ({})", dev.name, dev.path);
                    self.devs.push(dev);
                }
                Ok(None) => self.skipped.push(path),
                Err(e) => {
                    errors.push(e);
                    self.skipped.push(path);
                }
            }
        }
        self.status = if !self.devs.is_empty() {
            let names: Vec<String> = self
                .devs
                .iter()
                .map(|d| format!("{} [{}]", d.name, d.id))
                .collect();
            format!("capturing {}", names.join(", "))
        } else if !errors.is_empty() {
            errors.join("; ")
        } else {
            "no keyboard or mouse found".to_owned()
        };
    }

    /// Input read since the last call. A device that went away (switched off) is dropped
    /// and picked up again by a later rescan when it comes back.
    pub fn poll(&mut self) -> Vec<GrabEvent> {
        let size = std::mem::size_of::<libc::input_event>();
        let mut out = Vec::new();
        let mut buf = vec![0u8; size * 64];
        self.devs.retain_mut(|d| loop {
            match d.file.read(&mut buf) {
                Ok(0) => return false,
                Ok(n) => out.extend(decode_events(&buf[..n - n % size], size)),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return true,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => {
                    log::info!("input {} gone: {e}", d.name);
                    return false;
                }
            }
        });
        out
    }
}

impl Drop for InputGrab {
    fn drop(&mut self) {
        self.release();
    }
}

/// Open `path`; `Ok(None)` when it is not a wanted keyboard or mouse, `Err` when one could
/// not be taken.
fn open_device(path: &str, (keyboards, mice): (bool, bool)) -> Result<Option<Dev>, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
    {
        Ok(f) => f,
        // Nodes we may not open are usually not keyboards (sensors, the remote's IR);
        // report it only when nothing at all could be opened.
        Err(e) => return Err(format!("{path}: {e}")),
    };
    let fd = file.as_raw_fd();
    let mut name = [0u8; 128];
    ioctl(fd, eviocgname(name.len() as u32 - 1), name.as_mut_ptr().cast());
    let name = String::from_utf8_lossy(&name[..name.iter().position(|&b| b == 0).unwrap_or(0)]).into_owned();
    let mut id = [0u16; 4];
    if !ioctl(fd, EVIOCGID, id.as_mut_ptr().cast()) {
        return Ok(None);
    }
    let mut keys = [0u8; 96]; // KEY_MAX 0x2ff bits
    if !ioctl(
        fd,
        eviocgbit(EV_KEY as u32, keys.len() as u32),
        keys.as_mut_ptr().cast(),
    ) {
        return Ok(None);
    }
    let mut rels = [0u8; 2]; // REL_MAX 0x0f bits
    let has_rel = ioctl(
        fd,
        eviocgbit(EV_REL as u32, rels.len() as u32),
        rels.as_mut_ptr().cast(),
    );
    let kb = keyboards && looks_like_keyboard(&name, id[0], &keys);
    let mouse = mice && has_rel && looks_like_mouse(&name, id[0], &keys, &rels);
    if !kb && !mouse {
        return Ok(None);
    }
    let mut one: libc::c_int = 1;
    if !ioctl(fd, EVIOCGRAB, (&mut one as *mut libc::c_int).cast()) {
        return Err(format!(
            "{name}: grab refused ({})",
            std::io::Error::last_os_error()
        ));
    }
    Ok(Some(Dev {
        path: path.to_owned(),
        name,
        id: format!(
            "{} {:04x}:{:04x}",
            if id[0] == BUS_USB { "usb" } else { "bt" },
            id[1],
            id[2]
        ),
        mouse,
        file,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_numbers_match_linux_headers() {
        assert_eq!(EVIOCGRAB, 0x4004_4590);
        assert_eq!(EVIOCGID, 0x8008_4502);
        assert_eq!(eviocgname(127), 0x807f_4506);
        assert_eq!(eviocgbit(1, 96), 0x8060_4521);
    }

    fn bits(keys: &[usize]) -> Vec<u8> {
        let mut b = vec![0u8; 96];
        for &k in keys {
            b[k / 8] |= 1 << (k % 8);
        }
        b
    }

    #[test]
    fn keyboards_are_told_from_remotes() {
        let kb = bits(&[30, 44, 57, 28, 42, 1, 59]);
        assert!(looks_like_keyboard("Logitech K380", BUS_BLUETOOTH, &kb));
        assert!(looks_like_keyboard("USB Keyboard", BUS_USB, &kb));
        assert!(!looks_like_keyboard("LGE MRCU", BUS_BLUETOOTH, &kb));
        assert!(!looks_like_keyboard("uinput-virtual", 0x06, &kb));
        assert!(!looks_like_keyboard("IoT keypad", BUS_BLUETOOTH, &kb));
        assert!(!looks_like_keyboard("Bluetooth-audio-source", BUS_BLUETOOTH, &kb));
        assert!(looks_like_keyboard("MX Keys Mini Keyboard", BUS_BLUETOOTH, &kb));
        // A remote: arrows, OK, digits, but no letters.
        let remote = bits(&[103, 108, 105, 106, 28, 2, 3, 4]);
        assert!(!looks_like_keyboard("BT remote", BUS_BLUETOOTH, &remote));
    }

    #[test]
    fn mice_are_told_from_remotes() {
        let keys = bits(&[BTN_LEFT as usize, BTN_RIGHT as usize]);
        let rel = [0b0000_0011u8, 0b0000_0001]; // REL_X, REL_Y, REL_WHEEL
        assert!(looks_like_mouse("Logitech M720", BUS_BLUETOOTH, &keys, &rel));
        assert!(!looks_like_mouse("LGE Magic Remote", BUS_BLUETOOTH, &keys, &rel));
        assert!(!looks_like_mouse("Mouse", 0x06, &keys, &rel));
        assert!(!looks_like_mouse(
            "Logitech M720",
            BUS_BLUETOOTH,
            &bits(&[]),
            &rel
        ));
        assert!(!looks_like_mouse("Logitech M720", BUS_BLUETOOTH, &keys, &[0, 0]));
    }

    #[test]
    fn mouse_events_are_decoded() {
        fn ev(ty: u16, code: u16, value: i32) -> Vec<u8> {
            let mut v = vec![0u8; 8];
            v.extend_from_slice(&ty.to_ne_bytes());
            v.extend_from_slice(&code.to_ne_bytes());
            v.extend_from_slice(&value.to_ne_bytes());
            v
        }
        let mut buf = ev(EV_REL, REL_X, 3);
        buf.extend(ev(EV_REL, REL_Y, -2));
        buf.extend(ev(EV_KEY, BTN_LEFT, 1));
        buf.extend(ev(EV_REL, REL_WHEEL, -1));
        buf.extend(ev(EV_REL, 11, -120)); // REL_WHEEL_HI_RES: ignored
        buf.extend(ev(EV_KEY, BTN_LEFT, 0));
        assert_eq!(
            decode_events(&buf, 16),
            vec![
                GrabEvent::Motion(3, -2),
                GrabEvent::Button(EvButton::Left, true),
                GrabEvent::Wheel(0, -1),
                GrabEvent::Button(EvButton::Left, false),
            ]
        );
    }

    #[test]
    fn events_are_decoded() {
        fn ev(size: usize, ty: u16, code: u16, value: i32) -> Vec<u8> {
            let mut v = vec![0u8; size - 8];
            v.extend_from_slice(&ty.to_ne_bytes());
            v.extend_from_slice(&code.to_ne_bytes());
            v.extend_from_slice(&value.to_ne_bytes());
            v
        }
        for size in [16, 24] {
            let mut buf = ev(size, 4, 4, 0x70029); // EV_MSC scan
            buf.extend(ev(size, EV_KEY, 1, 1)); // Esc down
            buf.extend(ev(size, 0, 0, 0)); // SYN
            buf.extend(ev(size, EV_KEY, 1, 2)); // auto-repeat
            buf.extend(ev(size, EV_KEY, 1, 0)); // Esc up
            let k = |down, repeat| {
                GrabEvent::Key(EvKey {
                    code: 1,
                    down,
                    repeat,
                })
            };
            assert_eq!(
                decode_events(&buf, size),
                vec![k(true, false), k(true, true), k(false, false)]
            );
        }
    }
}
