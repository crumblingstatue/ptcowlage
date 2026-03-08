use {
    midly::{MetaMessage, MidiMessage, TrackEventKind, num::u7},
    ptcow::{Event, EventPayload, Herd, Song, Unit, UnitIdx, VoiceIdx},
    rustc_hash::FxHashMap,
};

/// Assume first tempo is "default" tempo.
///
/// We can't really handle songs with changing tempos.
fn guess_tempo(tracks: &[midly::Track]) -> Option<u32> {
    for track in tracks {
        for ev in track {
            if let TrackEventKind::Meta(msg) = ev.kind
                && let MetaMessage::Tempo(u24) = msg
            {
                return Some(u24.as_int());
            }
        }
    }
    None
}

/// Write midi song to pxtone
#[expect(
    clippy::unnecessary_wraps,
    reason = "Needs this signature due to fn pointer"
)]
pub fn write_midi_to_pxtone(
    mid_data: &[u8],
    herd: &mut Herd,
    song: &mut Song,
) -> anyhow::Result<()> {
    let mut used_programs = FxHashMap::default();
    let (header, track_iter) = midly::parse(mid_data).unwrap();
    let tracks = track_iter.collect_tracks().unwrap();
    let ticks_per_beat = match header.timing {
        midly::Timing::Metrical(u15) => u15.as_int(),
        midly::Timing::Timecode(_fps, _) => todo!(),
    };
    song.master.timing.bpm = guess_tempo(&tracks).map_or(120.0, ms_per_beat_to_bpm);
    song.events.eves.clear();
    herd.units.clear();
    song.master.timing.ticks_per_beat = ticks_per_beat;
    let mut max_clock = 0;
    let mut unit_counter = UnitIdx(0);
    for (track_idx, track) in tracks.iter().enumerate() {
        // Whether this track needs a unit to allocate
        // We assume if there is no "NoteOn" event for this track, there is no need for a unit
        let mut needs_unit = false;
        let mut clock = 0;
        let mut pitch_bend: f64 = 0.0;
        let mut last_key = None;
        for (ev_idx, event) in track.iter().enumerate() {
            // The delta is how much after the previous event this current event is,
            // so we start by incrementing the clock
            clock += f64::from(event.delta.as_int()) as u32;
            match event.kind {
                TrackEventKind::Midi { message, .. } => match message {
                    MidiMessage::NoteOff { .. } => {
                        // We calculate how long notes last in the `NoteOn` event, so we do nothing here
                    }
                    MidiMessage::NoteOn { key, vel } => {
                        needs_unit = true;
                        last_key = Some(key);
                        push_key_event(song, unit_counter, clock, pitch_bend, key);
                        // If velocity is zero, we don't want to emit an `On` event.
                        if vel == 0 {
                            //continue;
                        }
                        song.events.eves.push(Event {
                            payload: EventPayload::Velocity(i16::from(vel.as_int())),
                            unit: unit_counter,
                            tick: clock,
                        });
                        // Find the next note off event for the duration
                        let duration = 'block: {
                            let mut clock2 = clock;
                            for ev in track.iter().skip(ev_idx) {
                                clock2 += ev.delta.as_int();
                                if let TrackEventKind::Midi {
                                    channel: _,
                                    message,
                                } = ev.kind
                                {
                                    match message {
                                        MidiMessage::NoteOff { key: key2, .. } if key2 == key => {
                                            break 'block clock2 - clock;
                                        }
                                        // Tricky, but NoteOn with velocity of 0 also means note off, apparently.
                                        MidiMessage::NoteOn { vel, key: key2 }
                                            if key2 == key && vel == 0 =>
                                        {
                                            break 'block clock2 - clock;
                                        }
                                        _ => (),
                                    }
                                }
                            }
                            panic!("Couldn't determine note duration");
                        };
                        song.events.eves.push(Event {
                            payload: EventPayload::On { duration },
                            unit: unit_counter,
                            tick: clock,
                        });
                    }
                    MidiMessage::ProgramChange { program } => {
                        let len = used_programs.len();
                        let idx = used_programs
                            .entry(program)
                            .or_insert(VoiceIdx(len.try_into().unwrap()));
                        log::info!("Instrument change of {track_idx} to {program}");
                        song.events.eves.push(Event {
                            payload: EventPayload::SetVoice(*idx),
                            unit: unit_counter,
                            tick: clock,
                        });
                    }
                    MidiMessage::PitchBend { bend } => {
                        pitch_bend = bend.as_f64();
                        if let Some(last) = last_key {
                            push_key_event(song, unit_counter, clock, pitch_bend, last);
                        }
                    }
                    MidiMessage::Controller { controller, value } => {
                        match controller.as_int() {
                            // 7: "Channel volume"
                            // 11: "Expression" or secondary volume controller
                            7 | 11 => {
                                song.events.eves.push(Event {
                                    payload: EventPayload::Volume(i16::from(value.as_int())),
                                    unit: unit_counter,
                                    tick: clock,
                                });
                            }
                            _ => {
                                log::info!("c {controller} = {value}");
                            }
                        }
                    }
                    _ => log::warn!("Unhandled mid msg: {message:?}"),
                },
                TrackEventKind::Meta(meta_message) => match meta_message {
                    MetaMessage::TrackName(name_bytes) => {
                        log::info!("Track name: {:?}", std::str::from_utf8(name_bytes));
                    }
                    MetaMessage::EndOfTrack => {}
                    MetaMessage::TimeSignature(num, denom, cpt, npq_32nd) => {
                        log::info!("Time sig: {num} {denom} {cpt} {npq_32nd}");
                    }
                    _ => log::warn!("UNhandled meta: {meta_message:?}"),
                },
                _ => log::warn!("Unhandled event kind: {:?}", event.kind),
            }
        }
        max_clock = max_clock.max(clock);
        if needs_unit {
            let unit = Unit {
                name: format!("mtrk{track_idx:02}"),
                ..Default::default()
            };
            herd.units.push(unit);
            unit_counter.0 += 1;
        }
    }
    // Unset the last point (let it be calculated by PxTone)
    song.master.loop_points.last = None;

    // PxTone events seem to need to be stored in order of increasing clock value
    song.events.eves.sort_by_key(|ev| ev.tick);
    Ok(())
}

fn push_key_event(song: &mut Song, unit_idx: UnitIdx, clock: u32, pitch_bend: f64, key: u7) {
    let base_key = 27;
    let raw_key = i32::from(key.as_int() + base_key) * 256;
    // TODO: 2560 magic number, based on ear (and it being 10 times 256, something to do with cents?)
    let bend_mod = pitch_bend * 2560.0;
    if bend_mod != 0.0 {
        song.events.eves.push(Event {
            payload: EventPayload::PtcowDebug(bend_mod as i32),
            unit: unit_idx,
            tick: clock,
        });
    }
    song.events.eves.push(Event {
        payload: EventPayload::Key((f64::from(raw_key) + bend_mod) as i32),
        unit: unit_idx,
        tick: clock,
    });
}

/// Microseconds per minute
const MS_PER_MINUTE: u32 = 60_000_000;

fn ms_per_beat_to_bpm(ms_per_beat: u32) -> f32 {
    MS_PER_MINUTE as f32 / ms_per_beat as f32
}
