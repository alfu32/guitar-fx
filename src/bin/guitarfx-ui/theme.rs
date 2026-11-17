//! Per-effect colours and painted icons (drawn with shapes, so no font support is needed).

use eframe::egui::{self, epaint::Hsva, pos2, vec2, Color32, Pos2, Rect, Sense, Shape, Stroke};
use std::f32::consts::TAU;

pub fn accent(effect: &str) -> Color32 {
    let (r, g, b) = match effect {
        "overdrive" => (235, 85, 55),
        "compressor" => (70, 150, 235),
        "echo" => (155, 110, 235),
        "delay" => (105, 95, 225),
        "chorus" => (55, 190, 205),
        "flanger" => (55, 200, 150),
        "phaser" => (235, 110, 190),
        "tremolo" => (235, 190, 55),
        "wah" => (165, 205, 55),
        "harmonics" => (245, 150, 55),
        "reverb" => (95, 135, 205),
        "tone-shift" => (60, 205, 100),
        "autotune" => (235, 65, 130),
        "vocoder" => (190, 90, 225),
        _ => (150, 150, 150),
    };
    Color32::from_rgb(r, g, b)
}

/// A shade of the effect colour for its n-th control, so controls are told apart
/// while still reading as part of the same pedal.
pub fn control_accent(base: Color32, n: usize) -> Color32 {
    let mut h = Hsva::from(base);
    h.h = (h.h + 0.045 * (n as f32 - 1.5)).rem_euclid(1.0);
    h.s = (h.s * (0.9 + 0.05 * (n % 3) as f32)).min(1.0);
    Color32::from(h)
}

