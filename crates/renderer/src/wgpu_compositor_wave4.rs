//! Wave 4 `wgpu` helpers: multi-stop/conic gradient LUT binding, stencil clip masks, and 3D perspective layers.

use crate::gpu_context::GpuContext;
use crate::gradient_lut_cache::GradientLutCache;
use crate::sdf_instance::{
    encode_clip_geom, BoxInstanceSpec, SdfQuadInstance, SurfaceFrameTransform, KIND_SAMPLE_AUX_TEX,
};
use crate::tessellate::GpuVertex;
use crate::tessellate_aa::AA_TOLERANCE_PHYS_PX;
use crate::tessellate_primitives::{build_custom_lyon_path, to_ndc};
use crate::wgpu_compositor::{
    ActiveLayer, AuxBinding, EncoderSegment, RecordEnv, SegmentOp, WgpuFrameResources,
};
use crate::wgpu_pipeline::FormatPipelines;
use krui_core::ResolvedClipPath;
use krui_layout::LayoutRect;
use krui_style::{compute_projective_quad, Affine2, ClipFillRule, Color, CssFilterEffect, Mat4};
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
};
use std::ops::Range;

fn resolve_box_lut_aux(
    segment: &mut EncoderSegment,
    luts: &mut GradientLutCache,
    spec: &BoxInstanceSpec,
    env: &RecordEnv<'_>,
) -> AuxBinding {
    match crate::sdf_instance::box_gradient_lut(spec) {
        Some((stops, span)) => {
            let bg = luts.get_or_upload(env.ctx, stops, span);
            AuxBinding::Custom(segment.intern_aux_bg(bg))
        }
        None => AuxBinding::Dummy,
    }
}

fn push_spec_box(
    segment: &mut EncoderSegment,
    luts: &mut GradientLutCache,
    target_id: usize,
    initialized: &mut [bool],
    spec: &BoxInstanceSpec,
    env: &RecordEnv<'_>,
) {
    if let Some(inst) = SdfQuadInstance::styled_box(spec, env.sf) {
        let aux = resolve_box_lut_aux(segment, luts, spec, env);
        segment.push_sdf(target_id, initialized, env.clear_color, inst, aux);
    }
}

/// Emits single or stacked linear/radial/conic gradients onto `segment` with cached 256x1 LUTs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_box_gradient_stack(
    segment: &mut EncoderSegment,
    luts: &mut GradientLutCache,
    target_id: usize,
    initialized: &mut [bool],
    spec: &BoxInstanceSpec,
    env: &RecordEnv<'_>,
) {
    let linear_plus_other = spec.background_gradient.is_some()
        && (spec.radial_gradient.is_some() || spec.conic_gradient.is_some());
    if linear_plus_other {
        let mut base = *spec;
        base.border_width = 0.0;
        base.radial_gradient = None;
        base.conic_gradient = None;
        push_spec_box(segment, luts, target_id, initialized, &base, env);
        if let Some(rg) = spec.radial_gradient {
            let mut rad = *spec;
            rad.background_color = Color::TRANSPARENT;
            rad.background_gradient = None;
            rad.conic_gradient = None;
            rad.radial_gradient = Some(rg);
            if spec.conic_gradient.is_some() {
                rad.border_width = 0.0;
            }
            push_spec_box(segment, luts, target_id, initialized, &rad, env);
        }
        if let Some(cg) = spec.conic_gradient {
            let mut con = *spec;
            con.background_color = Color::TRANSPARENT;
            con.background_gradient = None;
            con.radial_gradient = None;
            con.conic_gradient = Some(cg);
            push_spec_box(segment, luts, target_id, initialized, &con, env);
        }
        return;
    }
    if spec.radial_gradient.is_some() && spec.conic_gradient.is_some() {
        let mut rad = *spec;
        rad.border_width = 0.0;
        rad.conic_gradient = None;
        push_spec_box(segment, luts, target_id, initialized, &rad, env);
        if let Some(cg) = spec.conic_gradient {
            let mut con = *spec;
            con.background_color = Color::TRANSPARENT;
            con.background_gradient = None;
            con.radial_gradient = None;
            con.conic_gradient = Some(cg);
            push_spec_box(segment, luts, target_id, initialized, &con, env);
        }
        return;
    }
    push_spec_box(segment, luts, target_id, initialized, spec, env);
}

