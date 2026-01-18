use {
    crate::{
        app::{
            FileOp, ModalPayload, SongState,
            command_queue::{Cmd, CommandQueue},
            ui::{
                Tab,
                file_ops::{FILT_MIDI, FILT_PIYOPIYO, FILT_PTCOP},
                piano_freeplay_ui,
            },
        },
        audio_out::{OutParams, prepare_song},
        pxtone_misc::poly_migrate_units,
    },
    eframe::egui::{
        self, KeyboardShortcut,
        containers::menu::{MenuButton, MenuConfig},
    },
    egui_file_dialog::DialogState,
    ptcow::{EventPayload, Unit, UnitIdx, timing::NonZeroMeas},
};

const OPEN_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::O);
const SAVE_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::S);
const RELOAD_SHORTCUT: KeyboardShortcut =
    KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::R);

pub fn top_panel(app: &mut crate::app::App, ui: &mut egui::Ui) {
    let [
        sc_open,
        sc_save,
        sc_reload,
        k_f4,
        k_f5,
        k_f6,
        k_f7,
        k_f8,
        k_f9,
        k_f10,
    ] = ui.input_mut(|inp| {
        [
            inp.consume_shortcut(&OPEN_SHORTCUT),
            inp.consume_shortcut(&SAVE_SHORTCUT),
            inp.consume_shortcut(&RELOAD_SHORTCUT),
            inp.key_pressed(egui::Key::F4),
            inp.key_pressed(egui::Key::F5),
            inp.key_pressed(egui::Key::F6),
            inp.key_pressed(egui::Key::F7),
            inp.key_pressed(egui::Key::F8),
            inp.key_pressed(egui::Key::F9),
            inp.key_pressed(egui::Key::F10),
        ]
    });
    let [mut bt_open, mut bt_reload, mut bt_save] = [false; _];
    let mut song_g = app.song.lock().unwrap();
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            bt_open = ui
                .add(
                    egui::Button::new("Open")
                        .shortcut_text(ui.ctx().format_shortcut(&OPEN_SHORTCUT)),
                )
                .clicked();
            bt_reload = ui
                .add(
                    egui::Button::new("Reload")
                        .shortcut_text(ui.ctx().format_shortcut(&RELOAD_SHORTCUT)),
                )
                .clicked();
            bt_save = ui
                .add(
                    egui::Button::new("Save")
                        .shortcut_text(ui.ctx().format_shortcut(&SAVE_SHORTCUT)),
                )
                .clicked();
            if ui.button("Save as").clicked() {
                app.file_dia.set_user_data(FileOp::SaveProjAs);
                app.file_dia.config_mut().default_save_extension = Some(FILT_PTCOP.into());
                app.file_dia.save_file();
            }
            ui.separator();
            if ui.button("Import midi").clicked() {
                app.file_dia.set_user_data(FileOp::ImportMidi);
                app.file_dia.config_mut().default_file_filter = Some(FILT_MIDI.into());
                app.file_dia.pick_file();
            }
            if ui.button("Import PiyoPiyo").clicked() {
                app.file_dia.set_user_data(FileOp::ImportPiyoPiyo);
                app.file_dia.config_mut().default_file_filter = Some(FILT_PIYOPIYO.into());
                app.file_dia.pick_file();
            }
            ui.separator();
            if ui.button("Quit").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        let song: &mut SongState = &mut song_g;
        ui.menu_button("Song", |ui| {
            ui.menu_button("Clear events", |ui| {
                if ui.button("Key and on events").clicked() {
                    song.song.events.eves.retain(|eve| {
                        !matches!(eve.payload, EventPayload::Key(_) | EventPayload::On { .. })
                    });
                }
                if ui.button("All events").clicked() {
                    song.song.events.eves.clear();
                }
            });
            ui.separator();
            if ui.button("Auto migrate overlapping events").clicked() {
                let orig_n_units: u8 = song.herd.units.len().try_into().unwrap();
                for mut migrate_from in 0..orig_n_units {
                    // Skip muted units
                    if song.herd.units[migrate_from as usize].mute {
                        continue;
                    }
                    while let migrate_to = UnitIdx(song.herd.units.len().try_into().unwrap())
                        && poly_migrate_units(UnitIdx(migrate_from), migrate_to, &mut song.song)
                    {
                        let unit = Unit {
                            name: format!("Poly{}", migrate_to.0),
                            ..Default::default()
                        };
                        song.herd.units.push(unit);
                        migrate_from = migrate_to.0;
                    }
                }
            }
        });
        let button = MenuButton::new("Timing").config(
            MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
        );
        button.ui(ui, |ui| {
            let full_w = ui.available_width();
            egui::Grid::new("timing_grid")
                .num_columns(2)
                .show(ui, |ui| {
                    timing_popup_ui(
                        &mut app.out,
                        &mut app.cmd,
                        &mut app.modal_payload,
                        song,
                        ui,
                        full_w,
                    );
                });
        });
        ui.separator();
        let mut tab = |tab, label, on| {
            ui.selectable_value(&mut app.ui_state.tab, tab, label);
            if on {
                app.ui_state.tab = tab;
            }
        };
        tab(Tab::Playback, "▶ Playback [F4]", k_f4);
        tab(Tab::Map, "📜 Map [F5]", k_f5);
        tab(Tab::PianoRoll, "🎹 Piano roll [F6]", k_f6);
        tab(Tab::Events, "󾠬 Events [F7]", k_f7);
        tab(Tab::Voices, "📢 Voices [F8]", k_f8);
        tab(Tab::Units, "🐄 Units [F9]", k_f9);
        tab(Tab::Effects, "🔊 Effects [F10]", k_f10);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(open_path) = &app.open_file {
                ui.label(format!("{}", open_path.file_name().unwrap().display()));
            }
        });
    });
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        if ui.button("⏮").clicked() {
            song_g.herd.seek_to_sample(0);
        }
        if !song_g.pause {
            if ui.button("⏹ Stop").clicked() {
                song_g.pause = true;
            }
            let label = if song_g.pause {
                "▶ Resume"
            } else {
                "⏸ Pause"
            };
            if ui.button(label).clicked() {
                song_g.pause ^= true;
            }
        } else if ui.button("▶ Play").clicked() {
            prepare_song(&mut song_g);
            song_g.pause = false;
        }
        ui.label("🔉");
        ui.add(
            egui::Slider::new(&mut song_g.master_vol, 0.0..=1.0)
                .custom_formatter(|val, _| ((val * 100.0).round() as u8).to_string())
                .custom_parser(|text| text.parse::<u8>().ok().map(|val| val as f64 / 100.0))
                .update_while_editing(false),
        );
        ui.separator();
        piano_freeplay_ui(
            &mut song_g,
            app.out.rate,
            ui,
            &mut app.ui_state.freeplay_piano,
            app.file_dia.state() == &DialogState::Open,
        );
    });
    drop(song_g);
    ui.add_space(2.0);

    if bt_open || sc_open {
        if let Some(path) = &app.open_file {
            app.file_dia.config_mut().initial_directory = path.parent().unwrap().to_path_buf();
        }
        app.file_dia.set_user_data(FileOp::OpenProj);
        app.file_dia.config_mut().default_file_filter = Some(FILT_PTCOP.into());
        app.file_dia.pick_file();
    }

    if bt_reload || sc_reload {
        app.cmd.push(Cmd::ReloadCurrentFile);
    }

    if bt_save || sc_save {
        app.cmd.push(Cmd::SaveCurrentFile);
    }
}

