//! libvpx VP8/VP9 software decoder (fallback when the hardware decoder is unavailable).

use crate::frame::YuvFrame;
use anyhow::{bail, Result};
use std::ffi::{c_char, c_int, c_uint, c_void, CStr};

#[repr(C)]
struct RdImage {
    planes: [*const u8; 3],
    strides: [c_int; 3],
    width: c_int,
    height: c_int,
    fmt: c_int,
}

extern "C" {
    fn rd_vpx_open(vp9: c_int, threads: c_int) -> *mut c_void;
    fn rd_vpx_decode(h: *mut c_void, data: *const u8, len: c_uint) -> c_int;
    fn rd_vpx_error(h: *mut c_void) -> *const c_char;
    fn rd_vpx_next(h: *mut c_void, out: *mut RdImage) -> c_int;
    fn rd_vpx_close(h: *mut c_void);
}

pub struct VpxDecoder {
    h: *mut c_void,
    pub vp9: bool,
}

// The libvpx context is only used from the owning decode thread.
unsafe impl Send for VpxDecoder {}

impl VpxDecoder {
    pub fn new(vp9: bool) -> Result<Self> {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(4))
            .unwrap_or(2) as c_int;
        let h = unsafe { rd_vpx_open(vp9 as c_int, threads) };
        if h.is_null() {
            bail!("libvpx decoder init failed");
        }
        Ok(VpxDecoder { h, vp9 })
    }

    /// Decode one compressed frame; returns the last picture it produced.
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<YuvFrame>> {
        if data.len() > 64 * 1024 * 1024 {
            bail!("oversized frame");
        }
        let rc = unsafe { rd_vpx_decode(self.h, data.as_ptr(), data.len() as c_uint) };
        if rc != 0 {
            let msg = unsafe { CStr::from_ptr(rd_vpx_error(self.h)) }
                .to_string_lossy()
                .into_owned();
            bail!("vpx decode error {rc}: {msg}");
        }
        let mut last = None;
        loop {
            let mut img = RdImage {
                planes: [std::ptr::null(); 3],
                strides: [0; 3],
                width: 0,
                height: 0,
                fmt: -1,
            };
            if unsafe { rd_vpx_next(self.h, &mut img) } == 0 {
                break;
            }
            if img.fmt != 0 {
                bail!("unsupported vpx output format {}", img.fmt);
            }
            last =
                unsafe { YuvFrame::copy_i420(img.planes, img.strides, img.width as u32, img.height as u32) };
        }
        Ok(last)
    }
}

impl Drop for VpxDecoder {
    fn drop(&mut self) {
        unsafe { rd_vpx_close(self.h) };
    }
}
