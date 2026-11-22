//! Kata di timeline video, potongan subtitle, dan ekspor SRT.

use crate::transcribe::Word;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TimedWord {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub clip: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub words: Vec<TimedWord>,
    pub start: f64,
    pub end: f64,
}

impl Chunk {
    pub fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }
    /// Indeks kata yang sedang diucapkan pada detik `t`.
    pub fn active(&self, t: f64) -> usize {
        let mut a = 0;
        for (i, w) in self.words.iter().enumerate() {
            if t >= w.start - 0.03 {
                a = i;
            }
        }
        a
    }
}

/// Pindahkan kata tiap klip ke waktu video (posisi klip + kecepatan).
pub fn timeline_words(clips: &[(Vec<Word>, f64, f64)]) -> Vec<TimedWord> {
    let mut out = vec![];
    for (ci, (words, start, speed)) in clips.iter().enumerate() {
        let sp = if *speed > 0.0 { *speed } else { 1.0 };
        for w in words {
            out.push(TimedWord { text: w.text.clone(), start: start + w.start / sp, end: start + w.end / sp, clip: ci });
        }
    }
    out.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
    out
}

fn ends_phrase(w: &str) -> bool {
    w.ends_with([',', '.', '?', '!', ';', ':'])
}

pub fn make_chunks(words: &[TimedWord], max_words: usize, max_chars: usize) -> Vec<Chunk> {
    let max_words = max_words.max(1);
    let mut chunks: Vec<Chunk> = vec![];
    let mut cur: Vec<TimedWord> = vec![];
    for (i, w) in words.iter().enumerate() {
        cur.push(w.clone());
        let next = words.get(i + 1);
        let len = cur.iter().map(|x| x.text.chars().count() + 1).sum::<usize>() - 1;
        let would_overflow = next.map(|n| len + 1 + n.text.chars().count() > max_chars).unwrap_or(true);
        let breaks = ends_phrase(&w.text)
            || next.map(|n| n.clip != w.clip || n.start - w.end > 0.6).unwrap_or(true)
            || cur.len() >= max_words
            || (would_overflow && !cur.is_empty());
        if breaks {
            let start = cur[0].start;
            let end = cur.last().unwrap().end;
            chunks.push(Chunk { words: std::mem::take(&mut cur), start, end });
        }
    }
    // Potongan bertahan sampai potongan berikut (dalam klip sama), paling lama +0,6 dtk.
    for i in 0..chunks.len() {
        let last = chunks[i].words.last().unwrap().clone();
        let mut end = last.end + 0.35;
        if let Some(n) = chunks.get(i + 1) {
            if n.words[0].clip == last.clip {
                end = n.start;
            }
            end = end.min(n.start);
        }
        chunks[i].end = end.min(last.end + 0.6).max(last.end);
    }
    chunks
}

pub fn chunk_at(chunks: &[Chunk], t: f64) -> Option<usize> {
    chunks.iter().position(|c| t >= c.start - 0.05 && t < c.end)
}

fn srt_time(t: f64) -> String {
    let ms = (t.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn to_srt(chunks: &[Chunk]) -> String {
    let mut s = String::new();
    for (i, c) in chunks.iter().enumerate() {
        s.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, srt_time(c.start), srt_time(c.end), c.text()));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(t: &str, s: f64, e: f64, clip: usize) -> TimedWord {
        TimedWord { text: t.into(), start: s, end: e, clip }
    }

    #[test]
    fn chunks_break_on_punctuation_and_limits() {
        let words = vec![
            w("Pernah", 0.62, 0.86, 0),
            w("merasa", 0.86, 1.22, 0),
            w("ponselmu", 1.22, 1.68, 0),
            w("bisa", 1.7, 1.9, 0),
            w("membaca?", 1.9, 2.3, 0),
            w("Padahal,", 2.6, 3.0, 0),
            w("Jejak", 10.7, 11.1, 1),
        ];
        let c = make_chunks(&words, 3, 15);
        let texts: Vec<String> = c.iter().map(|c| c.text()).collect();
        assert_eq!(texts, ["Pernah merasa", "ponselmu bisa", "membaca?", "Padahal,", "Jejak"]);
        assert!((c[0].end - 1.22).abs() < 1e-9); // bertahan sampai potongan berikut
        assert!((c[3].end - 3.35).abs() < 1e-9); // akhir klip: +0,35
    }

    #[test]
    fn active_word_follows_time() {
        let c = Chunk { words: vec![w("a", 1.0, 1.2, 0), w("b", 1.2, 1.5, 0)], start: 1.0, end: 1.6 };
        assert_eq!(c.active(1.1), 0);
        assert_eq!(c.active(1.3), 1);
    }

    #[test]
    fn timeline_scales_by_speed() {
        let words = vec![Word { text: "x".into(), start: 0.926, end: 1.852 }];
        let tw = timeline_words(&[(words, 10.0, 0.926)]);
        assert!((tw[0].start - 11.0).abs() < 1e-9);
        assert!((tw[0].end - 12.0).abs() < 1e-9);
    }

    #[test]
    fn srt_format() {
        let c = vec![Chunk { words: vec![w("Halo", 0.62, 1.0, 0)], start: 0.62, end: 61.5 }];
        assert_eq!(to_srt(&c), "1\n00:00:00,620 --> 00:01:01,500\nHalo\n\n");
    }
}
