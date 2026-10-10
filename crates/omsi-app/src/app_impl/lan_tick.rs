//! LAN play once a frame, and what the host's world and the other players' games ask of ours.

use super::*;

impl App {
    /// LAN play, once a frame: send our bus, take in the others', and keep a drawn
    /// vehicle for each of them.
    pub(crate) fn tick_lan(&mut self, dt: f32) {
        let walker = self.walker_pose();
        let radio_keyed = self.voice_radio_held();
        let Some(lan) = self.net.lan.as_mut() else {
            // (the session is over: the plugin is told so; its messages go with it)
            self.sound.voice = None;
            self.inbox.clear();
            self.inbox_unread = 0;
            return;
        };
        let duty = self
            .session.duty
            .as_ref()
            .map(|d| &d.trips[d.trip_index])
            .map(|t| (t.line.as_str(), t.terminus.as_str()));
        let frame = lan::Frame {
            audio: self.sound.audio.as_ref(),
            listener: self.camera.as_ref().map(|c| c.position),
            muffled: self.cam.in_cab || self.net.inside_remote.is_some(),
            riders: self.session.humans.as_ref().map(|h| h.riding()).unwrap_or(0),
            clock: Some(&self.clock),
            tour: self.session.duty.as_ref().map(|d| format!("{}/{}", d.line, d.tour)),
            progress: self.session.duty.as_ref().map(|d| crate::schedule::duty_progress(d, self.clock.time)),
            walker,
            inside_of: self.net.inside_remote,
            radio_keyed,
        };
        let updates = lan::tick(
            lan,
            &mut self.net.remotes,
            dt,
            &self.args,
            self.player.as_mut(),
            self.world.as_deref(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
            self.session.traffic.as_mut(),
            self.session.humans.as_mut(),
            &mut self.gfx.sim_view,
            duty,
            &frame,
        );
        for u in updates {
            self.apply_world_update(u);
        }
        let cmds = self.net.lan.as_mut().map(|l| l.take_commands()).unwrap_or_default();
        for (from, text) in cmds {
            self.lan_command(from, &text);
        }
        self.tick_voice(dt);
        self.tick_radio();
        self.tick_self_duty(dt);
        self.tick_rules(dt);
        // the server's welcome, once the world and the bus are there
        if self.welcome.is_none() && self.world.is_some() && self.player.is_some() {
            if let Some((text, pictures)) = crate::welcome::take() {
                self.welcome = Some(crate::welcome::Welcome::with_pictures(&text, pictures));
            }
        }
    }

    /// The dispatch radio (`phonie`), once a frame of a session.
    fn tick_radio(&mut self) {
        let (Some(lan), Some(radio)) = (self.net.lan.as_mut(), self.phonie.as_mut()) else {
            if self.net.lan.is_none() {
                self.phonie = None;
            }
            return;
        };
        // (the bus driven may have a terminal of its own: its scripts see the radio)
        if let Some(p) = self.player.as_mut() {
            radio.bus_link(&mut p.vehicle, lan);
        }
        radio.tick(lan, self.sound.audio.as_ref(), 1.0, &self.settings.phonie_microphone);
    }

    /// A command another player's game sent ours (`LanSession::command`).
    pub(crate) fn lan_command(&mut self, from: u32, text: &str) {
        let Some(lan) = self.net.lan.as_ref() else { return };
        let my_id = lan.my_id;
        if let Some(ev) = text.strip_prefix("trigger ") {
            // a switch worked by a passenger of ours: only by one who is in our bus
            let aboard = lan.peers().find(|p| p.pose.id == from).and_then(|p| p.pose.walker).and_then(|w| w.aboard).map(|a| a.owner == my_id).unwrap_or(false);
            if !aboard {
                log::info!("LAN: player {from} asked for switch {ev} of our bus from outside it: ignored");
                return;
            }
            if let Some(p) = self.player.as_mut() {
                log::info!("LAN: player {from} works {ev} in our bus");
                p.vehicle.trigger(ev.trim());
            }
            return;
        }
        // whether we may take a duty from our own menu (a dedicated server's `self_duty`)
        if let Some(arg) = text.strip_prefix("self-duty ") {
            if from == 1 && lan.role == omsi_net::Role::Client {
                let on = arg.trim() == "on";
                if self.self_duty.is_some_and(|was| was != on) {
                    let msg = if on { "You may take a duty yourself again" } else { "This server gives the duties: ask the dispatcher for one" };
                    self.service_msg = Some((msg.into(), 6.0));
                }
                self.self_duty = Some(on);
                self.self_duty_ask = 60.0;
            }
            return;
        }
        // the rules for the players' own buses (`rules spawn=on|off foot-only=on|off`): a
        // player's game hosting the session lets its friends put buses into the world (a
        // dedicated server answers by its own server.cfg)
        if text == "rules?" {
            if lan.role == omsi_net::Role::Host {
                if let Some(l) = self.net.lan.as_mut() {
                    l.command(from, "rules spawn=on foot-only=off");
                }
            }
            return;
        }
        if let Some(arg) = text.strip_prefix("rules ") {
            if from == 1 && lan.role == omsi_net::Role::Client {
                self.take_rules(arg);
            }
            return;
        }
        // the voice server of the session (`voice`): asked of the host, told by it
        if text == "voice?" {
            if lan.role == omsi_net::Role::Host {
                let answer = crate::voice::VoiceServer::command(crate::voice::hosted().as_ref());
                if let Some(l) = self.net.lan.as_mut() {
                    l.command(from, &answer);
                }
            }
            return;
        }
        if text.starts_with("voice ") {
            if from == 1 {
                if let (Some(v), Some(server)) = (self.sound.voice.as_mut(), crate::voice::VoiceServer::parse_command(text)) {
                    v.set_server(server);
                }
            }
            return;
        }
        // the dispatch radio (`phonie`): where ours stands, from the server
        if let Some(arg) = text.strip_prefix("radio ") {
            if from == 1 && lan.role == omsi_net::Role::Client {
                self.phonie.get_or_insert_with(crate::phonie::Radio::new).on_command(arg);
            }
            return;
        }
        // a plugin's message to the same plugin here (`lan.send`)
        if self.plugin_lan_message(from, text) {
            return;
        }
        crate::admin::command(self, from, text);
    }

    /// What the button of the welcome's window does when it asked something.
    pub(crate) fn welcome_act(&mut self, action: Option<crate::welcome::Action>) {
        match action {
            Some(crate::welcome::Action::LeaveDuty) => {
                if !self.duty_given {
                    crate::game_lists::leave_duty(self);
                }
            }
            None => {}
        }
    }

    /// A duty the player picks from the menu, refused when the dedicated server joined gives
    /// the duties itself (`self-duty off`): the player is told so.
    pub(crate) fn self_duty_refused(&mut self) -> bool {
        let client = self.net.lan.as_ref().is_some_and(|l| l.role == omsi_net::Role::Client);
        if client && self.self_duty == Some(false) {
            self.service_msg = Some(("This server gives the duties: ask the dispatcher for one".into(), 6.0));
            return true;
        }
        false
    }

    /// The server's rules for the players' own buses (`rules` from it): words
    /// `spawn=on|off` and `foot-only=on|off`, the others left for later games. A change is
    /// told to the player.
    fn take_rules(&mut self, arg: &str) {
        let on = |v: &str| matches!(v.trim(), "on" | "1" | "true" | "yes");
        let (mut spawn, mut foot) = (self.server_spawn, self.server_foot_only);
        for w in arg.split_whitespace() {
            match w.split_once('=') {
                Some(("spawn", v)) => spawn = Some(on(v)),
                Some(("foot-only", v)) => foot = Some(on(v)),
                _ => {}
            }
        }
        let was = self.may_spawn();
        let known = self.server_spawn.is_some() || self.server_foot_only.is_some();
        self.server_spawn = spawn;
        self.server_foot_only = foot;
        self.rules_ask = 60.0;
        if known && was != self.may_spawn() {
            let msg = if self.may_spawn() { "This server lets you put a bus of your own into the world again" } else { "This server no longer lets you put a bus of your own into the world" };
            self.service_msg = Some((msg.into(), 6.0));
        }
    }

    /// Whether the player may put a bus of their own into the world (the menu's "Place a
    /// vehicle" and "Swap"): anywhere but on a server joined, there only when it says so
    /// (`rules spawn=on`) and does not have its players on foot (`foot-only`).
    pub(crate) fn may_spawn(&self) -> bool {
        let client = self.net.lan.as_ref().is_some_and(|l| l.role == omsi_net::Role::Client);
        !client || (self.server_spawn == Some(true) && self.server_foot_only != Some(true))
    }

    /// A bus put into the world, refused when the server joined does not allow it: the
    /// player is told so.
    pub(crate) fn spawn_refused(&mut self) -> bool {
        if self.may_spawn() {
            return false;
        }
        let msg = if self.server_foot_only == Some(true) {
            "On this server you go on foot: take the wheel of a bus nobody drives (G at its driver's place)"
        } else {
            "This server does not let you put a bus of your own into the world"
        };
        self.service_msg = Some((msg.into(), 6.0));
        true
    }

    /// Ask the server joined its rules for the players' own buses now and then (the answer,
    /// or its change, may be lost on the way).
    fn tick_rules(&mut self, dt: f32) {
        let Some(lan) = self.net.lan.as_mut().filter(|l| l.role == omsi_net::Role::Client && l.connected) else {
            self.server_spawn = None;
            self.server_foot_only = None;
            self.rules_ask = 0.0;
            return;
        };
        self.rules_ask -= dt;
        if self.rules_ask <= 0.0 {
            lan.command(1, "rules?");
            let known = self.server_spawn.is_some() || self.server_foot_only.is_some();
            self.rules_ask = if known { 60.0 } else { 5.0 };
        }
    }

    /// Ask the server joined whether its players take their own duties (now and then: the
    /// answer, or its change, may be lost on the way).
    fn tick_self_duty(&mut self, dt: f32) {
        let Some(lan) = self.net.lan.as_mut().filter(|l| l.role == omsi_net::Role::Client && l.connected) else {
            self.self_duty = None;
            self.self_duty_ask = 0.0;
            return;
        };
        self.self_duty_ask -= dt;
        if self.self_duty_ask <= 0.0 {
            lan.command(1, "self-duty?");
            self.self_duty_ask = if self.self_duty.is_some() { 60.0 } else { 5.0 };
        }
    }

    /// The host's world as LAN play asks for it: its clock (set or caught up with) and its
    /// weather, for everything that keeps a clock of its own.
    pub(crate) fn apply_world_update(&mut self, u: lan::WorldUpdate) {
        match u {
            lan::WorldUpdate::Clock {
                year,
                day_of_year,
                time,
            } => {
                self.clock.year = year;
                self.clock.day_of_year = day_of_year;
                self.clock.time = time;
                if let Some(t) = self.session.traffic.as_mut() {
                    t.day_time = time;
                }
            }
            lan::WorldUpdate::Slew(s) => {
                self.clock.time = (self.clock.time + s).clamp(0.0, 86399.999);
                if let Some(t) = self.session.traffic.as_mut() {
                    t.day_time += s;
                }
            }
            lan::WorldUpdate::Weather(w) => {
                log::info!(
                    "LAN: the host's weather: {}",
                    w.as_deref().unwrap_or("the map's default")
                );
                // (coming over to it as the host does, not at a stroke - the streets stay
                // as wet as they are and dry or wet with it)
                self.change_weather(w, false, 240.0);
            }
            lan::WorldUpdate::Tours(tours) => {
                if let Some(s) = self.session.schedule.as_mut() {
                    s.set_lan_tours(tours);
                }
            }
        }
        if let Some(p) = self.player.as_mut() {
            p.vehicle.host.clock = self.clock.clone();
        }
    }
}
