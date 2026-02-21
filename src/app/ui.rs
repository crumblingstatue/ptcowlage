pub mod file_ops;
mod img;
pub mod left_panel;
pub mod top_panel;
mod unit;

use {
    crate::{
        app::{
            SongState,
            ui::{
                tabs::{
                    effects::EffectsUiState, events::RawEventsUiState, map::MapState,
                    piano_roll::PianoRollState, voices::VoicesUiState,
                },
                unit::{unit_color, unit_voice_img},
            },
        },
        audio_out::{AuxAudioState, AuxMsg, SongStateHandle},
    },
    eframe::egui::{self, AtomExt},
    egui_toast::Toasts,
    ptcow::{
        Event, EventPayload, GroupIdx, PcmData, SampleRate, UnitIdx, Voice, VoiceData, VoiceIdx,
    },
    rustc_hash::FxHashSet,
    rustysynth::SoundFont,
};

pub mod tabs {
    pub mod effects;
    pub mod events;
    pub mod map;
    pub mod piano_roll;
    pub mod playback;
    pub mod unit;
    pub mod voices;
}

pub struct FreeplayPianoState {
    /// Make this unit available for keyboard play
    pub toot: Option<UnitIdx>,
    /// Toot duration
    duration: u32,
    play_octave: i32,
    // Live record performance (store toots in events)
    record: bool,
}

impl Default for FreeplayPianoState {
    fn default() -> Self {
        Self {
            toot: None,
            // Long enough for default "moo" noise :)
            duration: 1024,
            play_octave: 7,
            record: false,
        }
    }
}

const FACTOR: i32 = 256;
pub const fn piano_key_to_pxtone_key(key: i32) -> i32 {
    FACTOR * key
}

fn piano_freeplay_ui(
    song: &mut SongState,
    dst_sps: SampleRate,
    ui: &mut egui::Ui,
    state: &mut FreeplayPianoState,
    file_dia_open: bool,
) {
    // Avoid tooting when we're inside a text edit, etc.
    if !ui.ctx().wants_keyboard_input() {
        piano_freeplay_input(song, dst_sps, ui, state, file_dia_open);
    }
    let (selected_text, selected_color) = state.toot.map_or(("None", egui::Color32::GRAY), |idx| {
        (
            song.herd
                .units
                .get(idx.usize())
                .map_or("<invalid>", |unit| &unit.name),
            unit_color(idx.usize()),
        )
    });
    ui.label("🎹").on_hover_text("Piano freeplay UI");
    if let Some(toot) = state.toot
        && let Some(unit) = song.herd.units.get(toot.usize())
    {
        ui.image(unit_voice_img(&song.ins, unit));
    }
    egui::ComboBox::new("unit_cb", "Unit")
        .selected_text(egui::RichText::new(selected_text).color(selected_color))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut state.toot, None, "None");
            for unit_idx in 0..song.herd.units.len() {
                let unit = &song.herd.units[unit_idx];
                ui.selectable_value(
                    &mut state.toot,
                    Some(UnitIdx(unit_idx as u8)),
                    (
                        unit_voice_img(&song.ins, unit).atom_size(egui::vec2(14.0, 14.0)),
                        egui::RichText::new(&unit.name).color(unit_color(unit_idx)),
                    ),
                );
            }
        });
    ui.label("Octave");
    ui.add(
        egui::DragValue::new(&mut state.play_octave)
            .speed(0.05)
            .range(2..=9),
    );
    ui.label("Duration");
    ui.add(egui::DragValue::new(&mut state.duration));
    let c = if state.record {
        let time = ui.input(|i| i.time);
        let t = ((time * 6.0).sin() * 0.5 + 0.5) as f32;
        lerp_color(egui::Color32::RED, egui::Color32::YELLOW, t)
    } else {
        egui::Color32::GRAY
    };
    ui.checkbox(&mut state.record, egui::RichText::new("⏺ Record").color(c))
        .on_hover_text("Record freeplay (ctrl+space)");
}

fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from_rgba_unmultiplied(
        egui::lerp(a.r() as f32..=b.r() as f32, t) as u8,
        egui::lerp(a.g() as f32..=b.g() as f32, t) as u8,
        egui::lerp(a.b() as f32..=b.b() as f32, t) as u8,
        egui::lerp(a.a() as f32..=b.a() as f32, t) as u8,
    )
}

