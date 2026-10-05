//! The title screen: the launcher as a game's main menu, when it is the menu of the one full
//! screen window (see the app's `shell.rs`). The chosen bus, in the chosen light, fills the
//! screen behind a column of entries - Continue, Drive, Multiplayer, Profile, Settings,
//! Controls, Mods, Quit - worked with the mouse or the keyboard (arrows, Enter, Escape).
//! The entries open the launcher's pages as they are; Escape or "Main menu" comes back here.

use super::theme::*;
use super::ui::{self, Key};
use super::{Launcher, Page};
use omsi_ui::{Color, Rect, Weight};
use omsi_ui::paint::Align;

/// Where the title screen stands.
#[derive(Default)]
pub struct Title {
    /// Shown (instead of a page).
    pub open: bool,
    /// The entry chosen, and where its frame is drawn now (it glides to the entry).
    sel: usize,
    frame_y: Option<f32>,
    /// Seconds since it was opened (it fades in).
    shown_for: f32,
    /// "Quit openOMSI?" asked.
    confirm_quit: bool,
    /// The player asked to quit: the window closes at the next frame.
    pub quit: bool,
}

/// The title screen is the launcher's face: a computer whose launcher is the game's menu.
pub fn enabled() -> bool {
    omsi_launcher_lib::in_process_games() && !super::mobile::mobile()
}

#[derive(Clone, Copy, PartialEq)]
enum Entry {
    Continue,
    Page(Page),
    Quit,
}

const ENTRIES: [(Entry, &str, &str); 8] = [
    (Entry::Continue, "Continue", "Drive again with the duty chosen last."),
    (Entry::Page(Page::Drive), "Drive", "Choose the bus, the map, the line and the time of day."),
    (Entry::Page(Page::Multiplayer), "Multiplayer", "Join a server or a friend's game, or host one."),
    (Entry::Page(Page::Profile), "Profile", "Your driver: name, career and records."),
    (Entry::Page(Page::Settings), "Settings", "Graphics, sound, driving and gameplay."),
    (Entry::Page(Page::Controls), "Controls", "Keyboard, wheels, pedals and gamepads."),
    (Entry::Page(Page::Mods), "Mods", "Install and manage add-ons."),
    (Entry::Quit, "Quit", "Close openOMSI."),
];

impl Title {
    /// Open from the start when the launcher is the game's menu.
    pub fn new() -> Title {
        Title { open: enabled(), ..Default::default() }
    }

    /// Back to the title screen (from a page, or from a drive that ended).
    pub fn show(&mut self) {
        if !self.open {
            self.open = true;
            self.shown_for = 0.0;
            self.confirm_quit = false;
        }
    }
}

/// What the Continue entry would drive, or why it cannot.
fn continue_line(l: &Launcher) -> Result<String, &'static str> {
    let (Some(bus), Some(map)) = (l.state.bus(), l.state.map()) else {
        return Err("Choose a bus and a map under Drive first.");
    };
    let c = &l.state.choice;
    let mut s = format!("{} · {}", bus.name, map.name);
    if let (Some(line), Some(tour)) = (c.line.as_ref(), c.tour.as_ref()) {
        if !c.free {
            s.push_str(&format!(" · {line}/{tour}"));
        }
    }
    Ok(s)
}

