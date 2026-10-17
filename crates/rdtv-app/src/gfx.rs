//! Rendering helpers over the SDL2 renderer: theme colours, rounded rectangles and a
//! glyph-cached text renderer (fontdue, DejaVu Sans). All coordinates are in the 1920x1080
//! logical UI space; SDL's logical size maps them to the real drawable.

use fontdue::{Font, FontSettings};
use sdl2::{
    pixels::{Color, PixelFormatEnum},
    rect::Rect,
    render::{BlendMode, Canvas, Texture, TextureCreator},
    video::{Window, WindowContext},
};
use std::collections::HashMap;

pub const W: i32 = 1920;
pub const H: i32 = 1080;

pub mod theme {
    use sdl2::pixels::Color;
    pub const BG: Color = Color::RGB(10, 18, 34);
    pub const PANEL: Color = Color::RGB(22, 33, 56);
    pub const PANEL_HI: Color = Color::RGB(32, 48, 80);
    pub const ROW: Color = Color::RGB(27, 40, 66);
    pub const TEXT: Color = Color::RGB(236, 241, 250);
    pub const TEXT_DIM: Color = Color::RGB(160, 175, 200);
    pub const ACCENT: Color = Color::RGB(0, 111, 255);
    pub const ACCENT2: Color = Color::RGB(92, 225, 250);
    pub const FOCUS: Color = Color::RGB(92, 225, 250);
    pub const DANGER: Color = Color::RGB(229, 72, 77);
    pub const WARN: Color = Color::RGB(245, 176, 65);
    pub const OK: Color = Color::RGB(70, 200, 120);
    pub const RED_KEY: Color = Color::RGB(220, 50, 50);
    pub const GREEN_KEY: Color = Color::RGB(40, 170, 70);
    pub const YELLOW_KEY: Color = Color::RGB(230, 200, 40);
    pub const BLUE_KEY: Color = Color::RGB(40, 110, 230);
    pub const SCRIM: Color = Color::RGBA(5, 10, 20, 200);
}

/// Text sizes (logical px), validated for couch distance as a starting point.
pub mod size {
    pub const SMALL: f32 = 24.0;
    pub const BODY: f32 = 28.0;
    pub const LARGE: f32 = 34.0;
    pub const TITLE: f32 = 44.0;
    pub const HUGE: f32 = 60.0;
}

struct Glyph {
    tex: Option<Texture<'static>>,
    w: u32,
    h: u32,
    xmin: i32,
    ymin: i32,
    advance: f32,
}

pub struct Gfx {
    pub canvas: Canvas<Window>,
    pub tc: &'static TextureCreator<WindowContext>,
    regular: Font,
    bold: Font,
    glyphs: HashMap<(char, u32, bool), Glyph>,
    circle: Texture<'static>,
    rings: HashMap<(i32, i32), Texture<'static>>,
    /// Multiplier from logical px to drawable px, for crisp glyph rasterization.
    pub raster_scale: f32,
    pub ui_scale: f32,
}

static REGULAR_TTF: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");
static BOLD_TTF: &[u8] = include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf");

impl Gfx {
    pub fn new(mut canvas: Canvas<Window>) -> anyhow::Result<Gfx> {
        canvas
            .set_logical_size(W as u32, H as u32)
            .map_err(anyhow::Error::msg)?;
        canvas.set_blend_mode(BlendMode::Blend);
        let (dw, _dh) = canvas.output_size().map_err(anyhow::Error::msg)?;
        let raster_scale = (dw as f32 / W as f32).clamp(0.5, 2.0);
        // The texture creator must outlive every texture; it lives for the whole process.
        let tc: &'static TextureCreator<WindowContext> = Box::leak(Box::new(canvas.texture_creator()));
        let regular = Font::from_bytes(REGULAR_TTF, FontSettings::default()).map_err(anyhow::Error::msg)?;
        let bold = Font::from_bytes(BOLD_TTF, FontSettings::default()).map_err(anyhow::Error::msg)?;
        let circle = make_circle(tc, 64)?;
        Ok(Gfx {
            canvas,
            tc,
            regular,
            bold,
            glyphs: HashMap::new(),
            circle,
            rings: HashMap::new(),
            raster_scale,
            ui_scale: 1.0,
        })
    }

