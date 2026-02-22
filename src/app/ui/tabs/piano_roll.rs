use {
    crate::{
        app::{
            command_queue::{Cmd, CommandQueue},
            ui::{
                FreeplayPianoState, SharedUiState, piano_freeplay_play_note,
                tabs::events::invert_color, unit_color,
            },
        },
        audio_out::SongState,
        herd_ext::HerdExt,
    },
    arrayvec::ArrayVec,
    eframe::egui::{self, PopupAnchor, scroll_area::ScrollBarVisibility},
    ptcow::{
        EventPayload, Key, Meas, SampleRate, SampleT, Tick, Timing, Unit, UnitIdx,
        timing::{NonZeroMeas, tick_to_meas},
    },
    rustc_hash::FxHashSet,
    std::collections::BTreeSet,
};

pub struct PianoRollState {
    tick_div: f32,
    n_rows: u8,
    row_size: f32,
    lowest_semitone: u8,
    follow_playhead: bool,
    shift_all_offset: i32,
    prev_frame_piano_roll_y_offset: f32,
    interact_mode: InteractMode,
    draw_debug_info: bool,
    // TODO: Implement Hash for `UnitIdx`
    pub hidden_units: FxHashSet<u8>,
    draw_meas_lines: bool,
    ui_cmd: Option<UiCmd>,
    /// Which events to show info/edit popup for, if any
    evs_popup: Option<EventsPopup>,
    /// Origin for things like selection boxes
    lmb_drag_origin: Option<egui::Pos2>,
    /// Using a `BTreeSet`, so indices are sorted
    selected_event_indices: BTreeSet<usize>,
    /// Information about note "just" placed with lmb press (haven't released lmb yet)
    just_placed_note: Option<PlacedNote>,
    /// Snap placed notes to quarter beat granularity
    snap_to_quarter_beat: bool,
}

struct PlacedNote {
    tick: Tick,
    unit: UnitIdx,
    key: Key,
}

struct EventsPopup {
    title: &'static str,
    indices: Vec<usize>,
    pos: egui::Pos2,
}

