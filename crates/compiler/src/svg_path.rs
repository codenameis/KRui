//! SVG `d="..."` vector path parser built on `svgtypes::PathParser`, converting relative
//! commands, smooth curves (`S`/`T`), and elliptical arcs (`A`/`a`) into [`CompiledPathSegment`]s.

use crate::CompileError;
use svgtypes::{PathParser, PathSegment as SvgSeg};

/// Serializable path segment inside a [`crate::CompiledSvgOp`].
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CompiledPathSegment {
    /// Move path cursor to `(x, y)`.
    MoveTo(f32, f32),
    /// Straight line segment to `(x, y)`.
    LineTo(f32, f32),
    /// Quadratic Bezier curve `(cx, cy, x, y)`.
    QuadTo { cx: f32, cy: f32, x: f32, y: f32 },
    /// Cubic Bezier curve `(c1x, c1y, c2x, c2y, x, y)`.
    CubicTo {
        c1x: f32,
        c1y: f32,
        c2x: f32,
        c2y: f32,
        x: f32,
        y: f32,
    },
    /// Center-parameterized elliptical arc `(cx, cy, rx, ry, start_rad, sweep_rad, x_rotation_rad)`.
    Arc {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        start_rad: f32,
        sweep_rad: f32,
        x_rotation_rad: f32,
    },
    /// Close current subpath back to its origin.
    Close,
}

#[derive(Clone, Copy, Default)]
struct PathCursor {
    cur: (f64, f64),
    subpath: (f64, f64),
    last_cubic_ctrl: Option<(f64, f64)>,
    last_quad_ctrl: Option<(f64, f64)>,
}

impl PathCursor {
    fn resolve(&self, abs: bool, x: f64, y: f64) -> (f64, f64) {
        if abs {
            (x, y)
        } else {
            (self.cur.0 + x, self.cur.1 + y)
        }
    }
}

/// Parses an SVG `d="..."` attribute string into normalized absolute [`CompiledPathSegment`]s.
pub(crate) fn parse_svg_path_data(
    source: &str,
    attr_offset: usize,
    d: &str,
) -> Result<Vec<CompiledPathSegment>, CompileError> {
    let mut out = Vec::new();
    let mut c = PathCursor::default();
    for item in PathParser::from(d) {
        let seg = item.map_err(|e| {
            CompileError::at_offset(source, attr_offset, format!("Invalid SVG path `d`: {e}"))
        })?;
        if !svg_seg_is_finite(&seg) {
            return Err(CompileError::at_offset(
                source,
                attr_offset,
                format!("Non-finite coordinate in SVG path `d`: `{d}`"),
            ));
        }
        append_svg_segment(&mut c, seg, &mut out);
    }
    if out.iter().any(|s| !compiled_seg_is_finite(s)) {
        return Err(CompileError::at_offset(
            source,
            attr_offset,
            format!("SVG path `d` overflowed to non-finite coordinates: `{d}`"),
        ));
    }
    Ok(out)
}

fn svg_seg_is_finite(seg: &SvgSeg) -> bool {
    match *seg {
        SvgSeg::MoveTo { x, y, .. }
        | SvgSeg::LineTo { x, y, .. }
        | SvgSeg::SmoothQuadratic { x, y, .. } => x.is_finite() && y.is_finite(),
        SvgSeg::HorizontalLineTo { x, .. } => x.is_finite(),
        SvgSeg::VerticalLineTo { y, .. } => y.is_finite(),
        SvgSeg::CurveTo {
            x1,
            y1,
            x2,
            y2,
            x,
            y,
            ..
        } => [x1, y1, x2, y2, x, y].into_iter().all(f64::is_finite),
        SvgSeg::SmoothCurveTo { x2, y2, x, y, .. }
        | SvgSeg::Quadratic {
            x1: x2,
            y1: y2,
            x,
            y,
            ..
        } => [x2, y2, x, y].into_iter().all(f64::is_finite),
        SvgSeg::EllipticalArc {
            rx,
            ry,
            x_axis_rotation,
            x,
            y,
            ..
        } => [rx, ry, x_axis_rotation, x, y]
            .into_iter()
            .all(f64::is_finite),
        SvgSeg::ClosePath { .. } => true,
    }
}

