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
    },
    bitflags::Flags as _,
    eframe::egui,
    ptcow::{
        Bps, ChNum, EnvPt, NoiseData, NoiseDesignOscillator, NoiseDesignUnit, NoiseDesignUnitFlags,
        NoiseTable, NoiseType, OsciArgs, OsciPt, SampleRate, UnitIdx, Voice, VoiceData, VoiceFlags,
        VoiceIdx, VoiceInstance, VoiceUnit, WaveData, noise_to_pcm,
    },
    rustc_hash::FxHashMap,
    std::iter::zip,
};

#[derive(Default)]
pub struct VoicesUiState {
    pub selected_idx: usize,
    dragged_idx: Option<usize>,
    // Keep track of (preview) sounds playing for each voice
    playing_sounds: FxHashMap<VoiceIdx, AuxAudioKey>,
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
        ui.menu_button("+ Add", |ui| {
            if ui.button("Wave").clicked() {
                let mut voice = Voice {
                    name: format!("Wave {}", song.ins.voices.len()),
                    ..Default::default()
                };
                voice.allocate::<false>();
                voice.units[0].pan = 64;
                voice.units[0].volume = 127;
                voice.units[0].flags |= VoiceFlags::WAVE_LOOP;
                voice.units[0].data = VoiceData::Wave(WaveData::Coord {
                    resolution: 64,
                    points: Vec::new(),
                });
                song.ins.voices.push(voice);
            }
            if ui.button("Noise").clicked() {
                let mut voice = Voice {
                    name: format!("Noise {}", song.ins.voices.len()),
                    ..Default::default()
                };
                voice.allocate::<false>();
                voice.units[0].pan = 64;
                voice.units[0].volume = 127;
                voice.units[0].flags |= VoiceFlags::WAVE_LOOP;
                voice.units[0].data = VoiceData::Noise(NoiseData::default());
                song.ins.voices.push(voice);
            }
        });
        ui.menu_button("Import ->", |ui| {
            if ui.button(".ptvoice").clicked() {
                app_cmd.push(Cmd::PromptImportPtVoice);
            }
            if ui.button(".ptnoise").clicked() {
                app_cmd.push(Cmd::PromptImportPtNoise);
            }
            if ui.button("Single from .sf2").clicked() {
                app_cmd.push(Cmd::PromptImportSf2Sound);
            }
        });
        ui.menu_button("Replace ->", |ui| {
            if ui.button("All from .ptcop...").clicked() {
                app_cmd.push(Cmd::PromptReplaceAllPtcop);
            }
            if ui.button("Current from .ptvoice").clicked() {
                app_cmd.push(Cmd::PromptReplacePtVoiceSingle(VoiceIdx(
                    ui_state.selected_idx as u8,
                )));
            }
            if ui.button("Current from .ptnoise").clicked() {
                app_cmd.push(Cmd::PromptReplacePtNoiseSingle(VoiceIdx(
                    ui_state.selected_idx as u8,
                )));
            }
            if ui.button("Current from .sf2").clicked() {
                app_cmd.push(Cmd::PromptReplaceSf2Single(VoiceIdx(
                    ui_state.selected_idx as u8,
                )));
            }
        });
        for (i, voice) in song.ins.voices.iter().enumerate() {
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
                let voice_idx = VoiceIdx(i as u8);
                for (i, unit) in song.herd.units.iter().enumerate() {
                    if unit.voice_idx == voice_idx {
                        shared.highlight_set.insert(UnitIdx(i as u8));
                    }
                }
            }
        }
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
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
                );
            }
        });
    if let Some(op) = op {
        match op {
            VoiceUiOp::MoveUp(idx) => {
                let voice = song.ins.voices.remove(idx);
                song.ins.voices.insert(idx.saturating_sub(1), voice);
            }
            VoiceUiOp::MoveDown(idx) => {
                let voice = song.ins.voices.remove(idx);
                song.ins.voices.insert(idx + 1, voice);
            }
            VoiceUiOp::MoveBegin(idx) => {
                let voice = song.ins.voices.remove(idx);
                song.ins.voices.insert(0, voice);
            }
            VoiceUiOp::MoveEnd(idx) => {
                let voice = song.ins.voices.remove(idx);
                song.ins.voices.push(voice);
            }
            VoiceUiOp::Swap(a, b) => {
                song.ins.voices.swap(a, b);
            }
            VoiceUiOp::Duplicate(idx) => {
                let dup = song.ins.voices[idx].clone();
                song.ins.voices.insert(idx, dup);
            }
            VoiceUiOp::Delete(idx) => {
                song.ins.voices.remove(idx);
            }
        }
    }
}

