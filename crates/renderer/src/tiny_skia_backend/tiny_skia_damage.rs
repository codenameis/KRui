//! Sub-rectangle clipped partial CPU rasterization (`render_scene_damaged`) for `TinySkiaRenderer`.

use super::tiny_skia_filter::{record_offscreen_prim_bounds, step_layer_op_skia};
use super::tiny_skia_shapes::to_skia_color;
use super::{render_primitive_skia, SkiaRenderCaches, TinySkiaRenderer};
use crate::RendererBackend;
use krui_core::{ActiveBackendKind, DrawPrimitive, FrameRenderStats, PaintScene};
use krui_layout::LayoutRect;
use krui_style::{Affine2, Color, DamageRect};

const FULL_REDRAW_AREA_PCT: u64 = 65;

impl TinySkiaRenderer {
    /// Rasterizes only the pixels covered by `dirty_rects`, preserving pixels outside `dirty_rects`
    /// untouched when dimensions already match. Falls back to `render_scene` on initial frame,
    /// viewport resize, or high damage coverage (`>= 65%`).
    pub fn render_scene_damaged(
        &mut self,
        scene: &PaintScene,
        dirty_rects: &[DamageRect],
    ) -> FrameRenderStats {
        let scale = scene.scale_factor.clamp(1.0, 4.0);
        let phys_w = ((scene.width as f32 * scale).round() as u32).max(1);
        let phys_h = ((scene.height as f32 * scale).round() as u32).max(1);
        if !self.has_rendered || self.pixmap.width() != phys_w || self.pixmap.height() != phys_h {
            return self.render_scene(scene);
        }
        if dirty_rects.is_empty() {
            return FrameRenderStats {
                backend_kind: ActiveBackendKind::TinySkiaCpuFallback,
                draw_primitives_submitted: 0,
                draw_primitives_clamped: 0,
                vertices_generated: 0,
                indices_generated: 0,
            };
        }
        let fb_area = (phys_w as u64) * (phys_h as u64);
        let dmg_area: u64 = dirty_rects
            .iter()
            .map(|r| r.clamp_to(phys_w, phys_h).area())
            .sum();
        if dmg_area.saturating_mul(100) >= fb_area.saturating_mul(FULL_REDRAW_AREA_PCT) {
            return self.render_scene(scene);
        }

        let max_cmds = scene
            .primitives
            .len()
            .min(self.limits.max_draw_commands_per_frame);
        let skipped = scene.primitives.len().saturating_sub(max_cmds);
        let (mut total_clamped, mut total_v, mut total_i) = (skipped, 0_usize, 0_usize);
        for &raw in dirty_rects {
            let rect = raw.clamp_to(phys_w, phys_h);
            if rect.is_empty() {
                continue;
            }
            clear_pixmap_subrect(&mut self.pixmap, rect, scene.clear_color);
            let (c, v, i) = self.rasterize_rect_region(&scene.primitives[..max_cmds], rect, scale);
            total_clamped += c;
            total_v += v;
            total_i += i;
        }
        self.layer_pool.truncate(4);
        FrameRenderStats {
            backend_kind: ActiveBackendKind::TinySkiaCpuFallback,
            draw_primitives_submitted: max_cmds
                .saturating_sub(total_clamped.saturating_sub(skipped)),
            draw_primitives_clamped: total_clamped,
            vertices_generated: total_v,
            indices_generated: total_i,
        }
    }

