use {
    crate::pxtone_misc::{hat_close_voice, square_wave_voice},
    midly::{MetaMessage, MidiMessage, TrackEventKind, num::u7},
    ptcow::{Event, EventPayload, Herd, MooInstructions, Song, Unit, UnitIdx, VoiceIdx},
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

const DRUM_CH: u8 = 9;
// "Special" drum program to map to a drum instrument
const DRUM_PRG: u8 = 255;

struct ChannelState {
    rpn_lsb: u8,
    rpn_msb: u8,
    pitch_bend: f64,
    pitch_bend_range_semitones: u8,
    last_key: Option<midly::num::u7>,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            rpn_lsb: 0,
            rpn_msb: 0,
            pitch_bend: 0.0,
            pitch_bend_range_semitones: 2,
            last_key: None,
        }
    }
}

/// Unit index to channel mapping
#[derive(Default)]
struct ChannelMapping {
    vec: Vec<u8>,
}

impl ChannelMapping {
    fn get_or_insert_for_ch(&mut self, ch: midly::num::u4) -> UnitIdx {
        if let Some(pos) = self.vec.iter().position(|ch2| *ch2 == ch.as_int()) {
            UnitIdx(pos as u8)
        } else {
            self.vec.push(ch.as_int());
            UnitIdx((self.vec.len() - 1) as u8)
        }
    }
    fn into_iter(self) -> impl Iterator<Item = u8> {
        self.vec.into_iter()
    }
}

type UsedPrograms = FxHashMap<u8, VoiceIdx>;

