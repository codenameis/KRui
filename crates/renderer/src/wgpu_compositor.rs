//! Compositor frame resources, segment recording, and `PushLayer`/`PopLayer` stack.

use crate::blur_pass::{BlurChainTargets, BlurStep, LayerTargetPool, OffscreenLayerTarget};
use crate::canvas_cache::CanvasMeshCache;
use crate::gpu_context::GpuContext;
use crate::sdf_instance::{SdfQuadInstance, SurfaceFrameTransform};
use crate::tessellate::GpuVertex;
use crate::wgpu_compositor_exec::submit_recorded_segment;
use crate::wgpu_pipeline::{create_sampleable_target, create_sdf_and_canvas_buffers};
use krui_core::{
    ActiveBackendKind, DrawPrimitive, FrameRenderStats, GpuResourceLimits, PaintScene,
};
use krui_layout::LayoutRect;
use krui_style::{Affine2, Color};
use std::ops::Range;
use std::sync::Arc;

const INITIAL_SDF_CAP: usize = 1024;
const INITIAL_CANVAS_V_CAP: usize = 2048;
const INITIAL_CANVAS_I_CAP: usize = 6144;
pub(crate) const MAX_LAYER_POOL_DEPTH: usize = 16;
const IDLE_LAYER_POOL_CAP: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AuxBinding {
    Dummy,
    BlurredHalfB,
    Layer(usize),
    Custom(usize),
}

pub(crate) enum PassDrawItem {
    SdfBatch {
        range: Range<u32>,
        aux: AuxBinding,
    },
    Canvas {
        index_range: Range<u32>,
        base_vertex: i32,
        scissor: Option<[u32; 4]>,
    },
}

pub(crate) enum SegmentOp {
    DrawPass {
        target_id: usize,
        load_clear: Option<Color>,
        items: Vec<PassDrawItem>,
    },
    BlurChain {
        steps: Vec<BlurStep>,
    },
}

#[derive(Default)]
pub(crate) struct EncoderSegment {
    pub instances: Vec<SdfQuadInstance>,
    pub canvas_verts: Vec<GpuVertex>,
    pub canvas_indices: Vec<u32>,
    pub blur_uniform_bytes: Vec<u8>,
    pub blur_count: usize,
    pub aux_bind_groups: Vec<Arc<wgpu::BindGroup>>,
    pub ops: Vec<SegmentOp>,
}

impl EncoderSegment {
    pub(crate) fn clear(&mut self) {
        self.instances.clear();
        self.canvas_verts.clear();
        self.canvas_indices.clear();
        self.blur_uniform_bytes.clear();
        self.blur_count = 0;
        self.aux_bind_groups.clear();
        self.ops.clear();
    }

    pub(crate) fn intern_aux_bg(&mut self, bg: Arc<wgpu::BindGroup>) -> usize {
        if let Some(pos) = self
            .aux_bind_groups
            .iter()
            .position(|existing| Arc::ptr_eq(existing, &bg))
        {
            return pos;
        }
        let idx = self.aux_bind_groups.len();
        self.aux_bind_groups.push(bg);
        idx
    }

    pub(crate) fn ensure_draw_pass(
        &mut self,
        target_id: usize,
        initialized: &mut [bool],
        root_clear: Option<Color>,
    ) -> &mut Vec<PassDrawItem> {
        let needs_new = !matches!(
            self.ops.last(),
            Some(SegmentOp::DrawPass { target_id: tid, .. }) if *tid == target_id
        );
        if needs_new {
            let clear = if !initialized[target_id] {
                initialized[target_id] = true;
                if target_id == 0 {
                    root_clear
                } else {
                    Some(Color::TRANSPARENT)
                }
            } else {
                None
            };
            self.ops.push(SegmentOp::DrawPass {
                target_id,
                load_clear: clear,
                items: Vec::with_capacity(4),
            });
        }
        match self.ops.last_mut() {
            Some(SegmentOp::DrawPass { items, .. }) => items,
            _ => unreachable!(),
        }
    }

