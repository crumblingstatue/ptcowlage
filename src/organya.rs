use std::num::NonZeroU32;

use ptcow::{
    Event, EventPayload, Herd, MooInstructions, PcmData, SampleRate, Song, Unit, UnitIdx, Voice,
    VoiceFlags,
};

fn org_tempo_to_bpm(tempo: u16, steps_per_beat: u8) -> f32 {
    60000. / (tempo as f32 * steps_per_beat as f32)
}
const DRUM_DATA: &[u8] = include_bytes!("../res/org-drums.pcm");
const WAVE_DATA: &[u8] = include_bytes!("../res/org-wave.pcm");

pub fn import(
    org: &organyacat::Song,
    herd: &mut Herd,
    song: &mut Song,
    ins: &mut MooInstructions,
    out_sample_rate: SampleRate,
) {
    song.master.timing.beats_per_meas = org.beats_per_measure;
    song.master.timing.bpm = org_tempo_to_bpm(org.tempo_ms, org.steps_per_beat);
    song.master.timing.ticks_per_beat = org.steps_per_beat as u16 * 120;
    let time_div = org.beats_per_measure as u32 * org.steps_per_beat as u32;
    song.master.loop_points.repeat = org.repeat_start / time_div;
    song.master.loop_points.last = NonZeroU32::new(org.repeat_end / time_div);
    let out_ev = &mut song.events.eves;
    out_ev.clear();
    herd.units.clear();
    ins.voices.clear();
    let mut unit_counter = 0;
    let base_key = 63 * 256;
    for (i, ch) in org.channels.iter().enumerate() {
        let ch_num = i + 1;
        // Skip empty channels
        if ch.events.is_empty() {
            continue;
        }
        let is_wave = i < 8;
        if is_wave {
            ins.voices.push(wave_voice(ch));
        } else {
            ins.voices.push(drum_voice(ch));
        }
        out_ev.push(Event {
            payload: EventPayload::SetVoice(ptcow::VoiceIdx(unit_counter)),
            unit: UnitIdx(unit_counter),
            tick: 0,
        });
        herd.units.push(Unit {
            name: format!("ch{ch_num}({})", ch.instrument),
            ..Default::default()
        });
        for ev in &ch.events {
            let tick = ev.position * 120;
            let unit = UnitIdx(unit_counter);
            // Drums need to last longer (?)
            let len_mul = if is_wave { 120 } else { 480 };
            out_ev.push(Event {
                payload: EventPayload::On {
                    duration: ev.length as u32 * len_mul,
                },
                unit,
                tick,
            });
            if ev.pitch != organyacat::PROPERTY_UNUSED {
                out_ev.push(Event {
                    payload: EventPayload::Key(base_key + ev.pitch as i32 * 256),
                    unit,
                    tick,
                });
            }
            if ev.volume != organyacat::PROPERTY_UNUSED {
                out_ev.push(Event {
                    payload: EventPayload::Volume(ev.volume as i16 / 2),
                    unit,
                    tick,
                });
            }
            if ev.pan != organyacat::PROPERTY_UNUSED {
                out_ev.push(Event {
                    payload: EventPayload::PanVol(ev.pan * 10),
                    unit,
                    tick,
                });
            }
        }
        unit_counter += 1;
    }
    out_ev.sort_by_key(|ev| ev.tick);
    ptcow::rebuild_tones(
        ins,
        out_sample_rate,
        &mut herd.delays,
        &mut herd.overdrives,
        &song.master,
    );
}

fn wave_voice(ch: &organyacat::Channel) -> Voice {
    let mut voice = Voice::default();
    voice.allocate::<false>();
    voice.name = format!("org wave {}", ch.instrument);
    let ins_offset = ch.instrument as usize * 256;
    let smp: Vec<u8> = WAVE_DATA[ins_offset..ins_offset + 256]
        .iter()
        .map(|samp| samp.wrapping_add(127))
        .collect();
    let pcm = PcmData {
        ch: ptcow::ChNum::Mono,
        sps: 44100,
        bps: ptcow::Bps::B8,
        num_samples: 256,
        smp,
    };
    voice.units[0].data = ptcow::VoiceData::Pcm(pcm);
    voice.units[0].flags.insert(VoiceFlags::WAVE_LOOP);
    voice.units[0].flags.insert(VoiceFlags::SMOOTH);
    // We tweak the basic key of the voices a bit to make the pitches for the instruments sound right.
    // This is probably a hack, but sounds close enough
    voice.units[0].basic_key = 17664 - (4 * 256);
    voice
}

fn drum_voice(ch: &organyacat::Channel) -> Voice {
    let mut voice = Voice::default();
    voice.allocate::<false>();
    let smp = find_drum_sample(ch.instrument).to_vec();
    voice.name = format!("org drum {}", ch.instrument);
    let pcm = PcmData {
        ch: ptcow::ChNum::Mono,
        sps: 22050,
        bps: ptcow::Bps::B8,
        num_samples: smp.len() as u32,
        smp,
    };
    voice.units[0].data = ptcow::VoiceData::Pcm(pcm);
    voice.units[0].flags.insert(VoiceFlags::SMOOTH);
    voice
}

fn find_drum_sample(n: u8) -> &'static [u8] {
    let mut data = DRUM_DATA;
    let mut i = 0;
    loop {
        let len = u32::from_le_bytes(data[..4].try_into().unwrap());
        if i == n {
            return &data[..len as usize];
        }
        data = &data[len as usize + 4..];
        i += 1;
    }
}
