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
        pxtone_misc::reset_voice_for_units_with_voice_idx,
    },
    arrayvec::ArrayVec,
    bitflags::Flags as _,
    eframe::egui::{self, AtomExt, collapsing_header::CollapsingState},
    ptcow::{
        Bps, ChNum, EnvPt, NoiseData, NoiseDesignOscillator, NoiseDesignUnit, NoiseDesignUnitFlags,
        NoiseTable, NoiseType, OsciArgs, OsciPt, SampleRate, UnitIdx, Voice, VoiceData, VoiceFlags,
        VoiceIdx, VoiceInstance, VoiceUnit, WaveData, noise_to_pcm,
    },
    rustc_hash::FxHashMap,
};

#[derive(Default)]
pub struct VoicesUiState {
    pub selected_idx: VoiceIdx,
    selected_vu: u8,
    dragged_idx: Option<VoiceIdx>,
    // Keep track of (preview) sounds playing for each voice
    playing_sounds: FxHashMap<VoiceIdx, AuxAudioKey>,
}

trait AtomExtExt<'a> {
    fn smol(self) -> egui::Atom<'a>;
}

impl<'a, T: AtomExt<'a>> AtomExtExt<'a> for T {
    fn smol(self) -> egui::Atom<'a> {
        self.atom_size(egui::vec2(16.0, 16.0))
    }
}

fn square_wave() -> WaveData {
    WaveData::Coord {
        points: vec![
            OsciPt { x: 0, y: 0 },
            OsciPt { x: 1, y: 48 },
            OsciPt { x: 99, y: 48 },
            OsciPt { x: 100, y: -48 },
            OsciPt { x: 199, y: -48 },
        ],
        resolution: 200,
    }
}

fn bass_drum() -> NoiseData {
    NoiseData {
        smp_num_44k: 8000,
        units: ArrayVec::try_from(
            &[NoiseDesignUnit {
                enves: [
                    EnvPt { x: 1, y: 100 },
                    EnvPt { x: 100, y: 20 },
                    EnvPt { x: 200, y: 0 },
                ]
                .into(),

                pan: 0,
                main: NoiseDesignOscillator {
                    type_: NoiseType::Sine,
                    freq: 50.0,
                    volume: 180.0,
                    offset: 2.0,
                    invert: false,
                },
                freq: NoiseDesignOscillator {
                    type_: NoiseType::Saw,
                    freq: 5.0,
                    volume: 2.0,
                    offset: 0.0,
                    invert: false,
                },
                volu: NoiseDesignOscillator::default(),
                ser_flags: NoiseDesignUnitFlags::OSC_MAIN,
            }][..],
        )
        .unwrap(),
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
                let mut voice = Voice {
                    name: format!("Wave {}", song.ins.voices.len()),
                    ..Default::default()
                };
                voice.allocate::<false>();
                voice.units[0].pan = 64;
                voice.units[0].volume = 127;
                voice.units[0].flags |= VoiceFlags::WAVE_LOOP;
                voice.units[0].data = VoiceData::Wave(square_wave());
                song.ins.voices.push(voice);
                let idx = VoiceIdx(song.ins.voices.len() as u8 - 1);
                ui_state.selected_idx = idx;
                reset_voice_for_units_with_voice_idx(song, idx);
            }
            if ui.button((img::DRUM.smol(), "Noise")).clicked() {
                let mut voice = Voice {
                    name: format!("Noise {}", song.ins.voices.len()),
                    ..Default::default()
                };
                voice.allocate::<false>();
                voice.units[0].pan = 64;
                voice.units[0].volume = 127;
                voice.units[0].data = VoiceData::Noise(bass_drum());
                song.ins.voices.push(voice);
                let idx = VoiceIdx(song.ins.voices.len() as u8 - 1);
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
        });
        for (i, voice) in song.ins.voices.iter().enumerate() {
            let idx = VoiceIdx(i as u8);
            let img = voice_img(voice);
            let button = egui::Button::selectable(ui_state.selected_idx == idx, (img, &voice.name))
                .sense(egui::Sense::click_and_drag());
            let re = ui.add(button);
            re.context_menu(|ui| {
                if ui.button("Duplicate").clicked() {
                    op = Some(VoiceUiOp::Duplicate(idx));
                }
            });
            if re.clicked() {
                ui_state.selected_idx = idx;
            }
            if re.drag_started() {
                ui_state.dragged_idx = Some(idx);
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
                    op = Some(VoiceUiOp::Swap(dragged_idx, idx));
                }
            }
            if re.hovered() {
                for (i, unit) in song.herd.units.iter().enumerate() {
                    if unit.voice_idx == idx {
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
            if let Some(voice) = song.ins.voices.get_mut(ui_state.selected_idx.usize()) {
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
        });
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
                let dup = song.ins.voices[idx.usize()].clone();
                song.ins.voices.insert(idx.usize(), dup);
            }
            VoiceUiOp::Delete(idx) => {
                song.ins.voices.remove(idx.usize());
            }
        }
    }
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
        for inst in voice.insts.iter() {
            play_sound_ui(ui, aux, ui_state, idx, &inst.sample_buf);
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
                app_cmd.push(Cmd::ResetUnitVoice {
                    unit: unit_idx,
                    voice: idx,
                });
            }
        }
    });

    voice_ui_inner(ui, voice, idx, out_rate, aux, ui_state);
}

