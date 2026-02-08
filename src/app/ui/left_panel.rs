use {
    crate::{
        app::{
            App,
            command_queue::CommandQueue,
            ui::{
                UiState, UnitPopupTab, UnitsCmd, handle_units_command, unit_color,
                unit_mute_unmute_all_ui, unit_popup_ctx_menu, unit_voice_img,
            },
        },
        audio_out::AuxAudioState,
        egui_ext::ImageExt,
    },
    eframe::egui,
    ptcow::{MooInstructions, SampleRate, Unit, UnitIdx},
};

pub struct LeftPanelState {
    unit_popup_tab: UnitPopupTab,
}

impl Default for LeftPanelState {
    fn default() -> Self {
        Self {
            unit_popup_tab: UnitPopupTab::Unit,
        }
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    let mut song = app.song.lock().unwrap();
    let song = &mut *song;
    let mut cmd = None;
    let n_units = song.herd.units.len();
    ui.horizontal(|ui| {
        ui.heading(format!("Units ({n_units})"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!song.herd.units.is_full(), egui::Button::new("+ New"))
                .clicked()
            {
                let unit = Unit {
                    name: format!("New unit ({})", song.herd.units.len()),
                    ..Default::default()
                };
                song.herd.units.push(unit);
            }
        });
    });
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 96.0)
        .auto_shrink(false)
        .show(ui, |ui| {
            for (i, unit) in song.herd.units.iter_mut().enumerate() {
                unit_ui(
                    app.out.rate,
                    &mut app.ui_state,
                    &mut app.aux_state,
                    ui,
                    &mut song.ins,
                    &mut cmd,
                    n_units,
                    i,
                    unit,
                    &mut app.cmd,
                );
            }
        });
    handle_units_command(cmd, song);
    unit_mute_unmute_all_ui(ui, &mut song.herd.units);
    ui.label("m: mute, s: solo");
    ui.label("h: hide, v: visual solo");
    if ui.button("Unhide all").clicked() {
        app.ui_state.piano_roll.hidden_units.clear();
    }
}

fn unit_ui(
    out_rate: SampleRate,
    ui_state: &mut UiState,
    aux_state: &mut Option<AuxAudioState>,
    ui: &mut egui::Ui,
    ins: &mut MooInstructions,
    cmd: &mut Option<UnitsCmd>,
    n_units: usize,
    i: usize,
    unit: &mut ptcow::Unit,
    app_cmd: &mut CommandQueue,
) {
    let c = unit_color(i);
    let n: i32 = unit.pan_time_bufs.iter().flatten().copied().sum();
    let tint = if n == 0 { egui::Color32::WHITE } else { c };
    ui.horizontal(|ui| {
        let mut any_hovered = false;
        if unit.mute {
            any_hovered |= ui.label("m").contains_pointer();
        }
        if ui_state.piano_roll.hidden_units.contains(&(i as u8)) {
            any_hovered |= ui.label("h").contains_pointer();
        }
        any_hovered |= ui
            .add(
                egui::Image::new(unit_voice_img(ins, unit))
                    .sense(egui::Sense::click())
                    .tint(tint)
                    .hflip(),
            )
            .contains_pointer();

        let re = ui.label(egui::RichText::new(&unit.name).color(c));
        if let Some(idx) = ui_state.shared.active_unit
            && idx.usize() == i
        {
            ui.painter().debug_rect(re.rect, egui::Color32::YELLOW, "");
        }
        if re.clicked() {
            ui_state.shared.active_unit = Some(UnitIdx(i as u8));
        }
        // Got to "Unit" tab on double click
        if re.double_clicked() {
            ui_state.tab = super::Tab::Unit;
        }
        unit_popup_ctx_menu(
            &re,
            UnitIdx(i as u8),
            unit,
            ins,
            cmd,
            &mut ui_state.left_panel.unit_popup_tab,
            out_rate,
            aux_state,
            &mut ui_state.voices,
            app_cmd,
        );
        any_hovered |= re.contains_pointer();
        if any_hovered {
            // Toggle unit solo/mute
            if ui.input(|inp| inp.key_pressed(egui::Key::S)) {
                *cmd = Some(UnitsCmd::ToggleSolo {
                    idx: UnitIdx(i as u8),
                });
            }
            if ui.input(|inp| inp.key_pressed(egui::Key::M)) {
                unit.mute ^= true;
            }
            // Toggle unit hide
            if ui.input(|inp| inp.key_pressed(egui::Key::H)) {
                if ui_state.piano_roll.hidden_units.contains(&(i as u8)) {
                    ui_state.piano_roll.hidden_units.remove(&(i as u8));
                } else {
                    ui_state.piano_roll.hidden_units.insert(i as u8);
                }
            }
            // Toggle visual solo
            if ui.input(|inp| inp.key_pressed(egui::Key::V)) {
                // We always remove ourselves from being hidden since it's solo
                ui_state.piano_roll.hidden_units.remove(&(i as u8));
                // All muted except one (us)
                let already_solo =
                    ui_state.piano_roll.hidden_units.len() == n_units.saturating_sub(1);
                // If already solo, unmute all units
                if already_solo {
                    ui_state.piano_roll.hidden_units.clear();
                } else {
                    // Insert all units
                    for i in 0..n_units {
                        ui_state.piano_roll.hidden_units.insert(i as u8);
                    }
                    // But remove self
                    ui_state.piano_roll.hidden_units.remove(&(i as u8));
                }
            }
        }
    });
}
