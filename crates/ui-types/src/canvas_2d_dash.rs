//! Path flattening and stroke-dash subdivision helpers for `CanvasContext2D`.

use super::{CanvasContext2D, PathSegment};

const CURVE_FLATTEN_STEPS: usize = 16;
const MAX_DASH_STEPS_PER_SEGMENT: usize = 8_192;

pub(crate) fn dash_path_segments(
    segments: &[PathSegment],
    dash: &[f32],
    offset: f32,
) -> Vec<PathSegment> {
    let valid_dash: Vec<f32> = dash
        .iter()
        .copied()
        .filter(|d| d.is_finite() && *d >= 0.0)
        .collect();
    let cycle: f32 = valid_dash.iter().copied().sum();
    if valid_dash.is_empty() || !cycle.is_finite() || cycle <= 1e-4 {
        return segments.to_vec();
    }
    let polylines = flatten_subpaths(segments);
    let mut out = Vec::new();
    let start_off = if offset.is_finite() {
        offset.rem_euclid(cycle)
    } else {
        0.0
    };
    for poly in &polylines {
        dash_single_polyline(poly, &valid_dash, cycle, start_off, &mut out);
    }
    out
}

fn flatten_subpaths(segments: &[PathSegment]) -> Vec<Vec<(f32, f32)>> {
    let _ = CanvasContext2D::new;
    let mut polys: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut cur: Vec<(f32, f32)> = Vec::new();
    for &seg in segments {
        match seg {
            PathSegment::MoveTo(x, y) => {
                if cur.len() >= 2 {
                    polys.push(std::mem::take(&mut cur));
                }
                cur.clear();
                cur.push((x, y));
            }
            PathSegment::LineTo(x, y) => cur.push((x, y)),
            PathSegment::QuadTo { cx, cy, x, y } => flatten_quad(&mut cur, cx, cy, x, y),
            PathSegment::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => flatten_cubic(&mut cur, (c1x, c1y), (c2x, c2y), (x, y)),
            PathSegment::Arc {
                cx,
                cy,
                rx,
                ry,
                start_rad,
                sweep_rad,
                x_rotation_rad,
            } => flatten_arc(
                &mut cur,
                (cx, cy, rx, ry),
                (start_rad, sweep_rad, x_rotation_rad),
            ),
            PathSegment::Close => {
                if let Some(&first) = cur.first() {
                    if cur.last().copied() != Some(first) {
                        cur.push(first);
                    }
                }
                if cur.len() >= 2 {
                    polys.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
        }
    }
    if cur.len() >= 2 {
        polys.push(cur);
    }
    polys
}

fn flatten_quad(cur: &mut Vec<(f32, f32)>, cx: f32, cy: f32, x: f32, y: f32) {
    let (x0, y0) = cur.last().copied().unwrap_or((x, y));
    for s in 1..=CURVE_FLATTEN_STEPS {
        let t = s as f32 / CURVE_FLATTEN_STEPS as f32;
        let u = 1.0 - t;
        cur.push((
            u * u * x0 + 2.0 * u * t * cx + t * t * x,
            u * u * y0 + 2.0 * u * t * cy + t * t * y,
        ));
    }
}

fn flatten_cubic(
    cur: &mut Vec<(f32, f32)>,
    (c1x, c1y): (f32, f32),
    (c2x, c2y): (f32, f32),
    (x, y): (f32, f32),
) {
    let (x0, y0) = cur.last().copied().unwrap_or((x, y));
    for s in 1..=CURVE_FLATTEN_STEPS {
        let t = s as f32 / CURVE_FLATTEN_STEPS as f32;
        let u = 1.0 - t;
        cur.push((
            u * u * u * x0 + 3.0 * u * u * t * c1x + 3.0 * u * t * t * c2x + t * t * t * x,
            u * u * u * y0 + 3.0 * u * u * t * c1y + 3.0 * u * t * t * c2y + t * t * t * y,
        ));
    }
}

fn flatten_arc(
    cur: &mut Vec<(f32, f32)>,
    (cx, cy, rx, ry): (f32, f32, f32, f32),
    (start, sweep, rot): (f32, f32, f32),
) {
    if !start.is_finite() || !sweep.is_finite() || !rot.is_finite() {
        return;
    }
    let (sin_r, cos_r) = rot.sin_cos();
    let steps = ((sweep.abs() / std::f32::consts::FRAC_PI_4).ceil() as usize)
        .saturating_mul(8)
        .clamp(4, 64);
    for s in 0..=steps {
        let ang = start + sweep * (s as f32 / steps as f32);
        let (ax, ay) = (rx * ang.cos(), ry * ang.sin());
        let px = cx + ax * cos_r - ay * sin_r;
        let py = cy + ax * sin_r + ay * cos_r;
        if s > 0 || cur.is_empty() {
            cur.push((px, py));
        }
    }
}

fn initial_dash_cursor(dash: &[f32], start_off: f32) -> (usize, f32) {
    let mut rem = start_off.max(0.0);
    let mut idx = 0_usize;
    for _ in 0..dash.len() * 2 {
        let cur = dash[idx];
        if rem < cur && cur > 1e-5 {
            return (idx, (cur - rem).max(0.0));
        }
        rem = (rem - cur).max(0.0);
        idx = (idx + 1) % dash.len();
    }
    (idx, dash[idx].max(1e-4))
}

fn dash_single_polyline(
    poly: &[(f32, f32)],
    dash: &[f32],
    cycle: f32,
    start_off: f32,
    out: &mut Vec<PathSegment>,
) {
    let (mut dash_idx, mut dash_rem) = initial_dash_cursor(dash, start_off);
    let mut pen_active = false;
    for seg in poly.windows(2) {
        let ((x0, y0), (x1, y1)) = (seg[0], seg[1]);
        if !x0.is_finite() || !y0.is_finite() || !x1.is_finite() || !y1.is_finite() {
            pen_active = false;
            continue;
        }
        let seg_len = (x1 - x0).hypot(y1 - y0);
        if !seg_len.is_finite() || seg_len <= 1e-5 {
            continue;
        }
        if seg_len / cycle > MAX_DASH_STEPS_PER_SEGMENT as f32 {
            out.push(PathSegment::MoveTo(x0, y0));
            out.push(PathSegment::LineTo(x1, y1));
            pen_active = false;
            continue;
        }
        let mut t = 0.0_f32;
        let mut steps = 0_usize;
        while t < 1.0 - 1e-5 && steps < MAX_DASH_STEPS_PER_SEGMENT {
            steps += 1;
            let draw_on = dash_idx.is_multiple_of(2);
            let avail_len = (1.0 - t) * seg_len;
            let step_len = dash_rem.min(avail_len);
            let t_next = (t + step_len / seg_len).min(1.0);
            if t_next <= t {
                break;
            }
            if draw_on && step_len > 1e-5 {
                let p0 = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
                let p1 = (x0 + (x1 - x0) * t_next, y0 + (y1 - y0) * t_next);
                if !pen_active {
                    out.push(PathSegment::MoveTo(p0.0, p0.1));
                    pen_active = true;
                }
                out.push(PathSegment::LineTo(p1.0, p1.1));
            } else {
                pen_active = false;
            }
            dash_rem -= step_len;
            if dash_rem <= 1e-5 {
                for _ in 0..dash.len() {
                    dash_idx = (dash_idx + 1) % dash.len();
                    if dash[dash_idx] > 1e-5 {
                        break;
                    }
                }
                dash_rem = dash[dash_idx].max(1e-4);
                pen_active = false;
            }
            t = t_next;
        }
    }
}
