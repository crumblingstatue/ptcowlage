use {
    crate::{
        app::{
            command_queue::{Cmd, CommandQueue},
            ui::{
                FreeplayPianoState, SharedUiState, img, unit_color, voice_data_img, voice_img,
                waveform_edit_widget,
            },
        },
        audio_out::{AuxAudioKey, AuxAudioState, AuxMsg, SongState},
        pxtone_misc::{bass_drum, reset_voice_for_units_with_voice_idx, square_wave},
    },
    bitflags::Flags as _,
    eframe::egui::{self, AtomExt, collapsing_header::CollapsingState},
    ptcow::{
        Bps, ChNum, EnvPt, EnvelopeSrc, NoiseDesignOscillator, NoiseDesignUnit,
        NoiseDesignUnitFlags, NoiseTable, NoiseType, OsciArgs, OsciPt, SampleRate, Voice,
        VoiceData, VoiceFlags, VoiceIdx, VoiceUnit, WaveData, WaveDataPoints, noise_to_pcm,
    },
    rustc_hash::FxHashMap,
};

#[derive(Default)]
pub struct VoicesUiState {
    pub selected_idx: VoiceIdx,
    sel_slot: SelectedSlot,
    dragged_idx: Option<VoiceIdx>,
    // Keep track of (preview) sounds playing for each voice
    playing_sounds: FxHashMap<VoiceIdx, AuxAudioKey>,
}

