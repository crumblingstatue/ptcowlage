//! GUI PxTone player/editor

#![allow(
    // I keep having to undo the collapses suggested by clippy when I implement more features
    // and make the logic more complex
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    // Especially hard to avoid when prototyping
    clippy::too_many_arguments
)]

use {clap::Parser, eframe::egui, std::path::PathBuf};

mod app;
mod audio_out;
mod egui_ext;
mod evilscript;
mod font_fallback;
mod herd_ext;
mod organya;
mod piyopiyo;
mod pxtone_misc;
mod util;

#[derive(clap::Parser)]
struct CliArgs {
    #[arg(long)]
    midi_import: Option<PathBuf>,
    #[arg(long)]
    piyo_import: Option<PathBuf>,
    #[arg(long)]
    org_import: Option<PathBuf>,
    #[arg(long)]
    voice_import: Option<PathBuf>,
    /// Optionally open a PxTone collage (.ptcop) file on startup
    open: Option<PathBuf>,
    /// Execute EvilScript after loading the initial song
    #[arg(long)]
    evil: Option<String>,
}

fn main() {
    let opts = eframe::NativeOptions::default();
    let args = CliArgs::parse();
    eframe::run_native(
        "ptcowlage",
        opts,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1280., 720.)));
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Title("pxtone Cowlage".into()));
            font_fallback::install_ja_fallback_font(&cc.egui_ctx);
            let app = app::App::new(args);
            Ok(Box::new(app))
        }),
    )
    .unwrap();
}
