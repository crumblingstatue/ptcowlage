pub mod file_ops;
mod img;
pub mod left_panel;
pub mod top_panel;

use {
    crate::{
        app::{
            SongState,
            command_queue::{Cmd, CommandQueue},
            ui::{
                left_panel::LeftPanelState,
                tabs::{
                    events::RawEventsUiState,
                    map::MapState,
                    piano_roll::PianoRollState,
                    playback::PlaybackUiState,
                    voices::{VoicesUiState, voice_ui_inner},
                },
            },
        },
        audio_out::AuxAudioState,
    },
    eframe::egui::{self, AtomExt},
    ptcow::{Event, EventPayload, GroupIdx, MooInstructions, SampleRate, Unit, UnitIdx, Voice},
};

mod tabs {
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
            duration: 512,
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
    pub playback: PlaybackUiState,
    pub freeplay_piano: FreeplayPianoState,
    pub raw_events: RawEventsUiState,
    pub voices: VoicesUiState,
    pub left_panel: LeftPanelState,
    pub shared: SharedUiState,
}

/// Ui state shared among different uis
#[derive(Default)]
pub struct SharedUiState {
    /// The active unit is the one that is:
    /// - Used to place notes in the piano roll
    /// - Shows up in the unit UI
    /// - Is highlighted in the left side units panel
    pub active_unit: Option<UnitIdx>,
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
            let should_pause = !egui::Popup::is_any_open(ui.ctx())
                && !file_dia_open
                && !ui.ctx().wants_keyboard_input();