enum UiCmd {
    ScrollToPlayhead,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum InteractMode {
    View,
    Edit,
    Place,
}

impl Default for PianoRollState {
    fn default() -> Self {
        Self {
            tick_div: 10.0,
            n_rows: 96,
            row_size: 12.0,
            lowest_semitone: 42,
            follow_playhead: true,
            shift_all_offset: 0,
            prev_frame_piano_roll_y_offset: 0.,
            interact_mode: InteractMode::View,
            draw_debug_info: false,
            hidden_units: FxHashSet::default(),
            draw_meas_lines: true,
            ui_cmd: None,
            evs_popup: None,
            lmb_drag_origin: None,
            selected_event_indices: BTreeSet::default(),
            just_placed_note: None,
            snap_to_quarter_beat: true,
        }
    }
}

fn top_ui(
    ui: &mut egui::Ui,
    song: &mut SongState,
    state: &mut PianoRollState,
    shared: &mut SharedUiState,
) {
    ui.horizontal(|ui| {
        let [key_f1, key_f2, key_f3] = ui.input(|inp| {
            [
                inp.key_pressed(egui::Key::F1),
                inp.key_pressed(egui::Key::F2),
                inp.key_pressed(egui::Key::F3),
            ]
        });
        ui.selectable_value(&mut state.interact_mode, InteractMode::View, "View [F1]");
        if key_f1 {
            state.interact_mode = InteractMode::View;
        }
        ui.selectable_value(&mut state.interact_mode, InteractMode::Edit, "Edit [F2]");
        if key_f2 {
            state.interact_mode = InteractMode::Edit;
        }
        if ui
            .selectable_value(&mut state.interact_mode, InteractMode::Place, "Place [F3]")
            .clicked()
            || key_f3
        {
            state.interact_mode = InteractMode::Place;
            if shared.active_unit.is_none() && !song.herd.units.is_empty() {
                shared.active_unit = Some(UnitIdx(0));
            }
        }
        let re = ui
            .add(egui::Label::new("▶ Follow").sense(egui::Sense::click()))
            .on_hover_text("Follow the playhead\n(You can also hold lmb on this label)");
        if re.is_pointer_button_down_on() {
            state.ui_cmd = Some(UiCmd::ScrollToPlayhead);
        }
        ui.checkbox(&mut state.follow_playhead, "");
        piano_roll_config_popup_button(ui, state);
        loop_points_popup_button(ui, song);
        experimental_popup_button(ui, song, state);
        if !state.selected_event_indices.is_empty() {
            ui.separator();
            if ui
                .link(format!(
                    "{} selected events",
                    state.selected_event_indices.len()
                ))
                .clicked()
            {
                state.evs_popup = Some(EventsPopup {
                    title: "Selected events",
                    pos: egui::pos2(0., 0.),
                    indices: state.selected_event_indices.iter().copied().collect(),
                });
            }
            if ui.button("Clear").clicked() || ui.input(|inp| inp.key_pressed(egui::Key::Escape)) {
                state.selected_event_indices.clear();
            }
        }
        ui.separator();
        help_popup_button(ui, state.interact_mode);
    });
    ui.separator();
}

pub fn ui(
    ui: &mut egui::Ui,
    song: &mut SongState,
    state: &mut PianoRollState,
    shared: &mut SharedUiState,
    cmd: &mut CommandQueue,
    dst_sps: SampleRate,
    piano_state: &mut FreeplayPianoState,
) {
    top_ui(ui, song, state, shared);
    ui.horizontal_top(|ui| {
        ui.style_mut().spacing.item_spacing = egui::Vec2::ZERO;
        piano_ui(
            song,
            state,
            ui,
            state.prev_frame_piano_roll_y_offset,
            piano_state,
            dst_sps,
        );
        roll_ui(song, state, shared, ui, cmd, dst_sps, piano_state);
    });
}

fn roll_ui(
    song: &mut SongState,
    state: &mut PianoRollState,
    shared: &mut SharedUiState,
    ui: &mut egui::Ui,
    cmd: &mut CommandQueue,
    dst_sps: SampleRate,
    piano_state: &mut FreeplayPianoState,
) {
    // We make the scroll bars be outside of the ScrollArea to resolve a conundrum of
    // `Response::contains_pointer` being true when dragging the scroll bars, and
    // `Response::hovered` being false on click when the ScrollArea is scrollable with
    // the mouse.
    ui.style_mut().spacing.scroll.floating = false;
    // We don't want dragging to scroll when editing
    let scroll_source = if state.interact_mode == InteractMode::View {
        egui::scroll_area::ScrollSource::ALL
    } else {
        egui::scroll_area::ScrollSource {
            scroll_bar: true,
            drag: false,
            mouse_wheel: true,
        }
    };
    let out = egui::ScrollArea::both()
        .scroll_source(scroll_source)
        .show(ui, |ui| {
            roll_ui_inner(song, state, shared, ui, cmd, dst_sps, piano_state);
        });
    state.prev_frame_piano_roll_y_offset = out.state.offset.y;
}

/// The ui inside the scroll area
#[expect(clippy::cast_precision_loss)]
fn roll_ui_inner(
    song: &mut SongState,
    state: &mut PianoRollState,
    shared: &mut SharedUiState,
    ui: &mut egui::Ui,
    cmd: &mut CommandQueue,
    dst_sps: SampleRate,
    piano_state: &mut FreeplayPianoState,
) {
    // We make up a value for number of ticks if there are no events in the song (empty song)
    // TODO: Maybe we can do something more clever here
    let last_tick = song.song.events.last().map_or(5000, |ev| ev.tick);
    let mut approx_end = last_tick as f32 / state.tick_div;
    // Safety for ui alloc
    if !approx_end.is_finite() {
        approx_end = 1.0;
    }
    // Leave some "breathing room" at end, so new events can be placed, etc.
    approx_end += 200.0;
    let clock = ptcow::current_tick(&song.herd, &song.ins);
    let [mod_shift, mod_alt] = ui.input(|inp| [inp.modifiers.shift, inp.modifiers.alt]);
    let (rect, re) = ui.allocate_exact_size(
        egui::vec2(approx_end, state.n_rows as f32 * state.row_size),
        egui::Sense::click(),
    );
    let pnt = ui.painter_at(rect);
    // BG fill
    pnt.rect_filled(rect, 2.0, egui::Color32::from_rgb(30, 30, 24));
    let cr = ui.clip_rect();
    // Draw guide lines
    let (mouse_screen_pos, lmb_clicked, lmb_pressed, lmb_released) = ui.input(|inp| {
        (
            inp.pointer.latest_pos(),
            inp.pointer.primary_clicked(),
            inp.pointer.primary_pressed(),
            inp.pointer.primary_released(),
        )
    });
    if lmb_pressed && re.hovered() && state.interact_mode != InteractMode::View {
        state.lmb_drag_origin = mouse_screen_pos;
        // Shift to expand selection rather than replace
        if !mod_shift {
            state.selected_event_indices.clear();
        }
    }
    if lmb_released {
        state.lmb_drag_origin = None;
    }
    for key in 0..state.n_rows {
        let info = key_info(state.lowest_semitone, key);
        let y = key as f32 * state.row_size;
        let y = rect.max.y - y;
        let sharp = [
            false, true, false, true, false, false, true, false, true, false, true, false,
        ];
        let row_rect = egui::Rect::from_min_max(
            egui::pos2(cr.min.x, y),
            egui::pos2(cr.max.x, y + state.row_size),
        );
        if sharp[info.c_scale_idx as usize] {
            pnt.rect_filled(row_rect, 0.0, egui::Color32::BLACK);
        }
        pnt.line_segment(
            [egui::pos2(cr.min.x, y), egui::pos2(cr.max.x, y)],
            egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
        );
        // Draw a highlight for the row mouse is on
        if let Some(mp) = mouse_screen_pos
            && row_rect.y_range().contains(mp.y)
        {
            pnt.rect_stroke(
                row_rect,
                0.0,
                egui::Stroke::new(1.0, egui::Color32::YELLOW),
                egui::StrokeKind::Inside,
            );
        }
    }
    // Lmb Drag selection box
    let mut sel_rect = None;
    if state.interact_mode == InteractMode::Edit
        && let Some(mp) = mouse_screen_pos
        && let Some(drag_origin) = state.lmb_drag_origin
    {
        let rect = egui::Rect::from_two_pos(drag_origin, mp);
        if rect.width() > 10. && rect.height() > 10. {
            sel_rect = Some(rect);
        }
    }
    if let Some(sel_rect) = sel_rect {
        pnt.debug_rect(sel_rect, egui::Color32::YELLOW, "Select");
    }
    // Draw the piano roll items based on events
    let default_y = key_y(
        state.lowest_semitone,
        state.row_size,
        rect,
        ptcow::DEFAULT_KEY,
    );
    // INVARIANT/TODO: This assumes there are enough units in the herd so no event refers to an
    // out of bounds index. Might not always hold true. Especially if deleting units is allowed.
    let mut unit_key_ys = vec![default_y; song.herd.units.len()];
    let [mut rects_drawn, mut circles_drawn, mut lines_drawn] = [0; _];
    let mut hovered_events = Vec::new();
    for (ev_idx, ev) in song.song.events.iter().enumerate() {
        if state.hidden_units.contains(&ev.unit.0) {
            continue;
        }
        let clock_approx = ev.tick as f32 / state.tick_div;
        let x = clock_approx + rect.min.x;
        // We take advantage of the fact that events are sorted by ticks, and
        // if the x is larger than the UI clip rect, we break, to save on rendering
        // a bunch of stuff
        if x > cr.max.x {
            break;
        }
        let clr = unit_color(ev.unit.usize());
        let mut interact_rect = None;
        match ev.payload {
            EventPayload::On { duration } => {
                // Sometimes, key events are after on in the event buffer,
                // but at the same tick. Look ahead to find such events, and correct
                // the y position
                for eve_ahead in &song.song.events[ev_idx..] {
                    if eve_ahead.tick != ev.tick {
                        break;
                    }
                    if eve_ahead.unit != ev.unit {
                        continue;
                    }
                    let EventPayload::Key(key) = eve_ahead.payload else {
                        continue;
                    };
                    unit_key_ys[ev.unit.usize()] =
                        key_y(state.lowest_semitone, state.row_size, rect, key);
                }
                let y = unit_key_ys[ev.unit.usize()];
                let rect = egui::Rect::from_min_max(
                    egui::pos2(x, y),
                    egui::pos2(x + duration as f32 / state.tick_div, y + state.row_size),
                );
                // We skip drawing the rect if it's outside to the left of the clip rect
                if rect.max.x < cr.min.x {
                    continue;
                }
                interact_rect = Some(rect);
                // We slightly shrink the drawn rect so it doesn't overlap guide lines or row
                // highlight
                let shrink_rect = rect.shrink2(egui::vec2(0.0, 2.0));
                pnt.rect_filled(shrink_rect, 2.0, clr);
                if state.selected_event_indices.contains(&ev_idx) {
                    pnt.rect_stroke(
                        shrink_rect,
                        2.0,
                        egui::Stroke::new(1.0, invert_color(clr)),
                        egui::StrokeKind::Outside,
                    );
                }
                rects_drawn += 1;
            }
            EventPayload::Key(k) => {
                let y = key_y(state.lowest_semitone, state.row_size, rect, k);
                unit_key_ys[ev.unit.usize()] = y;
                let radius = state.row_size / 4.0;
                // We skip drawing the circle if it's outside to the left of the clip rect
                if x + radius < cr.min.x {
                    continue;
                }
                interact_rect = Some(egui::Rect::from_center_size(
                    egui::pos2(x, y),
                    egui::vec2(radius * 2.0, radius * 2.0),
                ));
                pnt.circle(
                    egui::pos2(x + radius, y + radius),
                    radius,
                    clr,
                    egui::Stroke::new(1.0, invert_color(clr)),
                );
                circles_drawn += 1;
            }
            _ => {}
        }
        // The interact rectangles are at the pos where they are drawn on the screen,
        // so no mouse coordinate conversion is required.
        if let Some(irect) = interact_rect
            && let Some(mp) = mouse_screen_pos
        {
            // We want to make sure mouse pos is also in clip rect so clicks outside the scroll area
            // aren't registered
            if cr.contains(mp) && irect.contains(mp) {
                if ui.input(|inp| inp.pointer.secondary_clicked()) {
                    if state.interact_mode == InteractMode::Place
                        || state.interact_mode == InteractMode::Edit
                    {
                        cmd.push(Cmd::RemoveNoteAtIdx { idx: ev_idx });
                    }
                }
                hovered_events.push(ev_idx);
            }
        }
        // Add to selection if selection box is active
        if let Some(irect) = interact_rect
            && let Some(sel_rect) = sel_rect
        {
            if sel_rect.contains_rect(irect) {
                state.selected_event_indices.insert(ev_idx);
            }
        }
    }

    // Left click popup
    if let Some(mp) = mouse_screen_pos
        && state.interact_mode == InteractMode::View
        && re.hovered()
        && lmb_clicked
    {
        if !hovered_events.is_empty() {
            state.evs_popup = Some(EventsPopup {
                title: "Clicked events",
                indices: hovered_events.clone(),
                pos: mp,
            });
        }
    }

    // Alt hover tooltip
    if mod_alt && !hovered_events.is_empty() {
        egui::Tooltip::always_open(
            ui.ctx().clone(),
            ui.layer_id(),
            egui::Id::NULL,
            PopupAnchor::Pointer,
        )
        .gap(16.0)
        .show(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            for ev_idx in hovered_events {
                let ev = &song.song.events[ev_idx];
                let payload_text = match ev.payload {
                    EventPayload::On { duration } => {
                        format!("On event for {duration} ticks")
                    }
                    EventPayload::Key(key) => {
                        format!(
                            "Key: {key}\n{:#?}",
                            key_info(state.lowest_semitone, (key / 256) as u8)
                        )
                    }
                    _ => {
                        continue;
                    }
                };
                let unit_name = match song.herd.units.get(ev.unit.usize()) {
                    Some(unit) => unit.name.clone(),
                    None => format!("No such unit: {}", ev.unit.0),
                };
                ui.horizontal(|ui| {
                    ui.label(ev.tick.to_string());
                    ui.colored_label(unit_color(ev.unit.usize()), unit_name);
                    ui.label(payload_text);
                });
            }
        });
    }
    // Events left click window
    if let Some(popup) = &state.evs_popup {
        let mut open = true;
        egui::Window::new(popup.title)
            .default_pos(popup.pos)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                if ui.button("Up 1 key").clicked() {
                    for &idx in &popup.indices {
                        let ev = &mut song.song.events[idx];
                        if let EventPayload::Key(key) = &mut ev.payload {
                            *key += 256;
                        }
                    }
                }
                if ui.button("Down 1 key").clicked() {
                    for &idx in &popup.indices {
                        let ev = &mut song.song.events[idx];
                        if let EventPayload::Key(key) = &mut ev.payload {
                            *key -= 256;
                        }
                    }
                }
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("events_win_events")
                        .striped(true)
                        .show(ui, |ui| {
                            events_window_inner_ui(song, cmd, ui, popup);
                        });
                });
            });
        if !open {
            state.evs_popup = None;
        }
    }

    if state.draw_meas_lines {
        draw_meas_lines(
            song,
            state,
            last_tick,
            rect,
            &pnt,
            cr,
            &mut lines_drawn,
            mouse_screen_pos,
        );
    }

    if state.draw_debug_info {
        if let Some(mp) = mouse_screen_pos
            && ui.clip_rect().contains(mp)
        {
            let (tick, meas) = mouse_tick_meas(mp, rect, state.tick_div, song.song.master.timing);
            let pnt = ui.ctx().debug_painter();
            pnt.debug_text(
                mp + egui::vec2(0.0, -42.0),
                egui::Align2::LEFT_TOP,
                egui::Color32::WHITE,
                format!("tick: {tick}"),
            );
            pnt.debug_text(
                mp + egui::vec2(0.0, -24.0),
                egui::Align2::LEFT_TOP,
                egui::Color32::WHITE,
                format!("meas: {meas}"),
            );
            pnt.debug_text(
                mp + egui::vec2(0.0, -8.0),
                egui::Align2::LEFT_TOP,
                egui::Color32::WHITE,
                format!("drawn {circles_drawn} circles, {rects_drawn} rects, {lines_drawn} lines"),
            );
        }
    }

    // Draw play head line

    // Approximate clock to lower resolution
    let clock_approx = clock as f32 / state.tick_div;
    let playhead_x = clock_approx + rect.min.x;
    if state.follow_playhead && !song.pause {
        scroll_to_playhead(ui, cr, playhead_x);
    }
    pnt.line_segment(
        [
            egui::pos2(playhead_x, rect.min.y),
            egui::pos2(playhead_x, rect.max.y),
        ],
        egui::Stroke::new(2.0, egui::Color32::WHITE),
    );
    // Draw play repeat line
    let x = (song.herd.smp_repeat as f32 / song.ins.samples_per_tick / state.tick_div) + rect.min.x;
    pnt.line_segment(
        [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
        egui::Stroke::new(2.0, egui::Color32::GREEN),
    );
    // Draw play end line
    let x = (song.herd.smp_end as f32 / song.ins.samples_per_tick / state.tick_div) + rect.min.x;
    pnt.line_segment(
        [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
        egui::Stroke::new(2.0, egui::Color32::RED),
    );
    if let Some(pos) = mouse_screen_pos {
        let mx = pos.x - rect.min.x;
        let scaled = mx * state.tick_div;
        let sample = scaled * song.ins.samples_per_tick;
        let sample = sample as SampleT;
        if lmb_pressed && re.contains_pointer() {
            match state.interact_mode {
                InteractMode::View | InteractMode::Edit => {
                    if mod_shift {
                        song.herd.seek_to_sample(sample);
                    }
                }
                InteractMode::Place => 'block: {
                    if mod_shift {
                        song.herd.seek_to_sample(sample);
                        break 'block;
                    }
                    // Place note
                    let Some(unit) = shared.active_unit else {
                        return;
                    };
                    let Some(piano_key) = piano_key_from_y(pos, &re, rect, state) else {
                        return;
                    };
                    let key = piano_key * 256;
                    let ticks_per_q_beat = song.song.master.timing.ticks_per_beat as u32 / 4;
                    let tick = if state.snap_to_quarter_beat {
                        (scaled as u32 / ticks_per_q_beat) * ticks_per_q_beat
                    } else {
                        scaled as u32
                    };
                    state.just_placed_note = Some(PlacedNote { tick, unit, key });
                    piano_freeplay_play_note(song, dst_sps, piano_state, piano_key, unit);
                }
            }
        }
        if let Some(placed) = &state.just_placed_note {
            'block: {
                let ticks_per_q_beat = song.song.master.timing.ticks_per_beat as u32 / 4;
                let end_tick = if state.snap_to_quarter_beat {
                    ((scaled as u32).div_ceil(ticks_per_q_beat)) * ticks_per_q_beat
                } else {
                    scaled as u32
                };
                let Some(duration) = end_tick.checked_sub(placed.tick) else {
                    break 'block;
                };
                if duration == 0 {
                    break 'block;
                }
                // Draw "just placed" note
                let orig_x = placed.tick as f32 / state.tick_div;
                let dura_w = duration as f32 / state.tick_div;
                let x = orig_x + rect.min.x;
                let y = key_y(state.lowest_semitone, state.row_size, rect, placed.key);
                let rect = egui::Rect::from_min_max(
                    egui::pos2(x, y),
                    egui::pos2(x + dura_w, y + state.row_size),
                );
                pnt.debug_rect(rect, unit_color(placed.unit.usize()), "Just placed");
                // "Finalize" note when lmb is released
                if lmb_released {
                    song.song.events.push(ptcow::Event {
                        payload: EventPayload::Key(placed.key),
                        unit: placed.unit,
                        tick: placed.tick,
                    });
                    song.song.events.push(ptcow::Event {
                        payload: EventPayload::On { duration },
                        unit: placed.unit,
                        tick: placed.tick,
                    });
                    song.song.events.sort();
                    song.song.recalculate_length();
                    state.just_placed_note = None;
                }
            }
        }
        // Ensure end can't be smaller than repeat
        if song.herd.smp_end <= song.herd.smp_repeat {
            song.herd.smp_end = song.herd.smp_repeat + 1;
        }
    }
    // Toot the current row with backtick key
    let toot_unit = shared.active_unit.or(piano_state.toot);
    if let Some(mp) = mouse_screen_pos
        && let Some(unit) = toot_unit
    {
        if ui.input(|inp| inp.key_pressed(egui::Key::Backtick)) {
            let Some(piano_key) = piano_key_from_y(mp, &re, rect, state) else {
                return;
            };
            piano_freeplay_play_note(song, dst_sps, piano_state, piano_key, unit);
        }
    }
    // Scroll left/right when pressing arrow keys
    // Roghly half a screen worth of width to allow more precise navigation when placing notes, etc.
    let scroll_delta_w = cr.width() * 0.6;
    if ui.input(|inp| inp.key_pressed(egui::Key::ArrowRight)) {
        ui.scroll_with_delta(egui::vec2(-scroll_delta_w, 0.0));
    } else if ui.input(|inp| inp.key_pressed(egui::Key::ArrowLeft)) {
        ui.scroll_with_delta(egui::vec2(scroll_delta_w, 0.0));
    }
    // Scroll to begin/end with Home/End
    if ui.input(|inp| inp.key_pressed(egui::Key::Home)) {
        ui.scroll_with_delta(egui::vec2(rect.width(), 0.0));
    } else if ui.input(|inp| inp.key_pressed(egui::Key::End)) {
        ui.scroll_with_delta(egui::vec2(-rect.width(), 0.0));
    }
    // Delete selected notes with Del
    if ui.input(|inp| inp.key_pressed(egui::Key::Delete)) {
        let mut idx: usize = 0;
        song.song.events.retain(|_| {
            let retain = !state.selected_event_indices.contains(&idx);
            idx += 1;
            retain
        });
        state.selected_event_indices.clear();
    }
    if let Some(cmd) = state.ui_cmd.take() {
        match cmd {
            UiCmd::ScrollToPlayhead => scroll_to_playhead(ui, cr, playhead_x),
        }
    }
}