    pub fn clear(&mut self, c: Color) {
        self.canvas.set_draw_color(c);
        self.canvas.clear();
    }

    pub fn fill(&mut self, r: Rect, c: Color) {
        self.canvas.set_draw_color(c);
        let _ = self.canvas.fill_rect(r);
    }

    pub fn outline(&mut self, r: Rect, c: Color, thickness: i32) {
        self.canvas.set_draw_color(c);
        for i in 0..thickness {
            let rr = Rect::new(r.x() + i, r.y() + i, (r.width() as i32 - 2 * i).max(1) as u32, (r.height() as i32 - 2 * i).max(1) as u32);
            let _ = self.canvas.draw_rect(rr);
        }
    }

    /// Filled rounded rectangle (corner quarter-circles from an anti-aliased texture).
    pub fn round(&mut self, r: Rect, radius: i32, c: Color) {
        let rad = radius.min(r.width() as i32 / 2).min(r.height() as i32 / 2).max(0);
        if rad == 0 {
            return self.fill(r, c);
        }
        let (x, y, w, h) = (r.x(), r.y(), r.width() as i32, r.height() as i32);
        self.canvas.set_draw_color(c);
        let _ = self.canvas.fill_rect(Rect::new(x + rad, y, (w - 2 * rad) as u32, h as u32));
        let _ = self.canvas.fill_rect(Rect::new(x, y + rad, rad as u32, (h - 2 * rad).max(0) as u32));
        let _ = self.canvas.fill_rect(Rect::new(x + w - rad, y + rad, rad as u32, (h - 2 * rad).max(0) as u32));
        self.circle.set_color_mod(c.r, c.g, c.b);
        self.circle.set_alpha_mod(c.a);
        let half = 32;
        let corners = [
            (Rect::new(0, 0, half, half), Rect::new(x, y, rad as u32, rad as u32)),
            (Rect::new(half as i32, 0, half, half), Rect::new(x + w - rad, y, rad as u32, rad as u32)),
            (Rect::new(0, half as i32, half, half), Rect::new(x, y + h - rad, rad as u32, rad as u32)),
            (Rect::new(half as i32, half as i32, half, half), Rect::new(x + w - rad, y + h - rad, rad as u32, rad as u32)),
        ];
        for (src, dst) in corners {
            let _ = self.canvas.copy(&self.circle, src, dst);
        }
    }

    /// Rounded outline (focus ring) drawn just outside `r`.
    pub fn round_outline(&mut self, r: Rect, radius: i32, c: Color, thickness: i32) {
        let t = thickness.max(1);
        let (x, y, w, h) = (r.x() - t, r.y() - t, r.width() as i32 + 2 * t, r.height() as i32 + 2 * t);
        let rad = (radius + t).min(w / 2).min(h / 2).max(t);
        self.fill(Rect::new(x + rad, y, (w - 2 * rad).max(0) as u32, t as u32), c);
        self.fill(Rect::new(x + rad, y + h - t, (w - 2 * rad).max(0) as u32, t as u32), c);
        self.fill(Rect::new(x, y + rad, t as u32, (h - 2 * rad).max(0) as u32), c);
        self.fill(Rect::new(x + w - t, y + rad, t as u32, (h - 2 * rad).max(0) as u32), c);
        let key = (rad, t);
        if !self.rings.contains_key(&key) {
            if let Ok(tex) = make_ring(self.tc, rad as u32, t as f32) {
                self.rings.insert(key, tex);
            }
        }
        if let Some(tex) = self.rings.get_mut(&key) {
            tex.set_color_mod(c.r, c.g, c.b);
            tex.set_alpha_mod(c.a);
            let rr = rad as u32;
            let quads = [
                (Rect::new(0, 0, rr, rr), Rect::new(x, y, rr, rr)),
                (Rect::new(rad, 0, rr, rr), Rect::new(x + w - rad, y, rr, rr)),
                (Rect::new(0, rad, rr, rr), Rect::new(x, y + h - rad, rr, rr)),
                (Rect::new(rad, rad, rr, rr), Rect::new(x + w - rad, y + h - rad, rr, rr)),
            ];
            for (src, dst) in quads {
                let _ = self.canvas.copy(tex, src, dst);
            }
        }
    }

