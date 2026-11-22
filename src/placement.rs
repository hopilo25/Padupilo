//! Menentukan di detik berapa setiap klip suara diletakkan di atas video.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Jeda dari awal adegan sampai suara mulai (mode Per Adegan).
pub const SCENE_LEAD: f64 = 0.2;
pub const SPEED_MIN: f64 = 0.85;
pub const SPEED_MAX: f64 = 1.15;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Clip {
    pub path: PathBuf,
    /// Durasi asli dalam detik.
    pub dur: f64,
    /// Faktor kecepatan (1.0 = asli, <1 lebih lambat).
    pub speed: f64,
}

impl Clip {
    pub fn played_dur(&self) -> f64 {
        self.dur / self.speed.clamp(SPEED_MIN, SPEED_MAX)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Mode {
    PerAdegan,
    Berurutan { start: f64, gap: f64 },
    /// Posisi diatur sendiri; berisi detik mulai tiap klip.
    Manual { starts: Vec<f64> },
}

impl Default for Mode {
    fn default() -> Self {
        Mode::PerAdegan
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub start: f64,
    pub dur: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Placement {
    pub items: Vec<Placed>,
    pub warnings: Vec<String>,
}

/// Angka terakhir di nama file (Klip_3 -> 3, "a1b2-Klip10" -> 10), untuk urutan alami.
fn number_in(path: &Path) -> Option<u64> {
    let name = path.file_stem()?.to_string_lossy().to_string();
    let rev: String = name
        .chars()
        .rev()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    rev.chars().rev().collect::<String>().parse().ok()
}

/// Urutkan klip berdasarkan angka di nama file; tanpa angka di belakang, lalu nama.
pub fn natural_order(paths: &mut Vec<PathBuf>) {
    paths.sort_by(|a, b| {
        let (na, nb) = (number_in(a), number_in(b));
        match (na, nb) {
            (Some(x), Some(y)) if x != y => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });
}

fn fmt(s: f64) -> String {
    format!("{:.1}", s).replace('.', ",")
}

pub fn place(clips: &[Clip], scenes: &[f64], video_dur: f64, mode: &Mode) -> Placement {
    let mut out = Placement::default();
    match mode {
        Mode::PerAdegan => {
            let scenes: Vec<f64> = if scenes.is_empty() { vec![0.0] } else { scenes.to_vec() };
            if scenes.len() != clips.len() {
                out.warnings.push(format!(
                    "Jumlah adegan ({}) tidak sama dengan jumlah klip ({}).",
                    scenes.len(),
                    clips.len()
                ));
            }
            let mut prev_end = 0.0_f64;
            for (i, c) in clips.iter().enumerate() {
                let d = c.played_dur();
                let start = if i < scenes.len() {
                    scenes[i] + SCENE_LEAD
                } else {
                    // Klip lebih banyak dari adegan: sambung setelah klip sebelumnya.
                    prev_end + 0.3
                };
                let scene_end = if i + 1 < scenes.len() { scenes[i + 1] } else { video_dur };
                if i < scenes.len() && start + d > scene_end + 1e-6 {
                    out.warnings.push(format!(
                        "Klip {} ({} dtk) melebihi adegan {} (sisa {} dtk).",
                        i + 1,
                        fmt(d),
                        i + 1,
                        fmt(scene_end - start)
                    ));
                }
                prev_end = start + d;
                out.items.push(Placed { start, dur: d });
            }
        }
        Mode::Berurutan { start, gap } => {
            let mut t = *start;
            for c in clips {
                let d = c.played_dur();
                out.items.push(Placed { start: t, dur: d });
                t += d + gap;
            }
        }
        Mode::Manual { starts } => {
            let mut last = 0.0;
            for (i, c) in clips.iter().enumerate() {
                let s = starts.get(i).copied().unwrap_or(last);
                let d = c.played_dur();
                out.items.push(Placed { start: s, dur: d });
                last = s + d + 0.3;
            }
            let mut idx: Vec<usize> = (0..out.items.len()).collect();
            idx.sort_by(|a, b| out.items[*a].start.partial_cmp(&out.items[*b].start).unwrap());
            for w in idx.windows(2) {
                let (a, b) = (&out.items[w[0]], &out.items[w[1]]);
                if a.start + a.dur > b.start + 1e-6 {
                    out.warnings.push(format!("Klip {} dan klip {} bertumpuk.", w[0] + 1, w[1] + 1));
                }
            }
        }
    }
    if let Some(last_end) = out.items.iter().map(|p| p.start + p.dur).reduce(f64::max) {
        if last_end > video_dur + 1e-6 {
            out.warnings.push(format!(
                "Suara berakhir di {} dtk, lebih panjang dari video ({} dtk). Bagian yang lewat akan terpotong.",
                fmt(last_end),
                fmt(video_dur)
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(d: f64) -> Clip {
        Clip { path: "x.wav".into(), dur: d, speed: 1.0 }
    }
    const FYP: [f64; 6] = [8.40, 9.76, 8.08, 7.56, 9.24, 8.04];

    #[test]
    fn per_adegan_fyp() {
        let clips: Vec<Clip> = FYP.iter().map(|d| clip(*d)).collect();
        let scenes = [0.0, 10.0, 20.0, 30.0, 40.0, 50.0];
        let p = place(&clips, &scenes, 60.0, &Mode::PerAdegan);
        let starts: Vec<f64> = p.items.iter().map(|x| (x.start * 10.0).round() / 10.0).collect();
        assert_eq!(starts, vec![0.2, 10.2, 20.2, 30.2, 40.2, 50.2]);
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    }

    #[test]
    fn per_adegan_overflow_and_extra_clip() {
        let mut clips: Vec<Clip> = FYP.iter().map(|d| clip(*d)).collect();
        clips[1].speed = 0.9; // 9.76/0.9 = 10.84 > 9.8
        clips.push(clip(3.0));
        let scenes = [0.0, 10.0, 20.0, 30.0, 40.0, 50.0];
        let p = place(&clips, &scenes, 60.0, &Mode::PerAdegan);
        assert_eq!(p.items.len(), 7);
        assert!(p.warnings.iter().any(|w| w.contains("Jumlah adegan (6)")));
        assert!(p.warnings.iter().any(|w| w.starts_with("Klip 2")));
        assert!(p.items[6].start > p.items[5].start);
        assert!(p.warnings.iter().any(|w| w.contains("lebih panjang dari video")));
    }

    #[test]
    fn berurutan_gap() {
        let clips = vec![clip(2.0), clip(3.0)];
        let p = place(&clips, &[], 10.0, &Mode::Berurutan { start: 1.0, gap: 0.3 });
        assert_eq!(p.items[0], Placed { start: 1.0, dur: 2.0 });
        assert!((p.items[1].start - 3.3).abs() < 1e-9);
    }

    #[test]
    fn manual_overlap_warns() {
        let clips = vec![clip(2.0), clip(3.0)];
        let p = place(&clips, &[], 10.0, &Mode::Manual { starts: vec![0.0, 1.0] });
        assert!(p.warnings.iter().any(|w| w.contains("bertumpuk")));
    }

    #[test]
    fn natural_order_mixed_names() {
        let mut v: Vec<PathBuf> =
            ["Klip10.wav", "Klip_3.wav", "intro.wav", "81467ee1-Klip2.wav", "Klip1.wav"].iter().map(PathBuf::from).collect();
        natural_order(&mut v);
        let names: Vec<_> = v.iter().map(|p| p.to_string_lossy().to_string()).collect();
        assert_eq!(names, ["Klip1.wav", "81467ee1-Klip2.wav", "Klip_3.wav", "Klip10.wav", "intro.wav"]);
    }
}