    pub(crate) fn push_sdf(
        &mut self,
        target_id: usize,
        initialized: &mut [bool],
        root_clear: Option<Color>,
        inst: SdfQuadInstance,
        aux: AuxBinding,
    ) {
        let idx = self.instances.len() as u32;
        self.instances.push(inst);
        let items = self.ensure_draw_pass(target_id, initialized, root_clear);
        if let Some(PassDrawItem::SdfBatch { range, aux: prev }) = items.last_mut() {
            if *prev == aux && range.end == idx {
                range.end = idx + 1;
                return;
            }
        }
        items.push(PassDrawItem::SdfBatch {
            range: idx..idx + 1,
            aux,
        });
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum LayerSlotKind {
    Offscreen(usize),
    Overflow { prev_inline_opacity: f32 },
}

pub(crate) struct ActiveLayer {
    pub kind: LayerSlotKind,
    pub clip: Option<LayoutRect>,
    pub clip_radius: f32,
    pub clip_radii: [f32; 4],
    pub local_clip: Option<(LayoutRect, [f32; 4], Affine2)>,
    pub opacity: f32,
    pub drawn_bounds: Option<[f32; 4]>,
}

pub(crate) struct RecordCounters {
    pub clamped: usize,
    pub verts: usize,
    pub indices: usize,
}

impl RecordCounters {
    pub fn try_reserve(&mut self, v: usize, i: usize, limits: &GpuResourceLimits) -> bool {
        if self.verts + v > limits.max_vertices_per_frame
            || self.indices + i > limits.max_indices_per_frame
        {
            self.clamped += 1;
            false
        } else {
            self.verts += v;
            self.indices += i;
            true
        }
    }
}

pub(crate) struct RecordEnv<'a> {
    pub ctx: &'a GpuContext,
    pub clear_color: Option<Color>,
    pub sf: SurfaceFrameTransform,
    pub vp_size: (u32, u32),
    pub out_view: &'a wgpu::TextureView,
    pub out_format: wgpu::TextureFormat,
    pub use_swapchain_scene: bool,
    pub limits: GpuResourceLimits,
}

/// Reusable per-surface GPU buffers, `<canvas>` LRU cache, blur chain, and `PushLayer` pool.
pub(crate) struct WgpuFrameResources {
    pub sdf_buf: wgpu::Buffer,
    pub sdf_cap: usize,
    pub canvas_vbuf: wgpu::Buffer,
    pub canvas_v_cap: usize,
    pub canvas_ibuf: wgpu::Buffer,
    pub canvas_i_cap: usize,
    pub canvas_cache: CanvasMeshCache,
    pub blur_targets: BlurChainTargets,
    pub layer_pool: LayerTargetPool,
    pub root_blur_src_bg: Option<(u64, wgpu::BindGroup)>,
    pub swapchain_scene: Option<OffscreenLayerTarget>,
    pub(crate) segment: EncoderSegment,
}

impl WgpuFrameResources {
    pub fn new(device: &wgpu::Device) -> Self {
        let (sdf_buf, canvas_vbuf, canvas_ibuf) = create_sdf_and_canvas_buffers(
            device,
            INITIAL_SDF_CAP,
            INITIAL_CANVAS_V_CAP,
            INITIAL_CANVAS_I_CAP,
        );
        Self {
            sdf_buf,
            sdf_cap: INITIAL_SDF_CAP,
            canvas_vbuf,
            canvas_v_cap: INITIAL_CANVAS_V_CAP,
            canvas_ibuf,
            canvas_i_cap: INITIAL_CANVAS_I_CAP,
            canvas_cache: CanvasMeshCache::new(),
            blur_targets: BlurChainTargets::default(),
            layer_pool: LayerTargetPool::default(),
            root_blur_src_bg: None,
            swapchain_scene: None,
            segment: EncoderSegment::default(),
        }
    }