fn mouse_tick_meas(
    mp: egui::Pos2,
    rect: egui::Rect,
    tick_div: f32,
    timing: Timing,
) -> (Tick, Meas) {
    let pos = mp - rect.left_top();
    let tick = (pos.x * tick_div) as u32;
    let meas = tick_to_meas(tick, timing).saturating_sub(1);
    (tick, meas)
}

fn events_window_inner_ui(
    song: &mut SongState,
    cmd: &mut CommandQueue,
    ui: &mut egui::Ui,
    popup: &EventsPopup,
) {
    for &ev_idx in &popup.indices {
        let Some(ev) = song.song.events.get_mut(ev_idx) else {
            ui.label("<unresolved event (oob)>");
            continue;
        };
        if let Some(unit) = song.herd.units.get(ev.unit.usize()) {
            ui.colored_label(unit_color(ev.unit.usize()), &unit.name);
        } else {
            ui.label("<unresolved unit>");
        }
        ui.label("Tick");
        ui.add(egui::DragValue::new(&mut ev.tick));
        match &mut ev.payload {
            EventPayload::Null => {
                ui.label("<null>");
            }
            EventPayload::On { duration } => {
                ui.label("On for");
                ui.add(egui::DragValue::new(duration));
            }
            EventPayload::Key(key) => {
                ui.label("Key of");
                ui.add(egui::DragValue::new(key));
            }
            _ => {
                ui.label("<event>");
            }
        }
        if ui.button("Open in events tab").clicked() {
            cmd.push(Cmd::OpenEventInEventsTab { index: ev_idx });
        }
        ui.end_row();
    }
}

