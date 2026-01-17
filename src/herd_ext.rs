use ptcow::{Herd, Meas, MooInstructions, Song, timing};

pub trait HerdExt {
    fn seek_to_meas(&mut self, meas: Meas, song: &Song, ins: &MooInstructions);
}

impl HerdExt for Herd {
    fn seek_to_meas(&mut self, meas: Meas, song: &Song, ins: &MooInstructions) {
        self.seek_to_sample(timing::meas_to_sample(
            meas,
            ins.samples_per_tick,
            song.master.timing,
        ));
    }
}
