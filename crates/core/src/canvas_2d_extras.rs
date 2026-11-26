//! Gradient, text-alignment, transform-state stack, and path-dashing helpers for [`super::CanvasContext2D`].

use super::{CanvasFillRule, CanvasLineCap, CanvasLineJoin, PathSegment};
use krui_style::{Affine2, Color};

const CIRCLE_KAPPA: f32 = 0.552_284_8;
const CURVE_FLATTEN_STEPS: usize = 16;

/// Color stop along a [`CanvasGradient`] (`offset` normalized to `0.0..=1.0`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasGradientStop {
    /// Normalized stop offset in `0.0..=1.0`.
    pub offset: f32,
    /// Stop RGBA color.
    pub color: Color,
}

/// Linear or radial color gradient usable with `CanvasContext2D::fill_*_gradient`.
#[derive(Clone, Debug, PartialEq)]
pub enum CanvasGradient {
    /// Linear gradient from `(x0, y0)` to `(x1, y1)`.
    Linear {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        stops: Vec<CanvasGradientStop>,
    },
    /// Radial gradient interpolating from circle `(x0, y0, r0)` to `(x1, y1, r1)`.
    Radial {
        x0: f32,
        y0: f32,
        r0: f32,
        x1: f32,
        y1: f32,
        r1: f32,
        stops: Vec<CanvasGradientStop>,
    },
}

impl CanvasGradient {
    /// Creates an empty linear gradient from `(x0, y0)` to `(x1, y1)`.
    pub fn linear(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self::Linear {
            x0,
            y0,
            x1,
            y1,
            stops: Vec::new(),
        }
    }

    /// Creates an empty radial gradient from `(x0, y0, r0)` to `(x1, y1, r1)`.
    pub fn radial(x0: f32, y0: f32, r0: f32, x1: f32, y1: f32, r1: f32) -> Self {
        Self::Radial {
            x0,
            y0,
            r0: r0.max(0.0),
            x1,
            y1,
            r1: r1.max(0.0),
            stops: Vec::new(),
        }
    }

