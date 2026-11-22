//! Mengunduh komponen (ffmpeg, whisper.cpp, model) ke folder aplikasi saat pertama dipakai.

use anyhow::{anyhow, bail, Context, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const FFMPEG_URLS: [&str; 2] = [
    "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip",
    // Cadangan bila gyan.dev tidak bisa dijangkau.
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
];
/// Daftar rilis, bukan hanya "latest": rilis terbaru whisper.cpp kadang hanya berisi kode sumber.
pub const WHISPER_RELEASES_API: &str = "https://api.github.com/repos/ggml-org/whisper.cpp/releases?per_page=30";
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

fn is_windows_x64_zip(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".zip") && n.contains("x64") && !n.contains("arm64") && !n.contains("ubuntu") && !n.contains("xcframework")
}

fn is_cuda(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("cuda") || n.contains("cublas")
}

/// Pilih aset Windows x64 dari satu rilis whisper.cpp: versi CUDA bila ada GPU NVIDIA.
pub fn pick_whisper_asset(names: &[String], nvidia: bool) -> Option<String> {
    let win: Vec<&String> = names.iter().filter(|n| is_windows_x64_zip(n)).collect();
    let cuda = win.iter().filter(|n| is_cuda(n)).max();
    let cpu = win
        .iter()
        .find(|n| n.as_str() == "whisper-bin-x64.zip")
        .or_else(|| win.iter().find(|n| !is_cuda(n) && !n.contains("blas")))
        .or_else(|| win.iter().find(|n| !is_cuda(n)));
    if nvidia { cuda.or(cpu).map(|s| s.to_string()) } else { cpu.or(cuda).map(|s| s.to_string()) }
}

/// Dari daftar rilis (terbaru dulu), ambil rilis pertama yang punya aset Windows: (tag, url).
pub fn pick_release_asset(releases: &serde_json::Value, nvidia: bool) -> Option<(String, String)> {
    for r in releases.as_array()? {
        if r["draft"].as_bool().unwrap_or(false) {
            continue;
        }
        let assets = r["assets"].as_array().cloned().unwrap_or_default();
        let names: Vec<String> = assets.iter().filter_map(|a| a["name"].as_str().map(String::from)).collect();
        if let Some(pick) = pick_whisper_asset(&names, nvidia) {
            let url = assets.iter().find(|a| a["name"].as_str() == Some(pick.as_str()))?["browser_download_url"].as_str()?.to_string();
            return Some((r["tag_name"].as_str().unwrap_or("").to_string(), url));
        }
    }
    None
}

fn get_ffmpeg(bin: &Path, tmp: &Path, progress: Progress) -> Result<()> {
    let mut errors = vec![];
    for url in FFMPEG_URLS {
        let z = tmp.join("ffmpeg.zip");
        let r = download(url, &z, "ffmpeg", progress).and_then(|_| {
            progress("Mengekstrak ffmpeg", 1.0);
            unzip(&z, bin, Some(&["ffmpeg.exe", "ffprobe.exe"]))
        });
        let _ = std::fs::remove_file(&z);
        match r {
            Ok(_) if bin.join("ffmpeg.exe").is_file() && bin.join("ffprobe.exe").is_file() => return Ok(()),
            Ok(_) => errors.push(format!("{url}: ffmpeg.exe tidak ada di dalam zip")),
            Err(e) => errors.push(format!("{url}: {e:#}")),
        }
    }
    bail!("ffmpeg gagal diunduh dari semua sumber:\n{}", errors.join("\n"))
}

fn get_whisper(base: &Path, bin: &Path, tmp: &Path, progress: Progress) -> Result<()> {
    progress("Mencari whisper.cpp untuk Windows", 0.0);
    let mut resp = ureq::get(WHISPER_RELEASES_API)
        .header("User-Agent", "hopilo-padupilo")
        .header("Accept", "application/vnd.github+json")
        .call()
        .context("daftar rilis whisper.cpp di GitHub tidak bisa dibuka")?;
    let releases: serde_json::Value = serde_json::from_reader(resp.body_mut().as_reader()).context("daftar rilis whisper.cpp tidak terbaca")?;
    let (tag, url) = pick_release_asset(&releases, has_nvidia()).ok_or_else(|| anyhow!("tidak ada rilis whisper.cpp yang berisi versi Windows"))?;
    let z = tmp.join("whisper.zip");
    download(&url, &z, &format!("whisper.cpp {tag}"), progress)?;
    progress("Mengekstrak whisper.cpp", 1.0);
    unzip(&z, &bin.join("whisper"), None)?;
    let _ = std::fs::remove_file(z);
    if crate::media::Tools::locate(base).whisper.is_none() {
        bail!("whisper-cli.exe tidak ditemukan di dalam {url}");
    }
    Ok(())
}

