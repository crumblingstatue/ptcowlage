use ptcow::VoiceIdx;

#[derive(Clone, Copy, PartialEq, Eq)]
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
    ImportPtNoise,
    ImportPtVoice,
}

pub struct FileFilt {
    pub name: &'static str,
    pub ext: &'static str,
}
impl FileFilt {
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn web_filter(&self) -> String {
        [".", self.ext].concat()
    }
}

macro_rules! file_filts {
    ($($const_name:ident, $name:literal, $ext:literal;)*) => {
        $(
            pub const $const_name: FileFilt = FileFilt {
                name: $name,
                ext: $ext,
            };
        )+
    };
}

file_filts! {
    FILT_PTCOP, "PxTone collage", "ptcop";
    FILT_MIDI, "Midi file", "mid";
    FILT_PIYOPIYO, "PiyoPiyo file", "pmd";
    FILT_ORGANYA, "Organya file", "org";
    FILT_WAV, "WAVE file", "wav";
    FILT_SF2, "SoundFont2 file", "sf2";
    FILT_PTVOICE, "PxTone voice file", "ptvoice";
    FILT_PTNOISE, "PxTone noise file", "ptnoise";
}