enum VoiceUiOp {
    MoveUp(usize),
    MoveDown(usize),
    MoveBegin(usize),
    MoveEnd(usize),
    Delete(usize),
    Swap(usize, usize),
    Duplicate(usize),
}

fn voice_ui(
    ui: &mut egui::Ui,
    voice: &mut Voice,
    idx: usize,
    op: &mut Option<VoiceUiOp>,
    out_rate: SampleRate,
    aux: &mut Option<AuxAudioState>,
    ui_state: &mut VoicesUiState,
    piano_state: &FreeplayPianoState,
    herd: &mut ptcow::Herd,
) {
    let aux = aux.get_or_insert_with(|| crate::audio_out::spawn_aux_audio_thread(out_rate, 1024));
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut voice.name);
        for inst in voice.insts.iter() {
            play_sound_ui(ui, aux, ui_state, VoiceIdx(idx as u8), &inst.sample_buf);
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
            let unit = &mut herd.units[unit_idx.usize()];
            let label = egui::RichText::new(format!("🎹 Test with {}", unit.name))
                .color(unit_color(unit_idx.usize()));
            if ui.button(label).clicked() {
                unit.voice_idx = VoiceIdx(idx as u8);
            }
        }
    });

    voice_ui_inner(
        ui,
        voice,
        VoiceIdx(idx.try_into().unwrap()),
        out_rate,
        aux,
        ui_state,
    );
}

