//! GPU command encoder submission for `WgpuFrameResources` recorded segments.

use crate::blur_pass::{AllocatedBlurChain, BlurSourceKind, BlurStep, BlurTargetKind};
use crate::gpu_context::{GpuContext, SharedGpuAtlas};
use crate::sdf_instance::{SdfQuadInstance, SurfaceFrameTransform};
use crate::tessellate::GpuVertex;
use crate::wgpu_compositor::{AuxBinding, PassDrawItem, SegmentOp, WgpuFrameResources};
use crate::wgpu_pipeline::{create_sdf_and_canvas_buffers, FormatPipelines};
use krui_style::Color;

pub(crate) fn submit_recorded_segment(
    res: &mut WgpuFrameResources,
    ctx: &GpuContext,
    out_view: &wgpu::TextureView,
    out_format: wgpu::TextureFormat,
    (vw, vh): (u32, u32),
    use_swapchain_scene: bool,
    is_final_segment: bool,
) {
    if res.segment.ops.is_empty() && !(is_final_segment && use_swapchain_scene) {
        return;
    }
    let final_blit_idx = (is_final_segment && use_swapchain_scene)
        .then(|| push_swapchain_blit_instance(res, vw, vh))
        .flatten();
    upload_atlas_and_geometry_buffers(res, ctx);
    prepare_blur_resources(res, ctx, out_view, (vw, vh), use_swapchain_scene);
    crate::wgpu_compositor_wave4::ensure_stencil_target_if_needed(res, ctx, (vw, vh));

    let out_pipes = ctx.pipelines_for(out_format);
    let layer_pipes = ctx.pipelines_for(wgpu::TextureFormat::Rgba8Unorm);
    let desc = wgpu::CommandEncoderDescriptor {
        label: Some("KRui Phase4 Compositor Encoder"),
    };
    let mut encoder = ctx.device().create_command_encoder(&desc);
    for op_idx in 0..res.segment.ops.len() {
        encode_segment_op(
            &mut encoder,
            res,
            ctx,
            (&out_pipes, &layer_pipes),
            out_view,
            out_format,
            (vw, vh),
            use_swapchain_scene,
            op_idx,
        );
    }
    if let Some(blit_idx) = final_blit_idx {
        encode_swapchain_blit(&mut encoder, res, ctx, &out_pipes, out_view, blit_idx);
    }
    ctx.queue().submit(std::iter::once(encoder.finish()));
    // `out_view` may belong to a transient swapchain `SurfaceTexture` destroyed on `present()`;
    // never retain its bind group across frames.
    res.root_blur_src_bg = None;
}

fn push_swapchain_blit_instance(res: &mut WgpuFrameResources, vw: u32, vh: u32) -> Option<u32> {
    let sf = SurfaceFrameTransform {
        surface_scale: 1.0,
        glyph_raster_scale: 1.0,
        phys_w: vw as f32,
        phys_h: vh as f32,
    };
    SdfQuadInstance::composite_layer(
        None,
        0.0,
        [0.0; 4],
        None,
        Some([0.0, 0.0, sf.phys_w, sf.phys_h]),
        1.0,
        krui_style::CssFilterEffect::IDENTITY,
        sf,
    )
    .map(|inst| {
        let idx = res.segment.instances.len() as u32;
        res.segment.instances.push(inst);
        idx
    })
}

fn upload_atlas_and_geometry_buffers(res: &mut WgpuFrameResources, ctx: &GpuContext) {
    {
        let mut guard = ctx.atlas.lock().expect("atlas mutex");
        let SharedGpuAtlas {
            packer,
            mask_tex,
            color_tex,
            ..
        } = &mut *guard;
        packer.upload_dirty_pages(ctx.queue(), mask_tex, color_tex);
    }
    let need_sdf = res.segment.instances.len();
    let need_v = res.segment.canvas_verts.len();
    let need_i = res.segment.canvas_indices.len();
    if need_sdf > res.sdf_cap || need_v > res.canvas_v_cap || need_i > res.canvas_i_cap {
        res.sdf_cap = res.sdf_cap.max(need_sdf.next_power_of_two()).max(64);
        res.canvas_v_cap = res.canvas_v_cap.max(need_v.next_power_of_two()).max(64);
        res.canvas_i_cap = res.canvas_i_cap.max(need_i.next_power_of_two()).max(64);
        let (s, v, i) = create_sdf_and_canvas_buffers(
            ctx.device(),
            res.sdf_cap,
            res.canvas_v_cap,
            res.canvas_i_cap,
        );
        res.sdf_buf = s;
        res.canvas_vbuf = v;
        res.canvas_ibuf = i;
    }
    if !res.segment.instances.is_empty() {
        ctx.queue().write_buffer(
            &res.sdf_buf,
            0,
            bytemuck::cast_slice(&res.segment.instances),
        );
    }
    if !res.segment.canvas_verts.is_empty() && !res.segment.canvas_indices.is_empty() {
        ctx.queue().write_buffer(
            &res.canvas_vbuf,
            0,
            bytemuck::cast_slice(&res.segment.canvas_verts),
        );
        ctx.queue().write_buffer(
            &res.canvas_ibuf,
            0,
            bytemuck::cast_slice(&res.segment.canvas_indices),
        );
    }
}