fn piano_key_from_y(
    pos: egui::Pos2,
    re: &egui::Response,
    rect: egui::Rect,
    state: &PianoRollState,
) -> Option<ptcow::Key> {
    let scroll_y = pos.y - re.rect.min.y;
    let y_offset = rect.height() - scroll_y;
    let Ok(kb_key): Result<u8, _> = (y_offset as i32 / (state.row_size as i32)).try_into() else {
        return None;
    };
    let kb_key = kb_key.saturating_add(1);
    let piano_key = kb_key as i32 + state.lowest_semitone as i32;
    Some(piano_key)
}

fn scroll_to_playhead(ui: &mut egui::Ui, cr: egui::Rect, playhead: f32) {
    if playhead > cr.right() || playhead < cr.left() {
        ui.scroll_with_delta(egui::vec2(cr.left() - playhead / 1.1, 0.0));
    }
}

/// Draw meas lines, as well as beat lines and quarter beat lines that are hovered
///
/// TODO: Division lines don't work for meas lines that are out of view
fn draw_meas_lines(
    song: &mut SongState,
    state: &mut PianoRollState,
    last_tick: u32,
    rect: egui::Rect,
    pnt: &egui::Painter,
    cr: egui::Rect,
    lines_drawn: &mut i32,
    mouse_pos: Option<egui::Pos2>,
) {
    let last_meas = ptcow::timing::tick_to_meas(last_tick, song.song.master.timing);
    for meas in 0..last_meas {
        let line_tick = ptcow::timing::meas_to_tick(meas, song.song.master.timing);
        let x = (line_tick as f32 / state.tick_div) + rect.min.x;
        // Don't draw meas lines if they are out of clip rect bounds
        if x < cr.min.x {
            continue;
        }
        if x > cr.max.x {
            break;
        }
        pnt.line_segment(
            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
            egui::Stroke::new(1.0, egui::Color32::LIGHT_GRAY),
        );
        *lines_drawn += 1;
        pnt.text(
            egui::pos2(x + 2.0, cr.min.y),
            egui::Align2::LEFT_TOP,
            meas,
            egui::FontId::proportional(16.0),
            egui::Color32::LIGHT_YELLOW,
        );
        if let Some(mouse_pos) = mouse_pos {
            let (_mouse_tick, mouse_meas) =
                mouse_tick_meas(mouse_pos, rect, state.tick_div, song.song.master.timing);
            if mouse_meas == meas {
                // Draw beat lines
                let beatline_gap = song.song.master.timing.ticks_per_beat as f32 / state.tick_div;
                let mut beatline_x = x;
                let min_draw_gap = 5.0;
                for _ in 0..song.song.master.timing.beats_per_meas {
                    // Don't draw beat lines if too small gap
                    if beatline_gap < min_draw_gap {
                        break;
                    }
                    // Skip drawing the first line not to draw over the meas line
                    if beatline_x != x {
                        pnt.line_segment(
                            [
                                egui::pos2(beatline_x, rect.min.y),
                                egui::pos2(beatline_x, rect.max.y),
                            ],
                            egui::Stroke::new(1.0, egui::Color32::GRAY),
                        );
                    }

                    // Draw quarter beat lines
                    let quarter_beatline_gap = beatline_gap / 4.0;
                    let mut quarter_beatline_x = beatline_x + quarter_beatline_gap;
                    if mouse_pos.x > beatline_x && mouse_pos.x < beatline_x + beatline_gap {
                        // Don't draw quarter beat lines if too small gap
                        for _ in 0..3 {
                            if quarter_beatline_gap < min_draw_gap {
                                break;
                            }
                            pnt.line_segment(
                                [
                                    egui::pos2(quarter_beatline_x, rect.min.y),
                                    egui::pos2(quarter_beatline_x, rect.max.y),
                                ],
                                egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                            );
                            quarter_beatline_x += quarter_beatline_gap;
                        }
                    }
                    beatline_x += beatline_gap;
                }
            }
        }
    }
}

