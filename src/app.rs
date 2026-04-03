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
                modal::Modal,
            },
        },
        audio_out::{OutParams, SongState, SongStateHandle, spawn_ptcow_audio_thread},
        evilscript,
        pxtone_misc::{poly_migrate_units, reset_voice_for_units_with_voice_idx},
    },
    anyhow::Context,
    eframe::egui,
    egui_toast::{Toast, ToastKind, ToastOptions},
    ptcow::{
        Bps, ChNum, Event, EventPayload, NoiseTable, PcmData, SampleRate, UnitIdx, Voice, VoiceIdx,
    },
    std::{
        path::{Path, PathBuf},
        sync::{
            Arc, Mutex, RwLock,
            atomic::{AtomicBool, AtomicU32, Ordering},
        },
    },
    tinyaudio::OutputDevice,
};

pub mod command_queue;
pub mod ui;

fn auto_migrate_all(app_modal: &mut Modal, app_ui_state: &mut ui::UiState, song: &mut SongState) {
    let orig_n_units: u8 = song.herd.units.len();
    for mut migrate_from in (0..orig_n_units).map(UnitIdx) {
        // Skip muted units
        if song.herd.units[migrate_from].mute {
            continue;
        }
        while let Some(out) = poly_migrate_single(app_modal, song, migrate_from) {
            migrate_from = out;
        }
    }
    // Doesn't seem to sound right until we restart the song
    crate::app::post_load_prep(song, &mut app_ui_state.shared.active_unit);
}

#[derive(Default)]
pub struct SongLockMy {
    locked: bool,
    reason: &'static str,
}

#[derive(Default)]
pub struct SongLockShared {
    cancel_requested: AtomicBool,
    can_unlock: AtomicBool,
    /// Stores float data for progress, range 0..1
    progress: AtomicU32,
    error: RwLock<String>,
}

#[derive(Default)]
pub struct SongLock {
    my: SongLockMy,
    shared: Arc<SongLockShared>,
}
impl SongLock {
    fn lock(&mut self, reason: &'static str) {
        self.my.reason = reason;
        self.my.locked = true;
        self.shared.cancel_requested.store(false, Ordering::Relaxed);
        self.shared.can_unlock.store(false, Ordering::Relaxed);
        self.shared.progress.store(0, Ordering::Relaxed);
        self.shared.error.write().unwrap().clear();
    }
}

pub struct App {
    pub prefs: Preferences,
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
    modal: Modal,
    pub(crate) cmd: CommandQueue,
    /// If active, we don't try to lock the song Mutex, because it's being used
    song_lock: SongLock,
    #[cfg(target_arch = "wasm32")]
    web_cmd: crate::web_glue::WebCmdQueueHandle,
}

#[derive(Default)]
pub struct Preferences {
    pub jp_fallback_font_path: String,
    pub midi_auto_poly_migrate: bool,
}

impl Preferences {
    pub const JP_FALLBACK: &str = "jp_fallback_font_path";
}

pub type BundledSongs = &'static [(&'static str, &'static [u8])];