    fn glyph(&mut self, ch: char, px: f32, bold: bool) -> &Glyph {
        let key = (ch, (px * 4.0) as u32, bold);
        if !self.glyphs.contains_key(&key) {
            let font = if bold { &self.bold } else { &self.regular };
            let rpx = px * self.raster_scale;
            let (m, bitmap) = font.rasterize(ch, rpx);
            let tex = if m.width > 0 && m.height > 0 {
                self.tc
                    .create_texture_static(PixelFormatEnum::ABGR8888, m.width as u32, m.height as u32)
                    .ok()
                    .map(|mut t| {
                        let mut rgba = Vec::with_capacity(bitmap.len() * 4);
                        for a in bitmap {
                            rgba.extend_from_slice(&[255, 255, 255, a]);
                        }
                        let _ = t.update(None, &rgba, m.width * 4);
                        t.set_blend_mode(BlendMode::Blend);
                        t
                    })
            } else {
                None
            };
            let s = self.raster_scale;
            self.glyphs.insert(
                key,
                Glyph {
                    tex,
                    w: (m.width as f32 / s).round() as u32,
                    h: (m.height as f32 / s).round() as u32,
                    xmin: (m.xmin as f32 / s).round() as i32,
                    ymin: (m.ymin as f32 / s).round() as i32,
                    advance: m.advance_width / s,
                },
            );
            if self.glyphs.len() > 4000 {
                // Unbounded caches are a memory leak on a TV; rebuild lazily.
                let keep = key;
                self.glyphs.retain(|k, _| *k == keep);
            }
        }
        &self.glyphs[&key]
    }

    pub fn text_width(&mut self, s: &str, px: f32, bold: bool) -> i32 {
        let px = px * self.ui_scale;
        let mut w = 0.0;
        for ch in s.chars() {
            w += self.glyph(ch, px, bold).advance;
        }
        w.round() as i32
    }

    /// Draw text with its top-left at (x, y). Returns the advance width.
    pub fn text(&mut self, s: &str, x: i32, y: i32, px: f32, bold: bool, c: Color) -> i32 {
        let px = px * self.ui_scale;
        let ascent = px * 0.93;
        let mut pen = x as f32;
        for ch in s.chars() {
            let (w, h, xmin, ymin, adv) = {
                let g = self.glyph(ch, px, bold);
                (g.w, g.h, g.xmin, g.ymin, g.advance)
            };
            let key = (ch, (px * 4.0) as u32, bold);
            if let Some(tex) = self.glyphs.get_mut(&key).and_then(|g| g.tex.as_mut()) {
                tex.set_color_mod(c.r, c.g, c.b);
                tex.set_alpha_mod(c.a);
                let gy = y as f32 + ascent - h as f32 - ymin as f32;
                let dst = Rect::new((pen + xmin as f32).round() as i32, gy.round() as i32, w.max(1), h.max(1));
                let _ = self.canvas.copy(tex, None, dst);
            }
            pen += adv;
        }
        (pen - x as f32).round() as i32
    }

