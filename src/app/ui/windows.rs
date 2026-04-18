use {
    crate::{app::Preferences, audio_out::SongState},
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
    pub fn update(&mut self, ctx: &egui::Context, song: &mut SongState, prefs: &mut Preferences) {
        self.inner.retain(|_typeid, window| {
            let mut open = true;
            egui::Window::new(window.title())
                .open(&mut open)
                .show(ctx, |ui| {
                    window.update(ui, song, prefs);
                });
            open
        });
    }
}

pub trait Window {
    fn title(&self) -> &str;
    fn update(&mut self, ui: &mut egui::Ui, song: &mut SongState, prefs: &mut Preferences);
}

#[derive(Default)]
pub struct TitleAndCommentWindow;

impl Window for TitleAndCommentWindow {
    fn title(&self) -> &'static str {
        "Title and comment"
    }
    fn update(&mut self, ui: &mut egui::Ui, song: &mut SongState, _prefs: &mut Preferences) {
        ui.strong("Title");
        ui.text_edit_singleline(&mut song.song.text.name);
        ui.strong("Comment");
        ui.text_edit_multiline(&mut song.song.text.comment);
    }
}

#[derive(Default)]
pub struct LogWindow;

impl Window for LogWindow {
    fn title(&self) -> &'static str {
        "Log viewer"
    }

    fn update(&mut self, ui: &mut egui::Ui, _song: &mut SongState, _prefs: &mut Preferences) {
        egui_logger::logger_ui().show(ui);
    }
}

#[derive(Default)]
pub struct PreferencesWindow {
    #[cfg(not(target_arch = "wasm32"))]
    file_dia: egui_file_dialog::FileDialog,
}

impl Window for PreferencesWindow {
    fn title(&self) -> &'static str {
        "Preferences"
    }

    fn update(&mut self, ui: &mut egui::Ui, _song: &mut SongState, prefs: &mut Preferences) {
        #[cfg(not(target_arch = "wasm32"))]
        ui.horizontal(|ui| {
            ui.label("Japanese fallback font");
            ui.add(
                egui::TextEdit::singleline(&mut prefs.jp_fallback_font_path)
                    .hint_text("Path to font"),
            );
            if ui.button("...").clicked() {
                self.file_dia.pick_file();
            }
        });
        ui.separator();
        ui.checkbox(
            &mut prefs.midi_auto_poly_migrate,
            "Auto poly-migrate on midi import",
        );
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.file_dia.update(ui.ctx());
            if let Some(path) = self.file_dia.take_picked() {
                match path.to_str() {
                    Some(s) => s.clone_into(&mut prefs.jp_fallback_font_path),
                    None => prefs.jp_fallback_font_path = "<path not valid utf-8>".into(),
                }
            }
        }
    }
}