    fn rasterize_rect_region(
        &mut self,
        primitives: &[DrawPrimitive],
        rect: DamageRect,
        scale: f32,
    ) -> (usize, usize, usize) {
        let vp = (self.pixmap.width(), self.pixmap.height());
        let scale_tx = Affine2::scale(scale, scale);
        let dmg_logical = LayoutRect {
            x: rect.x as f32 / scale,
            y: rect.y as f32 / scale,
            width: rect.width as f32 / scale,
            height: rect.height as f32 / scale,
        };
        let (clamped, mut verts, mut idx) = (0_usize, 0_usize, 0_usize);
        let (mut stack, mut active, mut inline_op) = (Vec::new(), 0, 1.0_f32);
        let mut scratch: Option<DrawPrimitive> = None;

        for prim in primitives {
            if matches!(
                prim,
                DrawPrimitive::PushLayer { .. } | DrawPrimitive::PopLayer
            ) {
                let clipped = clip_primitive_to_damage(prim, dmg_logical, &mut scratch);
                let _ = step_layer_op_skia(
                    self,
                    clipped,
                    &mut stack,
                    &mut active,
                    &mut inline_op,
                    vp,
                    scale,
                );
                continue;
            }
            if !primitive_overlaps_damage(prim, rect, scale) {
                continue;
            }
            let clipped = clip_primitive_to_damage(prim, dmg_logical, &mut scratch);
            let mut caches = SkiaRenderCaches {
                mask: &mut self.mask_cache,
                shadow: &mut self.shadow_cache,
                image: &mut self.image_cache,
            };
            let (v, i) = render_primitive_skia(
                &mut self.pixmap,
                &mut caches,
                clipped,
                scale,
                scale_tx,
                inline_op,
            );
            record_offscreen_prim_bounds(&mut stack, clipped, scale, vp);
            verts += v;
            idx += i;
        }
        while !stack.is_empty() {
            let pop = DrawPrimitive::PopLayer;
            let _ = step_layer_op_skia(
                self,
                &pop,
                &mut stack,
                &mut active,
                &mut inline_op,
                vp,
                scale,
            );
        }
        (clamped, verts, idx)
    }
}

