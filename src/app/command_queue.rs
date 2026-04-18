use {
    crate::app::ui::{file_ops::FileOp, modal::Modal},
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
    ClearProject,
    #[cfg(not(target_arch = "wasm32"))]
    OpenPtcopFromPath {
        path: std::path::PathBuf,
    },
    ResetUnitVoice {
        unit: ptcow::UnitIdx,
        voice: VoiceIdx,
    },
    Modal(Box<dyn FnOnce(&mut Modal)>),
    ResetVoiceForUnitsWithVoiceIdx {
        idx: VoiceIdx,
    },
    FilePrompt(FileOp),
}

impl Cmd {
    /// Returns a copy of this command if it's repeatable, `None` otherwise
    fn repeatable(&self) -> Option<Self> {
        match self {
            Self::ReloadCurrentFile => Some(Self::ReloadCurrentFile),
            Self::SaveCurrentFile => Some(Self::SaveCurrentFile),
            Self::FilePrompt(op) => {
                match op {
                    FileOp::OpenProj
                    | FileOp::ImportAllPtcop
                    | FileOp::ImportMidi
                    | FileOp::SaveProjAs
                    | FileOp::ImportPiyoPiyo
                    | FileOp::ImportOrganya
                    | FileOp::ExportWav
                    | FileOp::ReplacePtVoiceSingle(..)
                    | FileOp::ReplacePtNoiseSingle(..)
                    | FileOp::ReplaceWavSingle(..)
                    | FileOp::ImportPtNoise
                    | FileOp::ImportPtVoice
                    | FileOp::ExportPtvoice { .. }
                    | FileOp::ExportPtnoise { .. }
                    | FileOp::ImportOggVorbis => {
                        // Avoid having to separately match each copiable variant by using a little unsafe

                        // # Safety
                        // These variants are trivially copiable
                        Some(unsafe { std::ptr::read(self) })
                    }
                    FileOp::ExportWavData { .. } => None,
                }
            }
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
