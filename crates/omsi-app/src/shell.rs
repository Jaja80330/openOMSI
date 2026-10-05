//! Big Picture: one window for the launcher and the game on a computer (Settings → Graphics
//! → "Big Picture", Windows): the launcher, full screen, is the game's menu; a duty starts in
//! the same window and the same process, and ending it (the game menu's Quit, Escape) comes
//! back to the menu instead of ending the program - what a phone does (see `android.rs`).
//! Closing the window (Alt+F4) still ends the program, from the menu or from a drive.

use super::*;

/// The setting, read before anything else (the log goes to a file in this mode, and that is
/// decided before the first line is written).
pub(crate) fn wanted() -> bool {
    if !cfg!(windows) {
        return false;
    }
    // only a start that opens the launcher: a double click, or `--launcher`
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !(args.is_empty() || args.iter().all(|a| a == "--launcher")) {
        return false;
    }
    if let Some(v) = omsi_cfg::env::var_os("OMSI_BIG_PICTURE") {
        return v != "0";
    }
    std::fs::read_to_string(omsi_launcher_lib::data_dir().join("settings.cfg"))
        .map(|t| setting_on(&t))
        .unwrap_or(false)
}

/// `big_picture=1` in a settings file (the last line of it wins, as in the game).
fn setting_on(text: &str) -> bool {
    text.lines()
        .filter_map(|l| l.trim().split_once('='))
        .filter(|(k, _)| k.trim().eq_ignore_ascii_case("big_picture"))
        .last()
        .map(|(_, v)| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "on" | "yes"))
        .unwrap_or(false)
}

/// Where the log goes: one program, so the game's lines and the launcher's are in one file,
/// `game.log` beside the launcher's, the previous run's kept as `game-prev.log` (what the
/// launcher reads to tell a crash, and a phone does the same).
pub(crate) fn log_file() -> Option<std::fs::File> {
    let dir = omsi_launcher_lib::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let log = dir.join("game.log");
    let _ = std::fs::rename(&log, dir.join("game-prev.log"));
    std::fs::File::create(log).ok()
}

/// The launcher, or the game in the launcher's window.
struct Shell {
    launcher: Box<launcher::Launcher>,
    game: Option<Box<App>>,
    /// The window was closed during a drive: once the session is written, the program ends.
    quit: bool,
    /// A drive asked for: it starts once the menu's loading screen is on the screen (making
    /// the game holds the window for seconds; the loading screen stays up meanwhile).
    pending: Option<Vec<String>>,
}

/// Run the launcher as the game's menu until the window is closed.
pub(crate) fn run(instance: wgpu::Instance) -> Result<()> {
    crate::platform::set_big_picture();
    omsi_launcher_lib::set_in_process_games(true);
    // (the window shows a loading screen while a drive is made or written: Windows' "not
    // responding" ghost over it, and its offer to close the program, would only be wrong)
    crate::platform::no_ghosting();
    log::info!("one window: the launcher is the game's menu, the drives play in it");
    let event_loop = EventLoop::new()?;
    // SIGTERM and Ctrl+C end a drive the way Escape does (see quit.rs)
    let proxy = event_loop.create_proxy();
    quit::install(move |_| {
        let _ = proxy.send_event(());
    });
    let mut shell = Shell { launcher: Box::new(launcher::Launcher::new(instance)), game: None, quit: false, pending: None };
    let r = event_loop.run_app(&mut shell);
    lan_mods::clean_up();
    r?;
    Ok(())
}

