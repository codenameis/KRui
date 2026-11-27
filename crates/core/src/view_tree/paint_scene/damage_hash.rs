//! Deterministic primitive signature hashing for [`super::damage_tracker::DamageTracker`].

use crate::scene::{CanvasCommand, DrawPrimitive, ResolvedConicGradient, ResolvedRadialGradient};
use krui_layout::LayoutRect;
use krui_style::Affine2;
use std::hash::{Hash, Hasher};

fn hash_rect(h: &mut impl Hasher, r: LayoutRect) {
    [
        r.x.to_bits(),
        r.y.to_bits(),
        r.width.to_bits(),
        r.height.to_bits(),
    ]
    .hash(h);
}

fn hash_affine(h: &mut impl Hasher, t: Affine2) {
    [t.a, t.b, t.c, t.d, t.tx, t.ty].map(f32::to_bits).hash(h);
}

fn hash_image_source(h: &mut impl Hasher, source: &crate::media::ImageSource) {
    match source {
        crate::media::ImageSource::Decoded(img) => {
            1_u8.hash(h);
            img.id.0.hash(h);
        }
        crate::media::ImageSource::External { id, width, height } => {
            2_u8.hash(h);
            id.0.hash(h);
            width.hash(h);
            height.hash(h);
        }
    }
}

fn hash_box_fills(
    h: &mut impl Hasher,
    bg: krui_style::Color,
    lg: &Option<krui_style::LinearGradient>,
    rg: &Option<ResolvedRadialGradient>,
    cg: &Option<ResolvedConicGradient>,
    shadow: &Option<krui_style::BoxShadow>,
) {
    [bg.r, bg.g, bg.b, bg.a].hash(h);
    if let Some(g) = lg {
        1_u8.hash(h);
        g.angle_deg.to_bits().hash(h);
        [
            g.start_color.r,
            g.start_color.g,
            g.start_color.b,
            g.start_color.a,
        ]
        .hash(h);
        [g.end_color.r, g.end_color.g, g.end_color.b, g.end_color.a].hash(h);
    }
    if let Some(r) = rg {
        2_u8.hash(h);
        [
            r.center_dx,
            r.center_dy,
            r.radius_x,
            r.radius_y,
            r.inner_stop,
            r.outer_stop,
        ]
        .map(f32::to_bits)
        .hash(h);
        [
            r.inner_color.r,
            r.inner_color.g,
            r.inner_color.b,
            r.inner_color.a,
        ]
        .hash(h);
        [
            r.outer_color.r,
            r.outer_color.g,
            r.outer_color.b,
            r.outer_color.a,
        ]
        .hash(h);
        r.extra_stops.cache_key().hash(h);
        r.repeating.hash(h);
    }
    if let Some(c) = cg {
        3_u8.hash(h);
        [c.center_dx, c.center_dy, c.from_angle_deg]
            .map(f32::to_bits)
            .hash(h);
        c.stops.cache_key().hash(h);
        c.repeating.hash(h);
    }
    if let Some(s) = shadow {
        4_u8.hash(h);
        [s.color.r, s.color.g, s.color.b, s.color.a].hash(h);
        [s.offset_x, s.offset_y, s.blur_radius, s.spread_radius]
            .map(f32::to_bits)
            .hash(h);
        s.inset.hash(h);
    }
}

fn hash_styled_box(h: &mut impl Hasher, prim: &DrawPrimitive) {
    let DrawPrimitive::StyledBox {
        rect,
        clip,
        clip_radii,
        opacity,
        background_color,
        background_gradient,
        radial_gradient,
        conic_gradient,
        border_color,
        border_width,
        border_radius,
        corner_radii,
        transform,
        box_shadow,
        backdrop_blur_px,
        ..
    } = prim
    else {
        return;
    };
    1_u8.hash(h);
    hash_rect(h, *rect);
    if let Some(c) = clip {
        1_u8.hash(h);
        hash_rect(h, *c);
        clip_radii.map(f32::to_bits).hash(h);
    }
    opacity.to_bits().hash(h);
    hash_box_fills(
        h,
        *background_color,
        background_gradient,
        radial_gradient,
        conic_gradient,
        box_shadow,
    );
    [
        border_color.r,
        border_color.g,
        border_color.b,
        border_color.a,
    ]
    .hash(h);
    [
        border_width.to_bits(),
        border_radius.to_bits(),
        backdrop_blur_px.to_bits(),
    ]
    .hash(h);
    corner_radii.map(f32::to_bits).hash(h);
    hash_affine(h, *transform);
}

fn hash_push_layer(h: &mut impl Hasher, prim: &DrawPrimitive) {
    let DrawPrimitive::PushLayer {
        bounds,
        clip,
        clip_radii,
        local_clip,
        clip_path,
        perspective,
        opacity,
        filter,
        ..
    } = prim
    else {
        return;
    };
    5_u8.hash(h);
    hash_rect(h, *bounds);
    if let Some(c) = clip {
        1_u8.hash(h);
        hash_rect(h, *c);
        clip_radii.map(f32::to_bits).hash(h);
    }
    if let Some((lc, lr, ltx)) = local_clip {
        2_u8.hash(h);
        hash_rect(h, *lc);
        lr.map(f32::to_bits).hash(h);
        hash_affine(h, *ltx);
    }
    if let Some(cp) = clip_path {
        3_u8.hash(h);
        cp.segments.len().hash(h);
        hash_affine(h, cp.transform);
    }
    if let Some((pr, mat)) = perspective {
        4_u8.hash(h);
        hash_rect(h, *pr);
        mat.0.map(f32::to_bits).hash(h);
    }
    opacity.to_bits().hash(h);
    [
        filter.invert,
        filter.grayscale,
        filter.brightness,
        filter.contrast,
        filter.saturate,
        filter.sepia,
        filter.opacity,
        filter.blur_px,
    ]
    .map(f32::to_bits)
    .hash(h);
}