            if should_pause {
                song.pause ^= true;
            }
        }
    }

    match app.ui_state.tab {
        Tab::Playback => tabs::playback::ui(
            ui,
            &mut song,
            &mut app.ui_state.playback,
            &mut app.ui_state.freeplay_piano,
            app.out.rate,
            &mut app.aux_state,
            &mut app.ui_state.voices,
            &mut app.cmd,
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
            &mut app.aux_state,
            &mut app.ui_state.voices,
            &mut app.cmd,
        ),
        Tab::Voices => tabs::voices::ui(
            ui,
            &mut song,
            #[cfg(not(target_arch = "wasm32"))]
            &mut app.file_dia,
            &mut app.ui_state.voices,
            app.out.rate,
            &mut app.aux_state,
        ),
        Tab::Unit => tabs::unit::ui(ui, &mut app.ui_state.shared, &mut song, &mut app.cmd),
        Tab::Effects => tabs::effects::ui(ui, &mut song, app.out.rate),
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

fn handle_units_command(cmd: Option<UnitsCmd>, song: &mut SongState) {
    if let Some(cmd) = cmd {
        match cmd {
            UnitsCmd::ToggleSolo { idx } => {
                let me_playing = !song.herd.units[idx.usize()].mute;
                let mut any_playing = false;
                for (i, unit) in song.herd.units.iter_mut().enumerate() {
                    if i == idx.usize() {
                        continue;
                    }
                    if !unit.mute {
                        any_playing = true;
                    }
                    unit.mute = true;
                }
                let already_solo = me_playing && !any_playing;
                // Unmute me always for solo
                song.herd.units[idx.usize()].mute = false;
                // Unmute all units if we were already solo
                if already_solo {
                    for unit in &mut song.herd.units {
                        unit.mute = false;
                    }
                }
            }
            UnitsCmd::SeekFirstOnEvent { idx } => {
                if let Some(ev) = song
                    .song
                    .events
                    .eves
                    .iter()
                    .find(|ev| ev.unit == idx && matches!(&ev.payload, EventPayload::On { .. }))
                {
                    song.herd.seek_to_sample(ptcow::timing::tick_to_sample(
                        ev.tick,
                        song.ins.samples_per_tick,
                    ));
                }
            }
            UnitsCmd::SeekNextOnEvent { idx } => {
                if let Some(ev) = song.song.events[song.herd.evt_idx..]
                    .iter()
                    .find(|ev| ev.unit == idx && matches!(&ev.payload, EventPayload::On { .. }))
                {
                    song.herd.seek_to_sample(ptcow::timing::tick_to_sample(
                        ev.tick,
                        song.ins.samples_per_tick,
                    ));
                }
            }
            UnitsCmd::DeleteUnit { idx } => {
                song.song.events.retain_mut(|eve| {
                    let retain = eve.unit != idx;
                    // If we removed a unit below this unit index, then we need to decrement it
                    if eve.unit.0 > idx.0 {
                        eve.unit.0 -= 1;
                    }
                    retain
                });
                song.herd.units.remove(idx.usize());
            }
        }
    }
}

enum UnitsCmd {
    ToggleSolo { idx: UnitIdx },
    SeekFirstOnEvent { idx: UnitIdx },
    SeekNextOnEvent { idx: UnitIdx },
    DeleteUnit { idx: UnitIdx },
}

fn unit_ui(
    ui: &mut egui::Ui,
    i: UnitIdx,
    unit: &mut Unit,
    ins: &MooInstructions,
    cmd: &mut Option<UnitsCmd>,
    app_cmd: &mut CommandQueue,
) {
    ui.horizontal(|ui| {
        ui.heading(format!("Unit {} {:?}", i.0, unit.name));
        ui.text_edit_singleline(&mut unit.name);
        if ui
            .button(
                egui::RichText::new("Delete unit")
                    .background_color(egui::Color32::DARK_RED)
                    .color(egui::Color32::WHITE),
            )
            .clicked()
        {
            *cmd = Some(UnitsCmd::DeleteUnit { idx: i });
        }
    });
    ui.indent("unit", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("key now");
            ui.add(egui::DragValue::new(&mut unit.key_now));
            ui.label("key start");
            ui.add(egui::DragValue::new(&mut unit.key_start));
            ui.label("key margin");
            ui.add(egui::DragValue::new(&mut unit.key_margin));
            ui.end_row();
            ui.label("portament sample pos");
            ui.add(egui::DragValue::new(&mut unit.porta_pos));
            ui.label("portament sample num");
            ui.add(egui::DragValue::new(&mut unit.porta_destination));
            ui.end_row();
            ui.label("Pan vol l");
            ui.add(egui::DragValue::new(&mut unit.pan_vols[0]));
            ui.label("r");
            ui.add(egui::DragValue::new(&mut unit.pan_vols[1]));
            ui.label("Pan time l");
            ui.add(egui::DragValue::new(&mut unit.pan_time_offs[0]));
            ui.label("r");
            ui.add(egui::DragValue::new(&mut unit.pan_time_offs[1]));
            ui.end_row();
            ui.label("volume");
            ui.add(egui::DragValue::new(&mut unit.volume));
            ui.label("velocity");
            ui.add(egui::DragValue::new(&mut unit.velocity));
            ui.end_row();
            ui.label("group");
            group_idx_slider(ui, &mut unit.group);
            ui.end_row();
            ui.label("tuning");
            ui.add(egui::DragValue::new(&mut unit.tuning).speed(0.001));
            ui.end_row();
            ui.label("voice index");
            ui.add(egui::Slider::new(
                &mut unit.voice_idx.0,
                0..=ins.voices.len().saturating_sub(1).try_into().unwrap(),
            ));
            if let Some(voice) = ins.voices.get(unit.voice_idx.usize()) {
                ui.image(voice_img(voice));
                if ui.link(&voice.name).clicked() {
                    app_cmd.push(Cmd::OpenVoice(unit.voice_idx));
                }
            } else {
                ui.label("<invalid voice>");
            }
            ui.end_row();
            ui.heading("Tones");
            ui.end_row();
            for tone in &mut unit.tones {
                ui.label("Tone");
                ui.end_row();
                ui.label("smp_pos");
                ui.add(egui::DragValue::new(&mut tone.smp_pos));
                ui.label("offset_freq");
                ui.add(egui::DragValue::new(&mut tone.offset_freq).speed(0.001));
                ui.label("env_volume");
                ui.add(egui::DragValue::new(&mut tone.env_volume));
                ui.label("life_count");
                ui.add(egui::DragValue::new(&mut tone.life_count));
                ui.label("on_count");
                ui.add(egui::DragValue::new(&mut tone.on_count));
                ui.label("env_start");
                ui.add(egui::DragValue::new(&mut tone.env_start));
                ui.label("env_pos");
                ui.add(egui::DragValue::new(&mut tone.env_pos));
                ui.label("env_release_clock");
                ui.add(egui::DragValue::new(&mut tone.env_release_clock));
                ui.end_row();
            }
        });
    });
}

fn group_idx_slider(ui: &mut egui::Ui, group_idx: &mut GroupIdx) {
    ui.add(egui::Slider::new(&mut group_idx.0, 0..=GroupIdx::MAX.0));
}

#[derive(PartialEq)]
enum UnitPopupTab {
    Unit,
    Voice,
}

