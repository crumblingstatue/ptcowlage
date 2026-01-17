use {
    eframe::egui,
    std::{path::Path, sync::Arc},
};

fn ja_fallback_font_path(coll: &mut fontique::Collection) -> Option<Arc<Path>> {
    let fam = coll
        .fallback_families(fontique::FallbackKey::new(fontique::Script(*b"Kana"), None))
        .next()?;
    let fam = coll.family(fam)?;
    let def_font = fam.default_font()?;
    match &def_font.source().kind {
        fontique::SourceKind::Memory(_blob) => None,
        fontique::SourceKind::Path(path) => Some(path.clone()),
    }
}

pub fn install_ja_fallback_font(ctx: &egui::Context) {
    let mut coll = fontique::Collection::new(fontique::CollectionOptions {
        shared: false,
        system_fonts: true,
    });
    if let Some(path) = ja_fallback_font_path(&mut coll) {
        install_fallback_font_from_path("ja_fallback", ctx, &path);
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
