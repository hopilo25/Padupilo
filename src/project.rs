//! File proyek .padupilo: semua yang dibutuhkan untuk membuka ulang dan merevisi video.

use crate::placement::{Clip, Mode};
use crate::render::Style;
use crate::transcribe::Word;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub version: u32,
    pub video: Option<PathBuf>,
    pub clips: Vec<Clip>,
    pub mode: Mode,
    pub scene_threshold: f64,
    pub scenes: Vec<f64>,
    /// Hasil transkripsi (sudah dikoreksi) per klip, urutan sama dengan `clips`.
    pub words: Vec<Option<Vec<Word>>>,
    pub style: Style,
    /// Suara asli video dalam dB; `None` = dibisukan.
    pub bg_db: Option<f64>,
    pub language: String,
    pub export_srt: bool,
}

impl Default for Project {
    fn default() -> Self {
        Project {
            version: 1,
            video: None,
            clips: vec![],
            mode: Mode::PerAdegan,
            scene_threshold: 0.08,
            scenes: vec![],
            words: vec![],
            style: Style::default(),
            bg_db: Some(-18.0),
            language: "id".into(),
            export_srt: true,
        }
    }
}

impl Project {
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Project> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    /// Nama dasar untuk hasil ekspor, diambil dari nama video.
    pub fn base_name(&self) -> String {
        self.video
            .as_ref()
            .and_then(|v| v.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "padupilo".into())
    }

    /// Jaga `words` sepanjang `clips`.
    pub fn sync_words(&mut self) {
        self.words.resize(self.clips.len(), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut p = Project::default();
        p.video = Some("C:/Video Saya/fyp.mp4".into());
        p.clips.push(Clip { path: "Klip1.wav".into(), dur: 8.4, speed: 0.926 });
        p.sync_words();
        p.words[0] = Some(vec![Word { text: "Pernah".into(), start: 0.4, end: 0.6 }]);
        p.mode = Mode::Berurutan { start: 0.5, gap: 0.3 };
        let dir = std::env::temp_dir().join("padupilo-test");
        let f = dir.join("uji.padupilo");
        p.save(&f).unwrap();
        assert_eq!(Project::load(&f).unwrap(), p);
        assert_eq!(p.base_name(), "fyp");
    }
}
