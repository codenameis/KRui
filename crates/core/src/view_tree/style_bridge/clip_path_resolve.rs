//! Resolution and hit-testing of CSS `clip-path` (`polygon()`, `path()`, `circle()`, `ellipse()`, `inset()`).

use crate::scene::{PathSegment, ResolvedClipPath};
use krui_layout::LayoutRect;
use krui_style::{Affine2, ClipCoord, ClipFillRule, ClipPathCmd, ClipRadius, ClipShape};
use std::f32::consts::TAU;
use std::sync::Arc;

fn resolve_radius(r: ClipRadius, pos: f32, extent: f32, fallback_other: f32) -> f32 {
    match r {
        ClipRadius::Length(c) => (c.px + extent * c.frac).max(0.0),
        ClipRadius::ClosestSide => pos
            .abs()
            .min((extent - pos).abs())
            .min(fallback_other)
            .max(0.0),
        ClipRadius::FarthestSide => pos.abs().max((extent - pos).abs()).max(fallback_other),
    }
}

fn push_ellipse_segments(segs: &mut Vec<PathSegment>, cx: f32, cy: f32, rx: f32, ry: f32) {
    let rx = rx.max(0.0);
    let ry = ry.max(0.0);
    segs.push(PathSegment::MoveTo(cx + rx, cy));
    segs.push(PathSegment::Arc {
        cx,
        cy,
        rx,
        ry,
        start_rad: 0.0,
        sweep_rad: TAU * 0.5,
        x_rotation_rad: 0.0,
    });
    segs.push(PathSegment::Arc {
        cx,
        cy,
        rx,
        ry,
        start_rad: std::f32::consts::PI,
        sweep_rad: TAU * 0.5,
        x_rotation_rad: 0.0,
    });
    segs.push(PathSegment::Close);
}

fn push_rounded_rect_segments(
    segs: &mut Vec<PathSegment>,
    (x, y, w, h): (f32, f32, f32, f32),
    [tl, tr, br, bl]: [f32; 4],
) {
    const KAPPA: f32 = 0.552_284_8;
    let [tl, tr, br, bl] =
        crate::view_tree::clip_shape::normalize_radii_4_core(w, h, [tl, tr, br, bl]);
    segs.push(PathSegment::MoveTo(x + tl, y));
    segs.push(PathSegment::LineTo(x + w - tr, y));
    if tr > 0.05 {
        let k = tr * KAPPA;
        segs.push(PathSegment::CubicTo {
            c1x: x + w - tr + k,
            c1y: y,
            c2x: x + w,
            c2y: y + tr - k,
            x: x + w,
            y: y + tr,
        });
    }
    segs.push(PathSegment::LineTo(x + w, y + h - br));
    if br > 0.05 {
        let k = br * KAPPA;
        segs.push(PathSegment::CubicTo {
            c1x: x + w,
            c1y: y + h - br + k,
            c2x: x + w - br + k,
            c2y: y + h,
            x: x + w - br,
            y: y + h,
        });
    }
    segs.push(PathSegment::LineTo(x + bl, y + h));
    if bl > 0.05 {
        let k = bl * KAPPA;
        segs.push(PathSegment::CubicTo {
            c1x: x + bl - k,
            c1y: y + h,
            c2x: x,
            c2y: y + h - bl + k,
            x,
            y: y + h - bl,
        });
    }
    segs.push(PathSegment::LineTo(x, y + tl));
    if tl > 0.05 {
        let k = tl * KAPPA;
        segs.push(PathSegment::CubicTo {
            c1x: x,
            c1y: y + tl - k,
            c2x: x + tl - k,
            c2y: y,
            x: x + tl,
            y,
        });
    }
    segs.push(PathSegment::Close);
}