pub fn voice_ui_inner(
    ui: &mut egui::Ui,
    voice: &mut Voice,
    voice_idx: VoiceIdx,
    out_rate: SampleRate,
    aux: &mut AuxAudioState,
    ui_state: &mut VoicesUiState,
) {
    ui.horizontal(|ui| {
        for (i, unit) in voice.units.iter().enumerate() {
            ui.selectable_value(
                &mut ui_state.selected_vu,
                i as u8,
                (
                    egui::Image::new(voice_data_img(&unit.data)),
                    format!("Unit {i}"),
                ),
            );
        }
        ui.separator();
        if ui
            .add_enabled(
                voice.insts.len() < 2 && voice.units.len() < 2,
                egui::Button::new("+"),
            )
            .clicked()
        {
            voice.units.push(voice.units[0].clone());
            voice.insts.push(VoiceInstance::default());
        }
        if ui
            .add_enabled(
                voice.insts.len() > 1 && voice.units.len() > 1,
                egui::Button::new("-"),
            )
            .clicked()
        {
            voice.insts.pop();
            voice.units.pop();
        }
    });
    ui.separator();
    // Ensure no out of bounds indexing (there is always at least one unit)
    if ui_state.selected_vu as usize >= voice.units.len() {
        ui_state.selected_vu = 0;
    }
    let unit = &mut voice.units[ui_state.selected_vu as usize];
    let inst = &mut voice.insts[ui_state.selected_vu as usize];
    voice_unit_ui(
        ui,
        unit,
        inst,
        out_rate,
        voice_idx,
        ui_state,
        aux,
        ui_state.selected_vu,
    );
    let id = ui.make_persistent_id("inst");
    CollapsingState::load_with_default_open(ui.ctx(), id, false)
        .show_header(ui, |ui| {
            ui.strong("Instance");
        })
        .body(|ui| {
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
            waveform_edit_widget(ui, &mut inst.sample_buf, 256., egui::Id::new("smp_buf"));
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
                waveform_edit_widget(ui, &mut inst.env, 256.0, egui::Id::new("env_buf"));
            }
            ui.label("Envelope release");
            ui.add(egui::DragValue::new(&mut inst.env_release));
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
    unit: &mut VoiceUnit,
    inst: &mut VoiceInstance,
    out_rate: SampleRate,
    voice_idx: VoiceIdx,
    ui_state: &mut VoicesUiState,
    aux: &AuxAudioState,
    unit_idx: u8,
) {
    match &mut unit.data {
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
            ui.horizontal(|ui| {
                ui.label("Kind");
                if ui
                    .selectable_label(
                        matches!(*wave_data, WaveData::Coord { .. }),
                        (img::SAXO, "Coordinate"),
                    )
                    .clicked()
                {
                    *wave_data = square_wave();
                }
                if ui
                    .selectable_label(
                        matches!(wave_data, WaveData::Overtone { .. }),
                        (img::ACCORDION, "Overtone"),
                    )
                    .clicked()
                {
                    *wave_data = WaveData::Overtone {
                        points: vec![OsciPt { x: 1, y: 16 }],
                    };
                }
            });
            match wave_data {
                WaveData::Coord { points, resolution } => {
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
                WaveData::Overtone { points } => {
                    ui.horizontal_top(|ui| {
                        draw_overtone_wavebox(ui, unit.volume, points);
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
            };

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
    unit_envelope_ui(ui, unit, inst, out_rate, unit_idx);
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
    });
}

fn unit_envelope_ui(
    ui: &mut egui::Ui,
    unit: &mut VoiceUnit,
    inst: &mut VoiceInstance,
    out_rate: u16,
    unit_idx: u8,
) {
    let env_w: u16 = unit.envelope.points.iter().map(|pt| pt.x).sum();
    ui.horizontal(|ui| {
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
    });
    ui.horizontal_top(|ui| {
        if !unit.envelope.points.is_empty() {
            draw_envelope_src(unit, ui, env_w, unit_idx);
        }
        ui.horizontal_wrapped(|ui| {
            if let Some((last, init)) = unit.envelope.points.split_last_mut() {
                for pt in init {
                    ui.add(egui::DragValue::new(&mut pt.x).prefix("x "));
                    ui.add(egui::DragValue::new(&mut pt.y).prefix("y "));
                }
                ui.end_row();
                ui.label(format!("envelope width: {env_w}"));
                ui.end_row();
                ui.label("Tail");
                ui.add(egui::DragValue::new(&mut last.x).prefix("x "));
                ui.add(egui::DragValue::new(&mut last.y).prefix("y "));
            }
        });
    });
}

fn draw_overtone_wavebox(ui: &mut egui::Ui, volume: i16, points: &[OsciPt]) {
    let size: u16 = 256;
    let (rect, _re) = ui.allocate_exact_size(
        egui::vec2(size as f32, size as f32),
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
    for i in 0..size + 1 {
        let amp = ptcow::overtone(args, points, i);
        let y = (amp * size as f64 / 2.0) as f32;
        egui_points.push(egui::pos2(lc.x + i as f32, lc.y - y));
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
        .map(|pt| egui::pos2(lc.x + pt.x as f32, lc.y - pt.y as f32))
        .collect();
    // pxtone Voice seems to add this point when drawing it
    egui_points.push(rect.right_center());
    p.line(egui_points, egui::Stroke::new(2.0, PAL.wave_stroke));
}

fn draw_envelope_src(unit: &mut VoiceUnit, ui: &mut egui::Ui, width: u16, unit_idx: u8) {
    let w = width as f32;
    egui::ScrollArea::horizontal()
        .id_salt(unit_idx)
        .max_width(384.0)
        .show(ui, |ui| {
            let (rect, _re) =
                ui.allocate_exact_size(egui::vec2(w, 128.0), egui::Sense::click_and_drag());
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
        });
}
