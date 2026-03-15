use {
    crate::{
        app::{
            command_queue::{Cmd, CommandQueue},
            just_load_ptnoise, just_load_ptvoice,
            ui::{
                SharedUiState, envelope_edit_widget, file_ops::FileOp, img, unit_color,
                voice_data_img, voice_img, waveform_edit_widget_16_bit_interleaved_stereo,
            },
        },
        audio_out::SongState,
        pxtone_misc::{
            hat_close_voice, reset_voice_for_units_with_voice_idx, square_wave, square_wave_voice,
        },
    },
    arrayvec::ArrayVec,
    bitflags::Flags as _,
    eframe::egui::{self, AtomExt, collapsing_header::CollapsingState},
    ptcow::{
        Bps, ChNum, EnvPt, EnvelopeSrc, Master, MooInstructions, NoiseDesignOscillator,
        NoiseDesignUnit, NoiseTable, NoiseType, OsciArgs, OsciPt, SampleRate, Voice, VoiceData,
        VoiceFlags, VoiceIdx, WaveData, WaveDataPoints, noise_to_pcm,
    },
    std::path::PathBuf,
};

#[derive(Default)]
pub struct VoicesUiState {
    pub selected_idx: VoiceIdx,
    sel_slot: SelectedSlot,
    dragged_idx: Option<VoiceIdx>,
    inst_sub: SubSliceUi,
    inst_env_sub: SubSliceUi,
    selected_noise_unit: usize,
    /// A "reset slot", and 3 manually saved slots for quicksaving/loading the currently edited voice
    save_slots: [Option<Voice>; 4],
    last_hovered_wave_idx: Option<usize>,
    file_dia_prev_sel: Option<PathBuf>,
    instance_tab: InstanceTab,
}

#[derive(Default, PartialEq)]
enum InstanceTab {
    #[default]
    Samples,
    Envelope,
}

impl VoicesUiState {
    /// "Soft reset" the state when clicking a new voice
    pub fn soft_reset(
        &mut self,
        song_ins: &MooInstructions,
        extra_voices: &[Voice],
        song_master: &Master,
        voice_test_unit: &mut ptcow::Unit,
    ) {
        self.sel_slot = SelectedSlot::Base;
        self.inst_sub = SubSliceUi::default();
        self.inst_env_sub = SubSliceUi::default();
        self.selected_noise_unit = 0;
        self.save_slots = [const { None }; 4];
        if let Some(voice) = song_ins.voices.get(self.selected_idx, extra_voices) {
            self.save_slots[0] = Some(voice.clone());
        }
        voice_test_unit.reset_voice(
            song_ins,
            self.selected_idx,
            song_master.timing,
            extra_voices,
        );
    }
}

#[derive(Default, PartialEq, Hash, Clone, Copy)]
#[repr(u8)]
pub enum SelectedSlot {
    #[default]
    Base = 0,
    Extra = 1,
}

trait AtomExtExt<'a> {
    fn smol(self) -> egui::Atom<'a>;
}

impl<'a, T: AtomExt<'a>> AtomExtExt<'a> for T {
    fn smol(self) -> egui::Atom<'a> {
        self.atom_size(egui::vec2(16.0, 16.0))
    }
}