fn unit_popup_ctx_menu(
    re: &egui::Response,
    idx: UnitIdx,
    unit: &mut Unit,
    ins: &mut MooInstructions,
    cmd: &mut Option<UnitsCmd>,
    tab: &mut UnitPopupTab,
    out_rate: SampleRate,
    aux: &mut Option<AuxAudioState>,
    voices_ui_state: &mut VoicesUiState,
    app_cmd: &mut CommandQueue,
) {
    egui::Popup::context_menu(re)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            unit_popup_ui(
                ui,
                idx,
                unit,
                ins,
                cmd,
                tab,
                out_rate,
                aux,
                voices_ui_state,
                app_cmd,
            )
        });
}

fn unit_popup_ui(
    ui: &mut egui::Ui,
    idx: UnitIdx,
    unit: &mut Unit,
    ins: &mut MooInstructions,
    cmd: &mut Option<UnitsCmd>,
    tab: &mut UnitPopupTab,
    out_rate: SampleRate,
    aux: &mut Option<AuxAudioState>,
    voices_ui_state: &mut VoicesUiState,
    app_cmd: &mut CommandQueue,
) {
    ui.horizontal(|ui| {
        if ui.button("ｘ").clicked() {
            ui.close();
        }
        ui.separator();
        ui.checkbox(&mut unit.mute, "Mute");
        if ui.button("Solo").clicked() {
            *cmd = Some(UnitsCmd::ToggleSolo { idx });
        }
        ui.separator();
        ui.selectable_value(tab, UnitPopupTab::Unit, "Unit");
        let voice_label = format!(
            "Voice ({})",
            ins.voices
                .get(unit.voice_idx.usize())
                .map_or("<invalid>", |v| &v.name)
        );
        ui.selectable_value(tab, UnitPopupTab::Voice, voice_label);
        ui.separator();
        ui.menu_button("Seek", |ui| {
            if ui.button("First On event").clicked() {
                *cmd = Some(UnitsCmd::SeekFirstOnEvent { idx });
            }
            if ui.button("Next On event").clicked() {
                *cmd = Some(UnitsCmd::SeekNextOnEvent { idx });
            }
        });
    });
    ui.separator();
    match tab {
        UnitPopupTab::Unit => unit_ui(ui, idx, unit, ins, cmd, app_cmd),
        UnitPopupTab::Voice => {
            if let Some(voice) = ins.voices.get_mut(unit.voice_idx.usize()) {
                voice_ui_inner(ui, voice, unit.voice_idx, out_rate, aux, voices_ui_state);
            } else {
                ui.label("Invalid voice index");
            }
        }
    }
}

const UNIT_COLORS: [egui::Color32; 22] = [
    egui::Color32::RED,
    egui::Color32::GREEN,
    egui::Color32::BLUE,
    egui::Color32::YELLOW,
    egui::Color32::PURPLE,
    egui::Color32::ORANGE,
    egui::Color32::MAGENTA,
    egui::Color32::CYAN,
    egui::Color32::LIGHT_BLUE,
    egui::Color32::LIGHT_GRAY,
    egui::Color32::LIGHT_GREEN,
    egui::Color32::LIGHT_RED,
    egui::Color32::LIGHT_YELLOW,
    egui::Color32::DARK_BLUE,
    egui::Color32::DARK_GREEN,
    egui::Color32::DARK_RED,
    egui::Color32::DARK_GRAY,
    egui::Color32::BROWN,
    egui::Color32::GOLD,
    egui::Color32::KHAKI,
    egui::Color32::WHITE,
    egui::Color32::from_rgb(40, 40, 80),
];

fn unit_color(idx: usize) -> egui::Color32 {
    UNIT_COLORS[idx % UNIT_COLORS.len()]
}

fn unit_voice_img(ins: &ptcow::MooInstructions, unit: &ptcow::Unit) -> egui::ImageSource<'static> {
    ins.voices
        .get(unit.voice_idx.usize())
        .map_or(img::X, |voic| voice_img(voic))
}

fn voice_img(voice: &Voice) -> egui::ImageSource<'static> {
    match &voice.units[0].data {
        ptcow::VoiceData::Noise(_) => img::DRUM,
        ptcow::VoiceData::Pcm(_) => img::MIC,
        ptcow::VoiceData::Wave(_) => img::SAXO,
        ptcow::VoiceData::OggV(_) => img::FISH,
    }
}

fn unit_mute_unmute_all_ui(ui: &mut egui::Ui, units: &mut [ptcow::Unit]) {
    ui.horizontal(|ui| {
        if ui.button("Mute all").clicked() {
            for unit in &mut *units {
                unit.mute = true;
            }
        }
        if ui.button("Unmute all").clicked() {
            for unit in &mut *units {
                unit.mute = false;
            }
        }
    });
}
