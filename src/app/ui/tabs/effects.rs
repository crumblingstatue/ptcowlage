use {
    crate::{app::ui::group_idx_slider, audio_out::SongState},
    eframe::egui,
    ptcow::{Delay, DelayUnit, Overdrive, SampleRate, Song},
};

pub fn ui(ui: &mut egui::Ui, song: &mut SongState, out_rate: SampleRate) {
    if ui.button("Clear effects").clicked() {
        song.herd.delays.clear();
        song.herd.overdrives.clear();
    }
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let mut msg = None;
            for (i, dela) in song.herd.delays.iter_mut().enumerate() {
                delay_ui(ui, &song.song, out_rate, i, dela, &mut msg);
            }

            if ui.button("+ Add delay").clicked() {
                ui.separator();
                song.herd.delays.push(Delay::default());
            }
            for (i, ovr) in song.herd.overdrives.iter_mut().enumerate() {
                ovr_ui(ui, i, ovr, &mut msg);
            }
            if ui.button("+ Add overdrive").clicked() {
                song.herd.overdrives.push(Overdrive::default());
            }
            if let Some(msg) = msg {
                match msg {
                    EffectsUiMsg::RemoveDelay { idx } => {
                        song.herd.delays.remove(idx);
                    }
                    EffectsUiMsg::RemoveOvr { idx } => {
                        let _ = song.herd.overdrives.remove(idx);
                    }
                }
            }
        });
}

fn ovr_ui(ui: &mut egui::Ui, i: usize, ovr: &mut Overdrive, msg: &mut Option<EffectsUiMsg>) {
    ui.horizontal(|ui| {
        ui.heading(format!("Overdrive {i}"));
        if ui.button("-").clicked() {
            *msg = Some(EffectsUiMsg::RemoveOvr { idx: i });
        }
        ui.add(egui::Checkbox::new(&mut ovr.on, "on"));
        ui.label(egui::RichText::new("⚠").color(egui::Color32::YELLOW))
            .on_hover_text("Careful, too large amplitude can be an eardrum massaging experience");
    });

    ui.indent("ovr", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.style_mut().spacing.slider_width = ui.available_width() - 240.0;
            ui.label("Amp");
            ui.add(egui::Slider::new(
                &mut ovr.amp_mul,
                Overdrive::AMP_VALID_RANGE,
            ));
            ui.end_row();
            ui.label("Cut");
            ui.add(
                egui::Slider::new(&mut ovr.cut_percent, Overdrive::CUT_VALID_RANGE)
                    .suffix("%")
                    .drag_value_speed(0.001),
            );
            ui.end_row();
            ui.label("Group");
            group_idx_slider(ui, &mut ovr.group);
            // Rebuild it continuously, it's not that expensive to rebuild
            ovr.rebuild();
        });
    });
}

enum EffectsUiMsg {
    RemoveDelay { idx: usize },
    RemoveOvr { idx: usize },
}

fn delay_ui(
    ui: &mut egui::Ui,
    song: &Song,
    out_rate: u16,
    i: usize,
    dela: &mut Delay,
    msg: &mut Option<EffectsUiMsg>,
) {
    ui.horizontal(|ui| {
        ui.heading(format!("Delay {i}"));
        if ui.button("-").clicked() {
            *msg = Some(EffectsUiMsg::RemoveDelay { idx: i });
        }
    });

    ui.indent("delay", |ui| {
        ui.style_mut().spacing.slider_width = ui.available_width() - 240.0;
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Unit");
            changed |= ui
                .selectable_value(&mut dela.unit, DelayUnit::Beat, "beat")
                .clicked();
            changed |= ui
                .selectable_value(&mut dela.unit, DelayUnit::Meas, "meas")
                .clicked();
            changed |= ui
                .selectable_value(&mut dela.unit, DelayUnit::Second, "second")
                .clicked();
        });
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label("Frequency");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut dela.freq, 0.1..=4096.0)
                            .logarithmic(true)
                            .update_while_editing(false),
                    )
                    .changed();
            });
            ui.horizontal(|ui| {
                ui.label("Rate");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut dela.rate, 0..=100)
                            .integer()
                            .update_while_editing(false),
                    )
                    .changed();
            });

            ui.label(format!(
                "Buffer size: {} samples ({} bytes)",
                dela.buf_len(),
                dela.buf_len() * 4
            ));
        });
        ui.label("Group");
        group_idx_slider(ui, &mut dela.group);
        if changed {
            dela.rebuild(
                song.master.timing.beats_per_meas,
                song.master.timing.bpm,
                out_rate,
            );
        };
    });
}