pub fn ui(
    ui: &mut egui::Ui,
    song: &mut SongState,
    ui_state: &mut VoicesUiState,
    shared: &mut SharedUiState,
    out_rate: SampleRate,
    app_cmd: &mut CommandQueue,
    #[cfg(not(target_arch = "wasm32"))] app_file_dia: &mut egui_file_dialog::FileDialog,
) {
    let mut op = None;
    ui.horizontal_wrapped(|ui| {
        ui.menu_button("☰", |ui| {
            ui.menu_button("✴ New", |ui| {
                if ui.button((img::SAXO.smol(), "Wave")).clicked() {
                    let mut voice = square_wave_voice();
                    voice.name = format!("Wave {}", song.ins.voices.len());
                    song.ins.voices.push(voice);
                    let idx = VoiceIdx(song.ins.voices.len() - 1);
                    ui_state.selected_idx = idx;
                    ui_state.soft_reset(
                        &song.ins,
                        &[],
                        &song.song.master,
                        &mut song.voice_test_unit,
                    );
                    reset_voice_for_units_with_voice_idx(song, idx);
                }
                if ui.button((img::DRUM.smol(), "Noise")).clicked() {
                    let mut voice = hat_close_voice();
                    voice.name = format!("Noise {}", song.ins.voices.len());
                    song.ins.voices.push(voice);
                    let idx = VoiceIdx(song.ins.voices.len() - 1);
                    ui_state.selected_idx = idx;
                    ui_state.soft_reset(
                        &song.ins,
                        &[],
                        &song.song.master,
                        &mut song.voice_test_unit,
                    );
                    reset_voice_for_units_with_voice_idx(song, idx);
                }
            });
            ui.menu_button(" Import", |ui| {
                if ui.button((img::COW.smol(), "All from .ptcop...")).clicked() {
                    app_cmd.push(Cmd::PromptImportAllPtcop);
                }
                if ui.button((img::SAXO.smol(), ".ptvoice")).clicked() {
                    app_cmd.push(Cmd::PromptImportPtVoice);
                }
                if ui.button((img::DRUM.smol(), ".ptnoise")).clicked() {
                    app_cmd.push(Cmd::PromptImportPtNoise);
                }
                if ui.button((img::FISH.smol(), ".ogg (vorbis)")).clicked() {
                    app_cmd.push(Cmd::PromptImportOggVorbis);
                }
            });
            ui.separator();
            if ui.button("✖ Clear all voices").clicked() {
                song.ins.voices.clear();
            }
        });
        for (i, voice) in song.ins.voices.enumerated() {
            let img = voice_img(voice);
            let button = egui::Button::selectable(ui_state.selected_idx == i, (img, &voice.name))
                .sense(egui::Sense::click_and_drag());
            let re = ui.add(button);
            re.context_menu(|ui| {
                if ui.button("Duplicate").clicked() {
                    op = Some(VoiceUiOp::Duplicate(i));
                }
            });
            if re.clicked() {
                ui_state.selected_idx = i;
                ui_state.soft_reset(
                    &song.ins,
                    std::slice::from_ref(&song.preview_voice),
                    &song.song.master,
                    &mut song.voice_test_unit,
                );
            }
            if re.drag_started() {
                ui_state.dragged_idx = Some(i);
            }
            if let Some(dragged_idx) = ui_state.dragged_idx
                && re.contains_pointer()
            {
                ui.painter().rect_stroke(
                    re.rect,
                    2.0,
                    egui::Stroke::new(1.0, egui::Color32::YELLOW),
                    egui::StrokeKind::Outside,
                );
                if ui.input(|inp| inp.pointer.primary_released()) {
                    ui_state.dragged_idx = None;
                    op = Some(VoiceUiOp::Swap(dragged_idx, i));
                }
            }
            if re.hovered() {
                for (unit_i, unit) in song.herd.units.enumerated() {
                    if unit.voice_idx == i {
                        shared.highlight_set.insert(unit_i);
                    }
                }
            }
        }
    });
    ui.separator();
    if let Some(voice) = song.ins.voices.get_mut(ui_state.selected_idx) {
        voice_ui(
            ui,
            voice,
            ui_state.selected_idx,
            &mut op,
            out_rate,
            ui_state,
            shared,
            &mut song.herd,
            &mut song.voice_test_unit,
            app_cmd,
        );
    }
    if let Some(op) = op {
        match op {
            VoiceUiOp::MoveUp(idx) => {
                let voice = song.ins.voices.remove(idx.usize());
                song.ins.voices.insert(idx.usize().saturating_sub(1), voice);
            }
            VoiceUiOp::MoveDown(idx) => {
                let voice = song.ins.voices.remove(idx.usize());
                song.ins.voices.insert(idx.usize() + 1, voice);
            }
            VoiceUiOp::MoveBegin(idx) => {
                let voice = song.ins.voices.remove(idx.usize());
                song.ins.voices.insert(0, voice);
            }
            VoiceUiOp::MoveEnd(idx) => {
                let voice = song.ins.voices.remove(idx.usize());
                song.ins.voices.push(voice);
            }
            VoiceUiOp::Swap(a, b) => {
                song.ins.voices.swap(a.usize(), b.usize());
            }
            VoiceUiOp::Duplicate(idx) => {
                let dup = song.ins.voices[idx].clone();
                // We want to insert the duplicate at the end, not to disrupt the existing indices
                song.ins.voices.push(dup);
            }
            VoiceUiOp::Delete(idx) => {
                song.ins.voices.remove(idx.usize());
            }
        }
    }
    // If file dialog has a voice selected, try to preview it
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(en) = app_file_dia.selected_entry() {
        if ui_state.file_dia_prev_sel.as_deref() != Some(en.as_path()) {
            ui_state.file_dia_prev_sel = Some(en.to_path_buf());
            if en.as_path().is_dir() {
                return;
            }
            if let Some(op) = app_file_dia.user_data::<FileOp>() {
                match op {
                    FileOp::ImportPtVoice | FileOp::ReplacePtVoiceSingle(_) => {
                        voice_import_preview(song, out_rate, en, just_load_ptvoice);
                    }
                    FileOp::ImportPtNoise | FileOp::ReplacePtNoiseSingle(_) => {
                        voice_import_preview(song, out_rate, en, just_load_ptnoise);
                    }
                    _ => {}
                }
            }
        }
    }
}

const PREVIEW_VOICE_IDX: VoiceIdx = VoiceIdx(100);