fn key_y(lowest_semitone: u8, row_size: f32, rect: egui::Rect, k: i32) -> f32 {
    let semitone = k as f32 / 256.0;
    let y_offset = (semitone - lowest_semitone as f32) * row_size;
    rect.max.y - y_offset
}

fn piano_ui(
    song: &mut SongState,
    state: &mut PianoRollState,
    ui: &mut egui::Ui,
    y_offset: f32,
    piano_state: &mut FreeplayPianoState,
    dst_sps: SampleRate,
) {
    egui::ScrollArea::vertical()
        .id_salt("left")
        .scroll_offset(egui::vec2(0., y_offset))
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            let (rect, _re) = ui.allocate_exact_size(
                egui::vec2(64.0, state.n_rows as f32 * state.row_size),
                egui::Sense::click(),
            );
            let black_color = egui::Color32::from_rgb(8, 8, 12);
            let white_color = egui::Color32::from_rgb(224, 224, 233);
            let pnt = ui.painter_at(rect);
            // Rect bg fill
            pnt.rect_filled(rect, 0.0, white_color);
            // Draw keys
            for key in 0..state.n_rows {
                let KeyInfo {
                    semitone,
                    c_scale_idx,
                    octave,
                } = key_info(state.lowest_semitone, key);
                let y = key as f32 * state.row_size;
                let y = rect.max.y - y;
                let names = [
                    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
                ];

                let notation = names[c_scale_idx as usize];
                let sharp = notation.contains("#");
                let bg_color = if sharp { black_color } else { white_color };
                let text_color = if sharp { white_color } else { black_color };
                // Draw sharp keys shorter, and have a little shortening for white keys
                // so they don't stretch out of bounds.
                let w_reduction = if sharp { 24.0 } else { 3.0 };
                // We start rendering out of bounds so they keys look like they come out from
                // under the left edge
                let key_x_off = rect.min.x - 4.0;
                let key_rect = egui::Rect::from_min_max(
                    egui::pos2(key_x_off, y),
                    egui::pos2(rect.max.x - w_reduction, y + state.row_size),
                );
                // Draw base fill for key
                pnt.rect_filled(key_rect, 3.0, bg_color);
                // Display highlight for up to 4 simultaneously playing units
                let mut playing_colors = ArrayVec::<_, 4>::new();
                for (i, unit) in song.herd.units.iter().enumerate() {
                    if unit.key_now / 256 == semitone as i32 && !unit.mute && unit_alive(unit) {
                        if playing_colors.is_full() {
                            break;
                        }
                        playing_colors.push(unit_color(i));
                    }
                }
                let len = playing_colors.len();
                for (i, col) in playing_colors.into_iter().enumerate() {
                    pnt.rect_filled(rect_nth_column(key_rect, i, len), 3.0, col);
                }
                // Draw outline stroke for key
                pnt.rect_stroke(
                    key_rect,
                    3.0,
                    egui::Stroke::new(1.0, black_color),
                    egui::StrokeKind::Inside,
                );
                // Draw text
                // We limit max font size so sharp key text won't stretch out of bounds.
                let font_size = f32::min(state.row_size - 2.0, 17.0);
                pnt.text(
                    egui::pos2(rect.min.x + 2.0, y),
                    egui::Align2::LEFT_TOP,
                    format!("{notation} {octave}"),
                    egui::FontId::proportional(font_size),
                    text_color,
                );
                // We let the keyboard key also be pressed with a keyboard key, because
                // it can be more ergonomic in certain cases.
                let mouse_toot_key = egui::Key::Backtick;
                if let Some(unit_no) = piano_state.toot
                    && let Some(mouse_pos) = ui.input(|inp| inp.pointer.latest_pos())
                    && ui.ui_contains_pointer()
                    && ui.input(|inp| {
                        inp.pointer.primary_pressed() || inp.key_pressed(mouse_toot_key)
                    })
                {
                    let piano_key = key as i32 + state.lowest_semitone as i32;
                    if key_rect.y_range().contains(mouse_pos.y) {
                        piano_freeplay_play_note(song, dst_sps, piano_state, piano_key, unit_no);
                    }
                }
            }
        });
}