fn piano_freeplay_input(
    song: &mut SongState,
    dst_sps: SampleRate,
    ui: &mut egui::Ui,
    state: &mut FreeplayPianoState,
    file_dia_open: bool,
) {
    // Play a cow on the keyboard
    // Ignores the keyboard if an egui popup is open or the file dialog is open
    if let Some(unit_no) = state.toot
        && !egui::Popup::is_any_open(ui.ctx())
        && !file_dia_open
    {
        let piano_keys: [bool; 30] = ui.input(|inp| {
            let mut down = [false; _];
            let kb_keys = [
                // Lower
                &[egui::Key::Z][..],
                &[egui::Key::S],
                &[egui::Key::X],
                &[egui::Key::D],
                &[egui::Key::C],
                &[egui::Key::F],
                &[egui::Key::V],
                &[egui::Key::B],
                &[egui::Key::H, egui::Key::Num1],
                &[egui::Key::N, egui::Key::Q],
                &[egui::Key::J, egui::Key::Num2],
                &[egui::Key::M, egui::Key::W],
                // Upper
                &[egui::Key::E, egui::Key::Comma],
                &[egui::Key::Num4, egui::Key::L],
                &[egui::Key::R, egui::Key::Period],
                &[egui::Key::Num5, egui::Key::Semicolon],
                &[egui::Key::T, egui::Key::Slash],
                &[egui::Key::Quote, egui::Key::Num6],
                &[egui::Key::Y],
                &[egui::Key::U],
                &[egui::Key::Num8],
                &[egui::Key::I],
                &[egui::Key::Num9],
                &[egui::Key::O],
                &[egui::Key::P],
                &[egui::Key::Minus],
                &[egui::Key::OpenBracket],
                &[egui::Key::Equals],
                &[egui::Key::CloseBracket],
                &[egui::Key::Backspace],
            ];
            for ev in &inp.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    ..
                } = ev
                    && let Some(idx) = kb_keys
                        .iter()
                        .position(|kb_keys| kb_keys.iter().any(|kb_key| key == kb_key))
                {
                    down[idx] = true;
                }
            }
            down
        });
        for (key, down) in piano_keys.iter().enumerate() {
            if *down {
                // I dunno, magic
                let base_key = 8;
                let piano_key = base_key + (state.play_octave * 12) + key as i32;
                piano_freeplay_play_note(song, dst_sps, state, piano_key, unit_no);
            }
        }
    }
}

fn piano_freeplay_play_note(
    song: &mut SongState,
    dst_sps: u16,
    state: &mut FreeplayPianoState,
    piano_key: i32,
    unit_no: UnitIdx,
) {
    let tick = ptcow::current_tick(&song.herd, &song.ins);
    let ev = Event {
        payload: EventPayload::Key(piano_key_to_pxtone_key(piano_key)),
        unit: unit_no,
        tick,
    };
    if state.record {
        // If the song is paused, unpause it
        song.pause = false;
        // Push the event
        song.song.events.push(ev);
    }
    let _ = ptcow::do_event(
        &mut song.herd,
        &song.ins,
        &song.song.events,
        &song.song.master,
        tick,
        dst_sps,
        &ev,
    );
    let ev = Event {
        payload: EventPayload::On {
            duration: state.duration,
        },
        unit: unit_no,
        tick,
    };
    if state.record {
        song.song.events.push(ev);
    }
    let _ = ptcow::do_event(
        &mut song.herd,
        &song.ins,
        &song.song.events,
        &song.song.master,
        tick,
        dst_sps,
        &ev,
    );
    if state.record {
        song.song.events.sort();
    }
}

#[derive(Default)]
pub struct UiState {
    pub tab: Tab,
    pub piano_roll: PianoRollState,
    pub map: MapState,
    pub freeplay_piano: FreeplayPianoState,
    pub raw_events: RawEventsUiState,
    pub voices: VoicesUiState,
    pub effects: EffectsUiState,
    pub shared: SharedUiState,
    pub sf2_import: Option<Sf2ImportDialog>,
}

