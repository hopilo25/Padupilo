//! Jendela utama Padupilo.

mod jobs;
mod timeline;

use eframe::egui::{self, Color32, RichText, TextureHandle};
use jobs::{Job, JobResult};
use padupilo::dict::{core_key, Dict};
use padupilo::export::{run_export, ExportJob, PlacedClip};
use padupilo::media::{self, Tools};
use padupilo::placement::{natural_order, place, Clip, Mode, Placement, SPEED_MAX, SPEED_MIN};
use padupilo::project::Project;
use padupilo::render::{composite, Renderer, Style, StyleKind};
use padupilo::subs::{make_chunks, timeline_words, Chunk};
use padupilo::transcribe::{retext, transcribe, words_to_text, Word};
use std::path::{Path, PathBuf};
use timeline::ACCENT;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Klip,
    Subtitle,
    Audio,
    Ekspor,
}

pub struct App {
    base: PathBuf,
    tools: Tools,
    project: Project,
    project_path: Option<PathBuf>,
    dict: Dict,
    tab: Tab,
    video_dur: f64,
    video_size: (u32, u32),
    video_fps: f64,
    /// Teks yang sedang diedit per klip.
    edit_text: Vec<String>,
    /// Hasil transkripsi mentah (untuk belajar koreksi).
    raw_words: Vec<Option<Vec<Word>>>,
    preview_t: f64,
    frame: Option<(f64, u32, u32, Vec<u8>)>,
    preview: Option<TextureHandle>,
    preview_key: String,
    job: Option<Job>,
    message: Option<(bool, String)>,
    last_export: Option<PathBuf>,
}

fn dialog() -> rfd::FileDialog {
    rfd::FileDialog::new()
}

fn open_in_explorer(path: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg("/select,").arg(path).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn();
}

