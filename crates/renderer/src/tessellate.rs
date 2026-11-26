//! Lyon 2D vector tessellation for `PaintScene` and `<canvas>` commands.

use crate::tessellate_primitives::{
    apply_opacity, build_rect_lyon_path, push_transformed_textured_quad, tessellate_canvas_cmd,
    tessellate_fill, tessellate_stroke,
};
use crate::GlyphAtlasPacker;
use bytemuck::{Pod, Zeroable};
use krui_core::{DrawPrimitive, GpuResourceLimits, PaintScene};
use krui_layout::LayoutRect;
use krui_style::Affine2;
use lyon_tessellation::{FillTessellator, StrokeTessellator, VertexBuffers};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable, PartialEq)]
pub struct GpuVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

/// A contiguous index range sharing the same physical scissor rectangle `[x, y, width, height]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuDrawBatch {
    pub index_range: (u32, u32),
    pub scissor: Option<[u32; 4]>,
}

/// Tessellates a `PaintScene` into contiguous `GpuVertex` and `u32` index arrays using `lyon_tessellation`,
/// enforcing `GpuResourceLimits` (Section 3.11 & Section 6).
pub fn tessellate_scene_to_mesh(
    scene: &PaintScene,
    limits: GpuResourceLimits,
) -> (Vec<GpuVertex>, Vec<u32>, usize) {
    let mut dummy_atlas = GlyphAtlasPacker::new();
    tessellate_scene_with_atlas(scene, limits, &mut dummy_atlas)
}

/// Tessellates a `PaintScene` while packing any new `TextGlyphs` bitmaps into `atlas`.
pub fn tessellate_scene_with_atlas(
    scene: &PaintScene,
    limits: GpuResourceLimits,
    atlas: &mut GlyphAtlasPacker,
) -> (Vec<GpuVertex>, Vec<u32>, usize) {
    let (v, i, _, clamped) = tessellate_scene_for_surface_batched(scene, limits, atlas, 0, 0);
    (v, i, clamped)
}

/// Tessellates a `PaintScene` for a physical swapchain surface (`target_phys_w x target_phys_h`)
/// so HiDPI glyphs map 1:1 to physical display pixels.
pub fn tessellate_scene_for_surface(
    scene: &PaintScene,
    limits: GpuResourceLimits,
    atlas: &mut GlyphAtlasPacker,
    target_phys_w: u32,
    target_phys_h: u32,
) -> (Vec<GpuVertex>, Vec<u32>, usize) {
    let (v, i, _, clamped) =
        tessellate_scene_for_surface_batched(scene, limits, atlas, target_phys_w, target_phys_h);
    (v, i, clamped)
}

pub(crate) fn compute_physical_scissor(
    clip: Option<LayoutRect>,
    scale: f32,
    phys_w: u32,
    phys_h: u32,
) -> Option<Option<[u32; 4]>> {
    let Some(r) = clip else {
        return Some(None);
    };
    let x0 = ((r.x * scale).floor().max(0.0) as u32).min(phys_w);
    let y0 = ((r.y * scale).floor().max(0.0) as u32).min(phys_h);
    let x1 = (((r.x + r.width) * scale).ceil().max(0.0) as u32).min(phys_w);
    let y1 = (((r.y + r.height) * scale).ceil().max(0.0) as u32).min(phys_h);
    let sw = x1.saturating_sub(x0);
    let sh = y1.saturating_sub(y0);
    if sw == 0 || sh == 0 {
        None
    } else {
        Some(Some([x0, y0, sw, sh]))
    }
}

/// Tessellates a `PaintScene` into `GpuVertex`/`u32` buffers and groups draw calls into
/// `GpuDrawBatch`es by physical scissor rectangle (`clip: Option<LayoutRect>`).
pub fn tessellate_scene_for_surface_batched(
    scene: &PaintScene,
    limits: GpuResourceLimits,
    atlas: &mut GlyphAtlasPacker,
    target_phys_w: u32,
    target_phys_h: u32,
) -> (Vec<GpuVertex>, Vec<u32>, Vec<GpuDrawBatch>, usize) {
    let surface_scale = if target_phys_w > 0 && target_phys_h > 0 {
        scene.scale_factor.clamp(1.0, 4.0)
    } else {
        1.0_f32
    };
    tessellate_scene_for_surface_with_scale_batched(
        scene,
        limits,
        atlas,
        target_phys_w,
        target_phys_h,
        surface_scale,
    )
}

