//! The loading screen, the launcher's and the game's alike: black, and in the middle the
//! map's or the server's name, a thin bar and a tip. Drawn with the launcher's toolkit and
//! its own textures - never with the scene's, which the map being loaded fills and reuses
//! meanwhile (the game's screen showed the sky through it, its texts as pieces of other
//! pictures) - and over nothing of the scene: the game shows once it is there.
//!
//! The edition's panel may give it a look (`load_theme`): a background picture over the whole
//! window, a logo, a line under the name, the colours, which parts show, and one of the
//! layouts (`load_theme::Template`): the plain one in the middle, or a bus network's own -
//! the line's diagram along a band at the bottom, a destination sign, a poster column. Those
//! are drawn in solid colours with square corners, so that they read the same over any
//! picture.

use glam::Vec2;
use omsi_ui::paint::Align;
use omsi_ui::{Color, Rect, Weight};

use crate::launcher::theme::*;
use crate::launcher::ui::Ui;
use crate::load_theme::{Style, Template};

/// The panel's pictures as one screen has them on its graphics card (`load_theme`): the
/// texture and its size.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub(crate) struct Look {
    pub(crate) background: Option<(usize, u32, u32)>,
    pub(crate) logo: Option<(usize, u32, u32)>,
}

/// What every layout draws: the texts, the colours and how far the load is.
struct Parts<'a> {
    title: &'a str,
    subtitle: &'a str,
    tip: &'a str,
    /// 0..1, or None while it cannot be told (the bar runs).
    progress: Option<f32>,
    accent: Color,
    /// The solid band, sign or column behind the texts.
    panel: Color,
    text: Color,
    /// The quieter texts: the text colour towards the panel's (solid, never see-through).
    quiet: Color,
    logo: Option<(usize, u32, u32)>,
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
    let rgb = |c: [u8; 3]| Color::rgba(c[0], c[1], c[2], 1.0);
    let accent = theme.as_ref().and_then(|t| t.accent).map(rgb).unwrap_or(ACCENT);
    let panel = st.panel_color.map(rgb).unwrap_or(Color::hex(0x0b1424));
    let text = st.text_color.map(rgb).unwrap_or(TEXT);
    let parts = Parts {
        title: if !st.show_title { "" } else if st.title.is_empty() { title } else { st.title.as_str() },
        subtitle: theme.as_ref().map(|t| t.subtitle.as_str()).unwrap_or(""),
        tip,
        progress,
        accent,
        panel,
        text,
        quiet: if st.text_color.is_some() { text.mix(panel, 0.3) } else { TEXT_DIM },
        logo: look.logo,
    };
    match st.template {
        Template::Classic => classic(ui, &st, &parts),
        Template::Line => line(ui, &st, &parts),
        Template::Sign => sign(ui, &st, &parts),
        Template::Poster => poster(ui, &st, &parts),
    }
}

/// The logo `h` high at the most (and `max_w` wide), its top left at `at`; its size.
fn logo(ui: &mut Ui, logo: Option<(usize, u32, u32)>, at: Vec2, h: f32, max_w: f32) -> Option<Vec2> {
    let (tex, w, lh) = logo?;
    let k = (h / lh.max(1) as f32).min(max_w / w.max(1) as f32);
    let (dw, dh) = ((w as f32 * k).round(), (lh as f32 * k).round());
    ui.image(Rect::new(at.x.round(), at.y.round(), dw, dh), tex, 0.0);
    Some(Vec2::new(dw, dh))
}

/// The bar that runs while the load cannot be told: where its piece is (0..1 along).
fn running(ui: &Ui) -> f32 {
    (ui.time * 0.6).fract()
}

/// The size `text` is drawn at to fit in `width`: `px`, or smaller down to `min` (and cut).
fn fit_px(ui: &Ui, text: &str, px: f32, min: f32, width: f32, weight: Weight) -> f32 {
    let w = ui.width(text, px, weight);
    if w > width { (px * width / w).floor().max(min) } else { px }
}

fn percent(p: f32) -> String {
    format!("{} %", (p.clamp(0.0, 1.0) * 100.0).round() as u32)
}

