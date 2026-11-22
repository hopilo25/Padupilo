//! Lokasi alat bantu (ffmpeg, whisper) dan analisis video.

use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, Default)]
pub struct Tools {
    pub base: PathBuf,
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
    pub whisper: Option<PathBuf>,
    pub model: Option<PathBuf>,
}

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

/// Perintah tanpa jendela konsol yang berkedip di Windows.
pub fn command(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

fn find_named(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = vec![];
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            subdirs.push(p);
        } else if p.file_name().map(|f| f.to_string_lossy().eq_ignore_ascii_case(name)).unwrap_or(false) {
            return Some(p);
        }
    }
    if depth == 0 {
        return None;
    }
    subdirs.into_iter().find_map(|d| find_named(&d, name, depth - 1))
}

fn on_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).map(|d| d.join(format!("{name}{EXE}"))).find(|p| p.is_file())
}

impl Tools {
    /// Cari alat di folder aplikasi (bin/, model/); ffmpeg boleh juga dari PATH.
    pub fn locate(base: &Path) -> Tools {
        let bin = base.join("bin");
        let ffmpeg = find_named(&bin, &format!("ffmpeg{EXE}"), 3).or_else(|| on_path("ffmpeg"));
        let ffprobe = find_named(&bin, &format!("ffprobe{EXE}"), 3).or_else(|| on_path("ffprobe"));
        let whisper = find_named(&bin, &format!("whisper-cli{EXE}"), 4).or_else(|| on_path("whisper-cli"));
        let model = ["ggml-small.bin", "ggml-medium.bin", "ggml-base.bin"]
            .iter()
            .map(|m| base.join("model").join(m))
            .find(|p| p.is_file());
        Tools { base: base.to_path_buf(), ffmpeg, ffprobe, whisper, model }
    }

    pub fn ffmpeg(&self) -> Result<&Path> {
        self.ffmpeg.as_deref().ok_or_else(|| anyhow!("ffmpeg belum ada. Tekan \"Unduh komponen\"."))
    }
    pub fn ffprobe(&self) -> Result<&Path> {
        self.ffprobe.as_deref().ok_or_else(|| anyhow!("ffprobe belum ada. Tekan \"Unduh komponen\"."))
    }
    pub fn ready_for_media(&self) -> bool {
        self.ffmpeg.is_some() && self.ffprobe.is_some()
    }
    pub fn ready_for_transcribe(&self) -> bool {
        self.whisper.is_some() && self.model.is_some() && self.ffmpeg.is_some()
    }
}

pub struct VideoInfo {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub has_audio: bool,
}

pub fn probe(tools: &Tools, file: &Path) -> Result<VideoInfo> {
    let out = command(tools.ffprobe()?)
        .args(["-v", "error", "-show_entries", "format=duration:stream=codec_type,width,height,r_frame_rate", "-of", "json"])
        .arg(file)
        .output()
        .context("gagal menjalankan ffprobe")?;
    if !out.status.success() {
        bail!("File tidak bisa dibaca: {}", String::from_utf8_lossy(&out.stderr));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let duration: f64 = v["format"]["duration"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let mut info = VideoInfo { duration, width: 0, height: 0, fps: 30.0, has_audio: false };
    for s in v["streams"].as_array().cloned().unwrap_or_default() {
        match s["codec_type"].as_str() {
            Some("video") if info.width == 0 => {
                info.width = s["width"].as_u64().unwrap_or(0) as u32;
                info.height = s["height"].as_u64().unwrap_or(0) as u32;
                if let Some((a, b)) = s["r_frame_rate"].as_str().and_then(|r| r.split_once('/')) {
                    let (a, b): (f64, f64) = (a.parse().unwrap_or(30.0), b.parse().unwrap_or(1.0));
                    if b > 0.0 && a > 0.0 {
                        info.fps = a / b;
                    }
                }
            }
            Some("audio") => info.has_audio = true,
            _ => {}
        }
    }
    Ok(info)
}

pub fn duration(tools: &Tools, file: &Path) -> Result<f64> {
    Ok(probe(tools, file)?.duration)
}

/// Waktu-waktu awal adegan (selalu diawali 0). `threshold` 0.05–0.4; makin kecil makin peka.
pub fn detect_scenes(tools: &Tools, video: &Path, threshold: f64) -> Result<Vec<f64>> {
    let filter = format!("select='gt(scene,{threshold:.3})',showinfo");
    let out = command(tools.ffmpeg()?)
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(video)
        .args(["-an", "-vf", &filter, "-f", "null", "-"])
        .output()
        .context("gagal menjalankan ffmpeg")?;
    Ok(parse_scene_times(&String::from_utf8_lossy(&out.stderr)))
}

pub fn parse_scene_times(log: &str) -> Vec<f64> {
    let mut times = vec![0.0];
    for line in log.lines().filter(|l| l.contains("showinfo")) {
        if let Some(i) = line.find("pts_time:") {
            let rest = &line[i + 9..];
            let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            if let Ok(t) = num.parse::<f64>() {
                // Gabungkan potongan yang terlalu rapat (< 1 detik), misalnya efek kilat.
                if t - times.last().copied().unwrap_or(0.0) >= 1.0 {
                    times.push(t);
                }
            }
        }
    }
    times
}

/// Ambil satu frame (PNG RGBA) di detik `t`, untuk pratinjau.
pub fn frame_at(tools: &Tools, video: &Path, t: f64, max_h: u32) -> Result<(u32, u32, Vec<u8>)> {
    let info = probe(tools, video)?;
    let scale = (max_h as f64 / info.height.max(1) as f64).min(1.0);
    let (w, h) = (((info.width as f64 * scale) as u32) & !1, ((info.height as f64 * scale) as u32) & !1);
    let out = command(tools.ffmpeg()?)
        .args(["-v", "error", "-ss", &format!("{t:.3}"), "-i"])
        .arg(video)
        .args(["-frames:v", "1", "-vf", &format!("scale={w}:{h}"), "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .output()?;
    if out.stdout.len() != (w * h * 4) as usize {
        bail!("frame tidak terbaca");
    }
    Ok((w, h, out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_log_parsing_merges_close_cuts() {
        let log = "[Parsed_showinfo_1 @ 0x1] n:0 pts:9 pts_time:0.3 duration\n\
                   [Parsed_showinfo_1 @ 0x1] n:1 pts:10 pts_time:0.333333 x\n\
                   [Parsed_showinfo_1 @ 0x1] n:2 pts:300 pts_time:10 x\n\
                   [Parsed_showinfo_1 @ 0x1] n:3 pts:600 pts_time:20.5 x\n";
        assert_eq!(parse_scene_times(log), vec![0.0, 10.0, 20.5]);
    }
}