pub struct Sf2ImportDialog {
    soundfont: SoundFont,
    selected: Option<usize>,
    /// If `Some`, replace the voice at index with the import
    target_voice_idx: Option<VoiceIdx>,
    filter_string: String,
}

impl Sf2ImportDialog {
    /// If `target_voice_idx` is `None`, it's import rather than replace
    pub fn new(soundfont: SoundFont, target_voice_idx: Option<VoiceIdx>) -> Self {
        Self {
            soundfont,
            selected: None,
            target_voice_idx,
            filter_string: String::new(),
        }
    }
}

/// Ui state shared among different uis
pub struct SharedUiState {
    /// The active unit is the one that is:
    /// - Used to place notes in the piano roll
    /// - Shows up in the unit UI
    /// - Is highlighted in the left side units panel
    pub active_unit: Option<UnitIdx>,
    pub toasts: Toasts,
    /// Units in this set will be highlighted
    pub highlight_set: FxHashSet<UnitIdx>,
}

impl Default for SharedUiState {
    fn default() -> Self {
        Self {
            active_unit: Default::default(),
            toasts: Toasts::new()
                .anchor(egui::Align2::RIGHT_BOTTOM, egui::Pos2::ZERO)
                .direction(egui::Direction::BottomUp),
            highlight_set: FxHashSet::default(),
        }
    }
}

impl UiState {
    pub fn show_left_panel(&self) -> bool {
        !matches!(self.tab, Tab::Playback)
    }
}

#[derive(Default, PartialEq, Clone, Copy)]
pub enum Tab {
    #[default]
    Playback,
    Map,
    PianoRoll,
    Voices,
    Unit,
    Effects,
    Events,
}

pub fn central_panel(app: &mut super::App, ui: &mut egui::Ui) {
    // Check whether space key is pressed
    // We ignore it if a popup is open, also if file dialog is open, or egui wants input, etc.
    #[cfg(not(target_arch = "wasm32"))]
    let file_dia_open = app.file_dia.state() == &egui_file_dialog::DialogState::Open;
    #[cfg(target_arch = "wasm32")]
    let file_dia_open = false;
    let [k_space, m_ctrl] = ui.input(|inp| [inp.key_pressed(egui::Key::Space), inp.modifiers.ctrl]);
    let mut song = app.song.lock().unwrap();
    if k_space {
        if m_ctrl {
            // Toggle record
            app.ui_state.freeplay_piano.record ^= true;
        } else {
            // Toggle pause
            let should_toggle_pause = !egui::Popup::is_any_open(ui.ctx())
                && !file_dia_open
                && !ui.ctx().wants_keyboard_input();

            if should_toggle_pause {
                song.pause ^= true;
            }
        }
    }

    match app.ui_state.tab {
        Tab::Playback => tabs::playback::ui(
            ui,
            &mut song,
            &mut app.ui_state.freeplay_piano,
            &mut app.cmd,
            &mut app.modal_payload,
        ),
        Tab::Map => {
            tabs::map::ui(ui, &mut song, &mut app.ui_state.map, &mut app.cmd);
        }
        Tab::PianoRoll => {
            tabs::piano_roll::ui(
                ui,
                &mut song,
                &mut app.ui_state.piano_roll,
                &mut app.ui_state.shared,
                &mut app.cmd,
                app.out.rate,
                &mut app.ui_state.freeplay_piano,
            );
        }
        Tab::Events => tabs::events::ui(
            ui,
            &mut song,
            &mut app.ui_state.raw_events,
            app.out.rate,
            &mut app.cmd,
            &mut app.modal_payload,
        ),
        Tab::Voices => tabs::voices::ui(
            ui,
            &mut song,
            &mut app.ui_state.voices,
            &mut app.ui_state.shared,
            app.out.rate,
            &mut app.aux_state,
            &app.ui_state.freeplay_piano,
            &mut app.cmd,
        ),
        Tab::Unit => tabs::unit::ui(
            ui,
            &mut app.ui_state.shared,
            &mut song,
            &mut app.cmd,
            &mut app.modal_payload,
        ),
        Tab::Effects => tabs::effects::ui(
            ui,
            &mut song,
            app.out.rate,
            &mut app.ui_state.effects,
            &mut app.ui_state.shared,
        ),
    }
    drop(song);
}

