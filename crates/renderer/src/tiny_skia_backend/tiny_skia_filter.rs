//! Offscreen CSS `filter` pipeline (`blur` + W3C color matrix), sub-bounds layer
//! compositing, and dirty-region pooling for `TinySkiaRenderer`.

use super::tiny_skia_blur::{blur_sub_pixmap_multiscale, sample_blurred_bilinear};
use super::tiny_skia_clip_cache::ClipMaskCache;
use super::tiny_skia_images::composite_layer_skia;
use super::tiny_skia_layer_bounds::{
    clip_rect_phys, estimate_prim_phys_bounds, intersect_bounds, union_bounds,
};
use super::TinySkiaRenderer;
use crate::MAX_LAYER_POOL_DEPTH;
use krui_core::{DrawPrimitive, ResolvedClipPath};
use krui_layout::LayoutRect;
use krui_style::{Affine2, CssFilterEffect, Mat4};

pub(super) struct SkiaOffscreenLayer {
    parent_px: tiny_skia::Pixmap,
    clip: Option<LayoutRect>,
    clip_radius: f32,
    clip_radii: [f32; 4],
    local_clip: Option<(LayoutRect, [f32; 4], Affine2)>,
    clip_path: Option<ResolvedClipPath>,
    perspective: Option<(LayoutRect, Mat4)>,
    opacity: f32,
    filter: CssFilterEffect,
    drawn_bounds: Option<[u32; 4]>,
}

pub(super) enum SkiaLayerEntry {
    Offscreen(Box<SkiaOffscreenLayer>),
    Overflow(f32),
}

