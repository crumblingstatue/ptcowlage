use {
    crate::{
        app::{
            command_queue::{Cmd, CommandQueue},
            ui::unit_color,
        },
        audio_out::SongState,
        herd_ext::HerdExt,
    },
    eframe::egui::{self, scroll_area::ScrollBarVisibility},
    ptcow::{
        EventPayload, SampleT,
        timing::{NonZeroMeas, tick_to_meas},
    },
    rustc_hash::FxHashSet,
};

pub struct MapState {
    tick_div: f32,
    row_size: f32,
    follow_playhead: bool,
    shift_all_offset: i32,
    prev_frame_piano_roll_y_offset: f32,
    draw_debug_info: bool,
    // TODO: Implement Hash for `UnitIdx`
    pub hidden_units: FxHashSet<u8>,
    draw_meas_lines: bool,
    ui_cmd: Option<UiCmd>,
}

enum UiCmd {
    ScrollToPlayhead,
}

impl Default for MapState {
    fn default() -> Self {
        Self {
            tick_div: 10.0,
            row_size: 20.0,
            follow_playhead: true,
            shift_all_offset: 0,
            prev_frame_piano_roll_y_offset: 0.,
            draw_debug_info: false,
            hidden_units: FxHashSet::default(),
            draw_meas_lines: true,
            ui_cmd: None,
        }
    }
}

fn top_ui(ui: &mut egui::Ui, song: &mut SongState, state: &mut MapState) {
    ui.horizontal(|ui| {
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
        help_popup_button(ui);
    });
    ui.separator();
}

pub fn ui(ui: &mut egui::Ui, song: &mut SongState, state: &mut MapState, cmd: &mut CommandQueue) {
    top_ui(ui, song, state);
    ui.horizontal_top(|ui| {
        ui.style_mut().spacing.item_spacing = egui::Vec2::ZERO;
        left_side_units_ui(song, state, ui, state.prev_frame_piano_roll_y_offset);
        roll_ui(song, state, ui, cmd);
    });
}

fn roll_ui(song: &mut SongState, state: &mut MapState, ui: &mut egui::Ui, cmd: &mut CommandQueue) {
    // We don't want dragging to scroll when editing
    let scroll_source = egui::scroll_area::ScrollSource::ALL;
    let out = egui::ScrollArea::both()
        .scroll_source(scroll_source)
        .show(ui, |ui| {
            roll_ui_inner(song, state, ui, cmd);
        });
    state.prev_frame_piano_roll_y_offset = out.state.offset.y;
}

const BG_FILL_COLOR: egui::Color32 = egui::Color32::from_rgb(30, 30, 24);

