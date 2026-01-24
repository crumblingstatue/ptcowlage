//! GUI PxTone player/editor

#![allow(
    // I keep having to undo the collapses suggested by clippy when I implement more features
    // and make the logic more complex
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    // Especially hard to avoid when prototyping
    clippy::too_many_arguments
)]

use std::path::PathBuf;

mod app;
mod audio_out;
mod egui_ext;
mod evilscript;
#[cfg(not(target_arch = "wasm32"))]
mod font_fallback;
mod herd_ext;
mod organya;
mod piyopiyo;
mod pxtone_misc;
mod util;
#[cfg(target_arch = "wasm32")]
mod web_glue;

#[cfg(not(target_arch = "wasm32"))]
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

// TODO: This is a hack, find a better solution
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
struct CliArgs {
    midi_import: Option<PathBuf>,
    piyo_import: Option<PathBuf>,
    org_import: Option<PathBuf>,
    voice_import: Option<PathBuf>,
    /// Optionally open a PxTone collage (.ptcop) file on startup
    open: Option<PathBuf>,
    /// Execute EvilScript after loading the initial song
    evil: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use {clap::Parser as _, eframe::egui};
    let opts = eframe::NativeOptions::default();
    let args = CliArgs::parse();
    eframe::run_native(
        "ptcowlage",
        opts,
        Box::new(|cc| {
            use crate::audio_out::OutParams;

            egui_extras::install_image_loaders(&cc.egui_ctx);
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1280., 720.)));
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Title("pxtone Cowlage".into()));
            font_fallback::install_ja_fallback_font(&cc.egui_ctx);
            let app = app::App::new(args, OutParams::default(), &[]);
            Ok(Box::new(app))
        }),
    )
    .unwrap();
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        use crate::app::BundledSongs;

        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        #[rustfmt::skip]
        static BUNDLED_SONGS: BundledSongs = &[
            ("The_Watcher_From_Afar.ptcop", include_bytes!("../bundled-songs/The_Watcher_From_Afar.ptcop")),
        ];

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| {
                    use crate::audio_out::OutParams;

                    egui_extras::install_image_loaders(&cc.egui_ctx);
                    let app = app::App::new(
                        CliArgs::default(),
                        OutParams {
                            // Web version is a bit slower, so let's use a bigger buf size
                            // in order to reduce audio glitching
                            buf_size: 4096,
                            rate: 44_100,
                        },
                        BUNDLED_SONGS,
                    );
                    // Enforce dark theme, as we don't support light theme for our custom colors
                    cc.egui_ctx.set_theme(eframe::egui::Theme::Dark);
                    Ok(Box::new(app))
                }),
            )
            .await;

        // Remove the loading text and spinner:
        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(_) => {
                    loading_text.remove();
                }
                Err(e) => {
                    loading_text.set_inner_html(
                        "<p> The app has crashed. See the developer console for details. </p>",
                    );
                    panic!("Failed to start eframe: {e:?}");
                }
            }
        }
    });
}