#[cfg(not(target_arch = "wasm32"))]
fn voice_import_preview(
    song: &mut SongState,
    out_rate: u16,
    en: &egui_file_dialog::DirectoryEntry,
    load_fun: fn(&[u8], path: &std::path::Path) -> ptcow::ReadResult<ptcow::Voice>,
) {
    let data = std::fs::read(en.as_path()).unwrap();
    let noise_tbl = NoiseTable::generate();
    match load_fun(&data, en.as_path()) {
        Ok(mut voice) => {
            voice.recalculate(&noise_tbl, out_rate);
            song.preview_voice = voice;
        }
        Err(e) => {
            eprintln!("Error loading voice: {e}");
        }
    }
    song.voice_test_unit.reset_voice(
        &song.ins,
        PREVIEW_VOICE_IDX,
        song.song.master.timing,
        std::slice::from_ref(&song.preview_voice),
    );
    song.voice_test_unit.on(
        SongState::VOICE_TEST_UNIT_IDX,
        &song.ins,
        &[],
        0,
        1000,
        0,
        song.herd.smp_end,
        std::slice::from_ref(&song.preview_voice),
    );
}

enum VoiceUiOp {
    MoveUp(VoiceIdx),
    MoveDown(VoiceIdx),
    MoveBegin(VoiceIdx),
    MoveEnd(VoiceIdx),
    Delete(VoiceIdx),
    Swap(VoiceIdx, VoiceIdx),
    Duplicate(VoiceIdx),
}

fn voice_ui(
    ui: &mut egui::Ui,
    voice: &mut Voice,
    idx: VoiceIdx,
    op: &mut Option<VoiceUiOp>,
    out_rate: SampleRate,
    ui_state: &mut VoicesUiState,
    shared: &mut SharedUiState,
    herd: &mut ptcow::Herd,
    voice_test_unit: &mut ptcow::Unit,
    app_cmd: &mut CommandQueue,
) {
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut voice.name);
        if ui.button("⬆").clicked() {
            *op = Some(VoiceUiOp::MoveUp(idx));
        }
        if ui.button("⬇").clicked() {
            *op = Some(VoiceUiOp::MoveDown(idx));
        }
        if ui.button("⮪").clicked() {
            *op = Some(VoiceUiOp::MoveBegin(idx));
        }
        if ui.button("⮫").clicked() {
            *op = Some(VoiceUiOp::MoveEnd(idx));
        }
        if ui.button("del").clicked() {
            *op = Some(VoiceUiOp::Delete(idx));
        }
        let unit = herd
            .units
            .get_mut(shared.active_unit)
            .unwrap_or(voice_test_unit);
        let label = egui::RichText::new(format!("🎹 Test with {}", unit.name))
            .color(unit_color(shared.active_unit));
        if ui.button(label).clicked() {
            app_cmd.push(Cmd::ResetUnitVoice {
                unit: shared.active_unit,
                voice: idx,
            });
        }
        ui.menu_button("🔁 Replace", |ui| {
            if ui.button((img::SAXO.smol(), "With .ptvoice")).clicked() {
                app_cmd.push(Cmd::PromptReplacePtVoiceSingle(idx));
            }
            if ui.button((img::DRUM.smol(), "With .ptnoise")).clicked() {
                app_cmd.push(Cmd::PromptReplacePtNoiseSingle(idx));
            }
            ui.menu_button("✴ With new", |ui| {
                if ui.button((img::SAXO.smol(), "Wave")).clicked() {
                    let sqr = square_wave_voice();
                    voice.base.data = sqr.base.data;
                    voice.base.unit = sqr.base.unit;
                    app_cmd.push(Cmd::ResetVoiceForUnitsWithVoiceIdx { idx });
                }
                if ui.button((img::DRUM.smol(), "Noise")).clicked() {
                    let bass = hat_close_voice();
                    voice.base.data = bass.base.data;
                    voice.base.unit = bass.base.unit;
                }
            });
        });
        match &voice.base.data {
            VoiceData::Noise(_) => {
                if ui.button("💾 Export .ptnoise").clicked() {
                    app_cmd.push(Cmd::PromptExportPtnoise { voice: idx });
                }
            }
            VoiceData::Wave(_) => {
                if ui.button("💾 Export .ptvoice").clicked() {
                    app_cmd.push(Cmd::PromptExportPtvoice { voice: idx });
                }
            }
            _ => {}
        }
    });

    ui.horizontal(|ui| {
        ui.label("Experimentation slots")
            .on_hover_text("These saves only last while this voice tab is open");
        for (i, save_slot) in ui_state.save_slots.iter_mut().enumerate() {
            ui.group(|ui| {
                ui.label(i.to_string());
                if ui.button("💾").clicked() {
                    *save_slot = Some(voice.clone());
                }
                match save_slot {
                    Some(saved_voice) => {
                        if ui.button("⮋").clicked() {
                            *voice = saved_voice.clone();
                        }
                    }
                    None => {
                        ui.add_enabled(false, egui::Button::new("⮋"));
                    }
                }
            });
        }
    });

    voice_ui_inner(
        ui,
        voice,
        idx,
        out_rate,
        ui_state,
        app_cmd,
        &herd.units,
        voice_test_unit,
    );
}

