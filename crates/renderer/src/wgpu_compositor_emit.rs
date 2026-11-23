//! Primitive emission helpers (`StyledBox`, `TextGlyphs`, `CanvasBatch`) for `WgpuFrameResources`.

use crate::blur_pass::plan_dual_kawase_steps;
use crate::sdf_instance::{BoxInstanceSpec, SdfQuadInstance};
use crate::wgpu_compositor::{
    ActiveLayer, AuxBinding, EncoderSegment, LayerSlotKind, RecordCounters, RecordEnv, SegmentOp,
    WgpuFrameResources,
};
use krui_core::DrawPrimitive;
use krui_style::BoxShadow;

impl WgpuFrameResources {
    pub(crate) fn record_primitives(
        &mut self,
        primitives: &[DrawPrimitive],
        env: &RecordEnv<'_>,
        initialized: &mut [bool],
        counters: &mut RecordCounters,
    ) {
        let mut layer_stack: Vec<ActiveLayer> = Vec::new();
        let mut free_layers: Vec<usize> = (0..self.layer_pool.layers.len()).rev().collect();
        let mut inline_opacity = 1.0_f32;
        for prim in primitives {
            if handle_layer_op(
                &mut self.segment,
                prim,
                &mut layer_stack,
                &mut free_layers,
                &mut inline_opacity,
                env,
                initialized,
            ) {
                continue;
            }
            let start_sdf = self.segment.instances.len();
            let start_cv = self.segment.canvas_verts.len();
            let active_tid = active_offscreen_tid(&layer_stack);
            self.record_draw_primitive(
                active_tid,
                inline_opacity,
                prim,
                env,
                initialized,
                counters,
                &mut free_layers,
            );
            accumulate_active_layer_bounds(
                &mut layer_stack,
                &self.segment,
                start_sdf,
                start_cv,
                env,
            );
        }
        while let Some(popped) = layer_stack.pop() {
            let parent_tid = active_offscreen_tid(&layer_stack);
            pop_layer_into_parent(&mut self.segment, &popped, parent_tid, env, initialized);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record_draw_primitive(
        &mut self,
        active_tid: usize,
        inline_opacity: f32,
        prim: &DrawPrimitive,
        env: &RecordEnv<'_>,
        initialized: &mut [bool],
        counters: &mut RecordCounters,
        free_layers: &mut Vec<usize>,
    ) {
        if let Some((mut spec, shadow, blur_px)) = unpack_styled_box_spec(prim) {
            spec.opacity *= inline_opacity;
            if counters.try_reserve(4, 6, &env.limits) {
                self.emit_styled_box_ops(active_tid, &spec, shadow, blur_px, env, initialized);
            }
            return;
        }
        self.record_non_box_primitive(
            active_tid,
            inline_opacity,
            prim,
            env,
            initialized,
            counters,
            free_layers,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn record_non_box_primitive(
        &mut self,
        active_tid: usize,
        inline_opacity: f32,
        prim: &DrawPrimitive,
        env: &RecordEnv<'_>,
        initialized: &mut [bool],
        counters: &mut RecordCounters,
        free_layers: &mut Vec<usize>,
    ) {
        if let DrawPrimitive::TextGlyphs {
            origin_x,
            origin_y,
            clip,
            clip_radius,
            clip_radii,
            transform,
            opacity,
            color,
            glyphs,
        } = prim
        {
            self.emit_text_glyphs(
                active_tid,
                (*origin_x, *origin_y),
                (*clip, *clip_radius, *clip_radii),
                *transform,
                *opacity * inline_opacity,
                *color,
                glyphs,
                env,
                initialized,
                counters,
            );
            return;
        }
        self.record_canvas_or_image(
            active_tid,
            inline_opacity,
            prim,
            env,
            initialized,
            counters,
            free_layers,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn record_canvas_or_image(
        &mut self,
        active_tid: usize,
        inline_opacity: f32,
        prim: &DrawPrimitive,
        env: &RecordEnv<'_>,
        initialized: &mut [bool],
        counters: &mut RecordCounters,
        free_layers: &mut Vec<usize>,
    ) {
        match prim {
            DrawPrimitive::CanvasBatch {
                viewport,
                clip,
                clip_radius,
                clip_radii,
                transform,
                opacity,
                commands,
            } => self.emit_canvas_batch(
                active_tid,
                *viewport,
                (*clip, *clip_radius, *clip_radii),
                *transform,
                *opacity * inline_opacity,
                commands,
                env,
                initialized,
                counters,
                free_layers,
            ),
            DrawPrimitive::Image {
                rect,
                uv_rect,
                clip,
                clip_radius,
                clip_radii,
                transform,
                opacity,
                border_radius,
                source,
            } => self.emit_image_primitive(
                active_tid,
                *rect,
                *uv_rect,
                (*clip, *clip_radius, *clip_radii),
                *transform,
                *opacity * inline_opacity,
                *border_radius,
                source,
                env,
                initialized,
                counters,
            ),
            _ => {}
        }
    }

    pub(crate) fn emit_styled_box_ops(
        &mut self,
        target_id: usize,
        spec: &BoxInstanceSpec,
        box_shadow: Option<BoxShadow>,
        backdrop_blur_px: f32,
        env: &RecordEnv<'_>,
        initialized: &mut [bool],
    ) {
        let blur_inst =
            self.plan_box_backdrop_blur(target_id, spec, backdrop_blur_px, env, initialized);
        if let Some(sh) = box_shadow.filter(|s| !s.inset) {
            if let Some(sh_inst) = SdfQuadInstance::box_shadow(spec, sh, env.sf) {
                self.segment.push_sdf(
                    target_id,
                    initialized,
                    env.clear_color,
                    sh_inst,
                    AuxBinding::Dummy,
                );
            }
        }
        if let Some(bi) = blur_inst {
            self.segment.push_sdf(
                target_id,
                initialized,
                env.clear_color,
                bi,
                AuxBinding::BlurredHalfB,
            );
        }
        if let Some(rg) = spec
            .radial_gradient
            .filter(|_| spec.background_gradient.is_some())
        {
            let mut base_spec = *spec;
            base_spec.border_width = 0.0;
            if let Some(inst) = SdfQuadInstance::styled_box(&base_spec, env.sf) {
                self.segment.push_sdf(
                    target_id,
                    initialized,
                    env.clear_color,
                    inst,
                    AuxBinding::Dummy,
                );
            }
            if let Some(rad_inst) = SdfQuadInstance::radial_overlay(spec, rg, env.sf) {
                self.segment.push_sdf(
                    target_id,
                    initialized,
                    env.clear_color,
                    rad_inst,
                    AuxBinding::Dummy,
                );
            }
        } else if let Some(inst) = SdfQuadInstance::styled_box(spec, env.sf) {
            self.segment.push_sdf(
                target_id,
                initialized,
                env.clear_color,
                inst,
                AuxBinding::Dummy,
            );
        }
        if let Some(sh) = box_shadow.filter(|s| s.inset) {
            if let Some(sh_inst) = SdfQuadInstance::box_shadow(spec, sh, env.sf) {
                self.segment.push_sdf(
                    target_id,
                    initialized,
                    env.clear_color,
                    sh_inst,
                    AuxBinding::Dummy,
                );
            }
        }
    }

    fn plan_box_backdrop_blur(
        &mut self,
        target_id: usize,
        spec: &BoxInstanceSpec,
        backdrop_blur_px: f32,
        env: &RecordEnv<'_>,
        initialized: &mut [bool],
    ) -> Option<SdfQuadInstance> {
        if backdrop_blur_px <= 0.5 || spec.opacity <= 0.0 {
            return None;
        }
        let blur_inst = SdfQuadInstance::backdrop_blur_box(spec, env.sf)?;
        let _ = self
            .segment
            .ensure_draw_pass(target_id, initialized, env.clear_color);
        let mut steps = Vec::with_capacity(5);
        plan_dual_kawase_steps(
            target_id,
            env.vp_size,
            blur_inst.screen_bounds,
            backdrop_blur_px * env.sf.surface_scale,
            env.ctx.blur.uniform_align,
            &mut self.segment.blur_uniform_bytes,
            &mut steps,
        );
        self.segment.blur_count += steps.len();
        self.segment.ops.push(SegmentOp::BlurChain { steps });
        Some(blur_inst)
    }
}

fn union_bounds(dst: &mut Option<[f32; 4]>, b: [f32; 4]) {
    *dst = Some(match *dst {
        Some(prev) => [
            prev[0].min(b[0]),
            prev[1].min(b[1]),
            prev[2].max(b[2]),
            prev[3].max(b[3]),
        ],
        None => b,
    });
}

fn accumulate_active_layer_bounds(
    layer_stack: &mut [ActiveLayer],
    segment: &EncoderSegment,
    start_sdf: usize,
    start_cv: usize,
    env: &RecordEnv<'_>,
) {
    let Some(layer) = layer_stack
        .iter_mut()
        .rev()
        .find(|l| matches!(l.kind, LayerSlotKind::Offscreen(_)))
    else {
        return;
    };
    for inst in &segment.instances[start_sdf..] {
        union_bounds(&mut layer.drawn_bounds, inst.screen_bounds);
    }
    for v in &segment.canvas_verts[start_cv..] {
        let px = (v.position[0] * 0.5 + 0.5) * env.sf.phys_w;
        let py = (0.5 - v.position[1] * 0.5) * env.sf.phys_h;
        union_bounds(&mut layer.drawn_bounds, [px, py, px + 1.0, py + 1.0]);
    }
}

fn active_offscreen_tid(layer_stack: &[ActiveLayer]) -> usize {
    layer_stack
        .iter()
        .rev()
        .find_map(|l| match l.kind {
            LayerSlotKind::Offscreen(idx) => Some(1 + idx),
            LayerSlotKind::Overflow { .. } => None,
        })
        .unwrap_or(0)
}

fn handle_layer_op(
    segment: &mut EncoderSegment,
    prim: &DrawPrimitive,
    layer_stack: &mut Vec<ActiveLayer>,
    free_layers: &mut Vec<usize>,
    inline_opacity: &mut f32,
    env: &RecordEnv<'_>,
    initialized: &mut [bool],
) -> bool {
    match prim {
        DrawPrimitive::PushLayer {
            clip,
            clip_radius,
            clip_radii,
            local_clip,
            opacity,
            ..
        } => {
            let kind = if let Some(pool_idx) = free_layers.pop() {
                initialized[1 + pool_idx] = false;
                LayerSlotKind::Offscreen(pool_idx)
            } else {
                let prev = *inline_opacity;
                *inline_opacity = (prev * opacity.clamp(0.0, 1.0)).clamp(0.0, 1.0);
                LayerSlotKind::Overflow {
                    prev_inline_opacity: prev,
                }
            };
            layer_stack.push(ActiveLayer {
                kind,
                clip: *clip,
                clip_radius: *clip_radius,
                clip_radii: *clip_radii,
                local_clip: *local_clip,
                opacity: *opacity,
                drawn_bounds: None,
            });
            true
        }
        DrawPrimitive::PopLayer => {
            if let Some(popped) = layer_stack.pop() {
                match popped.kind {
                    LayerSlotKind::Offscreen(pool_idx) => {
                        let start_sdf = segment.instances.len();
                        let parent_tid = active_offscreen_tid(layer_stack);
                        pop_layer_into_parent(segment, &popped, parent_tid, env, initialized);
                        accumulate_active_layer_bounds(
                            layer_stack,
                            segment,
                            start_sdf,
                            segment.canvas_verts.len(),
                            env,
                        );
                        free_layers.push(pool_idx);
                    }
                    LayerSlotKind::Overflow {
                        prev_inline_opacity,
                    } => {
                        *inline_opacity = prev_inline_opacity;
                    }
                }
            }
            true
        }
        _ => false,
    }
}

pub(crate) fn pop_layer_into_parent(
    segment: &mut EncoderSegment,
    popped: &ActiveLayer,
    parent_tid: usize,
    env: &RecordEnv<'_>,
    initialized: &mut [bool],
) {
    let LayerSlotKind::Offscreen(pool_idx) = popped.kind else {
        return;
    };
    if initialized[1 + pool_idx] {
        if let Some(inst) = SdfQuadInstance::composite_layer(
            popped.clip,
            popped.clip_radius,
            popped.clip_radii,
            popped.local_clip,
            popped.drawn_bounds,
            popped.opacity,
            env.sf,
        ) {
            segment.push_sdf(
                parent_tid,
                initialized,
                env.clear_color,
                inst,
                AuxBinding::Layer(pool_idx),
            );
        }
    }
}

fn unpack_styled_box_spec(
    prim: &DrawPrimitive,
) -> Option<(BoxInstanceSpec, Option<BoxShadow>, f32)> {
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
            border_color,
            border_width,
            border_radius,
            corner_radii,
            box_shadow,
            backdrop_blur_px,
        } => Some((
            BoxInstanceSpec {
                rect: *rect,
                clip: *clip,
                clip_radius: *clip_radius,
                clip_radii: *clip_radii,
                transform: *transform,
                opacity: *opacity,
                background_color: *background_color,
                background_gradient: *background_gradient,
                radial_gradient: *radial_gradient,
                border_color: *border_color,
                border_width: *border_width,
                border_radius: *border_radius,
                corner_radii: *corner_radii,
            },
            *box_shadow,
            *backdrop_blur_px,
        )),
        _ => None,
    }
}
