use ptcow::VoiceIdx;

#[derive(Clone, Copy)]
pub enum FileOp {
    OpenProj,
    ReplaceVoicesPtcop,
    ImportMidi,
    SaveProjAs,
    ImportPiyoPiyo,
    ImportOrganya,
    ExportWav,
    ReplaceSf2Single(VoiceIdx),
    ImportSf2Single,
}

pub const FILT_PTCOP: &str = "PxTone collage";
pub const FILT_MIDI: &str = "Midi file";
pub const FILT_PIYOPIYO: &str = "PiyoPiyo file";
pub const FILT_ORGANYA: &str = "Organya file";
pub const FILT_WAV: &str = "WAVE file";
pub const FILT_SF2: &str = "SoundFont2 file";
