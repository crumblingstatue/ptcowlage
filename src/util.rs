use {
    crate::audio_out::{SongState, prepare_song},
    hound::WavSpec,
    ptcow::{ChNum, SourceSampleRate},
    std::{
        io::{Seek, Write},
        sync::atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

/// 領域展開
/// Finds indices of items matching the `Coll` predicate in both directions from `center`.
/// Continues search while `Cont` predicate holds true.
/// The returned `Vec` also contains `center`
pub fn domain_expansion<T, Cont, Coll>(
    slice: &[T],
    center: usize,
    mut p_continue: Cont,
    mut p_collect: Coll,
) -> Vec<usize>
where
    Cont: FnMut(&T) -> bool,
    Coll: FnMut(&T) -> bool,
{
    let mut indices = vec![center];
    let mut cursor = center;
    loop {
        if cursor == 0 {
            break;
        }
        cursor -= 1;
        if !p_continue(&slice[cursor]) {
            break;
        }
        if p_collect(&slice[cursor]) {
            indices.push(cursor);
        }
    }
    cursor = center;
    loop {
        if cursor >= slice.len() {
            break;
        }
        cursor += 1;
        if !p_continue(&slice[cursor]) {
            break;
        }
        if p_collect(&slice[cursor]) {
            indices.push(cursor);
        }
    }
    indices
}

pub fn write_wav<W: Write + Seek>(
    w: W,
    n_ch: ChNum,
    samples: &[i16],
    sample_rate: SourceSampleRate,
) -> anyhow::Result<()> {
    let bits_per_sample: u16 = 16;
    let num_channels: u16 = n_ch as u16;
    let mut writer = hound::WavWriter::new(
        w,
        WavSpec {
            channels: num_channels,
            sample_rate,
            bits_per_sample,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for samp in samples {
        writer.write_sample(*samp)?;
    }
    writer.finalize()?;

    Ok(())
}

pub fn export_wav(
    song: &mut SongState,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> anyhow::Result<Vec<u8>> {
    let mut samp_data = Vec::new();
    let mut buf = [0; 8192];
    let mut wav_out = std::io::Cursor::new(Vec::new());
    // Prepare non-looping moo
    prepare_song(song, false);
    // Make sure we can moo
    song.herd.moo_end = false;
    while song
        .herd
        .moo(&mut song.ins, &mut song.song, &mut buf, true, &mut [], &[])
    {
        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("Cancelled");
        }
        samp_data.extend_from_slice(&buf);
        let progress_ratio = song.herd.smp_count as f32 / song.herd.smp_end as f32;
        progress.store(progress_ratio.to_bits(), Ordering::Relaxed);
    }
    write_wav(
        &mut wav_out,
        ChNum::Stereo,
        &samp_data,
        song.ins.out_sample_rate.into(),
    )?;
    Ok(wav_out.into_inner())
}

pub trait HashSetExt<T> {
    fn toggle(&mut self, item: &T);
}

impl<T: Eq + core::hash::Hash + Clone, S: std::hash::BuildHasher> HashSetExt<T>
    for std::collections::HashSet<T, S>
{
    fn toggle(&mut self, item: &T) {
        if self.contains(item) {
            self.remove(item);
        } else {
            self.insert(item.clone());
        }
    }
}
