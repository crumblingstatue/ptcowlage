use {
    crate::audio_out::{SongState, prepare_song},
    ptcow::ChNum,
    std::io::Write,
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

pub fn write_wav<W: Write>(mut w: W, n_ch: ChNum, samples: &[i16]) -> std::io::Result<()> {
    let sample_rate = 44_100;
    let bits_per_sample = 16;
    let num_channels: u32 = n_ch as u32;
    let num_samples: u32 = samples.len() as u32;

    let byte_rate = sample_rate * num_channels * bits_per_sample / 8;
    let block_align = num_channels * bits_per_sample / 8;
    let data_size = num_samples * block_align;
    let chunk_size = 36 + data_size;

    // RIFF header
    w.write_all(b"RIFF")?;
    w.write_all(&chunk_size.to_le_bytes())?;
    w.write_all(b"WAVE")?;

    // fmt chunk
    w.write_all(b"fmt ")?;
    w.write_all(&(16u32).to_le_bytes())?; // PCM
    w.write_all(&(1u16).to_le_bytes())?; // Audio format = PCM
    w.write_all(&(num_channels as u16).to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&(block_align as u16).to_le_bytes())?;
    w.write_all(&(bits_per_sample as u16).to_le_bytes())?;

    // data chunk
    w.write_all(b"data")?;
    w.write_all(&data_size.to_le_bytes())?;

    // Audio samples
    w.write_all(bytemuck::cast_slice(samples))?;

    Ok(())
}

pub fn export_wav(song: &mut SongState) -> std::io::Result<Vec<u8>> {
    let mut samp_data = Vec::new();
    let mut buf = [0; 16_384];
    let mut wav_out = Vec::new();
    // Prepare non-looping moo
    prepare_song(song, false);
    // Make sure we can moo
    song.herd.moo_end = false;
    while song.herd.moo(&song.ins, &song.song, &mut buf, true) {
        samp_data.extend_from_slice(&buf);
    }
    write_wav(&mut wav_out, ChNum::Stereo, &samp_data)?;
    Ok(wav_out)
}
