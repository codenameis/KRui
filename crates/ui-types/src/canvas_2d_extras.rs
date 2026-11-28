//! Gradient, text-alignment, transform-state stack, and path helpers for [`super::CanvasContext2D`].

use super::{CanvasFillRule, CanvasLineCap, CanvasLineJoin, PathSegment};
pub(crate) use crate::canvas_2d_dash::dash_path_segments;
use krui_style::{Affine2, Color};

const CIRCLE_KAPPA: f32 = 0.552_284_8;

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
        /// Start X.
        x0: f32,
        /// Start Y.
        y0: f32,
        /// End X.
        x1: f32,
        /// End Y.
        y1: f32,
        /// Ordered color stops.
        stops: Vec<CanvasGradientStop>,
    },
    /// Radial gradient interpolating from circle `(x0, y0, r0)` to `(x1, y1, r1)`.
    Radial {
        /// Inner circle center X.
        x0: f32,
        /// Inner circle center Y.
        y0: f32,
        /// Inner circle radius.
        r0: f32,
        /// Outer circle center X.
        x1: f32,
        /// Outer circle center Y.
        y1: f32,
        /// Outer circle radius.
        r1: f32,
        /// Ordered color stops.
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
    let (cdx, cdy, dr) = (x1 - x0, y1 - y0, r1 - r0);
    if cdx.abs() <= 1e-4 && cdy.abs() <= 1e-4 {
        let dist = (px - x0).hypot(py - y0);
        return if dr.abs() > 1e-5 {
            ((dist - r0) / dr).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    let (fx, fy) = (px - x0, py - y0);
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
    let (t0, t1) = ((-b + disc) / (2.0 * a), (-b - disc) / (2.0 * a));
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