pub struct SubSliceUi {
    max_size: usize,
    offset: usize,
}

impl Default for SubSliceUi {
    fn default() -> Self {
        Self {
            max_size: Self::DEFAULT_MAX_SIZE,
            offset: 0,
        }
    }
}

impl SubSliceUi {
    pub const DEFAULT_MAX_SIZE: usize = 16_384;
    pub fn subslice_ui<'a, T>(
        &mut self,
        ui: &mut egui::Ui,
        slice: &'a mut [T],
        step_by: u8,
    ) -> &'a mut [T] {
        ui.style_mut().spacing.slider_width = ui.available_width() - 100.0;
        ui.horizontal(|ui| {
            const MAX: usize = 262_144;
            ui.label("↔");
            let min = std::cmp::min(32, slice.len());
            let max = std::cmp::min(MAX, slice.len());
            ui.add(egui::Slider::new(&mut self.max_size, min..=max).step_by(f64::from(step_by)));
        });
        let end = slice.len().saturating_sub(self.max_size);
        ui.add_enabled_ui(end != 0, |ui| {
            ui.horizontal(|ui| {
                ui.label("⤵");
                ui.add_enabled(
                    end != 0,
                    egui::Slider::new(&mut self.offset, 0..=end).step_by(f64::from(step_by)),
                );
            });
        });

        let end = self.offset + self.max_size;
        &mut slice[self.offset..end]
    }
}

pub fn voice_ui_inner(
    ui: &mut egui::Ui,
    voice: &mut Voice,
    voice_idx: VoiceIdx,
    out_rate: SampleRate,
    ui_state: &mut VoicesUiState,
    app_cmd: &mut CommandQueue,
    units: &ptcow::Units,
    voice_test_unit: &ptcow::Unit,
) {
    // Add a slot selection UI for wave voices
    if let VoiceData::Wave(_) = &voice.base.data {
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut ui_state.sel_slot,
                SelectedSlot::Base,
                // We still want the image for coord/overtone distinction
                (voice_data_img(&voice.base.data), "Base"),
            );
            if let Some(extra) = &voice.extra {
                ui.selectable_value(
                    &mut ui_state.sel_slot,
                    SelectedSlot::Extra,
                    (voice_data_img(&extra.data), "Extra"),
                );
                if ui.button("-").clicked() {
                    voice.extra = None;
                }
            } else {
                if ui.button("+").clicked() {
                    voice.extra = Some(voice.base.clone());
                }
            }
        });
    }

    ui.separator();
    // Ensure we don't select extra slot if it's none
    if voice.extra.is_none() {
        ui_state.sel_slot = SelectedSlot::Base;
    }
    let slot = match ui_state.sel_slot {
        SelectedSlot::Base => &mut voice.base,
        // INVARIANT: We assume extra is only selected if it exists (see above)
        SelectedSlot::Extra => voice.extra.as_mut().unwrap(),
    };
    let id = ui.make_persistent_id("inst");
    let cs = CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let top_max_height = if cs.is_open() {
        ui.available_height() / 2.0
    } else {
        ui.available_height() - 32.0
    };
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .max_height(top_max_height)
        .show(ui, |ui| {
            voice_unit_ui(
                ui,
                slot,
                out_rate,
                voice_idx,
                ui_state,
                ui_state.sel_slot,
                app_cmd,
            );
        });
    ui.separator();
    cs.show_header(ui, |ui| {
        ui.strong("📈 Instance");
        ui.selectable_value(&mut ui_state.instance_tab, InstanceTab::Samples, "Samples");
        ui.selectable_value(
            &mut ui_state.instance_tab,
            InstanceTab::Envelope,
            "Envelope",
        );
        match ui_state.instance_tab {
            InstanceTab::Samples => {
                ui.label(format!(
                    "{} samples, {} bytes",
                    slot.inst.num_samples,
                    slot.inst.sample_buf.len()
                ));
            }
            InstanceTab::Envelope => {
                ui.label(format!(
                    "{} points, release: {}",
                    slot.inst.env.len(),
                    slot.inst.env_release
                ));
            }
        }
    })
    .body(|ui| {
        egui::ScrollArea::vertical()
            .id_salt("inst")
            .auto_shrink(false)
            .show(ui, |ui| {
                match ui_state.instance_tab {
                    InstanceTab::Samples => {
                        if slot.inst.sample_buf.is_empty() {
                            // Avoid (bytemuck) panic on empty sample buffer
                            ui.colored_label(egui::Color32::GRAY, "No sample data");
                        } else {
                            let samples = bytemuck::cast_slice_mut(&mut slot.inst.sample_buf);
                            let view = ui_state.inst_sub.subslice_ui(ui, samples, 2);
                            waveform_edit_widget_16_bit_interleaved_stereo(
                                ui,
                                view,
                                256.,
                                egui::Id::new("smp_buf"),
                            );
                        }
                    }
                    InstanceTab::Envelope => {
                        if !slot.inst.env.is_empty() {
                            let view = ui_state.inst_env_sub.subslice_ui(ui, &mut slot.inst.env, 1);
                            envelope_edit_widget(
                                ui,
                                view,
                                256.0,
                                egui::Id::new("env_buf"),
                                voice_idx,
                                ui_state.sel_slot,
                                units,
                                voice_test_unit,
                            );
                        }
                    }
                }
            })
    });
}

