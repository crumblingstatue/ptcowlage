//! Common UI code for units, including the Unit tab, and the unit popups

use {
    crate::{
        app::{
            ModalPayload,
            command_queue::{Cmd, CommandQueue},
            poly_migrate_single,
            ui::{
                Tab, group_idx_slider, img,
                tabs::{
                    events::Filter,
                    voices::{VoicesUiState, voice_ui_inner},
                },
                voice_img,
            },
        },
        audio_out::{AuxAudioState, SongState},
        egui_ext::ImageExt as _,
    },
    eframe::egui::{self, AtomExt},
    ptcow::{
        EveList, Event, EventPayload, GroupIdx, MooInstructions, PanTime, SampleRate, Unit,
        UnitIdx, VoiceIdx,
    },
};

pub enum UnitsCmd {
    ToggleSolo { idx: UnitIdx },
    SeekFirstOnEvent { idx: UnitIdx },
    SeekNextOnEvent { idx: UnitIdx },
    DeleteUnit { idx: UnitIdx },
    SeekPrevOnEvent { idx: UnitIdx },
    MigrateUnitEvents { idx: UnitIdx },
}

pub fn unit_ui(
    ui: &mut egui::Ui,
    idx: UnitIdx,
    unit: &mut Unit,
    ins: &MooInstructions,
    cmd: &mut Option<UnitsCmd>,
    app_cmd: &mut CommandQueue,
    evelist: &[Event],
) {
    ui.horizontal(|ui| {
        ui.add(egui::Image::new(img::COW).hflip());
        ui.heading(format!("{} {}", idx.0, unit.name));
        ui.text_edit_singleline(&mut unit.name);
        if ui
            .button(
                egui::RichText::new("Delete unit")
                    .background_color(egui::Color32::DARK_RED)
                    .color(egui::Color32::WHITE),
            )
            .clicked()
        {
            *cmd = Some(UnitsCmd::DeleteUnit { idx });
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut unit.mute, "Mute");
        if ui.button("Solo").clicked() {
            *cmd = Some(UnitsCmd::ToggleSolo { idx });
        }
        if ui.button("⏮ First On event").clicked() {
            *cmd = Some(UnitsCmd::SeekFirstOnEvent { idx });
        }
        if ui.button("◀ Prev On event").clicked() {
            *cmd = Some(UnitsCmd::SeekPrevOnEvent { idx });
        }
        if ui.button("▶ Next On event").clicked() {
            *cmd = Some(UnitsCmd::SeekNextOnEvent { idx });
        }
        if ui.button("Migrate overlapping events").clicked() {
            *cmd = Some(UnitsCmd::MigrateUnitEvents { idx });
        }
    });

    ui.separator();

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
        if ui.link("Pan vol l").clicked() {
            app_cmd.push(Cmd::SetEventsFilter(Filter {
                unit: Some(idx),
                event: Some(EventPayload::PanVol(0).discriminant()),
            }));
            app_cmd.push(Cmd::SetActiveTab(Tab::Events));
        }
        ui.add(egui::DragValue::new(&mut unit.pan_vols[0]));
        ui.label("r");
        ui.add(egui::DragValue::new(&mut unit.pan_vols[1]));
        let mut pan_time = PanTime::from_lr_offsets(unit.pan_time_offs, ins.out_sample_rate);
        if ui.link("Pan time").clicked() {
            app_cmd.push(Cmd::SetEventsFilter(Filter {
                unit: Some(idx),
                event: Some(EventPayload::PanTime(PanTime::default()).discriminant()),
            }));
            app_cmd.push(Cmd::SetActiveTab(Tab::Events));
        }
        if ui.add(pan_time_slider(&mut pan_time)).changed() {
            unit.pan_time_offs = pan_time.to_lr_offsets(ins.out_sample_rate);
        }
        ui.label("l");
        ui.add(egui::DragValue::new(&mut unit.pan_time_offs[0]));
        ui.label("r");
        ui.add(egui::DragValue::new(&mut unit.pan_time_offs[1]));
        ui.end_row();
        if ui.link("volume").clicked() {
            app_cmd.push(Cmd::SetEventsFilter(Filter {
                unit: Some(idx),
                event: Some(EventPayload::Volume(0).discriminant()),
            }));
            app_cmd.push(Cmd::SetActiveTab(Tab::Events));
        }
        ui.add(egui::Slider::new(&mut unit.volume, 0..=256));
        if ui.link("velocity").clicked() {
            app_cmd.push(Cmd::SetEventsFilter(Filter {
                unit: Some(idx),
                event: Some(EventPayload::Velocity(0).discriminant()),
            }));
            app_cmd.push(Cmd::SetActiveTab(Tab::Events));
        }
        ui.add(egui::Slider::new(&mut unit.velocity, 0..=256));
        ui.end_row();
        ui.label("group");
        group_idx_slider(ui, &mut unit.group);
        ui.end_row();
        ui.label("Group history");
        ui.end_row();
        let mut any_group_ev = false;
        for (ev_idx, ev) in evelist.iter().enumerate() {
            if ev.unit == idx
                && let EventPayload::SetGroup(group_idx) = &ev.payload
            {
                any_group_ev = true;
                ui.label(ev.tick.to_string());
                let mut idx = group_idx.0;
                if ui
                    .add(egui::DragValue::new(&mut idx).range(0..=GroupIdx::MAX.0))
                    .changed()
                {
                    app_cmd.push(Cmd::OverwriteEvent {
                        idx: ev_idx,
                        payload: EventPayload::SetGroup(GroupIdx(idx)),
                    });
                }
            }
        }
        if !any_group_ev {
            if ui.button("+ Add group event at tick 0").clicked() {
                app_cmd.push(Cmd::InsertEvent {
                    idx: 0,
                    event: Event {
                        payload: EventPayload::SetGroup(GroupIdx(0)),
                        unit: idx,
                        tick: 0,
                    },
                });
            }
        }
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
        ui.label("Voice history");
        ui.end_row();
        for (ev_idx, ev) in evelist.iter().enumerate() {
            if ev.unit == idx
                && let EventPayload::SetVoice(voic) = &ev.payload
            {
                let Some(voice) = &ins.voices.get(voic.usize()) else {
                    ui.label("Invalid voice index");
                    return;
                };
                ui.menu_button(
                    (&ev.tick.to_string(), voice_img(voice), &voice.name),
                    |ui| {
                        for (i, voice) in ins.voices.iter().enumerate() {
                            if ui
                                .button((
                                    voice_img(voice).atom_size(egui::vec2(16.0, 16.0)),
                                    &voice.name,
                                ))
                                .clicked()
                            {
                                app_cmd.push(Cmd::OverwriteEvent {
                                    idx: ev_idx,
                                    payload: EventPayload::SetVoice(VoiceIdx(i as u8)),
                                });
                            }
                        }
                    },
                );
            }
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
}

pub fn handle_units_command(
    cmd: Option<UnitsCmd>,
    song: &mut SongState,
    app_modal_payload: &mut Option<ModalPayload>,
) {
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
                    for unit in &mut *song.herd.units {
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
            UnitsCmd::SeekPrevOnEvent { idx } => {
                if let Some(ev) = song.song.events[..song.herd.evt_idx]
                    .iter()
                    .rfind(|ev| ev.unit == idx && matches!(&ev.payload, EventPayload::On { .. }))
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
            UnitsCmd::MigrateUnitEvents { idx } => {
                poly_migrate_single(app_modal_payload, song, idx);
            }
        }
    }
}

pub fn unit_popup_ctx_menu(
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
    evelist: &EveList,
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
                evelist,
            )
        });
}