/// Ensures `stencil_target` matches `(vw, vh)` whenever a `StencilClipComposite` op is queued.
pub(crate) fn ensure_stencil_target_if_needed(
    res: &mut WgpuFrameResources,
    ctx: &GpuContext,
    (vw, vh): (u32, u32),
) {
    let needs = res
        .segment
        .ops
        .iter()
        .any(|op| matches!(op, SegmentOp::StencilClipComposite { .. }));
    if !needs
        || res
            .stencil_target
            .as_ref()
            .is_some_and(|(sz, _, _)| *sz == (vw, vh))
    {
        return;
    }
    let tex = ctx.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("KRui Stencil Clip Depth24PlusStencil8"),
        size: wgpu::Extent3d {
            width: vw.max(1),
            height: vh.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24PlusStencil8,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    res.stencil_target = Some(((vw, vh), tex, view));
}

/// Tessellates `cp` into `segment` and returns `(base_vertex, index_range)`.
pub(crate) fn tessellate_clip_path_mask(
    segment: &mut EncoderSegment,
    cp: &ResolvedClipPath,
    sf: SurfaceFrameTransform,
) -> Option<(i32, Range<u32>)> {
    let phys_tx = Affine2::scale(sf.surface_scale, sf.surface_scale) * cp.transform;
    let lyon_path = build_custom_lyon_path(&cp.segments, phys_tx);
    let mut tmp: VertexBuffers<GpuVertex, u32> = VertexBuffers::new();
    let mut fill_tess = FillTessellator::new();
    let rule = match cp.fill_rule {
        ClipFillRule::NonZero => FillRule::NonZero,
        ClipFillRule::EvenOdd => FillRule::EvenOdd,
    };
    let opts = FillOptions::tolerance(AA_TOLERANCE_PHYS_PX).with_fill_rule(rule);
    fill_tess
        .tessellate_path(
            &lyon_path,
            &opts,
            &mut BuffersBuilder::new(&mut tmp, |v: FillVertex| {
                let p = v.position();
                GpuVertex {
                    position: to_ndc(p.x, p.y, sf.phys_w, sf.phys_h),
                    uv: [0.0, 0.0],
                    color: [1.0; 4],
                }
            }),
        )
        .ok()?;
    if tmp.indices.is_empty() {
        return None;
    }
    let base_v = segment.canvas_verts.len() as i32;
    let start_i = segment.canvas_indices.len() as u32;
    segment.canvas_verts.extend(tmp.vertices);
    segment.canvas_indices.extend(tmp.indices);
    Some((base_v, start_i..segment.canvas_indices.len() as u32))
}

/// Builds a non-affine projective `KIND_SAMPLE_AUX_TEX` composite quad for `perspective` layers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_perspective_composite_instance(
    clip: Option<LayoutRect>,
    clip_radius: f32,
    clip_radii_raw: [f32; 4],
    (src_rect, mat4): (LayoutRect, Mat4),
    opacity: f32,
    filter: CssFilterEffect,
    sf: SurfaceFrameTransform,
) -> Option<SdfQuadInstance> {
    let s = sf.surface_scale;
    let inv_s = 1.0 / s;
    let phys_mat = Mat4::scale(s, s, s) * mat4 * Mat4::scale(inv_s, inv_s, inv_s);
    let (rx, ry, rw, rh) = (
        src_rect.x * s,
        src_rect.y * s,
        src_rect.width * s,
        src_rect.height * s,
    );
    let quad = compute_projective_quad(phys_mat, rx, ry, rw, rh)?;
    let (clip_geom, clip_radii, _) = encode_clip_geom(clip, clip_radius, clip_radii_raw, s)?;
    let x0 = quad.screen_aabb[0].floor().clamp(0.0, sf.phys_w);
    let y0 = quad.screen_aabb[1].floor().clamp(0.0, sf.phys_h);
    let x1 = quad.screen_aabb[2].ceil().clamp(0.0, sf.phys_w);
    let y1 = quad.screen_aabb[3].ceil().clamp(0.0, sf.phys_h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let h = quad.inv_homography;
    let has_mat = filter.has_color_matrix();
    let m = filter.to_color_matrix_4x5();
    Some(SdfQuadInstance {
        screen_bounds: [x0, y0, x1, y1],
        center_and_vp: [
            0.5 * sf.phys_w,
            0.5 * sf.phys_h,
            1.0 / sf.phys_w,
            1.0 / sf.phys_h,
        ],
        inv_linear: [1.0, 0.0, 0.0, 1.0],
        box_geom: [rx + 0.5 * rw, ry + 0.5 * rh, 0.5 * rw, 0.5 * rh],
        clip_geom,
        color_a: if has_mat {
            [m[0][0], m[0][1], m[0][2], m[0][4]]
        } else {
            [1.0; 4]
        },
        color_b: if has_mat {
            [m[1][0], m[1][1], m[1][2], m[1][4]]
        } else {
            [0.0; 4]
        },
        grad_start: if has_mat {
            [m[2][0], m[2][1], m[2][2], m[2][4]]
        } else {
            [0.0; 4]
        },
        grad_end: [m[3][3], 0.0, 0.0, 0.0],
        uv_rect: [h[0], h[1], h[2], h[3]],
        shadow_params: [h[4], h[5], h[6], h[7]],
        extra_params: [
            opacity.clamp(0.0, 1.0),
            h[8],
            1.0 / sf.phys_w,
            1.0 / sf.phys_h,
        ],
        flags: [KIND_SAMPLE_AUX_TEX, u32::from(has_mat), 0, 1],
        corner_radii: [0.0; 4],
        clip_radii,
    })
}

/// Records `popped` composite onto `parent_tid` using `StencilClipComposite` if `clip_path` is set.
pub(crate) fn push_popped_composite_op(
    segment: &mut EncoderSegment,
    popped: &ActiveLayer,
    parent_tid: usize,
    env: &RecordEnv<'_>,
    initialized: &mut [bool],
    inst: SdfQuadInstance,
    aux: AuxBinding,
) {
    if let Some(cp) = popped.clip_path.as_ref() {
        if let Some((base_v, idx_range)) = tessellate_clip_path_mask(segment, cp, env.sf) {
            let clear = if !initialized[parent_tid] {
                initialized[parent_tid] = true;
                if parent_tid == 0 {
                    env.clear_color
                } else {
                    Some(Color::TRANSPARENT)
                }
            } else {
                None
            };
            let inst_idx = segment.instances.len() as u32;
            segment.instances.push(inst);
            segment.ops.push(SegmentOp::StencilClipComposite {
                target_id: parent_tid,
                load_clear: clear,
                mask_index_range: idx_range,
                mask_base_vertex: base_v,
                sdf_inst_idx: inst_idx,
                aux,
            });
            return;
        }
    }
    segment.push_sdf(parent_tid, initialized, env.clear_color, inst, aux);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_stencil_clip_composite(
    encoder: &mut wgpu::CommandEncoder,
    res: &WgpuFrameResources,
    ctx: &GpuContext,
    pipes: &FormatPipelines,
    view: &wgpu::TextureView,
    (load_clear, mask_idx_range, mask_base_v, sdf_idx, aux_bg): (
        Option<Color>,
        Range<u32>,
        i32,
        u32,
        &wgpu::BindGroup,
    ),
    (target_is_srgb, vw, vh): (bool, u32, u32),
    clear_fn: impl Fn(Color, bool) -> wgpu::Color,
) {
    let Some((_, _, stencil_view)) = res.stencil_target.as_ref() else {
        return;
    };
    let load = match load_clear {
        Some(c) => wgpu::LoadOp::Clear(clear_fn(c, target_is_srgb)),
        None => wgpu::LoadOp::Load,
    };
    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("KRui StencilClipComposite"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: stencil_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Discard,
            }),
            stencil_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(0),
                store: wgpu::StoreOp::Discard,
            }),
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    draw_stencil_mask_and_quad(
        &mut rpass,
        res,
        ctx,
        pipes,
        (mask_idx_range, mask_base_v, sdf_idx, aux_bg),
        (vw, vh),
    );
}

