use std::collections::BTreeMap;

use arrayvec::ArrayVec;
use ptcow::{
    DEFAULT_KEY, EnvPt, EveList, Event, EventPayload, NoiseData, NoiseDesignOscillator,
    NoiseDesignUnit, NoiseDesignUnitFlags, NoiseType, OsciPt, Song, Unit, UnitIdx, VoiceIdx,
    WaveData,
};

use crate::audio_out::SongState;

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

struct KeyEvOffsets {
    on: usize,
    key: usize,
}

pub(crate) fn split_unit_events_by_key(song: &mut SongState, idx: UnitIdx) {
    let eves = &mut song.song.events.eves;
    let offsets = key_ev_offsets(eves, idx);
    let mut key_map: BTreeMap<i32, Vec<KeyEvOffsets>> = BTreeMap::new();
    for offs in offsets {
        assert_eq!(eves[offs.key].unit, idx);
        assert_eq!(eves[offs.on].unit, idx);
        let EventPayload::Key(key) = eves[offs.key].payload else {
            continue;
        };
        let vec = key_map.entry(key).or_default();
        vec.push(offs);
    }
    let Some((fst_key, fst_offs)) = key_map.first_key_value() else {
        return;
    };
    for off in fst_offs {
        assert_eq!(eves[off.key].unit, idx);
        assert_eq!(eves[off.on].unit, idx);
        eves[off.key].payload = EventPayload::Key(DEFAULT_KEY);
    }
    let name = song.herd.units[idx].name.clone();
    song.herd.units[idx]
        .name
        .push_str(&format!("-{}", fst_key / 256));
    let fst_voice_idx = eves
        .iter()
        .find_map(|eve| {
            if eve.unit != idx {
                return None;
            }
            match eve.payload {
                EventPayload::SetVoice(idx) => Some(idx),
                _ => None,
            }
        })
        .unwrap_or(VoiceIdx(0));
    let mut unit_idx_counter = UnitIdx(song.herd.units.len());
    let mut events_to_insert = Vec::new();
    for (key, offs) in key_map.into_iter().skip(1) {
        for off in offs {
            assert_eq!(eves[off.key].unit, idx);
            assert_eq!(eves[off.on].unit, idx);
            eves[off.key].unit = unit_idx_counter;
            eves[off.key].payload = EventPayload::Key(DEFAULT_KEY);
            eves[off.on].unit = unit_idx_counter;
        }
        song.herd.units.push(Unit {
            name: format!("{name}-{}", key / 256),
            ..Default::default()
        });
        events_to_insert.push(Event {
            payload: EventPayload::SetVoice(fst_voice_idx),
            unit: unit_idx_counter,
            tick: 0,
        });
        unit_idx_counter.0 += 1;
    }
    eves.splice(0..0, events_to_insert);
    song.song.events.sort();
}

fn key_ev_offsets(eves: &[Event], unit_idx: UnitIdx) -> Vec<KeyEvOffsets> {
    let mut out = Vec::new();
    for (eve_idx, eve) in eves.iter().enumerate() {
        if eve.unit != unit_idx {
            continue;
        }
        // Find an on event
        if matches!(eve.payload, EventPayload::On { .. }) {
            // Find winning key event before next On event
            let Some(key_idx) = find_winning_key_ev(eves, eve_idx, unit_idx) else {
                continue;
            };
            debug_assert!(matches!(eves[key_idx].payload, EventPayload::Key { .. }));
            assert_eq!(eves[eve_idx].unit, unit_idx);
            assert_eq!(eves[key_idx].unit, unit_idx);
            out.push(KeyEvOffsets {
                on: eve_idx,
                key: key_idx,
            });
        }
    }
    out
}

fn find_winning_key_ev(eves: &[Event], on_idx: usize, unit_idx: UnitIdx) -> Option<usize> {
    let first_tick = first_tick(eves, on_idx);
    let next_on = next_on(eves, on_idx)?;
    eves[first_tick..next_on]
        .iter()
        .rposition(|eve| eve.unit == unit_idx && matches!(eve.payload, EventPayload::Key(_)))
        .map(|off| first_tick + off)
}

fn first_tick(eves: &[Event], idx: usize) -> usize {
    let tick = eves[idx].tick;
    let mut cursor = idx;
    while eves[cursor].tick == tick {
        // This is literally the first event
        if cursor == 0 {
            return 0;
        }
        cursor -= 1;
    }
    cursor
}

fn next_on(eves: &[Event], idx: usize) -> Option<usize> {
    eves[idx..]
        .iter()
        .position(|eve| matches!(eve.payload, EventPayload::On { .. }))
        .map(|off| idx + off)
}

pub fn reset_voice_for_units_with_voice_idx(song: &mut SongState, idx: VoiceIdx) {
    for unit in song.herd.units.iter_mut() {
        if unit.voice_idx == idx {
            unit.reset_voice(&song.ins, idx, song.song.master.timing);
        }
    }
}

pub fn square_wave() -> WaveData {
    WaveData::Coord {
        points: vec![
            OsciPt { x: 0, y: 0 },
            OsciPt { x: 1, y: 48 },
            OsciPt { x: 99, y: 48 },
            OsciPt { x: 100, y: -48 },
            OsciPt { x: 199, y: -48 },
        ],
        resolution: 200,
    }
}

pub fn bass_drum() -> NoiseData {
    NoiseData {
        smp_num_44k: 8000,
        units: ArrayVec::try_from(
            &[NoiseDesignUnit {
                enves: [
                    EnvPt { x: 1, y: 100 },
                    EnvPt { x: 100, y: 20 },
                    EnvPt { x: 200, y: 0 },
                ]
                .into(),

                pan: 0,
                main: NoiseDesignOscillator {
                    type_: NoiseType::Sine,
                    freq: 50.0,
                    volume: 180.0,
                    offset: 2.0,
                    invert: false,
                },
                freq: NoiseDesignOscillator {
                    type_: NoiseType::Saw,
                    freq: 5.0,
                    volume: 2.0,
                    offset: 0.0,
                    invert: false,
                },
                volu: NoiseDesignOscillator::default(),
                ser_flags: NoiseDesignUnitFlags::OSC_MAIN,
            }][..],
        )
        .unwrap(),
    }
}
