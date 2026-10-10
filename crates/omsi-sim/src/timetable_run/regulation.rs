//! A dispatcher's regulation of the player's duty (turns, deadhead runs, shifted times, early
//! departures) and the driver's re-sync at a stop or a terminus (the SAE menu).

use super::*;

/// How near the stop a bus must stand for the driver to re-sync the duty there (m): a bus
/// stops some way along its stop's object.
pub const RESYNC_REACH: f64 = 60.0;

/// How much later than another tour's trip back the duty's own may leave a stop where the
/// dispatcher turns the bus, for the turn to take the duty's (s).
const TURN_WAIT: f64 = 15.0 * 60.0;

/// A dispatcher's regulation of a player's duty (`regul …` from the server, see the game's
/// `admin`): stops by the timetable's numbers in the trip under way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Regulation {
    /// A commercial turn: the trip ends at `stop`, the next trip begins there (its stop of
    /// the same name), the time there its departure.
    Turn { stop: usize },
    /// A turn at `stop`, then a deadhead run to the next trip's stop `resume`.
    TurnEmpty { stop: usize, resume: usize },
    /// The trip given up: a deadhead run to the first stop of the trip `trips` on.
    Deadhead { trips: usize },
    /// The times from `stop` on, `secs` later (earlier: negative).
    Shift { stop: usize, secs: f64 },
    /// The trip about to leave its terminus leaves `secs` earlier.
    EarlyDeparture { secs: f64 },
}

impl Regulation {
    /// `turn <stop>`, `turn-empty <stop> <resume>`, `deadhead [<trips>]`, `shift <stop>
    /// <seconds>`, `early <seconds>`.
    pub fn parse(arg: &str) -> Option<Regulation> {
        let mut w = arg.split_whitespace();
        let op = w.next()?;
        let mut num = || w.next().and_then(|x| x.parse::<f64>().ok()).filter(|x| x.is_finite());
        Some(match op {
            "turn" => Regulation::Turn { stop: num()? as usize },
            "turn-empty" => Regulation::TurnEmpty { stop: num()? as usize, resume: num()? as usize },
            "deadhead" => Regulation::Deadhead { trips: num().map(|n| n as usize).unwrap_or(1).clamp(1, 20) },
            "shift" => Regulation::Shift { stop: num()? as usize, secs: num()?.clamp(-3600.0, 3600.0) },
            "early" => Regulation::EarlyDeparture { secs: num()?.clamp(0.0, 3600.0) },
            _ => return None,
        })
    }
}

/// The depot destination of a deadhead run in the bus's depot file: a terminus named for an
/// empty run ("haut-le-pied", "hors service", "dépôt", OMSI's "Betriebsfahrt"), else that.
pub fn deadhead_terminus(hof: Option<&omsi_vehicle::Hof>) -> String {
    const WORDS: [&str; 8] = ["haut-le-pied", "haut le pied", "hlp", "hors service", "betriebsfahrt", "dépôt", "depot", "sans voyageur"];
    hof.and_then(|h| {
        WORDS.iter().find_map(|w| h.termini.iter().find(|t| std::iter::once(&t.texture_id).chain(t.strings.iter()).any(|n| n.to_lowercase().contains(w))).map(|t| t.texture_id.clone()))
    })
    .unwrap_or_else(|| "Betriebsfahrt".into())
}

/// Why a re-sync was refused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resync {
    /// No such stop or trip.
    Unknown,
    /// The stop's place on the map is not known (its tile is not loaded).
    NoPlace,
    /// The bus stands this far from it (m).
    TooFar(f64),
}

impl PlayerDuty {
    /// The SAE menu's "re-sync at a stop": the stops of the trip under way the driver may
    /// say the bus stands at (index, name, departure in seconds of the day) - those it serves.
    pub fn resync_stops(&self) -> Vec<(usize, String, f64)> {
        self.trip().stops.iter().enumerate().filter(|(_, s)| s.stops).map(|(i, s)| (i, s.name.trim().to_string(), s.dep)).collect()
    }

