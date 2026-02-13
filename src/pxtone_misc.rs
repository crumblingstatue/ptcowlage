use ptcow::{EveList, EventPayload, Song, UnitIdx};

/// Migrate overlapping 'on' events from one unit to another
pub fn poly_migrate_units(src_unit: UnitIdx, dst_unit: UnitIdx, song: &mut Song) -> bool {
    let mut has_overlap = false;
    let n_events = song.events.len();
    for i in 0..n_events {
        let eve1 = &song.events[i];
        if eve1.unit != src_unit {
            continue;
        }
        let EventPayload::On { duration: dur1 } = eve1.payload else {
            continue;
        };
        let range1 = eve1.tick..eve1.tick + dur1;
        for j in i + 1..n_events {
            let eve2 = &mut song.events[j];
            let eve2_tick = eve2.tick;
            if eve2.unit != src_unit {
                continue;
            }
            if let EventPayload::On { duration: dur2 } = eve2.payload {
                let range2 = eve2.tick..eve2.tick + dur2;
                if overlap(range1.clone(), range2) {
                    has_overlap = true;
                    eve2.unit = dst_unit;
                } else {
                    continue;
                }
                // We assume that preceding events set the key and velocity before the on event,
                // and we migrate these as well
                if j >= 3 {
                    for k in j - 3..j {
                        if let Some(eve3) = song.events.get_mut(k)
                            && eve3.tick == eve2_tick
                            && let EventPayload::Key(_)
                            | EventPayload::Velocity(_)
                            | EventPayload::Volume(_) = eve3.payload
                        {
                            eve3.unit = dst_unit;
                        }
                    }
                }
            }
        }
    }
    has_overlap
}

fn overlap<T: PartialOrd>(r1: std::ops::Range<T>, r2: std::ops::Range<T>) -> bool {
    r1.start < r2.end && r1.end > r2.start
}

/// If two events of the same type for the same unit happen on the same tick, all but the last of
/// those events will "lose", meaning they have no effect.
///
/// This function removes such "losing" events.
pub fn clean_losing_events(events: &mut EveList) {
    events.reverse();
    // "Next" from the viewpoint of the unreversed event list
    let mut next_ev_discr = None;
    let mut next_tick = None;
    let mut next_unit = None;
    events.retain(|eve| {
        let same_as_next = next_ev_discr == Some(eve.payload.discriminant())
            && next_tick == Some(eve.tick)
            && next_unit == Some(eve.unit);
        next_ev_discr = Some(eve.payload.discriminant());
        next_tick = Some(eve.tick);
        next_unit = Some(eve.unit);
        !same_as_next
    });
    events.reverse();
}