/// Return the nth column of an `egui::Rect` divided into `total` columns
fn rect_nth_column(rect: egui::Rect, n: usize, total: usize) -> egui::Rect {
    assert!(n < total, "column index out of range");

    let left = rect.min.x + rect.width() * n as f32 / total as f32;
    let right = rect.min.x + rect.width() * (n + 1) as f32 / total as f32;

    egui::Rect::from_min_max(egui::pos2(left, rect.min.y), egui::pos2(right, rect.max.y))
}

#[derive(Debug)]
struct KeyInfo {
    semitone: u8,
    c_scale_idx: u16,
    octave: i16,
}

fn key_info(lowest_semitone: u8, key: u8) -> KeyInfo {
    let semitone = lowest_semitone + key;
    let name_offset = 9;
    let c_scale_idx = (semitone as u16 + name_offset) % 12;
    let octave = ((semitone as i16 + name_offset as i16) / 12) - 4;
    KeyInfo {
        semitone,
        c_scale_idx,
        octave,
    }
}

fn unit_alive(unit: &Unit) -> bool {
    unit.tones.iter().any(|tone| tone.life_count != 0)
}

fn piano_roll_config_popup_button(ui: &mut egui::Ui, state: &mut PianoRollState) {
    let re = ui.button("🎹 Piano roll config");
    egui::Popup::menu(&re)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.style_mut().spacing.slider_width = 240.0;
            egui::Grid::new("roll_cfg_grid").show(ui, |ui| {
                ui.label("Clock div");
                ui.add(egui::Slider::new(&mut state.tick_div, 0.1..=100.0));
                ui.end_row();
                ui.label("Num of rows");
                ui.add(egui::Slider::new(&mut state.n_rows, 8..=128));
                ui.end_row();
                ui.label("Row size");
                ui.add(egui::Slider::new(&mut state.row_size, 6.0..=48.0));
                ui.end_row();
                ui.label("Lowest semitone");
                ui.add(egui::Slider::new(&mut state.lowest_semitone, 0..=128));
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.draw_meas_lines, "Draw meas lines");
                ui.checkbox(&mut state.snap_to_quarter_beat, "Snap to quarter beat")
                    .on_hover_text("Snap placed notes to quarter beats");
            });
        });
}