/// Draws and edits a waveform.
/// `samples` are u8 values (0..255) representing the waveform.
/// `height` is the display height of the waveform rectangle.
pub fn waveform_edit_widget(ui: &mut egui::Ui, samples: &mut [u8], height: f32, id: egui::Id) {
    // We avoid rendering huge waveforms, which can tank performance, and cause audio glitching
    // due to lock contention
    if samples.len() > 32_768 {
        ui.label("Sample data too large to display");
        return;
    }
    // Unique ID to store previous pointer pos across frames
    let id = ui.id().with(id);

    // Allocate widget space
    let (rect, resp) = ui.allocate_exact_size(
        egui::Vec2::new(ui.available_width(), height),
        egui::Sense::click_and_drag(),
    );

    let hor_ratio = rect.width() / samples.len() as f32;
    let lt = rect.left_top();

    // Load previous pointer pos from temp storage
    let prev_pointer_pos: Option<egui::Pos2> = ui.data(|d| d.get_temp(id));

    // Get latest pointer pos (egui 0.33)
    let pointer_pos = ui.input(|i| i.pointer.latest_pos());

    // ---- Editing ----
    if resp.dragged()
        && let (Some(pos), Some(prev)) = (pointer_pos, prev_pointer_pos)
    {
        // Only draw inside the rect
        let pos = egui::Pos2::new(
            pos.x.clamp(rect.left(), rect.right()),
            pos.y.clamp(rect.top(), rect.bottom()),
        );
        let prev = egui::Pos2::new(
            prev.x.clamp(rect.left(), rect.right()),
            prev.y.clamp(rect.top(), rect.bottom()),
        );

        // Convert to sample-space coordinates
        let a = egui::Pos2::new((prev.x - lt.x) / hor_ratio, prev.y - lt.y);
        let b = egui::Pos2::new((pos.x - lt.x) / hor_ratio, pos.y - lt.y);

        for p in line_points_between(a, b) {
            let xi = p.x.round() as isize;
            if xi >= 0 && xi < samples.len() as isize {
                samples[xi as usize] = p.y.clamp(0.0, 255.0) as u8;
            }
        }
    }

    // Save current pointer pos for next frame
    if let Some(pos) = pointer_pos {
        ui.data_mut(|d| d.insert_temp(id, pos));
    } else {
        // Clear if pointer not present
        ui.data_mut(|d| d.remove::<egui::Pos2>(id));
    }

    // ---- Rendering ----
    let painter = ui.painter_at(rect);

    // Background
    painter.rect_filled(rect, 2.0, egui::Color32::BLACK);

    // Waveform line
    let points: Vec<egui::Pos2> = samples
        .iter()
        .enumerate()
        .map(|(i, val)| egui::Pos2::new(lt.x + i as f32 * hor_ratio, lt.y + *val as f32))
        .collect();

    painter.line(points, egui::Stroke::new(1.0, egui::Color32::YELLOW));
}

/// Bresenham line iterator (integer steps in sample space)
fn line_points_between(a: egui::Pos2, b: egui::Pos2) -> impl Iterator<Item = egui::Pos2> {
    let (mut x0, mut y0) = (a.x.round() as i32, a.y.round() as i32);
    let (x1, y1) = (b.x.round() as i32, b.y.round() as i32);

    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    std::iter::from_fn(move || {
        let p = egui::pos2(x0 as f32, y0 as f32);
        if x0 == x1 && y0 == y1 {
            return None;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
        Some(p)
    })
}

fn voice_img(voice: &Voice) -> egui::ImageSource<'static> {
    voice_data_img(&voice.units[0].data)
}

fn voice_data_img(data: &VoiceData) -> egui::ImageSource<'static> {
    match data {
        ptcow::VoiceData::Noise(_) => img::DRUM,
        ptcow::VoiceData::Pcm(_) => img::MIC,
        ptcow::VoiceData::Wave(data) => match data {
            ptcow::WaveData::Coord { .. } => img::SAXO,
            ptcow::WaveData::Overtone { .. } => img::ACCORDION,
        },
        ptcow::VoiceData::OggV(_) => img::FISH,
    }
}

