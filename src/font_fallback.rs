use {
    crate::app::Preferences,
    eframe::egui,
    std::{path::Path, sync::Arc},
};

pub fn install_ja_fallback_font(cc: &eframe::CreationContext, prefs: &mut Preferences) {
    let storage = cc.storage.unwrap();
    if let Some(path) = storage.get_string(Preferences::JP_FALLBACK) {
        install_fallback_font_from_path("ja_fallback", &cc.egui_ctx, path.as_ref());
        prefs.jp_fallback_font_path = path;
    }
}

fn install_fallback_font_from_path(name: &str, ctx: &egui::Context, path: &Path) {
    if path.exists() {
        let font_data = egui::FontData::from_owned(std::fs::read(path).unwrap());
        let mut font_defs = egui::FontDefinitions::default();
        font_defs.font_data.insert(name.into(), Arc::new(font_data));
        font_defs
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .push(name.into());
        font_defs
            .families
            .get_mut(&egui::FontFamily::Monospace)
            .unwrap()
            .push(name.into());
        ctx.set_fonts(font_defs);
    }
}