#[derive(Default, PartialEq, Hash, Clone, Copy)]
pub enum SelectedSlot {
    #[default]
    Base,
    Extra,
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
    aux: &mut Option<AuxAudioState>,
    piano_state: &FreeplayPianoState,
    app_cmd: &mut CommandQueue,
) {
    let mut op = None;
    ui.horizontal_wrapped(|ui| {
        ui.menu_button("✴ New...", |ui| {
            if ui.button((img::SAXO.smol(), "Wave")).clicked() {
                let mut voice = square_wave_voice();
                voice.name = format!("Wave {}", song.ins.voices.len());
                song.ins.voices.push(voice);
                let idx = VoiceIdx(song.ins.voices.len() - 1);
                ui_state.selected_idx = idx;
                reset_voice_for_units_with_voice_idx(song, idx);
            }
            if ui.button((img::DRUM.smol(), "Noise")).clicked() {
                let mut voice = bass_drum_voice();
                voice.name = format!("Noise {}", song.ins.voices.len());
                song.ins.voices.push(voice);
                let idx = VoiceIdx(song.ins.voices.len() - 1);
                ui_state.selected_idx = idx;
                reset_voice_for_units_with_voice_idx(song, idx);
            }
        });
        ui.menu_button(" Import...", |ui| {
            if ui.button((img::SAXO.smol(), ".ptvoice")).clicked() {
                app_cmd.push(Cmd::PromptImportPtVoice);
            }
            if ui.button((img::DRUM.smol(), ".ptnoise")).clicked() {
                app_cmd.push(Cmd::PromptImportPtNoise);
            }
            if ui.button("🎵 Single from .sf2").clicked() {
                app_cmd.push(Cmd::PromptImportSf2Sound);
            }
        });
        ui.menu_button("🔁 Replace...", |ui| {
            if ui.button((img::COW.smol(), "All from .ptcop...")).clicked() {
                app_cmd.push(Cmd::PromptReplaceAllPtcop);
            }
            if ui
                .button((img::SAXO.smol(), "Current from .ptvoice"))
                .clicked()
            {
                app_cmd.push(Cmd::PromptReplacePtVoiceSingle(ui_state.selected_idx));
            }
            if ui
                .button((img::DRUM.smol(), "Current from .ptnoise"))
                .clicked()
            {
                app_cmd.push(Cmd::PromptReplacePtNoiseSingle(ui_state.selected_idx));
            }
            if ui.button("🎵 Current from .sf2").clicked() {
                app_cmd.push(Cmd::PromptReplaceSf2Single(ui_state.selected_idx));
            }
            ui.menu_button("✴ Current with new", |ui| {
                if ui.button((img::SAXO.smol(), "Wave")).clicked() {
                    if let Some(voice) = song.ins.voices.get_mut(ui_state.selected_idx) {
                        let sqr = square_wave_voice();
                        voice.base.data = sqr.base.data;
                        voice.base.unit = sqr.base.unit;
                        reset_voice_for_units_with_voice_idx(song, ui_state.selected_idx);
                    }
                }
                if ui.button((img::DRUM.smol(), "Noise")).clicked() {
                    if let Some(voice) = song.ins.voices.get_mut(ui_state.selected_idx) {
                        let bass = bass_drum_voice();
                        voice.base.data = bass.base.data;
                        voice.base.unit = bass.base.unit;
                        reset_voice_for_units_with_voice_idx(song, ui_state.selected_idx);
                    }
                }
            });
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
            aux,
            ui_state,
            piano_state,
            &mut song.herd,
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
}

fn bass_drum_voice() -> Voice {
    let data = VoiceData::Noise(bass_drum());
    Voice::from_unit_and_data(VoiceUnit::default(), data)
}

fn square_wave_voice() -> Voice {
    let unit = VoiceUnit {
        flags: VoiceFlags::WAVE_LOOP,
        ..VoiceUnit::default()
    };
    let data = VoiceData::Wave(square_wave());
    Voice::from_unit_and_data(unit, data)
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
    aux: &mut Option<AuxAudioState>,
    ui_state: &mut VoicesUiState,
    piano_state: &FreeplayPianoState,
    herd: &mut ptcow::Herd,
    app_cmd: &mut CommandQueue,
) {
    let aux = aux.get_or_insert_with(|| crate::audio_out::spawn_aux_audio_thread(out_rate, 1024));
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut voice.name);
        for slot in voice.slots() {
            play_sound_ui(ui, aux, ui_state, idx, &slot.inst.sample_buf);
        }
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
        if let Some(unit_idx) = piano_state.toot {
            let unit = &mut herd.units[unit_idx];
            let label = egui::RichText::new(format!("🎹 Test with {}", unit.name))
                .color(unit_color(unit_idx));
            if ui.button(label).clicked() {
                app_cmd.push(Cmd::ResetUnitVoice {
                    unit: unit_idx,
                    voice: idx,
                });
            }
        }
        match &voice.base.data {
            VoiceData::Noise(_) => {
                if ui.button("Export .ptnoise").clicked() {
                    app_cmd.push(Cmd::PromptExportPtnoise { voice: idx });
                }
            }
            VoiceData::Wave(_) => {
                if ui.button("Export .ptvoice").clicked() {
                    app_cmd.push(Cmd::PromptExportPtvoice { voice: idx });
                }
            }
            _ => {}
        }
    });

    voice_ui_inner(ui, voice, idx, out_rate, aux, ui_state, app_cmd);
}

pub fn voice_ui_inner(
    ui: &mut egui::Ui,
    voice: &mut Voice,
    voice_idx: VoiceIdx,
    out_rate: SampleRate,
    aux: &mut AuxAudioState,
    ui_state: &mut VoicesUiState,
    app_cmd: &mut CommandQueue,
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
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            voice_unit_ui(
                ui,
                slot,
                out_rate,
                voice_idx,
                ui_state,
                aux,
                ui_state.sel_slot,
                app_cmd,
            );
            let id = ui.make_persistent_id("inst");
            CollapsingState::load_with_default_open(ui.ctx(), id, false)
                .show_header(ui, |ui| {
                    ui.strong("Instance");
                })
                .body(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Sample buf");
                        play_sound_ui(ui, aux, ui_state, voice_idx, &slot.inst.sample_buf);
                        let mut len = slot.inst.sample_buf.len();
                        if ui
                            .add(egui::DragValue::new(&mut len).update_while_editing(false))
                            .changed()
                        {
                            slot.inst.sample_buf.resize(len, 0);
                        }
                        ui.label("Number of samples");
                        ui.add(egui::DragValue::new(&mut slot.inst.num_samples));
                    });
                    waveform_edit_widget(
                        ui,
                        &mut slot.inst.sample_buf,
                        256.,
                        egui::Id::new("smp_buf"),
                    );
                    ui.horizontal(|ui| {
                        ui.label("Envelope");
                        let mut len = slot.inst.env.len();
                        if ui
                            .add(egui::DragValue::new(&mut len).update_while_editing(false))
                            .changed()
                        {
                            slot.inst.env.resize(len, 0);
                        }
                    });
                    if !slot.inst.env.is_empty() {
                        waveform_edit_widget(
                            ui,
                            &mut slot.inst.env,
                            256.0,
                            egui::Id::new("env_buf"),
                        );
                    }
                    ui.label("Envelope release");
                    ui.add(egui::DragValue::new(&mut slot.inst.env_release));
                });
        });
}