struct TessellationContext {
    mesh: VertexBuffers<GpuVertex, u32>,
    scratch: VertexBuffers<GpuVertex, u32>,
    fill_tess: FillTessellator,
    stroke_tess: StrokeTessellator,
    limits: GpuResourceLimits,
    scale: f32,
    shape_scale: f32,
    vw: f32,
    vh: f32,
}

/// Tessellates a `PaintScene` for a target surface of `(target_phys_w, target_phys_h)` with an
/// explicit `surface_scale` while preserving the glyph rasterization scale from `scene.scale_factor`.
pub fn tessellate_scene_for_surface_with_scale_batched(
    scene: &PaintScene,
    limits: GpuResourceLimits,
    atlas: &mut GlyphAtlasPacker,
    target_phys_w: u32,
    target_phys_h: u32,
    surface_scale: f32,
) -> (Vec<GpuVertex>, Vec<u32>, Vec<GpuDrawBatch>, usize) {
    let limits = limits.normalized();
    atlas.begin_frame();
    let (scale, phys_w, phys_h) =
        resolve_surface_dimensions(scene, &limits, target_phys_w, target_phys_h, surface_scale);
    let mut ctx = TessellationContext {
        mesh: VertexBuffers::with_capacity(2048, 6144),
        scratch: VertexBuffers::with_capacity(256, 768),
        fill_tess: FillTessellator::new(),
        stroke_tess: StrokeTessellator::new(),
        limits,
        scale,
        shape_scale: scene.scale_factor.clamp(1.0, 4.0),
        vw: (phys_w as f32) / scale,
        vh: (phys_h as f32) / scale,
    };

    let mut batches: Vec<GpuDrawBatch> = Vec::with_capacity(16);
    let max_cmds = scene
        .primitives
        .len()
        .min(limits.max_draw_commands_per_frame);
    let mut clamped_count = scene.primitives.len().saturating_sub(max_cmds);

    for prim in &scene.primitives[..max_cmds] {
        if ctx.mesh.vertices.len() >= limits.max_vertices_per_frame
            || ctx.mesh.indices.len() >= limits.max_indices_per_frame
        {
            clamped_count += 1;
            continue;
        }
        let Some(scissor) = compute_physical_scissor(prim.clip_rect(), scale, phys_w, phys_h)
        else {
            continue;
        };
        let start_idx = ctx.mesh.indices.len() as u32;
        if tessellate_primitive(&mut ctx, prim, atlas) {
            clamped_count += 1;
        }
        record_draw_batch(
            &mut batches,
            start_idx,
            ctx.mesh.indices.len() as u32,
            scissor,
        );
    }

    (ctx.mesh.vertices, ctx.mesh.indices, batches, clamped_count)
}

fn record_draw_batch(
    batches: &mut Vec<GpuDrawBatch>,
    start_idx: u32,
    end_idx: u32,
    scissor: Option<[u32; 4]>,
) {
    if end_idx <= start_idx {
        return;
    }
    if let Some(last) = batches.last_mut().filter(|b| b.scissor == scissor) {
        last.index_range.1 = end_idx;
    } else {
        batches.push(GpuDrawBatch {
            index_range: (start_idx, end_idx),
            scissor,
        });
    }
}

fn resolve_surface_dimensions(
    scene: &PaintScene,
    limits: &GpuResourceLimits,
    target_phys_w: u32,
    target_phys_h: u32,
    surface_scale: f32,
) -> (f32, u32, u32) {
    if target_phys_w > 0 && target_phys_h > 0 {
        let s = surface_scale.clamp(1.0, 4.0);
        let pw = target_phys_w.clamp(1, limits.max_viewport_dimension);
        let ph = target_phys_h.clamp(1, limits.max_viewport_dimension);
        (s, pw, ph)
    } else {
        let pw = scene.width.clamp(1, limits.max_viewport_dimension);
        let ph = scene.height.clamp(1, limits.max_viewport_dimension);
        (1.0_f32, pw, ph)
    }
}