fn voice_img_opt(opt_voice: Option<&Voice>) -> egui::ImageSource<'static> {
    match opt_voice {
        Some(voice) => voice_img(voice),
        None => img::X,
    }
}

/// Returns true if import ui should close
pub(crate) fn sf2_import_ui(
    ui: &mut egui::Ui,
    sf2: &mut Sf2ImportDialog,
    aux: &mut Option<AuxAudioState>,
    song: &SongStateHandle,
    out_rate: SampleRate,
) -> bool {
    let mut close = false;
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    if let Some(sel) = sf2.selected {
        let ins = &sf2.soundfont.get_instruments()[sel];
        ui.heading(ins.get_name());
        let aux = aux.get_or_insert_with(|| crate::audio_out::spawn_aux_audio_thread(44_100, 1024));
        for region in ins.get_regions() {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "coarse: {}, fine: {}",
                    region.get_coarse_tune(),
                    region.get_fine_tune()
                ));
                if ui.button("Play").clicked() {
                    let key = aux.next_key();
                    let start = region.get_sample_start() as usize;
                    let end = region.get_sample_end() as usize;
                    let sample_data: Vec<i16> = sf2.soundfont.get_wave_data()[start..end].to_vec();
                    aux.send
                        .send(crate::audio_out::AuxMsg::PlaySamples16 { key, sample_data })
                        .unwrap();
                }
                if ui.button("Import").clicked() {
                    let start = region.get_sample_start() as usize;
                    let end = region.get_sample_end() as usize;
                    let sample_data: Vec<i16> = sf2.soundfont.get_wave_data()[start..end].to_vec();
                    let pcm = PcmData {
                        ch: ptcow::ChNum::Mono,
                        sps: 44_100,
                        bps: ptcow::Bps::B16,
                        num_samples: sample_data.len() as u32,
                        smp: bytemuck::pod_collect_to_vec(&sample_data),
                    };
                    let data = VoiceData::Pcm(pcm);
                    let mut voice = Voice {
                        name: ins.get_name().to_string(),
                        ..Default::default()
                    };
                    voice.allocate::<false>();
                    let vu = &mut voice.units[0];
                    vu.data = data;
                    // The way basic key works, the lower the basic key, the higher the pitch
                    // So if we want to increase the pitch by 1 semitone, we need to subtract 256.
                    // So we subtract the fine tune values, rather than adding
                    // Coarse tune (semitone)
                    vu.basic_key -= region.get_coarse_tune() * 256;
                    // Fine tune (cent)
                    vu.basic_key -= (region.get_fine_tune() as f64 * 2.56) as i32;
                    let mut song = song.lock().unwrap();
                    let song = &mut *song;
                    if let Some(target_idx) = sf2.target_voice_idx {
                        song.ins.voices[target_idx.usize()] = voice;
                    } else {
                        song.ins.voices.push(voice);
                    }
                    ptcow::rebuild_tones(
                        &mut song.ins,
                        out_rate,
                        &mut song.herd.delays,
                        &mut song.herd.overdrives,
                        &song.song.master,
                    );
                    close = true;
                }
            });
        }
        if ui.button("Stop").clicked() {
            aux.send.send(AuxMsg::StopAll).unwrap();
        }
        ui.separator();
    }
    ui.add(egui::TextEdit::singleline(&mut sf2.filter_string).hint_text("🔎 Filter"));
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (i, instrument) in sf2.soundfont.get_instruments().iter().enumerate() {
            if !sf2.filter_string.is_empty()
                && !instrument
                    .get_name()
                    .to_ascii_lowercase()
                    .contains(&sf2.filter_string.to_ascii_lowercase())
            {
                continue;
            }
            if ui
                .selectable_label(sf2.selected == Some(i), instrument.get_name())
                .clicked()
            {
                sf2.selected = Some(i);
            }
        }
    });
    close
}

fn group_idx_slider(ui: &mut egui::Ui, group_idx: &mut GroupIdx) -> egui::Response {
    ui.style_mut().spacing.slider_width = 140.0;
    ui.add(egui::Slider::new(&mut group_idx.0, 0..=GroupIdx::MAX.0))
}