/// The plain screen: the name, the line under it, a bar and the tip, in the middle or near
/// the bottom, the logo above.
fn classic(ui: &mut Ui, st: &Style, x: &Parts) {
    let size = ui.size;
    let cx = size.x * 0.5;
    let cy = if st.bottom { (size.y - 120.0).max(size.y * 0.5) } else { size.y * 0.5 };
    if let Some((_, w, h)) = x.logo {
        let k = (st.logo_height / h.max(1) as f32).min(size.x * 0.5 / w.max(1) as f32);
        let (dw, dh) = (w as f32 * k, h as f32 * k);
        logo(ui, x.logo, Vec2::new(cx - dw * 0.5, cy - 84.0 - dh), st.logo_height, size.x * 0.5);
    }
    if !x.title.is_empty() {
        ui.text_in(x.title, Rect::new(0.0, cy - 58.0, size.x, 26.0), 20.0, Weight::Bold, x.text, Align::Center);
    }
    if !x.subtitle.is_empty() {
        ui.text_in(x.subtitle, Rect::new(0.0, cy - 29.0, size.x, 18.0), 13.0, Weight::Regular, x.text, Align::Center);
    }
    let bh = st.bar_height;
    let bw = if st.bar_wide { (size.x * 0.6).min(900.0) } else { 360.0f32.min(size.x * 0.6) };
    let bar = Rect::new((cx - bw * 0.5).round(), (cy - bh * 0.5).round(), bw, bh);
    let mut below = bar.bottom() + 8.5;
    if st.show_bar {
        ui.p().rect(bar, Color::rgba(44, 45, 48, 1.0));
        match x.progress {
            Some(p) => {
                let done = (bar.w * p.clamp(0.0, 1.0)).round();
                if done >= 1.0 {
                    ui.p().rect(Rect::new(bar.x, bar.y, done, bar.h), x.accent);
                }
            }
            None => {
                let seg = bar.w * 0.28;
                let sx = bar.x + (bar.w + seg) * running(ui) - seg;
                let (a, b) = (sx.max(bar.x), (sx + seg).min(bar.right()));
                if b > a {
                    ui.p().rect(Rect::new(a, bar.y, b - a, bar.h), x.accent);
                }
            }
        }
    }
    if let (Some(p), true) = (x.progress, st.show_percent) {
        ui.text_in(&percent(p), Rect::new(0.0, below, size.x, 18.0), 12.0, Weight::Regular, x.quiet, Align::Center);
        below += 30.0;
    } else {
        below += 6.0;
    }
    if !x.tip.is_empty() {
        ui.text_in(x.tip, Rect::new(0.0, below, size.x, 22.0), 13.0, Weight::Regular, x.quiet, Align::Center);
    }
}

/// How many stops the line's diagram has.
const STOPS: usize = 7;

/// The line: a solid band along the bottom under a stripe of the accent - the logo, "DESTINATION"
/// and the name large, the line under it - and on its right the line's diagram, the stops
/// passed lit as the load goes on; the tip in a strip of its own.
fn line(ui: &mut Ui, st: &Style, x: &Parts) {
    let size = ui.size;
    let band_h = 176.0;
    let band = Rect::new(0.0, size.y - band_h, size.x, band_h);
    ui.p().rect(band, x.panel);
    ui.p().rect(Rect::new(0.0, band.y, size.x, 6.0), x.accent);
    let pad = 56.0;
    let mut col_x = pad;
    if let Some(d) = logo(ui, x.logo, Vec2::new(pad, band.y + 34.0), st.logo_height.min(72.0), size.x * 0.2) {
        col_x += d.x + 32.0;
        ui.p().rect(Rect::new(col_x - 16.0, band.y + 34.0, 2.0, 72.0), x.panel.lighten(0.18));
    }
    // the destination: the left part of the band, the line's diagram on the right
    let split = (size.x * 0.52).max(col_x + 260.0).min(size.x - 360.0);
    let col_w = (split - col_x - 40.0).max(120.0);
    ui.text_in("DESTINATION", Rect::new(col_x, band.y + 30.0, col_w, 14.0), 11.0, Weight::Bold, x.accent, Align::Left);
    if !x.title.is_empty() {
        let t = x.title.to_uppercase();
        let px = fit_px(ui, &t, 34.0, 22.0, col_w, Weight::Black);
        ui.text_in(&t, Rect::new(col_x, band.y + 46.0, col_w, 42.0), px, Weight::Black, x.text, Align::Left);
    }
    if !x.subtitle.is_empty() {
        ui.text_in(x.subtitle, Rect::new(col_x, band.y + 92.0, col_w, 20.0), 14.0, Weight::Regular, x.quiet, Align::Left);
    }
    // the line's diagram: a track, the stops as squares, the part travelled in the accent
    let pct_w = if st.show_percent && x.progress.is_some() { 110.0 } else { 0.0 };
    let (l0, l1) = (split, size.x - pad - pct_w);
    let ly = band.y + 66.0;
    if st.show_bar && l1 - l0 > 120.0 {
        let track = x.panel.lighten(0.22);
        let p = x.progress.map(|p| p.clamp(0.0, 1.0));
        let at = p.unwrap_or_else(|| running(ui));
        let lit = |t: f32| p.map(|p| t <= p + 1e-4).unwrap_or(false);
        ui.p().rect(Rect::new(l0, ly - 2.0, l1 - l0, 4.0), track);
        let reached = l0 + (l1 - l0) * at;
        if p.is_some() {
            ui.p().rect(Rect::new(l0, ly - 2.0, (reached - l0).max(0.0), 4.0), x.accent);
        }
        let next = (0..STOPS).find(|&i| !lit(i as f32 / (STOPS - 1) as f32));
        for i in 0..STOPS {
            let t = i as f32 / (STOPS - 1) as f32;
            let sx = l0 + (l1 - l0) * t;
            let s = if Some(i) == next && p.is_some() { 22.0 } else { 14.0 };
            let r = Rect::new((sx - s * 0.5).round(), (ly - s * 0.5).round(), s, s);
            if lit(t) {
                ui.p().rect(r, x.accent);
            } else {
                ui.p().rect(r, track);
                ui.p().rect(r.inset(if s > 16.0 { 5.0 } else { 3.0 }), if Some(i) == next && p.is_some() { x.accent } else { x.panel });
            }
        }
        // while the load cannot be told, a bus-length piece runs along the line
        if p.is_none() {
            let w = (l1 - l0) * 0.12;
            let a = (reached - w).max(l0);
            ui.p().rect(Rect::new(a, ly - 2.0, (reached - a).max(0.0), 4.0), x.accent);
        }
        ui.text_in("DÉPART", Rect::new(l0 - 40.0, ly + 18.0, 80.0, 14.0), 10.0, Weight::Bold, x.quiet, Align::Center);
        ui.text_in("TERMINUS", Rect::new(l1 - 50.0, ly + 18.0, 100.0, 14.0), 10.0, Weight::Bold, x.quiet, Align::Center);
    }
    if let (Some(p), true) = (x.progress, st.show_percent) {
        ui.text_in(&percent(p), Rect::new(size.x - pad - 90.0, ly - 20.0, 90.0, 40.0), 30.0, Weight::Black, x.text, Align::Right);
    }
    // the tip: a strip of the accent with its label, and the tip beside it
    if !x.tip.is_empty() {
        let ty = band.bottom() - 42.0;
        let label = "INFO";
        let lw = ui.width(label, 11.0, Weight::Bold) + 20.0;
        ui.p().rect(Rect::new(col_x, ty, lw, 22.0), x.accent);
        ui.text_in(label, Rect::new(col_x, ty, lw, 22.0), 11.0, Weight::Bold, x.panel, Align::Center);
        ui.text_in(x.tip, Rect::new(col_x + lw + 14.0, ty, size.x - col_x - lw - 14.0 - pad, 22.0), 13.0, Weight::Regular, x.text, Align::Left);
    }
}

