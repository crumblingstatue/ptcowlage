use {
    ptcow::{Herd, MooInstructions, MooPlan, SampleRate, Song},
    rustc_hash::FxHashMap,
    std::{
        cell::Cell,
        collections::hash_map::Entry,
        iter::zip,
        ops::RangeInclusive,
        sync::{Arc, Mutex},
    },
};

#[derive(Clone, Copy)]
pub struct OutParams {
    pub buf_size: usize,
    pub rate: SampleRate,
}

impl Default for OutParams {
    fn default() -> Self {
        Self {
            buf_size: 2048,
            rate: 44_100,
        }
    }
}

impl OutParams {
    /// A buffer size outside of this range doesn't make sense
    pub const SANE_BUF_SIZE_RANGE: RangeInclusive<usize> = 32..=65536;
    /// An output rate outside of this range doesn't make sense
    ///
    /// On Firefox browser, 8000 seems to be the minimum supported sample rate.
    /// Trying to set below that causes a crash, which we want to avoid
    pub const SANE_RATE_RANGE: RangeInclusive<SampleRate> = 8000..=65535;
    pub fn latency_ms(self) -> f32 {
        // Stereo output
        let ch = 2.0;
        // 16 bit samples
        let samp = 2.0;
        (self.buf_size as f32 * 1000.) / (ch * samp * self.rate as f32)
    }
}

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

impl SongState {
    pub fn new(sample_rate: SampleRate) -> Self {
        Self {
            herd: Herd::default(),
            song: Song::default(),
            ins: MooInstructions::new(sample_rate),
            pause: true,
            master_vol: 1.0,
        }
    }
    pub fn prepare(&mut self, sample_rate: SampleRate) {
        // We want to be prepared to moo before we spawn the audio thread, so we can toot and stuff.
        crate::audio_out::prepare_song(self, true);
        ptcow::rebuild_tones(
            &mut self.ins,
            sample_rate,
            &mut self.herd.delays,
            &mut self.herd.overdrives,
            &self.song.master,
        );
    }
}

pub fn prepare_song(song: &mut SongState, loop_: bool) {
    ptcow::moo_prepare(
        &mut song.ins,
        &mut song.herd,
        &song.song,
        &MooPlan {
            start_pos: ptcow::StartPosPlan::Sample(0),
            meas_end: None,
            meas_repeat: None,
            loop_,
        },
    );
}

/// Main ptcow audio thread that handles the playback of the PxTone music
pub fn spawn_ptcow_audio_thread(
    out_params: OutParams,
    song: SongStateHandle,
) -> tinyaudio::OutputDevice {
    let params = tinyaudio::OutputDeviceParameters {
        sample_rate: out_params.rate as usize,
        channels_count: 2,
        channel_sample_count: out_params.buf_size / 2,
    };
    let mut out_buf_s16: Vec<i16> = vec![0; out_params.buf_size];
    tinyaudio::run_output_device(params, move |out| {
        // Critical section (should be kept as small as possible)
        let mut song_g = song.lock().unwrap();
        let song: &mut SongState = &mut song_g;
        let master_vol = song.master_vol;
        let out_buf_mut_ref = &mut out_buf_s16;
        // INVARIANT: We assume `moo` never panics. Panicking is a bug.
        song.herd
            .moo(&song.ins, &song.song, out_buf_mut_ref, !song.pause);
        drop(song_g);
        // End critical section

        // Convert to output format and apply master volume, then write to out buffer
        for (src, dst) in zip(&out_buf_s16, &mut *out) {
            *dst = s16_to_f32(*src) * master_vol;
        }
    })
    .unwrap()
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
                AuxMsg::StopAll => {
                    playing.clear();
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
    StopAll,
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
