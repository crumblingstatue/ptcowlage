use {
    crate::{
        app::{
            ModalPayload,
            command_queue::{Cmd, CommandQueue},
            ui::{
                group_idx_slider,
                tabs::voices::VoicesUiState,
                unit::{UnitPopupTab, handle_units_command, unit_popup_ui},
                unit_color,
            },
        },
        audio_out::{AuxAudioState, SongState},
        evilscript,
    },
    eframe::egui,
    egui_extras::Column,
    egui_toast::{Toast, ToastKind, ToastOptions, Toasts},
    ptcow::{Event, EventPayload, GroupIdx, PanTime, SampleRate, UnitIdx, VoiceIdx},
};

pub struct RawEventsUiState {
    follow: bool,
    unit_popup_tab: UnitPopupTab,
    pub filter: Filter,
    filtered_events: Vec<usize>,
    pub go_to: Option<usize>,
    pub highlight: Option<usize>,
    cmd_string_buf: String,
    toasts: Toasts,
    pub filter_needs_recalc: bool,
    preview_unit_changes: bool,
}

impl Default for RawEventsUiState {
    fn default() -> Self {
        Self {
            follow: Default::default(),
            unit_popup_tab: UnitPopupTab::Unit,
            filter: Filter::default(),
            filtered_events: Vec::new(),
            go_to: None,
            highlight: None,
            cmd_string_buf: String::new(),
            toasts: Toasts::new()
                .anchor(egui::Align2::RIGHT_BOTTOM, egui::Pos2::ZERO)
                .direction(egui::Direction::BottomUp),
            filter_needs_recalc: true,
            preview_unit_changes: true,
        }
    }
}

