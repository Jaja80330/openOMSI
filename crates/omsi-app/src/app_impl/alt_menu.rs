//! The quick menu (OMSI 2's Alt menu, see `shell::alt_menu`): Alt alone opens and closes
//! it, its tiles do what the game menu's lines and the vehicle and world pages do.

use super::*;
use crate::shell::alt_menu::{step, AltView, COLS, TILES};

impl App {
    /// Whether each tile of the quick menu can be used now.
    pub(crate) fn alt_enabled(&self) -> [bool; 15] {
        let bus = self.player.is_some();
        let client = self.net.lan.as_ref().is_some_and(|l| l.role == omsi_net::Role::Client);
        let server = on_server(&self.args);
        let mut out = [false; 15];
        for (k, &(id, ..)) in TILES.iter().enumerate() {
            out[k] = match id {
                "place" | "arrows" | "world" | "controls" => true,
                "switch" => bus || !self.session.placed.is_empty(),
                "remove" | "swap" | "dest" | "refuel" | "wash" | "repair" | "reset" => bus,
                "teleport" => bus && !server && self.menus.navigator.is_some(),
                // (on a server the duty is taken with the button beside the radio's)
                "duty" => bus && !client,
                "driver" => !server,
                _ => false,
            };
        }
        out
    }

    /// Alt pressed or let go alone, or another key while it is held (`on_key`): Alt pressed
    /// and let go with nothing between opens or closes the quick menu - Alt+F4, Alt+Enter
    /// and the scripts' Alt combinations stay as they are.
    pub(crate) fn alt_tap_key(&mut self, code: KeyCode, pressed: bool, repeat: bool) {
        let alt = matches!(code, KeyCode::AltLeft | KeyCode::AltRight);
        if !alt {
            if pressed {
                self.input.alt_tap = false;
            }
            return;
        }
        if pressed {
            if !repeat {
                self.input.alt_tap = self.menus.game_menu.is_none() && self.photo.is_none() && self.welcome.is_none();
            }
            return;
        }
        if std::mem::take(&mut self.input.alt_tap) {
            if self.menus.alt_menu.is_some() {
                self.menus.alt_menu = None;
            } else {
                self.release_vehicle_keys();
                self.menus.alt_menu = Some(self.alt_enabled().iter().position(|e| *e).unwrap_or(0));
                self.menus.menu_kbd = false;
            }
            // (the mouse steering lets the cursor go while it is open, and takes it back)
            self.sync_look_hold();
        }
    }

    /// A key while the quick menu is open: the arrows go from tile to tile, Enter does the
    /// tile, Escape closes it. True when the key was the menu's.
    pub(crate) fn alt_menu_key(&mut self, code: KeyCode, pressed: bool) -> bool {
        let Some(sel) = self.menus.alt_menu else { return false };
        if matches!(code, KeyCode::AltLeft | KeyCode::AltRight) {
            return false;
        }
        if !pressed {
            return true;
        }
        let en = self.alt_enabled();
        let by: isize = match code {
            KeyCode::ArrowLeft => -1,
            KeyCode::ArrowRight => 1,
            KeyCode::ArrowUp => -(COLS as isize),
            KeyCode::ArrowDown => COLS as isize,
            KeyCode::Escape => {
                self.menus.alt_menu = None;
                self.sync_look_hold();
                return true;
            }
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => {
                if en[sel] {
                    self.alt_do(sel);
                }
                return true;
            }
            // (the other keys are nobody's while it is open)
            _ => return true,
        };
        self.menus.alt_menu = Some(step(sel, &en, by));
        self.menus.menu_kbd = true;
        true
    }

    /// The quick menu over the picture (see `frame_shell`).
    pub(crate) fn alt_view(&self) -> Option<AltView> {
        let sel = self.menus.alt_menu?;
        let arrows_on = self.menus.navigator.as_ref().map_or(self.settings.nav_arrows, |n| n.arrows);
        Some(AltView { enabled: self.alt_enabled(), sel, kbd: self.menus.menu_kbd, arrows_on })
    }

    /// Tile `k` of the quick menu chosen (usize::MAX: a click beside it, which closes it).
    pub(crate) fn alt_do(&mut self, k: usize) {
        self.menus.alt_menu = None;
        self.sync_look_hold();
        let Some(&(id, ..)) = TILES.get(k) else { return };
        if !self.alt_enabled()[k] {
            return;
        }
        use crate::game_lists::ListKind;
        match id {
            "arrows" => {
                crate::game_lists::option_do(self, "nav_arrows", "", crate::game_lists::Move::Next);
                let on = self.menus.navigator.as_ref().map_or(self.settings.nav_arrows, |n| n.arrows);
                self.service_msg = Some((omsi_ui::tr(if on { "Route helpers shown" } else { "Route helpers hidden" }).into_owned(), 2.0));
            }
            // the lists open in the game menu, as from its lines
            "duty" => {
                if !self.self_duty_refused() {
                    self.open_game_menu();
                    self.open_list(ListKind::Lines);
                }
            }
            "world" | "controls" | "driver" | "dest" => {
                self.open_game_menu();
                self.open_list(match id {
                    "world" => ListKind::World(0),
                    "controls" => ListKind::Controls,
                    "driver" => ListKind::Drivers,
                    _ => ListKind::Destinations,
                });
            }
            // the vehicle page's buttons (what closes the menu closes it again)
            _ => {
                self.open_game_menu();
                self.page_action(id);
            }
        }
    }
}
