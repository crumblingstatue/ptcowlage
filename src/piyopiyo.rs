use piyopiyo::{DRUM_SAMPLES, piano_keys};
use ptcow::{
    EnvPt, EnvelopeSrc, Event, EventPayload, Herd, MooInstructions, OsciPt, PcmData, Song, Unit,
    UnitIdx, Voice, VoiceFlags, VoiceIdx, VoiceUnit, WaveData, WaveDataPoints,
};
use rustc_hash::FxHashMap;

use crate::pxtone_misc::BASIC_KEY_A2;

fn piyo_pan_to_pxtone_pan(piyo: i16) -> u8 {
    // These are the fixed values that the PiyoPiyo pan value can be
    let piyo_px_mapping = [
        (2560, 0),
        (1600, 18),
        (760, 36),
        (320, 54),
        (0, 64),
        (-320, 74),
        (-760, 92),
        (-1640, 128),
    ];
    piyo_px_mapping
        .into_iter()
        .find_map(|(piyo_v, px_v)| (piyo_v == piyo).then_some(px_v))
        .unwrap()
}

pub fn import(piyo: &piyopiyo::Song, herd: &mut Herd, song: &mut Song, ins: &mut MooInstructions) {
    song.events.clear();
    // We assume this default timing is good for all PiyoPiyo songs, which might not be true(?)
    song.master.timing.ticks_per_beat = 480;
    song.master.timing.bpm = 125.;
    herd.units.clear();
    ins.voices.clear();
    for (m_i, tr) in piyo.melody_tracks.iter().enumerate() {
        let points = tr
            .waveform
            .iter()
            .enumerate()
            .map(|(i, v)| OsciPt {
                x: i as u16,
                y: i16::from(*v),
            })
            .collect();
        let wave_points = WaveDataPoints::Coord {
            points,
            resolution: 256,
        };
        let unit = VoiceUnit {
            flags: VoiceFlags::WAVE_LOOP,
            basic_key: BASIC_KEY_A2,
            ..VoiceUnit::default()
        };
        // Seems like envelope values need to be scaled a bit to be more accurate
        let env_scale = 1.5;
        let env = EnvelopeSrc {
            seconds_per_point: 64,
            points: tr
                .envelope
                .iter()
                .map(|val| EnvPt {
                    x: 1,
                    y: (f64::from(*val) * env_scale) as u8,
                })
                .collect(),
        };
        let mut voice = Voice::from_unit_and_data(
            unit,
            ptcow::VoiceData::Wave(WaveData {
                points: wave_points,
                envelope: env,
                volume: 32,
                pan: 64,
            }),
        );
        voice.name = format!("Melody {m_i}");
        ins.voices.push(voice);
        let mut time_ms = 1;
        let mut units_needed = 0;
        let n_units = herd.units.len();
        let note_duration = u32::from(tr.len) / 22;
        let mut unit_end_ticks = FxHashMap::default();
        for ev in &tr.base.events {
            let mut unit_counter = 0;
            for key in piano_keys() {
                if ev.key_down(key) {
                    // See if unit is busy (still playing)
                    loop {
                        let still_playing = unit_end_ticks
                            .get(&unit_counter)
                            .is_some_and(|end| *end >= time_ms);
                        if still_playing {
                            unit_counter += 1;
                        } else {
                            break;
                        }
                    }
                    let base_key = 51 * 256;
                    let octave_shift = i32::from(tr.octave) * (12 * 256);
                    let ev_key = base_key + octave_shift + i32::from(key) * 256;
                    let pan = if let Some(pan) = ev.pan() {
                        piyo_pan_to_pxtone_pan(pan)
                    } else {
                        64
                    };
                    song.events.push(Event {
                        payload: EventPayload::PanVol(pan),
                        unit: UnitIdx(n_units + unit_counter),
                        tick: time_ms,
                    });
                    song.events.push(Event {
                        payload: ptcow::EventPayload::Key(ev_key),
                        unit: UnitIdx(n_units + unit_counter),
                        tick: time_ms,
                    });
                    song.events.push(Event {
                        payload: ptcow::EventPayload::On {
                            duration: note_duration,
                        },
                        unit: UnitIdx(n_units + unit_counter),
                        tick: time_ms,
                    });
                    unit_end_ticks.insert(unit_counter, time_ms + note_duration);
                    unit_counter += 1;
                }
            }
            units_needed = std::cmp::max(units_needed, unit_counter);
            time_ms += piyo.event_wait_ms;
        }
        for i in 0..units_needed {
            herd.units.push(Unit {
                name: format!("Melody {m_i}-{i}"),
                voice_idx: ptcow::VoiceIdx(m_i.try_into().unwrap()),
                ..Default::default()
            });
            song.events.push(Event {
                payload: EventPayload::SetVoice(ptcow::VoiceIdx(m_i.try_into().unwrap())),
                unit: UnitIdx(n_units + i),
                tick: 0,
            });
            song.events.push(Event {
                payload: EventPayload::Volume(tr.base.vol as i16),
                unit: UnitIdx(n_units + i),
                tick: 0,
            });
        }
    }
    let n_units = herd.units.len();
    let mut time_ms = 0;
    let mut units_needed = 0;
    for ev in &piyo.percussion_track.base.events {
        let mut unit_counter = 0;
        for key in piano_keys() {
            if ev.key_down(key) {
                song.events.push(Event {
                    payload: ptcow::EventPayload::SetVoice(VoiceIdx(3 + key)),
                    unit: UnitIdx(n_units + unit_counter),
                    tick: time_ms,
                });
                let duration = DRUM_SAMPLES[key as usize].len() as u32 / 12;
                song.events.push(Event {
                    payload: ptcow::EventPayload::On { duration },
                    unit: UnitIdx(n_units + unit_counter),
                    tick: time_ms,
                });
                unit_counter += 1;
            }
        }
        units_needed = std::cmp::max(units_needed, unit_counter);
        time_ms += piyo.event_wait_ms;
    }
    for i in 0..units_needed {
        herd.units.push(Unit {
            name: format!("Drum {i}"),
            ..Default::default()
        });
        let mut vol = piyo.percussion_track.base.vol as i16 / 4;
        // Drum sample volumes are weird in PiyoPiyo.
        // Volume of 1 is actually audible, but it's not so the case in PxTone.
        // TODO: Figure this out properly.
        // For now, we just clamp the volume to a reasonable(?) range.
        vol = vol.clamp(32, 96);
        song.events.push(Event {
            payload: EventPayload::Volume(vol),
            unit: UnitIdx(n_units + i),
            tick: 0,
        });
    }
    for (i, samp_data) in DRUM_SAMPLES.iter().enumerate() {
        let pcm = PcmData {
            ch: ptcow::ChNum::Mono,
            sps: 22050,
            num_samples: samp_data.len() as u32,
            smp: samp_data.to_vec(),
            ..Default::default()
        };
        let mut voice = Voice::from_data(ptcow::VoiceData::Pcm(pcm));
        voice.name = format!("Drum {i}");
        ins.voices.push(voice);
    }
    song.master.loop_points.repeat =
        ptcow::timing::tick_to_meas(piyo.repeat_range.start * 114, song.master.timing);
    song.master.loop_points.last = None;
    song.recalculate_length();
    song.events.sort();
    ptcow::rebuild_tones(ins, &mut herd.delays, &mut herd.overdrives, &song.master);
}
