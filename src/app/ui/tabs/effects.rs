use {
    crate::{
        app::ui::{SharedUiState, group_idx_slider},
        audio_out::SongState,
    },
    eframe::egui,
    ptcow::{Delay, DelayUnit, GroupIdx, Overdrive, SampleRate, Song, Unit, UnitIdx},
};

#[derive(Default)]
pub struct EffectsUiState {
    tab: Tab,
}

#[derive(Default, PartialEq)]
enum Tab {
    #[default]
    Delays,
    Overdrives,
}

pub fn ui(
    ui: &mut egui::Ui,
    song: &mut SongState,
    out_rate: SampleRate,
    ui_state: &mut EffectsUiState,
    shared: &mut SharedUiState,
) {
    ui.horizontal(|ui| {
        ui.selectable_value(
            &mut ui_state.tab,
            Tab::Delays,
            format!("Delays ({})", song.herd.delays.len()),
        );
        ui.selectable_value(
            &mut ui_state.tab,
            Tab::Overdrives,
            format!("Overdrives ({})", song.herd.overdrives.len()),
        );
        ui.separator();
        if ui
            .add_enabled(
                !song.herd.delays.is_full(),
                egui::Button::new("+ Add delay"),
            )
            .clicked()
        {
            ui.separator();
            let mut delay = Delay::default();
            // Set some not too terrible sounding defaults
            delay.rate = 30;
            delay.freq = 2.0;
            delay.rebuild(
                song.song.master.timing.beats_per_meas,
                song.song.master.timing.bpm,
                out_rate,
            );
            song.herd.delays.push(delay);
        }
        if ui
            .add_enabled(
                !song.herd.overdrives.is_full(),
                egui::Button::new("+ Add overdrive"),
            )
            .clicked()
        {
            song.herd.overdrives.push(Overdrive::default());
        }
        if ui.button("Clear effects").clicked() {
            song.herd.delays.clear();
            song.herd.overdrives.clear();
        }
    });

    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let mut msg = None;
            match ui_state.tab {
                Tab::Delays => {
                    for (i, dela) in song.herd.delays.iter_mut().enumerate() {
                        ui.add_space(32.0);
                        delay_ui(
                            ui,
                            &song.song,
                            out_rate,
                            i,
                            dela,
                            &mut msg,
                            &song.herd.units,
                            shared,
                        );
                    }
                }
                Tab::Overdrives => {
                    for (i, ovr) in song.herd.overdrives.iter_mut().enumerate() {
                        ui.add_space(32.0);
                        ovr_ui(ui, i, ovr, &mut msg, &song.herd.units, shared);
                    }
                }
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

fn ovr_ui(
    ui: &mut egui::Ui,
    i: usize,
    ovr: &mut Overdrive,
    msg: &mut Option<EffectsUiMsg>,
    units: &[Unit],
    shared: &mut SharedUiState,
) {
    ui.horizontal(|ui| {
        ui.strong(i.to_string());
        if ui.button("-").clicked() {
            *msg = Some(EffectsUiMsg::RemoveOvr { idx: i });
        }
        ui.add(egui::Checkbox::new(&mut ovr.on, "on"));
        ui.label(egui::RichText::new("⚠").color(egui::Color32::YELLOW))
            .on_hover_text("Careful, too large amplitude can be an eardrum massaging experience");
        ui.separator();
        group_ui(&mut ovr.group, units, shared, ui);
    });

    ui.group(|ui| {
        ui.style_mut().spacing.slider_width = ui.available_width() - 240.0;
        ui.horizontal(|ui| {
            ui.label("Amp");
            ui.add(egui::Slider::new(
                &mut ovr.amp_mul,
                Overdrive::AMP_VALID_RANGE,
            ));
        });
        ui.horizontal(|ui| {
            ui.label("Cut");
            ui.add(
                egui::Slider::new(&mut ovr.cut_percent, Overdrive::CUT_VALID_RANGE)
                    .suffix("%")
                    .drag_value_speed(0.001),
            );
        });

        // Rebuild it continuously, it's not that expensive to rebuild
        ovr.rebuild();
    });
}

fn group_ui(group: &mut GroupIdx, units: &[Unit], shared: &mut SharedUiState, ui: &mut egui::Ui) {
    let mut g_hover = ui.label("Group").hovered();
    g_hover |= group_idx_slider(ui, group).hovered();
    if g_hover {
        for (i, unit) in units.iter().enumerate() {
            if unit.group == *group {
                shared.highlight_set.insert(UnitIdx(i as u8));
            }
        }
    }
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
    units: &[Unit],
    shared: &mut SharedUiState,
) {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.strong(i.to_string());
        if ui.button("-").clicked() {
            *msg = Some(EffectsUiMsg::RemoveDelay { idx: i });
        }
        ui.separator();
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
        ui.separator();
        group_ui(&mut dela.group, units, shared, ui);
    });

    ui.style_mut().spacing.slider_width = ui.available_width() - 240.0;

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
    if changed {
        dela.rebuild(
            song.master.timing.beats_per_meas,
            song.master.timing.bpm,
            out_rate,
        );
    }
}
