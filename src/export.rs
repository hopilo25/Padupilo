//! Campur suara, setarakan kerasnya (-14 LUFS), gambar subtitle, lalu gabungkan jadi MP4.

use crate::media::{command, probe, Tools};
use crate::render::{Renderer, Style};
use crate::subs::{to_srt, Chunk};
use anyhow::{anyhow, bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug)]
pub struct PlacedClip {
    pub path: PathBuf,
    pub start: f64,
    pub speed: f64,
}

#[derive(Clone, Debug)]
pub struct ExportJob {
    pub video: PathBuf,
    pub clips: Vec<PlacedClip>,
    /// Suara asli video dalam dB (-18 = dikecilkan). `None` = dibisukan.
    pub bg_db: Option<f64>,
    pub chunks: Vec<Chunk>,
    pub style: Style,
    pub out_mp4: PathBuf,
    pub out_srt: Option<PathBuf>,
    pub work_dir: PathBuf,
}

pub type Progress<'a> = &'a dyn Fn(&str, f32);

fn run_ok(mut c: std::process::Command, what: &str) -> Result<std::process::Output> {
    let out = c.output().with_context(|| format!("gagal menjalankan ffmpeg ({what})"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("{what} gagal: {}", err.lines().rev().take(3).collect::<Vec<_>>().join(" | "));
    }
    Ok(out)
}

/// Rangkaian filter ffmpeg untuk mencampur suara latar + klip.
pub fn mix_filter(n_clips: usize, clips: &[PlacedClip], bg_db: Option<f64>, has_bg: bool) -> String {
    let mut f = String::new();
    let mut labels = vec![];
    if has_bg {
        if let Some(db) = bg_db {
            f.push_str(&format!("[0:a]volume={db:.1}dB,aresample=48000,aformat=channel_layouts=stereo[bg];"));
            labels.push("[bg]".to_string());
        }
    }
    for (i, c) in clips.iter().enumerate().take(n_clips) {
        let ms = (c.start.max(0.0) * 1000.0).round() as u64;
        let sp = c.speed.clamp(0.5, 2.0);
        let tempo = if (sp - 1.0).abs() > 1e-3 { format!(",atempo={sp:.4}") } else { String::new() };
        f.push_str(&format!(
            "[{}:a]aresample=48000,aformat=channel_layouts=stereo{tempo},highpass=f=70,adelay={ms}|{ms}[v{i}];",
            i + 1
        ));
        labels.push(format!("[v{i}]"));
    }
    if labels.is_empty() {
        f.push_str("anullsrc=r=48000:cl=stereo[m]");
    } else {
        f.push_str(&format!("{}amix=inputs={}:normalize=0:duration=longest,apad[m]", labels.concat(), labels.len()));
    }
    f
}

fn loudnorm_measure(tools: &Tools, wav: &Path) -> Result<serde_json::Value> {
    let mut c = command(tools.ffmpeg()?);
    c.args(["-hide_banner", "-nostats", "-i"]).arg(wav).args(["-af", "loudnorm=I=-14:TP=-1.5:LRA=11:print_format=json", "-f", "null", "-"]);
    let out = run_ok(c, "pengukuran kerasnya suara")?;
    let err = String::from_utf8_lossy(&out.stderr);
    let start = err.rfind('{').ok_or_else(|| anyhow!("hasil loudnorm tidak terbaca"))?;
    let end = err.rfind('}').ok_or_else(|| anyhow!("hasil loudnorm tidak terbaca"))?;
    Ok(serde_json::from_str(&err[start..=end])?)
}

static NVENC_OK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Coba encoder GPU NVIDIA sekali; kalau gagal pakai CPU (libx264).
pub fn nvenc_available(tools: &Tools) -> bool {
    *NVENC_OK.get_or_init(|| {
        let Ok(ff) = tools.ffmpeg() else { return false };
        command(ff)
            .args(["-v", "error", "-f", "lavfi", "-i", "color=s=256x256:d=0.2", "-c:v", "h264_nvenc", "-f", "null", "-"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

pub fn run_export(tools: &Tools, job: &ExportJob, progress: Progress, cancel: &AtomicBool) -> Result<()> {
    std::fs::create_dir_all(&job.work_dir)?;
    if let Some(d) = job.out_mp4.parent() {
        std::fs::create_dir_all(d)?;
    }
    let info = probe(tools, &job.video)?;
    let (w, h, fps, dur) = (info.width & !1, info.height & !1, info.fps, info.duration);
    if w == 0 || dur <= 0.0 {
        bail!("Video tidak terbaca");
    }

    // 1. Campur suara.
    progress("Mencampur suara", 0.02);
    let mix = job.work_dir.join("campuran.wav");
    let mut c = command(tools.ffmpeg()?);
    c.args(["-v", "error", "-y", "-i"]).arg(&job.video);
    for cl in &job.clips {
        c.arg("-i").arg(&cl.path);
    }
    c.args(["-filter_complex", &mix_filter(job.clips.len(), &job.clips, job.bg_db, info.has_audio)]);
    c.args(["-map", "[m]", "-t", &format!("{dur:.3}"), "-ar", "48000", "-c:a", "pcm_s16le"]).arg(&mix);
    run_ok(c, "pencampuran suara")?;

    // 2. Setarakan ke -14 LUFS (dua tahap).
    progress("Menyetarakan kerasnya suara", 0.06);
    let m = loudnorm_measure(tools, &mix)?;
    let g = |k: &str| m[k].as_str().unwrap_or("0").to_string();
    let master = job.work_dir.join("master.wav");
    let ln = format!(
        "loudnorm=I=-14:TP=-1.5:LRA=11:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true,aresample=48000",
        g("input_i"), g("input_tp"), g("input_lra"), g("input_thresh"), g("target_offset")
    );
    let mut c = command(tools.ffmpeg()?);
    c.args(["-v", "error", "-y", "-i"]).arg(&mix).args(["-af", &ln, "-c:a", "pcm_s16le"]).arg(&master);
    run_ok(c, "penyetaraan suara")?;

    // 3. Gambar subtitle per frame dan gabungkan.
    let renderer = Renderer::new(job.style.clone(), w, h, fps);
    let total_frames = (dur * fps).round() as u64;
    let enc: Vec<&str> = if nvenc_available(tools) {
        vec!["-c:v", "h264_nvenc", "-preset", "p5", "-rc", "vbr", "-cq", "19", "-b:v", "0"]
    } else {
        vec!["-c:v", "libx264", "-preset", "veryfast", "-crf", "18"]
    };
    let log_path = job.work_dir.join("ffmpeg-ekspor.log");
    let log = std::fs::File::create(&log_path)?;
    let mut c = command(tools.ffmpeg()?);
    c.args(["-v", "error", "-y", "-i"]).arg(&job.video);
    c.args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{}", renderer.strip_h), "-r", &format!("{fps}"), "-i", "-", "-i"]).arg(&master);
    c.args(["-filter_complex", &format!("[0:v]scale={w}:{h}[base];[base][1:v]overlay=0:{}:format=auto,format=yuv420p[v]", renderer.strip_y)]);
    c.args(["-map", "[v]", "-map", "2:a"]).args(&enc);
    c.args(["-c:a", "aac", "-b:a", "256k", "-movflags", "+faststart", "-t", &format!("{dur:.3}")]).arg(&job.out_mp4);
    c.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::from(log));
    let mut child = c.spawn().context("gagal menjalankan ffmpeg (ekspor)")?;
    {
        let mut stdin = child.stdin.take().unwrap();
        let blank = vec![0u8; (w * renderer.strip_h * 4) as usize];
        for i in 0..total_frames {
            if cancel.load(Ordering::Relaxed) {
                drop(stdin);
                let _ = child.kill();
                bail!("Ekspor dibatalkan");
            }
            let t = i as f64 / fps;
            let frame = if job.chunks.is_empty() { blank.clone() } else { renderer.frame(&job.chunks, t) };
            if stdin.write_all(&frame).is_err() {
                break; // ffmpeg berhenti; pesan errornya dibaca di bawah
            }
            if i % 15 == 0 {
                progress("Membuat video", 0.1 + 0.88 * i as f32 / total_frames.max(1) as f32);
            }
        }
    }
    let st = child.wait()?;
    if !st.success() {
        let err = std::fs::read_to_string(&log_path).unwrap_or_default();
        bail!("Pembuatan video gagal: {}", err.lines().rev().take(3).collect::<Vec<_>>().join(" | "));
    }
    if let Some(srt) = &job.out_srt {
        std::fs::write(srt, to_srt(&job.chunks))?;
    }
    progress("Selesai", 1.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_has_delay_tempo_and_bg() {
        let clips = vec![
            PlacedClip { path: "a.wav".into(), start: 0.2, speed: 1.0 },
            PlacedClip { path: "b.wav".into(), start: 10.2, speed: 0.926 },
        ];
        let f = mix_filter(2, &clips, Some(-18.0), true);
        assert!(f.starts_with("[0:a]volume=-18.0dB"));
        assert!(f.contains("[2:a]aresample=48000,aformat=channel_layouts=stereo,atempo=0.9260,highpass=f=70,adelay=10200|10200[v1]"));
        assert!(f.ends_with("[bg][v0][v1]amix=inputs=3:normalize=0:duration=longest,apad[m]"));
        let muted = mix_filter(2, &clips, None, true);
        assert!(!muted.contains("[bg]"));
    }
}
