//! The dispatcher's regulation of the player's duty and the driver's re-sync (the SAE menu).

use super::*;

fn duty_of(trips: Vec<PlannedTrip>) -> PlayerDuty {
    PlayerDuty {
        line: "5".into(),
        tour: "1".into(),
        trips,
        trip_index: 0,
        first_trip: 0,
        next_stop: 1,
        at_stop: false,
        arrived_late: None,
        done: false,
        served_terminus: None,
        left_late: None,
        held_back: false,
        placed: true,
        trip_changed: false,
        skipped: None,
        run: 0,
        finished: None,
        reopened: None,
        picked: true,
        first_update: None,
        heading: 90.0,
        position: None,
    }
}

/// The words of a regulation's notice, untranslated.
fn plain(s: &str) -> String {
    s.to_string()
}

/// The SAE menu's re-sync: the duty goes on from the stop or the trip the driver says the
/// bus stands at - only when it does stand there.
#[test]
fn the_driver_resyncs_the_duty_at_a_stop_or_a_terminus_where_the_bus_stands() {
    let t1 = planned(0.0, &[(0.0, 0.0, 0.0), (500.0, 100.0, 100.0), (1000.0, 200.0, 200.0)]);
    let t2 = planned(400.0, &[(1040.0, 400.0, 400.0), (1500.0, 500.0, 500.0)]);
    let mut d = duty_of(vec![t1, t2]);
    d.next_stop = 0;
    assert_eq!(d.resync_stops().len(), 3);
    // far from the stop: refused, nothing changes
    assert!(matches!(d.resync_stop(2, glam::DVec3::new(700.0, 0.0, 0.0)), Err(Resync::TooFar(_))));
    assert_eq!(d.next_stop, 0);
    // at it: the duty is there
    assert_eq!(d.resync_stop(2, glam::DVec3::new(1030.0, 10.0, 0.0)).as_deref(), Ok("s2"));
    assert_eq!(d.next_stop, 2);
    d.advance(glam::DVec3::new(1000.0, 0.0, 0.0), 190.0);
    assert!(d.at_stop);
    // and back to an earlier one
    assert!(d.resync_stop(1, glam::DVec3::new(510.0, 0.0, 0.0)).is_ok());
    assert_eq!((d.next_stop, d.done), (1, false));
    // the next trip, from its first stop
    assert_eq!(d.resync_termini().iter().map(|t| t.0).collect::<Vec<_>>(), [0, 1]);
    assert!(matches!(d.resync_terminus(1, glam::DVec3::new(0.0, 0.0, 0.0)), Err(Resync::TooFar(_))));
    assert_eq!(d.resync_terminus(1, glam::DVec3::new(1045.0, 0.0, 0.0)).as_deref(), Ok("s0"));
    assert_eq!((d.trip_index, d.next_stop), (1, 0));
    assert_eq!(d.resync_terminus(5, glam::DVec3::ZERO), Err(Resync::Unknown));
}

