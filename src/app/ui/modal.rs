use {
    crate::{app::ui::tabs::voices::SelectedSlot, audio_out::SongStateHandle},
    eframe::egui,
};

#[derive(Default)]
pub struct Modal {
    payload: Option<Payload>,
}

impl Modal {
    pub fn err(&mut self, msg: impl std::fmt::Display) {
        self.payload = Some(Payload::ErrMsg(msg.to_string()));
    }
    pub fn seek_to_sample(&mut self, t: ptcow::SampleT) {
        self.payload = Some(Payload::SeekToSamplePrompt(t));
    }
    pub(crate) fn replace_wave_data_slot(
        &mut self,
        voice_idx: ptcow::VoiceIdx,
        slot: SelectedSlot,
        with: ptcow::WaveData,
    ) {
        self.payload = Some(Payload::ReplaceWaveDataSlot {
            voice_idx,
            slot,
            with,
        });
    }
    pub fn update(&mut self, ctx: &egui::Context, song: &SongStateHandle) {
        if let Some(payload) = &mut self.payload {
            let mut close = false;
            egui::Modal::new("modal_popup".into()).show(ctx, |ui| match payload {
                Payload::ErrMsg(msg) => {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("！")
                                .color(egui::Color32::RED)
                                .strong()
                                .size(48.0),
                        );
                        ui.strong(&*msg);
                    });
                    ui.vertical_centered(|ui| {
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    });
                }
                Payload::SeekToSamplePrompt(samp) => {
                    ui.heading("Seek to sample");
                    ui.add(egui::DragValue::new(samp));
                    if ui.button("Seek").clicked() {
                        song.lock().unwrap().herd.seek_to_sample(*samp);
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                }
                Payload::ReplaceWaveDataSlot {
                    voice_idx,
                    slot,
                    with,
                } => {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.label("Replace existing data?");
                    ui.horizontal(|ui| {
                        if ui.button("Yes").clicked() {
                            voice_slot(&mut song.lock().unwrap().ins.voices, *voice_idx, *slot)
                                .data = ptcow::VoiceData::Wave(with.clone());
                            close = true;
                        }
                        if ui.button("No").clicked() {
                            close = true;
                        }
                    });
                }
            });
            if close {
                self.payload = None;
            }
        }
    }
}

fn voice_slot(
    voices: &mut ptcow::Voices,
    idx: ptcow::VoiceIdx,
    slot: SelectedSlot,
) -> &mut ptcow::VoiceSlot {
    let voice = &mut voices[idx];
    match slot {
        SelectedSlot::Base => &mut voice.base,
        SelectedSlot::Extra => voice.extra.as_mut().unwrap(),
    }
}

enum Payload {
    ErrMsg(String),
    SeekToSamplePrompt(ptcow::SampleT),
    ReplaceWaveDataSlot {
        voice_idx: ptcow::VoiceIdx,
        slot: SelectedSlot,
        with: ptcow::WaveData,
    },
}
