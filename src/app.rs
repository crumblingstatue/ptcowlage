use {
    crate::{
        CliArgs,
        app::{
            command_queue::{Cmd, CommandQueue},
            ui::file_ops::{FILT_MIDI, FILT_ORGANYA, FILT_PIYOPIYO, FILT_PTCOP, FileOp},
        },
        audio_out::{
            self, AuxAudioState, OutParams, SongState, SongStateHandle, spawn_ptcow_audio_thread,
        },
        evilscript,
    },
    anyhow::Context,
    eframe::egui,
    ptcow::{Event, EventPayload, Herd, MooInstructions, SampleT, Song, UnitIdx},
    std::{
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    },
    tinyaudio::OutputDevice,
};

pub mod command_queue;
mod ui;

pub struct App {
    song: SongStateHandle,
    #[cfg(not(target_arch = "wasm32"))]
    file_dia: egui_file_dialog::FileDialog,
    /// Main audio output for ptcow playback
    pt_audio_dev: Option<OutputDevice>,
    out: OutParams,
    ui_state: ui::UiState,
    /// Currently opened file
    open_file: Option<PathBuf>,
    midi: MidiImportOpts,
    modal_payload: Option<ModalPayload>,
    pub(crate) cmd: CommandQueue,
    aux_state: AuxAudioState,
}

enum ModalPayload {
    Msg(String),
    SeekToSamplePrompt(SampleT),
}

pub struct MidiImportOpts {
    base_key: u8,
}

pub type BundledSongs = &'static [(&'static str, &'static [u8])];

impl App {
    pub fn new(args: CliArgs, bundled_songs: BundledSongs) -> Self {
        let sample_rate = 44_100;
        let mut song_state = SongState {
            herd: Herd::default(),
            song: Song::default(),
            ins: MooInstructions::new(sample_rate),
            pause: true,
            master_vol: 1.0,
        };
        let mut modal_payload = None;
        if let Some(mid_path) = args.midi_import {
            let mid_data = std::fs::read(&mid_path).unwrap();
            match mid2ptcop::write_midi_to_pxtone(
                &mid_data,
                &mut song_state.herd,
                &mut song_state.song,
                32,
            ) {
                Ok(_) => {
                    song_state.song.recalculate_length();
                }
                Err(e) => {
                    modal_payload = Some(ModalPayload::Msg(e.to_string()));
                }
            };
        }
        if let Some(path) = args.piyo_import {
            let piyo_data = std::fs::read(&path).unwrap();
            let piyo = piyopiyo::Song::load(&piyo_data).unwrap();
            crate::piyopiyo::import(
                &piyo,
                &mut song_state.herd,
                &mut song_state.song,
                &mut song_state.ins,
                sample_rate,
            );
        }
        if let Some(path) = args.org_import {
            let data = std::fs::read(&path).unwrap();
            let mut org = organyacat::Song::default();
            org.read(&data).unwrap();
            crate::organya::import(
                &org,
                &mut song_state.herd,
                &mut song_state.song,
                &mut song_state.ins,
                sample_rate,
            );
        }
        if let Some(ptcop_path) = args.voice_import {
            import_voices(&ptcop_path, &mut song_state);
        }
        let aux_state = audio_out::spawn_aux_audio_thread(sample_rate, 1024);
        // We want to be prepared to moo before we spawn the audio thread, so we can toot and stuff.
        crate::audio_out::prepare_song(&mut song_state);
        ptcow::rebuild_tones(
            &mut song_state.ins,
            sample_rate,
            &mut song_state.herd.delays,
            &mut song_state.herd.overdrives,
            &song_state.song.master,
        );
        let song_state_handle = Arc::new(Mutex::new(song_state));
        let out_params = OutParams::default();
        let mut this = Self {
            song: song_state_handle.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            file_dia: egui_file_dialog::FileDialog::new()
                .add_file_filter_extensions(FILT_PTCOP, vec!["ptcop"])
                .add_save_extension(FILT_PTCOP, "ptcop")
                .add_file_filter_extensions(FILT_MIDI, vec!["mid"])
                .add_file_filter_extensions(FILT_PIYOPIYO, vec!["pmd"])
                .add_file_filter_extensions(FILT_ORGANYA, vec!["org"]),
            #[cfg(not(target_arch = "wasm32"))]
            pt_audio_dev: Some(spawn_ptcow_audio_thread(out_params, song_state_handle)),
            #[cfg(target_arch = "wasm32")]
            pt_audio_dev: None,
            out: out_params,
            ui_state: ui::UiState::default(),
            open_file: None,
            midi: MidiImportOpts { base_key: 32 },
            modal_payload,
            cmd: CommandQueue::default(),
            aux_state,
        };
        if let Some(path) = args.open {
            if let Err(e) = this.load_song_from_path(path) {
                this.modal_payload =
                    Some(ModalPayload::Msg(format!("Error loading project:\n{e}")));
            }
        } else if let Some(song) = bundled_songs.first() {
            // Load a bundled song if no song was requested to open
            if let Err(e) = this.load_song_from_bytes(song.1) {
                this.modal_payload =
                    Some(ModalPayload::Msg(format!("Error loading project:\n{e}")));
            } else {
                this.open_file = Some(song.0.into())
            }
        }
        // Do some EvilScript on the final state before running the app
        if let Some(evil) = args.evil
            && let Ok(cmd) = evilscript::parse(&evil)
        {
            evilscript::exec(cmd, &mut this.song.lock().unwrap());
        }
        this
    }