    /// The SAE menu's "re-sync at a terminus": the trips of the duty from the one under way
    /// on, by where they leave (index, first stop, departure in seconds of the day).
    pub fn resync_termini(&self) -> Vec<(usize, String, f64)> {
        self.trips
            .iter()
            .enumerate()
            .skip(self.trip_index)
            .filter_map(|(i, t)| t.stops.first().map(|s| (i, s.name.trim().to_string(), t.departure)))
            .collect()
    }

    /// Re-sync at a stop: the driver says the bus stands at stop `stop` of the trip under way
    /// (it is at `pos`); the duty goes on from there - the stops before it no longer due, and
    /// those after it due again. The bus is not moved: it must stand at the stop.
    pub fn resync_stop(&mut self, stop: usize, pos: glam::DVec3) -> Result<String, Resync> {
        let s = self.trip().stops.get(stop).ok_or(Resync::Unknown)?;
        let name = s.name.trim().to_string();
        Self::near(s.position, pos)?;
        self.next_stop = stop;
        self.at_stop = false;
        self.done = false;
        self.served_terminus = None;
        self.arrived_late = None;
        self.held_back = true;
        self.placed = true;
        Ok(name)
    }

    /// Re-sync at a terminus, between two trips: the driver says the bus stands at the
    /// first stop of the duty's trip `trip`, ready for it; the duty goes on with that trip.
    pub fn resync_terminus(&mut self, trip: usize, pos: glam::DVec3) -> Result<String, Resync> {
        let s = self.trips.get(trip).and_then(|t| t.stops.first()).ok_or(Resync::Unknown)?;
        let name = s.name.trim().to_string();
        Self::near(s.position, pos)?;
        if trip != self.trip_index || self.next_stop != 0 || self.done {
            self.set_trip(trip);
        }
        self.picked = true;
        self.placed = true;
        Ok(name)
    }

    /// The bus at `pos` stands at a stop at `at` (within `RESYNC_REACH`).
    fn near(at: Option<glam::DVec3>, pos: glam::DVec3) -> Result<(), Resync> {
        let at = at.ok_or(Resync::NoPlace)?;
        let d = (at - pos).truncate().length();
        if d > RESYNC_REACH {
            return Err(Resync::TooFar(d));
        }
        Ok(())
    }

    /// A dispatcher's regulation (`Regulation`); `depot` is the depot destination a
    /// deadhead run shows (the bus's depot file's, see `deadhead_terminus`). Returns what was
    /// done, for the driver's notice, its words put through `tr`; the stops are the
    /// timetable's numbers (those the server's control room gives).
    ///
    /// A commercial turn takes up the first trip back through the stop: one of the duty's
    /// (those before it are given up), or else `others` - the timetable's trips under way,
    /// see [`ScheduleSim::trips_between`] - when the duty has none soon enough.
    pub fn regulate(&mut self, r: Regulation, depot: &str, others: &[PlannedTrip], tr: &dyn Fn(&str) -> String) -> Result<String, String> {
        let done = self.regulate_inner(r, depot, others, tr);
        if done.is_ok() {
            // (the displays and the IBIS take the trip again)
            self.trip_changed = true;
            self.held_back = false;
        }
        done
    }