fn osci_ui(ui: &mut egui::Ui, osci: &mut NoiseDesignOscillator, name: &str) {
    ui.strong(name);
    ui.indent("osci", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.style_mut().spacing.slider_width = ui.available_width() - 160.0;
            ui.selectable_value(&mut osci.type_, NoiseType::Sine, "Sine");
            ui.selectable_value(&mut osci.type_, NoiseType::Saw, "Saw");
            ui.selectable_value(&mut osci.type_, NoiseType::Rect, "Rect");
            ui.selectable_value(&mut osci.type_, NoiseType::Random, "Random");
            ui.selectable_value(&mut osci.type_, NoiseType::Saw2, "Saw2");
            ui.selectable_value(&mut osci.type_, NoiseType::Rect2, "Rect2");
            ui.selectable_value(&mut osci.type_, NoiseType::Tri, "Tri");
            ui.selectable_value(&mut osci.type_, NoiseType::Random2, "Random2");
            ui.selectable_value(&mut osci.type_, NoiseType::Rect3, "Rect3");
            ui.selectable_value(&mut osci.type_, NoiseType::Rect4, "Rect4");
            ui.selectable_value(&mut osci.type_, NoiseType::Rect8, "Rect8");
            ui.selectable_value(&mut osci.type_, NoiseType::Rect16, "Rect16");
            ui.selectable_value(&mut osci.type_, NoiseType::Saw3, "Saw3");
            ui.selectable_value(&mut osci.type_, NoiseType::Saw4, "Saw4");
            ui.selectable_value(&mut osci.type_, NoiseType::Saw6, "Saw6");
            ui.selectable_value(&mut osci.type_, NoiseType::Saw8, "Saw8");
            ui.end_row();
            ui.label("freq");
            ui.add(
                egui::DragValue::new(&mut osci.freq)
                    .range(0.0..=44100.0)
                    .speed(0.1),
            );
            ui.label("Hz");
            ui.end_row();
            ui.label("volume");
            ui.add(egui::Slider::new(&mut osci.volume, 0.0..=200.0));
            ui.end_row();
            ui.label("offset");
            ui.add(egui::Slider::new(&mut osci.offset, 0.0..=100.0));
            ui.end_row();
            ui.checkbox(&mut osci.invert, "invert");
        });
    });
}

struct Pal {
    wave_bg: egui::Color32,
    wave_stroke: egui::Color32,
    env_bg: egui::Color32,
    env_stroke: egui::Color32,
}

const PAL: Pal = Pal {
    wave_bg: egui::Color32::from_rgb(0, 102, 67),
    wave_stroke: egui::Color32::from_rgb(34, 204, 110),
    env_bg: egui::Color32::from_rgb(102, 67, 0),
    env_stroke: egui::Color32::from_rgb(204, 110, 34),
};