/// The dispatcher's regulations: a turn, a turn and a deadhead run, a deadhead run, a
/// shift, an early departure.
#[test]
fn the_dispatcher_regulates_the_duty() {
    // out s0 s1 s2 s3, back on the same stops (named alike) s3' s2' s1' s0'
    // (each direction's stops are objects of their own, on their side of the street)
    let named = |mut t: PlannedTrip, names: &[&str], ids: i64| {
        for (s, n) in t.stops.iter_mut().zip(names) {
            s.name = n.to_string();
            s.object_id += ids;
        }
        t
    };
    let out = named(planned(0.0, &[(0.0, 0.0, 0.0), (500.0, 100.0, 100.0), (1000.0, 200.0, 200.0), (1500.0, 300.0, 300.0)]), &["A", "B", "C", "D"], 100);
    let back = named(planned(600.0, &[(1500.0, 600.0, 600.0), (1000.0, 700.0, 700.0), (500.0, 800.0, 800.0), (0.0, 900.0, 900.0)]), &["D", "C", "B", "A"], 200);
    let again = named(planned(1200.0, &[(0.0, 1200.0, 1200.0), (500.0, 1300.0, 1300.0)]), &["A", "B"], 100);
    assert_eq!(Regulation::parse("turn 2"), Some(Regulation::Turn { stop: 2 }));
    assert_eq!(Regulation::parse("nope 2"), None);

    // a commercial turn at C: the trip ends there, the way back begins at C at 700 s
    let mut d = duty_of(vec![out.clone(), back.clone(), again.clone()]);
    assert!(d.regulate(Regulation::Turn { stop: 2 }, "HLP", &[], &plain).is_ok());
    assert_eq!(d.trips[0].stops.len(), 3);
    assert_eq!((d.trips[1].stops[0].name.as_str(), d.trips[1].departure, d.trips[1].stops.len()), ("C", 700.0, 3));
    // (the control room still knows the trip by its timetable's departure and stops)
    d.set_trip(1);
    let p = d.progress_parts(650.0);
    assert_eq!((p.1, p.2), (600, 1));

    // the next trip is another line's, elsewhere: the duty's trip back after it is taken
    // up, the one between given up
    let mut other = named(planned(600.0, &[(0.0, 650.0, 650.0), (500.0, 750.0, 750.0)]), &["X", "Y"], 300);
    other.name = "other".into();
    other.line = "9".into();
    let later = |t: &PlannedTrip, by: f64| {
        let mut t = t.clone();
        t.departure += by;
        t.end += by;
        for s in &mut t.stops {
            s.arr += by;
            s.dep += by;
        }
        t
    };
    let mut d = duty_of(vec![out.clone(), other.clone(), later(&back, 300.0)]);
    assert!(d.regulate(Regulation::Turn { stop: 2 }, "HLP", &[], &plain).is_ok());
    assert_eq!(d.trips.len(), 2);
    assert_eq!((d.trips[1].stops[0].name.as_str(), d.trips[1].departure), ("C", 1000.0));
    // ... and none in the duty: another tour's, the duty's trip it overlaps given up
    let mut d = duty_of(vec![out.clone(), other.clone(), again.clone()]);
    assert_eq!(d.regulate(Regulation::Turn { stop: 2 }, "HLP", &[], &plain), Err("no trip back serves C".into()));
    // (an outbound trip through C does not take the bus back)
    let theirs = [later(&out, 100.0), back.clone()];
    assert!(d.regulate(Regulation::Turn { stop: 2 }, "HLP", &theirs, &plain).is_ok());
    assert_eq!(d.trips.iter().map(|t| t.stops[0].name.as_str()).collect::<Vec<_>>(), ["A", "C", "A"]);
    assert_eq!(d.trips[1].departure, 700.0);

    // a stop behind the bus: refused
    let mut d = duty_of(vec![out.clone(), back.clone()]);
    d.next_stop = 3;
    assert!(d.regulate(Regulation::Turn { stop: 1 }, "HLP", &[], &plain).is_err());

    // a turn at B, then empty to the way back's C
    let mut d = duty_of(vec![out.clone(), back.clone(), again.clone()]);
    assert!(d.regulate(Regulation::TurnEmpty { stop: 1, resume: 1 }, "HLP", &[], &plain).is_ok());
    assert_eq!(d.trips.len(), 4);
    assert!(d.trips[1].regul.deadhead && d.trips[1].line.is_empty());
    assert_eq!(d.trips[1].stops[0].name, "C");
    assert_eq!(d.trips[2].stops[0].name, "C");

    // a deadhead run past the way back, to the next outbound trip's first stop
    let mut d = duty_of(vec![out.clone(), back.clone(), again.clone()]);
    assert!(d.regulate(Regulation::Deadhead { trips: 2 }, "Haut-le-pied", &[], &plain).is_ok());
    assert_eq!(d.trips.len(), 3);
    assert_eq!(d.trip_index, 1);
    assert!(d.trip().regul.deadhead);
    assert_eq!((d.trip().terminus.as_str(), d.trip().stops[0].name.as_str()), ("Haut-le-pied", "A"));
    let (ibis, _) = d.trip_for_ibis();
    assert!(ibis.regul.deadhead, "the IBIS shows the deadhead run");
    let p = d.progress_parts(1000.0);
    assert!(p.6 && p.0 == again.name);
    // (not early on the way: late only once the trip cannot leave in time)
    assert_eq!(d.delay(1000.0), 0.0);
    assert_eq!(d.delay(1290.0), 90.0);

    // two minutes later from C on
    let mut d = duty_of(vec![out.clone(), back.clone()]);
    assert!(d.regulate(Regulation::Shift { stop: 2, secs: 120.0 }, "", &[], &plain).is_ok());
    let t = &d.trips[0];
    assert_eq!((t.stops[1].dep, t.stops[2].arr, t.stops[2].dep, t.stops[3].arr, t.end), (100.0, 200.0, 320.0, 420.0, 420.0));

    // an early departure of the trip at the terminus, the one before it done
    let mut d = duty_of(vec![out.clone(), back.clone()]);
    d.next_stop = 3;
    d.done = true;
    assert!(d.regulate(Regulation::EarlyDeparture { secs: 180.0 }, "", &[], &plain).is_ok());
    assert_eq!((d.trips[1].departure, d.trips[1].stops[3].arr), (420.0, 720.0));
    // a trip under way cannot leave earlier
    let mut d = duty_of(vec![out, back]);
    assert!(d.regulate(Regulation::EarlyDeparture { secs: 60.0 }, "", &[], &plain).is_err());
}
