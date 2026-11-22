//! Uji ujung-ke-ujung dengan video "Cara Kerja FYP". Jalan bila PADUPILO_FYP_DIR berisi
//! video, 6 klip, dan asr.json (kata per klip). Contoh: PADUPILO_FYP_DIR=/mnt/user-data/uploads

use padupilo::export::{run_export, ExportJob, PlacedClip};
use padupilo::media::{detect_scenes, duration, probe, Tools};
use padupilo::placement::{natural_order, place, Clip, Mode};
use padupilo::render::{Style, StyleKind};
use padupilo::subs::{make_chunks, timeline_words};
use padupilo::transcribe::Word;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

#[test]
fn fyp_end_to_end() {
    let Ok(dir) = std::env::var("PADUPILO_FYP_DIR") else { return };
    let dir = PathBuf::from(dir);
    let tools = Tools::locate(&dir);
    let video = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).find(|p| p.extension().map(|e| e == "mp4").unwrap_or(false)).unwrap();
    let mut wavs: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().contains("Klip")).collect();
    natural_order(&mut wavs);
    assert_eq!(wavs.len(), 6);

    let scenes = detect_scenes(&tools, &video, 0.08).unwrap();
    assert_eq!(scenes.iter().map(|s| s.round() as i64).collect::<Vec<_>>(), vec![0, 10, 20, 30, 40, 50]);

    let speeds = [0.926, 1.0, 0.926, 0.926, 1.0, 0.926];
    let clips: Vec<Clip> = wavs.iter().zip(speeds).map(|(p, s)| Clip { path: p.clone(), dur: duration(&tools, p).unwrap(), speed: s }).collect();
    let vdur = duration(&tools, &video).unwrap();
    let placement = place(&clips, &scenes, vdur, &Mode::PerAdegan);
    assert!(placement.warnings.is_empty(), "{:?}", placement.warnings);

    let asr: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(std::env::var("PADUPILO_ASR").unwrap()).unwrap()).unwrap();
    let per_clip: Vec<(Vec<Word>, f64, f64)> = (0..6)
        .map(|i| {
            let words = asr[(i + 1).to_string()]["chunks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| Word { text: c["text"].as_str().unwrap().trim().to_string(), start: c["timestamp"][0].as_f64().unwrap(), end: c["timestamp"][1].as_f64().unwrap() })
                .collect();
            (words, placement.items[i].start, speeds[i])
        })
        .collect();
    let kind = match std::env::var("PADUPILO_STYLE").as_deref() {
        Ok("karaoke") => StyleKind::Karaoke,
        Ok("minimal") => StyleKind::Minimal,
        _ => StyleKind::Beast,
    };
    let style = Style::preset(kind);
    let chunks = make_chunks(&timeline_words(&per_clip), style.max_words, style.max_chars());
    let out = std::env::temp_dir().join(format!("padupilo-fyp-{kind:?}.mp4"));
    let job = ExportJob {
        video: video.clone(),
        clips: clips.iter().zip(&placement.items).map(|(c, p)| PlacedClip { path: c.path.clone(), start: p.start, speed: c.speed }).collect(),
        bg_db: Some(-18.0),
        chunks,
        style,
        out_mp4: out.clone(),
        out_srt: Some(out.with_extension("srt")),
        work_dir: std::env::temp_dir().join("padupilo-kerja"),
    };
    let t0 = std::time::Instant::now();
    run_export(&tools, &job, &|_, _| {}, &AtomicBool::new(false)).unwrap();
    eprintln!("ekspor {:?}: {:.1} dtk -> {}", kind, t0.elapsed().as_secs_f64(), out.display());
    let info = probe(&tools, &out).unwrap();
    assert!((info.duration - vdur).abs() < 0.1, "durasi {}", info.duration);
    assert!(info.has_audio);
}
