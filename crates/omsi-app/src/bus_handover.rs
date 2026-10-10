//! A bus nobody drives taken over in a session: another player's bus whose driver has got
//! up (walking about, or sitting in it as a passenger) is driven by whoever comes to its
//! driver's place and presses G. Its driver's game is asked (`take-bus?`, passed on by the
//! host); while they are not at the wheel it gives the bus up (`take-bus ok`) and its
//! player stays on foot, and the one who asked drives the same bus - the same files, paint
//! and depot file, where it stood, every script variable as it was (the session carries
//! them all). A bus with its driver at the wheel is refused (`take-bus no driven`): it is
//! got into as a passenger, as before.
//!
//! A game drives one bus of its own at a time: a player with a bus of their own takes no
//! other (they remove theirs first, from the menu).

use crate::app::App;
use glam::DVec3;
use std::path::PathBuf;

/// How long the bus's driver's game is waited for (s).
const ASK_WAIT: f32 = 8.0;

/// How near the driver's place a walker in the bus stands to take the wheel (m), and how
/// near its driver's door one outside it.
const STAND_REACH: f64 = 1.8;
const DOOR_REACH: f64 = 3.2;

/// A bus asked for: its player, the seconds left to wait, and the bus as it was last seen
/// (its pose goes from the session the moment its driver's game gives it up).
pub(crate) struct BusAsk {
    pub(crate) owner: u32,
    pub(crate) left: f32,
    pub(crate) seen: Option<TakeOver>,
}

/// What the bus taken over is made from.
#[derive(Clone)]
pub(crate) struct TakeOver {
    pub(crate) file: PathBuf,
    pub(crate) paint: String,
    pub(crate) hof: Option<String>,
    pub(crate) at: DVec3,
    pub(crate) heading: f64,
    /// Every script variable and string as the session carried them (None: the vehicle
    /// files differ from theirs - only the pose's state came - and the bus starts as placed).
    pub(crate) vars: Option<(Vec<f32>, Vec<String>)>,
    pub(crate) name: String,
}

/// A player's bus stands without its driver at the wheel: they are out of the seat.
pub(crate) fn driverless(pose: &omsi_net::Pose) -> bool {
    pose.has_vehicle() && pose.walker.is_some()
}

impl App {
    /// The player whose bus's driver's place is within reach of the walker at `pos`: in the
    /// bus by the driver's place, or outside by its driver's door.
    pub(crate) fn remote_cab_near(&mut self, pos: DVec3, inside_of: Option<u32>) -> Option<u32> {
        let h = self.session.humans.as_mut()?;
        let mut best: Option<(u32, f64)> = None;
        for (&id, rv) in self.net.remotes.remotes.iter() {
            let v = rv.vehicle();
            let d = if inside_of == Some(id) {
                h.driver_stand(v).and_then(|l| h.vehicle_cabin_world(v, l)).map(|s| (s - pos).truncate().length()).filter(|d| *d < STAND_REACH)
            } else if inside_of.is_none() {
                h.vehicle_driver_door(v).map(|s| (s - pos).truncate().length()).filter(|d| *d < DOOR_REACH)
            } else {
                None
            };
            if let Some(d) = d {
                if best.is_none_or(|b| d < b.1) {
                    best = Some((id, d));
                }
            }
        }
        best.map(|b| b.0)
    }

    /// G at the driver's place of player `owner`'s bus: their game is asked for it, unless
    /// somebody drives it (then the walker gets in as a passenger, by the seats).
    pub(crate) fn ask_for_bus(&mut self, owner: u32) -> bool {
        let Some(rv) = self.net.remotes.remotes.get(&owner) else { return false };
        if !driverless(&rv.last) {
            self.service_msg = Some(("Somebody drives this bus: you may ride in it as a passenger".into(), 4.0));
            return false;
        }
        if self.player.is_some() {
            self.service_msg = Some(("You have a bus of your own: remove it (menu: Vehicle) to take another".into(), 5.0));
            return true;
        }
        if self.menus.pending_placement.is_some() || self.bus_ask.is_some() {
            self.service_msg = Some(("A bus is being taken over already".into(), 3.0));
            return true;
        }
        let seen = self.bus_seen(owner);
        let Some(lan) = self.net.lan.as_mut() else { return false };
        lan.command(owner, "take-bus?");
        self.bus_ask = Some(BusAsk { owner, left: ASK_WAIT, seen });
        self.service_msg = Some(("Taking the wheel...".into(), ASK_WAIT));
        true
    }

    /// Player `owner`'s bus as it stands here now (see `TakeOver`).
    fn bus_seen(&self, owner: u32) -> Option<TakeOver> {
        let rv = self.net.remotes.remotes.get(&owner)?;
        let file = crate::lan::remote_bus_file(&self.args, &rv.last.bus).ok()?;
        let v = rv.vehicle();
        Some(TakeOver {
            file,
            paint: rv.last.paint.clone(),
            hof: rv.hof_name(),
            at: v.position,
            heading: v.heading,
            vars: rv.all_vars().then(|| (v.state.vars.clone(), v.state.str_vars.clone())),
            name: format!("{} {}", v.ty.def.manufacturer, v.ty.def.type_name).trim().to_string(),
        })
    }

    /// Once a frame: the bus asked for kept as it is seen, the wait counted.
    pub(crate) fn tick_bus_ask(&mut self, dt: f32) {
        let Some(owner) = self.bus_ask.as_ref().map(|a| a.owner) else { return };
        if let Some(seen) = self.bus_seen(owner) {
            if let Some(a) = self.bus_ask.as_mut() {
                a.seen = Some(seen);
            }
        }
        let a = self.bus_ask.as_mut().unwrap();
        a.left -= dt;
        if a.left <= 0.0 || self.net.lan.is_none() {
            self.bus_ask = None;
            self.service_msg = Some(("The bus's driver's game did not answer".into(), 4.0));
        }
    }

