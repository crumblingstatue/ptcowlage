#[cfg(not(target_arch = "wasm32"))]
use {
    crate::app::ui::file_ops::{FILT_PTNOISE, FILT_PTVOICE, FILT_WAV},
    recently_used_list::RecentlyUsedList,
};
use {
    crate::{
        CliArgs,
        app::{
            command_queue::{Cmd, CommandQueue},
            ui::{
                Tab,
                file_ops::{
                    self, FILT_MIDI, FILT_ORGANYA, FILT_PIYOPIYO, FILT_PTCOP, FILT_SF2, FileFilt,
                    FileOp,
                },
            },
        },
        audio_out::{
            AuxAudioState, OutParams, SongState, SongStateHandle, spawn_ptcow_audio_thread,
        },
        evilscript,
        pxtone_misc::{poly_migrate_units, reset_voice_for_units_with_voice_idx},
    },
    anyhow::Context,
    eframe::egui,
    egui_toast::{Toast, ToastKind, ToastOptions},
    ptcow::{Event, EventPayload, NoiseTable, SampleRate, SampleT, UnitIdx, VoiceIdx},
    rustysynth::SoundFont,
    std::{
        fs::File,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    },
    tinyaudio::OutputDevice,
};

pub mod command_queue;
pub mod ui;

pub struct App {
    song: SongStateHandle,
    #[cfg(not(target_arch = "wasm32"))]
    pub file_dia: egui_file_dialog::FileDialog,
    #[cfg(not(target_arch = "wasm32"))]
    pub recently_opened: RecentlyUsedList<PathBuf>,
    /// Main audio output for ptcow playback
    pt_audio_dev: Option<OutputDevice>,
    pub out: OutParams,
    ui_state: ui::UiState,
    /// Currently opened file
    open_file: Option<PathBuf>,
    modal_payload: Option<ModalPayload>,
    pub(crate) cmd: CommandQueue,
    /// Auxiliary audio output (for example playing voice samples in voice UI)
    ///
    /// Spawned on-demand, mainly due to web needing interaction before spawning audio context.
    aux_state: Option<AuxAudioState>,
    #[cfg(target_arch = "wasm32")]
    web_cmd: crate::web_glue::WebCmdQueueHandle,
}

pub enum ModalPayload {
    Msg(String),
    SeekToSamplePrompt(SampleT),
}

pub type BundledSongs = &'static [(&'static str, &'static [u8])];