/// Draw the title screen and do what was chosen on it.
pub(super) fn draw(l: &mut Launcher) {
    let size = l.ui.size;
    let full = Rect::new(0.0, 0.0, size.x, size.y);
    l.title.shown_for += l.ui.dt;
    let fade = (l.title.shown_for / 0.35).min(1.0);

    // --- the chosen bus, the whole screen behind the menu (the showroom's picture)
    l.ui.solid(full);
    l.ui.p().rect(full, Color::rgba(8, 9, 11, 1.0));
    if !l.state.choice.bus.is_empty() {
        l.preview_rect = Some(full);
        if let (Some(tex), true) = (l.preview_tex, l.showroom.has_picture()) {
            // (darkened in the drawing itself: the menu reads over any bus and any light)
            l.ui.image_tinted(full, tex, 0.0, Color::rgba(110, 110, 115, 1.0));
        }
    }
    // darker towards the edges and the bottom, so that the menu reads over any picture
    l.ui.p().rect(full, Color::rgba(0, 0, 0, 0.18));
    // a soft dark band behind the column of the menu (the bus stands in the middle)
    let band_w = (420.0f32).min(size.x - 48.0);
    let feather = 120.0;
    let bx = (size.x - band_w) * 0.5;
    let shade = Color::rgba(0, 0, 0, 0.5);
    l.ui.p().rect(Rect::new(bx, 0.0, band_w, size.y), shade);
    l.ui.p().gradient_h(Rect::new(bx - feather, 0.0, feather, size.y), Color::rgba(0, 0, 0, 0.0), shade);
    l.ui.p().gradient_h(Rect::new(bx + band_w, 0.0, feather, size.y), shade, Color::rgba(0, 0, 0, 0.0));
    let (bottom, _) = full.cut_bottom(size.y * 0.45);
    l.ui.p().gradient(bottom, Color::rgba(0, 0, 0, 0.0), Color::rgba(0, 0, 0, 0.72));
    let (left, _) = full.cut_left(size.x * 0.3);
    l.ui.p().gradient_h(left, Color::rgba(0, 0, 0, 0.45), Color::rgba(0, 0, 0, 0.0));
    let (right, _) = full.cut_right(size.x * 0.3);
    l.ui.p().gradient_h(right, Color::rgba(0, 0, 0, 0.0), Color::rgba(0, 0, 0, 0.45));

    // --- the title
    let title_h = (size.y * 0.11).clamp(56.0, 120.0);
    let ty = size.y * 0.17;
    l.ui.text_in("openOMSI", Rect::new(0.0, ty, size.x, title_h), title_h * 0.82, Weight::Black, TEXT.alpha(fade), Align::Center);
    l.ui.text_in("BUS SIMULATOR", Rect::new(0.0, ty + title_h, size.x, 24.0), 14.0, Weight::Medium, TEXT_DIM.alpha(fade), Align::Center);

    // --- the entries
    let can_continue = continue_line(l);
    let row_h = 46.0;
    let w = 340.0f32.min(size.x - 48.0);
    let x = (size.x - w) * 0.5;
    let y0 = (size.y * 0.42).max(ty + title_h + 48.0);
    let mut chosen: Option<Entry> = None;
    let confirm = l.title.confirm_quit;
    for (i, (entry, label, _)) in ENTRIES.iter().enumerate() {
        let r = Rect::new(x, y0 + i as f32 * row_h, w, row_h - 8.0);
        let off = *entry == Entry::Continue && can_continue.is_err();
        if !confirm {
            let id = ui::id_of(&format!("title-{label}"));
            let (hover, _, clicked) = l.ui.interact(id, r);
            if hover && !off {
                l.title.sel = i;
            }
            if clicked && !off {
                chosen = Some(*entry);
            }
        }
        let sel = l.title.sel == i;
        let c = if off { TEXT_FAINT } else if sel { TEXT } else { TEXT_SOFT };
        l.ui.text_in(label, r, if sel { 17.0 } else { 16.0 }, if sel { Weight::Bold } else { Weight::Medium }, c.alpha(fade), Align::Center);
    }
    // the frame round the chosen entry, gliding to it
    let target = y0 + l.title.sel as f32 * row_h;
    let fy = match l.title.frame_y {
        Some(y) => y + (target - y) * (1.0 - (-l.ui.dt * 18.0).exp()),
        None => target,
    };
    l.title.frame_y = Some(fy);
    let frame = Rect::new(x, fy, w, row_h - 8.0);
    l.ui.p().rounded(frame, 3.0, Color::rgba(255, 255, 255, 0.06 * fade));
    l.ui.p().rounded_border(frame, 3.0, 1.5, TEXT.alpha(0.85 * fade));

    // --- what the chosen entry does, the keys, the version
    let (_, _, hint) = ENTRIES[l.title.sel];
    let hint_text = match (ENTRIES[l.title.sel].0, &can_continue) {
        (Entry::Continue, Ok(s)) => format!("{} {s}", omsi_ui::tr(hint)),
        (Entry::Continue, Err(why)) => omsi_ui::tr(why).to_string(),
        _ => omsi_ui::tr(hint).to_string(),
    };
    let hy = size.y - 86.0;
    l.ui.text_in(&hint_text, Rect::new(24.0, hy, size.x - 48.0, 22.0), 14.0, Weight::Regular, TEXT_SOFT.alpha(fade), Align::Center);
    let keys = [("Esc", "Quit"), ("Enter", "Select"), ("↑ ↓", "Move")];
    let kw = 150.0;
    let kx0 = (size.x - kw * keys.len() as f32) * 0.5;
    for (i, (k, what)) in keys.iter().enumerate() {
        let kx = kx0 + i as f32 * kw;
        let cap = Rect::new(kx + 20.0, hy + 32.0, 44.0, 22.0);
        l.ui.p().rounded_border(cap, 3.0, 1.0, TEXT_DIM.alpha(fade));
        l.ui.text_in(k, cap, 11.0, Weight::Bold, TEXT_SOFT.alpha(fade), Align::Center);
        l.ui.text_in(what, Rect::new(cap.right() + 8.0, cap.y, kw - 72.0, cap.h), 13.0, Weight::Regular, TEXT_SOFT.alpha(fade), Align::Left);
    }
    l.ui.text_in(crate::startup::VERSION, Rect::new(size.x - 260.0, size.y - 34.0, 236.0, 20.0), 12.0, Weight::Regular, TEXT_DIM.alpha(fade), Align::Right);

    // --- the keyboard (no text field on this screen: the keys are the menu's)
    let keys_now = l.ui.input.keys.clone();
    if confirm {
        if keys_now.contains(&Key::Enter) {
            l.title.quit = true;
        } else if keys_now.contains(&Key::Escape) {
            l.title.confirm_quit = false;
        }
        quit_dialog(l);
        return;
    }
    let n = ENTRIES.len();
    let usable = |i: usize| !(ENTRIES[i].0 == Entry::Continue && can_continue.is_err());
    for k in &keys_now {
        match k {
            Key::Down | Key::Tab => {
                let mut i = l.title.sel;
                for _ in 0..n {
                    i = (i + 1) % n;
                    if usable(i) {
                        break;
                    }
                }
                l.title.sel = i;
            }
            Key::Up => {
                let mut i = l.title.sel;
                for _ in 0..n {
                    i = (i + n - 1) % n;
                    if usable(i) {
                        break;
                    }
                }
                l.title.sel = i;
            }
            Key::Home => l.title.sel = if usable(0) { 0 } else { 1 },
            Key::End => l.title.sel = n - 1,
            Key::Enter if usable(l.title.sel) => chosen = Some(ENTRIES[l.title.sel].0),
            Key::Escape => l.title.confirm_quit = true,
            _ => {}
        }
    }
    if !usable(l.title.sel) {
        l.title.sel = 1;
    }
    match chosen {
        Some(Entry::Continue) => super::drive::start_from_phone(l),
        Some(Entry::Page(p)) => {
            l.title.open = false;
            l.go(p);
        }
        Some(Entry::Quit) => l.title.confirm_quit = true,
        None => {}
    }
}