fn tessellate_primitive(
    ctx: &mut TessellationContext,
    prim: &DrawPrimitive,
    atlas: &mut GlyphAtlasPacker,
) -> bool {
    match prim {
        DrawPrimitive::StyledBox {
            rect,
            transform,
            opacity,
            background_color,
            border_color,
            border_width,
            border_radius,
            ..
        } => tessellate_styled_box(
            ctx,
            *rect,
            *transform,
            *opacity,
            *background_color,
            *border_color,
            *border_width,
            *border_radius,
        ),
        DrawPrimitive::TextGlyphs {
            origin_x,
            origin_y,
            transform,
            opacity,
            color,
            glyphs,
            ..
        } => tessellate_text_glyphs(
            ctx, atlas, *origin_x, *origin_y, *transform, *opacity, *color, glyphs,
        ),
        DrawPrimitive::CanvasBatch {
            viewport,
            transform,
            opacity,
            commands,
            ..
        } => tessellate_canvas_batch(ctx, *viewport, *transform, *opacity, commands),
        DrawPrimitive::Image { .. } | DrawPrimitive::PushLayer { .. } | DrawPrimitive::PopLayer => {
            false
        }
    }
}

/// Standalone `lyon` tessellator for `<canvas>` 2D vector commands used by `CanvasMeshCache`.
pub(crate) fn tessellate_canvas_commands_standalone(
    commands: &[krui_core::CanvasCommand],
    viewport: LayoutRect,
    transform: Affine2,
    opacity: f32,
    surface_scale: f32,
    (vw, vh): (f32, f32),
    limits: GpuResourceLimits,
) -> (Vec<GpuVertex>, Vec<u32>, bool) {
    let limits = limits.normalized();
    let scale = surface_scale.clamp(1.0, 4.0);
    let mut ctx = TessellationContext {
        mesh: VertexBuffers::with_capacity(256, 768),
        scratch: VertexBuffers::with_capacity(128, 384),
        fill_tess: FillTessellator::new(),
        stroke_tess: StrokeTessellator::new(),
        limits,
        scale,
        shape_scale: scale,
        vw: vw.max(1.0),
        vh: vh.max(1.0),
    };
    let clamped = tessellate_canvas_batch(&mut ctx, viewport, transform, opacity, commands);
    (ctx.mesh.vertices, ctx.mesh.indices, clamped)
}

#[allow(clippy::too_many_arguments)]
fn tessellate_styled_box(
    ctx: &mut TessellationContext,
    rect: LayoutRect,
    transform: Affine2,
    opacity: f32,
    background_color: krui_style::Color,
    border_color: krui_style::Color,
    border_width: f32,
    border_radius: f32,
) -> bool {
    if opacity <= 0.0 {
        return false;
    }
    let path = build_rect_lyon_path(
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        border_radius,
        transform,
    );
    let mut prim_clamped = false;
    if background_color.a > 0 {
        let col = apply_opacity(background_color, opacity);
        ctx.scratch.vertices.clear();
        ctx.scratch.indices.clear();
        tessellate_fill(
            &path,
            col,
            ctx.vw,
            ctx.vh,
            &mut ctx.fill_tess,
            &mut ctx.scratch,
        );
        if !commit_scratch_if_within_limits(&mut ctx.mesh, &ctx.scratch, &ctx.limits) {
            prim_clamped = true;
        }
    }
    if border_width > 0.0 && border_color.a > 0 {
        let col = apply_opacity(border_color, opacity);
        ctx.scratch.vertices.clear();
        ctx.scratch.indices.clear();
        tessellate_stroke(
            &path,
            border_width,
            col,
            ctx.vw,
            ctx.vh,
            &mut ctx.stroke_tess,
            &mut ctx.scratch,
        );
        if !commit_scratch_if_within_limits(&mut ctx.mesh, &ctx.scratch, &ctx.limits) {
            prim_clamped = true;
        }
    }
    prim_clamped
}