/// The ui inside the scroll area
#[expect(clippy::cast_precision_loss)]
fn roll_ui_inner(
    song: &mut SongState,
    state: &mut MapState,
    ui: &mut egui::Ui,
    cmd: &mut CommandQueue,
) {
    // We make up a value for number of ticks if there are no events in the song (empty song)
    // TODO: Maybe we can do something more clever here
    let last_tick = song.song.events.eves.last().map_or(5000, |ev| ev.tick);
    let mut approx_end = last_tick as f32 / state.tick_div;
    // Safety for ui alloc
    if !approx_end.is_finite() {
        approx_end = 1.0;
    }
    // Leave some "breathing room" at end, so new events can be placed, etc.
    approx_end += 200.0;
    let clock = ptcow::current_tick(&song.herd, &song.ins);
    let [mod_shift, mod_alt] = ui.input(|inp| [inp.modifiers.shift, inp.modifiers.alt]);
    let n_units = song.herd.units.len();
    let (rect, re) = ui.allocate_exact_size(
        egui::vec2(approx_end, n_units as f32 * state.row_size),
        egui::Sense::click(),
    );
    let pnt = ui.painter_at(rect);
    // BG fill
    pnt.rect_filled(rect, 2.0, BG_FILL_COLOR);
    let cr = ui.clip_rect();
    // Draw guide lines
    for unit_no in 0..n_units {
        let y = unit_no as f32 * state.row_size;
        let y = rect.min.y + y;
        let sharp = unit_no % 2 == 0;
        if sharp {
            pnt.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(cr.min.x, y),
                    egui::pos2(cr.max.x, y + state.row_size),
                ),
                0.0,
                egui::Color32::BLACK,
            );
        }
        pnt.line_segment(
            [egui::pos2(cr.min.x, y), egui::pos2(cr.max.x, y)],
            egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
        );
    }
    // Draw the piano roll items based on events
    let [mut rects_drawn, mut lines_drawn] = [0; _];
    let mut hovered_events = Vec::new();
    let mouse_screen_pos = ui.input(|inp| inp.pointer.latest_pos());
    for (ev_idx, ev) in song.song.events.eves.iter().enumerate() {
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
        #[expect(clippy::single_match)]
        match ev.payload {
            EventPayload::On { duration } => {
                let y = rect.min.y + ev.unit.0 as f32 * state.row_size;
                // We don't want the rects to overlap the guide lines, so we leave a bit of a margin
                let margin = 2.0;
                let rect = egui::Rect::from_min_max(
                    egui::pos2(x, y + margin),
                    egui::pos2(
                        x + duration as f32 / state.tick_div,
                        y + state.row_size - margin,
                    ),
                );
                // We skip drawing the rect if it's outside to the left of the clip rect
                if rect.max.x < cr.min.x {
                    continue;
                }
                interact_rect = Some(rect);
                pnt.rect_filled(rect, 2.0, clr);
                rects_drawn += 1;
            }
            _ => {}
        }
        // The interact rectangles are at the pos where they are drawn on the screen,
        // so no mouse coordinate conversion is required.
        // Also make sure we don't handle click if we're obscured by e.g. a popup
        if let Some(irect) = interact_rect
            && let Some(mp) = mouse_screen_pos
            && ui.ui_contains_pointer()
        {
            if irect.contains(mp) {
                if ui.input(|inp| inp.pointer.primary_clicked()) {
                    cmd.push(Cmd::OpenEventInEventsTab { index: ev_idx });
                }
                if mod_alt {
                    hovered_events.push(ev);
                }
            }
        }
    }

    if state.draw_meas_lines {
        draw_meas_lines(song, state, last_tick, rect, &pnt, cr, &mut lines_drawn);
    }

    if state.draw_debug_info {
        if let Some(mouse_pos) = ui.input(|inp| inp.pointer.latest_pos())
            && ui.clip_rect().contains(mouse_pos)
        {
            let pos = mouse_pos - rect.left_top();
            let tick = (pos.x * state.tick_div) as u32;
            let meas = tick_to_meas(tick, song.song.master.timing).saturating_sub(1);
            let pnt = ui.ctx().debug_painter();
            pnt.debug_text(
                mouse_pos + egui::vec2(0.0, -42.0),
                egui::Align2::LEFT_TOP,
                egui::Color32::WHITE,
                format!("tick: {tick}"),
            );
            pnt.debug_text(
                mouse_pos + egui::vec2(0.0, -24.0),
                egui::Align2::LEFT_TOP,
                egui::Color32::WHITE,
                format!("meas: {meas}"),
            );
            pnt.debug_text(
                mouse_pos + egui::vec2(0.0, -8.0),
                egui::Align2::LEFT_TOP,
                egui::Color32::WHITE,
                format!("Drawn {rects_drawn} rects, {lines_drawn} lines"),
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
    if let Some(pos) = re.interact_pointer_pos() {
        let mx = pos.x - rect.min.x;
        let scaled = mx * state.tick_div;
        let sample = scaled * song.ins.samples_per_tick;
        let sample = sample as SampleT;
        if re.clicked_by(egui::PointerButton::Primary) {
            if mod_shift {
                song.herd.seek_to_sample(sample);
            }
        }
        // Ensure end can't be smaller than repeat
        if song.herd.smp_end <= song.herd.smp_repeat {
            song.herd.smp_end = song.herd.smp_repeat + 1;
        }
    }
    if let Some(cmd) = state.ui_cmd.take() {
        match cmd {
            UiCmd::ScrollToPlayhead => scroll_to_playhead(ui, cr, playhead_x),
        }
    }
}

fn scroll_to_playhead(ui: &mut egui::Ui, cr: egui::Rect, playhead: f32) {
    if playhead > cr.right() || playhead < cr.left() {
        ui.scroll_with_delta(egui::vec2(cr.left() - playhead / 1.1, 0.0));
    }
}

fn draw_meas_lines(
    song: &mut SongState,
    state: &mut MapState,
    last_tick: u32,
    rect: egui::Rect,
    pnt: &egui::Painter,
    cr: egui::Rect,
    lines_drawn: &mut i32,
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
    }
}

fn left_side_units_ui(
    song: &mut SongState,
    state: &mut MapState,
    ui: &mut egui::Ui,
    y_offset: f32,
) {
    let unit_no = song.herd.units.len();
    egui::ScrollArea::vertical()
        .id_salt("left")
        .scroll_offset(egui::vec2(0., y_offset))
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            let (rect, _re) = ui.allocate_exact_size(
                egui::vec2(96.0, unit_no as f32 * state.row_size),
                egui::Sense::click(),
            );
            let pnt = ui.painter_at(rect);
            // Rect bg fill
            pnt.rect_filled(rect, 0.0, BG_FILL_COLOR);
            // Draw keys
            for unit_idx in 0..unit_no {
                // Draw guide lines
                let y = unit_idx as f32 * state.row_size;
                let y = rect.min.y + y;
                let sharp = unit_idx % 2 == 0;
                let key_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.min.x, y),
                    egui::pos2(rect.max.x, y + state.row_size),
                );
                // Draw base fill for key
                if sharp {
                    pnt.rect_filled(key_rect, 3.0, egui::Color32::BLACK);
                }
                // Draw text
                // We limit max font size so sharp key text won't stretch out of bounds.
                let font_size = f32::min(state.row_size - 4.0, 14.0);
                let unit_name = &song.herd.units[unit_idx].name;
                pnt.text(
                    egui::pos2(rect.min.x + 2.0, y),
                    egui::Align2::LEFT_TOP,
                    unit_name,
                    egui::FontId::proportional(font_size),
                    unit_color(unit_idx),
                );
                // Draw guide lines
                let cr = ui.clip_rect();
                pnt.line_segment(
                    [egui::pos2(cr.min.x, y), egui::pos2(cr.max.x, y)],
                    egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                );
            }
        });
}