fn prepare_blur_resources(
    res: &mut WgpuFrameResources,
    ctx: &GpuContext,
    out_view: &wgpu::TextureView,
    (vw, vh): (u32, u32),
    use_swapchain_scene: bool,
) {
    if res.segment.blur_count == 0 {
        return;
    }
    let chain = res.blur_targets.ensure(
        ctx.device(),
        &ctx.blur,
        &ctx.aux_bgl,
        (vw, vh),
        res.segment.blur_count,
    );
    ctx.queue()
        .write_buffer(&chain.uniform_buf, 0, &res.segment.blur_uniform_bytes);
    let epoch = chain.uniform_buf_epoch;
    if use_swapchain_scene {
        if let Some(sc) = &mut res.swapchain_scene {
            if sc.blur_src_bg.as_ref().map(|(e, _)| *e) != Some(epoch) {
                let bg =
                    ctx.blur
                        .create_source_bind_group(ctx.device(), &sc.view, &chain.uniform_buf);
                sc.blur_src_bg = Some((epoch, bg));
            }
        }
    } else {
        let bg = ctx
            .blur
            .create_source_bind_group(ctx.device(), out_view, &chain.uniform_buf);
        res.root_blur_src_bg = Some((epoch, bg));
    }
    for layer in &mut res.layer_pool.layers {
        if layer.blur_src_bg.as_ref().map(|(e, _)| *e) != Some(epoch) {
            let bg =
                ctx.blur
                    .create_source_bind_group(ctx.device(), &layer.view, &chain.uniform_buf);
            layer.blur_src_bg = Some((epoch, bg));
        }
    }
}

fn resolve_segment_target<'a>(
    res: &'a WgpuFrameResources,
    out_view: &'a wgpu::TextureView,
    out_format: wgpu::TextureFormat,
    (out_pipes, layer_pipes): (&'a FormatPipelines, &'a FormatPipelines),
    target_id: usize,
    use_swapchain_scene: bool,
) -> (&'a wgpu::TextureView, &'a FormatPipelines, bool) {
    let direct_root = target_id == 0 && !use_swapchain_scene;
    let view = if direct_root {
        out_view
    } else if target_id == 0 {
        &res.swapchain_scene.as_ref().expect("swapchain scene").view
    } else {
        &res.layer_pool.layers[target_id - 1].view
    };
    let pipes = if direct_root { out_pipes } else { layer_pipes };
    (view, pipes, direct_root && out_format.is_srgb())
}

