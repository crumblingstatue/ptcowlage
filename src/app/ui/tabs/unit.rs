//! UI code for the Unit tab

use {
    crate::{
        app::{
            ModalPayload,
            command_queue::CommandQueue,
            ui::{
                SharedUiState,
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
    app_modal_payload: &mut Option<ModalPayload>,
) {
    let mut cmd = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| match shared.active_unit {
            Some(unit_idx) => {
                let Some(unit) = song.herd.units.get_mut(unit_idx.usize()) else {
                    ui.label("Invalid selected unit");
                    return;
                };
                unit_ui(
                    ui,
                    unit_idx,
                    unit,
                    &song.ins,
                    &mut cmd,
                    app_cmd,
                    &song.song.events,
                );
            }
            None => {
                ui.label("Select a unit from the left panel");
            }
        });
    handle_units_command(cmd, song, app_modal_payload);
}
