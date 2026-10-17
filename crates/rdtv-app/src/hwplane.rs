//! Experimental placement of the hardware video picture.
//!
//! NDL DirectMedia API 2 always presents full-screen; the Starfish pipeline can present into
//! an exported window. The SDL-webOS backport can export a
//! "video" punch-through window (`SDL_webOSCreateExportedWindow`) whose source crop and
//! destination rectangle the compositor applies (`SDL_webOSSetExportedWindow`, see
//! webosbrew/SDL-webOS `src/video/wayland/SDL_waylandwebos_foreign.c`). The window id goes into
//! the Starfish decoder's Load payload (`rdtv_media::smp`). If any piece is missing or fails,
//! the app keeps the NDL full-screen behaviour.
//!
//! The SDL symbols exist only in the webOS build of SDL, so they are looked up at runtime.

use std::ffi::{c_char, c_int, c_void, CStr, CString};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SdlRect {
    pub x: c_int,
    pub y: c_int,
    pub w: c_int,
    pub h: c_int,
}

type CreateFn = unsafe extern "C" fn(c_int) -> *const c_char;
type SetFn = unsafe extern "C" fn(*const c_char, *mut SdlRect, *mut SdlRect) -> c_int;
type DestroyFn = unsafe extern "C" fn(*const c_char);
type CropFn = unsafe extern "C" fn(*const c_char, *mut SdlRect, *mut SdlRect, *mut SdlRect) -> c_int;

const EXPORTED_WINDOW_TYPE_VIDEO: c_int = 0;

pub struct HwPlane {
    id: CString,
    set: SetFn,
    crop: Option<CropFn>,
    destroy: DestroyFn,
    last: Option<(SdlRect, SdlRect, SdlRect)>,
    generation: u64,
    pub failures: u32,
}

fn sym(name: &CStr) -> Option<*mut c_void> {
    let p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    (!p.is_null()).then_some(p)
}

fn sdl_error() -> String {
    unsafe { CStr::from_ptr(sdl2::sys::SDL_GetError()) }
        .to_string_lossy()
        .into_owned()
}

impl HwPlane {
    pub fn create() -> Result<HwPlane, String> {
        let create = sym(c"SDL_webOSCreateExportedWindow").ok_or("SDL_webOSCreateExportedWindow missing")?;
        let set = sym(c"SDL_webOSSetExportedWindow").ok_or("SDL_webOSSetExportedWindow missing")?;
        let destroy =
            sym(c"SDL_webOSDestroyExportedWindow").ok_or("SDL_webOSDestroyExportedWindow missing")?;
        let create: CreateFn = unsafe { std::mem::transmute_copy(&create) };
        let id = unsafe { create(EXPORTED_WINDOW_TYPE_VIDEO) };
        if id.is_null() {
            return Err(format!("exported window not created: {}", sdl_error()));
        }
        let id = unsafe { CStr::from_ptr(id) }.to_owned();
        log::info!("exported video window {}", id.to_string_lossy());
        Ok(HwPlane {
            id,
            set: unsafe { std::mem::transmute_copy(&set) },
            crop: sym(c"SDL_webOSExportedSetCropRegion")
                .map(|p| unsafe { std::mem::transmute_copy::<*mut c_void, CropFn>(&p) }),
            destroy: unsafe { std::mem::transmute_copy(&destroy) },
            last: None,
            generation: 0,
            failures: 0,
        })
    }

    pub fn id(&self) -> &str {
        self.id.to_str().unwrap_or("")
    }

    /// Whether part of the picture can be shown (zoom, 1:1 larger than the screen).
    pub fn can_crop(&self) -> bool {
        self.crop.is_some()
    }

