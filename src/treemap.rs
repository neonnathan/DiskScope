//! Squarified treemap layout (Bruls, Huizing & van Wijk).

use egui::{pos2, vec2, Color32, Rect};

pub const PALETTE: [Color32; 10] = [
    Color32::from_rgb(56, 100, 184),
    Color32::from_rgb(36, 128, 120),
    Color32::from_rgb(94, 80, 180),
    Color32::from_rgb(166, 116, 54),
    Color32::from_rgb(166, 74, 98),
    Color32::from_rgb(50, 112, 146),
    Color32::from_rgb(64, 138, 92),
    Color32::from_rgb(122, 92, 146),
    Color32::from_rgb(146, 94, 52),
    Color32::from_rgb(56, 96, 132),
];

pub fn color(i: usize) -> Color32 {
    PALETTE[i % PALETTE.len()]
}

fn worst(sum: f64, max: f64, min: f64, side: f64) -> f64 {
    if sum <= 0.0 || min <= 0.0 || side <= 0.0 {
        return f64::INFINITY;
    }
    let s2 = side * side;
    (s2 * max / (sum * sum)).max(sum * sum / (s2 * min))
}

/// Lay out `weights` (any positive numbers) into `rect`, preserving order.
pub fn squarify(weights: &[f64], rect: Rect) -> Vec<Rect> {
    let n = weights.len();
    let mut out = vec![Rect::NOTHING; n];
    if n == 0 || rect.width() <= 0.5 || rect.height() <= 0.5 {
        return out;
    }
    let total: f64 = weights.iter().map(|w| w.max(0.0)).sum();
    if total <= 0.0 {
        return out;
    }
    let scale = (rect.width() as f64 * rect.height() as f64) / total;
    let areas: Vec<f64> = weights.iter().map(|w| (w.max(0.0) * scale).max(1e-9)).collect();

    let mut r = rect;
    let mut i = 0usize;
    while i < n {
        if r.width() <= 0.5 || r.height() <= 0.5 {
            break;
        }
        let side = r.width().min(r.height()) as f64;
        let mut sum = 0.0;
        let mut max = 0.0f64;
        let mut min = f64::INFINITY;
        let mut best = f64::INFINITY;
        let mut j = i;
        while j < n {
            let a = areas[j];
            let ns = sum + a;
            let nmax = max.max(a);
            let nmin = min.min(a);
            let ratio = worst(ns, nmax, nmin, side);
            if j > i && ratio > best {
                break;
            }
            best = ratio;
            sum = ns;
            max = nmax;
            min = nmin;
            j += 1;
        }
        if sum <= 0.0 {
            break;
        }

        if r.width() >= r.height() {
            let w = (sum / r.height() as f64) as f32;
            let w = w.clamp(0.0, r.width());
            let mut y = r.top();
            for k in i..j {
                let h = (areas[k] / sum * r.height() as f64) as f32;
                let h = h.min(r.bottom() - y).max(0.0);
                out[k] = Rect::from_min_size(pos2(r.left(), y), vec2(w, h));
                y += h;
            }
            r.min.x += w;
        } else {
            let h = (sum / r.width() as f64) as f32;
            let h = h.clamp(0.0, r.height());
            let mut x = r.left();
            for k in i..j {
                let w = (areas[k] / sum * r.width() as f64) as f32;
                let w = w.min(r.right() - x).max(0.0);
                out[k] = Rect::from_min_size(pos2(x, r.top()), vec2(w, h));
                x += w;
            }
            r.min.y += h;
        }
        i = j;
    }
    out
}