pub fn tint(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

fn curve(r: Rect, color: Color32, w: f32, f: impl Fn(f32) -> f32) -> Shape {
    let pts: Vec<Pos2> = (0..=24)
        .map(|i| {
            let x = i as f32 / 24.0;
            pos2(r.left() + x * r.width(), r.top() + f(x).clamp(0.0, 1.0) * r.height())
        })
        .collect();
    Shape::line(pts, Stroke::new(w, color))
}

/// Draws the pedal's icon in a `size` square.
pub fn effect_icon(ui: &mut egui::Ui, name: &str, color: Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter();
    let r = rect.shrink(size * 0.12);
    let w = (size / 11.0).max(1.5);
    let st = Stroke::new(w, color);
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    let c = r.center();
    let rad = r.width() / 2.0;
    p.rect_filled(rect, size * 0.22, tint(color, 40));
    match name {
        "overdrive" => {
            // sine with flattened (clipped) peaks
            p.add(curve(r, color, w, |x| 0.5 - ((x * TAU).sin() * 1.8).clamp(-0.8, 0.8) * 0.5));
        }
        "compressor" => {
            // two arrows squeezing a level
            p.line_segment([at(0.0, 0.1), at(0.5, 0.4)], st);
            p.line_segment([at(1.0, 0.1), at(0.5, 0.4)], st);
            p.line_segment([at(0.0, 0.9), at(0.5, 0.6)], st);
            p.line_segment([at(1.0, 0.9), at(0.5, 0.6)], st);
            p.line_segment([at(0.2, 0.5), at(0.8, 0.5)], Stroke::new(w * 0.8, color));
        }
        "echo" => {
            for i in 0..3 {
                let x = 0.15 + i as f32 * 0.33;
                p.circle_filled(at(x, 0.5), rad * (0.38 - 0.1 * i as f32), tint(color, 255 - 70 * i as u8));
            }
        }
        "delay" => {
            // clock
            p.circle_stroke(c, rad, st);
            p.line_segment([c, at(0.5, 0.2)], st);
            p.line_segment([c, at(0.75, 0.6)], st);
        }
        "chorus" => {
            p.add(curve(r.shrink2(vec2(0.0, 0.0)), color, w, |x| 0.38 - 0.25 * (x * TAU).sin() * 0.9));
            p.add(curve(r, tint(color, 170), w, |x| 0.62 - 0.25 * (x * TAU + 1.0).sin() * 0.9));
        }
        "flanger" => {
            // comb of lines of changing height
            for i in 0..6 {
                let x = i as f32 / 5.0;
                let h = 0.5 + 0.4 * (x * 6.0).sin();
                p.line_segment([at(x, 0.5 - h / 2.0), at(x, 0.5 + h / 2.0)], st);
            }
        }
        "phaser" => {
            p.circle_stroke(c, rad, st);
            p.circle_stroke(c, rad * 0.55, Stroke::new(w, tint(color, 170)));
            p.circle_filled(c, rad * 0.14, color);
        }
        "tremolo" => {
            p.add(curve(r, color, w, |x| 0.5 - (x * TAU * 3.0).sin() * 0.45 * (1.0 - (x * 2.0 - 1.0).abs() * 0.0) * (0.35 + 0.65 * (x * TAU).cos().abs())));
        }
        "wah" => {
            // band-pass bump
            p.add(curve(r, color, w, |x| 0.85 - 0.7 * (-((x - 0.5) * 5.0).powi(2)).exp()));
        }
        "harmonics" => {
            for i in 0..5 {
                let x = 0.1 + i as f32 * 0.2;
                let h = 0.9 / (1.0 + i as f32);
                p.line_segment([at(x, 1.0), at(x, 1.0 - h)], Stroke::new(w * 1.5, color));
            }
        }
        "reverb" => {
            for i in 1..=3 {
                let rr = rad * i as f32 / 3.0;
                let pts: Vec<Pos2> = (0..=12)
                    .map(|k| {
                        let a = -0.9 + 1.8 * k as f32 / 12.0;
                        pos2(r.left() + r.width() * 0.1 + a.cos() * rr * 1.5, c.y + a.sin() * rr)
                    })
                    .collect();
                p.add(Shape::line(pts, Stroke::new(w, tint(color, 255 - 60 * (3 - i as u8)))));
            }
        }
        "tone-shift" => {
            p.line_segment([at(0.3, 0.9), at(0.3, 0.1)], st);
            p.line_segment([at(0.1, 0.3), at(0.3, 0.1)], st);
            p.line_segment([at(0.5, 0.3), at(0.3, 0.1)], st);
            p.line_segment([at(0.7, 0.1), at(0.7, 0.9)], st);
            p.line_segment([at(0.5, 0.7), at(0.7, 0.9)], st);
            p.line_segment([at(0.9, 0.7), at(0.7, 0.9)], st);
        }
        "autotune" => {
            p.circle_stroke(c, rad, st);
            p.line_segment([at(0.5, -0.1), at(0.5, 1.1)], Stroke::new(w * 0.7, color));
            p.line_segment([at(-0.1, 0.5), at(1.1, 0.5)], Stroke::new(w * 0.7, color));
            p.circle_filled(c, rad * 0.2, color);
        }
        "vocoder" => {
            for i in 0..5 {
                let x = 0.1 + i as f32 * 0.2;
                let h = [0.4, 0.8, 0.55, 0.95, 0.3][i];
                p.rect_filled(Rect::from_min_max(at(x - 0.07, 1.0 - h), at(x + 0.07, 1.0)), 1.0, color);
            }
        }
        _ => {
            p.circle_filled(c, rad * 0.5, color);
        }
    }
}

/// Small marker in front of a control's name; the shape hints at what it does.
pub fn control_icon(ui: &mut egui::Ui, key: &str, color: Color32) {
    let s = 14.0;
    let (rect, _) = ui.allocate_exact_size(vec2(s, s), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let r = s * 0.38;
    match key {
        // loudness: triangle
        "drive" | "level" | "makeup" | "amount" | "threshold" | "ratio" | "sensitivity" => {
            p.add(Shape::convex_polygon(vec![pos2(c.x - r, c.y + r), pos2(c.x + r, c.y + r), pos2(c.x, c.y - r)], color, Stroke::NONE));
        }
        // time: ring
        "time" | "rate" | "speed" | "attack" | "release" | "feedback" | "window" => {
            p.circle_stroke(c, r, Stroke::new(2.0, color));
            p.line_segment([c, pos2(c.x, c.y - r * 0.9)], Stroke::new(1.5, color));
        }
        // tone / pitch: wave
        "tone" | "freq" | "q" | "semitones" | "cents" | "key" | "scale" | "min" | "max" | "carrier" | "bands" => {
            let pts: Vec<Pos2> = (0..=8)
                .map(|i| {
                    let x = i as f32 / 8.0;
                    pos2(c.x - r + x * 2.0 * r, c.y - (x * TAU).sin() * r * 0.8)
                })
                .collect();
            p.add(Shape::line(pts, Stroke::new(2.0, color)));
        }
        // blend / space: diamond
        _ => {
            p.add(Shape::convex_polygon(
                vec![pos2(c.x - r, c.y), pos2(c.x, c.y - r), pos2(c.x + r, c.y), pos2(c.x, c.y + r)],
                color,
                Stroke::NONE,
            ));
        }
    }
}
