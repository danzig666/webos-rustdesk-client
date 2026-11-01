//! Exclusive capture of USB/Bluetooth keyboards during a session (evdev `EVIOCGRAB`).
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

use std::{
    fs::File,
    io::Read,
    os::fd::AsRawFd,
    time::{Duration, Instant},
};

const EV_KEY: u16 = 0x01;
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

/// One key from a grabbed keyboard: Linux evdev key code, pressed or released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvKey {
    pub code: u16,
    pub down: bool,
}

struct Dev {
    path: String,
    name: String,
    file: File,
}

#[derive(Default)]
pub struct KeyboardGrab {
    devs: Vec<Dev>,
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
/// letter keys), on a USB or Bluetooth bus, and not an LG remote by name.
pub fn looks_like_keyboard(name: &str, bus: u16, key_bits: &[u8]) -> bool {
    let lname = name.to_ascii_lowercase();
    let remote = ["lge", "magic", "remote", "rcu", "mrcu"]
        .iter()
        .any(|w| lname.contains(w));
    let keys = [30usize, 44, 57, 28, 42] // KEY_A, KEY_Z, KEY_SPACE, KEY_ENTER, KEY_LEFTSHIFT
        .iter()
        .all(|&k| bit(key_bits, k));
    keys && !remote && matches!(bus, BUS_USB | BUS_BLUETOOTH)
}

/// Decode `input_event`s from raw bytes (`event_size` is the platform struct size): key
/// presses and releases only; auto-repeat (value 2) is dropped, the computer repeats itself.
pub fn decode_events(buf: &[u8], event_size: usize) -> Vec<EvKey> {
    let mut out = Vec::new();
    // struct input_event { struct timeval time; __u16 type; __u16 code; __s32 value; }
    let off = event_size - 8;
    for ev in buf.chunks_exact(event_size) {
        let ty = u16::from_ne_bytes([ev[off], ev[off + 1]]);
        let code = u16::from_ne_bytes([ev[off + 2], ev[off + 3]]);
        let value = i32::from_ne_bytes([ev[off + 4], ev[off + 5], ev[off + 6], ev[off + 7]]);
        if ty == EV_KEY && (value == 0 || value == 1) {
            out.push(EvKey {
                code,
                down: value == 1,
            });
        }
    }
    out
}

impl KeyboardGrab {
    /// Hold (`true`) or release (`false`) the grab; call once per frame.
    pub fn set_active(&mut self, on: bool) {
        if !on {
            if !self.devs.is_empty() {
                for d in self.devs.drain(..) {
                    ioctl(d.file.as_raw_fd(), EVIOCGRAB, std::ptr::null_mut());
                    log::info!("keyboard released: {} ({})", d.name, d.path);
                }
            }
            self.last_scan = None;
            self.skipped.clear();
            return;
        }
        if self.last_scan.is_some_and(|t| t.elapsed() < RESCAN) {
            return;
        }
        self.last_scan = Some(Instant::now());
        self.scan();
    }

    pub fn active(&self) -> bool {
        !self.devs.is_empty()
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
            match open_keyboard(&path) {
                Ok(Some(dev)) => {
                    log::info!("keyboard grabbed: {} ({})", dev.name, dev.path);
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
            let names: Vec<&str> = self.devs.iter().map(|d| d.name.as_str()).collect();
            format!("capturing {}", names.join(", "))
        } else if !errors.is_empty() {
            errors.join("; ")
        } else {
            "no keyboard found".to_owned()
        };
    }

    /// Keys read since the last call. A device that went away (keyboard switched off) is
    /// dropped and picked up again by a later rescan when it comes back.
    pub fn poll(&mut self) -> Vec<EvKey> {
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
                    log::info!("keyboard {} gone: {e}", d.name);
                    return false;
                }
            }
        });
        out
    }
}

impl Drop for KeyboardGrab {
    fn drop(&mut self) {
        self.set_active(false);
    }
}

/// Open `path`; `Ok(None)` when it is not a keyboard, `Err` when a keyboard could not be taken.
fn open_keyboard(path: &str) -> Result<Option<Dev>, String> {
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
    if !looks_like_keyboard(&name, id[0], &keys) {
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
        // A remote: arrows, OK, digits, but no letters.
        let remote = bits(&[103, 108, 105, 106, 28, 2, 3, 4]);
        assert!(!looks_like_keyboard("BT remote", BUS_BLUETOOTH, &remote));
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
            buf.extend(ev(size, EV_KEY, 1, 2)); // repeat: dropped
            buf.extend(ev(size, EV_KEY, 1, 0)); // Esc up
            assert_eq!(
                decode_events(&buf, size),
                vec![EvKey { code: 1, down: true }, EvKey { code: 1, down: false }]
            );
        }
    }
}