/// "Quit openOMSI?" over the title screen.
fn quit_dialog(l: &mut Launcher) {
    let size = l.ui.size;
    let full = Rect::new(0.0, 0.0, size.x, size.y);
    l.ui.p().rect(full, Color::rgba(0, 0, 0, 0.55));
    let w = 420.0f32.min(size.x - 48.0);
    let r = Rect::new((size.x - w) * 0.5, size.y * 0.5 - 80.0, w, 160.0);
    l.ui.panel(r);
    l.ui.text_in("Quit openOMSI?", Rect::new(r.x, r.y + 24.0, r.w, 30.0), 20.0, Weight::Bold, TEXT, Align::Center);
    let bw = 150.0;
    let by = r.bottom() - 62.0;
    if l.ui.button("title-quit-no", Rect::new(r.center().x - bw - 8.0, by, bw, 38.0), "Back", None, ui::ButtonKind::Normal) {
        l.title.confirm_quit = false;
    }
    if l.ui.button("title-quit-yes", Rect::new(r.center().x + 8.0, by, bw, 38.0), "Quit", None, ui::ButtonKind::Primary) {
        l.title.quit = true;
    }
}

/// On a page of the menu: Escape (nothing being typed, no list open) goes back to the title
/// screen.
pub(super) fn back_from_page(l: &mut Launcher) {
    if l.ui.focus.is_none() && !l.ui.popup_open() && l.ui.input.keys.contains(&Key::Escape) {
        l.ui.input.keys.retain(|k| *k != Key::Escape);
        l.title.show();
    }
}

