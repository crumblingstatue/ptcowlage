pub enum FileOp {
    OpenProj,
    ImportVoices,
    ImportMidi,
    SaveProjAs,
    ImportPiyoPiyo,
    ImportOrganya,
}

pub const FILT_PTCOP: &str = "PxTone collage";
pub const FILT_MIDI: &str = "Midi file";
pub const FILT_PIYOPIYO: &str = "PiyoPiyo file";
pub const FILT_ORGANYA: &str = "Organya file";