fn compiled_seg_is_finite(seg: &CompiledPathSegment) -> bool {
    match *seg {
        CompiledPathSegment::MoveTo(x, y) | CompiledPathSegment::LineTo(x, y) => {
            x.is_finite() && y.is_finite()
        }
        CompiledPathSegment::QuadTo { cx, cy, x, y } => {
            [cx, cy, x, y].into_iter().all(f32::is_finite)
        }
        CompiledPathSegment::CubicTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
        } => [c1x, c1y, c2x, c2y, x, y].into_iter().all(f32::is_finite),
        CompiledPathSegment::Arc {
            cx,
            cy,
            rx,
            ry,
            start_rad,
            sweep_rad,
            x_rotation_rad,
        } => [cx, cy, rx, ry, start_rad, sweep_rad, x_rotation_rad]
            .into_iter()
            .all(f32::is_finite),
        CompiledPathSegment::Close => true,
    }
}

fn append_svg_segment(c: &mut PathCursor, seg: SvgSeg, out: &mut Vec<CompiledPathSegment>) {
    match seg {
        SvgSeg::MoveTo { abs, x, y } => {
            let p = c.resolve(abs, x, y);
            (c.cur, c.subpath) = (p, p);
            (c.last_cubic_ctrl, c.last_quad_ctrl) = (None, None);
            out.push(CompiledPathSegment::MoveTo(p.0 as f32, p.1 as f32));
        }
        SvgSeg::LineTo { abs, x, y } => {
            let p = c.resolve(abs, x, y);
            c.cur = p;
            (c.last_cubic_ctrl, c.last_quad_ctrl) = (None, None);
            out.push(CompiledPathSegment::LineTo(p.0 as f32, p.1 as f32));
        }
        SvgSeg::HorizontalLineTo { abs, x } => {
            let nx = if abs { x } else { c.cur.0 + x };
            c.cur.0 = nx;
            (c.last_cubic_ctrl, c.last_quad_ctrl) = (None, None);
            out.push(CompiledPathSegment::LineTo(nx as f32, c.cur.1 as f32));
        }
        SvgSeg::VerticalLineTo { abs, y } => {
            let ny = if abs { y } else { c.cur.1 + y };
            c.cur.1 = ny;
            (c.last_cubic_ctrl, c.last_quad_ctrl) = (None, None);
            out.push(CompiledPathSegment::LineTo(c.cur.0 as f32, ny as f32));
        }
        SvgSeg::ClosePath { .. } => {
            c.cur = c.subpath;
            (c.last_cubic_ctrl, c.last_quad_ctrl) = (None, None);
            out.push(CompiledPathSegment::Close);
        }
        other => append_svg_curve_segment(c, other, out),
    }
}

fn push_cubic(
    c: &mut PathCursor,
    p1: (f64, f64),
    p2: (f64, f64),
    p: (f64, f64),
    out: &mut Vec<CompiledPathSegment>,
) {
    c.cur = p;
    (c.last_cubic_ctrl, c.last_quad_ctrl) = (Some(p2), None);
    out.push(CompiledPathSegment::CubicTo {
        c1x: p1.0 as f32,
        c1y: p1.1 as f32,
        c2x: p2.0 as f32,
        c2y: p2.1 as f32,
        x: p.0 as f32,
        y: p.1 as f32,
    });
}

fn push_quad(
    c: &mut PathCursor,
    cp: (f64, f64),
    p: (f64, f64),
    out: &mut Vec<CompiledPathSegment>,
) {
    c.cur = p;
    (c.last_cubic_ctrl, c.last_quad_ctrl) = (None, Some(cp));
    out.push(CompiledPathSegment::QuadTo {
        cx: cp.0 as f32,
        cy: cp.1 as f32,
        x: p.0 as f32,
        y: p.1 as f32,
    });
}

fn append_svg_curve_segment(c: &mut PathCursor, seg: SvgSeg, out: &mut Vec<CompiledPathSegment>) {
    match seg {
        SvgSeg::CurveTo {
            abs,
            x1,
            y1,
            x2,
            y2,
            x,
            y,
        } => push_cubic(
            c,
            c.resolve(abs, x1, y1),
            c.resolve(abs, x2, y2),
            c.resolve(abs, x, y),
            out,
        ),
        SvgSeg::SmoothCurveTo { abs, x2, y2, x, y } => {
            let p1 = c
                .last_cubic_ctrl
                .map_or(c.cur, |(lx, ly)| (2.0 * c.cur.0 - lx, 2.0 * c.cur.1 - ly));
            push_cubic(c, p1, c.resolve(abs, x2, y2), c.resolve(abs, x, y), out);
        }
        other => append_svg_quad_or_arc_segment(c, other, out),
    }
}

