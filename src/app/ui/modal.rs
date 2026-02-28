use {crate::audio_out::SongStateHandle, eframe::egui};

#[derive(Default)]
pub struct Modal {
    payload: Option<Payload>,
}

impl Modal {
    pub fn msg(&mut self, msg: impl std::fmt::Display) {
        self.payload = Some(Payload::Msg(msg.to_string()));
    }
    pub fn seek_to_sample(&mut self, t: ptcow::SampleT) {
        self.payload = Some(Payload::SeekToSamplePrompt(t));
    }
    pub fn update(&mut self, ctx: &egui::Context, song: &SongStateHandle) {
        if let Some(payload) = &mut self.payload {
            let mut close = false;
            egui::Modal::new("modal_popup".into()).show(ctx, |ui| match payload {
                Payload::Msg(msg) => {
                    ui.label(&*msg);
                    if ui.button("Close").clicked() {
                        close = true;
                    }
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
            });
            if close {
                self.payload = None;
            }
        }
    }
}

enum Payload {
    Msg(String),
    SeekToSamplePrompt(ptcow::SampleT),
}