    /// Show source crop `src` (stream pixels, within `org` = the whole stream) at `dst`
    /// (window pixels). `SDL_webOSSetExportedWindow` only moves/scales the whole picture; a
    /// partial source needs `SDL_webOSExportedSetCropRegion`. Cheap when unchanged.
    pub fn place(&mut self, org: SdlRect, src: SdlRect, dst: SdlRect) -> bool {
        if self.last == Some((org, src, dst)) {
            return true;
        }
        let (mut o, mut s, mut d) = (org, src, dst);
        // Panning only changes the crop: re-sending the window first would briefly show the
        // whole picture squeezed into `dst` (the view seemed to resize and jump back).
        let crop_only = src != org
            && self.crop.is_some()
            && self
                .last
                .is_some_and(|(lo, ls, ld)| lo == org && ld == dst && ls != lo);
        let ok = crop_only || unsafe { (self.set)(self.id.as_ptr(), &mut o, &mut d) } != 0;
        if ok && src != org {
            if let Some(crop) = self.crop {
                let (mut o, mut d) = (org, dst);
                if unsafe { crop(self.id.as_ptr(), &mut o, &mut s, &mut d) } == 0 {
                    // Cropping refused: keep whole-picture placement, give up zoom/1:1.
                    log::warn!("SDL_webOSExportedSetCropRegion failed: {}", sdl_error());
                    self.crop = None;
                    self.last = None;
                    return true;
                }
            }
        }
        if ok {
            self.last = Some((org, src, dst));
        } else {
            self.failures += 1;
            if self.failures <= 3 {
                log::warn!("exported window placement failed: {}", sdl_error());
            }
        }
        ok
    }

    /// Forget the cached placement when the decoder was (re)loaded since the last call.
    pub fn note_generation(&mut self, generation: u64) {
        if self.generation != generation {
            self.generation = generation;
            self.last = None;
        }
    }
}

impl Drop for HwPlane {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.id.as_ptr()) };
    }
}

/// Source crop and destination for a picture of `stream` size shown at `v` (logical units),
/// clipped to the screen, converted to window pixels with `scale`.
pub fn crop_for(
    v: crate::view::RectF,
    stream: (f32, f32),
    screen: (f32, f32),
    scale: f32,
) -> Option<(SdlRect, SdlRect)> {
    let x0 = v.x.max(0.0);
    let y0 = v.y.max(0.0);
    let x1 = (v.x + v.w).min(screen.0);
    let y1 = (v.y + v.h).min(screen.1);
    if x1 <= x0 || y1 <= y0 || v.w <= 0.0 || v.h <= 0.0 {
        return None;
    }
    let sx = stream.0 / v.w;
    let sy = stream.1 / v.h;
    let src = SdlRect {
        x: ((x0 - v.x) * sx).round() as c_int,
        y: ((y0 - v.y) * sy).round() as c_int,
        w: ((x1 - x0) * sx).round().max(1.0) as c_int,
        h: ((y1 - y0) * sy).round().max(1.0) as c_int,
    };
    let dst = SdlRect {
        x: (x0 * scale).round() as c_int,
        y: (y0 * scale).round() as c_int,
        w: ((x1 - x0) * scale).round().max(1.0) as c_int,
        h: ((y1 - y0) * scale).round().max(1.0) as c_int,
    };
    Some((src, dst))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::RectF;

    #[test]
    fn shrink_uses_whole_source() {
        let (src, dst) = crop_for(
            RectF::new(240.0, 0.0, 1440.0, 810.0),
            (1920.0, 1080.0),
            (1920.0, 1080.0),
            1.0,
        )
        .unwrap();
        assert_eq!(
            src,
            SdlRect {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080
            }
        );
        assert_eq!(
            dst,
            SdlRect {
                x: 240,
                y: 0,
                w: 1440,
                h: 810
            }
        );
    }

    #[test]
    fn zoom_crops_source_to_visible_part() {
        // 2x zoom showing the right/bottom half.
        let (src, dst) = crop_for(
            RectF::new(-1920.0, -1080.0, 3840.0, 2160.0),
            (1920.0, 1080.0),
            (1920.0, 1080.0),
            1.0,
        )
        .unwrap();
        assert_eq!(
            src,
            SdlRect {
                x: 960,
                y: 540,
                w: 960,
                h: 540
            }
        );
        assert_eq!(
            dst,
            SdlRect {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080
            }
        );
    }
}
