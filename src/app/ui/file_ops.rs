use ptcow::{ChNum, SourceSampleRate, VoiceIdx};

#[derive(Clone, PartialEq, Eq)]
pub enum FileOp {
    OpenProj,
    ImportAllPtcop,
    ImportMidi,
    SaveProjAs,
    ImportPiyoPiyo,
    ImportOrganya,
    ExportWav,
    ReplacePtVoiceSingle(VoiceIdx),
    ReplacePtNoiseSingle(VoiceIdx),
    ReplaceWavSingle(VoiceIdx),
    ImportPtNoise,
    ImportPtVoice,
    ExportPtvoice {
        voice: VoiceIdx,
    },
    ExportPtnoise {
        voice: VoiceIdx,
    },
    ImportOggVorbis,
    ExportWavData {
        data: Vec<u8>,
        ch_num: ChNum,
        sample_rate: SourceSampleRate,
    },
}

impl FileOp {
    /// If true, this is a save prompt, else it's an open prompt
    pub fn is_save(&self) -> bool {
        match self {
            FileOp::OpenProj
            | FileOp::ImportAllPtcop
            | FileOp::ImportMidi
            | FileOp::ImportPiyoPiyo
            | FileOp::ImportOrganya
            | FileOp::ReplacePtVoiceSingle(..)
            | FileOp::ReplacePtNoiseSingle(..)
            | FileOp::ReplaceWavSingle(..)
            | FileOp::ImportPtNoise
            | FileOp::ImportPtVoice
            | FileOp::ImportOggVorbis => false,
            FileOp::SaveProjAs
            | FileOp::ExportWav
            | FileOp::ExportPtvoice { .. }
            | FileOp::ExportPtnoise { .. }
            | FileOp::ExportWavData { .. } => true,
        }
    }
    pub fn filt(&self) -> FileFilt {
        match self {
            FileOp::ImportMidi => FILT_MIDI,
            FileOp::OpenProj | FileOp::ImportAllPtcop | FileOp::SaveProjAs => FILT_PTCOP,
            FileOp::ImportPiyoPiyo => FILT_PIYOPIYO,
            FileOp::ImportOrganya => FILT_ORGANYA,
            FileOp::ExportWav | FileOp::ReplaceWavSingle(..) | FileOp::ExportWavData { .. } => {
                FILT_WAV
            }
            FileOp::ReplacePtVoiceSingle(..)
            | FileOp::ImportPtVoice
            | FileOp::ExportPtvoice { .. } => FILT_PTVOICE,
            FileOp::ReplacePtNoiseSingle(..)
            | FileOp::ImportPtNoise
            | FileOp::ExportPtnoise { .. } => FILT_PTNOISE,
            FileOp::ImportOggVorbis => FILT_OGG,
        }
    }
}

#[derive(Clone, Copy)]
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
    FILT_OGG, "Ogg/Vorbis file", "ogg";
    FILT_SF2, "SoundFont2 file", "sf2";
    FILT_PTVOICE, "PxTone voice file", "ptvoice";
    FILT_PTNOISE, "PxTone noise file", "ptnoise";
}