#[allow(clippy::too_many_arguments)]
fn encode_segment_op(
    encoder: &mut wgpu::CommandEncoder,
    res: &WgpuFrameResources,
    ctx: &GpuContext,
    pipes: (&FormatPipelines, &FormatPipelines),
    out_view: &wgpu::TextureView,
    out_format: wgpu::TextureFormat,
    (vw, vh): (u32, u32),
    use_swapchain_scene: bool,
    op_idx: usize,
) {
    match &res.segment.ops[op_idx] {
        SegmentOp::DrawPass {
            target_id,
            load_clear,
            items,
        } => {
            let (view, pass_pipes, target_srgb) = resolve_segment_target(
                res,
                out_view,
                out_format,
                pipes,
                *target_id,
                use_swapchain_scene,
            );
            encode_draw_pass(
                encoder,
                res,
                ctx,
                pass_pipes,
                view,
                *load_clear,
                target_srgb,
                items,
                (vw, vh),
            );
        }
        SegmentOp::BlurChain { steps } => {
            let chain = res.blur_targets.chain.as_ref().expect("blur chain");
            for step in steps {
                encode_blur_step(encoder, res, ctx, chain, step, use_swapchain_scene);
            }
        }
        SegmentOp::StencilClipComposite {
            target_id,
            load_clear,
            mask_index_range,
            mask_base_vertex,
            sdf_inst_idx,
            aux,
        } => {
            let (view, pass_pipes, target_srgb) = resolve_segment_target(
                res,
                out_view,
                out_format,
                pipes,
                *target_id,
                use_swapchain_scene,
            );
            let aux_bg = resolve_aux_bg(res, ctx, *aux);
            crate::wgpu_compositor_wave4::encode_stencil_clip_composite(
                encoder,
                res,
                ctx,
                pass_pipes,
                view,
                (
                    *load_clear,
                    mask_index_range.clone(),
                    *mask_base_vertex,
                    *sdf_inst_idx,
                    aux_bg,
                ),
                (target_srgb, vw, vh),
                premul_clear_color,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_draw_pass(
    encoder: &mut wgpu::CommandEncoder,
    res: &WgpuFrameResources,
    ctx: &GpuContext,
    pipes: &FormatPipelines,
    view: &wgpu::TextureView,
    load_clear: Option<Color>,
    target_is_srgb: bool,
    items: &[PassDrawItem],
    (vw, vh): (u32, u32),
) {
    let load = match load_clear {
        Some(c) => wgpu::LoadOp::Clear(premul_clear_color(c, target_is_srgb)),
        None => wgpu::LoadOp::Load,
    };
    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("KRui DrawPass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    let sizes = (
        (res.segment.instances.len() * std::mem::size_of::<SdfQuadInstance>()) as u64,
        (res.segment.canvas_verts.len() * std::mem::size_of::<GpuVertex>()) as u64,
        (res.segment.canvas_indices.len() * std::mem::size_of::<u32>()) as u64,
    );
    for item in items {
        encode_pass_draw_item(&mut rpass, item, res, ctx, pipes, sizes, (vw, vh));
    }
}

fn encode_pass_draw_item<'a>(
    rpass: &mut wgpu::RenderPass<'a>,
    item: &PassDrawItem,
    res: &'a WgpuFrameResources,
    ctx: &'a GpuContext,
    pipes: &'a FormatPipelines,
    (sdf_bytes, v_bytes, i_bytes): (u64, u64, u64),
    (vw, vh): (u32, u32),
) {
    match item {
        PassDrawItem::SdfBatch { range, aux } => {
            let aux_bg = resolve_aux_bg(res, ctx, *aux);
            rpass.set_pipeline(&pipes.sdf_pipeline);
            rpass.set_bind_group(0, &ctx.atlas_bind_group, &[]);
            rpass.set_bind_group(1, aux_bg, &[]);
            rpass.set_vertex_buffer(0, res.sdf_buf.slice(0..sdf_bytes));
            rpass.set_scissor_rect(0, 0, vw, vh);
            rpass.draw(0..6, range.clone());
        }
        PassDrawItem::Canvas {
            index_range,
            base_vertex,
            scissor,
        } => {
            let Some([sx, sy, sw, sh]) = clamp_scissor(*scissor, vw, vh) else {
                return;
            };
            rpass.set_pipeline(&pipes.canvas_pipeline);
            rpass.set_bind_group(0, &ctx.atlas_bind_group, &[]);
            rpass.set_bind_group(1, &ctx.dummy_aux_bg, &[]);
            rpass.set_vertex_buffer(0, res.canvas_vbuf.slice(0..v_bytes));
            rpass.set_index_buffer(res.canvas_ibuf.slice(0..i_bytes), wgpu::IndexFormat::Uint32);
            rpass.set_scissor_rect(sx, sy, sw, sh);
            rpass.draw_indexed(index_range.clone(), *base_vertex, 0..1);
        }
    }
}

fn encode_blur_step(
    encoder: &mut wgpu::CommandEncoder,
    res: &WgpuFrameResources,
    ctx: &GpuContext,
    chain: &AllocatedBlurChain,
    step: &BlurStep,
    use_swapchain_scene: bool,
) {
    let (dst_view, (dw, dh)) = match step.dst {
        BlurTargetKind::HalfA => (
            &chain.half_a_view,
            (
                (chain.full_size.0 / 2).max(1),
                (chain.full_size.1 / 2).max(1),
            ),
        ),
        BlurTargetKind::HalfB => (
            &chain.half_b_view,
            (
                (chain.full_size.0 / 2).max(1),
                (chain.full_size.1 / 2).max(1),
            ),
        ),
        BlurTargetKind::Quarter => (
            &chain.quarter_view,
            (
                (chain.full_size.0 / 4).max(1),
                (chain.full_size.1 / 4).max(1),
            ),
        ),
    };
    let sx = (step.uv_rect[0] * dw as f32).floor().max(0.0) as u32;
    let sy = (step.uv_rect[1] * dh as f32).floor().max(0.0) as u32;
    let ex = (step.uv_rect[2] * dw as f32).ceil().min(dw as f32) as u32;
    let ey = (step.uv_rect[3] * dh as f32).ceil().min(dh as f32) as u32;
    let src_bg = resolve_blur_source_bg(res, chain, step.src, use_swapchain_scene);
    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("KRui DualKawase Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: dst_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    if ex > sx && ey > sy {
        let pipe = if step.is_upsample {
            &ctx.blur.upsample_pipeline
        } else {
            &ctx.blur.downsample_pipeline
        };
        rpass.set_pipeline(pipe);
        rpass.set_bind_group(0, src_bg, &[step.dynamic_offset]);
        rpass.set_scissor_rect(sx, sy, ex - sx, ey - sy);
        rpass.draw(0..3, 0..1);
    }
}

fn resolve_blur_source_bg<'a>(
    res: &'a WgpuFrameResources,
    chain: &'a AllocatedBlurChain,
    src: BlurSourceKind,
    use_swapchain_scene: bool,
) -> &'a wgpu::BindGroup {
    match src {
        BlurSourceKind::ActiveTarget(0) => {
            if use_swapchain_scene {
                &res.swapchain_scene
                    .as_ref()
                    .expect("swapchain scene")
                    .blur_src_bg
                    .as_ref()
                    .expect("sc blur bg")
                    .1
            } else {
                &res.root_blur_src_bg.as_ref().expect("root blur bg").1
            }
        }
        BlurSourceKind::ActiveTarget(tid) => {
            &res.layer_pool.layers[tid - 1]
                .blur_src_bg
                .as_ref()
                .expect("layer blur bg")
                .1
        }
        BlurSourceKind::HalfA => &chain.half_a_src_bg,
        BlurSourceKind::HalfB => &chain.half_b_src_bg,
        BlurSourceKind::Quarter => &chain.quarter_src_bg,
    }
}

fn encode_swapchain_blit(
    encoder: &mut wgpu::CommandEncoder,
    res: &WgpuFrameResources,
    ctx: &GpuContext,
    pipes: &FormatPipelines,
    out_view: &wgpu::TextureView,
    blit_idx: u32,
) {
    let sc = res.swapchain_scene.as_ref().expect("swapchain scene");
    let sdf_bytes = (res.segment.instances.len() * std::mem::size_of::<SdfQuadInstance>()) as u64;
    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("KRui Swapchain Blit"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: out_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    rpass.set_pipeline(&pipes.sdf_pipeline);
    rpass.set_bind_group(0, &ctx.atlas_bind_group, &[]);
    rpass.set_bind_group(1, &sc.aux_bg, &[]);
    rpass.set_vertex_buffer(0, res.sdf_buf.slice(0..sdf_bytes));
    rpass.draw(0..6, blit_idx..blit_idx + 1);
}

fn resolve_aux_bg<'a>(
    res: &'a WgpuFrameResources,
    ctx: &'a GpuContext,
    aux: AuxBinding,
) -> &'a wgpu::BindGroup {
    match aux {
        AuxBinding::Dummy => &ctx.dummy_aux_bg,
        AuxBinding::BlurredHalfB => {
            &res.blur_targets
                .chain
                .as_ref()
                .expect("blur chain")
                .half_b_aux_bg
        }
        AuxBinding::Layer(idx) => &res.layer_pool.layers[idx].aux_bg,
        AuxBinding::Custom(idx) => &res.segment.aux_bind_groups[idx],
    }
}

fn clamp_scissor(scissor: Option<[u32; 4]>, vw: u32, vh: u32) -> Option<[u32; 4]> {
    let [sx, sy, sw, sh] = scissor.unwrap_or([0, 0, vw, vh]);
    let x0 = sx.min(vw.saturating_sub(1));
    let y0 = sy.min(vh.saturating_sub(1));
    let w = sw.min(vw.saturating_sub(x0));
    let h = sh.min(vh.saturating_sub(y0));
    (w > 0 && h > 0).then_some([x0, y0, w, h])
}

fn srgb_ch_to_linear_f64(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn premul_clear_color(c: Color, target_is_srgb: bool) -> wgpu::Color {
    let arr = c.to_f32_array();
    let (mut r, mut g, mut b, a) = (arr[0] as f64, arr[1] as f64, arr[2] as f64, arr[3] as f64);
    if target_is_srgb {
        r = srgb_ch_to_linear_f64(r);
        g = srgb_ch_to_linear_f64(g);
        b = srgb_ch_to_linear_f64(b);
    }
    wgpu::Color {
        r: r * a,
        g: g * a,
        b: b * a,
        a,
    }
}
