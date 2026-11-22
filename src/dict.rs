//! Kamus koreksi: perbaikan kata yang sering salah dengar, dipakai untuk semua proyek.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Dict {
    /// Kunci huruf besar tanpa tanda baca -> pengganti.
    pub map: BTreeMap<String, String>,
}

impl Default for Dict {
    fn default() -> Self {
        let mut map = BTreeMap::new();
        for (k, v) in [("FVP", "FYP"), ("FIP", "FYP"), ("FYB", "FYP"), ("HOPILLO", "hopilo"), ("HOPPILO", "hopilo")] {
            map.insert(k.to_string(), v.to_string());
        }
        Dict { map }
    }
}

pub fn core_key(word: &str) -> String {
    word.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_uppercase()).collect()
}

impl Dict {
    pub fn load(path: &Path) -> Dict {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn add(&mut self, wrong: &str, right: &str) {
        let k = core_key(wrong);
        if !k.is_empty() && core_key(right) != k {
            self.map.insert(k, right.trim().to_string());
        }
    }

    /// Ganti inti kata bila ada di kamus; tanda baca di depan/belakang tetap.
    pub fn apply(&self, word: &str) -> String {
        let Some(rep) = self.map.get(&core_key(word)) else { return word.to_string() };
        let lead: String = word.chars().take_while(|c| !c.is_alphanumeric()).collect();
        let trail: String = word.chars().rev().take_while(|c| !c.is_alphanumeric()).collect::<Vec<_>>().into_iter().rev().collect();
        format!("{lead}{rep}{trail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_punctuation() {
        let d = Dict::default();
        assert_eq!(d.apply("FVP,"), "FYP,");
        assert_eq!(d.apply("kucing."), "kucing.");
    }

    #[test]
    fn add_learns_correction() {
        let mut d = Dict::default();
        d.add("menyuakainya", "menyukainya");
        assert_eq!(d.apply("menyuakainya,"), "menyukainya,");
        d.add("sama", "Sama"); // beda kapital saja: tidak disimpan
        assert!(!d.map.contains_key("SAMA"));
    }
}