fn voice_unit_ui(
    ui: &mut egui::Ui,
    slot: &mut ptcow::VoiceSlot,
    out_rate: SampleRate,
    voice_idx: VoiceIdx,
    ui_state: &mut VoicesUiState,
    sel_slot: SelectedSlot,
    app_cmd: &mut CommandQueue,
) {
    key_tune_flags_ui(ui, slot, voice_idx, app_cmd);

    match &mut slot.data {
        VoiceData::Noise(noise) => {
            let total = noise.units.len();
            ui.horizontal(|ui| {
                ui.label("Number of samples");
                ui.add(egui::DragValue::new(&mut noise.smp_num_44k));
            });
            ui.horizontal(|ui| {
                for i in 0..total {
                    if ui
                        .selectable_label(
                            ui_state.selected_noise_unit == i,
                            (img::DRUM, format!("unit {i}")),
                        )
                        .clicked()
                    {
                        ui_state.selected_noise_unit = i;
                    }
                }
                if ui
                    .add_enabled(!noise.units.is_full(), egui::Button::new("+"))
                    .clicked()
                {
                    noise.units.push(default_noise_unit());
                }
                if ui
                    .add_enabled(!noise.units.is_empty(), egui::Button::new("-"))
                    .clicked()
                {
                    noise.units.remove(ui_state.selected_noise_unit);
                    ui_state.selected_noise_unit = 0;
                }
            });
            let Some(unit) = noise.units.get_mut(ui_state.selected_noise_unit) else {
                ui.label("No noise unit selected");
                return;
            };
            ui.horizontal(|ui| {
                ui.label("pan");
                ui.add(egui::Slider::new(&mut unit.pan, -100..=100));
            });
            osci_ui(ui, &mut unit.main, "main");
            osci_ui(ui, &mut unit.freq, "freq");
            osci_ui(ui, &mut unit.volu, "volu");
            ui.horizontal(|ui| {
                ui.label("Envelope points");
                if ui
                    .add_enabled(!unit.enves.is_full(), egui::Button::new("+"))
                    .clicked()
                {
                    unit.enves.push(EnvPt { x: 1, y: 1 });
                }
                if ui
                    .add_enabled(!unit.enves.is_empty(), egui::Button::new("-"))
                    .clicked()
                {
                    unit.enves.pop();
                }
            });
            ui.horizontal(|ui| {
                for env_pt in &mut unit.enves {
                    ui.group(|ui| {
                        ui.add(egui::DragValue::new(&mut env_pt.x).prefix("x "));
                        ui.add(egui::DragValue::new(&mut env_pt.y).prefix("y "));
                    });
                }
            });
            ui.separator();

            let tbl = NoiseTable::generate();
            slot.inst.sample_buf = noise_to_pcm(noise, &tbl).smp;
            slot.inst.num_samples = noise.smp_num_44k;
        }
        VoiceData::Pcm(pcm) => {
            ui.label(format!("pcm data ({} bytes)", pcm.smp.len()));
            ui.indent("pcm", |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut pcm.bps, Bps::B8, "8 bit");
                    ui.selectable_value(&mut pcm.bps, Bps::B16, "16 bit");
                    ui.separator();
                    ui.selectable_value(&mut pcm.ch, ChNum::Mono, "Mono");
                    ui.selectable_value(&mut pcm.ch, ChNum::Stereo, "Stereo");
                    ui.end_row();
                    ui.label("body");
                    ui.add(egui::DragValue::new(&mut pcm.num_samples));
                    ui.end_row();
                    ui.label("sample rate");
                    ui.add(egui::DragValue::new(&mut pcm.sps));
                });
            });
        }
        VoiceData::Wave(wave_data) => {
            ui.horizontal(|ui| {
                ui.label("Volume");
                ui.add(egui::DragValue::new(&mut wave_data.volume));
                ui.label("Pan");
                ui.add(egui::Slider::new(&mut wave_data.pan, 0..=128));
                ui.label("Kind");
                if ui
                    .selectable_label(
                        matches!(wave_data.points, WaveDataPoints::Coord { .. }),
                        (img::SAXO, "Coordinate"),
                    )
                    .clicked()
                {
                    app_cmd.modal(move |m| {
                        m.replace_wave_data_slot(voice_idx, sel_slot, square_wave());
                    });
                }
                if ui
                    .selectable_label(
                        matches!(wave_data.points, WaveDataPoints::Overtone { .. }),
                        (img::ACCORDION, "Overtone"),
                    )
                    .clicked()
                {
                    app_cmd.modal(move |m| {
                        m.replace_wave_data_slot(
                            voice_idx,
                            sel_slot,
                            WaveData {
                                points: WaveDataPoints::Overtone {
                                    points: vec![OsciPt { x: 1, y: 16 }],
                                },
                                envelope: EnvelopeSrc::default(),
                                volume: 127,
                                pan: 64,
                            },
                        );
                    });
                }
                ui.separator();
                match &mut wave_data.points {
                    WaveDataPoints::Coord { points, resolution } => {
                        ui.label("Resolution");
                        ui.add(egui::DragValue::new(resolution)).changed();
                        ui.end_row();
                        ui.label(format!("{} points", points.len()));
                        if ui.button("+").clicked() {
                            points.push(OsciPt {
                                x: points.last().map_or(0, |pt| pt.x) + 1,
                                y: 0,
                            });
                        }
                        if ui.button("-").clicked() {
                            points.pop();
                        }
                        if ui.button("Fix x coordinates").clicked() {
                            for i in 0..points.len() {
                                let Ok([a, b]) = points.get_disjoint_mut([i, i + 1]) else {
                                    break;
                                };
                                if b.x <= a.x {
                                    b.x = a.x + 1;
                                }
                            }
                        }
                    }
                    WaveDataPoints::Overtone { points } => {
                        ui.label(format!("{} points", points.len()));
                        if ui.button("+").clicked() {
                            points.push(OsciPt {
                                x: points.last().map_or(0, |pt| pt.x) + 1,
                                y: 1,
                            });
                        }
                        if ui.button("-").clicked() {
                            points.pop();
                        }
                    }
                }
            });
            match &mut wave_data.points {
                WaveDataPoints::Coord { points, resolution } => {
                    ui.horizontal_top(|ui| {
                        draw_coord_wavebox(ui, points, resolution, ui_state.last_hovered_wave_idx);
                        ui.horizontal_wrapped(|ui| {
                            // Highlight the hovered control in the wave visualization
                            ui_state.last_hovered_wave_idx = None;
                            for (i, pt) in points.iter_mut().enumerate() {
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut pt.x).prefix("x ").range(0..=255),
                                    )
                                    .hovered()
                                {
                                    ui_state.last_hovered_wave_idx = Some(i);
                                }
                                if ui
                                    .add(egui::Slider::new(&mut pt.y, -128..=127).prefix("y "))
                                    .hovered()
                                {
                                    ui_state.last_hovered_wave_idx = Some(i);
                                }
                            }
                        });
                    });
                }
                WaveDataPoints::Overtone { points } => {
                    ui.horizontal_top(|ui| {
                        draw_overtone_wavebox(ui, wave_data.volume, points);
                        ui.horizontal_wrapped(|ui| {
                            ui.style_mut().spacing.slider_width = 512.0;
                            for pt in &mut *points {
                                ui.add(egui::DragValue::new(&mut pt.x).range(1..=512).prefix("x "))
                                    .changed();
                                ui.add(egui::Slider::new(&mut pt.y, -128..=128).prefix("y "))
                                    .changed();
                                ui.end_row();
                            }
                        });
                    });
                }
            }

            slot.inst
                .recalc_wave_data(&wave_data.points, wave_data.volume, wave_data.pan);
        }
        VoiceData::OggV(oggv) => {
            ui.label("Ogg/Vorbis voice");
            ui.label("channel number");
            ui.add(egui::DragValue::new(&mut oggv.ch));
            ui.label("sps2");
            ui.add(egui::DragValue::new(&mut oggv.sps2));
        }
    }
    slot_wave_extra_ui(ui, slot, out_rate, sel_slot);
}

