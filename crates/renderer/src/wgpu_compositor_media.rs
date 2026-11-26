//! Canvas and raster/external image primitive emission helpers for `WgpuFrameResources`.

use crate::dual_atlas::AtlasPackError;
use crate::sdf_instance::SdfQuadInstance;
use crate::tessellate::compute_physical_scissor;
use crate::wgpu_compositor::{
    ActiveLayer, AuxBinding, LayerSlotKind, PassDrawItem, RecordCounters, RecordEnv,
    WgpuFrameResources,
};
use crate::wgpu_compositor_emit::pop_layer_into_parent;
use crate::wgpu_compositor_exec::submit_recorded_segment;
use krui_core::ImageSource;
use krui_layout::LayoutRect;
use krui_style::{Affine2, Color};
use std::sync::Arc;

fn reserve_canvas_budget(
    prim_clamped: bool,
    vert_len: usize,
    idx_len: usize,
    counters: &mut RecordCounters,
    limits: &krui_core::GpuResourceLimits,
) -> bool {
    let over_cap = counters.verts + vert_len > limits.max_vertices_per_frame
        || counters.indices + idx_len > limits.max_indices_per_frame;
    if prim_clamped || over_cap {
        counters.clamped += 1;
        if over_cap {
            return false;
        }
    }
    counters.verts += vert_len;
    counters.indices += idx_len;
    true
}

