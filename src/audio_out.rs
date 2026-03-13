use {
    ptcow::{Herd, MooInstructions, MooPlan, NoiseData, SampleRate, Song, Unit, Voice},
    std::{
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
        (self.buf_size as f32 * 1000.) / (ch * samp * f32::from(self.rate))
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
        let mut this = Self {
            herd: Herd::default(),
            song: Song::default(),
            ins: MooInstructions::new(sample_rate),
            pause: true,
            master_vol: 1.0,
        };
        // Set the end meas for new songs to make sure there is nice big area to play around with
        this.song.master.loop_points.last = Some(std::num::NonZero::new(100).unwrap());
        this.herd.units.push(Unit {
            name: "Cow".into(),
            ..Default::default()
        });
        let noise_data = NoiseData::from_ptnoise(include_bytes!("../res/cow.ptnoise")).unwrap();
        let mut voice = Voice::from_data(ptcow::VoiceData::Noise(noise_data));
        voice.name = "Moo".into();
        this.ins.voices.push(voice);
        this
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