// ---------------------------------------------------------------------------------------
// The loading screen: a drive is being made. It is the last picture the window shows until
// the game's own loading screen takes over (making the game holds the window meanwhile).

const TIPS: [&str; 6] = [
    "Escape opens the game menu: options, the city map, the line and tour, the main menu.",
    "Kneel the bus at the stops: passengers with prams and wheelchairs get on more easily.",
    "Keep to the timetable: the profile counts how early or late you were at every stop.",
    "The city map shows where you are and the stops of your line.",
    "Settings → Driving: an automatic clutch and gearbox for a first drive.",
    "In multiplayer, the code of your session lets friends join your game.",
];

pub(super) fn draw_loading(l: &mut Launcher) {
    let size = l.ui.size;
    let full = Rect::new(0.0, 0.0, size.x, size.y);
    l.ui.solid(full);
    l.ui.p().rect(full, Color::rgba(8, 9, 11, 1.0));
    if !l.state.choice.bus.is_empty() {
        l.preview_rect = Some(full);
        if let (Some(tex), true) = (l.preview_tex, l.showroom.has_picture()) {
            l.ui.image_tinted(full, tex, 0.0, Color::rgba(70, 70, 75, 1.0));
        }
    }
    let (bottom, _) = full.cut_bottom(size.y * 0.5);
    l.ui.p().gradient(bottom, Color::rgba(0, 0, 0, 0.0), Color::rgba(0, 0, 0, 0.8));
    let x = 64.0;
    let y = size.y - 230.0;
    l.ui.text_in("Loading", Rect::new(x, y, size.x * 0.6, 22.0), 14.0, Weight::Bold, ACCENT, Align::Left);
    let map = l.state.map().map(|m| m.name.clone()).unwrap_or_default();
    l.ui.text_in(&map, Rect::new(x, y + 24.0, size.x - 2.0 * x, 60.0), 46.0, Weight::Black, TEXT, Align::Left);
    let mut sub = l.state.bus().map(|b| b.name.clone()).unwrap_or_default();
    let c = &l.state.choice;
    if let (Some(line), Some(tour), false) = (c.line.as_ref(), c.tour.as_ref(), c.free) {
        sub.push_str(&format!(" · {} {line} / {tour}", omsi_ui::tr("Line")));
    }
    l.ui.text_in(&sub, Rect::new(x, y + 86.0, size.x - 2.0 * x, 24.0), 16.0, Weight::Medium, TEXT_SOFT, Align::Left);
    // a bar that runs while the drive is made (the window stands still for a while then:
    // this picture is the one that stays)
    let bar = Rect::new(x, y + 126.0, (size.x - 2.0 * x).min(520.0), 3.0);
    l.ui.p().rect(bar, Color::rgba(255, 255, 255, 0.12));
    let run = (l.ui.time * 0.6).fract();
    let seg = bar.w * 0.28;
    let sx = bar.x + (bar.w + seg) * run - seg;
    let (a, b) = (sx.max(bar.x), (sx + seg).min(bar.right()));
    if b > a {
        l.ui.p().rect(Rect::new(a, bar.y, b - a, bar.h), ACCENT);
    }
    // a tip, one per drive
    let tip = TIPS[(l.state.choice.time.unsigned_abs() as usize + map.len()) % TIPS.len()];
    l.ui.text_in(tip, Rect::new(x, size.y - 64.0, size.x - 2.0 * x, 22.0), 13.0, Weight::Regular, TEXT_DIM, Align::Left);
    l.ui.text_in(crate::startup::VERSION, Rect::new(size.x - 260.0, size.y - 34.0, 236.0, 20.0), 12.0, Weight::Regular, TEXT_DIM, Align::Right);
}