/// Unions the physical bounding box of `prim` into the active offscreen layer's dirty region.
pub(super) fn record_offscreen_prim_bounds(
    layer_stack: &mut [SkiaLayerEntry],
    prim: &DrawPrimitive,
    scale: f32,
    vp: (u32, u32),
) {
    let Some(SkiaLayerEntry::Offscreen(layer)) = layer_stack.last_mut() else {
        return;
    };
    let prim_bounds = estimate_prim_phys_bounds(prim, scale, vp);
    layer.drawn_bounds = union_bounds(layer.drawn_bounds, prim_bounds);
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
            let parent_px = std::mem::replace(&mut renderer.pixmap, fresh);
            SkiaLayerEntry::Offscreen(Box::new(SkiaOffscreenLayer {
                parent_px,
                clip: *clip,
                clip_radius: *clip_radius,
                clip_radii: *clip_radii,
                local_clip: *local_clip,
                clip_path: clip_path.clone(),
                perspective: *perspective,
                opacity: *opacity,
                filter: *filter,
                drawn_bounds: None,
            }))
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

fn composite_subrect_fast(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    bounds: [u32; 4],
    mask: Option<&tiny_skia::Mask>,
    opacity: f32,
) {
    let op_256 = (opacity.clamp(0.0, 1.0) * 256.0).round() as u32;
    if op_256 == 0 {
        return;
    }
    let pw = dst.width();
    let [x0, y0, x1, y1] = [
        bounds[0].min(pw),
        bounds[1].min(dst.height()),
        bounds[2].min(pw),
        bounds[3].min(dst.height()),
    ];
    let mask_bytes = mask.map(tiny_skia::Mask::data);
    let src_data = src.data();
    let dst_data = dst.data_mut();
    for py in y0..y1 {
        let row_px = (py * pw) as usize;
        for px in x0..x1 {
            let d_px = row_px + px as usize;
            let s = d_px * 4;
            let sa0 = src_data[s + 3] as u32;
            if sa0 == 0 {
                continue;
            }
            let m = mask_bytes.map_or(255_u32, |mb| mb[d_px] as u32);
            if m == 0 {
                continue;
            }
            if m == 255 && op_256 == 256 && sa0 == 255 {
                dst_data[s..s + 4].copy_from_slice(&src_data[s..s + 4]);
                continue;
            }
            let w256 = (op_256 * (m + (m >> 7))) >> 8;
            let sa = (sa0 * w256) >> 8;
            if sa == 0 {
                continue;
            }
            let inv = 255 - sa;
            dst_data[s] = (((src_data[s] as u32 * w256) >> 8)
                + ((dst_data[s] as u32 * inv + 127) / 255))
                .min(255) as u8;
            dst_data[s + 1] = (((src_data[s + 1] as u32 * w256) >> 8)
                + ((dst_data[s + 1] as u32 * inv + 127) / 255))
                .min(255) as u8;
            dst_data[s + 2] = (((src_data[s + 2] as u32 * w256) >> 8)
                + ((dst_data[s + 2] as u32 * inv + 127) / 255))
                .min(255) as u8;
            dst_data[s + 3] = (sa + ((dst_data[s + 3] as u32 * inv + 127) / 255)).min(255) as u8;
        }
    }
}

fn composite_identity_offscreen(
    parent_px: &mut tiny_skia::Pixmap,
    src_px: &tiny_skia::Pixmap,
    mask_cache: &mut ClipMaskCache,
    (clip, clip_radius, clip_radii, opacity): (Option<LayoutRect>, f32, [f32; 4], f32),
    bounds: [u32; 4],
    scale: f32,
) -> Option<[u32; 4]> {
    let vp = (parent_px.width(), parent_px.height());
    let eff_bounds = match clip {
        Some(c) => intersect_bounds(bounds, clip_rect_phys(c, scale, vp, 2.0)?)?,
        None => bounds,
    };
    let mask = clip.and_then(|c| {
        mask_cache.get_or_build_4(vp.0, vp.1, Some(c), clip_radius, clip_radii, scale)
    });
    composite_subrect_fast(parent_px, src_px, eff_bounds, mask, opacity);
    Some(eff_bounds)
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
        SkiaLayerEntry::Offscreen(mut layer) => {
            *active_offscreens = active_offscreens.saturating_sub(1);
            let (pw, ph) = (renderer.pixmap.width(), renderer.pixmap.height());
            let mut written_parent_bounds = layer.drawn_bounds;
            let fast_path = layer.filter.is_identity()
                && layer.perspective.is_none()
                && layer.local_clip.is_none()
                && layer.clip_path.is_none();
            if fast_path {
                if let Some(bounds) = layer.drawn_bounds {
                    written_parent_bounds = composite_identity_offscreen(
                        &mut layer.parent_px,
                        &renderer.pixmap,
                        &mut renderer.mask_cache,
                        (
                            layer.clip,
                            layer.clip_radius,
                            layer.clip_radii,
                            layer.opacity,
                        ),
                        bounds,
                        scale,
                    );
                }
            } else {
                apply_layer_filter_skia(&mut renderer.pixmap, layer.filter, scale);
                composite_layer_skia(
                    &mut layer.parent_px,
                    &renderer.pixmap,
                    &mut renderer.mask_cache,
                    (
                        layer.clip,
                        layer.clip_radius,
                        layer.clip_radii,
                        layer.local_clip,
                        layer.clip_path,
                    ),
                    layer.perspective,
                    layer.opacity,
                    scale,
                );
                layer.drawn_bounds = Some([0, 0, pw, ph]);
                written_parent_bounds = Some([0, 0, pw, ph]);
            }
            if let Some(SkiaLayerEntry::Offscreen(parent_layer)) = layer_stack.last_mut() {
                parent_layer.drawn_bounds =
                    union_bounds(parent_layer.drawn_bounds, written_parent_bounds);
            }
            let child_px = std::mem::replace(&mut renderer.pixmap, layer.parent_px);
            renderer.layer_pool.push((child_px, layer.drawn_bounds));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RendererBackend;
    use krui_core::{GpuResourceLimits, PaintScene};
    use krui_style::Color;

    fn mk_box(x: f32, y: f32, w: f32, h: f32, col: Color) -> DrawPrimitive {
        DrawPrimitive::StyledBox {
            rect: LayoutRect {
                x,
                y,
                width: w,
                height: h,
            },
            clip: None,
            clip_radius: 0.0,
            clip_radii: [0.0; 4],
            transform: Affine2::IDENTITY,
            opacity: 1.0,
            background_color: col,
            background_gradient: None,
            radial_gradient: None,
            conic_gradient: None,
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            border_radius: 4.0,
            corner_radii: [0.0; 4],
            box_shadow: None,
            backdrop_blur_px: 0.0,
        }
    }

    fn mk_scene(bx: f32, by: f32, col: Color, opacity: f32) -> PaintScene {
        let mut s = PaintScene::new(160, 120, Color::BLACK);
        s.primitives.push(DrawPrimitive::PushLayer {
            bounds: LayoutRect {
                x: bx,
                y: by,
                width: 48.0,
                height: 32.0,
            },
            clip: None,
            clip_radius: 0.0,
            clip_radii: [0.0; 4],
            local_clip: None,
            clip_path: None,
            perspective: None,
            opacity,
            filter: CssFilterEffect::IDENTITY,
        });
        s.primitives.push(mk_box(bx, by, 48.0, 32.0, col));
        s.primitives.push(DrawPrimitive::PopLayer);
        s
    }

    #[test]
    fn pooled_subrect_layer_clears_previous_frame_pixels_cleanly() {
        let mut r = TinySkiaRenderer::new(160, 120, GpuResourceLimits::default());
        let first = mk_scene(
            12.0,
            16.0,
            Color {
                r: 220,
                g: 60,
                b: 60,
                a: 255,
            },
            0.75,
        );
        r.render_scene(&first);
        let second = mk_scene(
            90.0,
            70.0,
            Color {
                r: 60,
                g: 200,
                b: 120,
                a: 255,
            },
            0.75,
        );
        r.render_scene(&second);

        let mut fresh = TinySkiaRenderer::new(160, 120, GpuResourceLimits::default());
        fresh.render_scene(&second);
        assert_eq!(r.framebuffer_rgba(), fresh.framebuffer_rgba());
    }
}