fn clear_pixmap_subrect(pixmap: &mut tiny_skia::Pixmap, rect: DamageRect, clear_color: Color) {
    let sc = to_skia_color(clear_color, 1.0);
    let r = (sc.red() * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = (sc.green() * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = (sc.blue() * 255.0).round().clamp(0.0, 255.0) as u8;
    let a = (sc.alpha() * 255.0).round().clamp(0.0, 255.0) as u8;
    let pm = [r, g, b, a];
    let stride = pixmap.width() as usize * 4;
    let data = pixmap.data_mut();
    for y in rect.y..rect.bottom() {
        let row_start = y as usize * stride + rect.x as usize * 4;
        let row_end = row_start + rect.width as usize * 4;
        for chunk in data[row_start..row_end].as_chunks_mut::<4>().0 {
            chunk.copy_from_slice(&pm);
        }
    }
}

fn intersect_opt_rect(a: Option<LayoutRect>, b: LayoutRect) -> Option<LayoutRect> {
    let Some(r) = a else {
        return Some(b);
    };
    let x0 = r.x.max(b.x);
    let y0 = r.y.max(b.y);
    let x1 = (r.x + r.width).min(b.x + b.width).max(x0);
    let y1 = (r.y + r.height).min(b.y + b.height).max(y0);
    Some(LayoutRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

fn clip_primitive_to_damage<'a>(
    prim: &'a DrawPrimitive,
    dmg: LayoutRect,
    scratch: &'a mut Option<DrawPrimitive>,
) -> &'a DrawPrimitive {
    let mut cloned = prim.clone();
    match &mut cloned {
        DrawPrimitive::StyledBox { clip, .. }
        | DrawPrimitive::TextGlyphs { clip, .. }
        | DrawPrimitive::Image { clip, .. }
        | DrawPrimitive::CanvasBatch { clip, .. }
        | DrawPrimitive::PushLayer { clip, .. } => {
            *clip = intersect_opt_rect(*clip, dmg);
        }
        DrawPrimitive::PopLayer => {}
    }
    *scratch = Some(cloned);
    scratch.as_ref().expect("assigned above")
}

fn glyph_batch_bounds(
    origin_x: f32,
    origin_y: f32,
    glyphs: &[krui_text::RasterizedGlyph],
) -> Option<LayoutRect> {
    if glyphs.is_empty() {
        return None;
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for g in glyphs.iter() {
        let (gx, gy) = (origin_x + g.x, origin_y + g.y);
        min_x = min_x.min(gx);
        min_y = min_y.min(gy);
        max_x = max_x.max(gx + g.width as f32);
        max_y = max_y.max(gy + g.height as f32);
    }
    Some(LayoutRect {
        x: min_x,
        y: min_y,
        width: (max_x - min_x).max(0.0),
        height: (max_y - min_y).max(0.0),
    })
}

fn primitive_overlaps_damage(prim: &DrawPrimitive, dmg: DamageRect, scale: f32) -> bool {
    let (r, tx, pad) = match prim {
        DrawPrimitive::StyledBox {
            rect,
            transform,
            box_shadow,
            ..
        } => {
            let p = box_shadow
                .map(|s| {
                    s.blur_radius.max(0.0) * 2.5
                        + s.spread_radius.abs()
                        + s.offset_x.abs().max(s.offset_y.abs())
                })
                .unwrap_or(0.0);
            (*rect, *transform, 2.0 + p)
        }
        DrawPrimitive::Image {
            rect, transform, ..
        } => (*rect, *transform, 2.0),
        DrawPrimitive::TextGlyphs {
            origin_x,
            origin_y,
            transform,
            glyphs,
            ..
        } => {
            let Some(rect) = glyph_batch_bounds(*origin_x, *origin_y, glyphs) else {
                return false;
            };
            (rect, *transform, 2.0)
        }
        DrawPrimitive::CanvasBatch {
            viewport,
            transform,
            ..
        } => (*viewport, *transform, 2.0),
        DrawPrimitive::PushLayer { .. } | DrawPrimitive::PopLayer => return true,
    };
    let pts = [
        tx.transform_point(r.x - pad, r.y - pad),
        tx.transform_point(r.x + r.width + pad, r.y - pad),
        tx.transform_point(r.x + r.width + pad, r.y + r.height + pad),
        tx.transform_point(r.x - pad, r.y + r.height + pad),
    ];
    let min_x = (pts.iter().map(|p| p.0).fold(f32::INFINITY, f32::min) * scale)
        .floor()
        .max(0.0) as u32;
    let min_y = (pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min) * scale)
        .floor()
        .max(0.0) as u32;
    let max_x = (pts.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max) * scale)
        .ceil()
        .max(0.0) as u32;
    let max_y = (pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max) * scale)
        .ceil()
        .max(0.0) as u32;
    DamageRect::new(
        min_x,
        min_y,
        max_x.saturating_sub(min_x),
        max_y.saturating_sub(min_y),
    )
    .intersects(dmg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use krui_core::GpuResourceLimits;
    use krui_style::Affine2;

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
            border_radius: 0.0,
            corner_radii: [0.0; 4],
            box_shadow: None,
            backdrop_blur_px: 0.0,
        }
    }

    #[test]
    fn damaged_partial_render_matches_full_render_pixel_for_pixel() {
        let mut s = PaintScene::new(
            160,
            120,
            Color {
                r: 12,
                g: 16,
                b: 24,
                a: 255,
            },
        );
        s.primitives.push(mk_box(
            10.0,
            10.0,
            140.0,
            100.0,
            Color {
                r: 30,
                g: 41,
                b: 59,
                a: 255,
            },
        ));
        s.primitives.push(mk_box(
            20.0,
            20.0,
            30.0,
            20.0,
            Color {
                r: 59,
                g: 130,
                b: 246,
                a: 255,
            },
        ));

        let mut partial = TinySkiaRenderer::new(160, 120, GpuResourceLimits::default());
        partial.render_scene(&s);
        s.primitives[1] = mk_box(
            20.0,
            20.0,
            30.0,
            20.0,
            Color {
                r: 34,
                g: 197,
                b: 94,
                a: 255,
            },
        );
        partial.render_scene_damaged(&s, &[DamageRect::new(18, 18, 34, 24)]);

        let mut full = TinySkiaRenderer::new(160, 120, GpuResourceLimits::default());
        full.render_scene(&s);
        assert_eq!(partial.framebuffer_rgba(), full.framebuffer_rgba());
    }
}