/// The destination sign: a black box as a bus's front display - the line's number on the
/// accent at its left, the name in capitals in the accent (its lamps) - its row of lamps
/// under it lighting up as the load goes on, the tip under them.
fn sign(ui: &mut Ui, st: &Style, x: &Parts) {
    let size = ui.size;
    let w = (size.x - 112.0).min(1040.0);
    let h = 116.0;
    let b = Rect::new(((size.x - w) * 0.5).round(), (size.y - h - 120.0).round(), w, h);
    if let Some((_, lw, lh)) = x.logo {
        let k = (st.logo_height.min(96.0) / lh.max(1) as f32).min(w * 0.5 / lw.max(1) as f32);
        logo(ui, x.logo, Vec2::new(b.x, b.y - 28.0 - lh as f32 * k), st.logo_height.min(96.0), w * 0.5);
    }
    ui.p().rect(b.inset(-4.0), Color::hex(0x1e1e1e));
    ui.p().rect(b, Color::hex(0x050505));
    // the line's number
    let label = if st.line_label.is_empty() { "TLA" } else { st.line_label.as_str() };
    let bw = (ui.width(label, 44.0, Weight::Black) + 40.0).max(h - 24.0);
    let badge = Rect::new(b.x + 12.0, b.y + 12.0, bw, h - 24.0);
    ui.p().rect(badge, x.accent);
    ui.text_in(label, badge, 44.0, Weight::Black, Color::hex(0x050505), Align::Center);
    // the destination in the sign's lamps
    let tx = badge.right() + 28.0;
    let tw = b.right() - tx - 24.0;
    let lamps = x.accent;
    if !x.title.is_empty() {
        let t = x.title.to_uppercase();
        let px = fit_px(ui, &t, 40.0, 26.0, tw, Weight::Bold);
        ui.text_in(&t, Rect::new(tx, b.y + 18.0, tw, 48.0), px, Weight::Bold, lamps, Align::Left);
    }
    if !x.subtitle.is_empty() {
        ui.text_in(&x.subtitle.to_uppercase(), Rect::new(tx, b.y + 70.0, tw, 26.0), 17.0, Weight::Condensed, lamps.darken(0.25), Align::Left);
    }
    // the row of lamps and the tip on a solid strip under the sign (read over any picture)
    let strip_h = if st.show_bar { 34.0 } else { 0.0 } + if x.tip.is_empty() { 0.0 } else { 22.0 };
    if strip_h > 0.0 {
        ui.p().rect(Rect::new(b.x - 4.0, b.bottom() + 4.0, b.w + 8.0, strip_h + 26.0), x.panel);
    }
    let mut y = b.bottom() + 22.0;
    if st.show_bar {
        let (seg, gap) = (14.0, 5.0);
        let pct_w = if st.show_percent && x.progress.is_some() { 72.0 } else { 0.0 };
        let n = (((b.w - pct_w) + gap) / (seg + gap)).floor().max(4.0) as usize;
        let off = Color::hex(0x1c1c1c);
        let run = running(ui);
        for i in 0..n {
            let t = (i as f32 + 0.5) / n as f32;
            let on = match x.progress {
                Some(p) => t <= p.clamp(0.0, 1.0),
                None => (t - run).abs() < 0.08,
            };
            ui.p().rect(Rect::new(b.x + i as f32 * (seg + gap), y, seg, 10.0), if on { lamps } else { off });
        }
        if let (Some(p), true) = (x.progress, st.show_percent) {
            ui.text_in(&percent(p), Rect::new(b.right() - 64.0, y - 6.0, 64.0, 22.0), 16.0, Weight::Condensed, x.text, Align::Right);
        }
        y += 34.0;
    }
    if !x.tip.is_empty() {
        ui.text_in(x.tip, Rect::new(b.x, y, b.w, 22.0), 14.0, Weight::Regular, x.text, Align::Left);
    }
}