fn key_tune_flags_ui(
    ui: &mut egui::Ui,
    slot: &mut ptcow::VoiceSlot,
    voice_idx: VoiceIdx,
    app_cmd: &mut CommandQueue,
) {
    ui.horizontal(|ui| {
        let mut changed = false;
        ui.label("Key");
        changed |= ui
            .add_enabled(
                !slot.unit.flags.contains(VoiceFlags::BEAT_FIT),
                egui::DragValue::new(&mut slot.unit.basic_key),
            )
            .changed();
        ui.label("Tune");
        changed |= ui
            .add(egui::DragValue::new(&mut slot.unit.tuning).speed(0.0001))
            .changed();
        for (name, flag) in VoiceFlags::iter_defined_names() {
            let mut contains = slot.unit.flags.contains(flag);
            // TODO: This is stupid
            let name = match name {
                "WAVE_LOOP" => "loop",
                "SMOOTH" => "smooth",
                "BEAT_FIT" => "fit",
                etc => etc,
            };
            if ui.checkbox(&mut contains, name).clicked() {
                if name == "fit" {
                    changed = true;
                }
                slot.unit.flags ^= flag;
            }
        }
        if changed {
            app_cmd.push(Cmd::ResetVoiceForUnitsWithVoiceIdx { idx: voice_idx });
        }
    });
}

fn default_noise_unit() -> NoiseDesignUnit {
    let mut new = NoiseDesignUnit {
        enves: ArrayVec::from([
            EnvPt { x: 0, y: 100 },
            EnvPt { x: 50, y: 50 },
            EnvPt { x: 100, y: 0 },
        ]),
        ..Default::default()
    };
    new.main.freq = 60.0;
    new.main.volume = 80.0;
    new
}

