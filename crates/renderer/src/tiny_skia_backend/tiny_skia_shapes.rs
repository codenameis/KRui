//! Circular-arc rounded paths, inset border rings, linear gradients,
//! and `<canvas>` path helpers for `TinySkiaRenderer`.

use crate::radii::effective_radii_4;
use krui_core::{
    CanvasCommand, CanvasLineCap, CanvasLineJoin, DrawPrimitive, ResolvedConicGradient,
    ResolvedRadialGradient,
};
use krui_layout::LayoutRect;
use krui_style::{Affine2, BoxShadow, Color, LinearGradient};

const CIRCLE_KAPPA: f32 = 0.552_284_8;

pub(super) struct StyledBoxSkiaArgs {
    pub rect: LayoutRect,
    pub clip: Option<LayoutRect>,
    pub clip_radius: f32,
    pub clip_radii: [f32; 4],
    pub scaled_transform: Affine2,
    pub scale: f32,
    pub opacity: f32,
    pub background_color: Color,
    pub background_gradient: Option<LinearGradient>,
    pub radial_gradient: Option<ResolvedRadialGradient>,
    pub conic_gradient: Option<ResolvedConicGradient>,
    pub border_color: Color,
    pub border_width: f32,
    pub corner_radii: [f32; 4],
    pub box_shadow: Option<BoxShadow>,
    pub backdrop_blur_px: f32,
}

impl StyledBoxSkiaArgs {
    pub fn from_primitive(prim: &DrawPrimitive, scale: f32, scale_tx: Affine2) -> Option<Self> {
        match prim {
            DrawPrimitive::StyledBox {
                rect,
                clip,
                clip_radius,
                clip_radii,
                transform,
                opacity,
                background_color,
                background_gradient,
                radial_gradient,
                conic_gradient,
                border_color,
                border_width,
                border_radius,
                corner_radii,
                box_shadow,
                backdrop_blur_px,
            } => Some(Self {
                rect: *rect,
                clip: *clip,
                clip_radius: *clip_radius,
                clip_radii: *clip_radii,
                scaled_transform: scale_tx * *transform,
                scale,
                opacity: *opacity,
                background_color: *background_color,
                background_gradient: *background_gradient,
                radial_gradient: *radial_gradient,
                conic_gradient: *conic_gradient,
                border_color: *border_color,
                border_width: *border_width,
                corner_radii: effective_radii_4(*border_radius, *corner_radii),
                box_shadow: *box_shadow,
                backdrop_blur_px: *backdrop_blur_px,
            }),
            _ => None,
        }
    }
}

pub(super) fn to_skia_transform(m: Affine2) -> tiny_skia::Transform {
    tiny_skia::Transform::from_row(m.a, m.b, m.c, m.d, m.tx, m.ty)
}