    pub fn invalidate_root_blur_bg(&mut self) {
        self.root_blur_src_bg = None;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn composite_scene(
        &mut self,
        ctx: &GpuContext,
        scene: &PaintScene,
        out_view: &wgpu::TextureView,
        out_format: wgpu::TextureFormat,
        out_is_sampleable: bool,
        vp_size: (u32, u32),
        surface_scale: f32,
        limits: GpuResourceLimits,
    ) -> FrameRenderStats {
        self.composite_scene_with_load(
            ctx,
            scene,
            out_view,
            out_format,
            out_is_sampleable,
            true,
            vp_size,
            surface_scale,
            limits,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn composite_scene_with_load(
        &mut self,
        ctx: &GpuContext,
        scene: &PaintScene,
        out_view: &wgpu::TextureView,
        out_format: wgpu::TextureFormat,
        out_is_sampleable: bool,
        clear_root: bool,
        vp_size: (u32, u32),
        surface_scale: f32,
        limits: GpuResourceLimits,
    ) -> FrameRenderStats {
        let limits = limits.normalized();
        let (max_cmds, max_depth, use_sc) = self.prepare_scene_targets(
            ctx,
            scene,
            out_format,
            out_is_sampleable,
            clear_root,
            vp_size,
            surface_scale,
            &limits,
        );
        let root_clear = (clear_root || use_sc).then_some(scene.clear_color);
        let env = make_record_env(
            ctx,
            scene,
            root_clear,
            out_view,
            out_format,
            vp_size,
            surface_scale,
            use_sc,
            limits,
        );
        let skipped = scene.primitives.len().saturating_sub(max_cmds);
        let mut initialized = vec![false; 1 + max_depth];
        let mut stats = RecordCounters {
            clamped: skipped,
            verts: 0,
            indices: 0,
        };
        self.segment.clear();
        self.record_primitives(
            &scene.primitives[..max_cmds],
            &env,
            &mut initialized,
            &mut stats,
        );
        if !initialized[0] && root_clear.is_some() {
            let _ = self
                .segment
                .ensure_draw_pass(0, &mut initialized, root_clear);
        }
        submit_recorded_segment(self, ctx, out_view, out_format, vp_size, use_sc, true);
        self.layer_pool.trim_idle(IDLE_LAYER_POOL_CAP);
        build_wgpu_frame_stats(max_cmds, skipped, &stats)
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_scene_targets(
        &mut self,
        ctx: &GpuContext,
        scene: &PaintScene,
        out_format: wgpu::TextureFormat,
        out_is_sampleable: bool,
        clear_root: bool,
        (vw, vh): (u32, u32),
        surface_scale: f32,
        limits: &GpuResourceLimits,
    ) -> (usize, usize, bool) {
        ctx.atlas
            .lock()
            .expect("atlas mutex")
            .packer
            .advance_epoch();
        let max_cmds = scene
            .primitives
            .len()
            .min(limits.max_draw_commands_per_frame);
        let prims = &scene.primitives[..max_cmds];
        let max_depth = (max_layer_nesting(prims)
            + usize::from(scene_has_rounded_canvas_clip(prims, surface_scale)))
        .min(MAX_LAYER_POOL_DEPTH);
        self.layer_pool.ensure_count(
            ctx.device(),
            &ctx.aux_bgl,
            max_depth,
            (vw, vh),
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let use_swapchain_scene = clear_root
            && scene_uses_backdrop_blur(prims)
            && (!out_is_sampleable || out_format.is_srgb());
        if use_swapchain_scene {
            ensure_swapchain_scene(
                &mut self.swapchain_scene,
                ctx,
                (vw, vh),
                wgpu::TextureFormat::Rgba8Unorm,
            );
        }
        (max_cmds, max_depth, use_swapchain_scene)
    }
}

#[allow(clippy::too_many_arguments)]
fn make_record_env<'a>(
    ctx: &'a GpuContext,
    scene: &PaintScene,
    clear_color: Option<Color>,
    out_view: &'a wgpu::TextureView,
    out_format: wgpu::TextureFormat,
    (vw, vh): (u32, u32),
    surface_scale: f32,
    use_swapchain_scene: bool,
    limits: GpuResourceLimits,
) -> RecordEnv<'a> {
    RecordEnv {
        ctx,
        clear_color,
        sf: SurfaceFrameTransform {
            surface_scale,
            glyph_raster_scale: scene.scale_factor.clamp(1.0, 4.0),
            phys_w: vw as f32,
            phys_h: vh as f32,
        },
        vp_size: (vw, vh),
        out_view,
        out_format,
        use_swapchain_scene,
        limits,
    }
}

fn scene_uses_backdrop_blur(prims: &[DrawPrimitive]) -> bool {
    prims.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox { backdrop_blur_px, opacity, .. }
                if *backdrop_blur_px > 0.5 && *opacity > 0.0
        )
    })
}

fn scene_has_rounded_canvas_clip(prims: &[DrawPrimitive], surface_scale: f32) -> bool {
    prims.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::CanvasBatch { clip: Some(_), clip_radius, clip_radii, opacity, .. }
                if (clip_radii.iter().copied().fold(*clip_radius, f32::max) * surface_scale) > 0.5
                    && *opacity > 0.0
        )
    })
}

fn max_layer_nesting(prims: &[DrawPrimitive]) -> usize {
    let (mut cur, mut max_d) = (0_usize, 0_usize);
    for p in prims {
        match p {
            DrawPrimitive::PushLayer { .. } => {
                cur += 1;
                max_d = max_d.max(cur);
            }
            DrawPrimitive::PopLayer => cur = cur.saturating_sub(1),
            _ => {}
        }
    }
    max_d.min(MAX_LAYER_POOL_DEPTH)
}

fn ensure_swapchain_scene(
    slot: &mut Option<OffscreenLayerTarget>,
    ctx: &GpuContext,
    size: (u32, u32),
    format: wgpu::TextureFormat,
) {
    if slot
        .as_ref()
        .is_none_or(|s| s.size != size || s.format != format)
    {
        let (tex, view, aux_bg) = create_sampleable_target(
            ctx.device(),
            &ctx.aux_bgl,
            size.0,
            size.1,
            format,
            wgpu::TextureUsages::empty(),
        );
        *slot = Some(OffscreenLayerTarget {
            size,
            format,
            _tex: tex,
            view,
            aux_bg,
            blur_src_bg: None,
        });
    }
}

fn build_wgpu_frame_stats(
    max_cmds: usize,
    skipped: usize,
    stats: &RecordCounters,
) -> FrameRenderStats {
    FrameRenderStats {
        backend_kind: ActiveBackendKind::Wgpu,
        draw_primitives_submitted: max_cmds.saturating_sub(stats.clamped.saturating_sub(skipped)),
        draw_primitives_clamped: stats.clamped,
        vertices_generated: stats.verts,
        indices_generated: stats.indices,
    }
}
