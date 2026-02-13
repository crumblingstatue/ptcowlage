use {ptcow::EventPayload, std::collections::VecDeque};

pub enum Cmd {
    ReloadCurrentFile,
    OpenEventInEventsTab { index: usize },
    RemoveNoteAtIdx { idx: usize },
    // Replace the current audio thread (should be called after the audio output is reconfigured)
    ReplaceAudioThread,
    SaveCurrentFile,
    OpenVoice(ptcow::VoiceIdx),
    OverwriteEvent { idx: usize, payload: EventPayload },
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
}
