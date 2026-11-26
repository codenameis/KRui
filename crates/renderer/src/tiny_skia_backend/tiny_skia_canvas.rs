//! Stateful `<canvas>` command execution (`SetTransform`, `PushClip`, `PopClip`,
//! `FillGradient`) for `TinySkiaRenderer` and offscreen clipped-canvas rasterization.

use super::tiny_skia_clip_cache::ClipMaskCache;
use super::tiny_skia_images::build_skia_path;
use super::tiny_skia_shapes::{render_canvas_cmd_skia, to_skia_color, to_skia_transform};
use krui_core::{CanvasCommand, CanvasFillRule, CanvasGradient, CanvasGradientStop};
use krui_layout::LayoutRect;
use krui_style::Affine2;

/// Renders a `CanvasBatch` command stream onto `pixmap`, tracking local transforms and nested `PushClip`/`PopClip`.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_canvas_batch_skia(
    pixmap: &mut tiny_skia::Pixmap,
    mask_cache: &mut ClipMaskCache,
    viewport: LayoutRect,
    (clip, clip_radius, clip_radii): (Option<LayoutRect>, f32, [f32; 4]),
    (transform, scale, scale_tx): (Affine2, f32, Affine2),
    opacity: f32,
    commands: &[CanvasCommand],
) -> (usize, usize) {
    if opacity <= 0.0 {
        return (0, 0);
    }
    let base_tx = scale_tx * (transform * Affine2::translation(viewport.x, viewport.y));
    let base_mask = mask_cache
        .get_or_build_4(
            pixmap.width(),
            pixmap.height(),
            clip,
            clip_radius,
            clip_radii,
            scale,
        )
        .cloned();
    render_canvas_with_base_mask(pixmap, base_tx, opacity, base_mask, commands);
    (commands.len() * 8, commands.len() * 12)
}

/// Rasterizes a local `<canvas>` vector/clip batch to a straight-RGBA8 `DecodedImage` for Wgpu `PushClip` parity.
#[cfg_attr(not(feature = "gpu"), allow(dead_code))]
pub(crate) fn rasterize_canvas_batch_to_decoded(
    viewport: LayoutRect,
    surface_scale: f32,
    commands: &[CanvasCommand],
) -> Option<krui_core::DecodedImage> {
    let scale = surface_scale.clamp(1.0, 4.0);
    let pw = ((viewport.width * scale).ceil() as u32).clamp(1, 4096);
    let ph = ((viewport.height * scale).ceil() as u32).clamp(1, 4096);
    let mut pixmap = tiny_skia::Pixmap::new(pw, ph)?;
    let base_tx = Affine2::scale(scale, scale);
    render_canvas_with_base_mask(&mut pixmap, base_tx, 1.0, None, commands);
    let straight = unpremultiply_rgba8(pixmap.data());
    krui_core::DecodedImage::from_rgba8(pw, ph, straight).ok()
}

fn render_canvas_with_base_mask(
    pixmap: &mut tiny_skia::Pixmap,
    base_tx: Affine2,
    opacity: f32,
    base_mask: Option<tiny_skia::Mask>,
    commands: &[CanvasCommand],
) {
    let mut local_tx = Affine2::IDENTITY;
    let mut clip_stack: Vec<Option<tiny_skia::Mask>> = vec![base_mask];
    for cmd in commands {
        let eff_skia_tx = to_skia_transform(base_tx * local_tx);
        let active_mask = clip_stack.last().and_then(|m| m.as_ref());
        match cmd {
            CanvasCommand::SetTransform(m) => local_tx = *m,
            CanvasCommand::PushClip {
                segments,
                fill_rule,
            } => {
                let next_mask = intersect_skia_path_mask(
                    (pixmap.width(), pixmap.height()),
                    active_mask,
                    segments,
                    *fill_rule,
                    eff_skia_tx,
                );
                clip_stack.push(next_mask);
            }
            CanvasCommand::PopClip => {
                if clip_stack.len() > 1 {
                    clip_stack.pop();
                }
            }
            CanvasCommand::FillGradient {
                segments,
                fill_rule,
                gradient,
                alpha,
            } => {
                fill_canvas_gradient_skia(
                    pixmap,
                    segments,
                    *fill_rule,
                    gradient,
                    opacity * *alpha,
                    eff_skia_tx,
                    active_mask,
                );
            }
            CanvasCommand::DrawText { .. } | CanvasCommand::DrawImage { .. } => {}
            _ => render_canvas_cmd_skia(pixmap, cmd, eff_skia_tx, opacity, active_mask),
        }
    }
}