/// Unduh semua komponen yang belum ada. Tiap komponen dicoba sendiri-sendiri,
/// jadi kegagalan satu komponen tidak menghentikan yang lain.
pub fn download_components(base: &Path, progress: Progress) -> Result<()> {
    if !cfg!(windows) {
        bail!("Unduh otomatis hanya untuk Windows. Pasang ffmpeg dan whisper-cli lewat paket sistem.");
    }
    let bin = base.join("bin");
    let tmp = base.join("sementara");
    std::fs::create_dir_all(&bin)?;
    std::fs::create_dir_all(&tmp)?;
    std::fs::create_dir_all(base.join("model"))?;
    let mut errors: Vec<String> = vec![];

    let tools = crate::media::Tools::locate(base);
    if tools.ffmpeg.is_none() || tools.ffprobe.is_none() {
        if let Err(e) = get_ffmpeg(&bin, &tmp, progress) {
            errors.push(format!("{e:#}"));
        }
    }
    if crate::media::Tools::locate(base).whisper.is_none() {
        if let Err(e) = get_whisper(base, &bin, &tmp, progress) {
            errors.push(format!("whisper.cpp: {e:#}"));
        }
    }
    if crate::media::Tools::locate(base).model.is_none() {
        let model = base.join("model").join("ggml-small.bin");
        if let Err(e) = download(MODEL_URL, &model, "model Whisper small", progress) {
            errors.push(format!("model Whisper: {e:#}"));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    if !errors.is_empty() {
        let msg = errors.join("\n");
        let _ = std::fs::write(base.join("log-unduhan.txt"), &msg);
        bail!("{msg}");
    }
    progress("Komponen siap", 1.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const B5454: [&str; 12] = [
        "whisper-b5454-xcframework.zip",
        "whisper-bin-ubuntu-arm64.tar.gz",
        "whisper-bin-ubuntu-x64.tar.gz",
        "whisper-bin-win-cpu-arm64.zip",
        "whisper-bin-win-cuda-11.8.0-x64.zip",
        "whisper-bin-win-cuda-12.4.0-x64.zip",
        "whisper-bin-win-cuda-13.4-arm64.zip",
        "whisper-bin-win-opencl-adreno-arm64.zip",
        "whisper-bin-Win32.zip",
        "whisper-bin-x64.zip",
        "whisper-blas-bin-Win32.zip",
        "whisper-blas-bin-x64.zip",
    ];

    #[test]
    fn picks_new_asset_names() {
        let names: Vec<String> = B5454.iter().map(|s| s.to_string()).collect();
        assert_eq!(pick_whisper_asset(&names, true).unwrap(), "whisper-bin-win-cuda-12.4.0-x64.zip");
        assert_eq!(pick_whisper_asset(&names, false).unwrap(), "whisper-bin-x64.zip");
    }

    #[test]
    fn skips_source_only_release() {
        // Rilis terbaru (v1.9.5) hanya berisi kode sumber: harus lompat ke b5454.
        let assets: Vec<serde_json::Value> =
            B5454.iter().map(|n| serde_json::json!({"name": n, "browser_download_url": format!("https://x/{n}")})).collect();
        let releases = serde_json::json!([
            {"tag_name": "v1.9.5", "draft": false, "assets": []},
            {"tag_name": "b5454", "draft": false, "assets": assets},
        ]);
        let (tag, url) = pick_release_asset(&releases, false).unwrap();
        assert_eq!(tag, "b5454");
        assert_eq!(url, "https://x/whisper-bin-x64.zip");
        assert!(pick_release_asset(&serde_json::json!([{"tag_name": "v1", "assets": []}]), false).is_none());
    }

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
