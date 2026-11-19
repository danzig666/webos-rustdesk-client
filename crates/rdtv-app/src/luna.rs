#![allow(dead_code)]
//! Minimal luna-service2 client over dlopen: register on the bus, run a GLib main loop on a
//! background thread, make one-reply calls (blocking) and subscriptions (callback). Ported from
//! the author's ytnative (`crates/app/src/ls2.rs`), where it runs on webOS TVs; used here to
//! hand updates to Homebrew Channel's install service.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::mpsc;
use std::time::Duration;

#[repr(C)]
struct LSError {
    error_code: c_int,
    message: *mut c_char,
    file: *const c_char,
    line: c_int,
    func: *const c_char,
    padding: *mut c_void,
    magic: usize,
}

type Filter = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> bool;

struct Api {
    error_init: unsafe extern "C" fn(*mut LSError) -> bool,
    error_free: unsafe extern "C" fn(*mut LSError),
    register: unsafe extern "C" fn(*const c_char, *mut *mut c_void, *mut LSError) -> bool,
    gmain_attach: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut LSError) -> bool,
    call: unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        *const c_char,
        Filter,
        *mut c_void,
        *mut libc::c_ulong,
        *mut LSError,
    ) -> bool,
    call_one: unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        *const c_char,
        Filter,
        *mut c_void,
        *mut libc::c_ulong,
        *mut LSError,
    ) -> bool,
    payload: unsafe extern "C" fn(*mut c_void) -> *const c_char,
}

pub struct Luna {
    api: Api,
    handle: *mut c_void,
}
unsafe impl Send for Luna {}
unsafe impl Sync for Luna {}

unsafe fn open(lib: &str) -> Result<*mut c_void, String> {
    let c = CString::new(lib).unwrap();
    let h = libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
    if h.is_null() {
        Err(format!("dlopen {lib}: {:?}", CStr::from_ptr(libc::dlerror())))
    } else {
        Ok(h)
    }
}

unsafe fn sym<T>(h: *mut c_void, name: &str) -> Result<T, String> {
    let c = CString::new(name).unwrap();
    let p = libc::dlsym(h, c.as_ptr());
    if p.is_null() {
        Err(format!("missing symbol {name}"))
    } else {
        Ok(std::mem::transmute_copy(&p))
    }
}

/// Boxed callback context: for one-reply calls a Sender, for subscriptions a FnMut.
enum Ctx {
    Once(mpsc::Sender<String>),
    Sub(Box<dyn FnMut(&str) + Send>),
}

unsafe extern "C" fn on_reply(_sh: *mut c_void, msg: *mut c_void, ctx: *mut c_void) -> bool {
    // Every connection loads the same library, so any of them reads the payload.
    let payload: unsafe extern "C" fn(*mut c_void) -> *const c_char = std::mem::transmute(PAYLOAD_FN);
    let p = payload(msg);
    let s = if p.is_null() {
        String::new()
    } else {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    };
    match &mut *(ctx as *mut Ctx) {
        Ctx::Once(tx) => {
            let _ = tx.send(s);
            // A one-reply call gets exactly this one callback: its context is done.
            drop(Box::from_raw(ctx as *mut Ctx));
        }
        Ctx::Sub(f) => f(&s),
    }
    true
}

static mut PAYLOAD_FN: usize = 0;
/// The GLib main loop every connection is attached to (one thread runs it).
static mut MAIN_LOOP: usize = 0;

