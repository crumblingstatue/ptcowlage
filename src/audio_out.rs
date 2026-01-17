use {
    ptcow::{Herd, MooInstructions, MooPlan, SampleRate, Song},
    rustc_hash::FxHashMap,
    std::{
        cell::Cell,
        collections::hash_map::Entry,
        iter::zip,
        panic::AssertUnwindSafe,
        sync::{Arc, Mutex},
    },
};

pub type SongStateHandle = Arc<Mutex<SongState>>;

/// The state shared between the main thread and the audio output thread
/// that's responsible for playing the song.
pub struct SongState {
    pub herd: Herd,
    pub song: Song,
    pub ins: MooInstructions,
    pub pause: bool,
    pub master_vol: f32,
}

pub fn prepare_song(song: &mut SongState) {
    ptcow::moo_prepare(
        &mut song.ins,
        &mut song.herd,
        &song.song,
        &MooPlan {
            start_pos: ptcow::StartPosPlan::Sample(0),
            meas_end: None,
            meas_repeat: None,
            loop_: true,
        },
    );
}

/// Main ptcow audio thread that handles the playback of the PxTone music
pub fn spawn_ptcow_audio_thread(
    out_rate: SampleRate,
    out_buf_size: usize,
    song: SongStateHandle,
) -> tinyaudio::OutputDevice {
    let params = tinyaudio::OutputDeviceParameters {
        sample_rate: out_rate as usize,
        channels_count: 2,
        channel_sample_count: out_buf_size / 2,
    };
    let mut out_buf_s16: Vec<i16> = vec![0; out_buf_size];
    tinyaudio::run_output_device(params, move |out| {
        // Critical section (should be kept as small as possible)
        let mut song_g = song.lock().unwrap();
        let song: &mut SongState = &mut song_g;
        let master_vol = song.master_vol;
        let out_buf_mut_ref = &mut out_buf_s16;
        if let Err(e) = std::panic::catch_unwind(AssertUnwindSafe(move || {
            song.herd
                .moo(&song.ins, &song.song, out_buf_mut_ref, !song.pause);
        })) {
            eprintln!("Audio playback panicked: {e:?}");
            reset_song(&mut song_g);
            song_g.pause = true;
            return;
        }
        drop(song_g);
        // End critical section

        // Convert to output format and apply master volume, then write to out buffer
        for (src, dst) in zip(&out_buf_s16, &mut *out) {
            *dst = s16_to_f32(*src) * master_vol;
        }
    })
    .unwrap()
}

/// Put the song in a sane state so it won't panic when trying to moo
fn reset_song(song: &mut SongState) {
    ptcow::moo_prepare(
        &mut song.ins,
        &mut song.herd,
        &song.song,
        &MooPlan {
            start_pos: ptcow::StartPosPlan::Sample(0),
            meas_end: None,
            meas_repeat: None,
            loop_: true,
        },
    );
}

fn s16_to_f32(src: i16) -> f32 {
    f32::from(src) / 32768.0
}

/// Auxiliary audio thread for playing additional sounds on top of the PxTone music playback
pub fn spawn_aux_audio_thread(out_rate: SampleRate, out_buf_size: usize) -> AuxAudioState {
    let params = tinyaudio::OutputDeviceParameters {
        sample_rate: out_rate as usize,
        channels_count: 2,
        channel_sample_count: out_buf_size / 2,
    };
    let (send, recv) = std::sync::mpsc::channel();
    let mut playing: FxHashMap<AuxAudioKey, SamplePlayer> = FxHashMap::default();
    let dev = tinyaudio::run_output_device(params, move |out_buf| {
        out_buf.fill(0.0);
        match recv.try_recv() {
            Ok(msg) => match msg {
                AuxMsg::PlaySamples16 { key, sample_data } => match playing.entry(key) {
                    Entry::Occupied(mut occupied_entry) => {
                        occupied_entry.get_mut().samp_data = sample_data
                    }
                    Entry::Vacant(vacant_entry) => {
                        vacant_entry.insert(SamplePlayer::new(sample_data));
                    }
                },
                AuxMsg::StopAudio { key } => {
                    playing.remove(&key);
                }
            },
            Err(e) => match e {
                std::sync::mpsc::TryRecvError::Empty => {}
                std::sync::mpsc::TryRecvError::Disconnected => todo!(),
            },
        }
        for player in playing.values_mut() {
            // Do nothing if sample data is empty.
            if player.samp_data.is_empty() {
                break;
            }
            let mut n_rendered = 0;
            for (src, dst) in zip(&player.samp_data[player.cursor..], &mut *out_buf) {
                // Mix in the sample
                *dst += s16_to_f32(*src);
                n_rendered += 1;
            }
            player.cursor += n_rendered;
            // Wrap around when cursor got to the end
            // INVARIANT: `samp_data` not empty
            player.cursor %= player.samp_data.len();
        }
    })
    .unwrap();
    AuxAudioState {
        _device: dev,
        send,
        key_counter: Cell::new(0),
    }
}

pub struct AuxAudioState {
    _device: tinyaudio::OutputDevice,
    pub send: std::sync::mpsc::Sender<AuxMsg>,
    key_counter: Cell<AuxAudioKey>,
}

impl AuxAudioState {
    pub fn next_key(&self) -> AuxAudioKey {
        let key = self.key_counter.get();
        self.key_counter.set(key + 1);
        key
    }
}

// Key for an aux audio
pub type AuxAudioKey = u64;

pub enum AuxMsg {
    /// Insert sample data for playback (or replace existing sample data for `key`)
    ///
    /// TODO: Maybe it makes sense to have separate play (that resets cursor) and update (that doesn't)
    PlaySamples16 {
        key: AuxAudioKey,
        sample_data: Vec<i16>,
    },
    StopAudio {
        key: AuxAudioKey,
    },
}

struct SamplePlayer {
    samp_data: Vec<i16>,
    cursor: usize,
}

impl SamplePlayer {
    pub fn new(samp_data: Vec<i16>) -> Self {
        Self {
            samp_data,
            cursor: 0,
        }
    }
}