fn intersect_skia_path_mask(
    (w, h): (u32, u32),
    prev_mask: Option<&tiny_skia::Mask>,
    segments: &[krui_core::PathSegment],
    fill_rule: CanvasFillRule,
    skia_tx: tiny_skia::Transform,
) -> Option<tiny_skia::Mask> {
    let mut next = match prev_mask {
        Some(m) => m.clone(),
        None => {
            let mut m = tiny_skia::Mask::new(w, h)?;
            m.clear();
            if let Some(path) = build_skia_path(segments) {
                m.fill_path(&path, to_skia_rule(fill_rule), true, skia_tx);
            }
            return Some(m);
        }
    };
    match build_skia_path(segments) {
        Some(path) => next.intersect_path(&path, to_skia_rule(fill_rule), true, skia_tx),
        None => next.clear(),
    }
    Some(next)
}

fn fill_canvas_gradient_skia(
    pixmap: &mut tiny_skia::Pixmap,
    segments: &[krui_core::PathSegment],
    fill_rule: CanvasFillRule,
    gradient: &CanvasGradient,
    opacity: f32,
    skia_tx: tiny_skia::Transform,
    mask: Option<&tiny_skia::Mask>,
) {
    if opacity <= 0.0 || gradient.stops().is_empty() {
        return;
    }
    let Some(path) = build_skia_path(segments) else {
        return;
    };
    let Some(shader) = build_skia_gradient_shader(gradient, opacity) else {
        return;
    };
    let paint = tiny_skia::Paint {
        shader,
        anti_alias: true,
        ..Default::default()
    };
    pixmap.fill_path(&path, &paint, to_skia_rule(fill_rule), skia_tx, mask);
}

fn build_skia_gradient_shader(
    gradient: &CanvasGradient,
    opacity: f32,
) -> Option<tiny_skia::Shader<'static>> {
    match gradient {
        CanvasGradient::Linear {
            x0,
            y0,
            x1,
            y1,
            stops,
        } => {
            let skia_stops = convert_stops(stops, opacity, 0.0, 1.0);
            tiny_skia::LinearGradient::new(
                tiny_skia::Point::from_xy(*x0, *y0),
                tiny_skia::Point::from_xy(*x1, *y1),
                skia_stops,
                tiny_skia::SpreadMode::Pad,
                tiny_skia::Transform::identity(),
            )
        }
        CanvasGradient::Radial {
            x0,
            y0,
            r0,
            x1,
            y1,
            r1,
            stops,
        } => {
            let outer_r = r1.max(*r0).max(1e-3);
            let inner_t = (r0 / outer_r).clamp(0.0, 1.0);
            let skia_stops = convert_stops(stops, opacity, inner_t, 1.0);
            tiny_skia::RadialGradient::new(
                tiny_skia::Point::from_xy(*x0, *y0),
                tiny_skia::Point::from_xy(*x1, *y1),
                outer_r,
                skia_stops,
                tiny_skia::SpreadMode::Pad,
                tiny_skia::Transform::identity(),
            )
        }
    }
}

fn convert_stops(
    stops: &[CanvasGradientStop],
    opacity: f32,
    range_start: f32,
    range_end: f32,
) -> Vec<tiny_skia::GradientStop> {
    let span = (range_end - range_start).max(1e-5);
    let mut out = Vec::with_capacity(stops.len() + 1);
    if range_start > 0.0 {
        if let Some(first) = stops.first() {
            out.push(tiny_skia::GradientStop::new(
                0.0,
                to_skia_color(first.color, opacity),
            ));
        }
    }
    for s in stops {
        let pos = (range_start + s.offset.clamp(0.0, 1.0) * span).clamp(0.0, 1.0);
        out.push(tiny_skia::GradientStop::new(
            pos,
            to_skia_color(s.color, opacity),
        ));
    }
    out
}

fn to_skia_rule(rule: CanvasFillRule) -> tiny_skia::FillRule {
    match rule {
        CanvasFillRule::NonZero => tiny_skia::FillRule::Winding,
        CanvasFillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
    }
}

#[cfg_attr(not(feature = "gpu"), allow(dead_code))]
fn unpremultiply_rgba8(pm: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pm.len());
    for px in pm.as_chunks::<4>().0 {
        let a = px[3];
        if a == 0 || a == 255 {
            out.extend_from_slice(px);
        } else {
            let af = 255.0 / a as f32;
            out.push((px[0] as f32 * af).round().clamp(0.0, 255.0) as u8);
            out.push((px[1] as f32 * af).round().clamp(0.0, 255.0) as u8);
            out.push((px[2] as f32 * af).round().clamp(0.0, 255.0) as u8);
            out.push(a);
        }
    }
    out
}