pub(super) fn to_skia_color(c: Color, opacity: f32) -> tiny_skia::Color {
    let a = ((c.a as f32 / 255.0) * opacity.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    tiny_skia::Color::from_rgba(
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        a,
    )
    .unwrap_or(tiny_skia::Color::BLACK)
}

pub(super) fn fill_gradient_skia(
    pixmap: &mut tiny_skia::Pixmap,
    path: &tiny_skia::Path,
    rect: LayoutRect,
    grad: LinearGradient,
    opacity: f32,
    skia_tx: tiny_skia::Transform,
    mask: Option<&tiny_skia::Mask>,
) {
    let (sin_a, cos_a) = grad.angle_deg.to_radians().sin_cos();
    let (dx, dy) = (sin_a, -cos_a);
    let (hw, hh) = (0.5 * rect.width, 0.5 * rect.height);
    let half_proj = (dx.abs() * hw + dy.abs() * hh).max(1e-3);
    let (cx, cy) = (rect.x + hw, rect.y + hh);
    let p0 = tiny_skia::Point::from_xy(cx - dx * half_proj, cy - dy * half_proj);
    let mut p1 = tiny_skia::Point::from_xy(cx + dx * half_proj, cy + dy * half_proj);
    let mut stops = Vec::new();
    if !grad.extra_stops.is_empty() {
        grad.extra_stops.with_stops(|raw| {
            let max_p = if grad.repeating {
                raw.last().map_or(1.0, |s| s.position).clamp(1e-4, 1.0)
            } else {
                1.0
            };
            p1 = tiny_skia::Point::from_xy(
                p0.x + (p1.x - p0.x) * max_p,
                p0.y + (p1.y - p0.y) * max_p,
            );
            for s in raw {
                stops.push(tiny_skia::GradientStop::new(
                    (s.position / max_p).clamp(0.0, 1.0),
                    to_skia_color(s.color, opacity),
                ));
            }
        });
    } else {
        let [p0_s, p1_s, p2_s] = grad.stop_positions;
        stops.push(tiny_skia::GradientStop::new(
            p0_s.clamp(0.0, 1.0),
            to_skia_color(grad.start_color, opacity),
        ));
        if let Some(col) = grad.mid_color {
            stops.push(tiny_skia::GradientStop::new(
                p1_s.clamp(0.0, 1.0),
                to_skia_color(col, opacity),
            ));
        }
        stops.push(tiny_skia::GradientStop::new(
            p2_s.clamp(0.0, 1.0),
            to_skia_color(grad.end_color, opacity),
        ));
    }
    let spread = if grad.repeating {
        tiny_skia::SpreadMode::Repeat
    } else {
        tiny_skia::SpreadMode::Pad
    };
    if let Some(shader) =
        tiny_skia::LinearGradient::new(p0, p1, stops, spread, tiny_skia::Transform::identity())
    {
        let paint = tiny_skia::Paint {
            shader,
            anti_alias: true,
            ..Default::default()
        };
        pixmap.fill_path(path, &paint, tiny_skia::FillRule::Winding, skia_tx, mask);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn fill_inset_border_skia(
    pixmap: &mut tiny_skia::Pixmap,
    rect: LayoutRect,
    radii: [f32; 4],
    bw: f32,
    color: Color,
    opacity: f32,
    skia_tx: tiny_skia::Transform,
    mask: Option<&tiny_skia::Mask>,
) {
    let mut pb = tiny_skia::PathBuilder::new();
    append_round_rect_4_subpath(&mut pb, rect.x, rect.y, rect.width, rect.height, radii);
    let rule = if 2.0 * bw < rect.width.min(rect.height) {
        append_round_rect_4_subpath(
            &mut pb,
            rect.x + bw,
            rect.y + bw,
            rect.width - 2.0 * bw,
            rect.height - 2.0 * bw,
            radii.map(|r| (r - bw).max(0.0)),
        );
        tiny_skia::FillRule::EvenOdd
    } else {
        tiny_skia::FillRule::Winding
    };
    if let Some(ring_path) = pb.finish() {
        let mut paint = tiny_skia::Paint {
            anti_alias: true,
            ..Default::default()
        };
        paint.set_color(to_skia_color(color, opacity));
        pixmap.fill_path(&ring_path, &paint, rule, skia_tx, mask);
    }
}

pub(super) fn append_round_rect_4_subpath(
    pb: &mut tiny_skia::PathBuilder,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radii: [f32; 4],
) {
    let [tl, tr, br, bl] = crate::radii::normalize_radii_4(w, h, radii);
    if tl <= 0.05 && tr <= 0.05 && br <= 0.05 && bl <= 0.05 {
        pb.move_to(x, y);
        pb.line_to(x + w, y);
        pb.line_to(x + w, y + h);
        pb.line_to(x, y + h);
        pb.close();
        return;
    }
    pb.move_to(x + tl, y);
    pb.line_to(x + w - tr, y);
    if tr > 0.05 {
        let k = tr * CIRCLE_KAPPA;
        pb.cubic_to(x + w - tr + k, y, x + w, y + tr - k, x + w, y + tr);
    }
    pb.line_to(x + w, y + h - br);
    if br > 0.05 {
        let k = br * CIRCLE_KAPPA;
        pb.cubic_to(
            x + w,
            y + h - br + k,
            x + w - br + k,
            y + h,
            x + w - br,
            y + h,
        );
    }
    pb.line_to(x + bl, y + h);
    if bl > 0.05 {
        let k = bl * CIRCLE_KAPPA;
        pb.cubic_to(x + bl - k, y + h, x, y + h - bl + k, x, y + h - bl);
    }
    pb.line_to(x, y + tl);
    if tl > 0.05 {
        let k = tl * CIRCLE_KAPPA;
        pb.cubic_to(x, y + tl - k, x + tl - k, y, x + tl, y);
    }
    pb.close();
}

pub(super) fn build_skia_round_rect_4(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radii: [f32; 4],
) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let mut pb = tiny_skia::PathBuilder::new();
    append_round_rect_4_subpath(&mut pb, x, y, w, h, radii);
    pb.finish()
}

pub(super) fn build_skia_round_rect(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
) -> Option<tiny_skia::Path> {
    build_skia_round_rect_4(x, y, w, h, [radius; 4])
}

pub(super) fn paint_styled_box_surfaces_skia(
    pixmap: &mut tiny_skia::Pixmap,
    mask: Option<&tiny_skia::Mask>,
    a: &StyledBoxSkiaArgs,
) {
    let Some(outer_path) = build_skia_round_rect_4(
        a.rect.x,
        a.rect.y,
        a.rect.width,
        a.rect.height,
        a.corner_radii,
    ) else {
        return;
    };
    let skia_tx = to_skia_transform(a.scaled_transform);
    if a.background_color.a > 0 {
        fill_skia_path(
            pixmap,
            &outer_path,
            a.background_color,
            a.opacity,
            skia_tx,
            mask,
        );
    }
    if let Some(grad) = a.background_gradient {
        fill_gradient_skia(pixmap, &outer_path, a.rect, grad, a.opacity, skia_tx, mask);
    }
    if let Some(rg) = a.radial_gradient {
        super::tiny_skia_radial::fill_radial_gradient_skia(
            pixmap,
            &outer_path,
            a.rect,
            rg,
            a.opacity,
            skia_tx,
            mask,
        );
    }
    if let Some(cg) = a.conic_gradient {
        super::tiny_skia_conic::fill_conic_gradient_skia(
            pixmap,
            &outer_path,
            a.rect,
            cg,
            a.opacity,
            skia_tx,
            mask,
        );
    }
    if a.border_width > 0.0 && a.border_color.a > 0 {
        fill_inset_border_skia(
            pixmap,
            a.rect,
            a.corner_radii,
            a.border_width,
            a.border_color,
            a.opacity,
            skia_tx,
            mask,
        );
    }
}

pub(super) fn fill_skia_path(
    pixmap: &mut tiny_skia::Pixmap,
    path: &tiny_skia::Path,
    color: Color,
    opacity: f32,
    skia_tx: tiny_skia::Transform,
    mask: Option<&tiny_skia::Mask>,
) {
    fill_skia_path_with_rule(
        pixmap,
        path,
        color,
        opacity,
        tiny_skia::FillRule::Winding,
        skia_tx,
        mask,
    );
}

pub(super) fn fill_skia_path_with_rule(
    pixmap: &mut tiny_skia::Pixmap,
    path: &tiny_skia::Path,
    color: Color,
    opacity: f32,
    rule: tiny_skia::FillRule,
    skia_tx: tiny_skia::Transform,
    mask: Option<&tiny_skia::Mask>,
) {
    let mut paint = tiny_skia::Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color(to_skia_color(color, opacity));
    pixmap.fill_path(path, &paint, rule, skia_tx, mask);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn stroke_skia_path_styled(
    pixmap: &mut tiny_skia::Pixmap,
    path: &tiny_skia::Path,
    color: Color,
    opacity: f32,
    line_width: f32,
    cap: CanvasLineCap,
    join: CanvasLineJoin,
    skia_tx: tiny_skia::Transform,
    mask: Option<&tiny_skia::Mask>,
) {
    let mut paint = tiny_skia::Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color(to_skia_color(color, opacity));
    let stroke = tiny_skia::Stroke {
        width: line_width,
        line_cap: match cap {
            CanvasLineCap::Butt => tiny_skia::LineCap::Butt,
            CanvasLineCap::Round => tiny_skia::LineCap::Round,
            CanvasLineCap::Square => tiny_skia::LineCap::Square,
        },
        line_join: match join {
            CanvasLineJoin::Miter => tiny_skia::LineJoin::Miter,
            CanvasLineJoin::Round => tiny_skia::LineJoin::Round,
            CanvasLineJoin::Bevel => tiny_skia::LineJoin::Bevel,
        },
        ..tiny_skia::Stroke::default()
    };
    pixmap.stroke_path(path, &paint, &stroke, skia_tx, mask);
}

pub(super) fn render_canvas_cmd_skia(
    pixmap: &mut tiny_skia::Pixmap,
    cmd: &CanvasCommand,
    skia_tx: tiny_skia::Transform,
    opacity: f32,
    mask: Option<&tiny_skia::Mask>,
) {
    match *cmd {
        CanvasCommand::FillRect {
            x,
            y,
            w,
            h,
            radius,
            color,
        } if w > 0.0 && h > 0.0 => {
            if let Some(p) = build_skia_round_rect(x, y, w, h, radius) {
                fill_skia_path(pixmap, &p, color, opacity, skia_tx, mask);
            }
        }
        CanvasCommand::FillCircle {
            cx,
            cy,
            radius,
            color,
        } if radius > 0.0 => {
            if let Some(p) = tiny_skia::PathBuilder::from_circle(cx, cy, radius) {
                fill_skia_path(pixmap, &p, color, opacity, skia_tx, mask);
            }
        }
        _ => {
            super::tiny_skia_images::render_canvas_stroke_or_path_skia(
                pixmap, cmd, skia_tx, opacity, mask,
            );
        }
    }
}
