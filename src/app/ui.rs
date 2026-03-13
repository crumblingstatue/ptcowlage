pub mod file_ops;
mod img;
pub mod left_panel;
pub mod modal;
pub mod top_panel;
mod unit;
pub mod windows;

use {
    crate::app::{
        SongState,
        ui::{
            left_panel::LeftPanelState,
            tabs::{
                effects::EffectsUiState,
                events::RawEventsUiState,
                map::MapState,
                piano_roll::PianoRollState,
                voices::{SelectedSlot, VoicesUiState},
            },
            unit::{unit_color, unit_voice_img},
            windows::Windows,
        },
    },
    eframe::egui,
    egui_toast::Toasts,
    ptcow::{Event, EventPayload, GroupIdx, UnitIdx, Voice, VoiceData, VoiceIdx, WaveDataPoints},
    rustc_hash::FxHashSet,
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
    /// Toot duration
    duration: u32,
    play_octave: i32,
    // Live record performance (store toots in events)
    record: bool,
    velocity: i16,
}

impl Default for FreeplayPianoState {
    fn default() -> Self {
        Self {
            // Long enough for default "moo" noise :)
            duration: 1024,
            play_octave: 7,
            record: false,
            velocity: 100,
        }
    }
}

const FACTOR: i32 = 256;
pub const fn piano_key_to_pxtone_key(key: i32) -> i32 {
    FACTOR * key
}

fn piano_freeplay_ui(
    song: &mut SongState,
    ui: &mut egui::Ui,
    state: &mut FreeplayPianoState,
    shared: &mut SharedUiState,
    file_dia_open: bool,
) {
    // Avoid tooting when we're inside a text edit, etc.
    if !ui.ctx().wants_keyboard_input() {
        piano_freeplay_input(song, ui, state, shared, file_dia_open);
    }
    ui.label("🎹").on_hover_text("Piano freeplay UI");
    ui.label("Octave");
    ui.add(
        egui::DragValue::new(&mut state.play_octave)
            .speed(0.05)
            .range(2..=9),
    );
    ui.label("Velocity");
    ui.add(egui::DragValue::new(&mut state.velocity));
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
        egui::lerp(f32::from(a.r())..=f32::from(b.r()), t) as u8,
        egui::lerp(f32::from(a.g())..=f32::from(b.g()), t) as u8,
        egui::lerp(f32::from(a.b())..=f32::from(b.b()), t) as u8,
        egui::lerp(f32::from(a.a())..=f32::from(b.a()), t) as u8,
    )
}

fn piano_freeplay_input(
    song: &mut SongState,
    ui: &mut egui::Ui,
    state: &mut FreeplayPianoState,
    shared: &mut SharedUiState,
    file_dia_open: bool,
) {
    // Play a cow on the keyboard
    // Ignores the keyboard if an egui popup is open or the file dialog is open

    if !egui::Popup::is_any_open(ui.ctx()) && !file_dia_open {
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
                piano_freeplay_play_note(song, state, piano_key, shared.active_unit);
            }
        }
    }
}