impl Luna {
    /// A connection that may call any service (here: Homebrew Channel). The role LG generates
    /// for a native app lets "com.webos.chromium.media.*" send anywhere, and the TV gives that
    /// name the "all" permission group (found and verified on a TV by ytnative); the app id and
    /// the media-client name are fallbacks.
    pub fn wide(app_id: &str) -> Result<&'static Luna, String> {
        static LOCK: std::sync::Mutex<Option<&'static Luna>> = std::sync::Mutex::new(None);
        let mut g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(l) = *g {
            return Ok(l);
        }
        let pid = std::process::id();
        let l = Self::connect(&[
            format!("com.webos.chromium.media.rustcouch{pid}"),
            format!("com.webos.media.client.rustcouch{pid}"),
            app_id.to_string(),
        ])?;
        *g = Some(l);
        Ok(l)
    }

    /// Registers on the bus under the first name the hub accepts.
    fn connect(names: &[String]) -> Result<&'static Luna, String> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            let ls = open("libluna-service2.so.3").or_else(|_| open("libluna-service2.so"))?;
            let glib = open("libglib-2.0.so.0")?;
            let api = Api {
                error_init: sym(ls, "LSErrorInit")?,
                error_free: sym(ls, "LSErrorFree")?,
                register: sym(ls, "LSRegister")?,
                gmain_attach: sym(ls, "LSGmainAttach")?,
                call: sym(ls, "LSCall")?,
                call_one: sym(ls, "LSCallOneReply")?,
                payload: sym(ls, "LSMessageGetPayload")?,
            };
            let loop_new: unsafe extern "C" fn(*mut c_void, c_int) -> *mut c_void =
                sym(glib, "g_main_loop_new")?;
            let loop_run: unsafe extern "C" fn(*mut c_void) = sym(glib, "g_main_loop_run")?;

            let mut handle = std::ptr::null_mut();
            let mut last_err = String::new();
            for name in names {
                let mut err: LSError = std::mem::zeroed();
                (api.error_init)(&mut err);
                let cname = CString::new(name.clone()).unwrap();
                if (api.register)(cname.as_ptr(), &mut handle, &mut err) {
                    log::info!("LSRegister ok as {name}");
                    break;
                }
                last_err = if err.message.is_null() {
                    "?".into()
                } else {
                    CStr::from_ptr(err.message).to_string_lossy().into_owned()
                };
                log::info!("LSRegister({name}) failed: {last_err}");
                (api.error_free)(&mut err);
                handle = std::ptr::null_mut();
            }
            if handle.is_null() {
                return Err(format!("LSRegister failed: {last_err}"));
            }

            // Default main context, run on its own thread; luna callbacks fire there.
            let first = MAIN_LOOP == 0;
            if first {
                MAIN_LOOP = loop_new(std::ptr::null_mut(), 0) as usize;
            }
            let main_loop = MAIN_LOOP;
            let mut err: LSError = std::mem::zeroed();
            (api.error_init)(&mut err);
            if !(api.gmain_attach)(handle, main_loop as *mut c_void, &mut err) {
                return Err("LSGmainAttach failed".into());
            }
            PAYLOAD_FN = api.payload as usize;
            if first {
                std::thread::spawn(move || loop_run(main_loop as *mut c_void));
            }
            Ok(Box::leak(Box::new(Luna { api, handle })))
        }
    }

    /// One-reply call, blocking up to 8 s.
    pub fn call(&self, uri: &str, payload: &str) -> String {
        let (tx, rx) = mpsc::channel();
        // Freed by `on_reply` (or below when the call is refused); leaked only if no reply ever comes.
        let ctx = Box::into_raw(Box::new(Ctx::Once(tx))) as *mut c_void;
        let (u, p) = (CString::new(uri).unwrap(), CString::new(payload).unwrap());
        unsafe {
            let mut err: LSError = std::mem::zeroed();
            (self.api.error_init)(&mut err);
            let mut tok: libc::c_ulong = 0;
            if !(self.api.call_one)(
                self.handle,
                u.as_ptr(),
                p.as_ptr(),
                on_reply,
                ctx,
                &mut tok,
                &mut err,
            ) {
                let m = if err.message.is_null() {
                    "?".into()
                } else {
                    CStr::from_ptr(err.message).to_string_lossy().into_owned()
                };
                (self.api.error_free)(&mut err);
                drop(Box::from_raw(ctx as *mut Ctx));
                return format!("<LSCallOneReply failed: {m}>");
            }
        }
        rx.recv_timeout(Duration::from_secs(8))
            .unwrap_or_else(|_| "<timeout>".into())
    }

    /// Subscription: `f` is called (on the luna thread) for every message.
    pub fn subscribe(&self, uri: &str, payload: &str, f: impl FnMut(&str) + Send + 'static) -> bool {
        let ctx = Box::into_raw(Box::new(Ctx::Sub(Box::new(f)))) as *mut c_void;
        let (u, p) = (CString::new(uri).unwrap(), CString::new(payload).unwrap());
        unsafe {
            let mut err: LSError = std::mem::zeroed();
            (self.api.error_init)(&mut err);
            let mut tok: libc::c_ulong = 0;
            (self.api.call)(
                self.handle,
                u.as_ptr(),
                p.as_ptr(),
                on_reply,
                ctx,
                &mut tok,
                &mut err,
            )
        }
    }
}