/// The poster: a solid column down the left edge beside a stripe of the accent - the logo at
/// the top, the name very large, the line under it, the percentage in large figures over a
/// bar, the tip at the bottom - and the picture on the rest of the window.
fn poster(ui: &mut Ui, st: &Style, x: &Parts) {
    let size = ui.size;
    let cw = (size.x * 0.36).clamp(380.0, 620.0).min(size.x);
    ui.p().rect(Rect::new(0.0, 0.0, cw, size.y), x.panel);
    ui.p().rect(Rect::new(cw, 0.0, 8.0, size.y), x.accent);
    let pad = 48.0;
    let inner = cw - pad * 2.0;
    let mut y = 56.0;
    if let Some(d) = logo(ui, x.logo, Vec2::new(pad, y), st.logo_height.min(110.0), inner) {
        y += d.y + 56.0;
    }
    if !x.title.is_empty() {
        let t = x.title.to_uppercase();
        let px = if ui.paragraph_height(&t, inner, 46.0, Weight::Black) > 46.0 * 1.38 * 3.0 { 34.0 } else { 46.0 };
        y += ui.paragraph(&t, Vec2::new(pad, y), inner, px, Weight::Black, x.text) - px * 0.2;
    }
    if !x.subtitle.is_empty() {
        ui.p().rect(Rect::new(pad, y + 6.0, 40.0, 4.0), x.accent);
        ui.paragraph(x.subtitle, Vec2::new(pad, y + 18.0), inner, 15.0, Weight::Regular, x.quiet);
    }
    // from the bottom up: the tip, the bar, the percentage
    let mut bottom = size.y - 56.0;
    if !x.tip.is_empty() {
        let th = ui.paragraph_height(x.tip, inner, 14.0, Weight::Regular);
        bottom -= th;
        ui.paragraph(x.tip, Vec2::new(pad, bottom), inner, 14.0, Weight::Regular, x.text);
        bottom -= 24.0;
        ui.text_in("INFO", Rect::new(pad, bottom, inner, 16.0), 11.0, Weight::Bold, x.accent, Align::Left);
        bottom -= 32.0;
    }
    if st.show_bar {
        let bh = st.bar_height.max(4.0);
        let bar = Rect::new(pad, bottom - bh, inner, bh);
        ui.p().rect(bar, x.panel.lighten(0.15));
        match x.progress {
            Some(p) => ui.p().rect(Rect::new(bar.x, bar.y, (bar.w * p.clamp(0.0, 1.0)).round(), bar.h), x.accent),
            None => {
                let seg = bar.w * 0.28;
                let sx = bar.x + (bar.w + seg) * running(ui) - seg;
                let (a, b) = (sx.max(bar.x), (sx + seg).min(bar.right()));
                if b > a {
                    ui.p().rect(Rect::new(a, bar.y, b - a, bar.h), x.accent);
                }
            }
        }
        bottom = bar.y - 14.0;
    }
    if let (Some(p), true) = (x.progress, st.show_percent) {
        ui.text_in(&percent(p), Rect::new(pad, bottom - 76.0, inner, 76.0), 72.0, Weight::Black, x.text, Align::Left);
    }
}