fn piano_roll_config_popup_button(ui: &mut egui::Ui, state: &mut MapState) {
    let re = ui.button("📜 Map config");
    egui::Popup::menu(&re)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.style_mut().spacing.slider_width = 240.0;
            egui::Grid::new("roll_cfg_grid").show(ui, |ui| {
                ui.label("Clock div");
                ui.add(egui::Slider::new(&mut state.tick_div, 0.1..=100.0));
                ui.end_row();
                ui.label("Row size");
                // Anything below this is way too small to make out.
                // Also, egui seems to panic at <=4.0 row size for some reason
                let min_row_size = 10.0;
                ui.add(egui::Slider::new(&mut state.row_size, min_row_size..=48.0));
            });
            ui.separator();
            ui.checkbox(&mut state.draw_meas_lines, "Draw meas lines");
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
                    && let Some(last) = song.song.events.eves.last()
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

fn experimental_popup_button(ui: &mut egui::Ui, song: &mut SongState, state: &mut MapState) {
    let re = ui.button("🐛 Debug/Experimental");
    egui::Popup::menu(&re).show(|ui| {
        egui::Grid::new("debug_exp_popup").show(ui, |ui| {
            ui.checkbox(&mut state.draw_debug_info, "Draw debug info");
            ui.end_row();
            ui.label("Shift all notes");
            let re = ui.add(egui::DragValue::new(&mut state.shift_all_offset).range(-48..=48));
            if re.dragged() {
                for ev in &mut song.song.events.eves {
                    ev.tick = ev.tick.saturating_add_signed(state.shift_all_offset);
                }
            }
            if ui.input(|inp| inp.pointer.primary_released()) {
                state.shift_all_offset = 0;
            }
        });
    });
}

fn help_popup_button(ui: &mut egui::Ui) {
    let re = ui.button("？ Help");
    egui::Popup::menu(&re).show(|ui| {
        egui::Grid::new("help_popup").show(ui, |ui| {
            ui.label("Seek");
            ui.input_label("Shift+lmb");
            ui.end_row();
            ui.label("Hover info");
            ui.input_label("Hold alt");
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