// ---------------------------------------------------------------------------------------
// The pages, as screens of the game's menu: the same picture behind them as the title
// screen, a header with the way back, the page's name and the other pages, the keys along
// the bottom; the page itself in the middle, as it is.

/// Height of the header and of the band along the bottom.
pub(super) const HEADER_H: f32 = 112.0;
pub(super) const FOOTER_H: f32 = 52.0;

/// The chosen bus behind a page, darker than behind the title screen.
pub(super) fn page_background(l: &mut Launcher) {
    let size = l.ui.size;
    let full = Rect::new(0.0, 0.0, size.x, size.y);
    l.ui.p().rect(full, Color::rgba(8, 9, 11, 1.0));
    if !l.state.choice.bus.is_empty() {
        l.preview_rect = Some(full);
        if let (Some(tex), true) = (l.preview_tex, l.showroom.has_picture()) {
            l.ui.image_tinted(full, tex, 0.0, Color::rgba(62, 62, 66, 1.0));
        }
    }
    l.ui.p().rect(full, Color::rgba(0, 0, 0, 0.25));
}

/// Where a page is laid out: between the header and the keys, no wider than reads well.
pub(super) fn page_content(sx: f32, sy: f32) -> Rect {
    let w = (sx - 96.0).min(1640.0);
    Rect::new((sx - w) * 0.5, HEADER_H + 8.0, w, (sy - HEADER_H - FOOTER_H - 16.0).max(0.0))
}

