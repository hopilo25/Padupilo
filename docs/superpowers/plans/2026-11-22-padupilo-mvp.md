# Padupilo v0.1 (MVP) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A portable Windows app that places voice clips on a video (Per Adegan / Berurutan), transcribes them, and exports an MP4 with animated word-highlight subtitles (Beast, Karaoke, Minimal hopilo) plus .srt.

**Architecture:** One Rust crate: `padupilo` lib (pure logic + ffmpeg/whisper-cli orchestration) and an eframe/egui binary. ffmpeg, ffprobe, whisper-cli and the ggml model are external files in the app folder, downloaded by the app on first run. Subtitles are rasterised per frame in Rust (ab_glyph + tiny-skia) and piped as raw RGBA into ffmpeg, as the proven Python prototype did.

**Tech Stack:** Rust 2021, eframe 0.37, ab_glyph, tiny-skia, serde/serde_json, ureq, zip, rfd. External: ffmpeg/ffprobe, whisper.cpp `whisper-cli`, `ggml-small.bin`.

**Spec:** PRD Padupilo (claude.ai artifact 7dbf22b4-f042-4f96-b726-6f9f65f6e509), sections v0.1.

## Global Constraints

- UI text in Indonesian; dark theme (hopilo black/white), accent configurable.
- Portable: every path resolved relative to the exe folder (`bin/`, `model/`, `font/`, `kamus/`, `proyek/`, `hasil/`).
- Per Adegan: clip n starts at scene n start + 0.2 s. Berurutan: start offset (default 0.0 s), gap default 0.3 s.
- Clip speed range 0.85–1.15, pitch preserved (ffmpeg `atempo`).
- Background audio default −18 dB; master −14 LUFS, true peak −1 dB, two-pass loudnorm.
- Subtitle zone default centre y = 1590 on 1080×1920; scaled by height for other sizes.
- Highlight timing within ±2 frames of word start.

## Review Focus

- Scene count ≠ clip count → warning, extra clips placed sequentially after the last scene, never silently dropped.
- Clip longer than its scene / total voice longer than video → warning listed before export; export still allowed.
- Filenames without numbers or mixed `Klip_3` / `Klip10` → natural numeric order, ties by name.
- Missing components (ffmpeg, whisper, model) → buttons disabled with a clear message + "Unduh komponen".
- Paths with spaces / non-ASCII (Windows user folders) → every ffmpeg call uses argument vectors, never a shell string.

---

### Task 1: Placement core
**Files:** `src/placement.rs`, tests inline.
**Produces:** `natural_order(paths: &mut Vec<PathBuf>)`, `struct Clip {path, dur, speed}`, `enum Mode {PerAdegan, Berurutan{start:f64, gap:f64}}`, `place(clips:&[Clip], scenes:&[f64], video_dur:f64, mode:&Mode) -> Placement {items: Vec<Placed{start:f64, dur:f64}>, warnings: Vec<String>}` (dur = clip.dur / speed).
- [ ] Tests: FYP case (6 clips, scenes 0,10..50) → starts 0.2,10.2,…; clip 2 9.76 s fits; 7 clips/6 scenes → warning + 7th after 6th; Berurutan gap 0.3; overflow warning; natural order `Klip1, Klip2, Klip_3, Klip10`.

### Task 2: Media probing & scene detection
**Files:** `src/media.rs`. **Produces:** `Tools {ffmpeg, ffprobe, whisper, model}` + `Tools::locate(base:&Path)`, `duration(&Tools,&Path)->Result<f64>`, `detect_scenes(&Tools,&Path, threshold:f64)->Result<Vec<f64>>` (starts at 0, merges cuts < 1 s apart, uses `select='gt(scene,T)',showinfo`).
- [ ] Integration test on the FYP video → `[0,10,20,30,40,50]`.

