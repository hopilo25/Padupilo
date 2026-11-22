//! Transkripsi klip suara memakai whisper.cpp (whisper-cli) di PC sendiri.

use crate::media::{command, Tools};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Word {
    pub text: String,
    /// Detik, relatif terhadap awal klip asli (sebelum diubah kecepatannya).
    pub start: f64,
    pub end: f64,
}

fn is_punct_only(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| !c.is_alphanumeric())
}

/// Baca keluaran `whisper-cli -oj -ml 1 -sow`.
pub fn parse_whisper_json(json: &str) -> Result<Vec<Word>> {
    let v: serde_json::Value = serde_json::from_str(json).context("JSON whisper tidak valid")?;
    let segs = v["transcription"].as_array().ok_or_else(|| anyhow!("JSON whisper tanpa 'transcription'"))?;
    let mut words: Vec<Word> = Vec::new();
    for s in segs {
        let raw = s["text"].as_str().unwrap_or("");
        let text = raw.trim();
        if text.is_empty() || text.starts_with('[') {
            continue; // [BLANK_AUDIO], [Musik], dsb.
        }
        let from = s["offsets"]["from"].as_f64().unwrap_or(0.0) / 1000.0;
        let to = s["offsets"]["to"].as_f64().unwrap_or(0.0) / 1000.0;
        let glued = !raw.starts_with(' ') && !words.is_empty();
        if is_punct_only(text) || glued {
            if let Some(last) = words.last_mut() {
                last.text.push_str(text);
                last.end = last.end.max(to);
                continue;
            }
        }
        // Satu segmen kadang berisi beberapa kata: bagi waktunya rata menurut panjang huruf.
        let parts: Vec<&str> = text.split_whitespace().collect();
        let total: usize = parts.iter().map(|p| p.chars().count()).sum::<usize>().max(1);
        let mut t = from;
        for p in parts {
            let d = (to - from) * p.chars().count() as f64 / total as f64;
            words.push(Word { text: p.to_string(), start: t, end: t + d });
            t += d;
        }
    }
    Ok(words)
}

/// Transkripsi satu klip. `lang`: "id", "en", atau "auto".
pub fn transcribe(tools: &Tools, clip: &Path, lang: &str, work_dir: &Path) -> Result<Vec<Word>> {
    let whisper = tools.whisper.as_deref().ok_or_else(|| anyhow!("whisper-cli belum ada. Tekan \"Unduh komponen\"."))?;
    let model = tools.model.as_deref().ok_or_else(|| anyhow!("Model Whisper belum ada. Tekan \"Unduh komponen\"."))?;
    std::fs::create_dir_all(work_dir)?;
    let stem = clip.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "klip".into());
    let wav = work_dir.join(format!("{stem}-16k.wav"));
    let st = command(tools.ffmpeg()?)
        .args(["-v", "error", "-y", "-i"])
        .arg(clip)
        .args(["-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
        .arg(&wav)
        .status()?;
    if !st.success() {
        bail!("Klip suara tidak bisa dibaca: {}", clip.display());
    }
    let out_base = work_dir.join(format!("{stem}-transkrip"));
    let out = command(whisper)
        .arg("-m")
        .arg(model)
        .arg("-f")
        .arg(&wav)
        .args(["-l", lang, "-oj", "-ml", "1", "-sow", "-np", "-of"])
        .arg(&out_base)
        .output()
        .context("gagal menjalankan whisper-cli")?;
    let json_path = out_base.with_extension("json");
    if !out.status.success() || !json_path.is_file() {
        bail!("Transkripsi gagal: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or(""));
    }
    let text = std::fs::read_to_string(&json_path)?;
    parse_whisper_json(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_word_segments() {
        let j = r#"{"transcription":[
          {"offsets":{"from":0,"to":420},"text":""},
          {"offsets":{"from":420,"to":660},"text":" Pernah"},
          {"offsets":{"from":660,"to":1020},"text":" merasa"},
          {"offsets":{"from":1020,"to":1480},"text":" ponselmu"},
          {"offsets":{"from":1480,"to":1500},"text":"?"},
          {"offsets":{"from":1500,"to":1600},"text":" [BLANK_AUDIO]"}]}"#;
        let w = parse_whisper_json(j).unwrap();
        assert_eq!(w.len(), 3);
        assert_eq!(w[0].text, "Pernah");
        assert!((w[0].start - 0.42).abs() < 1e-9);
        assert_eq!(w[2].text, "ponselmu?");
        assert!((w[2].end - 1.5).abs() < 1e-9);
    }

    #[test]
    fn splits_multi_word_segment() {
        let j = r#"{"transcription":[{"offsets":{"from":0,"to":1000},"text":" di sana"}]}"#;
        let w = parse_whisper_json(j).unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[1].text, "sana");
        assert!((w[1].start - 1.0 / 3.0).abs() < 1e-9);
    }
}