fn piano_freeplay_play_note(
    song: &mut SongState,
    state: &mut FreeplayPianoState,
    piano_key: i32,
    unit_no: UnitIdx,
) {
    let tick = ptcow::current_tick(&song.herd, &song.ins);
    // Set key
    let key = piano_key_to_pxtone_key(piano_key);
    // Fall back to the "voice test unit" if the index is out of bounds
    let unit = song
        .herd
        .units
        .get_mut(unit_no)
        .unwrap_or(&mut song.voice_test_unit);
    unit.set_key(key);
    if state.record {
        // If the song is paused, unpause it
        song.pause = false;
        // Push the event
        song.song.events.push(Event {
            payload: EventPayload::Key(key),
            unit: unit_no,
            tick,
        });
    }
    // Set velocity
    unit.velocity = state.velocity;
    if state.record {
        song.song.events.push(Event {
            payload: EventPayload::Velocity(state.velocity),
            unit: unit_no,
            tick,
        });
    }
    // Play the note
    if state.record {
        song.song.events.push(Event {
            payload: EventPayload::On {
                duration: state.duration,
            },
            unit: unit_no,
            tick,
        });
    }
    unit.on(
        unit_no,
        &song.ins,
        &[],
        0,
        state.duration,
        tick,
        song.herd.smp_end,
        std::slice::from_ref(&song.preview_voice),
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
    pub windows: Windows,
    pub left: LeftPanelState,
}

/// Ui state shared among different uis
pub struct SharedUiState {
    /// The active unit is the one that is:
    /// - Used to place notes in the piano roll
    /// - Used for freeplay
    /// - Shows up in the unit UI
    /// - Is highlighted in the left side units panel
    pub active_unit: UnitIdx,
    pub toasts: Toasts,
    /// Units in this set will be highlighted
    pub highlight_set: FxHashSet<UnitIdx>,
}

impl SharedUiState {
    pub const VOICE_TEST_UNIT_IDX: UnitIdx = UnitIdx(255);
}

impl Default for SharedUiState {
    fn default() -> Self {
        Self {
            active_unit: Self::VOICE_TEST_UNIT_IDX,
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
            &mut app.ui_state.shared,
            &mut app.cmd,
            &mut app.modal,
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
                &mut app.ui_state.freeplay_piano,
            );
        }
        Tab::Events => tabs::events::ui(
            ui,
            &mut song,
            &mut app.ui_state.raw_events,
            app.out.rate,
            &mut app.cmd,
            &mut app.modal,
            &mut app.ui_state.shared,
        ),
        Tab::Voices => tabs::voices::ui(
            ui,
            &mut song,
            &mut app.ui_state.voices,
            &mut app.ui_state.shared,
            app.out.rate,
            &mut app.cmd,
            #[cfg(not(target_arch = "wasm32"))]
            &mut app.file_dia,
        ),
        Tab::Unit => tabs::unit::ui(
            ui,
            &mut app.ui_state.shared,
            &mut song,
            &mut app.cmd,
            &mut app.modal,
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
pub fn envelope_edit_widget(
    ui: &mut egui::Ui,
    samples: &mut [u8],
    height: f32,
    id: egui::Id,
    voice_idx: VoiceIdx,
    sel_slot: SelectedSlot,
    units: &ptcow::Units,
) {
    // Unique ID to store previous pointer pos across frames
    let id = ui.id().with(id);

    // Allocate widget space
    let (rect, resp) = ui.allocate_exact_size(
        egui::Vec2::new(ui.available_width(), height),
        egui::Sense::click_and_drag(),
    );

    let hor_ratio = rect.width() / samples.len() as f32;
    let lt = rect.left_top();
    let lb = rect.left_bottom();

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
                samples[xi as usize] = 255 - p.y.clamp(0.0, 255.0) as u8;
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
        .map(|(i, val)| egui::Pos2::new(lt.x + i as f32 * hor_ratio, lb.y - f32::from(*val)))
        .collect();

    painter.line(points, egui::Stroke::new(1.0, egui::Color32::YELLOW));

    // For units that are playing this voice, draw a line for what position they are in the envelope
    for (idx, unit) in units.enumerated() {
        if unit.voice_idx == voice_idx {
            let tone = &unit.tones[sel_slot as usize];
            let pos = tone.env_pos;
            let x = lt.x + pos as f32 * hor_ratio;
            painter.line_segment(
                [egui::pos2(x, lt.y), egui::pos2(x, lb.y)],
                egui::Stroke::new(1.0, unit_color(idx)),
            );
        }
    }
}

/// Draws and edits an interleaved stereo signed 16 bit waveform.
/// `height` is the display height of the waveform rectangle.
pub fn waveform_edit_widget_16_bit_interleaved_stereo(
    ui: &mut egui::Ui,
    samples: &mut [i16],
    height: f32,
    id: egui::Id,
) {
    // Unique ID to store previous pointer pos across frames
    let id = ui.id().with(id);

    // Allocate widget space
    let (rect, resp) = ui.allocate_exact_size(
        egui::Vec2::new(ui.available_width(), height),
        egui::Sense::click_and_drag(),
    );

    let hor_ratio = rect.width() / (samples.len() / 2) as f32;
    let lc = rect.left_center();

    // Load previous pointer pos from temp storage
    let prev_pointer_pos: Option<egui::Pos2> = ui.data(|d| d.get_temp(id));

    // Get latest pointer pos (egui 0.33)
    let (pointer_pos, lmb, rmb) = ui.input(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.primary_down(),
            i.pointer.secondary_down(),
        )
    });

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
        let a = egui::Pos2::new((prev.x - lc.x) / (hor_ratio / 2.0), lc.y - prev.y);
        let b = egui::Pos2::new((pos.x - lc.x) / (hor_ratio / 2.0), lc.y - pos.y);

        for p in line_points_between(a, b) {
            let xi = p.x.round() as isize;
            if xi >= 0 {
                let val = (p.y * 256.0) as i16;
                if lmb {
                    let idx = if xi % 2 == 0 { xi } else { xi + 1 };
                    if let Some(samp) = samples.get_mut(idx as usize) {
                        *samp = val;
                    }
                }
                if rmb {
                    let idx = if xi % 2 != 0 { xi } else { xi + 1 };
                    if let Some(samp) = samples.get_mut(idx as usize) {
                        *samp = val;
                    }
                }
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
    let (points_l, points_r): (Vec<egui::Pos2>, Vec<egui::Pos2>) = samples
        .as_chunks::<2>()
        .0
        .iter()
        .enumerate()
        .map(|(i, [l, r])| {
            (
                egui::Pos2::new(lc.x + i as f32 * hor_ratio, lc.y - f32::from(*l) / 256.),
                egui::Pos2::new(lc.x + i as f32 * hor_ratio, lc.y - f32::from(*r) / 256.),
            )
        })
        .collect();

    painter.line(points_l, egui::Stroke::new(1.0, egui::Color32::RED));
    painter.line(points_r, egui::Stroke::new(1.0, egui::Color32::BLUE));
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
    voice_data_img(&voice.base.data)
}

fn voice_data_img(data: &VoiceData) -> egui::ImageSource<'static> {
    match data {
        ptcow::VoiceData::Noise(_) => img::DRUM,
        ptcow::VoiceData::Pcm(_) => img::MIC,
        ptcow::VoiceData::Wave(data) => match data.points {
            WaveDataPoints::Coord { .. } => img::SAXO,
            WaveDataPoints::Overtone { .. } => img::ACCORDION,
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

fn group_idx_slider(ui: &mut egui::Ui, group_idx: &mut GroupIdx) -> egui::Response {
    ui.style_mut().spacing.slider_width = 140.0;
    ui.add(egui::Slider::new(&mut group_idx.0, 0..=GroupIdx::MAX.0))
}
