use {
    crate::{
        app::{
            ModalPayload, SongState,
            command_queue::{Cmd, CommandQueue},
            poly_migrate_single,
            ui::{Tab, piano_freeplay_ui},
        },
        audio_out::{OutParams, prepare_song},
    },
    eframe::egui::{
        self, KeyboardShortcut,
        containers::menu::{MenuButton, MenuConfig},
    },
    ptcow::{EveList, EventPayload, MooPlan, UnitIdx, VoiceIdx, timing::NonZeroMeas},
    std::collections::{HashMap, HashSet},
};

const OPEN_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::O);
const SAVE_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::S);
const RELOAD_SHORTCUT: KeyboardShortcut =
    KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::R);

fn used_voices(eves: &EveList) -> HashSet<VoiceIdx> {
    let mut used = HashSet::new();
    for eve in eves.iter() {
        if let EventPayload::SetVoice(idx) = eve.payload {
            used.insert(idx);
        }
    }
    used
}

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
            file_menu_ui(
                ui,
                &mut bt_open,
                &mut bt_reload,
                &mut bt_save,
                app.open_file.is_some(),
                &mut app.cmd,
            );
        });
        ui.menu_button("View", |ui| {
            egui::gui_zoom::zoom_menu_buttons(ui);
        });
        let song: &mut SongState = &mut song_g;
        ui.menu_button("Song", |ui| {
            ui.menu_button("Clear events", |ui| {
                if ui.button("Key and on events").clicked() {
                    song.song.events.retain(|eve| {
                        !matches!(eve.payload, EventPayload::Key(_) | EventPayload::On { .. })
                    });
                }
                if ui.button("All events").clicked() {
                    song.song.events.clear();
                }
            });
            if ui.button("Remove unused voices").clicked() {
                let used_voices = used_voices(&song.song.events);
                let mut idx = VoiceIdx(0);
                let mut new_idx = VoiceIdx(0);
                let mut index_map = HashMap::new();
                song.ins.voices.retain(|_| {
                    let retain = used_voices.contains(&idx);
                    if retain {
                        index_map.insert(idx, new_idx);
                        new_idx.0 += 1;
                    }
                    idx.0 += 1;
                    retain
                });
                for eve in song.song.events.iter_mut() {
                    if let EventPayload::SetVoice(idx) = &mut eve.payload {
                        *idx = index_map[idx];
                    }
                }
                for unit in song.herd.units.iter_mut() {
                    unit.voice_idx = index_map[&unit.voice_idx];
                }
            }
            ui.separator();
            if ui.button("Auto migrate overlapping events").clicked() {
                let orig_n_units: u8 = song.herd.units.len().try_into().unwrap();
                for mut migrate_from in (0..orig_n_units).map(UnitIdx) {
                    // Skip muted units
                    if song.herd.units[migrate_from.usize()].mute {
                        continue;
                    }
                    while let Some(out) =
                        poly_migrate_single(&mut app.modal_payload, song, migrate_from)
                    {
                        migrate_from = out;
                    }
                }
                // Doesn't seem to sound right until we restart the song
                crate::app::post_load_prep(
                    song,
                    app.out.rate,
                    &mut app.ui_state.freeplay_piano.toot,
                );
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
        ui.menu_button("Help", |ui| {
            ui.menu_button("About", |ui| {
                ui.label(concat!(
                    "🐄 Pxtone Cowlage version ",
                    env!("CARGO_PKG_VERSION")
                ));
                ui.hyperlink_to(" Github", "https://github.com/crumblingstatue/ptcowlage");
                ui.separator();
                ui.label("Community");
                ui.hyperlink_to("▶ pxtone web", "https://ptweb.me/");
                ui.hyperlink_to("🐷 Discord", "https://discord.gg/2uQjHu8");
            });
        });
        ui.separator();
        let mut tab = |tab, label, fkey: &str, on| {
            ui.selectable_value(
                &mut app.ui_state.tab,
                tab,
                (
                    label,
                    egui::RichText::new(fkey)
                        .size(11.0)
                        .color(egui::Color32::WHITE)
                        .background_color(egui::Color32::DARK_GRAY),
                ),
            );
            if on {
                app.ui_state.tab = tab;
            }
        };
        tab(Tab::Playback, "▶ Playback", "F4", k_f4);
        tab(Tab::Map, "📜 Map", "F5", k_f5);
        tab(Tab::PianoRoll, "🎹 Piano roll", "F6", k_f6);
        tab(Tab::Events, "󾠬 Events", "F7", k_f7);
        tab(Tab::Voices, "📢 Voices", "F8", k_f8);
        tab(Tab::Unit, "🐄 Unit", "F9", k_f9);
        tab(Tab::Effects, "🔊 Effects", "F10", k_f10);
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
        if ui.button("⏹ Stop").clicked() {
            prepare_song(&mut song_g, true);
            song_g.pause = true;
        }
        if !song_g.pause {
            if ui.button("⏸ Pause").clicked() {
                song_g.pause = true;
            }
        } else if ui.button("▶ Play").clicked() {
            song_g.herd.moo_end = false;
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
            #[cfg(not(target_arch = "wasm32"))]
            (app.file_dia.state() == &egui_file_dialog::DialogState::Open),
            #[cfg(target_arch = "wasm32")]
            false,
        );
    });
    drop(song_g);
    ui.add_space(2.0);

    if bt_open || sc_open {
        app.cmd.push(Cmd::PromptOpenPtcop);
    }

    if bt_reload || sc_reload {
        app.cmd.push(Cmd::ReloadCurrentFile);
    }

    if bt_save || sc_save {
        app.cmd.push(Cmd::SaveCurrentFile);
    }

    if app.pt_audio_dev.is_none() {
        ui.horizontal(|ui| {
            ui.colored_label(egui::Color32::RED, "⚠ Audio thread is not running.");
            if ui.button("Restart audio thread").clicked() {
                app.cmd.push(Cmd::ReplaceAudioThread);
            }
        });
    }
}