    /// The first trip back through the duty's stop `k` of the trip under way, for a
    /// commercial turn there: (in the duty: its index, else in `others`: its index, its stop
    /// there). A trip back leaves the stop after the bus is due there and goes on to stops
    /// the bus came by; one of the duty's is preferred unless it leaves much later.
    fn trip_back(&self, k: usize, others: &[PlannedTrip]) -> Result<(Result<usize, usize>, usize), String> {
        let cur = self.trip();
        let here = &cur.stops[k];
        let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
        let came_by = &cur.stops[..k];
        let serves = |t: &PlannedTrip| -> Option<usize> {
            if t.regul.deadhead {
                return None;
            }
            let j = t.stops.iter().position(|s| same(&s.name, &here.name) || s.object_id == here.object_id && s.object_id != 0)?;
            let back = t.stops[j + 1..].iter().any(|s| came_by.iter().any(|c| same(&c.name, &s.name)));
            (j + 1 < t.stops.len() && back && t.stops[j].dep >= here.arr - 60.0).then_some(j)
        };
        let mine = (self.trip_index + 1..self.trips.len()).find_map(|i| serves(&self.trips[i]).map(|j| (i, j)));
        // (not one the duty drives anyway, by its name and its time)
        let theirs = others
            .iter()
            .enumerate()
            .filter(|(_, t)| !self.trips.iter().any(|d| d.name == t.name && (d.departure - t.departure).abs() < 1.0))
            .filter_map(|(i, t)| serves(t).map(|j| (i, j)))
            .min_by(|a, b| {
                let key = |&(i, j): &(usize, usize)| (others[i].line != cur.line, others[i].stops[j].dep);
                key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal)
            });
        match (mine, theirs) {
            (Some((i, j)), Some((o, oj))) if self.trips[i].stops[j].dep > others[o].stops[oj].dep + TURN_WAIT => Ok((Err(o), oj)),
            (Some((i, j)), _) => Ok((Ok(i), j)),
            (None, Some((o, oj))) => Ok((Err(o), oj)),
            (None, None) => Err(format!("no trip back serves {}", here.name.trim())),
        }
    }

    fn regulate_inner(&mut self, r: Regulation, depot: &str, others: &[PlannedTrip], tr: &dyn Fn(&str) -> String) -> Result<String, String> {
        let offset = self.trip().regul.stop_offset;
        let local = |k: usize| k.checked_sub(offset).ok_or_else(|| "this stop is behind the bus".to_string());
        match r {
            Regulation::Turn { stop } | Regulation::TurnEmpty { stop, .. } => {
                let k = local(stop)?;
                let last = self.trip().stops.len().saturating_sub(1);
                if k >= last {
                    return Err("the trip ends there anyway".into());
                }
                if k < self.next_stop || self.done {
                    return Err("this stop is behind the bus".into());
                }
                let next_i = self.trip_index + 1;
                let here = self.trip().stops[k].clone();
                // where the next trip is taken up: for a commercial turn, the first trip back
                // through the stop (the other side of the street, by its name), the duty's
                // trips before it given up - or one of another tour put in the duty; the stop
                // asked for after a deadhead run
                let j = match r {
                    Regulation::TurnEmpty { resume, .. } => {
                        if next_i >= self.trips.len() {
                            return Err("no trip after this one".into());
                        }
                        resume.checked_sub(self.trips[next_i].regul.stop_offset).filter(|j| *j < self.trips[next_i].stops.len()).ok_or("no such stop on the next trip")?
                    }
                    _ => match self.trip_back(k, others)? {
                        (Ok(i), j) => {
                            self.trips.drain(next_i..i);
                            j
                        }
                        (Err(o), j) => {
                            let back = others[o].clone();
                            // (the duty's trips it overlaps go: the bus drives this one)
                            let keep = self.trips[next_i..].iter().position(|t| t.departure >= back.end).map_or(self.trips.len(), |p| next_i + p);
                            self.trips.splice(next_i..keep, [back]);
                            j
                        }
                    },
                };
                if j + 1 >= self.trips[next_i].stops.len() {
                    return Err("the next trip would have no stop left".into());
                }
                // this trip ends at the stop
                let cur = &mut self.trips[self.trip_index];
                cur.stops.truncate(k + 1);
                cur.end = cur.stops[k].arr;
                // the next one begins at its stop `j`, its time there as its departure
                let next = &mut self.trips[next_i];
                next.regul.timetable_departure.get_or_insert(next.departure);
                next.regul.stop_offset += j;
                next.stops.drain(..j);
                next.departure = next.stops[0].dep;
                next.set_dirs();
                let take_up = next.stops[0].name.trim().to_string();
                let at = hhmm(next.departure.rem_euclid(DAY));
                if let Regulation::TurnEmpty { .. } = r {
                    let leg = self.deadhead_leg(next_i, depot);
                    self.trips.insert(next_i, leg);
                    return Ok(format!("{} {} | {} {take_up} | {at}", tr("Turn at"), here.name.trim(), tr("then empty to")));
                }
                let next = &self.trips[next_i];
                Ok(format!("{} {} | {} {at} ({} {} -> {})", tr("Commercial turn at"), here.name.trim(), tr("next trip from there at"), tr("Line"), next.line.trim(), next.terminus.trim()))
            }
            Regulation::Deadhead { trips } => {
                let target = self.trip_index + trips.max(1);
                if target >= self.trips.len() {
                    return Err("no such trip later in the duty".into());
                }
                // the trips skipped go; the bus runs empty to the target's first stop
                self.trips.drain(self.trip_index + 1..target);
                let leg = self.deadhead_leg(self.trip_index + 1, depot);
                self.trips.insert(self.trip_index + 1, leg);
                self.set_trip(self.trip_index + 1);
                self.picked = true;
                let t = &self.trips[self.trip_index + 1];
                Ok(format!("{} {} | {}", tr("Deadhead run to"), t.stops[0].name.trim(), hhmm(t.departure.rem_euclid(DAY))))
            }
            Regulation::Shift { stop, secs } => {
                let k = local(stop)?;
                let t = &mut self.trips[self.trip_index];
                if k >= t.stops.len() {
                    return Err("no such stop".into());
                }
                let name = t.stops[k].name.trim().to_string();
                for (i, s) in t.stops.iter_mut().enumerate().skip(k) {
                    // (the arrival at the stop shifted stays: the bus waits there longer)
                    if i > k {
                        s.arr += secs;
                    }
                    s.dep += secs;
                }
                t.end += secs;
                if k == 0 {
                    t.departure += secs;
                }
                Ok(format!("{} {:+} min {} {name}", tr("Timetable shifted"), (secs / 60.0).round(), tr("from")))
            }
            Regulation::EarlyDeparture { secs } => {
                // the trip about to leave its terminus: the next one at the end of a trip, this
                // one while the bus has not left its first stop
                let ti = if self.done {
                    self.trip_index + 1
                } else if self.next_stop == 0 {
                    self.trip_index
                } else {
                    return Err("the trip has left its terminus".into());
                };
                let t = self.trips.get_mut(ti).ok_or("no trip to start")?;
                t.regul.timetable_departure.get_or_insert(t.departure);
                for s in &mut t.stops {
                    s.arr -= secs;
                    s.dep -= secs;
                }
                t.departure -= secs;
                t.end -= secs;
                Ok(format!("{} {}", tr("Early departure at"), hhmm(t.departure.rem_euclid(DAY))))
            }
        }
    }

    /// A deadhead run to the first stop of trip `to`: out of service, its only stop that one.
    fn deadhead_leg(&self, to: usize, depot: &str) -> PlannedTrip {
        let t = &self.trips[to];
        let from = self.trip().end.min(t.departure);
        let mut stop = t.stops[0].clone();
        stop.arr = t.stops[0].arr.min(t.departure);
        stop.dep = stop.arr;
        PlannedTrip {
            name: format!("HLP {}", t.name),
            line: String::new(),
            terminus: depot.to_string(),
            departure: from,
            end: stop.arr,
            stops: vec![stop],
            regul: TripRegulation { deadhead: true, ..Default::default() },
        }
    }

    /// Where the duty is, for the server the game plays on: (the trip under way, its
    /// departure by the timetable, the stop the bus stands at or drives to by the
    /// timetable's numbers, standing at it, the trip done, the delay the IBIS shows, a
    /// deadhead run). A deadhead run gives the trip it takes up, at the stop it takes it up
    /// from.
    pub fn progress_parts(&self, now: f64) -> (String, u32, u32, bool, bool, i32, bool) {
        let deadhead = self.trip().regul.deadhead && self.trip_index + 1 < self.trips.len();
        let t = if deadhead { &self.trips[self.trip_index + 1] } else { self.trip() };
        let stop = if deadhead { 0 } else { self.next_stop.min(t.stops.len().saturating_sub(1)) };
        (
            t.name.clone(),
            t.regul.timetable_departure.unwrap_or(t.departure).rem_euclid(DAY).round() as u32,
            (stop + t.regul.stop_offset) as u32,
            self.at_stop && !deadhead,
            self.done && !deadhead,
            self.delay(now).round().clamp(-86400.0, 86400.0) as i32,
            deadhead,
        )
    }
}