fn slot_wave_extra_ui(
    ui: &mut egui::Ui,
    slot: &mut ptcow::VoiceSlot,
    out_rate: u16,
    sel_slot: SelectedSlot,
) {
    let VoiceData::Wave(data) = &mut slot.data else {
        return;
    };
    // When calculating width, ignore last point (release)
    let env_w: u16 = data.envelope.points[..data.envelope.points.len().saturating_sub(1)]
        .iter()
        .map(|pt| pt.x)
        .sum();
    ui.horizontal(|ui| {
        ui.strong(format!("Envelope ({} points)", data.envelope.points.len()));
        ui.label("fps");
        ui.add(egui::DragValue::new(&mut data.envelope.seconds_per_point).range(1..=999_999));
        if data.envelope.points.is_empty() {
            if ui.button("+ Add release point").clicked() {
                data.envelope.points.push(EnvPt { x: 0, y: 0 });
            }
        } else {
            if ui.button("+").clicked() {
                data.envelope
                    .points
                    .insert(data.envelope.points.len() - 1, EnvPt { x: 1, y: 64 });
            }
            if data.envelope.points.len() == 1 {
                if ui.button("- Remove release point").clicked() {
                    data.envelope.points.pop();
                }
            } else {
                if ui.button("-").clicked() {
                    data.envelope.points.remove(data.envelope.points.len() - 2);
                }
            }
        }
        let prepared_size = data.envelope.prepared_size(out_rate);
        if prepared_size <= 88_200 {
            slot.inst.recalc_envelope(out_rate, &data.envelope);
        } else {
            ui.label(format!("Prepared size: {prepared_size}"));
            if ui.button("Recalculate").clicked() {
                slot.inst.recalc_envelope(out_rate, &data.envelope);
            }
        }
    });
    ui.horizontal_top(|ui| {
        if !data.envelope.points.is_empty() {
            draw_envelope_src(&data.envelope, ui, env_w, sel_slot);
        }
        ui.horizontal_wrapped(|ui| {
            if let Some((last, init)) = data.envelope.points.split_last_mut() {
                for pt in init {
                    ui.add(egui::DragValue::new(&mut pt.x).prefix("x "));
                    ui.add(egui::DragValue::new(&mut pt.y).prefix("y "));
                }
                ui.end_row();
                ui.label(format!("envelope width: {env_w}"));
                ui.end_row();
                ui.label("Release");
                // Only the x value is used for release
                ui.add(egui::DragValue::new(&mut last.x));
            }
        });
    });
}

fn draw_overtone_wavebox(ui: &mut egui::Ui, volume: i16, points: &[OsciPt]) {
    let size: u16 = 256;
    let (rect, _re) = ui.allocate_exact_size(
        egui::vec2(f32::from(size), f32::from(size)),
        egui::Sense::click_and_drag(),
    );
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, PAL.wave_bg);
    let lc = rect.left_center();
    let args = OsciArgs {
        volume,
        sample_num: size.into(),
    };
    let mut egui_points: Vec<egui::Pos2> = Vec::new();
    for i in 0..=size {
        let amp = ptcow::overtone(args, points, i);
        let y = (amp * f64::from(size) / 2.0) as f32;
        egui_points.push(egui::pos2(lc.x + f32::from(i), lc.y - y));
    }
    p.line(egui_points, egui::Stroke::new(2.0, PAL.wave_stroke));
}

fn draw_coord_wavebox(
    ui: &mut egui::Ui,
    points: &[OsciPt],
    resolution: &mut u16,
    highlight: Option<usize>,
) {
    let reso = f32::from(*resolution);
    let (rect, _re) = ui.allocate_exact_size(egui::vec2(reso, 256.), egui::Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, PAL.wave_bg);
    let lc = rect.left_center();
    let mut egui_points: Vec<egui::Pos2> = points
        .iter()
        .map(|pt| egui::pos2(lc.x + f32::from(pt.x), lc.y - f32::from(pt.y)))
        .collect();
    // We insert points at each extreme, to accurately reflect that
    // the generated waveform loops back onto itself.
    let fst_y = egui_points.first().map_or(rect.left_center().y, |pt| pt.y);
    egui_points.insert(0, egui::pos2(rect.left(), fst_y));
    egui_points.push(egui::pos2(rect.right(), fst_y));
    let hi_point = highlight.map(|idx| egui_points[idx + 1]);
    p.line(egui_points, egui::Stroke::new(2.0, PAL.wave_stroke));
    if let Some(pt) = hi_point {
        p.circle_filled(pt, 4.0, egui::Color32::YELLOW);
    }
}

fn draw_envelope_src(env_src: &EnvelopeSrc, ui: &mut egui::Ui, width: u16, sel_slot: SelectedSlot) {
    let w = f32::from(width);
    egui::ScrollArea::horizontal()
        .id_salt(sel_slot)
        .max_width(384.0)
        .show(ui, |ui| {
            let (rect, _re) =
                ui.allocate_exact_size(egui::vec2(w, 128.0), egui::Sense::click_and_drag());
            let p = ui.painter_at(rect);
            let lb = rect.left_bottom();
            p.rect_filled(rect, 2.0, PAL.env_bg);
            let mut x_cursor = 0;
            let mut egui_points: Vec<egui::Pos2> = env_src.points
                // We don't want to include the last point (release)
                [..env_src.points.len().saturating_sub(1)]
                .iter()
                .map(|pt| {
                    x_cursor += pt.x;
                    egui::pos2(lb.x + f32::from(x_cursor), lb.y - f32::from(pt.y))
                })
                .collect();
            // Ptvoice seems to have a point at (0, bottom) when drawing
            egui_points.insert(0, egui::pos2(lb.x, lb.y));
            p.line(egui_points, egui::Stroke::new(2.0, PAL.env_stroke));
        });
}
