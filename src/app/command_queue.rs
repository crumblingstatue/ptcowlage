use {
    egui_toast::ToastKind,
    ptcow::{EventPayload, VoiceIdx},
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
    PromptImportSf2Sound,
    PromptReplaceAllPtcop,
    PromptReplaceSf2Single(VoiceIdx),
    PromptSaveAs,
    PromptImportMidi,
    PromptImportPiyo,
    PromptImportOrg,
    PromptExportWav,
    PromptOpenPtcop,
    PromptReplacePtVoiceSingle(VoiceIdx),
    PromptReplacePtNoiseSingle(VoiceIdx),
    ClearProject,
    #[cfg(not(target_arch = "wasm32"))]
    OpenPtcopFromPath {
        path: std::path::PathBuf,
    },
    ResetUnitVoice {
        unit: ptcow::UnitIdx,
        voice: VoiceIdx,
    },
}

#[derive(Default)]
pub struct CommandQueue {
    queue: VecDeque<Cmd>,
}

impl CommandQueue {
    pub fn push(&mut self, cmd: Cmd) {
        self.queue.push_back(cmd);
    }
    pub fn pop(&mut self) -> Option<Cmd> {
        self.queue.pop_front()
    }
    pub fn toast(&mut self, kind: ToastKind, text: String, duration: f64) {
        self.push(Cmd::Toast {
            kind,
            text,
            duration,
        });
    }
}
