use {
    crate::app::ui::modal::Modal,
    egui_toast::ToastKind,
    ptcow::{ChNum, EventPayload, SourceSampleRate, VoiceIdx},
    std::collections::VecDeque,
};

pub enum Cmd {
    ReloadCurrentFile,
    OpenEventInEventsTab {
        index: usize,
    },
    RemoveNoteAtIdx {
        idx: usize,
    },
    // Replace the current audio thread (should be called after the audio output is reconfigured)
    ReplaceAudioThread,
    SaveCurrentFile,
    OpenVoice(ptcow::VoiceIdx),
    OverwriteEvent {
        idx: usize,
        payload: EventPayload,
    },
    InsertEvent {
        idx: usize,
        event: ptcow::Event,
    },
    SetActiveTab(crate::app::ui::Tab),
    SetEventsFilter(crate::app::ui::tabs::events::Filter),
    Toast {
        kind: ToastKind,
        text: String,
        duration: f64,
    },
    PromptImportPtVoice,
    PromptImportPtNoise,
    PromptImportAllPtcop,
    PromptSaveAs,
    PromptImportMidi,
    PromptImportPiyo,
    PromptImportOrg,
    PromptExportWav,
    PromptOpenPtcop,
    PromptReplacePtVoiceSingle(VoiceIdx),
    PromptReplacePtNoiseSingle(VoiceIdx),
    PromptReplaceWavSingle(VoiceIdx),
    ClearProject,
    #[cfg(not(target_arch = "wasm32"))]
    OpenPtcopFromPath {
        path: std::path::PathBuf,
    },
    ResetUnitVoice {
        unit: ptcow::UnitIdx,
        voice: VoiceIdx,
    },
    PromptExportPtnoise {
        voice: VoiceIdx,
    },
    PromptExportPtvoice {
        voice: VoiceIdx,
    },
    Modal(Box<dyn FnOnce(&mut Modal)>),
    PromptImportOggVorbis,
    ResetVoiceForUnitsWithVoiceIdx {
        idx: VoiceIdx,
    },
    PromptExportWavData {
        data: Vec<u8>,
        ch_num: ChNum,
        sample_rate: SourceSampleRate,
    },
}

impl Cmd {
    /// Returns a copy of this command if it's repeatable, `None` otherwise
    fn repeatable(&self) -> Option<Self> {
        match self {
            cmd @ (Cmd::ReloadCurrentFile
            | Cmd::SaveCurrentFile
            | Cmd::PromptImportPtVoice
            | Cmd::PromptImportPtNoise
            | Cmd::PromptImportAllPtcop
            | Cmd::PromptSaveAs
            | Cmd::PromptImportMidi
            | Cmd::PromptImportPiyo
            | Cmd::PromptImportOrg
            | Cmd::PromptExportWav
            | Cmd::PromptOpenPtcop
            | Cmd::PromptReplacePtNoiseSingle(_)
            | Cmd::PromptReplacePtVoiceSingle(_)) => Some(unsafe {
                // Avoid having to separately match each copiable variant by using a little unsafe

                // # Safety
                // These variants are trivially copiable
                std::ptr::read(cmd)
            }),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct CommandQueue {
    queue: VecDeque<Cmd>,
    last: Option<Cmd>,
}

impl CommandQueue {
    pub fn push(&mut self, cmd: Cmd) {
        self.last = cmd.repeatable();
        self.queue.push_back(cmd);
    }
    pub fn pop(&mut self) -> Option<Cmd> {
        self.queue.pop_front()
    }
    pub fn toast(&mut self, kind: ToastKind, msg: impl std::fmt::Display, duration: f64) {
        self.push(Cmd::Toast {
            kind,
            text: msg.to_string(),
            duration,
        });
    }
    pub fn modal(&mut self, f: impl FnOnce(&mut Modal) + 'static) {
        self.push(Cmd::Modal(Box::new(f)));
    }
    pub fn tab(&mut self, tab: crate::app::ui::Tab) {
        self.push(Cmd::SetActiveTab(tab));
    }

    pub(crate) fn repeat_last(&mut self) {
        if let Some(cmd) = &self.last {
            // We already know it's repeatable because it was added via `repeatable()`
            self.push(cmd.repeatable().unwrap());
        }
    }
}