fn loop_points_popup_button(ui: &mut egui::Ui, song: &mut SongState) {
    let re = ui.button("🔁 Loop points");
    egui::Popup::menu(&re)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let mut loop_points_changed = false;
            egui::Grid::new("loop_pts_grid").show(ui, |ui| {
                if ui.link("Repeat meas").clicked() {
                    song.herd.seek_to_meas(
                        song.song.master.loop_points.repeat,
                        &song.song,
                        &song.ins,
                    );
                }
                loop_points_changed |= ui
                    .add(egui::DragValue::new(&mut song.song.master.loop_points.repeat).speed(0.1))
                    .changed();
                ui.end_row();
                let mut seek_to_meas = None;
                match &mut song.song.master.loop_points.last {
                    Some(last) => {
                        if ui.link("Last meas").clicked() {
                            seek_to_meas = Some(*last);
                        }
                        loop_points_changed |=
                            ui.add(egui::DragValue::new(last).speed(0.1)).changed();
                        if ui.button("Remove").clicked() {
                            song.song.master.loop_points.last = None;
                            loop_points_changed = true;
                        }
                    }
                    None => {
                        if ui.button("Add last").clicked() {
                            song.song.master.loop_points.last = NonZeroMeas::new(1);
                            loop_points_changed = true;
                        }
                    }
                }
                if let Some(meas) = seek_to_meas {
                    song.herd.seek_to_meas(meas.get(), &song.song, &song.ins);
                }
                if ui.button("🔚").clicked()
                    && let Some(last) = song.song.events.last()
                {
                    let last_tick = last.tick;
                    song.song.master.loop_points.last = NonZeroMeas::new(
                        ptcow::timing::tick_to_meas(last_tick, song.song.master.timing),
                    );
                    loop_points_changed = true;
                }
            });

            if loop_points_changed {
                song.herd.smp_repeat = ptcow::timing::meas_to_sample(
                    song.song.master.loop_points.repeat,
                    song.ins.samples_per_tick,
                    song.song.master.timing,
                );
                if let Some(last) = song.song.master.loop_points.last {
                    song.herd.smp_end = ptcow::timing::meas_to_sample(
                        last.get(),
                        song.ins.samples_per_tick,
                        song.song.master.timing,
                    );
                } else {
                    song.herd.smp_end = ptcow::timing::meas_to_sample(
                        song.song.master.end_meas(),
                        song.ins.samples_per_tick,
                        song.song.master.timing,
                    );
                }
            }
        });
}