fn file_menu_ui(
    ui: &mut egui::Ui,
    bt_open: &mut bool,
    bt_reload: &mut bool,
    bt_save: &mut bool,
    can_save: bool,
    app_cmd: &mut CommandQueue,
) {
    *bt_open = ui
        .add(egui::Button::new("Open").shortcut_text(ui.ctx().format_shortcut(&OPEN_SHORTCUT)))
        .clicked();
    if cfg!(not(target_arch = "wasm32")) {
        *bt_reload = ui
            .add(
                egui::Button::new("Reload")
                    .shortcut_text(ui.ctx().format_shortcut(&RELOAD_SHORTCUT)),
            )
            .clicked();
        *bt_save = ui
            .add_enabled(
                can_save,
                egui::Button::new("Save").shortcut_text(ui.ctx().format_shortcut(&SAVE_SHORTCUT)),
            )
            .clicked();
    }
    if ui.button("Save as").clicked() {
        app_cmd.push(Cmd::PromptSaveAs);
    }
    ui.separator();
    if ui.button("Import midi").clicked() {
        app_cmd.push(Cmd::PromptImportMidi);
    }
    if ui.button("Import PiyoPiyo").clicked() {
        app_cmd.push(Cmd::PromptImportPiyo);
    }
    if ui.button("Import Organya").clicked() {
        app_cmd.push(Cmd::PromptImportOrg);
    }
    ui.separator();
    if ui.button("Export wav").clicked() {
        app_cmd.push(Cmd::PromptExportWav);
    }
    ui.separator();
    if cfg!(not(target_arch = "wasm32")) {
        if ui.button("Quit").clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
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
    let mut timing_changed = false;

    ui.label("BPM").on_hover_text("Beats per minute");
    timing_changed ^= ui
        .add(
            egui::DragValue::new(&mut song.song.master.timing.bpm)
                .range(1.0..=99_999.0)
                .update_while_editing(false),
        )
        .changed();
    ui.end_row();
    ui.label("Ticks per beat")
        .on_hover_text("How many clock ticks happen during a beat");
    timing_changed ^= ui
        .add(
            egui::DragValue::new(&mut song.song.master.timing.ticks_per_beat)
                .range(1..=65536)
                .update_while_editing(false),
        )
        .changed();
    // Let ptcow reconfigure the timing after we changed the timing parameters
    if timing_changed {
        let last_played_sample = song.herd.smp_count;
        ptcow::moo_prepare(
            &mut song.ins,
            &mut song.herd,
            &song.song,
            &MooPlan {
                start_pos: ptcow::StartPosPlan::Sample(last_played_sample),
                meas_end: None,
                meas_repeat: None,
                loop_: true,
            },
        );
    }
    ui.end_row();
    h_sep(ui, full_w);
    ui.label("Samples per tick");
    ui.add(egui::DragValue::new(&mut song.ins.samples_per_tick).speed(0.01));
    ui.end_row();
    h_sep(ui, full_w);
    ui.label("Beats per meas");
    ui.add(
        egui::DragValue::new(&mut song.song.master.timing.beats_per_meas)
            .range(1..=255)
            .update_while_editing(false),
    );
    ui.end_row();
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
        .add(
            egui::DragValue::new(&mut app_out.rate)
                .range(OutParams::SANE_RATE_RANGE)
                .update_while_editing(false)
                .suffix(" Hz"),
        )
        .changed()
        && app_out.rate != prev_out_rate
    {
        song.ins.out_sample_rate = app_out.rate;
        prepare_song(song, true);
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
        ui.add(
            egui::DragValue::new(&mut app_out.buf_size)
                .range(OutParams::SANE_BUF_SIZE_RANGE)
                .update_while_editing(false),
        );
        ui.label(format!("{:.2}ms", app_out.latency_ms()))
            .on_hover_text("Latency");
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
