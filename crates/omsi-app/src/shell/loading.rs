//! The loading screen, the launcher's and the game's alike: black, and in the middle the
//! map's or the server's name, a thin bar and a tip. Drawn with the launcher's toolkit and
//! its own textures - never with the scene's, which the map being loaded fills and reuses
//! meanwhile (the game's screen showed the sky through it, its texts as pieces of other
//! pictures) - and over nothing of the scene: the game shows once it is there.

use omsi_ui::paint::Align;
use omsi_ui::{Color, Rect, Weight};

use crate::launcher::theme::*;
use crate::launcher::ui::Ui;

/// `progress` 0..1, or None while it cannot be told (a bar that runs).
pub(crate) fn draw(ui: &mut Ui, title: &str, progress: Option<f32>, tip: &str) {
    let size = ui.size;
    let full = Rect::new(0.0, 0.0, size.x, size.y);
    ui.solid(full);
    ui.p().rect(full, Color::rgba(0, 0, 0, 1.0));
    let (cx, cy) = (size.x * 0.5, size.y * 0.5);
    ui.text_in(title, Rect::new(0.0, cy - 58.0, size.x, 26.0), 20.0, Weight::Bold, TEXT, Align::Center);
    let bw = 360.0f32.min(size.x * 0.6);
    let bar = Rect::new((cx - bw * 0.5).round(), (cy - 1.5).round(), bw, 3.0);
    ui.p().rect(bar, Color::rgba(44, 45, 48, 1.0));
    match progress {
        Some(p) => {
            let done = (bar.w * p.clamp(0.0, 1.0)).round();
            if done >= 1.0 {
                ui.p().rect(Rect::new(bar.x, bar.y, done, bar.h), ACCENT);
            }
            ui.text_in(&format!("{} %", (p.clamp(0.0, 1.0) * 100.0).round() as u32), Rect::new(0.0, cy + 10.0, size.x, 18.0), 12.0, Weight::Regular, TEXT_DIM, Align::Center);
        }
        None => {
            let run = (ui.time * 0.6).fract();
            let seg = bar.w * 0.28;
            let sx = bar.x + (bar.w + seg) * run - seg;
            let (a, b) = (sx.max(bar.x), (sx + seg).min(bar.right()));
            if b > a {
                ui.p().rect(Rect::new(a, bar.y, b - a, bar.h), ACCENT);
            }
        }
    }
    if !tip.is_empty() {
        ui.text_in(tip, Rect::new(0.0, cy + 40.0, size.x, 22.0), 13.0, Weight::Regular, TEXT_DIM, Align::Center);
    }
}
