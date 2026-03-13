use {
    crate::{
        app::{
            App,
            command_queue::CommandQueue,
            ui::{
                UiState,
                unit::{
                    UnitsCmd, handle_units_command, unit_mute_unmute_all_ui, unit_popup_ctx_menu,
                },
                unit_color, unit_voice_img,
            },
        },
        egui_ext::ImageExt,
        util::HashSetExt as _,
    },
    eframe::egui::{
        self,
        containers::menu::{MenuButton, MenuConfig},
    },
    ptcow::{EveList, MooInstructions, Unit, UnitIdx, Voice},
    rustc_hash::FxHashSet,
};

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
                let mut unit = Unit {
                    name: format!("New unit ({})", song.herd.units.len()),
                    ..Default::default()
                };
                unit.reset_voice(
                    &song.ins,
                    app.ui_state.voices.selected_idx,
                    song.song.master.timing,
                    std::slice::from_ref(&song.preview_voice),
                );
                let toot_idx = song.herd.units.len();
                song.herd.units.push(unit);
                app.ui_state.shared.active_unit = UnitIdx(toot_idx);
            }
        });
    });
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 96.0)
        .auto_shrink(false)
        .show(ui, |ui| {
            for (i, unit) in song.herd.units.enumerated_mut() {
                unit_ui(
                    &mut app.ui_state,
                    ui,
                    &mut song.ins,
                    std::slice::from_ref(&song.preview_voice),
                    &mut cmd,
                    n_units,
                    i,
                    unit,
                    &mut app.cmd,
                    &song.song.events,
                );
            }
            ui.separator();
            unit_ui(
                &mut app.ui_state,
                ui,
                &mut song.ins,
                std::slice::from_ref(&song.preview_voice),
                &mut cmd,
                n_units,
                UnitIdx(255),
                &mut song.voice_test_unit,
                &mut app.cmd,
                &song.song.events,
            );
        });
    handle_units_command(cmd, song, &mut app.modal);
    ui.checkbox(&mut app.ui_state.left.select_mode, "Select mode");
    if !app.ui_state.left.selected_units.is_empty() {
        let button = MenuButton::new("Actions").config(
            MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
        );
        button.ui(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut app.ui_state.left.batch_rename_buf)
                        .hint_text("Name"),
                );
                if ui.button("Batch rename").clicked() {
                    let mut indices: Vec<UnitIdx> =
                        app.ui_state.left.selected_units.iter().copied().collect();
                    indices.sort_by_key(|idx| idx.0);
                    for (num, idx) in indices.into_iter().enumerate() {
                        song.herd.units[idx].name =
                            format!("{}{num:02}", app.ui_state.left.batch_rename_buf);
                    }
                }
            });
        });
    }
    unit_mute_unmute_all_ui(ui, &mut song.herd.units);
    ui.label("m: mute, s: solo");
    ui.label("h: hide, v: visual solo");
    if ui.button("Unhide all").clicked() {
        app.ui_state.piano_roll.hidden_units.clear();
    }
    app.ui_state.shared.highlight_set.clear();
}

#[derive(Default)]
pub struct LeftPanelState {
    select_mode: bool,
    selected_units: FxHashSet<UnitIdx>,
    batch_rename_buf: String,
}

fn unit_ui(
    ui_state: &mut UiState,
    ui: &mut egui::Ui,
    ins: &mut MooInstructions,
    extra_voices: &[Voice],
    cmd: &mut Option<UnitsCmd>,
    n_units: u8,
    i: UnitIdx,
    unit: &mut ptcow::Unit,
    app_cmd: &mut CommandQueue,
    evelist: &EveList,
) {
    let c = unit_color(i);
    let n: i32 = unit.pan_time_bufs.iter().flatten().copied().sum();
    ui.horizontal(|ui| {
        let mut any_hovered = false;
        if ui_state.left.select_mode {
            let mut selected = ui_state.left.selected_units.contains(&i);
            if ui.checkbox(&mut selected, "").clicked() {
                ui_state.left.selected_units.toggle(&i);
            }
        }
        if unit.mute {
            any_hovered |= ui.label("m").contains_pointer();
        }
        if ui_state.piano_roll.hidden_units.contains(&i) {
            any_hovered |= ui.label("h").contains_pointer();
        }
        any_hovered |= ui
            .add(
                egui::Image::new(unit_voice_img(ins, extra_voices, unit))
                    .sense(egui::Sense::click())
                    .hflip(),
            )
            .contains_pointer();

        let re = ui.label(egui::RichText::new(&unit.name).color(c));
        if n != 0 {
            // We need to ensure min <= max when clamping, otherwise panic
            let max = f32::max(4.0, ui.available_width());
            let w = (n.abs() as f32 / 5000.0).clamp(4.0, max);
            let left_center = ui.cursor().left_center();
            ui.painter().line_segment(
                [left_center, left_center + egui::vec2(w, 0.0)],
                egui::Stroke::new(3.0, c),
            );
        }
        if ui_state.shared.active_unit == i {
            ui.painter().debug_rect(re.rect, egui::Color32::YELLOW, "");
        }
        if ui_state.shared.highlight_set.contains(&i) {
            ui.painter().debug_rect(re.rect, egui::Color32::WHITE, "");
        }
        if re.clicked() {
            ui_state.shared.active_unit = i;
        }
        // Got to "Unit" tab on double click
        if re.double_clicked() {
            ui_state.tab = super::Tab::Unit;
        }
        unit_popup_ctx_menu(&re, i, unit, ins, extra_voices, cmd, app_cmd, evelist);
        any_hovered |= re.contains_pointer();
        if any_hovered {
            // Toggle unit solo/mute
            if ui.input(|inp| inp.key_pressed(egui::Key::S)) {
                *cmd = Some(UnitsCmd::ToggleSolo { idx: i });
            }
            if ui.input(|inp| inp.key_pressed(egui::Key::M)) {
                unit.mute ^= true;
            }
            // Toggle unit hide
            if ui.input(|inp| inp.key_pressed(egui::Key::H)) {
                if ui_state.piano_roll.hidden_units.contains(&i) {
                    ui_state.piano_roll.hidden_units.remove(&i);
                } else {
                    ui_state.piano_roll.hidden_units.insert(i);
                }
            }
            // Toggle visual solo
            if ui.input(|inp| inp.key_pressed(egui::Key::V)) {
                // We always remove ourselves from being hidden since it's solo
                ui_state.piano_roll.hidden_units.remove(&i);
                // All muted except one (us)
                let already_solo =
                    ui_state.piano_roll.hidden_units.len() as u8 == n_units.saturating_sub(1);
                // If already solo, unmute all units
                if already_solo {
                    ui_state.piano_roll.hidden_units.clear();
                } else {
                    // Insert all units
                    for i in 0..n_units {
                        ui_state.piano_roll.hidden_units.insert(UnitIdx(i));
                    }
                    // But remove self
                    ui_state.piano_roll.hidden_units.remove(&i);
                }
            }
        }
    });
}
