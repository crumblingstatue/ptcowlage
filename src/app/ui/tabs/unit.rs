//! UI code for the Unit tab

use {
    crate::{
        app::{
            command_queue::CommandQueue,
            ui::{
                SharedUiState,
                modal::Modal,
                unit::{handle_units_command, unit_ui},
            },
        },
        audio_out::SongState,
    },
    eframe::egui,
};

pub fn ui(
    ui: &mut egui::Ui,
    shared: &mut SharedUiState,
    song: &mut SongState,
    app_cmd: &mut CommandQueue,
    app_modal: &mut Modal,
) {
    let mut cmd = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let unit = song
                .herd
                .units
                .get_mut(shared.active_unit)
                .unwrap_or(&mut song.freeplay_assist_units[0]);
            unit_ui(
                ui,
                shared.active_unit,
                unit,
                &song.ins,
                std::slice::from_ref(&song.preview_voice),
                &mut cmd,
                app_cmd,
                &song.song.events,
            );
        });
    handle_units_command(cmd, song, app_modal, shared);
}