enum EventListCmd {
    Remove { idx: usize },
    Insert { idx: usize, event: Event },
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct Filter {
    pub unit: Option<UnitIdx>,
    pub event: Option<u8>,
}

impl Filter {
    const fn is_active(&self) -> bool {
        self.unit.is_some() || self.event.is_some()
    }
}

pub fn ui(
    ui: &mut egui::Ui,
    song: &mut SongState,
    ui_state: &mut RawEventsUiState,
    out_rate: SampleRate,
    aux: &mut Option<AuxAudioState>,
    voices_ui_state: &mut VoicesUiState,
    app_cmd: &mut CommandQueue,
    app_modal_payload: &mut Option<ModalPayload>,
) {
    ui_state.toasts.show(ui.ctx());
    top_ui(ui, song, ui_state);

    ui.separator();
    // Work around overlapping borrows of units
    let unit_names: Vec<String> = song
        .herd
        .units
        .iter()
        .map(|unit| unit.name.clone())
        .collect();
    let mut unit_cmd = None;
    let mut ev_list_cmd = None;
    let k_c = ui.input(|inp| inp.key_pressed(egui::Key::C));
    let mut table = egui_extras::TableBuilder::new(ui)
        .striped(true)
        .column(Column::initial(48.0))
        .column(Column::auto())
        .column(Column::auto())
        .column(Column::remainder());
    if let Some(go_to) = ui_state.go_to.take() {
        ui_state.follow = false;
        ui_state.highlight = Some(go_to);
        table = table.scroll_to_row(go_to, Some(egui::Align::Center));
    }
    if ui_state.follow {
        if ui_state.filter.is_active() {
            if let Some(idx) = ui_state
                .filtered_events
                .iter()
                .position(|idx| *idx == song.herd.evt_idx)
            {
                table = table.scroll_to_row(idx, Some(egui::Align::Center));
            }
        } else {
            table = table.scroll_to_row(song.herd.evt_idx, Some(egui::Align::Center));
        }
    }
    table
        .header(16.0, |mut header| {
            header.col(|ui| {
                ui.label("no.");
            });
            header.col(|ui| {
                ui.label("tick");
            });
            header.col(|ui| {
                ui.label("unit");
            });
            header.col(|ui| {
                ui.label("payload");
            });
        })
        .body(|mut body| {
            body.ui_mut().style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            let rows_len = if ui_state.filter.is_active() {
                ui_state.filtered_events.len()
            } else {
                song.song.events.len()
            };
            body.rows(16.0, rows_len, |mut row| {
                let mut idx = row.index();
                // Redirect to event from filtered index
                if ui_state.filter.is_active() {
                    idx = ui_state.filtered_events[idx];
                }
                if song.herd.evt_idx == idx {
                    row.set_selected(true);
                }
                if let Some(highlight) = ui_state.highlight
                    && highlight == idx
                {
                    row.set_selected(true);
                }
                // Unfortunately we need to clone the events here to get around overlapping borrow issue.
                // This clone is used inside the unit popup ui for the voice history feature.
                let eves_clone = song.song.events.clone();
                let Some(ev) = song.song.events.get_mut(idx) else {
                    row.col(|ui| {
                        ui.label("<out of bounds>");
                    });
                    return;
                };
                let (_rect, re) = row.col(|ui| {
                    ui.add(egui::Label::new(idx.to_string()).sense(egui::Sense::click()))
                        .context_menu(|ui| {
                            if ui.button("Delete").clicked() {
                                ev_list_cmd = Some(EventListCmd::Remove { idx });
                            }
                            ui.menu_button("Insert", |ui| {
                                let mut payload = None;
                                if ui.button("Volume").clicked() {
                                    payload = Some(EventPayload::Volume(127));
                                }
                                if ui.button("Voice").clicked() {
                                    payload = Some(EventPayload::SetVoice(VoiceIdx(0)));
                                }
                                if ui.button("Group").clicked() {
                                    payload = Some(EventPayload::SetGroup(GroupIdx(0)));
                                }
                                if let Some(payload) = payload {
                                    let event = Event {
                                        payload,
                                        unit: ev.unit,
                                        tick: ev.tick,
                                    };
                                    ev_list_cmd = Some(EventListCmd::Insert { idx, event });
                                }
                            });
                            if ui.button("Clone (C)").clicked() {
                                ev_list_cmd = Some(EventListCmd::Insert { idx, event: *ev });
                            }
                        });
                });
                if re.hovered() && k_c {
                    ev_list_cmd = Some(EventListCmd::Insert { idx, event: *ev });
                }
                row.col(|ui| {
                    if ui.link(ev.tick.to_string()).clicked() {
                        song.herd.evt_idx = idx;
                        song.herd.seek_to_sample(ptcow::timing::tick_to_sample(
                            ev.tick,
                            song.ins.samples_per_tick,
                        ));
                    }
                });
                row.col(|ui| match song.herd.units.get_mut(ev.unit.usize()) {
                    Some(unit) => {
                        #[derive(Clone, Copy)]
                        enum PopupKind {
                            UnitUi,
                            UnitPicker,
                        }

                        let re = ui.link(unit_rich_text(ev.unit, &unit.name));
                        let mut toggle = false;
                        if re.clicked_by(egui::PointerButton::Secondary) {
                            ui.memory_mut(|mem| mem.data.insert_temp(re.id, PopupKind::UnitUi));
                            toggle = true;
                        } else if re.clicked_by(egui::PointerButton::Primary) {
                            ui.memory_mut(|mem| mem.data.insert_temp(re.id, PopupKind::UnitPicker));
                            toggle = true;
                        }
                        let popup_kind = ui.memory(|mem| mem.data.get_temp::<PopupKind>(re.id));
                        let close_behavior = match popup_kind {
                            Some(PopupKind::UnitUi) => {
                                egui::PopupCloseBehavior::CloseOnClickOutside
                            }
                            _ => egui::PopupCloseBehavior::CloseOnClick,
                        };
                        egui::Popup::from_response(&re)
                            .close_behavior(close_behavior)
                            .open_memory(toggle.then_some(egui::SetOpenCommand::Toggle))
                            .show(|ui| {
                                let Some(kind) = popup_kind else {
                                    return;
                                };
                                match kind {
                                    PopupKind::UnitUi => {
                                        let idx = ev.unit;
                                        unit_popup_ui(
                                            ui,
                                            idx,
                                            unit,
                                            &mut song.ins,
                                            &mut unit_cmd,
                                            &mut ui_state.unit_popup_tab,
                                            out_rate,
                                            aux,
                                            voices_ui_state,
                                            app_cmd,
                                            &eves_clone,
                                        );
                                    }
                                    PopupKind::UnitPicker => {
                                        for (idx, unit_name) in unit_names.iter().enumerate() {
                                            if ui
                                                .button(
                                                    egui::RichText::new(unit_name)
                                                        .color(unit_color(idx)),
                                                )
                                                .clicked()
                                            {
                                                ev.unit = UnitIdx(idx as u8);
                                            }
                                        }
                                    }
                                }
                            });
                    }
                    None => {
                        ui.label(
                            egui::RichText::new(format!("<invalid:{}>", ev.unit.0))
                                .color(egui::Color32::RED),
                        );
                    }
                });
                row.col(|ui| match &mut ev.payload {
                    EventPayload::Null => {
                        ui.label("null");
                    }
                    EventPayload::On { duration } => {
                        ui.horizontal(|ui| {
                            ui.label("On");
                            ui.add(egui::DragValue::new(duration));
                        });
                    }
                    EventPayload::Key(key) => {
                        ui.horizontal(|ui| {
                            ui.label("Key");
                            ui.add(egui::DragValue::new(key));
                        });
                    }
                    EventPayload::PanVol(vol) => {
                        ui.horizontal(|ui| {
                            ui.label("Pan volume");
                            ui.add(egui::DragValue::new(vol));
                        });
                    }
                    EventPayload::Velocity(vel) => {
                        ui.horizontal(|ui| {
                            ui.label("Velocity");
                            ui.add(egui::DragValue::new(vel));
                        });
                    }
                    EventPayload::Volume(vol) => {
                        ui.horizontal(|ui| {
                            ui.label("Volume");
                            let changed = ui.add(egui::Slider::new(vol, 0..=256)).changed();
                            if changed && ui_state.preview_unit_changes {
                                song.herd.units[ev.unit.usize()].volume = *vol;
                            }
                        });
                    }
                    EventPayload::Portament { duration } => {
                        ui.horizontal(|ui| {
                            ui.label("Portament");
                            ui.add(egui::DragValue::new(duration));
                        });
                    }
                    EventPayload::BeatClock => {
                        ui.label("BeatClock");
                    }
                    EventPayload::BeatTempo => {
                        ui.label("BeatTempo");
                    }
                    EventPayload::BeatNum => {
                        ui.label("BeatNum");
                    }
                    EventPayload::Repeat => {
                        ui.label("Repeat");
                    }
                    EventPayload::Last => {
                        ui.label("Last");
                    }
                    EventPayload::SetVoice(v_idx) => {
                        ui.horizontal(|ui| {
                            let mut num_usize = v_idx.usize();
                            egui::ComboBox::new("v_dropdown", "Voice").show_index(
                                ui,
                                &mut num_usize,
                                song.ins.voices.len(),
                                |idx| {
                                    song.ins.voices.get(idx).map_or_else(
                                        || {
                                            egui::RichText::new("<invalid>")
                                                .color(egui::Color32::RED)
                                        },
                                        |v| egui::RichText::new(&v.name),
                                    )
                                },
                            );
                            *v_idx = VoiceIdx(num_usize.try_into().unwrap());
                            if ui.button("⮩").clicked() {
                                app_cmd.push(crate::app::command_queue::Cmd::OpenVoice(*v_idx));
                            }
                        });
                    }
                    EventPayload::SetGroup(group_idx) => {
                        ui.horizontal(|ui| {
                            ui.label("Group");
                            group_idx_slider(ui, group_idx);
                        });
                    }
                    EventPayload::Tuning(val) => {
                        ui.horizontal(|ui| {
                            ui.label("Tuning");
                            ui.add(egui::DragValue::new(val));
                        });
                    }
                    EventPayload::PanTime(val) => {
                        ui.horizontal(|ui| {
                            ui.label("Pan time");
                            let changed =
                                ui.add(crate::app::ui::unit::pan_time_slider(val)).changed();
                            if changed && ui_state.preview_unit_changes {
                                song.herd.units[ev.unit.usize()].pan_time_offs =
                                    val.to_lr_offsets(out_rate);
                            }
                        });
                    }
                    EventPayload::PtcowDebug(val) => {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("Debug: {val}"))
                                    .color(egui::Color32::YELLOW),
                            );
                        });
                    }
                });
            });
        });
    if let Some(evt_discr) = ui_state.filter.event
        && let Some(unit_idx) = ui_state.filter.unit
        && ui_state.filtered_events.is_empty()
    {
        ui.label("Looks like there are no events for this filter");
        if ui.button("Insert at beginning").clicked() {
            'block: {
                let payload = match evt_discr {
                    1 => EventPayload::On { duration: 0 },
                    2 => EventPayload::Key(0),
                    3 => EventPayload::PanVol(0),
                    4 => EventPayload::Velocity(0),
                    5 => EventPayload::Volume(0),
                    6 => EventPayload::Portament { duration: 0 },
                    7 => EventPayload::BeatClock,
                    8 => EventPayload::BeatTempo,
                    9 => EventPayload::BeatNum,
                    10 => EventPayload::Repeat,
                    11 => EventPayload::Last,
                    12 => EventPayload::SetVoice(VoiceIdx(0)),
                    13 => EventPayload::SetGroup(GroupIdx(0)),
                    14 => EventPayload::Tuning(0.0),
                    15 => EventPayload::PanTime(PanTime::default()),
                    _ => break 'block,
                };
                app_cmd.push(Cmd::InsertEvent {
                    idx: 0,
                    event: Event {
                        payload,
                        unit: unit_idx,
                        tick: 0,
                    },
                });
                ui_state.filter_needs_recalc = true;
            }
        }
    }
    handle_units_command(unit_cmd, song, app_modal_payload);
    if let Some(cmd) = ev_list_cmd {
        match cmd {
            EventListCmd::Remove { idx } => {
                song.song.events.remove(idx);
            }
            EventListCmd::Insert { idx, event } => {
                song.song.events.insert(idx, event);
            }
        }
        ui_state.filter_needs_recalc = true;
    }
}

