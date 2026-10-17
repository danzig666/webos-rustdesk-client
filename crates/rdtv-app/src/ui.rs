//! A small immediate-mode UI toolkit for TV navigation.
//!
//! Every frame, widgets register `(id, rect)`. Input collected since the previous frame is
//! resolved against the previous frame's layout: arrows move focus geometrically, OK
//! activates the focused widget, the pointer focuses what it hovers and clicks activate.
//! Only one layer is live at a time: when a modal is open, the layers below are drawn
//! "inert" so neither focus nor clicks can reach them.

use crate::gfx::{size, theme, Gfx};
use sdl2::{pixels::Color, rect::Rect};
use std::collections::HashMap;

pub type Id = u64;

pub fn id(s: &str) -> Id {
    // FNV-1a: stable, cheap.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn idx(s: &str, i: usize) -> Id {
    id(s) ^ ((i as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiIn {
    Nav(Nav),
    Activate,
    PointerMove(i32, i32),
    Click(i32, i32),
    Wheel(i32),
}

#[derive(Default)]
pub struct UiState {
    pub focused: Option<Id>,
    prev: Vec<(Id, Rect)>,
    pending: Vec<UiIn>,
    pub pointer: Option<(i32, i32)>,
    pub scroll: HashMap<Id, i32>,
    /// Requested focus for the next frame (e.g. when a dialog opens).
    pub want_focus: Option<Id>,
    /// Drop the current focus at the next frame (screen change).
    pub reset: bool,
}

impl UiState {
    pub fn push(&mut self, i: UiIn) {
        self.pending.push(i);
    }
}

pub struct Frame<'a> {
    pub g: &'a mut Gfx,
    pub st: &'a mut UiState,
    items: Vec<(Id, Rect)>,
    activated: Option<Id>,
    pub wheel: i32,
    inert: bool,
    pub now_ms: u64,
}

fn center(r: &Rect) -> (f32, f32) {
    (
        r.x() as f32 + r.width() as f32 / 2.0,
        r.y() as f32 + r.height() as f32 / 2.0,
    )
}

fn navigate(prev: &[(Id, Rect)], from: Id, dir: Nav) -> Option<Id> {
    let fr = prev.iter().find(|(i, _)| *i == from)?.1;
    let (fx, fy) = center(&fr);
    let mut best: Option<(f32, Id)> = None;
    for (i, r) in prev {
        if *i == from {
            continue;
        }
        let (cx, cy) = center(r);
        let (dx, dy) = (cx - fx, cy - fy);
        let (primary, secondary) = match dir {
            Nav::Up => (-dy, dx.abs()),
            Nav::Down => (dy, dx.abs()),
            Nav::Left => (-dx, dy.abs()),
            Nav::Right => (dx, dy.abs()),
        };
        if primary <= 1.0 {
            continue;
        }
        // Prefer items overlapping on the cross axis.
        let overlap = match dir {
            Nav::Up | Nav::Down => r.x() < fr.x() + fr.width() as i32 && fr.x() < r.x() + r.width() as i32,
            Nav::Left | Nav::Right => {
                r.y() < fr.y() + fr.height() as i32 && fr.y() < r.y() + r.height() as i32
            }
        };
        let score = primary + secondary * if overlap { 0.3 } else { 2.5 };
        if best.is_none_or(|(s, _)| score < s) {
            best = Some((score, *i));
        }
    }
    best.map(|(_, i)| i)
}

impl<'a> Frame<'a> {
    pub fn begin(g: &'a mut Gfx, st: &'a mut UiState, now_ms: u64) -> Frame<'a> {
        let mut activated = None;
        let mut wheel = 0;
        let pending = std::mem::take(&mut st.pending);
        for i in pending {
            match i {
                UiIn::Nav(d) => {
                    st.focused = match st.focused {
                        Some(f) if st.prev.iter().any(|(i, _)| *i == f) => {
                            navigate(&st.prev, f, d).or(Some(f))
                        }
                        _ => st.prev.first().map(|(i, _)| *i),
                    };
                }
                UiIn::Activate => activated = st.focused,
                UiIn::PointerMove(x, y) => {
                    st.pointer = Some((x, y));
                    if let Some((i, _)) = st.prev.iter().rev().find(|(_, r)| r.contains_point((x, y))) {
                        st.focused = Some(*i);
                    }
                }
                UiIn::Click(x, y) => {
                    st.pointer = Some((x, y));
                    if let Some((i, _)) = st.prev.iter().rev().find(|(_, r)| r.contains_point((x, y))) {
                        st.focused = Some(*i);
                        activated = Some(*i);
                    }
                }
                UiIn::Wheel(d) => wheel += d,
            }
        }
        if std::mem::take(&mut st.reset) {
            st.focused = None;
        }
        if let Some(w) = st.want_focus.take() {
            st.focused = Some(w);
        }
        Frame {
            g,
            st,
            items: Vec::new(),
            activated,
            wheel,
            inert: false,
            now_ms,
        }
    }

    /// Finish the frame; returns true when nothing was focusable (caller may reset focus).
    pub fn end(self) {
        let items = self.items;
        if let Some(f) = self.st.focused {
            if !items.iter().any(|(i, _)| *i == f) {
                self.st.focused = items.first().map(|(i, _)| *i);
            }
        } else {
            self.st.focused = items.first().map(|(i, _)| *i);
        }
        self.st.prev = items;
    }

    pub fn set_inert(&mut self, inert: bool) {
        self.inert = inert;
    }

    pub fn is_focused(&self, id: Id) -> bool {
        !self.inert && self.st.focused == Some(id)
    }

    /// Register a focusable rect; returns (focused, activated).
    pub fn register(&mut self, id: Id, r: Rect) -> (bool, bool) {
        if self.inert {
            return (false, false);
        }
        self.items.push((id, r));
        let f = self.st.focused == Some(id);
        let a = self.activated == Some(id);
        if a {
            self.activated = None;
        }
        (f, a)
    }

    pub fn focus_ring(&mut self, r: Rect, radius: i32) {
        self.g.round_outline(r, radius, theme::FOCUS, 4);
    }

    pub fn panel(&mut self, r: Rect) {
        self.g.round(r, 20, theme::PANEL);
    }

    /// Standard button. `primary` uses the accent colour.
    pub fn button(&mut self, id: Id, r: Rect, label: &str, primary: bool) -> bool {
        let (f, a) = self.register(id, r);
        let bg = if primary {
            theme::ACCENT
        } else if f {
            theme::PANEL_HI
        } else {
            theme::ROW
        };
        self.g.round(r, 14, bg);
        if f {
            self.focus_ring(r, 14);
        }
        self.g.text_centered(label, r, size::BODY, true, theme::TEXT);
        a
    }

    /// Button with a coloured remote-key badge (text label always present; colour is
    /// never the only cue).
    pub fn key_button(
        &mut self,
        id: Id,
        r: Rect,
        label: &str,
        key_color: Option<Color>,
        key_name: &str,
    ) -> bool {
        let (f, a) = self.register(id, r);
        self.g.round(r, 14, if f { theme::PANEL_HI } else { theme::ROW });
        if f {
            self.focus_ring(r, 14);
        }
        let mut x = r.x() + 16;
        if let Some(c) = key_color {
            let b = Rect::new(x, r.y() + (r.height() as i32 - 26) / 2, 26, 26);
            self.g.round(b, 13, c);
            x += 36;
        }
        let ty = r.y() + (r.height() as i32 - self.g.line_height(size::BODY)) / 2 + 2;
        let w = self.g.text(label, x, ty, size::BODY, true, theme::TEXT);
        if !key_name.is_empty() {
            let kx = x + w + 12;
            self.g
                .text(key_name, kx, ty + 3, size::SMALL, false, theme::TEXT_DIM);
        }
        a
    }

    /// Toggle row: label + On/Off state as text.
    pub fn toggle(&mut self, id: Id, r: Rect, label: &str, on: bool) -> bool {
        let (f, a) = self.register(id, r);
        self.g.round(r, 12, if f { theme::PANEL_HI } else { theme::ROW });
        if f {
            self.focus_ring(r, 12);
        }
        let ty = r.y() + (r.height() as i32 - self.g.line_height(size::BODY)) / 2 + 2;
        self.g.text_fit(
            label,
            r.x() + 24,
            ty,
            size::BODY,
            false,
            theme::TEXT,
            r.width() as i32 - 200,
        );
        let pill = Rect::new(
            r.x() + r.width() as i32 - 140,
            r.y() + (r.height() as i32 - 40) / 2,
            116,
            40,
        );
        self.g
            .round(pill, 20, if on { theme::ACCENT } else { theme::PANEL });
        self.g.text_centered(
            if on { "On" } else { "Off" },
            pill,
            size::SMALL,
            true,
            theme::TEXT,
        );
        a
    }

    /// Choice row: label + current value; activating cycles/opens.
    pub fn choice(&mut self, id: Id, r: Rect, label: &str, value: &str) -> bool {
        let (f, a) = self.register(id, r);
        self.g.round(r, 12, if f { theme::PANEL_HI } else { theme::ROW });
        if f {
            self.focus_ring(r, 12);
        }
        let ty = r.y() + (r.height() as i32 - self.g.line_height(size::BODY)) / 2 + 2;
        self.g.text_fit(
            label,
            r.x() + 24,
            ty,
            size::BODY,
            false,
            theme::TEXT,
            r.width() as i32 / 2,
        );
        let vw = self.g.text_width(value, size::BODY, true);
        let vx = r.x() + r.width() as i32 - 24 - vw - 30;
        self.g.text(value, vx, ty, size::BODY, true, theme::ACCENT2);
        self.g.text(
            "›",
            r.x() + r.width() as i32 - 40,
            ty,
            size::BODY,
            true,
            theme::TEXT_DIM,
        );
        a
    }

    /// Text field display (editing happens in the on-screen keyboard / physical keys).
    #[allow(clippy::too_many_arguments)]
    pub fn field(
        &mut self,
        id: Id,
        r: Rect,
        label: &str,
        value: &str,
        secret: bool,
        placeholder: &str,
        editing: bool,
    ) -> bool {
        let (f, a) = self.register(id, r);
        self.g
            .round(r, 12, if f || editing { theme::PANEL_HI } else { theme::ROW });
        if f || editing {
            self.focus_ring(r, 12);
        }
        self.g
            .text(label, r.x() + 20, r.y() + 8, size::SMALL, false, theme::TEXT_DIM);
        let shown: String = if secret {
            "•".repeat(value.chars().count())
        } else {
            value.to_owned()
        };
        let vy = r.y() + r.height() as i32 - self.g.line_height(size::LARGE) - 6;
        let max_w = r.width() as i32 - 40;
        if shown.is_empty() && !editing {
            self.g.text_fit(
                placeholder,
                r.x() + 20,
                vy,
                size::LARGE,
                false,
                Color::RGB(110, 125, 150),
                max_w,
            );
        } else {
            // Show the tail when the text overflows.
            let mut s = shown.clone();
            while self.g.text_width(&s, size::LARGE, false) > max_w - 20 && !s.is_empty() {
                s.remove(0);
            }
            let w = self.g.text(&s, r.x() + 20, vy, size::LARGE, false, theme::TEXT);
            if editing && (self.now_ms / 500).is_multiple_of(2) {
                self.g.fill(
                    Rect::new(
                        r.x() + 22 + w,
                        vy + 2,
                        3,
                        self.g.line_height(size::LARGE) as u32 - 6,
                    ),
                    theme::ACCENT2,
                );
            }
        }
        a
    }

    /// List row with a title and subtitle.
    pub fn row(
        &mut self,
        id: Id,
        r: Rect,
        title: &str,
        subtitle: &str,
        badge: Option<(&str, Color)>,
    ) -> bool {
        let (f, a) = self.register(id, r);
        self.g.round(r, 14, if f { theme::PANEL_HI } else { theme::ROW });
        if f {
            self.focus_ring(r, 14);
        }
        let max_w = r.width() as i32 - 48 - if badge.is_some() { 180 } else { 0 };
        self.g.text_fit(
            title,
            r.x() + 24,
            r.y() + 12,
            size::LARGE,
            true,
            theme::TEXT,
            max_w,
        );
        if !subtitle.is_empty() {
            self.g.text_fit(
                subtitle,
                r.x() + 24,
                r.y() + r.height() as i32 - 40,
                size::SMALL,
                false,
                theme::TEXT_DIM,
                max_w,
            );
        }
        if let Some((b, c)) = badge {
            let bw = self.g.text_width(b, size::SMALL, true) + 28;
            let br = Rect::new(r.x() + r.width() as i32 - bw - 20, r.y() + 16, bw as u32, 36);
            self.g.round(br, 18, c);
            self.g.text_centered(b, br, size::SMALL, true, theme::TEXT);
        }
        a
    }

    /// Keep the focused index of a vertical list visible; returns the first visible index.
    pub fn list_scroll(
        &mut self,
        list: Id,
        focused_index: Option<usize>,
        visible: usize,
        len: usize,
    ) -> usize {
        let mut first = *self.st.scroll.get(&list).unwrap_or(&0) as usize;
        if let Some(fi) = focused_index {
            if fi < first {
                first = fi;
            } else if fi >= first + visible {
                first = fi + 1 - visible;
            }
        }
        if self.wheel != 0 && len > visible {
            first = (first as i32 - self.wheel).clamp(0, (len - visible) as i32) as usize;
            self.wheel = 0;
        }
        first = first.min(len.saturating_sub(visible));
        self.st.scroll.insert(list, first as i32);
        first
    }

    pub fn scrim(&mut self) {
        self.g.fill(
            Rect::new(0, 0, crate::gfx::W as u32, crate::gfx::H as u32),
            theme::SCRIM,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometric_navigation() {
        let items = vec![
            (1, Rect::new(0, 0, 100, 50)),
            (2, Rect::new(200, 0, 100, 50)),
            (3, Rect::new(0, 100, 100, 50)),
            (4, Rect::new(200, 100, 100, 50)),
        ];
        assert_eq!(navigate(&items, 1, Nav::Right), Some(2));
        assert_eq!(navigate(&items, 1, Nav::Down), Some(3));
        assert_eq!(navigate(&items, 4, Nav::Up), Some(2));
        assert_eq!(navigate(&items, 4, Nav::Left), Some(3));
        assert_eq!(navigate(&items, 1, Nav::Up), None);
    }

    #[test]
    fn ids_are_stable_and_distinct() {
        assert_eq!(id("home"), id("home"));
        assert_ne!(idx("row", 0), idx("row", 1));
    }
}
