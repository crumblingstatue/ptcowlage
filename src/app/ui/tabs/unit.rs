use {
    crate::{
        app::{
            command_queue::CommandQueue,
            ui::{SharedUiState, handle_units_command, unit_ui},
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
) {
    let mut cmd = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            if let Some(unit_idx) = shared.active_unit {
                let unit = &mut song.herd.units[unit_idx.usize()];
                unit_ui(ui, unit_idx, unit, &song.ins, &mut cmd, app_cmd);
            }
        });
    handle_units_command(cmd, song);
}