### Task 3: Transcription & dictionary
**Files:** `src/transcribe.rs`, `src/dict.rs`. **Produces:** `struct Word {text, start, end}`; `parse_whisper_json(&str)->Result<Vec<Word>>` (whisper-cli `-oj -ml 1 -sow` format: `transcription[].offsets.{from,to}` ms + `text`); `transcribe(&Tools, clip:&Path, lang:&str)->Result<Vec<Word>>`; `Dict {map}` load/save JSON, `apply(&self, word)->String` (case-insensitive on stripped core, keeps punctuation).
- [ ] Tests: parse fixture; empty/whitespace tokens skipped; dict FVP→FYP keeps trailing comma.

### Task 4: Chunking & SRT
**Files:** `src/subs.rs`. **Produces:** `struct TimedWord {text, start, end, clip:usize}`, `struct Chunk {words: Vec<TimedWord>, end:f64}`, `make_chunks(words, max_words, max_chars) -> Vec<Chunk>` (break after `,.?!`, at clip change; chunk end = next chunk start in same clip, capped last.end+0.6), `to_srt(&[Chunk])->String`.
- [ ] Tests: FYP clip 1 words → first chunk "Pernah merasa"; srt timestamps `00:00:00,620 --> …`.

### Task 5: Subtitle renderer (3 styles)
**Files:** `src/render.rs`. **Produces:** `enum StyleKind {Beast, Karaoke, Minimal}`, `struct Style {kind, highlight:[u8;3], size:f32, center_y:f32, uppercase:bool, max_words:usize}`, `struct Renderer` with `new(font_bytes, style, width, strip_h)` and `frame(&self, chunks:&[Chunk], t:f64) -> Vec<u8>` (RGBA strip). Beast = prototype (pop 0.55→1.12→0.97→1 over 8 frames, ±1.5° tilt alternating, active word highlight ×1.12, black stroke + soft shadow). Karaoke = one line, colour fill sweeps through active word. Minimal = white text on rounded black 60% box, fade+rise 6 frames.
- [ ] Tests: blank frame when no chunk; non-transparent pixels when chunk active; highlight colour present in Beast frame at active word.

### Task 6: Audio mix & export pipeline
**Files:** `src/export.rs`, `src/audio.rs`. **Produces:** `struct ExportJob {video, placed: Vec<(PathBuf, start, speed)>, bg_db:f64, chunks, style, out_mp4, out_srt:Option}`; `run_export(&Tools, &ExportJob, progress:&dyn Fn(f32)) -> Result<()>`: mix (adelay, atempo, amix normalize=0) → loudnorm pass 1 JSON → pass 2 linear → render pipe → mux (`h264_nvenc` if listed in `ffmpeg -encoders`, else libx264 crf 18), `+faststart`.
- [ ] Integration test (Linux, real FYP files, words from fixture): output duration 60 s ±0.05, loudness −14 ±1 LUFS.

### Task 7: Project file & setup downloader
**Files:** `src/project.rs`, `src/setup.rs`. **Produces:** `Project` (serde) save/load `.padupilo`; `download_components(base, progress)` (ffmpeg essentials zip from gyan.dev; whisper.cpp latest Windows release asset via GitHub API, prefer `cublas`/`cuda` asset if NVIDIA present else `bin-x64`; `ggml-small.bin` from Hugging Face; Montserrat Black bundled in repo `font/`).
- [ ] Test: project round-trip.

### Task 8: GUI
**Files:** `src/main.rs`, `src/ui/*.rs`. Window per PRD sketch: top bar (Buka/Simpan/Ekspor), left preview (still frame + subtitle at chosen second via slider), right tabs Klip Suara / Subtitle / Audio / Ekspor, bottom timeline (scenes, clip blocks, chunks). Long jobs on a worker thread with progress bar.
- [ ] Manual check: build release; launch; run FYP project end-to-end.

### Task 9: Packaging
**Files:** `PANDUAN.md`, `.github/workflows/windows.yml` (builds `Padupilo.exe` on windows-latest and uploads portable zip artifact), `README.md`.
- [ ] Verify workflow YAML parses; zip layout matches PRD folder list.