    /// Appends a stop at `offset` (`0.0..=1.0`) in-place, preserving stop order.
    pub fn add_stop(&mut self, offset: f32, color: Color) -> &mut Self {
        let clamped = if offset.is_finite() {
            offset.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let stops = match self {
            Self::Linear { stops, .. } | Self::Radial { stops, .. } => stops,
        };
        stops.push(CanvasGradientStop {
            offset: clamped,
            color,
        });
        stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        self
    }

    /// Builder variant of [`Self::add_stop`].
    pub fn with_stop(mut self, offset: f32, color: Color) -> Self {
        self.add_stop(offset, color);
        self
    }

    /// Returns the gradient stop slice.
    pub fn stops(&self) -> &[CanvasGradientStop] {
        match self {
            Self::Linear { stops, .. } | Self::Radial { stops, .. } => stops,
        }
    }

    /// Evaluates the interpolated RGBA color at canvas coordinate `(x, y)`.
    pub fn sample(&self, x: f32, y: f32) -> Color {
        let (t, stops) = match self {
            Self::Linear {
                x0,
                y0,
                x1,
                y1,
                stops,
            } => {
                let (dx, dy) = (x1 - x0, y1 - y0);
                let len2 = dx * dx + dy * dy;
                let t = if len2 > 1e-6 {
                    ((x - x0) * dx + (y - y0) * dy) / len2
                } else {
                    0.0
                };
                (t.clamp(0.0, 1.0), stops.as_slice())
            }
            Self::Radial {
                x0,
                y0,
                r0,
                x1,
                y1,
                r1,
                stops,
            } => (
                sample_radial_t((x, y), (*x0, *y0, *r0), (*x1, *y1, *r1)),
                stops.as_slice(),
            ),
        };
        sample_stop_ramp(stops, t)
    }
}

fn sample_radial_t(
    (px, py): (f32, f32),
    (x0, y0, r0): (f32, f32, f32),
    (x1, y1, r1): (f32, f32, f32),
) -> f32 {
    let cdx = x1 - x0;
    let cdy = y1 - y0;
    let dr = r1 - r0;
    if cdx.abs() <= 1e-4 && cdy.abs() <= 1e-4 {
        let dist = (px - x0).hypot(py - y0);
        return if dr.abs() > 1e-5 {
            ((dist - r0) / dr).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    let fx = px - x0;
    let fy = py - y0;
    let a = cdx * cdx + cdy * cdy - dr * dr;
    let b = -2.0 * (fx * cdx + fy * cdy + r0 * dr);
    let c = fx * fx + fy * fy - r0 * r0;
    if a.abs() <= 1e-5 {
        return if b.abs() > 1e-5 {
            (-c / b).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    let disc = (b * b - 4.0 * a * c).max(0.0).sqrt();
    let t0 = (-b + disc) / (2.0 * a);
    let t1 = (-b - disc) / (2.0 * a);
    let best = if r0 + t0 * dr >= 0.0 && (t0 >= t1 || r0 + t1 * dr < 0.0) {
        t0
    } else {
        t1
    };
    best.clamp(0.0, 1.0)
}

fn sample_stop_ramp(stops: &[CanvasGradientStop], t: f32) -> Color {
    if stops.is_empty() {
        return Color::TRANSPARENT;
    }
    if stops.len() == 1 || t <= stops[0].offset {
        return stops[0].color;
    }
    for win in stops.windows(2) {
        let (a, b) = (win[0], win[1]);
        if t <= b.offset {
            let span = (b.offset - a.offset).max(1e-6);
            let local_t = ((t - a.offset) / span).clamp(0.0, 1.0);
            return lerp_color_u8(a.color, b.color, local_t);
        }
    }
    stops[stops.len() - 1].color
}

fn lerp_color_u8(a: Color, b: Color, t: f32) -> Color {
    let mix = |u: u8, v: u8| -> u8 {
        (u as f32 + (v as f32 - u as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color {
        r: mix(a.r, b.r),
        g: mix(a.g, b.g),
        b: mix(a.b, b.b),
        a: mix(a.a, b.a),
    }
}

/// Horizontal text anchor alignment for `CanvasContext2D::draw_text_styled`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CanvasTextAlign {
    /// `x` marks the left edge of the text run (default).
    #[default]
    Left,
    /// `x` marks the horizontal center of the text run.
    Center,
    /// `x` marks the right edge of the text run.
    Right,
}

#[derive(Clone, Debug)]
pub(crate) struct CanvasState {
    pub transform: Affine2,
    pub global_alpha: f32,
    pub line_cap: CanvasLineCap,
    pub line_join: CanvasLineJoin,
    pub fill_rule: CanvasFillRule,
    pub line_dash: Vec<f32>,
    pub line_dash_offset: f32,
    pub clip_depth: usize,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            transform: Affine2::IDENTITY,
            global_alpha: 1.0,
            line_cap: CanvasLineCap::Butt,
            line_join: CanvasLineJoin::Miter,
            fill_rule: CanvasFillRule::NonZero,
            line_dash: Vec::new(),
            line_dash_offset: 0.0,
            clip_depth: 0,
        }
    }
}

pub(crate) fn scale_color_alpha(color: Color, global_alpha: f32) -> Color {
    let a = ((color.a as f32 / 255.0) * global_alpha.clamp(0.0, 1.0) * 255.0)
        .round()
        .clamp(0.0, 255.0) as u8;
    Color {
        r: color.r,
        g: color.g,
        b: color.b,
        a,
    }
}

pub(crate) fn rect_to_path_segments(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
) -> Vec<PathSegment> {
    if w <= 0.0 || h <= 0.0 {
        return Vec::new();
    }
    let r = radius.clamp(0.0, (w.min(h) * 0.5).max(0.0));
    if r <= 0.05 {
        return vec![
            PathSegment::MoveTo(x, y),
            PathSegment::LineTo(x + w, y),
            PathSegment::LineTo(x + w, y + h),
            PathSegment::LineTo(x, y + h),
            PathSegment::Close,
        ];
    }
    let k = r * CIRCLE_KAPPA;
    vec![
        PathSegment::MoveTo(x + r, y),
        PathSegment::LineTo(x + w - r, y),
        PathSegment::CubicTo {
            c1x: x + w - r + k,
            c1y: y,
            c2x: x + w,
            c2y: y + r - k,
            x: x + w,
            y: y + r,
        },
        PathSegment::LineTo(x + w, y + h - r),
        PathSegment::CubicTo {
            c1x: x + w,
            c1y: y + h - r + k,
            c2x: x + w - r + k,
            c2y: y + h,
            x: x + w - r,
            y: y + h,
        },
        PathSegment::LineTo(x + r, y + h),
        PathSegment::CubicTo {
            c1x: x + r - k,
            c1y: y + h,
            c2x: x,
            c2y: y + h - r + k,
            x,
            y: y + h - r,
        },
        PathSegment::LineTo(x, y + r),
        PathSegment::CubicTo {
            c1x: x,
            c1y: y + r - k,
            c2x: x + r - k,
            c2y: y,
            x: x + r,
            y,
        },
        PathSegment::Close,
    ]
}

pub(crate) fn circle_to_path_segments(cx: f32, cy: f32, radius: f32) -> Vec<PathSegment> {
    if radius <= 0.0 {
        return Vec::new();
    }
    vec![
        PathSegment::MoveTo(cx + radius, cy),
        PathSegment::Arc {
            cx,
            cy,
            rx: radius,
            ry: radius,
            start_rad: 0.0,
            sweep_rad: std::f32::consts::TAU,
            x_rotation_rad: 0.0,
        },
        PathSegment::Close,
    ]
}

pub(crate) fn dash_path_segments(
    segments: &[PathSegment],
    dash: &[f32],
    offset: f32,
) -> Vec<PathSegment> {
    let cycle: f32 = dash.iter().copied().sum();
    if dash.is_empty() || cycle <= 1e-4 {
        return segments.to_vec();
    }
    let polylines = flatten_subpaths(segments);
    let mut out = Vec::new();
    let start_off = offset.rem_euclid(cycle);
    for poly in &polylines {
        dash_single_polyline(poly, dash, start_off, &mut out);
    }
    out
}

fn flatten_subpaths(segments: &[PathSegment]) -> Vec<Vec<(f32, f32)>> {
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
    let (sin_r, cos_r) = rot.sin_cos();
    let steps = ((sweep.abs() / std::f32::consts::FRAC_PI_4).ceil() as usize * 8).clamp(4, 64);
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
    let mut rem = start_off;
    let mut idx = 0_usize;
    while rem >= dash[idx] {
        rem -= dash[idx];
        idx = (idx + 1) % dash.len();
    }
    (idx, (dash[idx] - rem).max(0.0))
}

fn dash_single_polyline(
    poly: &[(f32, f32)],
    dash: &[f32],
    start_off: f32,
    out: &mut Vec<PathSegment>,
) {
    let (mut dash_idx, mut dash_rem) = initial_dash_cursor(dash, start_off);
    let mut pen_active = false;
    for seg in poly.windows(2) {
        let ((x0, y0), (x1, y1)) = (seg[0], seg[1]);
        let seg_len = (x1 - x0).hypot(y1 - y0);
        if seg_len <= 1e-5 {
            continue;
        }
        let mut t = 0.0_f32;
        while t < 1.0 - 1e-5 {
            let draw_on = dash_idx.is_multiple_of(2);
            let avail_len = (1.0 - t) * seg_len;
            let step_len = dash_rem.min(avail_len);
            let t_next = (t + step_len / seg_len).min(1.0);
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
                dash_idx = (dash_idx + 1) % dash.len();
                dash_rem = dash[dash_idx];
                pen_active = false;
            }
            t = t_next;
        }
    }
}
