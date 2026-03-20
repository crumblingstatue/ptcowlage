use {
    crate::{
        app::{
            SongState,
            command_queue::CommandQueue,
            ui::{
                SharedUiState, img,
                modal::Modal,
                unit::{UnitsCmd, handle_units_command, unit_mute_unmute_all_ui},
                unit_color, voice_data_img,
            },
        },
        egui_ext::ImageExt as _,
    },
    eframe::egui,
    ptcow::UnitIdx,
};

pub fn ui(
    ui: &mut egui::Ui,
    song: &mut SongState,
    shared: &mut SharedUiState,
    app_cmd: &mut CommandQueue,
    app_modal: &mut Modal,
) {
    if !song.song.text.name.is_empty() {
        ui.strong(&song.song.text.name);
    }
    if !song.song.text.comment.is_empty() {
        ui.small(&song.song.text.comment);
    }
    ui.horizontal(|ui| {
        ui.style_mut().spacing.slider_width = ui.available_width() - 400.0;
        ui.label("Sample");
        let end = song.herd.smp_end;
        let re =
            ui.add(egui::Slider::new(&mut song.herd.smp_count, 0..=end).suffix(format!("/{end}")));
        if re.changed() {
            song.herd.seek_to_sample(song.herd.smp_count);
        }
        ui.label(format!(
            "Clock: {}",
            ptcow::current_tick(&song.herd, &song.ins)
        ));
        ui.label(format!(
            "Event: {}/{}",
            song.herd.evt_idx,
            song.song.events.len().saturating_sub(1)
        ));
    });

    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 32.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            playback_cows_ui(ui, song, shared, app_cmd, app_modal);
        });
    ui.label("Cows are interactive. m: mute, s: solo");

    ui.horizontal(|ui| {
        unit_mute_unmute_all_ui(ui, &mut song.herd.units);
        ui.separator();
    });
}

fn playback_cows_ui(
    ui: &mut egui::Ui,
    song: &mut SongState,
    shared: &mut SharedUiState,
    app_cmd: &mut CommandQueue,
    app_modal: &mut Modal,
) {
    let mut cmd = None;
    for (i, unit) in song.herd.units.enumerated_mut().chain(
        song.freeplay_assist_units
            .iter_mut()
            .enumerate()
            .map(|(i, u)| (UnitIdx(SongState::EXTRA_UNITS_START_IDX.0 + i as u8), u)),
    ) {
        ui.horizontal(|ui| {
            ui.set_height(32.0);
            ui.scope(|ui| {
                ui.set_width(120.0);
                ui.label(egui::RichText::new(&unit.name).color(unit_color(i)));
                if shared.active_unit == i {
                    ui.label("*");
                }
                if unit.mute {
                    ui.label("m");
                }
            });
            let re = ui.add(
                egui::Image::new(img::COW)
                    .hflip()
                    .sense(egui::Sense::click()),
            );
            if re.contains_pointer() {
                if ui.input(|inp| inp.pointer.primary_clicked()) {
                    shared.active_unit = i;
                }
                if ui.input(|inp| inp.key_pressed(egui::Key::S)) {
                    cmd = Some(UnitsCmd::ToggleSolo { idx: i });
                }
                if ui.input(|inp| inp.key_pressed(egui::Key::M)) {
                    unit.mute ^= true;
                }
            }
            let extra_voices = std::slice::from_ref(&song.preview_voice);
            crate::app::ui::unit::unit_popup_ctx_menu(
                &re,
                i,
                unit,
                &mut song.ins,
                extra_voices,
                &mut cmd,
                app_cmd,
                &song.song.events,
            );
            // Make the left cow's instrument represent the voice unit 1,
            // and right cow unit 2, if exists. Otherwise, right cow represents unit 1 as well.
            let opt_voice = song.ins.voices.get(unit.voice_idx, extra_voices);
            let vu1_img = opt_voice.map_or(img::X, |voice| voice_data_img(&voice.base.data));
            ui.add(egui::Image::new(vu1_img.clone()).hflip());
            let p = ui.painter();
            let mut offs = ui.cursor().left_center();
            for buf in &unit.pan_time_bufs {
                let mut points = Vec::new();
                for smp in buf {
                    #[expect(clippy::cast_precision_loss)]
                    let yoff = *smp as f32 / 512.0;
                    points.push(egui::pos2(offs.x, offs.y + yoff));
                    offs.x += 3.0;
                }
                p.line(points, egui::Stroke::new(1.0, egui::Color32::LIGHT_GRAY));
                offs.x += 16.0;
            }
            ui.add_space(408.0);
            ui.add(egui::Image::new(
                opt_voice
                    .and_then(|voice| voice.extra.as_ref())
                    .map_or(vu1_img, |slot| voice_data_img(&slot.data)),
            ));
            let re = ui.add(egui::Image::new(img::COW).sense(egui::Sense::click()));
            crate::app::ui::unit::unit_popup_ctx_menu(
                &re,
                i,
                unit,
                &mut song.ins,
                extra_voices,
                &mut cmd,
                app_cmd,
                &song.song.events,
            );
        });
    }
    handle_units_command(cmd, song, app_modal, shared);
}