fn push_inset_or_ellipse(shape: ClipShape, rect: LayoutRect, segs: &mut Vec<PathSegment>) {
    match shape {
        ClipShape::Circle { cx, cy, radius } => {
            let x = rect.x + cx.resolve(rect.width);
            let y = rect.y + cy.resolve(rect.height);
            let diag = (rect.width * rect.width + rect.height * rect.height).sqrt()
                * std::f32::consts::FRAC_1_SQRT_2;
            let r = match radius {
                ClipRadius::Length(c) => (c.px + diag * c.frac).max(0.0),
                ClipRadius::ClosestSide => {
                    let dx = (x - rect.x).abs().min((rect.x + rect.width - x).abs());
                    let dy = (y - rect.y).abs().min((rect.y + rect.height - y).abs());
                    dx.min(dy).max(0.0)
                }
                ClipRadius::FarthestSide => {
                    let dx = (x - rect.x).abs().max((rect.x + rect.width - x).abs());
                    let dy = (y - rect.y).abs().max((rect.y + rect.height - y).abs());
                    dx.max(dy).max(0.0)
                }
            };
            push_ellipse_segments(segs, x, y, r, r);
        }
        ClipShape::Ellipse { cx, cy, rx, ry } => {
            let lx = cx.resolve(rect.width);
            let ly = cy.resolve(rect.height);
            let rx_px = resolve_radius(rx, lx, rect.width, f32::INFINITY);
            let ry_px = resolve_radius(ry, ly, rect.height, f32::INFINITY);
            push_ellipse_segments(segs, rect.x + lx, rect.y + ly, rx_px, ry_px);
        }
        ClipShape::Inset {
            top,
            right,
            bottom,
            left,
            radii,
        } => {
            let (t, r, b, l) = (
                top.resolve(rect.height),
                right.resolve(rect.width),
                bottom.resolve(rect.height),
                left.resolve(rect.width),
            );
            let w = (rect.width - l - r).max(0.0);
            let h = (rect.height - t - b).max(0.0);
            push_rounded_rect_segments(segs, (rect.x + l, rect.y + t, w, h), radii);
        }
        _ => {}
    }
}

/// Resolves a CSS [`ClipShape`] over `world_rect` and `transform` into scene segments.
pub(crate) fn resolve_clip_path(
    shape: ClipShape,
    world_rect: LayoutRect,
    transform: Affine2,
) -> ResolvedClipPath {
    let mut segs = Vec::new();
    let fill_rule = match shape {
        ClipShape::Polygon { fill_rule, points } => {
            points.with_coords(|coords: &[ClipCoord]| {
                for (idx, &[cx, cy]) in coords.as_chunks::<2>().0.iter().enumerate() {
                    let px = world_rect.x + cx.resolve(world_rect.width);
                    let py = world_rect.y + cy.resolve(world_rect.height);
                    if idx == 0 {
                        segs.push(PathSegment::MoveTo(px, py));
                    } else {
                        segs.push(PathSegment::LineTo(px, py));
                    }
                }
                if !coords.is_empty() {
                    segs.push(PathSegment::Close);
                }
            });
            fill_rule
        }
        ClipShape::Path { fill_rule, cmds } => {
            let (ox, oy) = (world_rect.x, world_rect.y);
            cmds.with_cmds(|list| {
                for &c in list {
                    match c {
                        ClipPathCmd::MoveTo(x, y) => segs.push(PathSegment::MoveTo(ox + x, oy + y)),
                        ClipPathCmd::LineTo(x, y) => segs.push(PathSegment::LineTo(ox + x, oy + y)),
                        ClipPathCmd::QuadTo(cx, cy, x, y) => segs.push(PathSegment::QuadTo {
                            cx: ox + cx,
                            cy: oy + cy,
                            x: ox + x,
                            y: oy + y,
                        }),
                        ClipPathCmd::CubicTo(c1x, c1y, c2x, c2y, x, y) => {
                            segs.push(PathSegment::CubicTo {
                                c1x: ox + c1x,
                                c1y: oy + c1y,
                                c2x: ox + c2x,
                                c2y: oy + c2y,
                                x: ox + x,
                                y: oy + y,
                            });
                        }
                        ClipPathCmd::Close => segs.push(PathSegment::Close),
                    }
                }
            });
            fill_rule
        }
        other => {
            push_inset_or_ellipse(other, world_rect, &mut segs);
            ClipFillRule::NonZero
        }
    };
    ResolvedClipPath {
        fill_rule,
        segments: Arc::from(segs),
        transform,
    }
}