impl App {
    pub fn new(args: CliArgs, out_params: OutParams, bundled_songs: BundledSongs) -> Self {
        let sample_rate = 44_100;
        let mut song_state = SongState::new(sample_rate);
        let mut modal_payload = None;
        if let Some(mid_path) = args.midi_import {
            let mid_data = std::fs::read(&mid_path).unwrap();
            match mid2ptcop::write_midi_to_pxtone(
                &mid_data,
                &mut song_state.herd,
                &mut song_state.song,
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
        song_state.prepare(sample_rate);
        let song_state_handle = Arc::new(Mutex::new(song_state));
        let mut this = Self {
            song: song_state_handle.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            file_dia: egui_file_dialog::FileDialog::new()
                .add_file_filter_extensions(FILT_PTCOP.name, vec![FILT_PTCOP.ext])
                .add_save_extension(FILT_PTCOP.name, FILT_PTCOP.ext)
                .add_save_extension(FILT_WAV.name, FILT_WAV.ext)
                .add_file_filter_extensions(FILT_MIDI.name, vec![FILT_MIDI.ext])
                .add_file_filter_extensions(FILT_PIYOPIYO.name, vec![FILT_PIYOPIYO.ext])
                .add_file_filter_extensions(FILT_ORGANYA.name, vec![FILT_ORGANYA.ext])
                .add_file_filter_extensions(FILT_SF2.name, vec![FILT_SF2.ext])
                .add_file_filter_extensions(FILT_PTVOICE.name, vec![FILT_PTVOICE.ext])
                .add_file_filter_extensions(FILT_PTNOISE.name, vec![FILT_PTNOISE.ext]),
            #[cfg(not(target_arch = "wasm32"))]
            recently_opened: RecentlyUsedList::default(),
            #[cfg(not(target_arch = "wasm32"))]
            pt_audio_dev: Some(spawn_ptcow_audio_thread(out_params, song_state_handle)),
            #[cfg(target_arch = "wasm32")]
            pt_audio_dev: None,
            out: out_params,
            ui_state: ui::UiState::default(),
            open_file: None,
            modal_payload,
            cmd: CommandQueue::default(),
            aux_state: None,
            #[cfg(target_arch = "wasm32")]
            web_cmd: Default::default(),
        };
        // SongState comes with a default unit by... default, so let's toot that
        this.ui_state.freeplay_piano.toot = Some(UnitIdx(0));
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
        match mid2ptcop::write_midi_to_pxtone(mid_data, &mut song.herd, &mut song.song) {
            Ok(_) => {
                song.song.recalculate_length();
            }
            Err(e) => {
                self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
            }
        }
        post_load_prep(song, self.out.rate, &mut self.ui_state.freeplay_piano.toot);
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
        post_load_prep(song, self.out.rate, &mut self.ui_state.freeplay_piano.toot);
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
        post_load_prep(song, self.out.rate, &mut self.ui_state.freeplay_piano.toot);
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_file_dia_update(&mut self, ctx: &egui::Context) -> (Option<PathBuf>, Option<FileOp>) {
        self.file_dia.update(ctx);
        let picked_path = self.file_dia.take_picked();
        let file_op = self.file_dia.user_data::<FileOp>().copied();
        (picked_path, file_op)
    }

    fn open_file_prompt(&mut self, filt: FileFilt, file_op: FileOp, save: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if file_op == FileOp::OpenProj
                && let Some(path) = &self.open_file
            {
                self.file_dia.config_mut().initial_directory = path.parent().unwrap().to_path_buf();
            }
            self.file_dia.set_user_data(file_op);
            if save {
                self.file_dia.config_mut().default_save_extension = Some(filt.name.into());
                self.file_dia.save_file();
            } else {
                self.file_dia.config_mut().default_file_filter = Some(filt.name.into());
                self.file_dia.pick_file();
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            use crate::web_glue::WebCmdQueueHandleExt;
            let web_cmd = self.web_cmd.clone();
            if save {
                let (data, filename): (Vec<u8>, &str) = match file_op {
                    FileOp::SaveProjAs => {
                        let song = self.song.lock().unwrap();
                        let data =
                            ptcow::serialize_project(&song.song, &song.herd, &song.ins).unwrap();
                        (data, "out.ptcop")
                    }
                    FileOp::ExportWav => {
                        use crate::audio_out::prepare_song;

                        let mut song = self.song.lock().unwrap();
                        // Kill audio thread
                        self.pt_audio_dev = None;
                        let wav = match crate::util::export_wav(&mut song) {
                            Ok(wav) => wav,
                            Err(e) => {
                                self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
                                return;
                            }
                        };
                        // Now we can resume playback
                        prepare_song(&mut song, true);
                        song.herd.moo_end = false;
                        self.cmd.push(Cmd::ReplaceAudioThread);
                        (wav, "out.wav")
                    }
                    _ => return,
                };
                crate::web_glue::save_file(&data, filename);
            } else {
                wasm_bindgen_futures::spawn_local(async move {
                    let file = crate::web_glue::open_file(&filt.web_filter()).await;
                    web_cmd.push(crate::web_glue::WebCmd::from_file_op(
                        file_op, file.data, file.name,
                    ));
                });
            }
        }
    }

    fn import_ptvoice(&mut self, data: Vec<u8>, path: &Path) {
        match load_and_recalc_voice(data, path, just_load_ptvoice, self.out.rate) {
            Ok(voice) => {
                let mut song = self.song.lock().unwrap();
                song.ins.voices.push(voice);
                let idx = VoiceIdx(song.ins.voices.len() as u8 - 1);
                reset_voice_for_units_with_voice_idx(&mut song, idx);
                self.ui_state.voices.selected_idx = idx;
            }
            Err(e) => self.modal_payload = Some(ModalPayload::Msg(e.to_string())),
        }
    }

    fn import_ptnoise(&mut self, data: Vec<u8>, path: &Path) {
        match load_and_recalc_voice(data, path, just_load_ptnoise, self.out.rate) {
            Ok(voice) => {
                let mut song = self.song.lock().unwrap();
                song.ins.voices.push(voice);
                let idx = VoiceIdx(song.ins.voices.len() as u8 - 1);
                reset_voice_for_units_with_voice_idx(&mut song, idx);
                self.ui_state.voices.selected_idx = idx;
            }
            Err(e) => self.modal_payload = Some(ModalPayload::Msg(e.to_string())),
        }
    }
}

fn load_and_recalc_voice(
    data: Vec<u8>,
    path: &Path,
    loadfn: fn(Vec<u8>, &Path) -> ptcow::ReadResult<ptcow::Voice>,
    out_rate: SampleRate,
) -> ptcow::ReadResult<ptcow::Voice> {
    let mut voice = loadfn(data, path)?;
    let noise_tbl = NoiseTable::generate();
    voice.recalculate(&noise_tbl, out_rate);
    Ok(voice)
}

fn just_load_ptvoice(data: Vec<u8>, path: &Path) -> ptcow::ReadResult<ptcow::Voice> {
    let mut voice = ptcow::Voice::from_ptvoice(&data)?;
    if let Some(os_str) = path.file_stem() {
        voice.name = os_str.to_string_lossy().into_owned();
    }
    Ok(voice)
}

fn just_load_ptnoise(data: Vec<u8>, path: &Path) -> ptcow::ReadResult<ptcow::Voice> {
    let noise = ptcow::NoiseData::from_ptnoise(&data)?;
    let mut voice = ptcow::Voice::default();
    voice.allocate::<false>();
    voice.units[0].data = ptcow::VoiceData::Noise(noise);
    if let Some(os_str) = path.file_stem() {
        voice.name = os_str.to_string_lossy().into_owned();
    }
    Ok(voice)
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
                FileOp::ReplaceVoicesPtcop => {
                    let mut song = self.song.lock().unwrap();
                    import_voices(&path, &mut song);
                }
                FileOp::ReplacePtVoiceSingle(voice_idx) => {
                    let data = std::fs::read(&path).unwrap();
                    match load_and_recalc_voice(data, &path, just_load_ptvoice, self.out.rate) {
                        Ok(voice) => {
                            let mut song = self.song.lock().unwrap();
                            if let Some(voice_of_idx) = song.ins.voices.get_mut(voice_idx.usize()) {
                                *voice_of_idx = voice;
                            } else {
                                song.ins.voices.push(voice);
                            }
                            reset_voice_for_units_with_voice_idx(&mut song, voice_idx);
                        }
                        Err(e) => {
                            self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
                        }
                    }
                }
                FileOp::ReplacePtNoiseSingle(voice_idx) => {
                    let data = std::fs::read(&path).unwrap();
                    match load_and_recalc_voice(data, &path, just_load_ptnoise, self.out.rate) {
                        Ok(voice) => {
                            let mut song = self.song.lock().unwrap();
                            if let Some(voice_of_idx) = song.ins.voices.get_mut(voice_idx.usize()) {
                                *voice_of_idx = voice;
                            } else {
                                song.ins.voices.push(voice);
                            }
                            reset_voice_for_units_with_voice_idx(&mut song, voice_idx);
                        }
                        Err(e) => {
                            self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
                        }
                    }
                }
                FileOp::ReplaceSf2Single(voice_idx) => {
                    let mut sf2_file = File::open(path).unwrap();
                    match SoundFont::new(&mut sf2_file) {
                        Ok(soundfont) => {
                            self.ui_state.sf2_import =
                                Some(ui::Sf2ImportDialog::new(soundfont, Some(voice_idx)));
                        }
                        Err(e) => {
                            self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
                        }
                    }
                }
                FileOp::ImportSf2Single => {
                    let mut sf2_file = File::open(path).unwrap();
                    match SoundFont::new(&mut sf2_file) {
                        Ok(soundfont) => {
                            self.ui_state.sf2_import =
                                Some(ui::Sf2ImportDialog::new(soundfont, None));
                        }
                        Err(e) => {
                            self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
                        }
                    }
                }
                FileOp::ImportPtVoice => {
                    let data = std::fs::read(&path).unwrap();
                    self.import_ptvoice(data, &path);
                }
                FileOp::ImportPtNoise => {
                    let data = std::fs::read(&path).unwrap();
                    self.import_ptnoise(data, &path);
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
                FileOp::ExportWav => {
                    // Disable audio device for export duration
                    self.pt_audio_dev = None;
                    let mut song = self.song.lock().unwrap();
                    match crate::util::export_wav(&mut song) {
                        Ok(data) => {
                            if let Err(e) = std::fs::write(path, data) {
                                self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
                            }
                        }
                        Err(e) => {
                            self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
                        }
                    }
                    // We can restart audio thread now
                    self.cmd.push(Cmd::ReplaceAudioThread);
                    post_load_prep(
                        &mut song,
                        self.out.rate,
                        &mut self.ui_state.freeplay_piano.toot,
                    );
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
        if let Some(sf2) = &mut self.ui_state.sf2_import {
            let mut close = false;
            egui::Modal::new("sf2_import_popup".into()).show(ctx, |ui| {
                close = ui::sf2_import_ui(ui, sf2, &mut self.aux_state, &self.song, self.out.rate);
            });
            if close {
                self.ui_state.sf2_import = None;
            }
        }
        self.ui_state.shared.toasts.show(ctx);
        // Do queue commands
        while let Some(cmd) = self.cmd.pop() {
            self.do_cmd(cmd);
        }
        // Do web commands as well
        #[cfg(target_arch = "wasm32")]
        {
            loop {
                let cmd = self.web_cmd.borrow_mut().pop();
                match cmd {
                    Some(cmd) => self.do_web_cmd(cmd),
                    None => break,
                }
            }
        }
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            eframe::set_value(
                storage,
                "pinned-folders",
                &self.file_dia.storage_mut().pinned_folders,
            );
            eframe::set_value(storage, "recently-opened", &self.recently_opened);
        }
        storage.set_string("out-buf-size", self.out.buf_size.to_string());
    }
    fn persist_egui_memory(&self) -> bool {
        false
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
        #[cfg(not(target_arch = "wasm32"))]
        self.recently_opened.use_(path.clone());
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
        post_load_prep(
            song_ref,
            self.out.rate,
            &mut self.ui_state.freeplay_piano.toot,
        );
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
                let eves = &mut song.song.events;
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
            Cmd::OpenVoice(idx) => {
                self.ui_state.tab = Tab::Voices;
                self.ui_state.voices.selected_idx = idx;
            }
            Cmd::OverwriteEvent { idx, payload } => {
                let mut song = self.song.lock().unwrap();
                let eves = &mut song.song.events;
                eves[idx].payload = payload;
            }
            Cmd::InsertEvent { idx, event } => {
                let mut song = self.song.lock().unwrap();
                let eves = &mut song.song.events;
                eves.insert(idx, event);
            }
            Cmd::SetActiveTab(tab) => {
                self.ui_state.tab = tab;
            }
            Cmd::SetEventsFilter(filter) => {
                self.ui_state.raw_events.filter = filter;
                self.ui_state.raw_events.filter_needs_recalc = true;
            }
            Cmd::Toast {
                kind,
                text,
                duration,
            } => {
                self.ui_state.shared.toasts.add(
                    Toast::new()
                        .kind(kind)
                        .text(text)
                        .options(ToastOptions::default().duration_in_seconds(duration)),
                );
            }
            Cmd::PromptImportPtVoice => {
                self.open_file_prompt(file_ops::FILT_PTVOICE, FileOp::ImportPtVoice, false);
            }
            Cmd::PromptImportPtNoise => {
                self.open_file_prompt(file_ops::FILT_PTNOISE, FileOp::ImportPtNoise, false);
            }
            Cmd::PromptImportSf2Sound => {
                self.open_file_prompt(file_ops::FILT_SF2, FileOp::ImportSf2Single, false);
            }
            Cmd::PromptReplaceAllPtcop => {
                self.open_file_prompt(file_ops::FILT_PTCOP, FileOp::ReplaceVoicesPtcop, false)
            }
            Cmd::PromptReplacePtVoiceSingle(voice_idx) => {
                self.open_file_prompt(
                    file_ops::FILT_PTVOICE,
                    FileOp::ReplacePtVoiceSingle(voice_idx),
                    false,
                );
            }
            Cmd::PromptReplacePtNoiseSingle(voice_idx) => {
                self.open_file_prompt(
                    file_ops::FILT_PTNOISE,
                    FileOp::ReplacePtNoiseSingle(voice_idx),
                    false,
                );
            }
            Cmd::PromptReplaceSf2Single(voice_idx) => self.open_file_prompt(
                file_ops::FILT_SF2,
                FileOp::ReplaceSf2Single(voice_idx),
                false,
            ),
            Cmd::PromptSaveAs => {
                self.open_file_prompt(file_ops::FILT_PTCOP, FileOp::SaveProjAs, true);
            }
            Cmd::PromptImportMidi => {
                self.open_file_prompt(file_ops::FILT_MIDI, FileOp::ImportMidi, false);
            }
            Cmd::PromptImportPiyo => {
                self.open_file_prompt(file_ops::FILT_PIYOPIYO, FileOp::ImportPiyoPiyo, false);
            }
            Cmd::PromptImportOrg => {
                self.open_file_prompt(file_ops::FILT_ORGANYA, FileOp::ImportOrganya, false);
            }
            Cmd::PromptExportWav => {
                self.open_file_prompt(file_ops::FILT_WAV, FileOp::ExportWav, true);
            }
            Cmd::PromptOpenPtcop => {
                self.open_file_prompt(file_ops::FILT_PTCOP, FileOp::OpenProj, false);
            }
            Cmd::ClearProject => {
                let mut song = self.song.lock().unwrap();
                *song = SongState::new(self.out.rate);
                song.prepare(self.out.rate);
                self.open_file = None;
                // Toot default unit that comes with clean state
                self.ui_state.freeplay_piano.toot = Some(UnitIdx(0));
            }
            #[cfg(not(target_arch = "wasm32"))]
            Cmd::OpenPtcopFromPath { path } => {
                if let Err(e) = self.load_song_from_path(path) {
                    self.modal_payload =
                        Some(ModalPayload::Msg(format!("Error loading project:\n{e}")));
                }
            }
            Cmd::ResetUnitVoice { unit, voice } => {
                let mut song = self.song.lock().unwrap();
                let song = &mut *song;
                if let Some(unit) = song.herd.units.get_mut(unit.usize()) {
                    unit.reset_voice(&song.ins, voice, song.song.master.timing);
                }
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    fn do_web_cmd(&mut self, cmd: crate::web_glue::WebCmd) {
        use crate::web_glue::WebCmd;
        match cmd {
            WebCmd::OpenFile { data, name } => {
                if let Err(e) = self.load_song_from_bytes(&data) {
                    self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
                }
                self.open_file = Some(name.into());
            }
            WebCmd::ImportMidi { data } => {
                self.import_midi_from_bytes(&data);
            }
            WebCmd::ImportPiyo { data } => {
                self.import_piyopiyo_from_bytes(&data);
            }
            WebCmd::ImportOrganya { data } => {
                self.import_organya_from_bytes(&data);
            }
            WebCmd::ImportPtVoice { data, name } => {
                self.import_ptvoice(data, name.as_ref());
            }
            WebCmd::ImportPtNoise { data, name } => {
                self.import_ptnoise(data, name.as_ref());
            }
            WebCmd::ReplaceVoicesPtCop { data } => {
                let (_, _, ins) = ptcow::read_song(&data, 44_100).unwrap();
                let mut song = self.song.lock().unwrap();
                song.ins.voices = ins.voices;
            }
            WebCmd::ReplacePtVoiceSingle {
                data,
                name,
                voice_idx,
            } => match load_and_recalc_voice(data, name.as_ref(), just_load_ptvoice, self.out.rate)
            {
                Ok(voice) => {
                    let mut song = self.song.lock().unwrap();
                    if let Some(voice_of_idx) = song.ins.voices.get_mut(voice_idx.usize()) {
                        *voice_of_idx = voice;
                    } else {
                        song.ins.voices.push(voice);
                    }
                }
                Err(e) => {
                    self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
                }
            },
            WebCmd::ReplacePtNoiseSingle {
                data,
                name,
                voice_idx,
            } => match load_and_recalc_voice(data, name.as_ref(), just_load_ptnoise, self.out.rate)
            {
                Ok(voice) => {
                    let mut song = self.song.lock().unwrap();
                    if let Some(voice_of_idx) = song.ins.voices.get_mut(voice_idx.usize()) {
                        *voice_of_idx = voice;
                    } else {
                        song.ins.voices.push(voice);
                    }
                }
                Err(e) => {
                    self.modal_payload = Some(ModalPayload::Msg(e.to_string()));
                }
            },
        }
    }
    fn reload_current_file(&mut self) {
        match self.open_file.as_ref().cloned() {
            Some(path) => match self.load_song_from_path(path.clone()) {
                Ok(()) => {
                    self.cmd
                        .toast(ToastKind::Info, format!("Reloaded {}", path.display()), 3.0);
                }
                Err(e) => {
                    self.cmd.toast(ToastKind::Error, e.to_string(), 6.0);
                }
            },
            None => {
                self.cmd
                    .toast(ToastKind::Error, "No file to reload".into(), 5.0);
            }
        }
    }
    fn save_current_file(&mut self) {
        if let Some(path) = &self.open_file {
            let song = self.song.lock().unwrap();
            let serialized = ptcow::serialize_project(&song.song, &song.herd, &song.ins).unwrap();
            std::fs::write(path, serialized).unwrap();
            self.cmd
                .toast(ToastKind::Info, format!("Saved {}", path.display()), 3.0);
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

fn post_load_prep(
    song_ref: &mut SongState,
    out_rate: SampleRate,
    freeplay_toot: &mut Option<UnitIdx>,
) {
    // We want to be prepared to moo before we spawn the audio thread, so we can toot and stuff.
    crate::audio_out::prepare_song(song_ref, true);
    ptcow::rebuild_tones(
        &mut song_ref.ins,
        out_rate,
        &mut song_ref.herd.delays,
        &mut song_ref.herd.overdrives,
        &song_ref.song.master,
    );
    // Set a default toot unit if units aren't empty
    let has_units = !song_ref.herd.units.is_empty();
    if has_units {
        *freeplay_toot = Some(UnitIdx(0));
        // Set initial voices, etc.
        do_tick0_events(song_ref);
    }
    // Make sure `moo_end` is not set, so mooing does something
    song_ref.herd.moo_end = false;
}

// Apply things like setting initial voices for units on tick 0
fn do_tick0_events(song: &mut SongState) {
    for ev in song.song.events.iter().take_while(|ev| ev.tick == 0) {
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

fn poly_migrate_single(
    app_modal_payload: &mut Option<ModalPayload>,
    song: &mut SongState,
    migrate_from: UnitIdx,
) -> Option<UnitIdx> {
    let migrate_to = UnitIdx(song.herd.units.len().try_into().unwrap());
    if migrate_to.0 >= 50 {
        *app_modal_payload = Some(ModalPayload::Msg(
            "Error: Cannot create more units than 50".to_string(),
        ));
        return None;
    }
    if !poly_migrate_units(migrate_from, migrate_to, &mut song.song) {
        return None;
    }
    // Duplicate certain types of events for the migrated-to unit
    let mut dups = Vec::new();
    for (idx, eve) in song.song.events.eves.iter().enumerate() {
        if eve.unit == migrate_from
            && matches!(
                eve.payload,
                EventPayload::SetVoice(_)
                    | EventPayload::SetGroup(_)
                    | EventPayload::Volume(_)
                    | EventPayload::PanTime(_)
            )
        {
            let mut dup = song.song.events.eves[idx];
            dup.unit = migrate_to;
            dups.push((idx + 1, dup));
        }
    }
    for (idx, dup) in dups {
        song.song.events.eves.insert(idx, dup);
    }
    let from_name = &song.herd.units[migrate_from.usize()].name;
    let unit = ptcow::Unit {
        name: format!("{from_name}-p"),
        ..Default::default()
    };
    song.herd.units.push(unit);
    Some(migrate_to)
}
