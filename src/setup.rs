//! Mengunduh komponen (ffmpeg, whisper.cpp, model) ke folder aplikasi saat pertama dipakai.

use anyhow::{anyhow, bail, Context, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const FFMPEG_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
pub const WHISPER_RELEASE_API: &str = "https://api.github.com/repos/ggml-org/whisper.cpp/releases/latest";
pub const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin";

pub type Progress<'a> = &'a dyn Fn(&str, f32);

fn download(url: &str, dest: &Path, label: &str, progress: Progress) -> Result<()> {
    let mut resp = ureq::get(url)
        .header("User-Agent", "hopilo-padupilo")
        .call()
        .with_context(|| format!("gagal mengunduh {label}"))?;
    let total: u64 = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let tmp = dest.with_extension("unduh");
    let mut f = std::fs::File::create(&tmp)?;
    let mut reader = resp.body_mut().as_reader();
    let mut buf = vec![0u8; 1 << 16];
    let mut got: u64 = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n])?;
        got += n as u64;
        if total > 0 {
            progress(&format!("Mengunduh {label} ({} / {} MB)", got / 1_048_576, total / 1_048_576), got as f32 / total as f32);
        } else {
            progress(&format!("Mengunduh {label} ({} MB)", got / 1_048_576), 0.5);
        }
    }
    drop(f);
    if total > 0 && got != total {
        bail!("Unduhan {label} terputus. Coba lagi.");
    }
    std::fs::rename(tmp, dest)?;
    Ok(())
}

/// Ekstrak zip. `only`: bila ada, hanya file dengan nama tersebut (tanpa folder).
fn unzip(zip_path: &Path, dest: &Path, only: Option<&[&str]>) -> Result<()> {
    let f = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(f)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let Some(rel) = entry.enclosed_name() else { continue };
        let out: PathBuf = match only {
            Some(names) => {
                let name = rel.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                    continue;
                }
                dest.join(name)
            }
            None => dest.join(rel),
        };
        if let Some(d) = out.parent() {
            std::fs::create_dir_all(d)?;
        }
        let mut o = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut o)?;
    }
    Ok(())
}

fn has_nvidia() -> bool {
    crate::media::command(Path::new("nvidia-smi"))
        .arg("-L")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Pilih aset Windows dari rilis whisper.cpp: versi CUDA bila ada GPU NVIDIA.
pub fn pick_whisper_asset(names: &[String], nvidia: bool) -> Option<String> {
    let win: Vec<&String> = names.iter().filter(|n| n.ends_with("bin-x64.zip")).collect();
    let cuda = win.iter().filter(|n| n.contains("cublas") || n.contains("cuda")).max();
    let cpu = win.iter().find(|n| n.as_str() == "whisper-bin-x64.zip").or_else(|| win.iter().find(|n| !n.contains("cublas") && !n.contains("cuda")));
    if nvidia { cuda.or(cpu).map(|s| s.to_string()) } else { cpu.or(cuda).map(|s| s.to_string()) }
}

pub fn download_components(base: &Path, progress: Progress) -> Result<()> {
    if !cfg!(windows) {
        bail!("Unduh otomatis hanya untuk Windows. Pasang ffmpeg dan whisper-cli lewat paket sistem.");
    }
    let bin = base.join("bin");
    let tmp = base.join("sementara");
    std::fs::create_dir_all(&bin)?;
    std::fs::create_dir_all(&tmp)?;
    std::fs::create_dir_all(base.join("model"))?;

    if !bin.join("ffmpeg.exe").is_file() {
        let z = tmp.join("ffmpeg.zip");
        download(FFMPEG_URL, &z, "ffmpeg", progress)?;
        progress("Mengekstrak ffmpeg", 1.0);
        unzip(&z, &bin, Some(&["ffmpeg.exe", "ffprobe.exe"]))?;
        let _ = std::fs::remove_file(z);
    }

    if crate::media::Tools::locate(base).whisper.is_none() {
        progress("Mencari whisper.cpp terbaru", 0.0);
        let mut resp = ureq::get(WHISPER_RELEASE_API).header("User-Agent", "hopilo-padupilo").call()?;
        let v: serde_json::Value = serde_json::from_reader(resp.body_mut().as_reader())?;
        let assets = v["assets"].as_array().ok_or_else(|| anyhow!("daftar rilis whisper.cpp tidak terbaca"))?;
        let names: Vec<String> = assets.iter().filter_map(|a| a["name"].as_str().map(String::from)).collect();
        let pick = pick_whisper_asset(&names, has_nvidia()).ok_or_else(|| anyhow!("aset Windows whisper.cpp tidak ditemukan"))?;
        let url = assets
            .iter()
            .find(|a| a["name"].as_str() == Some(pick.as_str()))
            .and_then(|a| a["browser_download_url"].as_str())
            .ok_or_else(|| anyhow!("tautan whisper.cpp tidak ditemukan"))?
            .to_string();
        let z = tmp.join("whisper.zip");
        download(&url, &z, "whisper.cpp", progress)?;
        progress("Mengekstrak whisper.cpp", 1.0);
        unzip(&z, &bin.join("whisper"), None)?;
        let _ = std::fs::remove_file(z);
    }

    let model = base.join("model").join("ggml-small.bin");
    if crate::media::Tools::locate(base).model.is_none() {
        download(MODEL_URL, &model, "model Whisper small", progress)?;
    }
    let _ = std::fs::remove_dir_all(&tmp);
    progress("Komponen siap", 1.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_cuda_only_with_nvidia() {
        let names: Vec<String> = ["whisper-bin-Win32.zip", "whisper-bin-x64.zip", "whisper-blas-bin-x64.zip", "whisper-cublas-12.4.0-bin-x64.zip", "whisper-cublas-11.8.0-bin-x64.zip", "whisper-v1.8.0-xcframework.zip"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(pick_whisper_asset(&names, true).unwrap(), "whisper-cublas-12.4.0-bin-x64.zip");
        assert_eq!(pick_whisper_asset(&names, false).unwrap(), "whisper-bin-x64.zip");
    }
}