impl Shell {
    /// After every event: a drive the menu asked for starts, a drive that ended gives the
    /// window back to the menu.
    fn switch(&mut self, event_loop: &ActiveEventLoop) {
        if self.game.is_some() {
            if !crate::platform::take_leave() {
                return;
            }
            // (the game menu's "Quit openOMSI": not back to the menu, the program ends)
            if crate::platform::take_quit_all() {
                self.quit = true;
            }
            let mut game = self.game.take().unwrap();
            // the session is written and the world let go: a loading screen meanwhile, not the
            // last picture of the drive standing still
            game.still_frame(&omsi_ui::tr(if self.quit { "Saving…" } else { "Back to the main menu" }));
            game.exiting(event_loop);
            let window = game.window.take();
            drop(game);
            lan_mods::clean_up();
            if self.quit {
                log::info!("session ended and the window closed: the program ends");
                event_loop.exit();
                return;
            }
            log::info!("session ended: back to the menu");
            if let Some(w) = window {
                self.launcher.adopt_window(w);
            }
            self.launcher.resumed(event_loop);
            return;
        }
        // a drive asked for: the loading screen first, the drive once it is on the screen
        if let Some(line) = omsi_launcher_lib::take_in_process_launch() {
            self.launcher.begin_loading();
            self.pending = Some(line);
            return;
        }
        if self.pending.is_none() || !self.launcher.loading_shown() {
            return;
        }
        let line = self.pending.take().unwrap();
        log::info!("starting the game: {}", line.join(" "));
        let argv: Vec<String> = std::iter::once("openomsi".to_string()).chain(line).collect();
        let args = match Args::try_parse_from(&argv) {
            Ok(a) => a,
            Err(e) => {
                log::error!("the launcher's command line: {e}");
                self.launcher.end_loading(Some(format!("The game could not start: {e}")));
                return;
            }
        };
        let game = prepare(args, false).and_then(|p| match p {
            Some((args, server)) => make_app(args, server),
            None => Ok(None),
        });
        let mut app = match game {
            Ok(Some(app)) => app,
            Ok(None) => {
                self.launcher.end_loading(None);
                return;
            }
            Err(e) => {
                log::error!("the game could not start: {e:#}");
                self.launcher.end_loading(Some(format!("The game could not start: {e:#}")));
                return;
            }
        };
        self.launcher.end_loading(None);
        let window = self.launcher.release_window();
        app.create_window(event_loop, window);
        self.game = Some(Box::new(app));
    }
}

impl ApplicationHandler for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        match self.game.as_mut() {
            Some(g) => g.resumed(event_loop),
            None => self.launcher.resumed(event_loop),
        }
        self.switch(event_loop);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        match self.game.as_mut() {
            Some(g) => g.suspended(event_loop),
            None => self.launcher.suspended(event_loop),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match self.game.as_mut() {
            Some(g) => {
                // (the window closed during a drive: the program ends, not only the drive)
                if matches!(event, WindowEvent::CloseRequested) {
                    self.quit = true;
                }
                g.window_event(event_loop, id, event)
            }
            None => self.launcher.window_event(event_loop, id, event),
        }
        self.switch(event_loop);
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, id: winit::event::DeviceId, event: DeviceEvent) {
        match self.game.as_mut() {
            Some(g) => g.device_event(event_loop, id, event),
            None => self.launcher.device_event(event_loop, id, event),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ()) {
        // a quit signal: the drive ends as Escape ends it, then the program; in the menu, at once
        match self.game.as_mut() {
            Some(g) => {
                self.quit = true;
                g.user_event(event_loop, event);
            }
            None => event_loop.exit(),
        }
        self.switch(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match self.game.as_mut() {
            Some(g) => {
                event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
                g.about_to_wait(event_loop)
            }
            None => self.launcher.about_to_wait(event_loop),
        }
        self.switch(event_loop);
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(g) = self.game.as_mut() {
            g.exiting(event_loop);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_setting_is_read_as_the_game_reads_its_settings() {
        assert!(super::setting_on("msaa=4\nbig_picture=1\n"));
        assert!(super::setting_on("big_picture = true"));
        assert!(!super::setting_on("msaa=4\n"));
        // the last line wins
        assert!(!super::setting_on("big_picture=1\nbig_picture=0\n"));
    }
}