fn draw_stencil_mask_and_quad<'a>(
    rpass: &mut wgpu::RenderPass<'a>,
    res: &'a WgpuFrameResources,
    ctx: &'a GpuContext,
    pipes: &'a FormatPipelines,
    (mask_idx_range, mask_base_v, sdf_idx, aux_bg): (Range<u32>, i32, u32, &'a wgpu::BindGroup),
    (vw, vh): (u32, u32),
) {
    let sdf_bytes = (res.segment.instances.len() * std::mem::size_of::<SdfQuadInstance>()) as u64;
    let v_bytes = (res.segment.canvas_verts.len() * std::mem::size_of::<GpuVertex>()) as u64;
    let i_bytes = (res.segment.canvas_indices.len() * std::mem::size_of::<u32>()) as u64;
    rpass.set_scissor_rect(0, 0, vw, vh);
    rpass.set_pipeline(&pipes.stencil_write_pipeline);
    rpass.set_stencil_reference(1);
    rpass.set_bind_group(0, &ctx.atlas_bind_group, &[]);
    rpass.set_bind_group(1, &ctx.dummy_aux_bg, &[]);
    rpass.set_vertex_buffer(0, res.canvas_vbuf.slice(0..v_bytes));
    rpass.set_index_buffer(res.canvas_ibuf.slice(0..i_bytes), wgpu::IndexFormat::Uint32);
    rpass.draw_indexed(mask_idx_range, mask_base_v, 0..1);

    rpass.set_pipeline(&pipes.sdf_stencil_test_pipeline);
    rpass.set_stencil_reference(1);
    rpass.set_bind_group(0, &ctx.atlas_bind_group, &[]);
    rpass.set_bind_group(1, aux_bg, &[]);
    rpass.set_vertex_buffer(0, res.sdf_buf.slice(0..sdf_bytes));
    rpass.draw(0..6, sdf_idx..sdf_idx + 1);
}
