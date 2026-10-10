//! The quick menu, as OMSI 2's Alt menu: a grid of tiles over the picture, each one a thing
//! a driver does often - another bus, the timetable, the destination display, the service at
//! the depot, the time and the weather. Alt alone opens and closes it; the arrows, Enter and
//! the mouse work it. (OMSI 2's "Hand over bus to AI" and "Call police" are not there: the
//! game has neither.)

use glam::Vec2;
use omsi_ui::paint::Align;
use omsi_ui::{Rect, Weight};

use super::{Action, Shell};
use crate::launcher::theme::*;
use crate::launcher::ui::id_of;

/// The tiles in their order, row by row: (what it does, its icon, its name).
pub(crate) const TILES: [(&str, &str, &str); 15] = [
    ("place", "add", "New bus"),
    ("switch", "sync_alt", "Switch to another bus"),
    ("remove", "delete", "Delete bus"),
    ("teleport", "location_on", "Place bus elsewhere"),
    ("swap", "autorenew", "Swap bus"),
    ("duty", "take_duty", "Set timetable"),
    ("dest", "directions_bus", "Destination and route"),
    ("refuel", "local_parking", "Refuel"),
    ("wash", "water_drop", "Wash"),
    ("repair", "construction", "Repair bus"),
    ("arrows", "alt_route", "Route helpers"),
    ("world", "partly_cloudy_day", "Time and weather"),
    ("controls", "sports_esports", "Controls"),
    ("driver", "badge", "Driver profile"),
    ("reset", "restart_alt", "Back on its wheels"),
];

/// Tiles in a row.
pub(crate) const COLS: usize = 5;
const TILE: f32 = 104.0;
const GAP_T: f32 = 8.0;
const PAD: f32 = 16.0;

/// What the quick menu shows.
pub(crate) struct AltView {
    /// Whether each tile can be used now (a greyed one cannot).
    pub enabled: [bool; 15],
    /// The tile the keyboard is on.
    pub sel: usize,
    /// The keyboard chose last: its tile is shown lit.
    pub kbd: bool,
    /// Route helpers shown now (their tile is lit as a switch that is on).
    pub arrows_on: bool,
}

pub(crate) fn draw(sh: &mut Shell, v: &AltView) {
    let size = sh.ui.size;
    let rows = TILES.len().div_ceil(COLS);
    let w = COLS as f32 * TILE + (COLS - 1) as f32 * GAP_T + 2.0 * PAD;
    let h = rows as f32 * TILE + (rows - 1) as f32 * GAP_T + 2.0 * PAD + 26.0;
    let panel = Rect::new(((size.x - w) * 0.5).round(), ((size.y - h) * 0.5).round(), w, h);
    let ui = &mut sh.ui;
    // (a click beside the grid closes it)
    let full = Rect::new(0.0, 0.0, size.x, size.y);
    let (_, _, outside) = ui.interact(id_of("alt-outside"), full);
    ui.panel(panel);
    ui.text(&omsi_ui::tr("Quick menu").to_uppercase(), Vec2::new(panel.x + PAD, panel.y + PAD + 10.0), 11.0, Weight::Bold, TEXT_DIM, Align::Left);
    let mut picked = None;
    for (k, &(id, icon, label)) in TILES.iter().enumerate() {
        let (col, row) = (k % COLS, k / COLS);
        let r = Rect::new(panel.x + PAD + col as f32 * (TILE + GAP_T), panel.y + PAD + 26.0 + row as f32 * (TILE + GAP_T), TILE, TILE);
        let on = v.enabled[k];
        let (hover, held, clicked) = ui.interact(id_of(&format!("alt-{id}")), r);
        let lit = on && (hover || (v.kbd && v.sel == k));
        let bg = if held && on { SELECTED } else if lit { HOVER } else { FIELD };
        ui.p().rounded(r, RADIUS, bg);
        if v.kbd && v.sel == k {
            ui.p().rounded_border(r, RADIUS, 1.5, ACCENT);
        }
        let switched_on = id == "arrows" && v.arrows_on;
        let ic = if !on { TEXT_FAINT } else if switched_on { ACCENT } else { TEXT };
        ui.icon(icon, Vec2::new(r.x + r.w * 0.5, r.y + 40.0), 34.0, ic);
        let tc = if on { TEXT_SOFT } else { TEXT_FAINT };
        ui.text_in(&omsi_ui::tr(label), Rect::new(r.x + 6.0, r.y + 66.0, r.w - 12.0, 30.0), 12.0, Weight::Medium, tc, Align::Center);
        if clicked && on {
            picked = Some(k);
        }
    }
    if let Some(k) = picked {
        sh.actions.push(Action::Alt(k));
    } else if outside && !sh.ui.hover(panel) {
        sh.actions.push(Action::Alt(usize::MAX));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arrows_go_round_the_grid_past_the_greyed_tiles() {
        let mut en = [true; 15];
        assert_eq!(step(0, &en, 1), 1);
        assert_eq!(step(0, &en, -1), 14);
        assert_eq!(step(2, &en, COLS as isize), 7);
        assert_eq!(step(12, &en, COLS as isize), 2);
        // a greyed tile is passed: to the side to the next one, down to the next one along
        en[1] = false;
        en[7] = false;
        assert_eq!(step(0, &en, 1), 2);
        assert_eq!(step(2, &en, COLS as isize), 8);
        // nothing to go to: it stays
        assert_eq!(step(4, &[false; 15], 1), 4);
    }
}

/// The tile `by` tiles on from `sel` (the arrows: one to the side, a row up or down), round
/// the ends; onto a greyed tile, the next one that can be used that way.
pub(crate) fn step(sel: usize, enabled: &[bool; 15], by: isize) -> usize {
    let n = TILES.len() as isize;
    let mut k = (sel as isize + by).rem_euclid(n);
    for _ in 0..n {
        if enabled[k as usize] {
            return k as usize;
        }
        k = (k + by.signum()).rem_euclid(n);
    }
    sel
}