    /// Text clipped with an ellipsis to `max_w`.
    pub fn text_fit(&mut self, s: &str, x: i32, y: i32, px: f32, bold: bool, c: Color, max_w: i32) {
        if self.text_width(s, px, bold) <= max_w {
            self.text(s, x, y, px, bold, c);
            return;
        }
        let mut out = String::new();
        for ch in s.chars() {
            out.push(ch);
            if self.text_width(&format!("{out}…"), px, bold) > max_w {
                out.pop();
                break;
            }
        }
        out.push('…');
        self.text(&out, x, y, px, bold, c);
    }

    pub fn text_centered(&mut self, s: &str, r: Rect, px: f32, bold: bool, c: Color) {
        let w = self.text_width(s, px, bold);
        let h = (px * self.ui_scale * 1.15) as i32;
        self.text(s, r.x() + (r.width() as i32 - w) / 2, r.y() + (r.height() as i32 - h) / 2, px, bold, c);
    }

    pub fn line_height(&self, px: f32) -> i32 {
        (px * self.ui_scale * 1.25) as i32
    }

    /// Word-wrapped paragraph; returns the height used.
    pub fn paragraph(&mut self, s: &str, x: i32, y: i32, max_w: i32, px: f32, c: Color) -> i32 {
        let lh = self.line_height(px);
        let mut cy = y;
        for para in s.split('\n') {
            let mut line = String::new();
            for word in para.split(' ') {
                let cand = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
                if self.text_width(&cand, px, false) > max_w && !line.is_empty() {
                    self.text(&line, x, cy, px, false, c);
                    cy += lh;
                    line = word.to_owned();
                } else {
                    line = cand;
                }
            }
            self.text(&line, x, cy, px, false, c);
            cy += lh;
        }
        cy - y
    }

    pub fn texture_creator(&self) -> &'static TextureCreator<WindowContext> {
        self.tc
    }
}

fn make_circle(tc: &'static TextureCreator<WindowContext>, d: u32) -> anyhow::Result<Texture<'static>> {
    let mut t = tc
        .create_texture_static(PixelFormatEnum::ABGR8888, d, d)
        .map_err(anyhow::Error::msg)?;
    let r = d as f32 / 2.0;
    let mut px = Vec::with_capacity((d * d * 4) as usize);
    for y in 0..d {
        for x in 0..d {
            let dx = x as f32 + 0.5 - r;
            let dy = y as f32 + 0.5 - r;
            let dist = (dx * dx + dy * dy).sqrt();
            let a = ((r - dist + 0.5).clamp(0.0, 1.0) * 255.0) as u8;
            px.extend_from_slice(&[255, 255, 255, a]);
        }
    }
    t.update(None, &px, (d * 4) as usize).map_err(|e| anyhow::anyhow!("{e}"))?;
    t.set_blend_mode(BlendMode::Blend);
    Ok(t)
}

fn make_ring(tc: &'static TextureCreator<WindowContext>, rad: u32, t: f32) -> anyhow::Result<Texture<'static>> {
    let d = rad * 2;
    let mut tex = tc
        .create_texture_static(PixelFormatEnum::ABGR8888, d, d)
        .map_err(anyhow::Error::msg)?;
    let r = rad as f32;
    let mut px = Vec::with_capacity((d * d * 4) as usize);
    for y in 0..d {
        for x in 0..d {
            let dx = x as f32 + 0.5 - r;
            let dy = y as f32 + 0.5 - r;
            let dist = (dx * dx + dy * dy).sqrt();
            let outer = (r - dist + 0.5).clamp(0.0, 1.0);
            let inner = (dist - (r - t) + 0.5).clamp(0.0, 1.0);
            px.extend_from_slice(&[255, 255, 255, (outer * inner * 255.0) as u8]);
        }
    }
    tex.update(None, &px, (d * 4) as usize).map_err(|e| anyhow::anyhow!("{e}"))?;
    tex.set_blend_mode(BlendMode::Blend);
    Ok(tex)
}