fn append_svg_quad_or_arc_segment(
    c: &mut PathCursor,
    seg: SvgSeg,
    out: &mut Vec<CompiledPathSegment>,
) {
    match seg {
        SvgSeg::Quadratic { abs, x1, y1, x, y } => {
            push_quad(c, c.resolve(abs, x1, y1), c.resolve(abs, x, y), out);
        }
        SvgSeg::SmoothQuadratic { abs, x, y } => {
            let cp = c
                .last_quad_ctrl
                .map_or(c.cur, |(qx, qy)| (2.0 * c.cur.0 - qx, 2.0 * c.cur.1 - qy));
            push_quad(c, cp, c.resolve(abs, x, y), out);
        }
        SvgSeg::EllipticalArc {
            abs,
            rx,
            ry,
            x_axis_rotation,
            large_arc,
            sweep,
            x,
            y,
        } => {
            let p = c.resolve(abs, x, y);
            out.push(endpoint_arc_to_center(
                c.cur,
                p,
                rx,
                ry,
                x_axis_rotation,
                large_arc,
                sweep,
            ));
            c.cur = p;
            (c.last_cubic_ctrl, c.last_quad_ctrl) = (None, None);
        }
        _ => {}
    }
}

/// Converts W3C SVG endpoint elliptical arc parameters into center parameterization (SVG F.6.5/F.6.6).
fn endpoint_arc_to_center(
    (x1, y1): (f64, f64),
    (x2, y2): (f64, f64),
    rx_raw: f64,
    ry_raw: f64,
    x_rot_deg: f64,
    large_arc: bool,
    sweep: bool,
) -> CompiledPathSegment {
    let (mut rx, mut ry) = (rx_raw.abs(), ry_raw.abs());
    if ((x1 - x2).abs() < 1e-9 && (y1 - y2).abs() < 1e-9) || rx < 1e-9 || ry < 1e-9 {
        return CompiledPathSegment::LineTo(x2 as f32, y2 as f32);
    }
    let phi = x_rot_deg.to_radians();
    let (sin_p, cos_p) = phi.sin_cos();
    let (dx2, dy2) = ((x1 - x2) * 0.5, (y1 - y2) * 0.5);
    let x1p = cos_p * dx2 + sin_p * dy2;
    let y1p = -sin_p * dx2 + cos_p * dy2;
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let rx2 = rx * rx;
    let ry2 = ry * ry;
    let num = (rx2 * ry2 - rx2 * y1p * y1p - ry2 * x1p * x1p).max(0.0);
    let den = (rx2 * y1p * y1p + ry2 * x1p * x1p).max(1e-18);
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let coef = sign * (num / den).sqrt();
    let cxp = coef * (rx * y1p / ry);
    let cyp = coef * (-ry * x1p / rx);
    let cx = cos_p * cxp - sin_p * cyp + (x1 + x2) * 0.5;
    let cy = sin_p * cxp + cos_p * cyp + (y1 + y2) * 0.5;
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let start = vec_angle(1.0, 0.0, ux, uy);
    let mut dtheta = vec_angle(ux, uy, vx, vy);
    if !sweep && dtheta > 0.0 {
        dtheta -= std::f64::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f64::consts::TAU;
    }
    CompiledPathSegment::Arc {
        cx: cx as f32,
        cy: cy as f32,
        rx: rx as f32,
        ry: ry as f32,
        start_rad: start as f32,
        sweep_rad: dtheta as f32,
        x_rotation_rad: phi as f32,
    }
}

fn vec_angle(ux: f64, uy: f64, vx: f64, vy: f64) -> f64 {
    let dot = ux * vx + uy * vy;
    let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy))
        .sqrt()
        .max(1e-18);
    let ang = (dot / len).clamp(-1.0, 1.0).acos();
    if ux * vy - uy * vx < 0.0 {
        -ang
    } else {
        ang
    }
}