impl WgpuFrameResources {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_canvas_batch(
        &mut self,
        target_id: usize,
        viewport: LayoutRect,
        clip_pair: (Option<LayoutRect>, f32, [f32; 4]),
        transform: Affine2,
        opacity: f32,
        commands: &Arc<[krui_core::CanvasCommand]>,
        env: &RecordEnv<'_>,
        initialized: &mut [bool],
        counters: &mut RecordCounters,
        free_layers: &mut Vec<usize>,
    ) {
        if opacity <= 0.0 {
            return;
        }
        if commands
            .iter()
            .any(|c| matches!(c, krui_core::CanvasCommand::PushClip { .. }))
        {
            self.emit_clipped_canvas_batch(
                target_id,
                viewport,
                clip_pair,
                transform,
                opacity,
                commands,
                env,
                initialized,
                counters,
            );
            return;
        }
        let (clip, clip_radius, _) = clip_pair;
        let (vw, vh) = env.vp_size;
        let Some(scissor) = compute_physical_scissor(clip, env.sf.surface_scale, vw, vh) else {
            return;
        };
        let logical_vp = (
            env.sf.phys_w / env.sf.surface_scale,
            env.sf.phys_h / env.sf.surface_scale,
        );
        let cached = self.canvas_cache.get_or_tessellate(
            commands,
            viewport,
            clip,
            clip_radius,
            env.sf.surface_scale,
            transform,
            opacity,
            logical_vp,
            env.limits,
        );
        let (verts, indices, prim_clamped) = cached.as_ref();
        if !reserve_canvas_budget(
            *prim_clamped,
            verts.len(),
            indices.len(),
            counters,
            &env.limits,
        ) || verts.is_empty()
            || indices.is_empty()
        {
            return;
        }
        self.push_canvas_draw(
            target_id,
            verts,
            indices,
            scissor,
            clip_pair,
            env,
            initialized,
            free_layers,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_clipped_canvas_batch(
        &mut self,
        target_id: usize,
        viewport: LayoutRect,
        clip_pair: (Option<LayoutRect>, f32, [f32; 4]),
        transform: Affine2,
        opacity: f32,
        commands: &Arc<[krui_core::CanvasCommand]>,
        env: &RecordEnv<'_>,
        initialized: &mut [bool],
        counters: &mut RecordCounters,
    ) {
        if let Some(decoded) =
            self.canvas_cache
                .get_or_rasterize_clipped(commands, viewport, env.sf.surface_scale)
        {
            self.emit_image_primitive(
                target_id,
                viewport,
                [0.0, 0.0, 1.0, 1.0],
                clip_pair,
                transform,
                opacity,
                0.0,
                &ImageSource::Decoded(decoded),
                env,
                initialized,
                counters,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_canvas_draw(
        &mut self,
        target_id: usize,
        verts: &[crate::tessellate::GpuVertex],
        indices: &[u32],
        scissor: Option<[u32; 4]>,
        (clip, clip_radius, clip_radii): (Option<LayoutRect>, f32, [f32; 4]),
        env: &RecordEnv<'_>,
        initialized: &mut [bool],
        free_layers: &mut Vec<usize>,
    ) {
        let base_vertex = self.segment.canvas_verts.len() as i32;
        let idx_start = self.segment.canvas_indices.len() as u32;
        self.segment.canvas_verts.extend_from_slice(verts);
        self.segment.canvas_indices.extend_from_slice(indices);
        let idx_end = self.segment.canvas_indices.len() as u32;
        let max_r = clip_radii.into_iter().fold(clip_radius, f32::max);
        let rounded_layer = (clip.is_some() && (max_r * env.sf.surface_scale) > 0.5)
            .then(|| free_layers.pop())
            .flatten();
        let draw_tid = match rounded_layer {
            Some(pool_idx) => {
                initialized[1 + pool_idx] = false;
                1 + pool_idx
            }
            None => target_id,
        };
        let items = self
            .segment
            .ensure_draw_pass(draw_tid, initialized, env.clear_color);
        items.push(PassDrawItem::Canvas {
            index_range: idx_start..idx_end,
            base_vertex,
            scissor,
        });
        if let Some(pool_idx) = rounded_layer {
            let layer = ActiveLayer {
                kind: LayerSlotKind::Offscreen(pool_idx),
                clip,
                clip_radius,
                clip_radii,
                local_clip: None,
                clip_path: None,
                perspective: None,
                opacity: 1.0,
                filter: krui_style::CssFilterEffect::IDENTITY,
                drawn_bounds: Some([0.0, 0.0, env.sf.phys_w, env.sf.phys_h]),
            };
            pop_layer_into_parent(&mut self.segment, &layer, target_id, env, initialized);
            free_layers.push(pool_idx);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_image_primitive(
        &mut self,
        target_id: usize,
        rect: LayoutRect,
        uv_rect: [f32; 4],
        (clip, clip_radius, clip_radii): (Option<LayoutRect>, f32, [f32; 4]),
        transform: Affine2,
        opacity: f32,
        border_radius: f32,
        source: &ImageSource,
        env: &RecordEnv<'_>,
        initialized: &mut [bool],
        counters: &mut RecordCounters,
    ) {
        if !counters.try_reserve(4, 6, &env.limits) {
            return;
        }
        let is_premul = matches!(source, ImageSource::Decoded(_));
        let Some(inst) = SdfQuadInstance::image_quad(
            rect,
            uv_rect,
            clip,
            clip_radius,
            clip_radii,
            transform,
            opacity,
            border_radius,
            is_premul,
            env.sf,
        ) else {
            return;
        };
        let aux_bg = match source {
            ImageSource::Decoded(img) => Some(env.ctx.resolve_image_aux_bg(img)),
            ImageSource::External { id, .. } => env.ctx.resolve_external_aux_bg(*id),
        };
        let Some(bg) = aux_bg else {
            return;
        };
        let aux_idx = self.segment.intern_aux_bg(bg);
        self.segment.push_sdf(
            target_id,
            initialized,
            env.clear_color,
            inst,
            AuxBinding::Custom(aux_idx),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_text_glyphs(
        &mut self,
        target_id: usize,
        origin: (f32, f32),
        (clip, clip_radius, clip_radii): (Option<LayoutRect>, f32, [f32; 4]),
        transform: Affine2,
        opacity: f32,
        color: Color,
        glyphs: &[krui_text::RasterizedGlyph],
        env: &RecordEnv<'_>,
        initialized: &mut [bool],
        counters: &mut RecordCounters,
    ) {
        if opacity <= 0.0 {
            return;
        }
        for g in glyphs {
            if !counters.try_reserve(4, 6, &env.limits) {
                break;
            }
            let (is_color, raw): (bool, &[u8]) = match &g.color_rgba {
                Some(rgba) => (true, rgba.as_ref()),
                None => (false, g.alpha_mask.as_ref()),
            };
            let Some(slot) =
                self.pack_or_evict_glyph(g.cache_key, g.width, g.height, is_color, raw, env)
            else {
                continue;
            };
            if let Some(inst) = SdfQuadInstance::glyph(
                g,
                slot,
                origin,
                clip,
                clip_radius,
                clip_radii,
                transform,
                opacity,
                color,
                env.sf,
            ) {
                self.segment.push_sdf(
                    target_id,
                    initialized,
                    env.clear_color,
                    inst,
                    AuxBinding::Dummy,
                );
            }
        }
    }

    fn pack_or_evict_glyph(
        &mut self,
        cache_key: u64,
        w: u32,
        h: u32,
        is_color: bool,
        raw: &[u8],
        env: &RecordEnv<'_>,
    ) -> Option<crate::dual_atlas::GlyphSlot> {
        let first_try = {
            let mut guard = env.ctx.atlas.lock().expect("atlas mutex");
            guard.packer.try_pack_glyph(cache_key, w, h, is_color, raw)
        };
        match first_try {
            Ok(slot) => Some(slot),
            Err(AtlasPackError::InvalidOrOversized) => None,
            Err(AtlasPackError::AllLayersFull) => {
                submit_recorded_segment(
                    self,
                    env.ctx,
                    env.out_view,
                    env.out_format,
                    env.vp_size,
                    env.use_swapchain_scene,
                    false,
                );
                self.segment.clear();
                let mut guard = env.ctx.atlas.lock().expect("atlas mutex");
                let _ = guard.packer.evict_oldest_page(is_color);
                guard
                    .packer
                    .try_pack_glyph(cache_key, w, h, is_color, raw)
                    .ok()
            }
        }
    }
}
