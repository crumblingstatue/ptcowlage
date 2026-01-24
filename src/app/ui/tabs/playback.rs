use {
    crate::{
        app::{
            SongState,
            ui::{
                FreeplayPianoState, UnitPopupTab, UnitsCmd, handle_units_command, img,
                tabs::voices::VoicesUiState, unit_color, unit_mute_unmute_all_ui,
            },
        },
        audio_out::AuxAudioState,
        egui_ext::ImageExt as _,
    },
    eframe::egui,
    ptcow::{SampleRate, UnitIdx},
};

pub fn ui(
    ui: &mut egui::Ui,
    song: &mut SongState,
    ui_state: &mut PlaybackUiState,
    piano_state: &mut FreeplayPianoState,
    dst_sps: SampleRate,
    aux: &mut Option<AuxAudioState>,
    voices_ui_state: &mut VoicesUiState,
) {
    if !song.song.text.name.is_empty() {
        ui.label(&song.song.text.name);
    }
    if !song.song.text.comment.is_empty() {
        ui.label(&song.song.text.comment);
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
            song.song.events.eves.len().saturating_sub(1)
        ));
    });

    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 32.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            playback_cows_ui(
                ui,
                song,
                ui_state,
                piano_state,
                dst_sps,
                aux,
                voices_ui_state,
            );
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
    ui_state: &mut PlaybackUiState,
    piano_state: &mut FreeplayPianoState,
    out_rate: SampleRate,
    aux: &mut Option<AuxAudioState>,
    voices_ui_state: &mut VoicesUiState,
) {
    let mut cmd = None;
    for (i, unit) in song.herd.units.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.set_height(32.0);
            ui.scope(|ui| {
                ui.set_width(120.0);
                ui.label(egui::RichText::new(&unit.name).color(unit_color(i)));
                if piano_state.toot == Some(UnitIdx(i as u8)) {
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
                    piano_state.toot = Some(UnitIdx(i as u8));
                }
                if ui.input(|inp| inp.key_pressed(egui::Key::S)) {
                    cmd = Some(UnitsCmd::ToggleSolo {
                        idx: UnitIdx(i as u8),
                    });
                }
                if ui.input(|inp| inp.key_pressed(egui::Key::M)) {
                    unit.mute ^= true;
                }
            }

            crate::app::ui::unit_popup_ctx_menu(
                &re,
                UnitIdx(i as u8),
                unit,
                &mut song.ins,
                &mut cmd,
                &mut ui_state.unit_popup_tab,
                out_rate,
                aux,
                voices_ui_state,
            );

            macro_rules! inst_img {
                () => {
                    crate::app::ui::unit_voice_img(&song.ins, unit)
                };
            }
            ui.add(egui::Image::new(inst_img!()).hflip());
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
            ui.add(egui::Image::new(inst_img!()));
            let re = ui.add(egui::Image::new(img::COW).sense(egui::Sense::click()));
            crate::app::ui::unit_popup_ctx_menu(
                &re,
                UnitIdx(i as u8),
                unit,
                &mut song.ins,
                &mut cmd,
                &mut ui_state.unit_popup_tab,
                out_rate,
                aux,
                voices_ui_state,
            );
        });
    }
    handle_units_command(cmd, song);
}

pub struct PlaybackUiState {
    unit_popup_tab: UnitPopupTab,
}

impl Default for PlaybackUiState {
    fn default() -> Self {
        Self {
            unit_popup_tab: UnitPopupTab::Unit,
        }
    }
}