pub fn voice_ui_inner(
    ui: &mut egui::Ui,
    voice: &mut Voice,
    voice_idx: VoiceIdx,
    out_rate: SampleRate,
    aux: &mut AuxAudioState,
    ui_state: &mut VoicesUiState,
) {
    let total = voice.units.len();
    for (i, (unit, inst)) in zip(&mut voice.units, &mut voice.insts).enumerate() {
        ui.horizontal(|ui| {
            ui.image(voice_data_img(&unit.data));
            ui.strong(format!("unit {}/{total}", i + 1));
        });
        ui.indent("vu", |ui| {
            voice_unit_ui(ui, unit, inst, out_rate, voice_idx, ui_state, aux);
        });
        ui.strong(format!("instance {}/{total}", i + 1));
        ui.indent("vi", |ui| {
            ui.horizontal(|ui| {
                ui.label("Sample buf");
                play_sound_ui(ui, aux, ui_state, voice_idx, &inst.sample_buf);
                let mut len = inst.sample_buf.len();
                if ui
                    .add(egui::DragValue::new(&mut len).update_while_editing(false))
                    .changed()
                {
                    inst.sample_buf.resize(len, 0);
                }
                ui.label("Number of samples");
                ui.add(egui::DragValue::new(&mut inst.num_samples));
            });
            waveform_edit_widget(
                ui,
                &mut inst.sample_buf,
                256.,
                egui::Id::new("smp_buf").with(i),
            );
            ui.horizontal(|ui| {
                ui.label("Envelope");
                let mut len = inst.env.len();
                if ui
                    .add(egui::DragValue::new(&mut len).update_while_editing(false))
                    .changed()
                {
                    inst.env.resize(len, 0);
                }
            });
            if !inst.env.is_empty() {
                waveform_edit_widget(ui, &mut inst.env, 256.0, egui::Id::new("env_buf").with(i));
            }
            ui.label("Envelope release");
            ui.add(egui::DragValue::new(&mut inst.env_release));
        });
    }
    if voice.insts.len() > 1 && voice.units.len() > 1 {
        if ui.button("pop").clicked() {
            voice.insts.pop();
            voice.units.pop();
        }
    }
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
            ui.add(egui::DragValue::new(&mut osci.freq));
            ui.label("volume");
            ui.add(egui::DragValue::new(&mut osci.volume));
            ui.label("offset");
            ui.add(egui::DragValue::new(&mut osci.offset));
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
    unit: &mut VoiceUnit,
    inst: &mut VoiceInstance,
    out_rate: SampleRate,
    voice_idx: VoiceIdx,
    ui_state: &mut VoicesUiState,
    aux: &AuxAudioState,
) {
    match &mut unit.data {
        VoiceData::Noise(noise) => {
            ui.label("smp num 44k");
            ui.add(egui::DragValue::new(&mut noise.smp_num_44k));
            noise.units.retain(|unit| {
                let mut retain = true;
                ui.horizontal(|ui| {
                    ui.label("pan");
                    ui.add(egui::DragValue::new(&mut unit.pan));
                    if ui.button("-").clicked() {
                        retain = false;
                    }
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
                });
                for env_pt in &mut unit.enves {
                    ui.add(egui::DragValue::new(&mut env_pt.x));
                    ui.add(egui::DragValue::new(&mut env_pt.y));
                }
                retain
            });

            if ui.button("+").clicked() {
                noise.units.push(NoiseDesignUnit::default());
            }
            let tbl = NoiseTable::generate();
            inst.sample_buf = noise_to_pcm(noise, &tbl).smp;
            inst.num_samples = noise.smp_num_44k;
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
            ui.horizontal_wrapped(|ui| {
                match wave_data {
                    WaveData::Coord { points, resolution } => {
                        ui.label(format!("{} points", points.len()));
                        for pt in &mut *points {
                            ui.add(egui::DragValue::new(&mut pt.x).prefix("x "))
                                .changed();
                            ui.add(egui::DragValue::new(&mut pt.y).prefix("y "))
                                .changed();
                        }
                        if ui.button("+").clicked() {
                            points.push(OsciPt {
                                x: points.last().map_or(0, |pt| pt.x) + 16,
                                y: 0,
                            });
                        }
                        if ui.button("-").clicked() {
                            points.pop();
                        }
                        ui.label("Resolution");
                        ui.add(egui::DragValue::new(resolution)).changed();
                    }
                    WaveData::Overtone { points } => {
                        ui.label(format!("{} points", points.len()));
                        for pt in &mut *points {
                            ui.add(egui::DragValue::new(&mut pt.x).prefix("x "))
                                .changed();
                            ui.add(egui::DragValue::new(&mut pt.y).prefix("y "))
                                .changed();
                        }
                        if ui.button("+").clicked() {
                            points.push(OsciPt {
                                x: points.last().map_or(0, |pt| pt.x) + 16,
                                y: 0,
                            });
                        }
                        if ui.button("-").clicked() {
                            points.pop();
                        }
                    }
                };

                ui.end_row();
                ui.label("Kind");
                if ui
                    .selectable_label(
                        matches!(*wave_data, WaveData::Coord { .. }),
                        (img::SAXO, "Coordinate"),
                    )
                    .clicked()
                {
                    *wave_data = WaveData::Coord {
                        points: Vec::new(),
                        resolution: 64,
                    };
                }
                if ui
                    .selectable_label(
                        matches!(wave_data, WaveData::Overtone { .. }),
                        (img::ACCORDION, "Overtone"),
                    )
                    .clicked()
                {
                    *wave_data = WaveData::Overtone { points: Vec::new() };
                }
            });

            match wave_data {
                WaveData::Coord { points, resolution } => {
                    let reso = f32::from(*resolution);
                    let (rect, _re) = ui
                        .allocate_exact_size(egui::vec2(reso, reso), egui::Sense::click_and_drag());
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, 2.0, PAL.wave_bg);
                    let lc = rect.left_center();
                    let mut egui_points: Vec<egui::Pos2> = points
                        .iter()
                        .map(|pt| egui::pos2(lc.x + pt.x as f32, lc.y - pt.y as f32))
                        .collect();
                    // pxtone Voice seems to add this point when drawing it
                    egui_points.push(rect.right_center());
                    p.line(egui_points, egui::Stroke::new(2.0, PAL.wave_stroke));
                }
                WaveData::Overtone { points } => {
                    let size: u16 = 256;
                    let (rect, _re) = ui.allocate_exact_size(
                        egui::vec2(size as f32, size as f32),
                        egui::Sense::click_and_drag(),
                    );
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, 2.0, PAL.wave_bg);
                    let lc = rect.left_center();
                    let args = OsciArgs {
                        volume: unit.volume,
                        sample_num: size.into(),
                    };
                    let mut egui_points: Vec<egui::Pos2> = Vec::new();
                    for i in 0..size + 1 {
                        let amp = ptcow::overtone(args, points, i);
                        let y = (amp * size as f64) as f32;
                        egui_points.push(egui::pos2(lc.x + i as f32, lc.y - y));
                    }
                    p.line(egui_points, egui::Stroke::new(2.0, PAL.wave_stroke));
                }
            }
            inst.recalc_wave_data(wave_data, unit.volume, unit.pan);
        }
        VoiceData::OggV(oggv) => {
            ui.label("Ogg/Vorbis voice");
            ui.label("channel number");
            ui.add(egui::DragValue::new(&mut oggv.ch));
            ui.label("sps2");
            ui.add(egui::DragValue::new(&mut oggv.sps2));
        }
    }
    // If the sound is aux playing currently, update its buffer as well
    if let Some(key) = ui_state.playing_sounds.get(&voice_idx) {
        aux.send
            .send(AuxMsg::PlaySamples16 {
                key: *key,
                sample_data: bytemuck::pod_collect_to_vec(&inst.sample_buf),
            })
            .unwrap();
    }
    ui.horizontal_wrapped(|ui| {
        ui.label("Flags");
        for (name, flag) in VoiceFlags::iter_defined_names() {
            let mut contains = unit.flags.contains(flag);
            if ui.checkbox(&mut contains, name).clicked() {
                unit.flags ^= flag;
            }
        }
        ui.end_row();
        ui.label("Basic key");
        ui.add(egui::DragValue::new(&mut unit.basic_key));
        ui.end_row();
        ui.label("Volume");
        ui.add(egui::DragValue::new(&mut unit.volume));
        ui.label("Pan");
        ui.add(egui::Slider::new(&mut unit.pan, 0..=128));
        ui.label("Tuning");
        ui.add(egui::DragValue::new(&mut unit.tuning).speed(0.001));
        ui.end_row();
        ui.strong(format!("Envelope ({} points)", unit.envelope.points.len()));
        ui.label("fps");
        ui.add(egui::DragValue::new(&mut unit.envelope.seconds_per_point));
        if ui.button("+").clicked() {
            unit.envelope.points.push(EnvPt {
                x: unit.envelope.points.last().map_or(0, |pt| pt.x) + 16,
                y: 0,
            });
        }
        if ui.button("-").clicked() {
            unit.envelope.points.pop();
        }
        if ui.button("Recalculate").clicked() {
            inst.recalc_envelope(unit, out_rate);
        }
        ui.end_row();
        let mut x_cursor = 0;
        if let Some((last, init)) = unit.envelope.points.split_last_mut() {
            for pt in init {
                x_cursor += pt.x;
                ui.label("x");
                ui.add(egui::DragValue::new(&mut pt.x));
                ui.label("y");
                ui.add(egui::DragValue::new(&mut pt.y));
            }
            ui.label(format!("envelope width: {x_cursor}"));
            ui.label("Tail");
            ui.add(egui::DragValue::new(&mut last.x));
            ui.add(egui::DragValue::new(&mut last.y));
        }
        ui.end_row();
        if !unit.envelope.points.is_empty() {
            envelope_src_ui(unit, ui, x_cursor);
        }
    });
}

fn envelope_src_ui(unit: &mut VoiceUnit, ui: &mut egui::Ui, x_cursor: u16) {
    let (rect, _re) = ui.allocate_exact_size(
        egui::vec2(x_cursor as f32, 128.0),
        egui::Sense::click_and_drag(),
    );
    let p = ui.painter_at(rect);
    let lb = rect.left_bottom();
    p.rect_filled(rect, 2.0, PAL.env_bg);
    let mut x_cursor = 0;
    let mut egui_points: Vec<egui::Pos2> = unit
        .envelope
        .points
        .iter()
        .map(|pt| {
            x_cursor += pt.x;
            egui::pos2(lb.x + x_cursor as f32, lb.y - pt.y as f32)
        })
        .collect();
    // Ptvoice seems to have a point at (0, bottom) when drawing
    egui_points.insert(0, egui::pos2(lb.x, lb.y));
    p.line(egui_points, egui::Stroke::new(2.0, PAL.env_stroke));
}