fn push_quad_subpath(poly: &mut Vec<(f32, f32)>, p0: (f32, f32), cx: f32, cy: f32, x: f32, y: f32) {
    for s in 1..=8 {
        let t = s as f32 / 8.0;
        let u = 1.0 - t;
        poly.push((
            u * u * p0.0 + 2.0 * u * t * cx + t * t * x,
            u * u * p0.1 + 2.0 * u * t * cy + t * t * y,
        ));
    }
}

fn push_cubic_subpath(
    poly: &mut Vec<(f32, f32)>,
    p0: (f32, f32),
    c1: (f32, f32),
    c2: (f32, f32),
    end: (f32, f32),
) {
    for s in 1..=10 {
        let t = s as f32 / 10.0;
        let u = 1.0 - t;
        poly.push((
            u * u * u * p0.0 + 3.0 * u * u * t * c1.0 + 3.0 * u * t * t * c2.0 + t * t * t * end.0,
            u * u * u * p0.1 + 3.0 * u * u * t * c1.1 + 3.0 * u * t * t * c2.1 + t * t * t * end.1,
        ));
    }
}

fn flatten_segments_to_subpaths(segs: &[PathSegment]) -> Vec<Vec<(f32, f32)>> {
    let mut polys: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut cur = (0.0_f32, 0.0_f32);
    for &seg in segs {
        match seg {
            PathSegment::MoveTo(x, y) => {
                cur = (x, y);
                polys.push(vec![cur]);
            }
            PathSegment::LineTo(x, y) => {
                cur = (x, y);
                if let Some(p) = polys.last_mut() {
                    p.push(cur);
                }
            }
            PathSegment::QuadTo { cx, cy, x, y } => {
                if let Some(p) = polys.last_mut() {
                    push_quad_subpath(p, cur, cx, cy, x, y);
                }
                cur = (x, y);
            }
            PathSegment::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                if let Some(p) = polys.last_mut() {
                    push_cubic_subpath(p, cur, (c1x, c1y), (c2x, c2y), (x, y));
                }
                cur = (x, y);
            }
            PathSegment::Arc {
                cx,
                cy,
                rx,
                ry,
                start_rad,
                sweep_rad,
                ..
            } => {
                if let Some(p) = polys.last_mut() {
                    for s in 1..=16 {
                        let a = start_rad + sweep_rad * (s as f32 / 16.0);
                        cur = (cx + rx * a.cos(), cy + ry * a.sin());
                        p.push(cur);
                    }
                }
            }
            PathSegment::Close => {
                if let Some(&first) = polys.last().and_then(|p| p.first()) {
                    if let Some(p) = polys.last_mut() {
                        p.push(first);
                    }
                    cur = first;
                }
            }
        }
    }
    polys
}

/// Returns `true` if screen-space `(sx, sy)` falls inside `clip` respecting `fill_rule`.
pub(crate) fn point_in_resolved_clip_path(sx: f32, sy: f32, clip: &ResolvedClipPath) -> bool {
    let Some(inv) = clip.transform.inverse() else {
        return false;
    };
    let (px, py) = inv.transform_point(sx, sy);
    let polys = flatten_segments_to_subpaths(&clip.segments);
    let mut winding = 0_i32;
    let mut crossings = 0_usize;
    for poly in &polys {
        for win in poly.windows(2) {
            let ((x0, y0), (x1, y1)) = (win[0], win[1]);
            if (y0 <= py && y1 > py) || (y1 <= py && y0 > py) {
                let x_hit = x0 + (py - y0) * (x1 - x0) / (y1 - y0);
                if px < x_hit {
                    crossings += 1;
                }
                if y0 <= py {
                    if (x1 - x0) * (py - y0) - (px - x0) * (y1 - y0) > 0.0 {
                        winding += 1;
                    }
                } else if (x1 - x0) * (py - y0) - (px - x0) * (y1 - y0) < 0.0 {
                    winding -= 1;
                }
            }
        }
    }
    match clip.fill_rule {
        ClipFillRule::NonZero => winding != 0,
        ClipFillRule::EvenOdd => crossings % 2 == 1,
    }
}