fn load_persistence(cc: &eframe::CreationContext<'_>, app: &mut App) {
    if let Some(storage) = cc.storage {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(folders) = eframe::get_value(storage, "pinned-folders") {
                app.file_dia.storage_mut().pinned_folders = folders;
            }
            if let Some(list) = eframe::get_value(storage, "recently-opened") {
                app.recently_opened = list;
            }
        }
        if let Some(text) = storage.get_string("out-buf-size") {
            if let Ok(num) = text.parse() {
                app.out.buf_size = num;
            }
        }
    }
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        args: CliArgs,
        out_params: OutParams,
        bundled_songs: BundledSongs,
    ) -> Self {
        let sample_rate = 44_100;
        let mut song_state = SongState::new(sample_rate);
        let mut modal = Modal::default();
        if let Some(mid_path) = args.midi_import {
            let mid_data = std::fs::read(&mid_path).unwrap();
            match crate::midi::write_midi_to_pxtone(
                &mid_data,
                &mut song_state.herd,
                &mut song_state.song,
                &mut song_state.ins,
            ) {
                Ok(()) => {
                    song_state.song.recalculate_length();
                }
                Err(e) => {
                    modal.err(e);
                }
            }
        }
        if let Some(path) = args.piyo_import {
            let piyo_data = std::fs::read(&path).unwrap();
            let piyo = piyopiyo::Song::load(&piyo_data).unwrap();
            crate::piyopiyo::import(
                &piyo,
                &mut song_state.herd,
                &mut song_state.song,
                &mut song_state.ins,
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
            );
        }
        if let Some(ptcop_path) = args.voice_import {
            import_voices_from_ptcop(&ptcop_path, &mut song_state);
        }
        song_state.prepare();
        let song_state_handle = Arc::new(Mutex::new(song_state));
        let mut this = Self {
            prefs: Preferences::default(),
            song: song_state_handle.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            file_dia: egui_file_dialog::FileDialog::new()
                .add_file_filter_extensions(FILT_PTCOP.name, vec![FILT_PTCOP.ext])
                .add_save_extension(FILT_PTCOP.name, FILT_PTCOP.ext)
                .add_save_extension(FILT_WAV.name, FILT_WAV.ext)
                .add_save_extension(FILT_PTVOICE.name, FILT_PTVOICE.ext)
                .add_save_extension(FILT_PTNOISE.name, FILT_PTNOISE.ext)
                .add_file_filter_extensions(FILT_WAV.name, vec![FILT_WAV.ext])
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
            modal,
            cmd: CommandQueue::default(),
            song_lock: SongLock::default(),
            #[cfg(target_arch = "wasm32")]
            web_cmd: Default::default(),
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            // Additional file dialog configuration
            this.file_dia.config_mut().retain_selected_entry = true;
        }
        load_persistence(cc, &mut this);
        if let Some(path) = args.open {
            if let Err(e) = this.load_song_from_path(path) {
                this.modal.err(format!("Error loading project:\n{e}"));
            }
        } else if args.recent {
            // Try to open most recent file
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(path) = this.recently_opened.most_recent() {
                if let Err(e) = this.load_song_from_path(path.clone()) {
                    this.modal.err(format!("Error loading project:\n{e}"));
                }
            }
        } else if let Some(song) = bundled_songs.first() {
            // Load a bundled song if no song was requested to open
            if let Err(e) = this.load_song_from_bytes(song.1) {
                this.modal.err(format!("Error loading project:\n{e}"));
            } else {
                this.open_file = Some(song.0.into());
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

    fn import_midi_from_bytes(&mut self, mid_data: &[u8]) -> anyhow::Result<()> {
        let mut song = self.song.lock().unwrap();
        let song = &mut *song;
        crate::midi::write_midi_to_pxtone(mid_data, &mut song.herd, &mut song.song, &mut song.ins)?;
        song.song.recalculate_length();
        if self.prefs.midi_auto_poly_migrate {
            auto_migrate_all(&mut self.modal, &mut self.ui_state, song);
        }
        post_load_prep(song, &mut self.ui_state.shared.active_unit);
        Ok(())
    }

    fn import_piyopiyo_from_bytes(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let piyo = piyopiyo::Song::load(data)?;
        let mut song = self.song.lock().unwrap();
        let song = &mut *song;
        crate::piyopiyo::import(&piyo, &mut song.herd, &mut song.song, &mut song.ins);
        post_load_prep(song, &mut self.ui_state.shared.active_unit);
        Ok(())
    }

    fn import_organya_from_bytes(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let mut org = organyacat::Song::default();
        org.read(data)?;
        let mut song = self.song.lock().unwrap();
        let song = &mut *song;
        crate::organya::import(&org, &mut song.herd, &mut song.song, &mut song.ins);
        post_load_prep(song, &mut self.ui_state.shared.active_unit);
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_file_dia_update(&mut self, ctx: &egui::Context) -> (Option<PathBuf>, Option<FileOp>) {
        use egui_file_dialog::DialogState;

        match self.file_dia.user_data::<FileOp>() {
            Some(
                FileOp::ImportPtNoise
                | FileOp::ImportPtVoice
                | FileOp::ReplacePtNoiseSingle(_)
                | FileOp::ReplacePtVoiceSingle(_),
            ) => {
                self.file_dia
                    .update_with_right_panel_ui(ctx, &mut |ui, _dia| {
                        let mut song = self.song.lock().unwrap();
                        let re = ui.add(
                            egui::TextEdit::singleline(&mut String::new())
                                .hint_text("Click here to play"),
                        );
                        if re.has_focus() {
                            ui::piano_freeplay_input(
                                &mut song,
                                ui,
                                &mut self.ui_state.shared,
                                // FIXME: We're lying here, lol
                                false,
                            );
                        }
                        ui::piano_freeplay_ui(
                            &mut song,
                            ui,
                            &mut self.ui_state.shared,
                            // FIXME: We're lying here, lol
                            false,
                            false,
                        );
                    });
            }
            _ => {
                self.file_dia.update(ctx);
            }
        }
        let picked_path = self.file_dia.take_picked();
        let file_op = self.file_dia.user_data::<FileOp>().cloned();
        if *self.file_dia.state() == DialogState::Cancelled {
            // Reset voice test unit index after closing dialog (after preview)
            self.song.lock().unwrap().freeplay_assist_units[0].voice_idx =
                self.ui_state.voices.selected_idx;
        }
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
                    FileOp::ExportPtnoise { voice } => {
                        let song = self.song.lock().unwrap();
                        let ptcow::VoiceData::Noise(noise) = &song.ins.voices[voice].base.data
                        else {
                            return;
                        };
                        (noise.to_ptnoise(), "out.ptnoise")
                    }
                    FileOp::ExportPtvoice { voice } => {
                        let song = self.song.lock().unwrap();
                        match song.ins.voices[voice].to_ptvoice() {
                            Ok(data) => (data, "out.ptvoice"),
                            Err(e) => {
                                self.cmd.toast(ToastKind::Error, format!("{e}"), 5.0);
                                return;
                            }
                        }
                    }
                    FileOp::ExportWavData {
                        data,
                        ch_num,
                        sample_rate,
                    } => {
                        use ptcow::ChNum;

                        let mut out = std::io::Cursor::new(Vec::new());
                        let _ = crate::util::write_wav(
                            &mut out,
                            ChNum::Mono,
                            bytemuck::cast_slice(&data),
                            sample_rate,
                        );
                        (out.into_inner(), "out.wav")
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

    fn import_ptvoice(&mut self, data: &[u8], path: &Path) {
        match load_and_recalc_voice(data, path, just_load_ptvoice, self.out.rate) {
            Ok(voice) => {
                let mut song = self.song.lock().unwrap();
                let song = &mut *song;
                song.ins.voices.push(voice);
                let idx = VoiceIdx(song.ins.voices.len() - 1);
                reset_voice_for_units_with_voice_idx(song, idx);
                self.ui_state.voices.selected_idx = idx;
                self.ui_state.voices.soft_reset(
                    &song.ins,
                    &[],
                    &song.song.master,
                    &mut song.freeplay_assist_units[0],
                );
            }
            Err(e) => self.modal.err(e),
        }
    }

    fn import_ptnoise(&mut self, data: &[u8], path: &Path) {
        match load_and_recalc_voice(data, path, just_load_ptnoise, self.out.rate) {
            Ok(voice) => {
                let mut song = self.song.lock().unwrap();
                let song = &mut *song;
                song.ins.voices.push(voice);
                let idx = VoiceIdx(song.ins.voices.len() - 1);
                reset_voice_for_units_with_voice_idx(song, idx);
                self.ui_state.voices.selected_idx = idx;
                self.ui_state.voices.soft_reset(
                    &song.ins,
                    &[],
                    &song.song.master,
                    &mut song.freeplay_assist_units[0],
                );
            }
            Err(e) => self.modal.err(e),
        }
    }
    fn import_ogg_vorbis(&mut self, data: &[u8], path: &Path) {
        match load_and_recalc_voice(data, path, just_load_ogg, self.out.rate) {
            Ok(voice) => {
                let mut song = self.song.lock().unwrap();
                song.ins.voices.push(voice);
                let idx = VoiceIdx(song.ins.voices.len() - 1);
                reset_voice_for_units_with_voice_idx(&mut song, idx);
                self.ui_state.voices.selected_idx = idx;
            }
            Err(_) => todo!(),
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn desktop_handle_file_op(&mut self, path: PathBuf, op: FileOp) -> anyhow::Result<()> {
        match op {
            FileOp::OpenProj => {
                if let Err(e) = self.load_song_from_path(path) {
                    self.modal.err(format!("Error loading project:\n{e}"));
                }
            }
            FileOp::ImportAllPtcop => {
                let mut song = self.song.lock().unwrap();
                import_voices_from_ptcop(&path, &mut song);
            }
            FileOp::ReplacePtVoiceSingle(voice_idx) => {
                let data = std::fs::read(&path).unwrap();
                match load_and_recalc_voice(&data, &path, just_load_ptvoice, self.out.rate) {
                    Ok(voice) => {
                        let mut song = self.song.lock().unwrap();
                        if let Some(voice_of_idx) = song.ins.voices.get_mut(voice_idx) {
                            *voice_of_idx = voice;
                        } else {
                            song.ins.voices.push(voice);
                        }
                        // Reset back test unit voice idx from preview, to the normal one
                        song.freeplay_assist_units[0].voice_idx = voice_idx;
                        reset_voice_for_units_with_voice_idx(&mut song, voice_idx);
                    }
                    Err(e) => {
                        self.modal.err(e);
                    }
                }
            }
            FileOp::ReplacePtNoiseSingle(voice_idx) => {
                let data = std::fs::read(&path).unwrap();
                match load_and_recalc_voice(&data, &path, just_load_ptnoise, self.out.rate) {
                    Ok(voice) => {
                        let mut song = self.song.lock().unwrap();
                        if let Some(voice_of_idx) = song.ins.voices.get_mut(voice_idx) {
                            *voice_of_idx = voice;
                        } else {
                            song.ins.voices.push(voice);
                        }
                        // Reset back test unit voice idx from preview, to the normal one
                        song.freeplay_assist_units[0].voice_idx = voice_idx;
                        reset_voice_for_units_with_voice_idx(&mut song, voice_idx);
                    }
                    Err(e) => {
                        self.modal.err(e);
                    }
                }
            }
            FileOp::ReplaceWavSingle(voice_idx) => {
                let data = std::fs::read(&path)?;
                match load_and_recalc_voice(&data, &path, just_load_wav, self.out.rate) {
                    Ok(voice) => {
                        let mut song = self.song.lock().unwrap();
                        if let Some(voice_of_idx) = song.ins.voices.get_mut(voice_idx) {
                            *voice_of_idx = voice;
                        } else {
                            song.ins.voices.push(voice);
                        }
                        // Reset back test unit voice idx from preview, to the normal one
                        song.freeplay_assist_units[0].voice_idx = voice_idx;
                        reset_voice_for_units_with_voice_idx(&mut song, voice_idx);
                    }
                    Err(e) => {
                        self.modal.err(e);
                    }
                }
            }
            FileOp::ImportPtVoice => {
                let data = std::fs::read(&path)?;
                self.import_ptvoice(&data, &path);
            }
            FileOp::ImportPtNoise => {
                let data = std::fs::read(&path)?;
                self.import_ptnoise(&data, &path);
            }
            FileOp::ImportOggVorbis => {
                let data = std::fs::read(&path)?;
                self.import_ogg_vorbis(&data, &path);
            }
            FileOp::ImportMidi => {
                let mid_data = std::fs::read(&path)?;
                self.import_midi_from_bytes(&mid_data)?;
            }
            FileOp::ImportPiyoPiyo => {
                let data = std::fs::read(&path)?;
                self.import_piyopiyo_from_bytes(&data)?;
            }
            FileOp::ImportOrganya => {
                let data = std::fs::read(&path)?;
                self.import_organya_from_bytes(&data)?;
            }
            FileOp::SaveProjAs => {
                let song = self.song.lock().unwrap();
                match ptcow::serialize_project(&song.song, &song.herd, &song.ins) {
                    Ok(bytes) => {
                        std::fs::write(&path, bytes)?;
                        self.recently_opened.use_(path.clone());
                        self.open_file = Some(path);
                    }
                    Err(e) => {
                        self.modal.err(e);
                    }
                }
                drop(song);
            }
            FileOp::ExportWav => {
                // Disable audio device for export duration
                self.pt_audio_dev = None;
                let song = self.song.clone();
                self.song_lock.lock("Exporting .wav ...");
                let song_lock = self.song_lock.shared.clone();

                std::thread::spawn(move || {
                    let mut song = song.lock().unwrap();
                    match crate::util::export_wav(
                        &mut song,
                        &song_lock.progress,
                        &song_lock.cancel_requested,
                    ) {
                        Ok(data) => {
                            if let Err(e) = std::fs::write(path, data) {
                                *song_lock.error.write().unwrap() = e.to_string();
                            }
                        }
                        Err(e) => {
                            *song_lock.error.write().unwrap() = e.to_string();
                        }
                    }
                    song_lock.can_unlock.store(true, Ordering::Relaxed);
                });
            }
            FileOp::ExportWavData {
                ch_num,
                data,
                sample_rate,
            } => {
                let f = std::fs::File::create(path).unwrap();
                match crate::util::write_wav(f, ch_num, bytemuck::cast_slice(&data), sample_rate) {
                    Ok(()) => (),
                    Err(e) => {
                        self.modal.err(e);
                    }
                }
            }
            FileOp::ExportPtvoice { voice } => {
                let song = self.song.lock().unwrap();
                match song.ins.voices[voice].to_ptvoice() {
                    Ok(data) => {
                        if let Err(e) = std::fs::write(&path, data) {
                            self.modal.err(e);
                        }
                    }
                    Err(e) => self.modal.err(e),
                }
                self.cmd.toast(
                    ToastKind::Success,
                    format_args!("Exported to {}", path.display()),
                    5.0,
                );
            }
            FileOp::ExportPtnoise { voice } => {
                use ptcow::VoiceData;

                let song = self.song.lock().unwrap();
                let voice = &song.ins.voices[voice];
                let VoiceData::Noise(noise) = &voice.base.data else {
                    anyhow::bail!("Voice not a noise");
                };
                if let Err(e) = std::fs::write(&path, noise.to_ptnoise()) {
                    self.modal.err(e);
                }
                self.cmd.toast(
                    ToastKind::Success,
                    format_args!("Exported to {}", path.display()),
                    5.0,
                );
            }
        }
        Ok(())
    }

    fn handle_dropped_file(
        &mut self,
        dropfile: &egui::DroppedFile,
        bytes: &Arc<[u8]>,
    ) -> anyhow::Result<()> {
        if let Some((name, ext)) = dropfile.name.split_once('.') {
            match ext {
                "ptcop" | "pttune" => {
                    // Web version loads dropped files directly as bytes
                    if let Err(e) = self.load_song_from_bytes(bytes) {
                        self.modal.err(format!("Error loading project:\n{e}"));
                    }
                }
                "mid" => {
                    self.import_midi_from_bytes(bytes)?;
                }
                "pmd" => {
                    self.import_piyopiyo_from_bytes(bytes)?;
                }
                "org" => {
                    self.import_organya_from_bytes(bytes)?;
                }
                _ => {}
            }
            self.open_file = Some(format!("{name}.{ext}").into());
        }
        Ok(())
    }
}

fn load_and_recalc_voice(
    data: &[u8],
    path: &Path,
    loadfn: fn(&[u8], &Path) -> anyhow::Result<ptcow::Voice>,
    out_rate: SampleRate,
) -> anyhow::Result<ptcow::Voice> {
    let mut voice = loadfn(data, path)?;
    let noise_tbl = NoiseTable::generate();
    voice.recalculate(&noise_tbl, out_rate);
    Ok(voice)
}

fn just_load_ptvoice(data: &[u8], path: &Path) -> anyhow::Result<ptcow::Voice> {
    let mut voice = ptcow::Voice::from_ptvoice(data)?;
    if let Some(os_str) = path.file_stem() {
        voice.name = os_str.to_string_lossy().into_owned();
    }
    Ok(voice)
}

fn just_load_ptnoise(data: &[u8], path: &Path) -> anyhow::Result<ptcow::Voice> {
    let noise = ptcow::NoiseData::from_ptnoise(data)?;
    let mut voice = ptcow::Voice::from_data(ptcow::VoiceData::Noise(noise));
    if let Some(os_str) = path.file_stem() {
        voice.name = os_str.to_string_lossy().into_owned();
    }
    Ok(voice)
}

fn just_load_wav(data: &[u8], path: &Path) -> anyhow::Result<ptcow::Voice> {
    let wav = hound::WavReader::new(data)?;
    let spec = wav.spec();
    let ch = match spec.channels {
        1 => ChNum::Mono,
        2 => ChNum::Stereo,
        _ => anyhow::bail!("Unsupported ch num: {}", spec.channels),
    };
    let bps = match spec.bits_per_sample {
        8 => Bps::B8,
        16 => Bps::B16,
        _ => anyhow::bail!("Unsupported bps: {}", spec.bits_per_sample),
    };
    let data: Result<Vec<i16>, _> = wav.into_samples().collect();
    let data = data?;
    let pcm_data = PcmData {
        ch,
        sps: spec.sample_rate,
        bps,
        num_samples: data.len() as u32,
        smp: bytemuck::pod_collect_to_vec(&data),
    };
    let mut voice = Voice::from_data(ptcow::VoiceData::Pcm(pcm_data));
    if let Some(os_str) = path.file_stem() {
        voice.name = os_str.to_string_lossy().into_owned();
    }
    Ok(voice)
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Needs this signature due to fn pointer"
)]
fn just_load_ogg(data: &[u8], path: &Path) -> anyhow::Result<ptcow::Voice> {
    let oggv = ptcow::OggVData {
        raw_bytes: data.to_vec(),
        ch: 1,
        sps2: 0,
        smp_num: 0,
    };
    let mut voice = ptcow::Voice::from_data(ptcow::VoiceData::OggV(oggv));
    if let Some(os_str) = path.file_stem() {
        voice.name = os_str.to_string_lossy().into_owned();
    }
    Ok(voice)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.request_repaint();
        if self.song_lock.my.locked {
            egui::Modal::new("song_lock".into()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(self.song_lock.my.reason);
                    ui.spinner();
                });
                let progress =
                    f32::from_bits(self.song_lock.shared.progress.load(Ordering::Relaxed));
                ui.add(egui::ProgressBar::new(progress).show_percentage());
                if ui.button("Cancel").clicked() {
                    self.song_lock
                        .shared
                        .cancel_requested
                        .store(true, Ordering::Relaxed);
                }
            });
            if self.song_lock.shared.can_unlock.load(Ordering::Relaxed) {
                // We can restart audio thread now
                self.cmd.push(Cmd::ReplaceAudioThread);
                post_load_prep(
                    &mut self.song.lock().unwrap(),
                    &mut self.ui_state.shared.active_unit,
                );
                self.song_lock.my.locked = false;
                let err = self.song_lock.shared.error.read().unwrap();
                if err.is_empty() {
                    self.cmd
                        .toast(ToastKind::Success, ".wav successfully exported!", 5.0);
                } else {
                    self.cmd
                        .toast(ToastKind::Error, format!("Error exporting wav: {err}"), 5.0);
                }
            }
            return;
        }
        // Clean up unused extra freeplay units
        {
            let mut song = self.song.lock().unwrap();
            let mut idx = 0;
            song.freeplay_assist_units.retain(|u| {
                let mut retain = true;
                if idx != 0 {
                    if u.tones[0].life_count == 0 {
                        retain = false;
                    }
                }
                idx += 1;
                retain
            });
        }
        egui::Panel::top("top_panel").show_inside(ui, |ui| ui::top_panel::top_panel(self, ui));
        if self.ui_state.show_left_panel() {
            egui::Panel::left("left_panel").show_inside(ui, |ui| ui::left_panel::ui(self, ui));
        }
        egui::CentralPanel::default().show_inside(ui, |ui| ui::central_panel(self, ui));
        self.ui_state
            .windows
            .update(ui, &mut self.song.lock().unwrap(), &mut self.prefs);

        #[cfg(not(target_arch = "wasm32"))]
        let (mut picked_path, mut file_op) = self.handle_file_dia_update(ui);
        #[cfg(target_arch = "wasm32")]
        let (mut picked_path, mut file_op) = (None, None);

        ui.input(|inp| {
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
                    if let Err(e) = self.handle_dropped_file(dropfile, bytes) {
                        self.modal.err(e);
                    }
                }
            }
        });

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = picked_path
            && let Some(op) = file_op
        {
            if let Err(e) = self.desktop_handle_file_op(path, op) {
                self.modal.err(e);
            }
        }
        self.modal.update(ui, &self.song);
        self.ui_state.shared.toasts.show(ui);
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
        if self.ui_state.show_style_ed {
            self.ui_state.style_ed.show_window(ui, &mut []);
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
        storage.set_string(
            Preferences::JP_FALLBACK,
            self.prefs.jp_fallback_font_path.clone(),
        );
    }
    fn persist_egui_memory(&self) -> bool {
        false
    }
}

fn import_voices_from_ptcop(path: &Path, song: &mut SongState) {
    let data = std::fs::read(path).unwrap();
    let (_, _, ins) = ptcow::read_song(&data, 44_100).unwrap();

    song.ins.voices.extend(ins.voices.iter().cloned());
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
        post_load_prep(song_ref, &mut self.ui_state.shared.active_unit);
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
                indices.sort_unstable();
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
                let mut song = self.song.lock().unwrap();
                let song = &mut *song;
                self.ui_state.voices.soft_reset(
                    &song.ins,
                    std::slice::from_ref(&song.preview_voice),
                    &song.song.master,
                    &mut song.freeplay_assist_units[0],
                );
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
            Cmd::PromptImportOggVorbis => {
                self.open_file_prompt(file_ops::FILT_OGG, FileOp::ImportOggVorbis, false);
            }
            Cmd::PromptImportAllPtcop => {
                self.open_file_prompt(file_ops::FILT_PTCOP, FileOp::ImportAllPtcop, false);
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
            Cmd::PromptReplaceWavSingle(voice_idx) => {
                self.open_file_prompt(
                    file_ops::FILT_WAV,
                    FileOp::ReplaceWavSingle(voice_idx),
                    false,
                );
            }
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
            Cmd::PromptExportWavData {
                data,
                ch_num,
                sample_rate,
            } => {
                self.open_file_prompt(
                    file_ops::FILT_WAV,
                    FileOp::ExportWavData {
                        data,
                        ch_num,
                        sample_rate,
                    },
                    true,
                );
            }
            Cmd::PromptExportPtnoise { voice } => {
                self.open_file_prompt(
                    file_ops::FILT_PTNOISE,
                    FileOp::ExportPtnoise { voice },
                    true,
                );
            }
            Cmd::PromptExportPtvoice { voice } => {
                self.open_file_prompt(
                    file_ops::FILT_PTVOICE,
                    FileOp::ExportPtvoice { voice },
                    true,
                );
            }
            Cmd::PromptOpenPtcop => {
                self.open_file_prompt(file_ops::FILT_PTCOP, FileOp::OpenProj, false);
            }
            Cmd::ClearProject => {
                let mut song = self.song.lock().unwrap();
                *song = SongState::new(self.out.rate);
                song.prepare();
                self.open_file = None;
                self.ui_state.shared.active_unit = SongState::VOICE_TEST_UNIT_IDX;
            }
            #[cfg(not(target_arch = "wasm32"))]
            Cmd::OpenPtcopFromPath { path } => {
                if let Err(e) = self.load_song_from_path(path) {
                    self.modal.err(format!("Error loading project:\n{e}"));
                }
            }
            Cmd::ResetUnitVoice { unit, voice } => {
                let mut song = self.song.lock().unwrap();
                let song = &mut *song;
                let unit = song
                    .herd
                    .units
                    .get_mut(unit)
                    .unwrap_or(&mut song.freeplay_assist_units[0]);
                unit.reset_voice(
                    &song.ins,
                    voice,
                    song.song.master.timing,
                    std::slice::from_ref(&song.preview_voice),
                );
            }
            Cmd::Modal(f) => {
                f(&mut self.modal);
            }
            Cmd::ResetVoiceForUnitsWithVoiceIdx { idx } => {
                let mut song = self.song.lock().unwrap();
                reset_voice_for_units_with_voice_idx(&mut song, idx);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    fn do_web_cmd(&mut self, cmd: crate::web_glue::WebCmd) {
        use crate::web_glue::WebCmd;
        match cmd {
            WebCmd::OpenFile { data, name } => {
                if let Err(e) = self.load_song_from_bytes(&data) {
                    self.cmd.toast(ToastKind::Error, format!("{e}"), 5.0);
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
                self.import_ptvoice(&data, name.as_ref());
            }
            WebCmd::ImportPtNoise { data, name } => {
                self.import_ptnoise(&data, name.as_ref());
            }
            WebCmd::ImportOggVorbis { data, name } => {
                self.import_ogg_vorbis(&data, name.as_ref());
            }
            WebCmd::ImportAllPtcop { data } => {
                let (_, _, ins) = ptcow::read_song(&data, 44_100).unwrap();
                let mut song = self.song.lock().unwrap();
                song.ins.voices.extend(ins.voices.iter().cloned());
            }
            WebCmd::ReplacePtVoiceSingle {
                data,
                name,
                voice_idx,
            } => {
                match load_and_recalc_voice(&data, name.as_ref(), just_load_ptvoice, self.out.rate)
                {
                    Ok(voice) => {
                        let mut song = self.song.lock().unwrap();
                        if let Some(voice_of_idx) = song.ins.voices.get_mut(voice_idx) {
                            *voice_of_idx = voice;
                        } else {
                            song.ins.voices.push(voice);
                        }
                    }
                    Err(e) => {
                        self.cmd.toast(ToastKind::Error, format!("{e}"), 5.0);
                    }
                }
            }
            WebCmd::ReplacePtNoiseSingle {
                data,
                name,
                voice_idx,
            } => {
                match load_and_recalc_voice(&data, name.as_ref(), just_load_ptnoise, self.out.rate)
                {
                    Ok(voice) => {
                        let mut song = self.song.lock().unwrap();
                        if let Some(voice_of_idx) = song.ins.voices.get_mut(voice_idx) {
                            *voice_of_idx = voice;
                        } else {
                            song.ins.voices.push(voice);
                        }
                    }
                    Err(e) => {
                        self.cmd.toast(ToastKind::Error, format!("{e}"), 5.0);
                    }
                }
            }
            WebCmd::ReplaceWavSingle {
                data,
                name,
                voice_idx,
            } => match load_and_recalc_voice(&data, name.as_ref(), just_load_wav, self.out.rate) {
                Ok(voice) => {
                    let mut song = self.song.lock().unwrap();
                    if let Some(voice_of_idx) = song.ins.voices.get_mut(voice_idx) {
                        *voice_of_idx = voice;
                    } else {
                        song.ins.voices.push(voice);
                    }
                }
                Err(e) => {
                    self.cmd.toast(ToastKind::Error, format!("{e}"), 5.0);
                }
            },
        }
    }
    fn reload_current_file(&mut self) {
        match self.open_file.clone() {
            Some(path) => match self.load_song_from_path(path.clone()) {
                Ok(()) => {
                    self.cmd
                        .toast(ToastKind::Info, format!("Reloaded {}", path.display()), 3.0);
                }
                Err(e) => {
                    self.cmd.toast(ToastKind::Error, e, 6.0);
                }
            },
            None => {
                self.cmd.toast(ToastKind::Error, "No file to reload", 5.0);
            }
        }
    }
    fn save_current_file(&mut self) {
        if let Some(path) = &self.open_file {
            let song = self.song.lock().unwrap();
            match ptcow::serialize_project(&song.song, &song.herd, &song.ins) {
                Ok(out) => {
                    std::fs::write(path, out).unwrap();
                    self.cmd.toast(
                        ToastKind::Info,
                        format_args!("Saved {}", path.display()),
                        3.0,
                    );
                }
                Err(e) => {
                    self.modal.err(format_args!("Error saving: {e}"));
                }
            }
        }
    }
    /// Replace already running ptcow audio thread with a new one
    ///
    /// IMPORTANT: This should be called *OUTSIDE* of any critical section involving the song state handle
    /// Otherwise, a deadlock can happen.
    ///
    /// You should probably be sending [`crate::app::command_queue::Cmd::ReplaceAudioThread`] instead.
    fn replace_pt_audio_thread(
        app_pt_audio_dev: &mut Option<OutputDevice>,
        app_out: OutParams,
        app_song: SongStateHandle,
    ) {
        // Drop the old handle, so the thread can join, and we avoid a deadlock.
        *app_pt_audio_dev = None;
        // Now we can spawn the new thread
        *app_pt_audio_dev = Some(spawn_ptcow_audio_thread(app_out, app_song));
    }
}

fn post_load_prep(song_ref: &mut SongState, freeplay_toot: &mut UnitIdx) {
    // We want to be prepared to moo before we spawn the audio thread, so we can toot and stuff.
    crate::audio_out::prepare_song(song_ref, true);
    ptcow::rebuild_tones(
        &mut song_ref.ins,
        &mut song_ref.herd.delays,
        &mut song_ref.herd.overdrives,
        &song_ref.song.master,
    );
    // Also make sure to properly reset voices so things don't sound off-key
    song_ref
        .herd
        .tune_cow_voices(&song_ref.ins, song_ref.song.master.timing, &[]);
    for unit in &mut song_ref.freeplay_assist_units {
        unit.reset_voice(&song_ref.ins, VoiceIdx(0), song_ref.song.master.timing, &[]);
    }
    let has_units = !song_ref.herd.units.is_empty();
    if has_units {
        // Set initial voices, etc.
        do_tick0_events(song_ref);
    } else {
        // If there are no units, set it to the voice test unit.
        *freeplay_toot = SongState::VOICE_TEST_UNIT_IDX;
    }
    // Make sure `moo_end` is not set, so mooing does something
    song_ref.herd.moo_end = false;
}

// Apply things like setting initial voices for units on tick 0
fn do_tick0_events(song: &mut SongState) {
    for ev in song.song.events.iter().take_while(|ev| ev.tick == 0) {
        let Some(unit) = song.herd.units.get_mut(ev.unit) else {
            continue;
        };
        match ev.payload {
            EventPayload::Velocity(vel) => unit.velocity = vel,
            EventPayload::Volume(vol) => unit.volume = vol,
            EventPayload::SetVoice(idx) => {
                unit.reset_voice(
                    &song.ins,
                    idx,
                    song.song.master.timing,
                    std::slice::from_ref(&song.preview_voice),
                );
            }
            EventPayload::SetGroup(group_idx) => unit.group = group_idx,
            _ => {}
        }
    }
}

fn poly_migrate_single(
    app_modal: &mut Modal,
    song: &mut SongState,
    migrate_from: UnitIdx,
) -> Option<UnitIdx> {
    let migrate_to = UnitIdx(song.herd.units.len());
    if migrate_to.0 >= 50 {
        app_modal.err("Error: Cannot create more units than 50");
        return None;
    }
    if !poly_migrate_units(migrate_from, migrate_to, &mut song.song.events) {
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
    // Since we inserted new events, we must now sort
    song.song.events.sort();
    let from_name = &song.herd.units[migrate_from].name;
    let unit = ptcow::Unit {
        name: format!("{from_name}-p"),
        ..Default::default()
    };
    song.herd.units.push(unit);
    Some(migrate_to)
}