fn tessellate_canvas_batch(
    ctx: &mut TessellationContext,
    viewport: LayoutRect,
    transform: Affine2,
    opacity: f32,
    commands: &[krui_core::CanvasCommand],
) -> bool {
    if opacity <= 0.0 {
        return false;
    }
    let s = ctx.scale;
    let base_tx = if transform == Affine2::IDENTITY {
        Affine2 {
            a: s,
            b: 0.0,
            c: 0.0,
            d: s,
            tx: (viewport.x * s).round(),
            ty: (viewport.y * s).round(),
        }
    } else {
        Affine2::scale(s, s) * transform * Affine2::translation(viewport.x, viewport.y)
    };
    let (phys_w, phys_h) = ((ctx.vw * s).max(1.0), (ctx.vh * s).max(1.0));
    let mut local_tx = Affine2::IDENTITY;
    let mut cmd_clamped = false;
    for cmd in commands.iter() {
        if let krui_core::CanvasCommand::SetTransform(m) = cmd {
            local_tx = *m;
            continue;
        }
        ctx.scratch.vertices.clear();
        ctx.scratch.indices.clear();
        tessellate_canvas_cmd(
            cmd,
            base_tx * local_tx,
            opacity,
            phys_w,
            phys_h,
            &mut ctx.fill_tess,
            &mut ctx.stroke_tess,
            &mut ctx.scratch,
        );
        if !commit_scratch_if_within_limits(&mut ctx.mesh, &ctx.scratch, &ctx.limits) {
            cmd_clamped = true;
            break;
        }
    }
    cmd_clamped
}

#[allow(clippy::too_many_arguments)]
fn tessellate_text_glyphs(
    ctx: &mut TessellationContext,
    atlas: &mut GlyphAtlasPacker,
    origin_x: f32,
    origin_y: f32,
    transform: Affine2,
    opacity: f32,
    color: krui_style::Color,
    glyphs: &[krui_text::RasterizedGlyph],
) -> bool {
    if opacity <= 0.0 {
        return false;
    }
    let pure_trans = transform.is_pure_translation();
    let col = apply_opacity(color, opacity);
    let mut glyph_clamped = false;
    for g in glyphs.iter() {
        if ctx.mesh.vertices.len() + 4 > ctx.limits.max_vertices_per_frame
            || ctx.mesh.indices.len() + 6 > ctx.limits.max_indices_per_frame
        {
            glyph_clamped = true;
            break;
        }
        let Some(uv_rect) = atlas.pack_glyph_by_key(g.cache_key, g.width, g.height, &g.alpha_mask)
        else {
            continue;
        };
        let lx = origin_x + g.x;
        let ly = origin_y + g.y;
        let gw = (g.width as f32) / ctx.shape_scale;
        let gh = (g.height as f32) / ctx.shape_scale;
        let corners = if pure_trans {
            let ox_phys = ((origin_x + transform.tx) * ctx.scale).round();
            let oy_phys = ((origin_y + transform.ty) * ctx.scale).round();
            let gx = (ox_phys + (g.x * ctx.scale).round()) / ctx.scale;
            let gy = (oy_phys + (g.y * ctx.scale).round()) / ctx.scale;
            [(gx, gy), (gx + gw, gy), (gx + gw, gy + gh), (gx, gy + gh)]
        } else {
            [
                transform.transform_point(lx, ly),
                transform.transform_point(lx + gw, ly),
                transform.transform_point(lx + gw, ly + gh),
                transform.transform_point(lx, ly + gh),
            ]
        };
        push_transformed_textured_quad(&mut ctx.mesh, corners, uv_rect, col, ctx.vw, ctx.vh);
    }
    glyph_clamped
}

fn commit_scratch_if_within_limits(
    mesh: &mut VertexBuffers<GpuVertex, u32>,
    scratch: &VertexBuffers<GpuVertex, u32>,
    limits: &GpuResourceLimits,
) -> bool {
    if scratch.vertices.is_empty() || scratch.indices.is_empty() {
        return true;
    }
    if mesh.vertices.len() + scratch.vertices.len() > limits.max_vertices_per_frame
        || mesh.indices.len() + scratch.indices.len() > limits.max_indices_per_frame
    {
        return false;
    }
    let base = mesh.vertices.len() as u32;
    mesh.vertices.extend_from_slice(&scratch.vertices);
    mesh.indices
        .extend(scratch.indices.iter().map(|&idx| base + idx));
    true
}
