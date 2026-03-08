use {
    crate::audio_out::SongState,
    eframe::egui,
    rustc_hash::FxHashMap,
    std::{any::TypeId, collections::hash_map::Entry},
};

#[derive(Default)]
pub struct Windows {
    inner: FxHashMap<TypeId, Box<dyn Window>>,
}

impl Windows {
    pub fn toggle<T: Window + 'static + Default>(&mut self) {
        let type_id = TypeId::of::<T>();
        if let Entry::Vacant(e) = self.inner.entry(type_id) {
            e.insert(Box::new(T::default()));
        } else {
            self.inner.remove(&type_id);
        }
    }
    pub fn update(&mut self, ctx: &egui::Context, song: &mut SongState) {
        self.inner.retain(|_typeid, window| {
            let mut open = true;
            egui::Window::new(window.title())
                .open(&mut open)
                .show(ctx, |ui| {
                    window.update(ui, song);
                });
            open
        });
    }
}

pub trait Window {
    fn title(&self) -> &str;
    fn update(&mut self, ui: &mut egui::Ui, song: &mut SongState);
}

#[derive(Default)]
pub struct TitleAndCommentWindow;

impl Window for TitleAndCommentWindow {
    fn title(&self) -> &str {
        "Title and comment"
    }
    fn update(&mut self, ui: &mut egui::Ui, song: &mut SongState) {
        ui.strong("Title");
        ui.text_edit_singleline(&mut song.song.text.name);
        ui.strong("Comment");
        ui.text_edit_multiline(&mut song.song.text.comment);
    }
}

#[derive(Default)]
pub struct LogWindow;

impl Window for LogWindow {
    fn title(&self) -> &str {
        "Log viewer"
    }

    fn update(&mut self, ui: &mut egui::Ui, _song: &mut SongState) {
        egui_logger::logger_ui().show(ui);
    }
}