fn timing_popup_ui(
    app_out: &mut OutParams,
    app_cmd: &mut CommandQueue,
    app_modal_payload: &mut Option<ModalPayload>,
    song: &mut SongState,
    ui: &mut egui::Ui,
    full_w: f32,
) {
    ui.label("Ticks per beat")
        .on_hover_text("How many clock ticks happen during a beat");
    ui.add(egui::DragValue::new(&mut song.song.master.timing.ticks_per_beat).range(1..=65536));
    ui.end_row();
    ui.label("BPM").on_hover_text("Beats per minute");
    ui.add(egui::DragValue::new(&mut song.song.master.timing.bpm));
    ui.end_row();
    ui.label("Beats per meas");
    ui.add(egui::DragValue::new(&mut song.song.master.timing.beats_per_meas).range(1..=255));
    ui.end_row();
    h_sep(ui, full_w);
    ui.label("Samples per tick");
    ui.add(egui::DragValue::new(&mut song.ins.samples_per_tick).speed(0.01));
    ui.end_row();
    h_sep(ui, full_w);
    ui.label("Last meas");
    match &mut song.song.master.loop_points.last {
        Some(last) => {
            ui.add(egui::DragValue::new(last));
        }
        None => {
            if ui.button("Add").clicked() {
                song.song.master.loop_points.last = Some(NonZeroMeas::new(1).unwrap());
            }
        }
    }
    ui.end_row();
    ui.label("Repeat meas");
    ui.add(egui::DragValue::new(
        &mut song.song.master.loop_points.repeat,
    ));
    ui.end_row();
    h_sep(ui, full_w);
    ui.label("Out rate");
    let prev_out_rate = app_out.rate;
    if ui
        .add(egui::DragValue::new(&mut app_out.rate).update_while_editing(false))
        .changed()
        && app_out.rate != prev_out_rate
    {
        song.ins.out_sample_rate = app_out.rate;
        prepare_song(song);
        ptcow::rebuild_tones(
            &mut song.ins,
            app_out.rate,
            &mut song.herd.delays,
            &mut song.herd.overdrives,
            &song.song.master,
        );
        app_cmd.push(Cmd::ReplaceAudioThread);
    }
    let prev_buf_size = app_out.buf_size;
    ui.end_row();
    ui.label("Buf size");
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut app_out.buf_size).update_while_editing(false));
        ui.label(format!("{:.2}ms", app_out.latency_ms()));
    });
    if app_out.buf_size != prev_buf_size {
        app_cmd.push(Cmd::ReplaceAudioThread);
    }
    ui.end_row();
    h_sep(ui, full_w);
    ui.label("Repeat sample");
    ui.add(egui::DragValue::new(&mut song.herd.smp_repeat));
    ui.end_row();
    ui.label("End sample");
    ui.add(egui::DragValue::new(&mut song.herd.smp_end));
    ui.end_row();
    if ui.button("Seek to sample...").clicked() {
        *app_modal_payload = Some(ModalPayload::SeekToSamplePrompt(song.herd.smp_count));
    }
}

// Awkward full width horizontal separator line for grid layouts
fn h_sep(ui: &mut egui::Ui, full_w: f32) {
    let (_id, rect) = ui.allocate_space(egui::vec2(1.0, 1.0));
    ui.painter().line_segment(
        [
            rect.left_center(),
            rect.left_center() + egui::vec2(full_w, 0.0),
        ],
        egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
    );
    ui.end_row();
}
