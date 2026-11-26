//! Offscreen CSS `filter` pipeline (`blur` + W3C color matrix) and layer stack ops for `TinySkiaRenderer`.

use super::tiny_skia_blur::{blur_sub_pixmap_multiscale, sample_blurred_bilinear};
use super::tiny_skia_images::composite_layer_skia;
use super::TinySkiaRenderer;
use crate::MAX_LAYER_POOL_DEPTH;
use krui_core::{DrawPrimitive, ResolvedClipPath};
use krui_layout::LayoutRect;
use krui_style::{Affine2, CssFilterEffect, Mat4};

#[allow(clippy::large_enum_variant)]
pub(super) enum SkiaLayerEntry {
    Offscreen(
        tiny_skia::Pixmap,
        Option<LayoutRect>,
        f32,
        [f32; 4],
        Option<(LayoutRect, [f32; 4], Affine2)>,
        Option<ResolvedClipPath>,
        Option<(LayoutRect, Mat4)>,
        f32,
        CssFilterEffect,
    ),
    Overflow(f32),
}

pub(super) fn step_layer_op_skia(
    renderer: &mut TinySkiaRenderer,
    prim: &DrawPrimitive,
    layer_stack: &mut Vec<SkiaLayerEntry>,
    active_offscreens: &mut usize,
    inline_opacity: &mut f32,
    (pw, ph): (u32, u32),
    scale: f32,
) -> bool {
    match prim {
        DrawPrimitive::PushLayer { .. } => {
            push_layer_skia(
                renderer,
                prim,
                layer_stack,
                active_offscreens,
                inline_opacity,
                (pw, ph),
            );
            true
        }
        DrawPrimitive::PopLayer => {
            pop_layer_skia(
                renderer,
                layer_stack,
                active_offscreens,
                inline_opacity,
                scale,
            );
            true
        }
        _ => false,
    }
}

fn push_layer_skia(
    renderer: &mut TinySkiaRenderer,
    prim: &DrawPrimitive,
    layer_stack: &mut Vec<SkiaLayerEntry>,
    active_offscreens: &mut usize,
    inline_opacity: &mut f32,
    (pw, ph): (u32, u32),
) {
    let DrawPrimitive::PushLayer {
        clip,
        clip_radius,
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
    let entry = if *active_offscreens < MAX_LAYER_POOL_DEPTH {
        if let Some(fresh) = renderer.acquire_layer_pixmap(pw, ph) {
            *active_offscreens += 1;
            let prev = std::mem::replace(&mut renderer.pixmap, fresh);
            SkiaLayerEntry::Offscreen(
                prev,
                *clip,
                *clip_radius,
                *clip_radii,
                *local_clip,
                clip_path.clone(),
                *perspective,
                *opacity,
                *filter,
            )
        } else {
            let prev_op = *inline_opacity;
            *inline_opacity = (prev_op * opacity.clamp(0.0, 1.0)).clamp(0.0, 1.0);
            SkiaLayerEntry::Overflow(prev_op)
        }
    } else {
        let prev_op = *inline_opacity;
        *inline_opacity = (prev_op * opacity.clamp(0.0, 1.0)).clamp(0.0, 1.0);
        SkiaLayerEntry::Overflow(prev_op)
    };
    layer_stack.push(entry);
}

fn pop_layer_skia(
    renderer: &mut TinySkiaRenderer,
    layer_stack: &mut Vec<SkiaLayerEntry>,
    active_offscreens: &mut usize,
    inline_opacity: &mut f32,
    scale: f32,
) {
    let Some(popped) = layer_stack.pop() else {
        return;
    };
    match popped {
        SkiaLayerEntry::Offscreen(
            mut parent_px,
            clip,
            clip_radius,
            clip_radii,
            local_clip,
            clip_path,
            perspective,
            opacity,
            filter,
        ) => {
            *active_offscreens = active_offscreens.saturating_sub(1);
            apply_layer_filter_skia(&mut renderer.pixmap, filter, scale);
            composite_layer_skia(
                &mut parent_px,
                &renderer.pixmap,
                &mut renderer.mask_cache,
                (clip, clip_radius, clip_radii, local_clip, clip_path),
                perspective,
                opacity,
                scale,
            );
            let child_px = std::mem::replace(&mut renderer.pixmap, parent_px);
            renderer.layer_pool.push(child_px);
        }
        SkiaLayerEntry::Overflow(prev_op) => *inline_opacity = prev_op,
    }
}

/// Applies `filter` in place onto an offscreen premultiplied RGBA8 layer `pixmap`.
pub(super) fn apply_layer_filter_skia(
    pixmap: &mut tiny_skia::Pixmap,
    filter: CssFilterEffect,
    scale: f32,
) {
    if filter.is_identity() {
        return;
    }
    if filter.blur_px > 0.5 {
        let (pw, ph) = (pixmap.width() as usize, pixmap.height() as usize);
        let radius = ((filter.blur_px * scale).round() as usize).clamp(1, 48);
        let ds = if radius >= 4 && pw >= 16 && ph >= 16 {
            4
        } else {
            1
        };
        let (bw, bh, blurred) =
            blur_sub_pixmap_multiscale(pixmap.data(), pw, (0, 0), (pw, ph), radius, ds);
        let dst = pixmap.data_mut();
        for py in 0..ph {
            for px in 0..pw {
                let off = (py * pw + px) * 4;
                let sample = sample_blurred_bilinear(&blurred, bw, bh, ds, px, py);
                dst[off..off + 4].copy_from_slice(&sample);
            }
        }
    }
    if filter.has_color_matrix() {
        filter.apply_premultiplied_rgba8_slice(pixmap.data_mut());
    }
}