    fn import_midi_from_bytes(&mut self, mid_data: &[u8]) {
        let mut song = self.song.lock().unwrap();
        let song = &mut *song;
        match mid2ptcop::write_midi_to_pxtone(
            mid_data,
            &mut song.herd,
            &mut song.song,
            self.midi.base_key,
        ) {
            Ok(_) => {
                song.song.recalculate_length();
            }
            Err(e) => {
                self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
            }
        }
    }

    fn import_piyopiyo_from_bytes(&mut self, data: &[u8]) {
        let piyo = piyopiyo::Song::load(data).unwrap();
        let mut song = self.song.lock().unwrap();
        let song = &mut *song;
        crate::piyopiyo::import(
            &piyo,
            &mut song.herd,
            &mut song.song,
            &mut song.ins,
            self.out.rate,
        );
    }

    fn import_organya_from_bytes(&mut self, data: &[u8]) {
        let mut org = organyacat::Song::default();
        org.read(data).unwrap();
        let mut song = self.song.lock().unwrap();
        let song = &mut *song;
        crate::organya::import(
            &org,
            &mut song.herd,
            &mut song.song,
            &mut song.ins,
            self.out.rate,
        );
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_file_dia_update(&mut self, ctx: &egui::Context) -> (Option<PathBuf>, Option<FileOp>) {
        match self.file_dia.user_data::<FileOp>() {
            Some(FileOp::ImportMidi) => {
                self.file_dia
                    .update_with_right_panel_ui(ctx, &mut |ui, _dia| {
                        ui.heading("Midi import");
                        ui.label("Base key");
                        ui.add(egui::DragValue::new(&mut self.midi.base_key));
                    });
            }
            _ => {
                self.file_dia.update(ctx);
            }
        }

        let picked_path = self.file_dia.take_picked();
        let file_op = self.file_dia.user_data::<FileOp>().copied();
        (picked_path, file_op)
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| ui::top_panel::top_panel(self, ui));
        if self.ui_state.show_left_panel() {
            egui::SidePanel::left("left_panel").show(ctx, |ui| ui::left_panel::ui(self, ui));
        }
        egui::CentralPanel::default().show(ctx, |ui| ui::central_panel(self, ui));

        #[cfg(not(target_arch = "wasm32"))]
        let (mut picked_path, mut file_op) = self.handle_file_dia_update(ctx);
        #[cfg(target_arch = "wasm32")]
        let (mut picked_path, mut file_op) = (None, None);

        ctx.input(|inp| {
            for dropfile in &inp.raw.dropped_files {
                if let Some(path) = &dropfile.path {
                    picked_path = Some(path.clone());
                    if let Some(ext) = path.extension().map(|ext| ext.to_str().unwrap()) {
                        file_op = match ext {
                            "ptcop" | "pttune" => Some(FileOp::OpenProj),
                            "mid" | "midi" => Some(FileOp::ImportMidi),
                            "pmd" => Some(FileOp::ImportPiyoPiyo),
                            "org" => Some(FileOp::ImportOrganya),
                            _ => None,
                        };
                    }
                } else if let Some(bytes) = &dropfile.bytes {
                    if let Some((name, ext)) = dropfile.name.split_once(".") {
                        match ext {
                            "ptcop" | "pttune" => {
                                // Web version loads dropped files directly as bytes
                                if let Err(e) = self.load_song_from_bytes(bytes) {
                                    self.modal_payload = Some(ModalPayload::Msg(format!(
                                        "Error loading project:\n{e}"
                                    )));
                                }
                            }
                            "mid" => {
                                self.import_midi_from_bytes(bytes);
                            }
                            "pmd" => {
                                self.import_piyopiyo_from_bytes(bytes);
                            }
                            "org" => {
                                self.import_organya_from_bytes(bytes);
                            }
                            _ => {}
                        }
                        self.open_file = Some(format!("{name}.{ext}").into());
                    }
                }
            }
        });

        if let Some(path) = picked_path
            && let Some(op) = file_op
        {
            match op {
                FileOp::OpenProj => {
                    if let Err(e) = self.load_song_from_path(path) {
                        self.modal_payload =
                            Some(ModalPayload::Msg(format!("Error loading project:\n{e}")));
                    }
                }
                FileOp::ImportVoices => {
                    let mut song = self.song.lock().unwrap();
                    import_voices(&path, &mut song);
                }
                FileOp::ImportMidi => {
                    let mid_data = std::fs::read(&path).unwrap();
                    self.import_midi_from_bytes(&mid_data);
                }
                FileOp::ImportPiyoPiyo => {
                    let data = std::fs::read(&path).unwrap();
                    self.import_piyopiyo_from_bytes(&data);
                }
                FileOp::ImportOrganya => {
                    let data = std::fs::read(&path).unwrap();
                    self.import_organya_from_bytes(&data);
                }
                FileOp::SaveProjAs => {
                    let song = self.song.lock().unwrap();
                    match ptcow::serialize_project(&song.song, &song.herd, &song.ins) {
                        Ok(bytes) => {
                            std::fs::write(&path, bytes).unwrap();
                            self.open_file = Some(path);
                        }
                        Err(e) => {
                            self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
                        }
                    }
                    drop(song);
                }
            }
        }
        if let Some(payload) = &mut self.modal_payload {
            let mut close = false;
            egui::Modal::new("modal_popup".into()).show(ctx, |ui| match payload {
                ModalPayload::Msg(msg) => {
                    ui.label(&*msg);
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                }
                ModalPayload::SeekToSamplePrompt(samp) => {
                    ui.heading("Seek to sample");
                    ui.add(egui::DragValue::new(samp));
                    if ui.button("Seek").clicked() {
                        self.song.lock().unwrap().herd.seek_to_sample(*samp);
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                }
            });
            if close {
                self.modal_payload = None;
            }
        }
        while let Some(cmd) = self.cmd.pop() {
            self.do_cmd(cmd);
        }
    }
}

fn import_voices(path: &Path, song: &mut SongState) {
    let data = std::fs::read(path).unwrap();
    let (_, _, ins) = ptcow::read_song(&data, 44_100).unwrap();

    song.ins.voices = ins.voices;
}

impl App {
    // INVARIANT: Locks the song
    pub fn load_song_from_path(&mut self, path: PathBuf) -> anyhow::Result<()> {
        let data = std::fs::read(&path).context("Failed to read file")?;
        self.load_song_from_bytes(&data)?;
        self.open_file = Some(path);
        Ok(())
    }
    // INVARIANT: Locks the song
    pub fn load_song_from_bytes(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let (song, herd, ins) = ptcow::read_song(data, self.out.rate)?;
        let mut song_g = self.song.lock().unwrap();
        let song_ref = &mut *song_g;
        song_ref.song = song;
        song_ref.herd = herd;
        song_ref.ins = ins;
        // We want to be prepared to moo before we spawn the audio thread, so we can toot and stuff.
        crate::audio_out::prepare_song(song_ref);
        ptcow::rebuild_tones(
            &mut song_ref.ins,
            self.out.rate,
            &mut song_ref.herd.delays,
            &mut song_ref.herd.overdrives,
            &song_ref.song.master,
        );
        // Set a default toot unit if units aren't empty
        let has_units = !song_ref.herd.units.is_empty();
        if has_units {
            self.ui_state.freeplay_piano.toot = Some(UnitIdx(0));
            // Set initial voices, etc.
            do_tick0_events(song_ref);
        }
        Ok(())
    }
    /// INVARIANT: Call this outside of any critical section, because it locks the song handle
    fn do_cmd(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::ReloadCurrentFile => self.reload_current_file(),
            Cmd::SaveCurrentFile => self.save_current_file(),
            Cmd::OpenEventInEventsTab { index } => {
                self.ui_state.tab = ui::Tab::Events;
                self.ui_state.raw_events.go_to = Some(index);
            }
            Cmd::RemoveNoteAtIdx { idx } => {
                let mut song = self.song.lock().unwrap();
                let eves = &mut song.song.events.eves;
                let target_ev = eves[idx];
                // Remove this event and all key events for this unit on the same tick
                let coll = |eve: &Event| {
                    eve.unit == target_ev.unit && matches!(eve.payload, EventPayload::Key(_))
                };
                let cont = |eve: &Event| eve.tick == target_ev.tick;
                let mut indices = crate::util::domain_expansion(eves, idx, cont, coll);
                indices.sort();
                // Remove indices in reverse order to not invalidate indices
                for idx in indices.iter().rev() {
                    eves.remove(*idx);
                }
            }
            Cmd::ReplaceAudioThread => {
                Self::replace_pt_audio_thread(&mut self.pt_audio_dev, self.out, self.song.clone());
            }
        }
    }
    fn reload_current_file(&mut self) {
        if let Some(path) = &self.open_file
            && let Err(e) = self.load_song_from_path(path.clone())
        {
            self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
        }
    }
    fn save_current_file(&self) {
        if let Some(path) = &self.open_file {
            let song = self.song.lock().unwrap();
            let serialized = ptcow::serialize_project(&song.song, &song.herd, &song.ins).unwrap();
            std::fs::write(path, serialized).unwrap();
        }
    }
    /// Replace already running ptcow audio thread with a new one
    ///
    /// IMPORTANT: This should be called *OUTSIDE* of any critical section involving the song state handle
    /// Otherwise, a deadlock can happen.
    ///
    /// You should probably be sending [crate::app::command_queue::Cmd::ReplaceAudioThread] instead.
    fn replace_pt_audio_thread(
        app_pt_audio_dev: &mut Option<OutputDevice>,
        app_out: OutParams,
        app_song: SongStateHandle,
    ) {
        // Drop the old handle, so the thread can join, and we avoid a deadlock.
        *app_pt_audio_dev = None;
        // Now we can spawn the new thread
        *app_pt_audio_dev = Some(spawn_ptcow_audio_thread(app_out, app_song))
    }
}

// Apply things like setting initial voices for units on tick 0
fn do_tick0_events(song: &mut SongState) {
    for ev in song.song.events.eves.iter().take_while(|ev| ev.tick == 0) {
        let Some(unit) = song.herd.units.get_mut(ev.unit.usize()) else {
            continue;
        };
        match ev.payload {
            EventPayload::Velocity(vel) => unit.velocity = vel,
            EventPayload::Volume(vol) => unit.volume = vol,
            EventPayload::SetVoice(idx) => {
                unit.reset_voice(&song.ins, idx, song.song.master.timing)
            }
            EventPayload::SetGroup(group_idx) => unit.group = group_idx,
            _ => {}
        }
    }
}