fn play_sound_ui(
    ui: &mut egui::Ui,
    aux: &AuxAudioState,
    ui_state: &mut VoicesUiState,
    voice_idx: VoiceIdx,
    data: &[u8],
) {
    if let Some(sound_key) = ui_state.playing_sounds.get(&voice_idx) {
        if ui.button("Stop").clicked() {
            aux.send
                .send(AuxMsg::StopAudio { key: *sound_key })
                .unwrap();
            ui_state.playing_sounds.remove(&voice_idx);
        }
    } else {
        if ui.button("▶ Play").clicked() {
            let key = aux.next_key();
            ui_state.playing_sounds.insert(voice_idx, key);
            aux.send
                .send(AuxMsg::PlaySamples16 {
                    key,
                    sample_data: bytemuck::pod_collect_to_vec(data),
                })
                .unwrap();
        }
    }
}

/// Returns whether the serialize checkbox was clicked
fn osci_ui(
    ui: &mut egui::Ui,
    osci: &mut NoiseDesignOscillator,
    name: &str,
    mut serialize: bool,
) -> bool {
    let mut ser_clicked = false;
    ui.horizontal(|ui| {
        ui.heading(name);
        ser_clicked = ui.checkbox(&mut serialize, "serialize").clicked();
    });
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
    ser_clicked
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
    aux: &AuxAudioState,
    sel_slot: SelectedSlot,
    app_cmd: &mut CommandQueue,
) {
    match &mut slot.data {
        VoiceData::Noise(noise) => {
            ui.label("smp num 44k");
            ui.add(egui::DragValue::new(&mut noise.smp_num_44k));
            let mut i = 0;
            let total = noise.units.len();
            noise.units.retain(|unit| {
                let mut retain = true;
                ui.horizontal(|ui| {
                    ui.strong(format!("design unit {}/{total}", i + 1));
                    if ui.button("-").clicked() {
                        retain = false;
                    }
                });

                ui.indent(egui::Id::new("du").with(i), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("pan");
                        ui.add(egui::Slider::new(&mut unit.pan, -100..=100));
                    });
                    let clicked = osci_ui(
                        ui,
                        &mut unit.main,
                        "main",
                        unit.ser_flags.contains(NoiseDesignUnitFlags::OSC_MAIN),
                    );
                    if clicked {
                        unit.ser_flags.toggle(NoiseDesignUnitFlags::OSC_MAIN);
                    }
                    let clicked = osci_ui(
                        ui,
                        &mut unit.freq,
                        "freq",
                        unit.ser_flags.contains(NoiseDesignUnitFlags::OSC_FREQ),
                    );
                    if clicked {
                        unit.ser_flags.toggle(NoiseDesignUnitFlags::OSC_FREQ);
                    }
                    let clicked = osci_ui(
                        ui,
                        &mut unit.volu,
                        "volu",
                        unit.ser_flags.contains(NoiseDesignUnitFlags::OSC_VOLU),
                    );
                    if clicked {
                        unit.ser_flags.toggle(NoiseDesignUnitFlags::OSC_VOLU);
                    }
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
                });
                i += 1;

                retain
            });
            ui.separator();

            if ui.button("+Add unit").clicked() {
                noise.units.push(NoiseDesignUnit::default());
            }
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
                ui.label("Kind");
                if ui
                    .selectable_label(
                        matches!(wave_data.points, WaveDataPoints::Coord { .. }),
                        (img::SAXO, "Coordinate"),
                    )
                    .clicked()
                {
                    app_cmd.modal(move |m| {
                        m.replace_wave_data_slot(voice_idx, sel_slot, square_wave())
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
                        )
                    });
                }
            });
            match &mut wave_data.points {
                WaveDataPoints::Coord { points, resolution } => {
                    ui.horizontal_top(|ui| {
                        draw_coord_wavebox(ui, points, resolution);
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Resolution");
                            ui.add(egui::DragValue::new(resolution)).changed();
                            ui.end_row();
                            ui.label(format!("{} points", points.len()));
                            if ui.button("+").clicked() {
                                points.push(OsciPt {
                                    x: points.last().map_or(0, |pt| pt.x) + 16,
                                    y: 0,
                                });
                            }
                            if ui.button("-").clicked() {
                                points.pop();
                            }
                            ui.end_row();
                            for pt in &mut *points {
                                ui.add(egui::DragValue::new(&mut pt.x).prefix("x "))
                                    .changed();
                                ui.add(egui::DragValue::new(&mut pt.y).prefix("y "))
                                    .changed();
                            }
                        });
                    });
                }
                WaveDataPoints::Overtone { points } => {
                    ui.horizontal_top(|ui| {
                        draw_overtone_wavebox(ui, wave_data.volume, points);
                        ui.horizontal_wrapped(|ui| {
                            ui.style_mut().spacing.slider_width = 512.0;
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
                            ui.end_row();
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
    // If the sound is aux playing currently, update its buffer as well
    if let Some(key) = ui_state.playing_sounds.get(&voice_idx) {
        aux.send
            .send(AuxMsg::PlaySamples16 {
                key: *key,
                sample_data: bytemuck::pod_collect_to_vec(&slot.inst.sample_buf),
            })
            .unwrap();
    }
    ui.horizontal_wrapped(|ui| {
        ui.label("Flags");
        for (name, flag) in VoiceFlags::iter_defined_names() {
            let mut contains = slot.unit.flags.contains(flag);
            if ui.checkbox(&mut contains, name).clicked() {
                slot.unit.flags ^= flag;
            }
        }
        ui.end_row();
        ui.label("Basic key");
        ui.add(egui::DragValue::new(&mut slot.unit.basic_key));
        ui.label("Tuning");
        ui.add(egui::DragValue::new(&mut slot.unit.tuning).speed(0.001));
    });
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
    ui.horizontal(|ui| {
        ui.label("Volume");
        ui.add(egui::DragValue::new(&mut data.volume));
        ui.label("Pan");
        ui.add(egui::Slider::new(&mut data.pan, 0..=128));
    });
    // When calculating width, ignore last point (release)
    let env_w: u16 = data.envelope.points[..data.envelope.points.len().saturating_sub(1)]
        .iter()
        .map(|pt| pt.x)
        .sum();
    ui.horizontal(|ui| {
        ui.strong(format!("Envelope ({} points)", data.envelope.points.len()));
        ui.label("fps");
        ui.add(egui::DragValue::new(&mut data.envelope.seconds_per_point).range(1..=999_999));
        if ui.button("+").clicked() {
            data.envelope.points.push(EnvPt {
                x: data.envelope.points.last().map_or(0, |pt| pt.x) + 16,
                y: 0,
            });
        }
        if ui.button("-").clicked() {
            data.envelope.points.pop();
        }
        if ui.button("Recalculate").clicked() {
            slot.inst.recalc_envelope(out_rate, &data.envelope);
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

fn draw_coord_wavebox(ui: &mut egui::Ui, points: &[OsciPt], resolution: &mut u16) {
    let reso = f32::from(*resolution);
    let (rect, _re) = ui.allocate_exact_size(egui::vec2(reso, reso), egui::Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, PAL.wave_bg);
    let lc = rect.left_center();
    let mut egui_points: Vec<egui::Pos2> = points
        .iter()
        .map(|pt| egui::pos2(lc.x + f32::from(pt.x), lc.y - f32::from(pt.y)))
        .collect();
    // pxtone Voice seems to add this point when drawing it
    egui_points.push(rect.right_center());
    p.line(egui_points, egui::Stroke::new(2.0, PAL.wave_stroke));
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
