//! Timeline: garis adegan, posisi klip suara, potongan subtitle, dan penanda waktu pratinjau.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use padupilo::placement::Placed;
use padupilo::subs::Chunk;

pub const ACCENT: Color32 = Color32::from_rgb(255, 225, 40);

/// Menggambar timeline; mengembalikan detik yang diklik/diseret bila ada.
pub fn show(ui: &mut egui::Ui, dur: f64, scenes: &[f64], items: &[Placed], chunks: &[Chunk], playhead: f64) -> Option<f64> {
    let label_w = 70.0;
    let row_h = 22.0;
    let (resp, painter) = ui.allocate_painter(Vec2::new(ui.available_width(), row_h * 3.0 + 34.0), Sense::click_and_drag());
    let r = resp.rect;
    let x0 = r.left() + label_w;
    let w = (r.width() - label_w - 8.0).max(10.0);
    let dur = dur.max(0.1);
    let x_of = |t: f64| x0 + (t / dur) as f32 * w;
    let dim = Color32::from_gray(150);
    let line = Color32::from_gray(90);
    let font = FontId::proportional(12.0);

    let rows = ["Video", "Suara", "Subtitle"];
    for (i, name) in rows.iter().enumerate() {
        let y = r.top() + 6.0 + i as f32 * (row_h + 6.0);
        painter.text(Pos2::new(r.left() + 4.0, y + row_h / 2.0), Align2::LEFT_CENTER, *name, font.clone(), dim);
        let row = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, row_h));
        match i {
            0 => {
                painter.rect_stroke(row, 4.0, Stroke::new(1.0, line), StrokeKind::Inside);
                for s in scenes.iter().skip(1) {
                    let x = x_of(*s);
                    painter.line_segment([Pos2::new(x, y), Pos2::new(x, y + row_h)], Stroke::new(1.0, dim));
                }
            }
            1 => {
                for (n, p) in items.iter().enumerate() {
                    let rr = Rect::from_min_max(Pos2::new(x_of(p.start), y), Pos2::new(x_of(p.start + p.dur).min(x0 + w + 40.0), y + row_h));
                    let over = p.start + p.dur > dur + 1e-6;
                    let c = if over { Color32::from_rgb(255, 120, 80) } else { Color32::WHITE };
                    painter.rect_filled(rr, 4.0, c.gamma_multiply(0.18));
                    painter.rect_stroke(rr, 4.0, Stroke::new(1.2, c), StrokeKind::Inside);
                    painter.text(rr.left_center() + Vec2::new(5.0, 0.0), Align2::LEFT_CENTER, format!("{}", n + 1), font.clone(), c);
                }
            }
            _ => {
                for c in chunks {
                    let rr = Rect::from_min_max(Pos2::new(x_of(c.start), y + 3.0), Pos2::new(x_of(c.end).max(x_of(c.start) + 2.0), y + row_h - 3.0));
                    painter.rect_stroke(rr, 2.0, Stroke::new(1.0, line), StrokeKind::Inside);
                }
            }
        }
    }
    // Skala detik
    let step = if dur > 120.0 { 30.0 } else if dur > 40.0 { 10.0 } else { 5.0 };
    let mut t = 0.0;
    while t <= dur + 1e-6 {
        painter.text(Pos2::new(x_of(t), r.bottom() - 6.0), Align2::CENTER_BOTTOM, format!("{}s", t as i64), FontId::proportional(10.5), dim);
        t += step;
    }
    let px = x_of(playhead.clamp(0.0, dur));
    painter.line_segment([Pos2::new(px, r.top()), Pos2::new(px, r.bottom() - 16.0)], Stroke::new(2.0, ACCENT));

    if resp.clicked() || resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let t = ((pos.x - x0) / w) as f64 * dur;
            return Some(t.clamp(0.0, dur));
        }
    }
    None
}