    /// A command about the players' buses another game sent ours: true when it was one.
    pub(crate) fn bus_handover_command(&mut self, from: u32, text: &str) -> bool {
        match text.trim() {
            "take-bus?" => {
                self.give_up_bus(from);
                true
            }
            "take-bus ok" => {
                if let Some(a) = self.bus_ask.take().filter(|a| a.owner == from) {
                    match a.seen {
                        Some(t) => self.take_over(t),
                        None => self.service_msg = Some(("The bus could not be made here".into(), 4.0)),
                    }
                }
                true
            }
            t if t.starts_with("take-bus no") => {
                if self.bus_ask.as_ref().is_some_and(|a| a.owner == from) {
                    self.bus_ask = None;
                    let msg = match t.trim_start_matches("take-bus no").trim() {
                        "driven" => "Somebody has just taken the wheel of this bus",
                        _ => "This bus is no longer there",
                    };
                    self.service_msg = Some((msg.into(), 4.0));
                }
                true
            }
            _ => false,
        }
    }

    /// Player `to` asks for our bus: given up when we are not at its wheel (we stay on foot
    /// where we were, or by its driver's door when we were in it), refused otherwise.
    fn give_up_bus(&mut self, to: u32) {
        let answer = if self.player.is_none() {
            "take-bus no gone"
        } else if self.session.on_foot.is_none() {
            "take-bus no driven"
        } else {
            "take-bus ok"
        };
        if let Some(l) = self.net.lan.as_mut() {
            l.command(to, answer);
        }
        if answer != "take-bus ok" {
            return;
        }
        let who = self.net.remotes.remotes.get(&to).map(|r| r.name.clone()).unwrap_or_else(|| format!("{} {to}", omsi_ui::tr("Player")));
        // (where the walker was, when it was outside the bus: it stays there)
        let outside = self.session.on_foot.as_ref().filter(|f| f.inside.is_none() && f.seat.is_none() && f.transit.is_none()).map(|f| (f.pos, f.heading));
        // the duty went with the bus
        if self.session.duty.is_some() {
            crate::game_lists::leave_duty(self);
        }
        self.remove_driven_vehicle();
        if let (Some((pos, heading)), Some(f)) = (outside, self.session.on_foot.as_mut()) {
            f.pos = pos;
            f.heading = heading;
        }
        log::info!("LAN: our bus given up to player {to} ({who})");
        self.service_msg = Some((format!("{who} {}", omsi_ui::tr("has taken the wheel of your bus: you are on foot")), 6.0));
    }

    /// The bus given up to us: made here where it stood, as it was, and driven.
    fn take_over(&mut self, t: TakeOver) {
        let (Some(w), Some(r)) = (self.world.clone(), self.renderer.as_ref()) else { return };
        let one = crate::Args {
            bus: Some(t.file.to_string_lossy().into_owned()),
            spawn: Some(format!("{},{},{}", t.at.x, t.at.y, t.heading)),
            situation_vars: Vec::new(),
            situation_strvars: Vec::new(),
            situation_odometer_km: None,
            situation_others: Vec::new(),
            line: None,
            tour: None,
            trip: None,
            autostart: false,
            paint: Some(t.paint.clone()).filter(|p| !p.is_empty()),
            hof: t.hof.clone().or(self.args.hof.clone()),
            ..self.args.clone()
        };
        let prefetch = w.placement_prefetch(r);
        let worker_args = one.clone();
        let (tx, receiver) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("bus-takeover".into()).spawn(move || {
            let result = crate::spawn::PreparedPlayer::load(&worker_args).map(|mut prepared| {
                prepared.prefetch(prefetch, worker_args.paint.as_deref());
                prepared
            });
            let _ = tx.send(result);
        });
        if spawned.is_err() {
            self.service_msg = Some(("The bus could not be made here".into(), 4.0));
            return;
        }
        log::info!("LAN: taking over {} at ({:.1}, {:.1})", t.name, t.at.x, t.at.y);
        self.menus.pending_placement = Some(crate::spawn::PendingPlacement {
            receiver,
            world: w,
            args: one,
            name: t.name.clone(),
            replace: None,
            reload: false,
            heading: t.heading,
            takeover: Some(Box::new(t)),
        });
    }

    /// The bus taken over is made (see `poll_vehicle_placement`): its state as it was, and
    /// the player at its wheel.
    pub(crate) fn drive_taken_over(&mut self, mut q: crate::player::Player, t: &TakeOver) {
        if let Some((vars, strs)) = t.vars.as_ref() {
            let s = &mut q.vehicle.state;
            if s.vars.len() == vars.len() && s.str_vars.len() == strs.len() {
                s.vars.clone_from(vars);
                s.str_vars.clone_from(strs);
            }
        }
        q.vehicle.heading = t.heading;
        self.session.placed.push(q);
        let k = self.session.placed.len() - 1;
        self.take_placed(k);
        self.service_msg = Some((format!("{}: {}", omsi_ui::tr("You have taken the wheel"), t.name), 4.0));
    }
}

#[cfg(test)]
mod tests {
    use super::driverless;

    #[test]
    fn a_bus_is_driverless_while_its_player_is_out_of_the_seat() {
        let mut p = omsi_net::Pose { bus: "Vehicles/A/a.bus".into(), flags: omsi_net::FLAG_VEHICLE, ..Default::default() };
        // at the wheel: no walker
        assert!(!driverless(&p));
        // walking about, or sitting in it as a passenger
        p.walker = Some(omsi_net::Walker::default());
        assert!(driverless(&p));
        // a player without a bus has none to give
        p.bus.clear();
        assert!(!driverless(&p));
    }
}