fn experimental_popup_button(ui: &mut egui::Ui, song: &mut SongState, state: &mut PianoRollState) {
    let re = ui.button("🐛 Debug/Experimental");
    egui::Popup::menu(&re).show(|ui| {
        egui::Grid::new("debug_exp_popup").show(ui, |ui| {
            ui.checkbox(&mut state.draw_debug_info, "Draw debug info");
            ui.end_row();
            ui.label("Shift all notes");
            let re = ui.add(egui::DragValue::new(&mut state.shift_all_offset).range(-48..=48));
            if re.dragged() {
                for ev in &mut *song.song.events {
                    ev.tick = ev.tick.saturating_add_signed(state.shift_all_offset);
                }
            }
            if ui.input(|inp| inp.pointer.primary_released()) {
                state.shift_all_offset = 0;
            }
        });
    });
}

fn help_popup_button(ui: &mut egui::Ui, interact_mode: InteractMode) {
    let re = ui.button("？ Help");
    egui::Popup::menu(&re).show(|ui| {
        egui::Grid::new("help_popup").show(ui, |ui| {
            ui.label("Seek");
            ui.input_label("Shift+lmb");
            ui.end_row();
            match interact_mode {
                InteractMode::View => {
                    ui.label("Note info");
                    ui.input_label("lmb");
                }
                InteractMode::Edit => {
                    ui.label("Selection box");
                    ui.input_label("lmb drag");
                }
                InteractMode::Place => {
                    ui.label("Place note");
                    ui.input_label("lmb");
                    ui.end_row();
                    ui.label("Remove note");
                    ui.input_label("rmb");
                }
            }
            ui.end_row();
            ui.label("Hover info");
            ui.input_label("Hold alt");
            ui.end_row();
            ui.label("Scroll left/right");
            ui.input_label("Left arrow");
            ui.input_label("Right arrow");
            ui.end_row();
            ui.label("Scroll to start/end");
            ui.input_label("Home");
            ui.input_label("End");
            ui.end_row();
            ui.label("Delete selected items");
            ui.input_label("Del");
        });
    });
}

trait PianoRollUiExt {
    fn input_label(&mut self, text: &str);
}

impl PianoRollUiExt for egui::Ui {
    fn input_label(&mut self, text: &str) {
        egui::Frame::default()
            .stroke(egui::Stroke::new(1.0, egui::Color32::BLACK))
            .inner_margin(4.0)
            .fill(DARKER_GRAY)
            .corner_radius(3.0)
            .show(self, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.label(egui::RichText::new(text).color(egui::Color32::LIGHT_GRAY));
            });
    }
}

pub const DARKER_GRAY: egui::Color32 = egui::Color32::from_rgb(54, 54, 54);