fn hash_canvas_geom_cmd(h: &mut impl Hasher, cmd: &CanvasCommand) -> bool {
    match cmd {
        CanvasCommand::FillRect {
            x,
            y,
            w,
            h: rh,
            radius,
            color,
        } => {
            [x, y, w, rh, radius].map(|v| v.to_bits()).hash(h);
            [color.r, color.g, color.b, color.a].hash(h);
        }
        CanvasCommand::StrokeRect {
            x,
            y,
            w,
            h: rh,
            line_width,
            color,
        } => {
            [x, y, w, rh, line_width].map(|v| v.to_bits()).hash(h);
            [color.r, color.g, color.b, color.a].hash(h);
        }
        CanvasCommand::FillCircle {
            cx,
            cy,
            radius,
            color,
        } => {
            [cx, cy, radius].map(|v| v.to_bits()).hash(h);
            [color.r, color.g, color.b, color.a].hash(h);
        }
        CanvasCommand::StrokeLine {
            x1,
            y1,
            x2,
            y2,
            line_width,
            color,
            ..
        } => {
            [x1, y1, x2, y2, line_width].map(|v| v.to_bits()).hash(h);
            [color.r, color.g, color.b, color.a].hash(h);
        }
        _ => return false,
    }
    true
}

fn hash_canvas_cmd(h: &mut impl Hasher, cmd: &CanvasCommand) {
    std::mem::discriminant(cmd).hash(h);
    if hash_canvas_geom_cmd(h, cmd) {
        return;
    }
    match cmd {
        CanvasCommand::FillPath {
            segments, color, ..
        } => {
            segments.len().hash(h);
            [color.r, color.g, color.b, color.a].hash(h);
        }
        CanvasCommand::StrokePath {
            segments,
            line_width,
            color,
            ..
        } => {
            segments.len().hash(h);
            line_width.to_bits().hash(h);
            [color.r, color.g, color.b, color.a].hash(h);
        }
        CanvasCommand::SetTransform(aff) => hash_affine(h, *aff),
        CanvasCommand::PushClip { segments, .. } => segments.len().hash(h),
        CanvasCommand::FillGradient {
            segments, alpha, ..
        } => {
            segments.len().hash(h);
            alpha.to_bits().hash(h);
        }
        CanvasCommand::DrawText {
            text,
            x,
            y,
            font_size,
            font_weight,
            color,
            ..
        } => {
            text.hash(h);
            [x, y, font_size].map(|v| v.to_bits()).hash(h);
            font_weight.hash(h);
            [color.r, color.g, color.b, color.a].hash(h);
        }
        CanvasCommand::DrawImage {
            source,
            dst_rect,
            alpha,
            ..
        } => {
            hash_image_source(h, source);
            hash_rect(h, *dst_rect);
            alpha.to_bits().hash(h);
        }
        _ => {}
    }
}

pub(super) fn hash_primitive(prim: &DrawPrimitive) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match prim {
        DrawPrimitive::StyledBox { .. } => hash_styled_box(&mut h, prim),
        DrawPrimitive::TextGlyphs {
            origin_x,
            origin_y,
            color,
            opacity,
            glyphs,
            ..
        } => {
            2_u8.hash(&mut h);
            [origin_x.to_bits(), origin_y.to_bits(), opacity.to_bits()].hash(&mut h);
            [color.r, color.g, color.b, color.a].hash(&mut h);
            glyphs.len().hash(&mut h);
            for g in glyphs.iter() {
                g.cache_key.hash(&mut h);
                [g.x.to_bits(), g.y.to_bits()].hash(&mut h);
            }
        }
        DrawPrimitive::Image {
            rect,
            source,
            uv_rect,
            opacity,
            transform,
            ..
        } => {
            3_u8.hash(&mut h);
            hash_rect(&mut h, *rect);
            hash_image_source(&mut h, source);
            uv_rect.map(f32::to_bits).hash(&mut h);
            hash_affine(&mut h, *transform);
            opacity.to_bits().hash(&mut h);
        }
        DrawPrimitive::CanvasBatch {
            commands,
            opacity,
            transform,
            ..
        } => {
            4_u8.hash(&mut h);
            hash_affine(&mut h, *transform);
            opacity.to_bits().hash(&mut h);
            commands.len().hash(&mut h);
            for cmd in commands.iter() {
                hash_canvas_cmd(&mut h, cmd);
            }
        }
        DrawPrimitive::PushLayer { .. } => hash_push_layer(&mut h, prim),
        DrawPrimitive::PopLayer => 6_u8.hash(&mut h),
    }
    h.finish()
}
