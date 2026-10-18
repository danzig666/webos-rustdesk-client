//! The single transform shared by video presentation and pointer input.
//!
//! Spaces:
//! * UI logical: the 1920x1080 space every SDL event and draw call uses (SDL's logical
//!   size already maps window/drawable units, i.e. `W`, onto it).
//! * Monitor: the selected remote display in host coordinates, origin `(mon.x, mon.y)`
//!   (may be negative), size `mon.w x mon.h`.
//! * `V`: the rectangle in UI logical space where the selected monitor is shown.
//!
//! A UI point `p` maps to host coordinates as `mon.origin + (p - V.origin) * mon.size / V.size`
//! (`V^-1`), and the monitor origin is added exactly once because RustDesk mouse events use
//! global desktop coordinates. Points outside `V` (letterbox bars) map to nothing.

use rdtv_core::store::ViewMode;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        RectF { x, y, w, h }
    }
    /// `self` moved `t` (0..1) of the way to `to`.
    pub fn lerp(self, to: RectF, t: f32) -> RectF {
        let l = |a: f32, b: f32| a + (b - a) * t;
        RectF::new(l(self.x, to.x), l(self.y, to.y), l(self.w, to.w), l(self.h, to.h))
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
    pub fn to_sdl(self) -> sdl2::rect::Rect {
        sdl2::rect::Rect::new(
            self.x.round() as i32,
            self.y.round() as i32,
            self.w.round().max(1.0) as u32,
            self.h.round().max(1.0) as u32,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewState {
    pub mode: ViewMode,
    /// Extra magnification over "fit" in Zoom mode (>= 1).
    pub zoom: f32,
    /// Point of the monitor kept at the centre of the area, as fractions (0..1).
    pub pan_x: f32,
    pub pan_y: f32,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            mode: ViewMode::Fit,
            zoom: 1.0,
            pan_x: 0.5,
            pan_y: 0.5,
        }
    }
}

pub const MAX_ZOOM: f32 = 6.0;

/// Compute `V` for a monitor shown inside `area` (UI logical).
///
/// `px_per_logical` is drawable pixels per logical unit (1.0 on a 1920x1080 UI plane): it
/// makes "1:1" mean one stream pixel per drawable pixel.
pub fn view_rect(area: RectF, stream_w: f32, stream_h: f32, st: &ViewState, px_per_logical: f32) -> RectF {
    if stream_w <= 0.0 || stream_h <= 0.0 || area.w <= 0.0 || area.h <= 0.0 {
        return area;
    }
    let fit = (area.w / stream_w).min(area.h / stream_h);
    let scale = match st.mode {
        ViewMode::Fit => fit,
        ViewMode::Original => 1.0 / px_per_logical.max(0.01),
        ViewMode::Zoom => fit * st.zoom.clamp(1.0, MAX_ZOOM),
    };
    let w = stream_w * scale;
    let h = stream_h * scale;
    let place = |area_pos: f32, area_len: f32, len: f32, pan: f32| -> f32 {
        if len <= area_len {
            area_pos + (area_len - len) / 2.0
        } else {
            // Keep the pan point centred, clamped so both edges stay reachable.
            let want = area_pos + area_len / 2.0 - pan.clamp(0.0, 1.0) * len;
            want.clamp(area_pos + area_len - len, area_pos)
        }
    };
    RectF::new(
        place(area.x, area.w, w, st.pan_x),
        place(area.y, area.h, h, st.pan_y),
        w,
        h,
    )
}

/// UI logical point -> global host coordinates, or `None` outside the picture.
pub fn to_host(v: RectF, mon: Monitor, px: f32, py: f32) -> Option<(i32, i32)> {
    if !v.contains(px, py) || mon.w <= 0 || mon.h <= 0 {
        return None;
    }
    let lx = ((px - v.x) / v.w * mon.w as f32).floor() as i32;
    let ly = ((py - v.y) / v.h * mon.h as f32).floor() as i32;
    Some((mon.x + lx.clamp(0, mon.w - 1), mon.y + ly.clamp(0, mon.h - 1)))
}

/// Global host coordinates -> UI logical point (for drawing the remote cursor).
pub fn from_host(v: RectF, mon: Monitor, hx: i32, hy: i32) -> (f32, f32) {
    let lx = (hx - mon.x) as f32;
    let ly = (hy - mon.y) as f32;
    (
        v.x + lx * v.w / mon.w.max(1) as f32,
        v.y + ly * v.h / mon.h.max(1) as f32,
    )
}

/// Clamp host coordinates into the monitor (D-pad virtual mouse).
pub fn clamp_host(mon: Monitor, x: i32, y: i32) -> (i32, i32) {
    (
        x.clamp(mon.x, mon.x + mon.w - 1),
        y.clamp(mon.y, mon.y + mon.h - 1),
    )
}

/// Pan so that host point (hx, hy) stays visible (used when the virtual mouse nears an
/// edge in Zoom/1:1). Returns new pan fractions.
pub fn pan_to_include(st: &ViewState, v: RectF, area: RectF, mon: Monitor, hx: i32, hy: i32) -> (f32, f32) {
    let (ux, uy) = from_host(v, mon, hx, hy);
    let margin = 80.0;
    let mut pan_x = st.pan_x;
    let mut pan_y = st.pan_y;
    if v.w > area.w && (ux < area.x + margin || ux > area.x + area.w - margin) {
        pan_x = (hx - mon.x) as f32 / mon.w.max(1) as f32;
    }
    if v.h > area.h && (uy < area.y + margin || uy > area.y + area.h - margin) {
        pan_y = (hy - mon.y) as f32 / mon.h.max(1) as f32;
    }
    (pan_x, pan_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: RectF = RectF {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
    };

    #[test]
    fn fit_letterboxes_and_maps_corners() {
        // 4:3 monitor on a 16:9 screen: pillarboxed.
        let st = ViewState::default();
        let v = view_rect(FULL, 1600.0, 1200.0, &st, 1.0);
        assert_eq!(v.h, 1080.0);
        assert!((v.w - 1440.0).abs() < 0.01);
        assert!((v.x - 240.0).abs() < 0.01);
        let mon = Monitor {
            x: 0,
            y: 0,
            w: 1600,
            h: 1200,
        };
        assert_eq!(to_host(v, mon, 240.0, 0.0), Some((0, 0)));
        assert_eq!(to_host(v, mon, 240.0 + 1439.99, 1079.99), Some((1599, 1199)));
        // Letterbox bar: no click.
        assert_eq!(to_host(v, mon, 100.0, 500.0), None);
    }

    #[test]
    fn monitor_origin_applied_once_including_negative() {
        let st = ViewState::default();
        let v = view_rect(FULL, 1920.0, 1080.0, &st, 1.0);
        let mon = Monitor {
            x: -1920,
            y: -200,
            w: 1920,
            h: 1080,
        };
        assert_eq!(to_host(v, mon, 0.0, 0.0), Some((-1920, -200)));
        assert_eq!(to_host(v, mon, 960.0, 540.0), Some((-960, 340)));
        let (ux, uy) = from_host(v, mon, -960, 340);
        assert!((ux - 960.0).abs() < 0.01 && (uy - 540.0).abs() < 0.01);
    }

    #[test]
    fn different_stream_and_monitor_sizes() {
        // Retina-style: stream 3840x2160, host coordinates 1920x1080.
        let st = ViewState::default();
        let v = view_rect(FULL, 3840.0, 2160.0, &st, 1.0);
        let mon = Monitor {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        assert_eq!(to_host(v, mon, 1919.0, 1079.0), Some((1919, 1079)));
    }

    #[test]
    fn zoom_pan_keeps_edges_reachable() {
        let mut st = ViewState {
            mode: ViewMode::Zoom,
            zoom: 2.0,
            pan_x: 0.0,
            pan_y: 0.0,
        };
        let v = view_rect(FULL, 1920.0, 1080.0, &st, 1.0);
        assert_eq!((v.x, v.y), (0.0, 0.0), "top-left pan shows the top-left edge");
        st.pan_x = 1.0;
        st.pan_y = 1.0;
        let v = view_rect(FULL, 1920.0, 1080.0, &st, 1.0);
        assert!((v.x + v.w - 1920.0).abs() < 0.01 && (v.y + v.h - 1080.0).abs() < 0.01);
        let mon = Monitor {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        assert_eq!(to_host(v, mon, 1919.99, 1079.99), Some((1919, 1079)));
    }

    #[test]
    fn original_size_is_one_stream_pixel_per_drawable_pixel() {
        let st = ViewState {
            mode: ViewMode::Original,
            ..Default::default()
        };
        let v = view_rect(FULL, 2560.0, 1440.0, &st, 1.0);
        assert_eq!((v.w, v.h), (2560.0, 1440.0));
        let v = view_rect(FULL, 2560.0, 1440.0, &st, 2.0); // 4K UI plane
        assert_eq!((v.w, v.h), (1280.0, 720.0));
    }

    #[test]
    fn keyboard_shrink_area_keeps_whole_monitor() {
        let area = RectF::new(0.0, 0.0, 1920.0, 1080.0 - 420.0);
        let v = view_rect(area, 1920.0, 1080.0, &ViewState::default(), 1.0);
        assert!(v.y >= 0.0 && v.y + v.h <= area.h + 0.01);
        assert!((v.w / v.h - 16.0 / 9.0).abs() < 0.001, "aspect ratio preserved");
        let mon = Monitor {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        assert_eq!(to_host(v, mon, v.x, v.y), Some((0, 0)));
        assert_eq!(
            to_host(v, mon, 100.0, 900.0),
            None,
            "keyboard area never clicks the desktop"
        );
    }

    #[test]
    fn portrait_monitor() {
        let v = view_rect(FULL, 1080.0, 1920.0, &ViewState::default(), 1.0);
        assert_eq!(v.h, 1080.0);
        let mon = Monitor {
            x: 1920,
            y: 0,
            w: 1080,
            h: 1920,
        };
        assert_eq!(to_host(v, mon, v.x, v.y), Some((1920, 0)));
    }
}
