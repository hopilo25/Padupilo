//! Menggambar subtitle beranimasi per frame (RGBA, alpha lurus) untuk ditumpuk ffmpeg.

use crate::subs::{chunk_at, Chunk};
use ab_glyph::{Font, FontRef, OutlineCurve};
use serde::{Deserialize, Serialize};
use tiny_skia::{
    Color, FillRule, Mask, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform,
};

pub const FONT_BLACK: &[u8] = include_bytes!("../font/Montserrat-Black.ttf");
pub const FONT_BOLD: &[u8] = include_bytes!("../font/Montserrat-Bold.ttf");

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum StyleKind {
    Beast,
    Karaoke,
    Minimal,
}

impl StyleKind {
    pub const ALL: [StyleKind; 3] = [StyleKind::Beast, StyleKind::Karaoke, StyleKind::Minimal];
    pub fn label(&self) -> &'static str {
        match self {
            StyleKind::Beast => "Beast (ala MrBeast)",
            StyleKind::Karaoke => "Karaoke",
            StyleKind::Minimal => "Minimal hopilo",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Style {
    pub kind: StyleKind,
    pub highlight: [u8; 3],
    /// Ukuran huruf pada video setinggi 1920 px.
    pub size: f32,
    /// Titik tengah subtitle pada video setinggi 1920 px.
    pub center_y: f32,
    pub uppercase: bool,
    pub max_words: usize,
}

impl Style {
    pub fn preset(kind: StyleKind) -> Style {
        match kind {
            StyleKind::Beast => Style { kind, highlight: [255, 225, 40], size: 96.0, center_y: 1590.0, uppercase: true, max_words: 3 },
            StyleKind::Karaoke => Style { kind, highlight: [0, 229, 255], size: 70.0, center_y: 1590.0, uppercase: false, max_words: 5 },
            StyleKind::Minimal => Style { kind, highlight: [255, 255, 255], size: 58.0, center_y: 1600.0, uppercase: false, max_words: 6 },
        }
    }
    pub fn max_chars(&self) -> usize {
        match self.kind {
            StyleKind::Beast => 15,
            StyleKind::Karaoke => 30,
            StyleKind::Minimal => 36,
        }
    }
}

impl Default for Style {
    fn default() -> Self {
        Style::preset(StyleKind::Beast)
    }
}

pub struct Renderer {
    font: FontRef<'static>,
    pub style: Style,
    pub width: u32,
    pub strip_h: u32,
    pub strip_y: u32,
    pub fps: f64,
    /// Skala terhadap video 1920 px tinggi.
    k: f32,
}

fn color(rgb: [u8; 3], a: f32) -> Color {
    Color::from_rgba8(rgb[0], rgb[1], rgb[2], (a.clamp(0.0, 1.0) * 255.0) as u8)
}

fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

impl Renderer {
    pub fn new(style: Style, width: u32, height: u32, fps: f64) -> Renderer {
        let font_bytes = if style.kind == StyleKind::Beast { FONT_BLACK } else { FONT_BOLD };
        let font = FontRef::try_from_slice(font_bytes).expect("font bawaan rusak");
        let k = height as f32 / 1920.0;
        let strip_h = (((440.0 * k) as u32).max(2)) & !1;
        let cy = style.center_y * k;
        let strip_y = ((cy - strip_h as f32 / 2.0).max(0.0) as u32).min(height.saturating_sub(strip_h)) & !1;
        Renderer { font, style, width, strip_h, strip_y, fps, k }
    }

    fn word_text(&self, s: &str) -> String {
        if self.style.uppercase { s.to_uppercase() } else { s.to_string() }
    }

    /// Lebar teks (px) pada ukuran `px`.
    fn measure(&self, text: &str, px: f32) -> f32 {
        let scale = px / self.font.units_per_em().unwrap_or(1000.0);
        let mut w = 0.0;
        let mut prev = None;
        for ch in text.chars() {
            let id = self.font.glyph_id(ch);
            if let Some(p) = prev {
                w += self.font.kern_unscaled(p, id) * scale;
            }
            w += self.font.h_advance_unscaled(id) * scale;
            prev = Some(id);
        }
        w
    }

    /// Bentuk huruf sebagai path; `baseline` di y, mulai dari x.
    fn text_path(&self, text: &str, px: f32, x: f32, baseline: f32) -> Option<Path> {
        let scale = px / self.font.units_per_em().unwrap_or(1000.0);
        let mut pb = PathBuilder::new();
        let mut pen = x;
        let mut prev = None;
        for ch in text.chars() {
            let id = self.font.glyph_id(ch);
            if let Some(p) = prev {
                pen += self.font.kern_unscaled(p, id) * scale;
            }
            if let Some(outline) = self.font.outline(id) {
                let tp = |p: ab_glyph::Point| (pen + p.x * scale, baseline - p.y * scale);
                let mut last: Option<(f32, f32)> = None;
                for c in &outline.curves {
                    let (s, end) = match c {
                        OutlineCurve::Line(a, b) => (*a, *b),
                        OutlineCurve::Quad(a, _, b) => (*a, *b),
                        OutlineCurve::Cubic(a, _, _, b) => (*a, *b),
                    };
                    let sp = tp(s);
                    if last.map(|l| (l.0 - sp.0).abs() > 0.01 || (l.1 - sp.1).abs() > 0.01).unwrap_or(true) {
                        if last.is_some() {
                            pb.close();
                        }
                        pb.move_to(sp.0, sp.1);
                    }
                    match c {
                        OutlineCurve::Line(_, b) => {
                            let q = tp(*b);
                            pb.line_to(q.0, q.1)
                        }
                        OutlineCurve::Quad(_, c1, b) => {
                            let (q1, q) = (tp(*c1), tp(*b));
                            pb.quad_to(q1.0, q1.1, q.0, q.1)
                        }
                        OutlineCurve::Cubic(_, c1, c2, b) => {
                            let (q1, q2, q) = (tp(*c1), tp(*c2), tp(*b));
                            pb.cubic_to(q1.0, q1.1, q2.0, q2.1, q.0, q.1)
                        }
                    }
                    last = Some(tp(end));
                }
                if last.is_some() {
                    pb.close();
                }
            }
            pen += self.font.h_advance_unscaled(id) * scale;
            prev = Some(id);
        }
        pb.finish()
    }

    /// Satu frame strip subtitle pada detik `t` (RGBA, alpha lurus).
    pub fn frame(&self, chunks: &[Chunk], t: f64) -> Vec<u8> {
        let mut pm = Pixmap::new(self.width, self.strip_h).unwrap();
        if let Some(ci) = chunk_at(chunks, t) {
            let ch = &chunks[ci];
            let frames_in = ((t - ch.start) * self.fps).round().max(0.0) as f32;
            match self.style.kind {
                StyleKind::Beast => self.draw_beast(&mut pm, ch, ci, t, frames_in),
                StyleKind::Karaoke => self.draw_karaoke(&mut pm, ch, t, frames_in),
                StyleKind::Minimal => self.draw_minimal(&mut pm, ch, t, frames_in),
            }
        }
        unpremultiply(pm.take())
    }

    fn fit_size(&self, texts: &[String], base: f32, active: Option<usize>, gap_em: f32) -> f32 {
        let max_w = self.width as f32 * 0.87;
        let mut px = base;
        loop {
            let total: f32 = texts
                .iter()
                .enumerate()
                .map(|(i, s)| self.measure(s, if Some(i) == active { px * 1.12 } else { px }))
                .sum::<f32>()
                + gap_em * px * (texts.len().saturating_sub(1)) as f32;
            if total <= max_w || px < base * 0.55 {
                return px;
            }
            px *= 0.95;
        }
    }

    fn draw_beast(&self, pm: &mut Pixmap, ch: &Chunk, ci: usize, t: f64, frames_in: f32) {
        let texts: Vec<String> = ch.words.iter().map(|w| self.word_text(&w.text)).collect();
        let act = ch.active(t);
        let px = self.fit_size(&texts, self.style.size * self.k, Some(act), 0.28);
        let gap = 0.28 * px;
        let widths: Vec<f32> = texts.iter().enumerate().map(|(i, s)| self.measure(s, if i == act { px * 1.12 } else { px })).collect();
        let total: f32 = widths.iter().sum::<f32>() + gap * (texts.len() - 1) as f32;
        let cx = self.width as f32 / 2.0;
        let cy = self.strip_h as f32 / 2.0;
        let baseline = cy + px * 0.36;
        // pop: 0.55 -> 1.12 -> 0.97 -> 1.0 dalam 8 frame (30 fps), miring bergantian.
        let f = frames_in * 30.0 / self.fps as f32;
        let keys = [(0.0, 0.55), (3.0, 1.12), (6.0, 0.97), (8.0, 1.0)];
        let mut sc = 1.0;
        for w in keys.windows(2) {
            if f >= w[0].0 && f < w[1].0 {
                let p = (f - w[0].0) / (w[1].0 - w[0].0);
                let p = 1.0 - (1.0 - p) * (1.0 - p);
                sc = w[0].1 + (w[1].1 - w[0].1) * p;
            }
        }
        let rot = if ci % 2 == 1 { 1.5 } else { -1.5 };
        let tf = Transform::from_translate(cx, cy).pre_rotate(rot).pre_scale(sc, sc).pre_translate(-cx, -cy);
        let stroke_w = (px / 9.0).max(4.0) * 2.0;
        let mut x = cx - total / 2.0;
        let mut items = vec![];
        for (i, s) in texts.iter().enumerate() {
            let p = if i == act { px * 1.12 } else { px };
            if let Some(path) = self.text_path(s, p, x, baseline) {
                items.push((path, i == act));
            }
            x += widths[i] + gap;
        }
        let shadow = paint(Color::from_rgba8(0, 0, 0, 150));
        let mut st = Stroke::default();
        st.width = stroke_w;
        st.line_join = tiny_skia::LineJoin::Round;
        let off = Transform::from_translate(6.0 * self.k, 9.0 * self.k).pre_concat(tf);
        for (path, _) in &items {
            pm.stroke_path(path, &shadow, &st, off, None);
            pm.fill_path(path, &shadow, FillRule::Winding, off, None);
        }
        let black = paint(Color::BLACK);
        for (path, _) in &items {
            pm.stroke_path(path, &black, &st, tf, None);
        }
        for (path, is_act) in &items {
            let c = if *is_act { color(self.style.highlight, 1.0) } else { Color::WHITE };
            pm.fill_path(path, &paint(c), FillRule::Winding, tf, None);
        }
    }

    fn draw_karaoke(&self, pm: &mut Pixmap, ch: &Chunk, t: f64, frames_in: f32) {
        let texts: Vec<String> = ch.words.iter().map(|w| self.word_text(&w.text)).collect();
        let act = ch.active(t);
        let px = self.fit_size(&texts, self.style.size * self.k, None, 0.3);
        let gap = 0.3 * px;
        let widths: Vec<f32> = texts.iter().map(|s| self.measure(s, px)).collect();
        let total: f32 = widths.iter().sum::<f32>() + gap * (texts.len() - 1) as f32;
        let alpha = (frames_in / 4.0).min(1.0);
        let baseline = self.strip_h as f32 / 2.0 + px * 0.36;
        let mut x = self.width as f32 / 2.0 - total / 2.0;
        let mut st = Stroke::default();
        st.width = (px / 7.0).max(4.0);
        st.line_join = tiny_skia::LineJoin::Round;
        let tf = Transform::identity();
        for (i, s) in texts.iter().enumerate() {
            if let Some(path) = self.text_path(s, px, x, baseline) {
                pm.stroke_path(&path, &paint(Color::from_rgba(0.0, 0.0, 0.0, 0.85 * alpha).unwrap()), &st, tf, None);
                pm.fill_path(&path, &paint(Color::from_rgba(1.0, 1.0, 1.0, alpha).unwrap()), FillRule::Winding, tf, None);
                let w = &ch.words[i];
                let progress = if i < act {
                    1.0
                } else if i == act {
                    (((t - w.start) / (w.end - w.start).max(0.05)) as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                if progress > 0.0 {
                    let fill_w = (widths[i] + 4.0) * progress;
                    if let Some(rect) = Rect::from_xywh(x - 2.0, 0.0, fill_w, self.strip_h as f32) {
                        let mut mask = Mask::new(self.width, self.strip_h).unwrap();
                        mask.fill_path(&PathBuilder::from_rect(rect), FillRule::Winding, false, tf);
                        pm.fill_path(&path, &paint(color(self.style.highlight, alpha)), FillRule::Winding, tf, Some(&mask));
                    }
                }
            }
            x += widths[i] + gap;
        }
    }

    fn draw_minimal(&self, pm: &mut Pixmap, ch: &Chunk, t: f64, frames_in: f32) {
        let texts: Vec<String> = ch.words.iter().map(|w| self.word_text(&w.text)).collect();
        let act = ch.active(t);
        let px = self.fit_size(&texts, self.style.size * self.k, None, 0.28);
        let gap = 0.28 * px;
        let widths: Vec<f32> = texts.iter().map(|s| self.measure(s, px)).collect();
        let total: f32 = widths.iter().sum::<f32>() + gap * (texts.len() - 1) as f32;
        let p = (frames_in / 6.0).min(1.0);
        let ease = 1.0 - (1.0 - p) * (1.0 - p);
        let rise = (1.0 - ease) * 24.0 * self.k;
        let tf = Transform::from_translate(0.0, rise);
        let cx = self.width as f32 / 2.0;
        let cy = self.strip_h as f32 / 2.0;
        let pad_x = px * 0.55;
        let pad_y = px * 0.42;
        if let Some(r) = Rect::from_xywh(cx - total / 2.0 - pad_x, cy - px * 0.5 - pad_y, total + 2.0 * pad_x, px + 2.0 * pad_y) {
            let mut pb = PathBuilder::new();
            let rad = px * 0.35;
            let (l, tp, rr, b) = (r.left(), r.top(), r.right(), r.bottom());
            pb.move_to(l + rad, tp);
            pb.line_to(rr - rad, tp);
            pb.quad_to(rr, tp, rr, tp + rad);
            pb.line_to(rr, b - rad);
            pb.quad_to(rr, b, rr - rad, b);
            pb.line_to(l + rad, b);
            pb.quad_to(l, b, l, b - rad);
            pb.line_to(l, tp + rad);
            pb.quad_to(l, tp, l + rad, tp);
            pb.close();
            if let Some(path) = pb.finish() {
                pm.fill_path(&path, &paint(Color::from_rgba(0.0, 0.0, 0.0, 0.62 * ease).unwrap()), FillRule::Winding, tf, None);
            }
        }
        let baseline = cy + px * 0.36;
        let mut x = cx - total / 2.0;
        for (i, s) in texts.iter().enumerate() {
            if let Some(path) = self.text_path(s, px, x, baseline) {
                let c = if i == act { color(self.style.highlight, ease) } else { Color::from_rgba(1.0, 1.0, 1.0, 0.7 * ease).unwrap() };
                pm.fill_path(&path, &paint(c), FillRule::Winding, tf, None);
            }
            x += widths[i] + gap;
        }
    }
}

/// tiny-skia menyimpan warna premultiplied; ffmpeg overlay butuh alpha lurus.
fn unpremultiply(mut data: Vec<u8>) -> Vec<u8> {
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    data
}

/// Tumpuk strip RGBA di atas frame RGBA (untuk pratinjau).
pub fn composite(frame: &mut [u8], fw: u32, strip: &[u8], strip_y: u32) {
    let row = (fw * 4) as usize;
    for (i, src) in strip.chunks_exact(4).enumerate() {
        let a = src[3] as u32;
        if a == 0 {
            continue;
        }
        let idx = strip_y as usize * row + i * 4;
        if idx + 3 >= frame.len() {
            break;
        }
        for c in 0..3 {
            let d = frame[idx + c] as u32;
            frame[idx + c] = ((src[c] as u32 * a + d * (255 - a)) / 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subs::TimedWord;

    fn chunk() -> Vec<Chunk> {
        let w = |t: &str, s: f64, e: f64| TimedWord { text: t.into(), start: s, end: e, clip: 0 };
        vec![Chunk { words: vec![w("Seperti", 1.0, 1.3), w("bisa", 1.3, 1.6)], start: 1.0, end: 1.8 }]
    }

    fn has_color(buf: &[u8], rgb: [u8; 3]) -> bool {
        buf.chunks_exact(4).any(|p| p[3] == 255 && (p[0] as i32 - rgb[0] as i32).abs() < 6 && (p[1] as i32 - rgb[1] as i32).abs() < 6 && (p[2] as i32 - rgb[2] as i32).abs() < 6)
    }

    #[test]
    fn blank_when_no_chunk() {
        for kind in StyleKind::ALL {
            let r = Renderer::new(Style::preset(kind), 540, 960, 30.0);
            assert!(r.frame(&chunk(), 0.2).chunks_exact(4).all(|p| p[3] == 0));
        }
    }

    #[test]
    fn styles_draw_and_highlight() {
        for kind in StyleKind::ALL {
            let style = Style { highlight: [255, 0, 128], ..Style::preset(kind) };
            let r = Renderer::new(style, 540, 960, 30.0);
            let f = r.frame(&chunk(), 1.45);
            assert!(f.chunks_exact(4).filter(|p| p[3] > 0).count() > 500, "{kind:?} kosong");
            assert!(has_color(&f, [255, 0, 128]), "{kind:?} tanpa warna sorot");
        }
    }
}