/// The header and the band along the bottom, over the page; the page's keys (Q / E: the
/// page before / after).
pub(super) fn page_chrome(l: &mut Launcher) {
    let size = l.ui.size;
    // --- header: a dark band fading out, the way back, the page's name, the pages
    let (head, _) = Rect::new(0.0, 0.0, size.x, size.y).cut_top(HEADER_H + 24.0);
    l.ui.solid(Rect::new(0.0, 0.0, size.x, HEADER_H));
    l.ui.p().gradient(head, Color::rgba(0, 0, 0, 0.82), Color::rgba(0, 0, 0, 0.0));
    let back = Rect::new(36.0, 20.0, 230.0, 30.0);
    let (hover, _, clicked) = l.ui.interact(ui::id_of("page-back"), back);
    if clicked {
        l.title.show();
    }
    let c = if hover { TEXT } else { TEXT_SOFT };
    l.ui.text_in("‹", Rect::new(back.x, back.y - 2.0, 16.0, back.h), 22.0, Weight::Bold, c, Align::Left);
    l.ui.text_in("Main menu", Rect::new(back.x + 20.0, back.y, 120.0, back.h), 14.0, Weight::Medium, c, Align::Left);
    let cap = Rect::new(back.x + 146.0, back.y + 4.0, 38.0, 22.0);
    l.ui.p().rounded_border(cap, 3.0, 1.0, TEXT_DIM);
    l.ui.text_in("Esc", cap, 11.0, Weight::Bold, TEXT_SOFT, Align::Center);
    let name = super::PAGES.iter().find(|p| p.0 == l.page).map(|p| p.1).unwrap_or("");
    let title = omsi_ui::tr(name).to_uppercase();
    l.ui.text_in(&title, Rect::new(36.0, 52.0, size.x * 0.4, 48.0), 38.0, Weight::Black, TEXT, Align::Left);
    // the pages, right of the name; the open one underlined
    let n = super::PAGES.len();
    let tab_w = ((size.x * 0.58) / n as f32).clamp(88.0, 150.0);
    let x0 = size.x - 36.0 - tab_w * n as f32;
    for (i, (p, label, _)) in super::PAGES.iter().enumerate() {
        let r = Rect::new(x0 + i as f32 * tab_w, 58.0, tab_w, 40.0);
        let (h, _, clicked) = l.ui.interact(ui::id_of(&format!("page-tab-{label}")), r);
        if clicked {
            l.go(*p);
        }
        let open = l.page == *p;
        let c = if open { TEXT } else if h { TEXT_SOFT } else { TEXT_DIM };
        l.ui.text_in(label, r, 13.5, if open { Weight::Bold } else { Weight::Medium }, c, Align::Center);
        if open {
            l.ui.p().rect(Rect::new(r.x + 18.0, r.bottom() - 4.0, r.w - 36.0, 3.0), ACCENT);
        }
    }
    l.ui.p().rect(Rect::new(36.0, HEADER_H - 2.0, size.x - 72.0, 1.0), Color::rgba(255, 255, 255, 0.08));

    // --- the band along the bottom: what happened last, the keys, the version
    let (foot, _) = Rect::new(0.0, 0.0, size.x, size.y).cut_bottom(FOOTER_H + 20.0);
    l.ui.solid(Rect::new(0.0, size.y - FOOTER_H, size.x, FOOTER_H));
    l.ui.p().gradient(foot, Color::rgba(0, 0, 0, 0.0), Color::rgba(0, 0, 0, 0.8));
    let y = size.y - FOOTER_H + 14.0;
    let (text, err, at) = l.state.status.clone();
    let fade = if err { 1.0 } else { (1.0 - (at.elapsed().as_secs_f32() - 6.0) / 1.5).clamp(0.0, 1.0) };
    if !text.is_empty() && fade > 0.0 {
        let first = text.lines().next().unwrap_or("").to_string();
        let r = Rect::new(36.0, y, size.x * 0.45, 24.0);
        l.ui.text_in(&first, r, 13.0, Weight::Regular, (if err { DANGER } else { TEXT_SOFT }).alpha(fade), Align::Left);
        l.ui.tooltip(r, &text);
    }
    let keys = [("Esc", "Main menu"), ("Q  E", "Pages")];
    let kw = 190.0;
    let kx0 = size.x * 0.5 + 40.0;
    for (i, (k, what)) in keys.iter().enumerate() {
        let cap = Rect::new(kx0 + i as f32 * kw, y, 44.0, 22.0);
        l.ui.p().rounded_border(cap, 3.0, 1.0, TEXT_DIM);
        l.ui.text_in(k, cap, 11.0, Weight::Bold, TEXT_SOFT, Align::Center);
        l.ui.text_in(what, Rect::new(cap.right() + 8.0, cap.y, kw - 60.0, cap.h), 13.0, Weight::Regular, TEXT_SOFT, Align::Left);
    }
    l.ui.text_in(crate::startup::VERSION, Rect::new(size.x - 260.0, y, 224.0, 22.0), 12.0, Weight::Regular, TEXT_DIM, Align::Right);

    // --- Q / E: the page before / after (nothing typed; a key a page took is gone already)
    if l.ui.focus.is_none() && !l.ui.popup_open() {
        let step = match l.ui.input.raw_key {
            Some(winit::keyboard::KeyCode::KeyQ) | Some(winit::keyboard::KeyCode::PageUp) => Some(n - 1),
            Some(winit::keyboard::KeyCode::KeyE) | Some(winit::keyboard::KeyCode::PageDown) => Some(1),
            _ => None,
        };
        if let Some(d) = step {
            let i = super::PAGES.iter().position(|p| p.0 == l.page).unwrap_or(0);
            l.go(super::PAGES[(i + d) % n].0);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_entry_says_what_it_does() {
        for (_, label, hint) in super::ENTRIES {
            assert!(!label.is_empty() && hint.ends_with('.'), "{label}");
        }
    }
}
