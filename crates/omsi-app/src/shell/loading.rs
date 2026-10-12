//! The loading screen, the launcher's and the game's alike: black, and in the middle the
//! map's or the server's name, a thin bar and a tip. Drawn with the launcher's toolkit and
//! its own textures - never with the scene's, which the map being loaded fills and reuses
//! meanwhile (the game's screen showed the sky through it, its texts as pieces of other
//! pictures) - and over nothing of the scene: the game shows once it is there.
//!
//! The edition's panel may give it a look (`load_theme`): a background picture over the whole
//! window, darkened for the texts, a logo above the name, a line under it, the bar's colour
//! and the tips, and which parts show and where (`load_theme::Style`).

use omsi_ui::paint::Align;
use omsi_ui::{Color, Rect, Weight};

use crate::launcher::theme::*;
use crate::launcher::ui::Ui;

/// The panel's pictures as one screen has them on its graphics card (`load_theme`): the
/// texture and its size.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub(crate) struct Look {
    pub(crate) background: Option<(usize, u32, u32)>,
    pub(crate) logo: Option<(usize, u32, u32)>,
}

/// `progress` 0..1, or None while it cannot be told (a bar that runs).
pub(crate) fn draw(ui: &mut Ui, title: &str, progress: Option<f32>, tip: &str, look: Look) {
    let theme = crate::load_theme::current();
    let st = theme.as_ref().map(|t| t.style.clone()).unwrap_or_default();
    let size = ui.size;
    let full = Rect::new(0.0, 0.0, size.x, size.y);
    ui.solid(full);
    ui.p().rect(full, Color::rgba(0, 0, 0, 1.0));
    if let Some((tex, w, h)) = look.background {
        // over the whole window, cut evenly where its shape is not the picture's
        let k = (size.x / w.max(1) as f32).max(size.y / h.max(1) as f32);
        let (dw, dh) = (w as f32 * k, h as f32 * k);
        ui.image(Rect::new(((size.x - dw) * 0.5).round(), ((size.y - dh) * 0.5).round(), dw.round(), dh.round()), tex, 0.0);
        let dim = theme.as_ref().map(|t| t.dim).unwrap_or(0.0);
        if dim > 0.0 {
            ui.p().rect(full, Color::rgba(0, 0, 0, dim));
        }
    }
    let accent = theme.as_ref().and_then(|t| t.accent).map(|c| Color::rgba(c[0], c[1], c[2], 1.0)).unwrap_or(ACCENT);
    // the names in the text colour, the percentage and the tip in it a little faded
    let (text, dim_text) = match st.text_color {
        Some(c) => (Color::rgba(c[0], c[1], c[2], 1.0), Color::rgba(c[0], c[1], c[2], 0.7)),
        None => (TEXT, TEXT_DIM),
    };
    // (the bar's middle: the window's, or near its bottom)
    let cx = size.x * 0.5;
    let cy = if st.bottom { (size.y - 120.0).max(size.y * 0.5) } else { size.y * 0.5 };
    if let Some((tex, w, h)) = look.logo {
        let k = (st.logo_height / h.max(1) as f32).min(size.x * 0.5 / w.max(1) as f32);
        let (dw, dh) = ((w as f32 * k).round(), (h as f32 * k).round());
        ui.image(Rect::new((cx - dw * 0.5).round(), (cy - 84.0 - dh).round(), dw, dh), tex, 0.0);
    }
    if st.show_title {
        let title = if st.title.is_empty() { title } else { st.title.as_str() };
        ui.text_in(title, Rect::new(0.0, cy - 58.0, size.x, 26.0), 20.0, Weight::Bold, text, Align::Center);
    }
    if let Some(sub) = theme.as_ref().map(|t| t.subtitle.as_str()).filter(|s| !s.is_empty()) {
        ui.text_in(sub, Rect::new(0.0, cy - 29.0, size.x, 18.0), 13.0, Weight::Regular, text, Align::Center);
    }
    let bh = st.bar_height;
    let bw = if st.bar_wide { (size.x * 0.6).min(900.0) } else { 360.0f32.min(size.x * 0.6) };
    let bar = Rect::new((cx - bw * 0.5).round(), (cy - bh * 0.5).round(), bw, bh);
    let mut below = bar.bottom() + 8.5;
    if st.show_bar {
        ui.p().rect(bar, Color::rgba(44, 45, 48, 1.0));
        match progress {
            Some(p) => {
                let done = (bar.w * p.clamp(0.0, 1.0)).round();
                if done >= 1.0 {
                    ui.p().rect(Rect::new(bar.x, bar.y, done, bar.h), accent);
                }
            }
            None => {
                let run = (ui.time * 0.6).fract();
                let seg = bar.w * 0.28;
                let sx = bar.x + (bar.w + seg) * run - seg;
                let (a, b) = (sx.max(bar.x), (sx + seg).min(bar.right()));
                if b > a {
                    ui.p().rect(Rect::new(a, bar.y, b - a, bar.h), accent);
                }
            }
        }
    }
    if let (Some(p), true) = (progress, st.show_percent) {
        ui.text_in(&format!("{} %", (p.clamp(0.0, 1.0) * 100.0).round() as u32), Rect::new(0.0, below, size.x, 18.0), 12.0, Weight::Regular, dim_text, Align::Center);
        below += 30.0;
    } else {
        below += 6.0;
    }
    if !tip.is_empty() {
        ui.text_in(tip, Rect::new(0.0, below, size.x, 22.0), 13.0, Weight::Regular, dim_text, Align::Center);
    }
}
