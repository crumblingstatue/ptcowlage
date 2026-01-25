use {
    crate::{
        app::ui::{SharedUiState, handle_units_command, unit_ui},
        audio_out::SongState,
    },
    eframe::egui,
    ptcow::Unit,
};

pub fn ui(ui: &mut egui::Ui, shared: &mut SharedUiState, song: &mut SongState) {
    let mut cmd = None;
    if ui.button("+ Add").clicked() {
        let unit = Unit {
            name: format!("New unit ({})", song.herd.units.len()),
            ..Default::default()
        };
        song.herd.units.push(unit);
    }
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            if let Some(unit_idx) = shared.active_unit {
                let unit = &mut song.herd.units[unit_idx.usize()];
                unit_ui(ui, unit_idx, unit, &song.ins, &mut cmd);
            }
        });
    handle_units_command(cmd, song);
}