pub fn unit_popup_ui(
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
    evelist: &[Event],
) {
    ui.horizontal(|ui| {
        if ui.button("ｘ").clicked() {
            ui.close();
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
    });
    ui.separator();
    match tab {
        UnitPopupTab::Unit => unit_ui(ui, idx, unit, ins, cmd, app_cmd, evelist),
        UnitPopupTab::Voice => {
            if let Some(voice) = ins.voices.get_mut(unit.voice_idx.usize()) {
                let aux = aux.get_or_insert_with(|| {
                    crate::audio_out::spawn_aux_audio_thread(out_rate, 1024)
                });
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

pub fn unit_color(idx: usize) -> egui::Color32 {
    UNIT_COLORS[idx % UNIT_COLORS.len()]
}

pub fn unit_voice_img(
    ins: &ptcow::MooInstructions,
    unit: &ptcow::Unit,
) -> egui::ImageSource<'static> {
    ins.voices
        .get(unit.voice_idx.usize())
        .map_or(img::X, |voic| voice_img(voic))
}

pub fn unit_mute_unmute_all_ui(ui: &mut egui::Ui, units: &mut [ptcow::Unit]) {
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

#[derive(PartialEq)]
pub enum UnitPopupTab {
    Unit,
    Voice,
}

pub fn pan_time_slider(pan_time: &'_ mut PanTime) -> egui::Slider<'_> {
    egui::Slider::new(&mut pan_time.0, PanTime::RANGE)
}