/// Write midi song to pxtone
#[expect(
    clippy::unnecessary_wraps,
    reason = "Needs this signature due to fn pointer"
)]
pub fn write_midi_to_pxtone(
    mid_data: &[u8],
    herd: &mut Herd,
    song: &mut Song,
    ins: &mut MooInstructions,
) -> anyhow::Result<()> {
    let mut used_programs: UsedPrograms = FxHashMap::default();
    let (header, track_iter) = midly::parse(mid_data).unwrap();
    let tracks = track_iter.collect_tracks().unwrap();
    let ticks_per_beat = match header.timing {
        midly::Timing::Metrical(u15) => u15.as_int(),
        midly::Timing::Timecode(_fps, _) => todo!(),
    };
    song.master.timing.bpm = guess_tempo(&tracks).map_or(120.0, ms_per_beat_to_bpm);
    song.events.eves.clear();
    song.master.timing.ticks_per_beat = ticks_per_beat;
    let mut ch_map = ChannelMapping::default();
    let mut channel_states: FxHashMap<u8, ChannelState> = FxHashMap::default();
    for track in &tracks {
        let mut clock = 0;
        for (ev_idx, event) in track.iter().enumerate() {
            // The delta is how much after the previous event this current event is,
            // so we start by incrementing the clock
            clock += event.delta.as_int();
            match event.kind {
                TrackEventKind::Midi { channel, message } => {
                    let unit = ch_map.get_or_insert_for_ch(channel);
                    let state = channel_states.entry(channel.as_int()).or_insert_with(|| {
                        // Here we can put code that runs once on each new channel.

                        // Always insert drum voice for the drum channel, even if there is no
                        // midi program change event that does it
                        if channel == DRUM_CH {
                            program_change(
                                song,
                                &mut used_programs,
                                clock,
                                channel,
                                unit,
                                DRUM_PRG,
                            );
                        } else {
                            // Do the same, but use "piano" program for other channels
                            program_change(song, &mut used_programs, clock, channel, unit, 0);
                        }
                        ChannelState::default()
                    });
                    match message {
                        MidiMessage::NoteOff { .. } => {
                            // We calculate how long notes last in the `NoteOn` event, so we do nothing here
                        }
                        MidiMessage::NoteOn { key, vel } => {
                            state.last_key = Some(key);
                            push_key_event(song, unit, clock, state, key);
                            // If velocity is zero, we don't want to emit an `On` event.
                            if vel == 0 {
                                //continue;
                            }
                            song.events.eves.push(Event {
                                payload: EventPayload::Velocity(i16::from(vel.as_int())),
                                unit,
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
                                            MidiMessage::NoteOff { key: key2, .. }
                                                if key2 == key =>
                                            {
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
                                unit,
                                tick: clock,
                            });
                        }
                        MidiMessage::ProgramChange { program } => {
                            program_change(
                                song,
                                &mut used_programs,
                                clock,
                                channel,
                                unit,
                                program.as_int(),
                            );
                        }
                        MidiMessage::PitchBend { bend } => {
                            state.pitch_bend = bend.as_f64();
                            if let Some(last) = state.last_key {
                                push_key_event(song, unit, clock, state, last);
                            }
                        }
                        MidiMessage::Controller { controller, value } => {
                            match controller.as_int() {
                                // 7: "Channel volume"
                                // 11: "Expression" or secondary volume controller
                                7 | 11 => {
                                    song.events.eves.push(Event {
                                        payload: EventPayload::Volume(i16::from(value.as_int())),
                                        unit,
                                        tick: clock,
                                    });
                                }
                                6 => {
                                    if state.rpn_lsb == 0 && state.rpn_msb == 0 {
                                        log::info!("Pitch bend range msb: {value}");
                                        state.pitch_bend_range_semitones = value.as_int();
                                    } else {
                                        log::warn!(
                                            "Unhandled rpn {} {}",
                                            state.rpn_lsb,
                                            state.rpn_msb
                                        );
                                    }
                                }
                                38 => {
                                    if state.rpn_lsb == 0 && state.rpn_msb == 0 {
                                        log::info!("Pitch bend range lsb: {value}");
                                    } else {
                                        log::warn!(
                                            "Unhandled rpn {} {}",
                                            state.rpn_lsb,
                                            state.rpn_msb
                                        );
                                    }
                                }
                                100 => {
                                    state.rpn_lsb = value.as_int();
                                }
                                101 => {
                                    state.rpn_msb = value.as_int();
                                }
                                _ => {
                                    log::info!("c {controller} = {value}");
                                }
                            }
                        }
                        _ => log::warn!("Unhandled mid msg: {message:?}"),
                    }
                }
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
    }

    herd.units.clear();
    for ch in ch_map.into_iter() {
        let name = if ch == DRUM_CH {
            "drum ch".into()
        } else {
            format!("ch{ch}")
        };
        herd.units.push(Unit {
            name,
            ..Default::default()
        });
    }

    replace_voices(ins, used_programs);
    // Unset the last point (let it be calculated by PxTone)
    song.master.loop_points.last = None;

    // PxTone events seem to need to be stored in order of increasing clock value
    song.events.eves.sort_by_key(|ev| ev.tick);
    Ok(())
}

fn program_change(
    song: &mut Song,
    used_programs: &mut UsedPrograms,
    clock: u32,
    channel: midly::num::u4,
    unit: UnitIdx,
    program: u8,
) {
    let len = used_programs.len();
    let program = if channel.as_int() == DRUM_CH {
        DRUM_PRG
    } else {
        program
    };
    let idx = used_programs
        .entry(program)
        .or_insert(VoiceIdx(len.try_into().unwrap()));
    log::info!("Instrument change of {channel} to {program}");
    song.events.eves.push(Event {
        payload: EventPayload::SetVoice(*idx),
        unit,
        tick: clock,
    });
}

/// Replace the existing voices with a voice mapped for each "program"
fn replace_voices(ins: &mut MooInstructions, used_programs: UsedPrograms) {
    ins.voices.clear();
    let mut pairs: Vec<_> = used_programs.into_iter().collect();
    pairs.sort_by_key(|pair| pair.1.0);
    for (prg, _) in pairs {
        if prg == DRUM_PRG {
            let mut voice = hat_close_voice();
            voice.name = "drum".into();
            ins.voices.push(voice);
        } else {
            let mut voice = square_wave_voice();
            let nam = PROGRAM_NAMES[prg as usize];
            voice.name = format!("[{prg}] {nam}");
            ins.voices.push(voice);
        }
    }
    // If there were no program events or whatever, we still want at least one voice
    if ins.voices.is_empty() {
        let mut voice = square_wave_voice();
        voice.name = "fallback".into();
        ins.voices.push(voice);
    }
}

fn push_key_event(song: &mut Song, unit_idx: UnitIdx, clock: u32, state: &ChannelState, key: u7) {
    let base_key = 27;
    let raw_key = i32::from(key.as_int() + base_key) * 256;
    let bend_mod = state.pitch_bend * f64::from(state.pitch_bend_range_semitones) * 256.0;
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

const PROGRAM_NAMES: [&str; 128] = [
    "acoustic gr.",
    "brght acous.",
    "electric gr.",
    "honky-tonk",
    "rhodes",
    "chorus piano",
    "harpsichord",
    "clavinet",
    "celesta",
    "glockenspiel",
    "music box",
    "vibraphone",
    "marimba",
    "xylophone",
    "tub. bells",
    "dulcimer",
    "hammond org.",
    "percus. org.",
    "rock organ",
    "church organ",
    "reed organ",
    "accordion",
    "harmonica",
    "tango accord",
    "aco. nyl. g.",
    "aco. stel g.",
    "ele. jazz g.",
    "ele. clan g.",
    "ele. mutd g.",
    "overdrv g.",
    "dist. g.",
    "guitar har.",
    "acoustic bs.",
    "fingered bs.",
    "plucked bs.",
    "fretless bs.",
    "slap bs. 1",
    "slap bs. 2",
    "synth bs. 1",
    "synth bs. 2",
    "violin",
    "viola",
    "cello",
    "contrabass",
    "tremo. str.",
    "pizzi. str.",
    "orches. harp",
    "timpani",
    "str. ensm. 1",
    "str. ensm. 2",
    "synth str. 1",
    "synth str. 2",
    "choir aah",
    "choir ooh",
    "synth voice",
    "orch. hit",
    "trumpet",
    "trombone",
    "tuba",
    "muted trm.",
    "french horn",
    "brass sct.",
    "synth brs 1",
    "synth brs 2",
    "soprano sax",
    "alto sax",
    "tenor sax",
    "baritone sax",
    "oboe",
    "eng. horn",
    "bassoon",
    "clarinet",
    "piccolo",
    "flute",
    "recorder",
    "pan flute",
    "bottle blow",
    "shakuhachi",
    "whistle",
    "ocarina",
    "square",
    "sawtooth",
    "calliope ld",
    "chiff ld",
    "charang ld",
    "voice ld",
    "fifths ld",
    "bass ld",
    "new age pad",
    "warm pad",
    "polysth pad",
    "choir pad",
    "bowed pad",
    "metallic pad",
    "halo pad",
    "sweep pad",
    "rain",
    "soundtrack",
    "crystal",
    "atmosphere",
    "brightness",
    "goblins",
    "echoes",
    "sci-fi",
    "sitar",
    "banjo",
    "shamisen",
    "koto",
    "kalimba",
    "bagpipe",
    "fiddle",
    "shanai",
    "tinkle bell",
    "agogo",
    "steel drums",
    "woodblock",
    "taiko drum",
    "melodic tom",
    "synth drum",
    "rever. cym.",
    "guitar fret",
    "breath",
    "seashore",
    "bird",
    "telephone",
    "helicopter",
    "applause",
    "gun shot",
];