fn setup_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(14, 14, 14);
    v.window_fill = Color32::from_rgb(20, 20, 20);
    v.extreme_bg_color = Color32::from_rgb(8, 8, 8);
    v.faint_bg_color = Color32::from_rgb(24, 24, 24);
    v.selection.bg_fill = Color32::from_rgb(70, 70, 70);
    v.selection.stroke.color = Color32::WHITE;
    v.hyperlink_color = ACCENT;
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|s| {
        s.visuals = v.clone();
        s.spacing.item_spacing = egui::vec2(8.0, 7.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
    });
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        setup_style(&cc.egui_ctx);
        let base = padupilo::app_dir();
        for d in ["proyek", "hasil", "kamus"] {
            let _ = std::fs::create_dir_all(base.join(d));
        }
        let dict = Dict::load(&base.join("kamus").join("koreksi.json"));
        let mut app = App {
            tools: Tools::locate(&base),
            base,
            project: Project::default(),
            project_path: None,
            dict,
            tab: Tab::Klip,
            video_dur: 0.0,
            video_size: (1080, 1920),
            video_fps: 30.0,
            edit_text: vec![],
            raw_words: vec![],
            preview_t: 1.0,
            frame: None,
            preview: None,
            preview_key: String::new(),
            job: None,
            message: None,
            last_export: None,
        };
        // Dibuka dengan klik dua kali file .padupilo (atau seret ke ikon aplikasi).
        if let Some(arg) = std::env::args_os().nth(1).map(PathBuf::from) {
            if arg.extension().map(|e| e == "padupilo").unwrap_or(false) {
                app.load_project(arg, &cc.egui_ctx);
            }
        }
        app
    }

    fn info(&mut self, s: impl Into<String>) {
        self.message = Some((false, s.into()));
    }
    fn error(&mut self, s: impl Into<String>) {
        self.message = Some((true, s.into()));
    }
    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn placement(&self) -> Placement {
        place(&self.project.clips, &self.project.scenes, self.video_dur, &self.project.mode)
    }

    fn chunks(&self, placement: &Placement) -> Vec<Chunk> {
        let per_clip: Vec<(Vec<Word>, f64, f64)> = self
            .project
            .clips
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let w = self.project.words.get(i)?.as_ref()?;
                Some((w.clone(), placement.items.get(i)?.start, c.speed))
            })
            .collect();
        let st = &self.project.style;
        make_chunks(&timeline_words(&per_clip), st.max_words, st.max_chars())
    }

    fn sync_edit_text(&mut self) {
        self.project.sync_words();
        self.edit_text = self.project.words.iter().map(|w| w.as_ref().map(|w| words_to_text(w)).unwrap_or_default()).collect();
        self.raw_words.resize(self.project.clips.len(), None);
    }

    // ---------- tindakan ----------

    fn set_video(&mut self, path: PathBuf, ctx: &egui::Context) {
        match media::probe(&self.tools, &path) {
            Ok(info) => {
                self.video_dur = info.duration;
                self.video_size = (info.width, info.height);
                self.video_fps = info.fps;
                self.project.video = Some(path);
                self.frame = None;
                self.preview_t = self.preview_t.min(info.duration);
                self.detect_scenes(ctx);
            }
            Err(e) => self.error(format!("Video tidak bisa dibuka: {e}")),
        }
    }

    fn detect_scenes(&mut self, ctx: &egui::Context) {
        let Some(video) = self.project.video.clone() else { return };
        let (tools, th) = (self.tools.clone(), self.project.scene_threshold);
        self.job = Some(Job::spawn("Mendeteksi pergantian adegan", ctx.clone(), move |_, _| match media::detect_scenes(&tools, &video, th) {
            Ok(s) => JobResult::Scenes(s),
            Err(e) => JobResult::Failed(format!("Deteksi adegan gagal: {e}")),
        }));
    }

    fn add_clips(&mut self, mut paths: Vec<PathBuf>) {
        natural_order(&mut paths);
        let mut failed = vec![];
        for p in paths {
            match media::duration(&self.tools, &p) {
                Ok(d) if d > 0.0 => self.project.clips.push(Clip { path: p, dur: d, speed: 1.0 }),
                _ => failed.push(p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()),
            }
        }
        // Urutkan semua klip lagi supaya klip tambahan masuk di tempatnya.
        let mut entries: Vec<(Clip, Option<Vec<Word>>, String)> = self
            .project
            .clips
            .drain(..)
            .zip(self.project.words.drain(..).chain(std::iter::repeat(None)))
            .zip(self.edit_text.drain(..).chain(std::iter::repeat(String::new())))
            .map(|((c, w), t)| (c, w, t))
            .collect();
        let mut order: Vec<PathBuf> = entries.iter().map(|e| e.0.path.clone()).collect();
        natural_order(&mut order);
        entries.sort_by_key(|e| order.iter().position(|p| *p == e.0.path).unwrap_or(usize::MAX));
        for (c, w, t) in entries {
            self.project.clips.push(c);
            self.project.words.push(w);
            self.edit_text.push(t);
        }
        self.raw_words = vec![None; self.project.clips.len()];
        if !failed.is_empty() {
            self.error(format!("Tidak bisa dibaca: {}", failed.join(", ")));
        }
    }

    fn remove_clip(&mut self, i: usize) {
        self.project.clips.remove(i);
        self.project.sync_words();
        self.project.words.remove(i);
        if i < self.edit_text.len() {
            self.edit_text.remove(i);
        }
        if i < self.raw_words.len() {
            self.raw_words.remove(i);
        }
        if let Mode::Manual { starts } = &mut self.project.mode {
            if i < starts.len() {
                starts.remove(i);
            }
        }
    }

    fn move_clip(&mut self, i: usize, j: usize) {
        self.project.sync_words();
        self.edit_text.resize(self.project.clips.len(), String::new());
        self.raw_words.resize(self.project.clips.len(), None);
        self.project.clips.swap(i, j);
        self.project.words.swap(i, j);
        self.edit_text.swap(i, j);
        self.raw_words.swap(i, j);
    }

    fn transcribe_all(&mut self, ctx: &egui::Context) {
        let clips: Vec<PathBuf> = self.project.clips.iter().map(|c| c.path.clone()).collect();
        let (tools, lang, dict) = (self.tools.clone(), self.project.language.clone(), self.dict.clone());
        let work = self.base.join("sementara");
        self.job = Some(Job::spawn("Transkripsi", ctx.clone(), move |report, cancel| {
            let mut out = vec![];
            for (i, c) in clips.iter().enumerate() {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return JobResult::Failed("Transkripsi dibatalkan".into());
                }
                report(&format!("Transkripsi klip {} dari {}", i + 1, clips.len()), i as f32 / clips.len() as f32);
                match transcribe(&tools, c, &lang, &work) {
                    Ok(words) => out.push(Some(words.into_iter().map(|w| Word { text: dict.apply(&w.text), ..w }).collect())),
                    Err(e) => return JobResult::Failed(format!("Klip {}: {e}", i + 1)),
                }
            }
            JobResult::Words(out)
        }));
    }

    fn learn_corrections(&mut self) {
        let mut learned = 0;
        for (raw, cur) in self.raw_words.iter().zip(&self.project.words) {
            let (Some(raw), Some(cur)) = (raw, cur) else { continue };
            if raw.len() != cur.len() {
                continue;
            }
            for (a, b) in raw.iter().zip(cur) {
                if core_key(&a.text) != core_key(&b.text) && !core_key(&b.text).is_empty() {
                    let right: String = b.text.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
                    self.dict.add(&a.text, &right);
                    learned += 1;
                }
            }
        }
        let path = self.base.join("kamus").join("koreksi.json");
        match self.dict.save(&path) {
            Ok(_) => self.info(format!("{learned} koreksi disimpan ke kamus.")),
            Err(e) => self.error(format!("Kamus gagal disimpan: {e}")),
        }
    }

    fn export(&mut self, ctx: &egui::Context) {
        let Some(video) = self.project.video.clone() else { return self.error("Pilih video dulu.") };
        let placement = self.placement();
        let chunks = self.chunks(&placement);
        let name = self.project.base_name();
        let mut out = self.base.join("hasil").join(format!("{name}-padupilo.mp4"));
        let mut n = 2;
        while out.exists() {
            out = self.base.join("hasil").join(format!("{name}-padupilo-{n}.mp4"));
            n += 1;
        }
        let job = ExportJob {
            video,
            clips: self
                .project
                .clips
                .iter()
                .zip(&placement.items)
                .map(|(c, p)| PlacedClip { path: c.path.clone(), start: p.start, speed: c.speed })
                .collect(),
            bg_db: self.project.bg_db,
            chunks,
            style: self.project.style.clone(),
            out_srt: self.project.export_srt.then(|| out.with_extension("srt")),
            out_mp4: out.clone(),
            work_dir: self.base.join("sementara"),
        };
        let tools = self.tools.clone();
        self.job = Some(Job::spawn("Ekspor", ctx.clone(), move |report, cancel| match run_export(&tools, &job, report, cancel) {
            Ok(_) => JobResult::Exported(out),
            Err(e) => JobResult::Failed(e.to_string()),
        }));
    }

    fn download(&mut self, ctx: &egui::Context) {
        let base = self.base.clone();
        self.job = Some(Job::spawn("Unduh komponen", ctx.clone(), move |report, _| match padupilo::setup::download_components(&base, report) {
            Ok(_) => JobResult::Downloaded,
            Err(e) => JobResult::Failed(format!("Unduhan gagal: {e}")),
        }));
    }

    fn save_project(&mut self, ask: bool) {
        let path = match (&self.project_path, ask) {
            (Some(p), false) => Some(p.clone()),
            _ => dialog()
                .set_directory(self.base.join("proyek"))
                .set_file_name(format!("{}.padupilo", self.project.base_name()))
                .add_filter("Proyek Padupilo", &["padupilo"])
                .save_file(),
        };
        if let Some(p) = path {
            self.project.sync_words();
            match self.project.save(&p) {
                Ok(_) => {
                    self.info(format!("Proyek disimpan: {}", p.display()));
                    self.project_path = Some(p);
                }
                Err(e) => self.error(format!("Gagal menyimpan: {e}")),
            }
        }
    }

    fn open_project(&mut self, ctx: &egui::Context) {
        let Some(p) = dialog().set_directory(self.base.join("proyek")).add_filter("Proyek Padupilo", &["padupilo"]).pick_file() else { return };
        self.load_project(p, ctx);
    }

    fn load_project(&mut self, p: PathBuf, ctx: &egui::Context) {
        match Project::load(&p) {
            Ok(proj) => {
                let scenes = proj.scenes.clone();
                self.project = proj;
                self.project_path = Some(p);
                self.sync_edit_text();
                self.raw_words = vec![None; self.project.clips.len()];
                if let Some(v) = self.project.video.clone() {
                    if let Ok(info) = media::probe(&self.tools, &v) {
                        self.video_dur = info.duration;
                        self.video_size = (info.width, info.height);
                        self.video_fps = info.fps;
                    } else {
                        self.error("Video proyek tidak ditemukan. Pilih ulang videonya.");
                    }
                }
                self.project.scenes = scenes;
                self.frame = None;
                let _ = ctx;
            }
            Err(e) => self.error(format!("Proyek tidak bisa dibuka: {e}")),
        }
    }

    fn handle_job(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.job else { return };
        let Some(result) = job.take_result() else { return };
        self.job = None;
        match result {
            JobResult::Scenes(s) => {
                let n = s.len();
                self.project.scenes = s;
                self.info(format!("{n} adegan terdeteksi."));
            }
            JobResult::Words(w) => {
                self.raw_words = w.clone();
                self.project.words = w;
                self.sync_edit_text();
                self.info("Transkripsi selesai. Periksa teksnya di tab Subtitle.");
                self.tab = Tab::Subtitle;
            }
            JobResult::Exported(p) => {
                self.info(format!("Video jadi: {}", p.display()));
                self.last_export = Some(p);
            }
            JobResult::Downloaded => {
                self.tools = Tools::locate(&self.base);
                self.info("Komponen siap dipakai.");
                if self.project.video.is_some() && self.project.scenes.is_empty() {
                    self.detect_scenes(ctx);
                }
            }
            JobResult::Failed(e) => self.error(e),
        }
    }

    // ---------- tampilan ----------

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.label(RichText::new("hopilo").strong().size(18.0).color(Color32::WHITE));
            ui.label(RichText::new("Padupilo").size(18.0).color(Color32::from_gray(170)));
            if let Some(p) = &self.project_path {
                ui.label(RichText::new(p.file_name().unwrap_or_default().to_string_lossy()).color(Color32::from_gray(120)));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_export = !self.busy() && self.project.video.is_some() && self.tools.ready_for_media();
                if ui.add_enabled(can_export, egui::Button::new(RichText::new("Ekspor MP4").strong().color(Color32::BLACK)).fill(ACCENT)).clicked() {
                    self.export(&ctx);
                }
                if ui.button("Simpan sebagai").clicked() {
                    self.save_project(true);
                }
                if ui.button("Simpan").clicked() {
                    self.save_project(false);
                }
                if ui.add_enabled(!self.busy(), egui::Button::new("Buka proyek")).clicked() {
                    self.open_project(&ctx);
                }
                if ui.add_enabled(!self.busy(), egui::Button::new("Proyek baru")).clicked() {
                    self.project = Project::default();
                    self.project_path = None;
                    self.edit_text.clear();
                    self.raw_words.clear();
                    self.frame = None;
                    self.video_dur = 0.0;
                }
            });
        });
    }

    fn components_banner(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.tools.ready_for_media() && self.tools.ready_for_transcribe() {
            return;
        }
        let mut missing = vec![];
        if self.tools.ffmpeg.is_none() || self.tools.ffprobe.is_none() {
            missing.push("ffmpeg");
        }
        if self.tools.whisper.is_none() {
            missing.push("whisper.cpp");
        }
        if self.tools.model.is_none() {
            missing.push("model Whisper (±470 MB)");
        }
        egui::Frame::new().fill(Color32::from_rgb(40, 34, 10)).corner_radius(6.0).inner_margin(10.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Komponen belum lengkap: {}.", missing.join(", "))).color(ACCENT));
                if ui.add_enabled(!self.busy(), egui::Button::new("Unduh komponen")).clicked() {
                    self.download(&ctx);
                }
                if ui.button("Periksa ulang").clicked() {
                    self.tools = Tools::locate(&self.base);
                }
            });
        });
    }

    fn preview_panel(&mut self, ui: &mut egui::Ui, chunks: &[Chunk]) {
        let ctx = ui.ctx().clone();
        // Ambil frame video bila waktunya berubah.
        if let Some(video) = &self.project.video {
            let need = self.frame.as_ref().map(|f| (f.0 - self.preview_t).abs() > 0.02).unwrap_or(true);
            if need && self.tools.ready_for_media() {
                match media::frame_at(&self.tools, video, self.preview_t, 720) {
                    Ok((w, h, data)) => self.frame = Some((self.preview_t, w, h, data)),
                    Err(_) => self.frame = Some((self.preview_t, 0, 0, vec![])),
                }
                self.preview_key.clear();
            }
        }
        let key = format!("{:.3}|{:?}|{}", self.preview_t, self.project.style, chunks.len() * 1000 + chunks.iter().map(|c| c.words.len()).sum::<usize>());
        if key != self.preview_key {
            if let Some((_, w, h, data)) = &self.frame {
                if *w > 0 {
                    let mut img = data.clone();
                    let r = Renderer::new(self.project.style.clone(), *w, *h, self.video_fps);
                    let strip = r.frame(chunks, self.preview_t);
                    composite(&mut img, *w, &strip, r.strip_y);
                    let ci = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], &img);
                    self.preview = Some(ctx.load_texture("pratinjau", ci, egui::TextureOptions::LINEAR));
                }
            }
            self.preview_key = key;
        }
        let avail = ui.available_size();
        if let Some(tex) = &self.preview {
            let s = tex.size_vec2();
            let k = ((avail.y - 40.0) / s.y).min(avail.x / s.x).max(0.05);
            ui.vertical_centered(|ui| {
                ui.image((tex.id(), s * k));
            });
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(avail.y * 0.35);
                ui.label(RichText::new("Pratinjau muncul setelah video dipilih").color(Color32::from_gray(120)));
            });
        }
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(format!("Detik {:.1} · klik timeline untuk pindah", self.preview_t)).size(12.5).color(Color32::from_gray(130)));
        });
    }

    fn tab_klip(&mut self, ui: &mut egui::Ui, placement: &Placement) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.busy() && self.tools.ready_for_media(), egui::Button::new("Pilih video")).clicked() {
                if let Some(p) = dialog().add_filter("Video", &["mp4", "mov", "mkv", "webm", "avi"]).pick_file() {
                    self.set_video(p, &ctx);
                }
            }
            match &self.project.video {
                Some(v) => ui.label(format!(
                    "{}  ·  {:.1} dtk  ·  {}×{}",
                    v.file_name().unwrap_or_default().to_string_lossy(),
                    self.video_dur,
                    self.video_size.0,
                    self.video_size.1
                )),
                None => ui.label(RichText::new("Belum ada video").color(Color32::from_gray(130))),
            };
        });
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(RichText::new("Mode penempatan").strong());
            let mut m = match self.project.mode {
                Mode::PerAdegan => 0,
                Mode::Berurutan { .. } => 1,
                Mode::Manual { .. } => 2,
            };
            let before = m;
            ui.selectable_value(&mut m, 0, "Per Adegan");
            ui.selectable_value(&mut m, 1, "Berurutan");
            ui.selectable_value(&mut m, 2, "Manual");
            if m != before {
                self.project.mode = match m {
                    0 => Mode::PerAdegan,
                    1 => Mode::Berurutan { start: 0.2, gap: 0.3 },
                    _ => Mode::Manual { starts: placement.items.iter().map(|p| p.start).collect() },
                };
            }
        });
        match &mut self.project.mode {
            Mode::PerAdegan => {
                ui.horizontal(|ui| {
                    ui.label(format!("{} adegan terdeteksi.", self.project.scenes.len()));
                    ui.label("Kepekaan");
                    ui.add(egui::Slider::new(&mut self.project.scene_threshold, 0.03..=0.4).fixed_decimals(2));
                    if ui.add_enabled(!self.busy() && self.project.video.is_some(), egui::Button::new("Deteksi ulang")).clicked() {
                        self.detect_scenes(&ctx);
                    }
                });
                ui.label(RichText::new("Klip ke-n mulai 0,2 detik setelah awal adegan ke-n. Kepekaan lebih kecil = lebih banyak adegan.").size(12.5).color(Color32::from_gray(130)));
            }
            Mode::Berurutan { start, gap } => {
                ui.horizontal(|ui| {
                    ui.label("Mulai di detik");
                    ui.add(egui::DragValue::new(start).range(0.0..=600.0).speed(0.05).fixed_decimals(2));
                    ui.label("Jeda antarklip");
                    ui.add(egui::DragValue::new(gap).range(0.0..=5.0).speed(0.05).fixed_decimals(2).suffix(" dtk"));
                });
            }
            Mode::Manual { starts } => {
                starts.resize(self.project.clips.len(), 0.0);
                ui.label(RichText::new("Atur detik mulai tiap klip di daftar bawah.").size(12.5).color(Color32::from_gray(130)));
            }
        }
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Klip suara ({})", self.project.clips.len())).strong());
            if ui.add_enabled(!self.busy() && self.tools.ready_for_media(), egui::Button::new("+ Tambah klip suara")).clicked() {
                if let Some(ps) = dialog().add_filter("Suara", &["wav", "mp3", "m4a", "flac", "ogg"]).pick_files() {
                    self.add_clips(ps);
                }
            }
        });
        let mut action: Option<(usize, i32)> = None;
        let scene_count = self.project.scenes.len();
        egui::ScrollArea::vertical().id_salt("klip").max_height(ui.available_height() - 70.0).show(ui, |ui| {
            egui::Grid::new("daftar-klip").striped(true).num_columns(6).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("#");
                ui.label("File");
                ui.label("Durasi");
                ui.label("Posisi");
                ui.label("Kecepatan");
                ui.label("");
                ui.end_row();
                let manual = matches!(self.project.mode, Mode::Manual { .. });
                for i in 0..self.project.clips.len() {
                    let c = &mut self.project.clips[i];
                    ui.label(format!("{}", i + 1));
                    ui.label(c.path.file_name().unwrap_or_default().to_string_lossy());
                    ui.label(format!("{:.2} dtk", c.played_dur()));
                    if manual {
                        if let Mode::Manual { starts } = &mut self.project.mode {
                            ui.add(egui::DragValue::new(&mut starts[i]).range(0.0..=3600.0).speed(0.05).fixed_decimals(2).suffix(" dtk"));
                        }
                    } else if let Some(p) = placement.items.get(i) {
                        let scene = if matches!(self.project.mode, Mode::PerAdegan) && i < scene_count { format!("adegan {} · ", i + 1) } else { String::new() };
                        ui.label(format!("{scene}{:.2} dtk", p.start));
                    } else {
                        ui.label("");
                    }
                    let c = &mut self.project.clips[i];
                    ui.add(egui::Slider::new(&mut c.speed, SPEED_MIN..=SPEED_MAX).fixed_decimals(2).suffix("×"));
                    ui.horizontal(|ui| {
                        if ui.small_button("Naik").clicked() && i > 0 {
                            action = Some((i, -1));
                        }
                        if ui.small_button("Turun").clicked() {
                            action = Some((i, 1));
                        }
                        if ui.small_button("Hapus").clicked() {
                            action = Some((i, 0));
                        }
                    });
                    ui.end_row();
                }
            });
        });
        match action {
            Some((i, 0)) => self.remove_clip(i),
            Some((i, d)) => {
                let j = i as i32 + d;
                if j >= 0 && (j as usize) < self.project.clips.len() {
                    self.move_clip(i, j as usize);
                }
            }
            None => {}
        }
        for w in &placement.warnings {
            ui.label(RichText::new(format!("Peringatan: {w}")).color(Color32::from_rgb(255, 170, 60)));
        }
    }

    fn tab_subtitle(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            let ok = !self.busy() && self.tools.ready_for_transcribe() && !self.project.clips.is_empty();
            if ui.add_enabled(ok, egui::Button::new("Transkripsi semua klip")).clicked() {
                self.transcribe_all(&ctx);
            }
            ui.label("Bahasa");
            egui::ComboBox::from_id_salt("bahasa").selected_text(match self.project.language.as_str() {
                "id" => "Indonesia",
                "en" => "Inggris",
                _ => "Otomatis",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.project.language, "id".to_string(), "Indonesia");
                ui.selectable_value(&mut self.project.language, "en".to_string(), "Inggris");
                ui.selectable_value(&mut self.project.language, "auto".to_string(), "Otomatis");
            });
            if ui.add_enabled(self.raw_words.iter().any(|w| w.is_some()), egui::Button::new("Ingat koreksi ke kamus")).on_hover_text("Kata yang kamu betulkan akan dibetulkan otomatis di video berikutnya").clicked() {
                self.learn_corrections();
            }
        });
        ui.separator();

        ui.label(RichText::new("Gaya").strong());
        let st = &mut self.project.style;
        ui.horizontal_wrapped(|ui| {
            for k in StyleKind::ALL {
                if ui.selectable_label(st.kind == k, k.label()).clicked() && st.kind != k {
                    *st = Style::preset(k);
                }
            }
        });
        egui::Grid::new("gaya").num_columns(4).spacing([14.0, 6.0]).show(ui, |ui| {
            ui.label("Warna sorot");
            ui.color_edit_button_srgb(&mut st.highlight);
            ui.label("Ukuran");
            ui.add(egui::Slider::new(&mut st.size, 40.0..=140.0).fixed_decimals(0));
            ui.end_row();
            ui.label("Kata per potongan");
            ui.add(egui::Slider::new(&mut st.max_words, 1..=8));
            ui.label("Posisi tinggi");
            ui.add(egui::Slider::new(&mut st.center_y, 300.0..=1750.0).fixed_decimals(0)).on_hover_text("Diukur pada video setinggi 1920 px; bawaan 1590");
            ui.end_row();
            ui.label("Huruf kapital");
            ui.checkbox(&mut st.uppercase, "");
            ui.end_row();
        });
        ui.separator();

        ui.label(RichText::new("Teks per klip (betulkan kata yang salah dengar)").strong());
        self.edit_text.resize(self.project.clips.len(), String::new());
        self.project.sync_words();
        egui::ScrollArea::vertical().id_salt("teks").show(ui, |ui| {
            for i in 0..self.project.clips.len() {
                let name = self.project.clips[i].path.file_name().unwrap_or_default().to_string_lossy().to_string();
                ui.label(RichText::new(format!("{}. {name}", i + 1)).color(Color32::from_gray(160)));
                if self.project.words[i].is_none() {
                    ui.label(RichText::new("Belum ditranskripsi").size(12.5).color(Color32::from_gray(110)));
                    continue;
                }
                let r = ui.add(egui::TextEdit::multiline(&mut self.edit_text[i]).desired_rows(2).desired_width(f32::INFINITY));
                if r.changed() {
                    let base = self.raw_words.get(i).cloned().flatten().or_else(|| self.project.words[i].clone()).unwrap_or_default();
                    let cur = self.project.words[i].clone().unwrap_or_default();
                    // Pakai waktu terbaru bila jumlah kata sama, kalau tidak bagi ulang dari rentang aslinya.
                    let src = if cur.len() == self.edit_text[i].split_whitespace().count() { cur } else { base };
                    self.project.words[i] = Some(retext(&src, &self.edit_text[i]));
                }
            }
        });
    }

    fn tab_audio(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Suara asli video (musik latar)").strong());
        let mut mute = self.project.bg_db.is_none();
        ui.horizontal(|ui| {
            if ui.radio(!mute, "Pertahankan, dikecilkan").clicked() {
                mute = false;
                if self.project.bg_db.is_none() {
                    self.project.bg_db = Some(-18.0);
                }
            }
            if ui.radio(mute, "Bisukan").clicked() {
                mute = true;
                self.project.bg_db = None;
            }
        });
        if let Some(db) = &mut self.project.bg_db {
            ui.add(egui::Slider::new(db, -40.0..=0.0).suffix(" dB").fixed_decimals(0).text("Volume"));
            ui.label(RichText::new("Bawaan -18 dB: musik terdengar tapi narasi tetap jelas.").size(12.5).color(Color32::from_gray(130)));
        }
        ui.separator();
        ui.label(RichText::new("Hasil akhir disetarakan ke -14 LUFS (standar YouTube), puncak -1 dB.").color(Color32::from_gray(160)));
    }

    fn tab_ekspor(&mut self, ui: &mut egui::Ui, placement: &Placement, chunks: &[Chunk]) {
        let ctx = ui.ctx().clone();
        ui.label(RichText::new("Ringkasan").strong());
        ui.label(format!("Video: {:.1} dtk · {} klip suara · {} potongan subtitle", self.video_dur, self.project.clips.len(), chunks.len()));
        ui.label(format!("Gaya: {}", self.project.style.kind.label()));
        ui.label(format!("Encoder: {}", if self.tools.ready_for_media() && padupilo::export::nvenc_available(&self.tools) { "GPU NVIDIA (NVENC)" } else { "CPU" }));
        if chunks.is_empty() && !self.project.clips.is_empty() {
            ui.label(RichText::new("Belum ada subtitle: video akan diekspor tanpa teks.").color(Color32::from_rgb(255, 170, 60)));
        }
        for w in &placement.warnings {
            ui.label(RichText::new(format!("Peringatan: {w}")).color(Color32::from_rgb(255, 170, 60)));
        }
        ui.checkbox(&mut self.project.export_srt, "Simpan juga file .srt");
        ui.label(RichText::new(format!("Hasil disimpan di {}", self.base.join("hasil").display())).size(12.5).color(Color32::from_gray(130)));
        ui.add_space(8.0);
        let can = !self.busy() && self.project.video.is_some() && self.tools.ready_for_media();
        if ui.add_enabled(can, egui::Button::new(RichText::new("Ekspor MP4").strong().color(Color32::BLACK)).fill(ACCENT).min_size(egui::vec2(160.0, 34.0))).clicked() {
            self.export(&ctx);
        }
        if let Some(p) = self.last_export.clone() {
            ui.horizontal(|ui| {
                ui.label(format!("Terakhir: {}", p.file_name().unwrap_or_default().to_string_lossy()));
                if ui.button("Buka folder").clicked() {
                    open_in_explorer(&p);
                }
            });
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_job(&ctx);

        // Seret file ke jendela: video atau klip suara.
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if !dropped.is_empty() && !self.busy() {
            let is_video = |p: &PathBuf| p.extension().map(|e| ["mp4", "mov", "mkv", "webm", "avi"].contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false);
            let (videos, audios): (Vec<PathBuf>, Vec<PathBuf>) = dropped.into_iter().partition(is_video);
            if let Some(v) = videos.into_iter().next() {
                self.set_video(v, &ctx);
            }
            if !audios.is_empty() {
                self.add_clips(audios);
            }
        }

        let placement = self.placement();
        let chunks = self.chunks(&placement);
        let panel = egui::Frame::new().fill(Color32::from_rgb(14, 14, 14)).inner_margin(10.0);

        egui::Panel::top("atas").frame(panel).show(ui, |ui| {
            self.top_bar(ui);
            self.components_banner(ui);
        });

        egui::Panel::bottom("bawah").frame(panel).show(ui, |ui| {
            if let Some(job) = &self.job {
                let (label, p) = job.snapshot();
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label(format!("{}: {label}", job.title));
                    ui.add(egui::ProgressBar::new(p).desired_width(260.0).show_percentage());
                    if ui.button("Batal").clicked() {
                        job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            } else if let Some((err, msg)) = &self.message {
                ui.label(RichText::new(msg).color(if *err { Color32::from_rgb(255, 110, 90) } else { Color32::from_gray(190) }));
            }
            if self.video_dur > 0.0 {
                if let Some(t) = timeline::show(ui, self.video_dur, &self.project.scenes, &placement.items, &chunks, self.preview_t) {
                    self.preview_t = t;
                }
            }
        });

        egui::Panel::left("pratinjau").frame(panel).resizable(true).default_size(380.0).show(ui, |ui| {
            self.preview_panel(ui, &chunks);
        });

        egui::CentralPanel::default().frame(panel).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (t, name) in [(Tab::Klip, "Klip Suara"), (Tab::Subtitle, "Subtitle"), (Tab::Audio, "Audio"), (Tab::Ekspor, "Ekspor")] {
                    let sel = self.tab == t;
                    let txt = RichText::new(name).strong().color(if sel { Color32::BLACK } else { Color32::from_gray(200) });
                    if ui.add(egui::Button::new(txt).fill(if sel { Color32::WHITE } else { Color32::from_gray(30) })).clicked() {
                        self.tab = t;
                    }
                }
            });
            ui.add_space(6.0);
            match self.tab {
                Tab::Klip => self.tab_klip(ui, &placement),
                Tab::Subtitle => self.tab_subtitle(ui),
                Tab::Audio => self.tab_audio(ui),
                Tab::Ekspor => self.tab_ekspor(ui, &placement, &chunks),
            }
        });
    }
}