fn top_ui(ui: &mut egui::Ui, song: &mut SongState, ui_state: &mut RawEventsUiState) {
    ui.horizontal(|ui| {
        let re = ui.add(
            egui::TextEdit::singleline(&mut ui_state.cmd_string_buf).hint_text("Evil command line"),
        );
        if re.lost_focus() && ui.input(|inp| inp.key_pressed(egui::Key::Enter)) {
            match evilscript::parse(&ui_state.cmd_string_buf) {
                Ok(cmd) => {
                    if let Some(out) = evilscript::exec(cmd, song) {
                        ui_state.toasts.add(
                            Toast::new()
                                .kind(ToastKind::Info)
                                .text(out)
                                .options(ToastOptions::default().duration_in_seconds(15.0)),
                        );
                    }
                }
                Err(e) => {
                    ui_state.toasts.add(
                        Toast::new()
                            .kind(ToastKind::Error)
                            .text(e.to_string())
                            .options(ToastOptions::default().duration_in_seconds(5.0)),
                    );
                }
            }
            ui_state.cmd_string_buf.clear();
        }
        ui.checkbox(&mut ui_state.follow, "Follow");
        ui.separator();
        ui.label("Filter");
        let selected_text = match &ui_state.filter.unit {
            Some(u) => unit_rich_text(
                *u,
                song.herd
                    .units
                    .get(u.usize())
                    .map_or("unresolved", |unit| &unit.name),
            ),
            None => "Off".into(),
        };
        egui::ComboBox::new("filter_cb", "Unit")
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(ui_state.filter.unit.is_none(), "Off")
                    .clicked()
                {
                    ui_state.filter.unit = None;
                    ui_state.filter_needs_recalc = true;
                }
                for (i, unit) in song.herd.units.iter().enumerate() {
                    let idx = UnitIdx(i as u8);
                    if ui
                        .selectable_label(
                            ui_state.filter.unit == Some(idx),
                            unit_rich_text(idx, &unit.name),
                        )
                        .clicked()
                    {
                        ui_state.filter.unit = Some(idx);
                        ui_state.filter_needs_recalc = true;
                    }
                }
            });
        let selected_text = ui_state
            .filter
            .event
            .map_or("Off", |disc| ev_discr_name(disc));
        egui::ComboBox::new("event_cb", "Event")
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                if ui.button("Off").clicked() {
                    ui_state.filter.event = None;
                    ui_state.filter_needs_recalc = true;
                }
                for i in 0..16 {
                    if ui
                        .selectable_label(ui_state.filter.event == Some(i), ev_discr_name(i))
                        .clicked()
                    {
                        ui_state.filter.event = Some(i);
                        ui_state.filter_needs_recalc = true;
                    }
                }
            });
        if ui.button("ｘ").on_hover_text("Clear").clicked() {
            ui_state.filter = Filter::default();
            ui_state.filter_needs_recalc = true;
        }
        ui.separator();
        ui.checkbox(&mut ui_state.preview_unit_changes, "Preview unit changes");
        if ui
            .button("⚠ Clean up")
            .on_hover_text("[EXPERIMENTAL] Remove \"losing\" events on the same tick")
            .clicked()
        {
            let orig_len = song.song.events.len();
            crate::pxtone_misc::clean_losing_events(&mut song.song.events);
            let n_removed = orig_len - song.song.events.len();
            ui_state.toasts.add(
                Toast::new()
                    .kind(ToastKind::Info)
                    .text(format!("Removed {n_removed} events"))
                    .options(ToastOptions::default().duration_in_seconds(8.0)),
            );
            ui_state.filter_needs_recalc = true;
        }
        // Recalculate filtered events if filter changed
        if ui_state.filter_needs_recalc {
            ui_state.filtered_events = song
                .song
                .events
                .eves
                .iter()
                .enumerate()
                .filter_map(|(idx, evt)| {
                    if let Some(idx) = ui_state.filter.unit
                        && evt.unit != idx
                    {
                        return None;
                    }
                    if let Some(disc) = ui_state.filter.event
                        && disc != evt.payload.discriminant()
                    {
                        return None;
                    }
                    Some(idx)
                })
                .collect();
            ui_state.filter_needs_recalc = false;
        }
    });
}

const fn ev_discr_name(discr: u8) -> &'static str {
    match discr {
        0 => "Null",
        1 => "On",
        2 => "Key",
        3 => "PanVolume",
        4 => "Velocity",
        5 => "Volume",
        6 => "Portament",
        7 => "BeatClock",
        8 => "BeatTempo",
        9 => "BeatNum",
        10 => "Repeat",
        11 => "Last",
        12 => "VoiceNo",
        13 => "GroupNo",
        14 => "Tuning",
        15 => "PanTime",
        16 => "PtcowDebug",
        _ => "Unknown",
    }
}

fn unit_rich_text(idx: UnitIdx, text: &str) -> egui::RichText {
    let color = unit_color(idx.usize());
    egui::RichText::new(text)
        .color(invert_color(color))
        .background_color(color)
}

pub fn invert_color(color: egui::Color32) -> egui::Color32 {
    // Color32 stores rgba as u8 values
    let [r, g, b, a] = color.to_array();

    egui::Color32::from_rgba_premultiplied(
        255 - r,
        255 - g,
        255 - b,
        a, // keep alpha the same
    )
}
