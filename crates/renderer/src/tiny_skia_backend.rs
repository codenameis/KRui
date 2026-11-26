//! Low-power CPU rasterizer backend built on `tiny-skia` with cached clip `Mask`s,
//! pooled `PushLayer`/`PopLayer` compositing, exact inset borders, gradients, shadows, and blur.

mod rgb565_dither;
mod tiny_skia_blur;
pub(crate) mod tiny_skia_canvas;
mod tiny_skia_clip_cache;
mod tiny_skia_conic;
mod tiny_skia_damage;
mod tiny_skia_effects;
mod tiny_skia_filter;
mod tiny_skia_glyphs;
mod tiny_skia_images;
mod tiny_skia_perspective;
mod tiny_skia_radial;
mod tiny_skia_shadow_cache;
mod tiny_skia_shapes;

pub use rgb565_dither::convert_rgba8_to_rgb565_le_dithered;

use crate::RendererBackend;
use krui_core::{
    ActiveBackendKind, DrawPrimitive, FrameRenderStats, GpuResourceLimits, PaintScene,
};
use krui_layout::LayoutRect;
use krui_style::{Affine2, Color};
use tiny_skia_blur::render_backdrop_blur_skia;
use tiny_skia_clip_cache::{aabb_needs_clip_mask, ClipMaskCache};
use tiny_skia_effects::{render_box_shadow_skia, SkiaBoxEffectCtx};
use tiny_skia_filter::step_layer_op_skia;
use tiny_skia_glyphs::{rasterize_glyphs_skia, GlyphBlitParams};
use tiny_skia_images::SkiaImageCache;
use tiny_skia_shadow_cache::ShadowPatchCache;
use tiny_skia_shapes::{paint_styled_box_surfaces_skia, to_skia_color, StyledBoxSkiaArgs};

/// Pure-Rust CPU rasterizer backend built on `tiny-skia` (Section 3.5).
pub struct TinySkiaRenderer {
    pixmap: tiny_skia::Pixmap,
    mask_cache: ClipMaskCache,
    shadow_cache: ShadowPatchCache,
    image_cache: SkiaImageCache,
    layer_pool: Vec<tiny_skia::Pixmap>,
    limits: GpuResourceLimits,
    has_rendered: bool,
}

impl TinySkiaRenderer {
    /// Creates a new CPU fallback renderer for `(width, height)` bounded by `limits`.
    pub fn new(width: u32, height: u32, limits: GpuResourceLimits) -> Self {
        let limits = limits.normalized();
        let w = width.clamp(1, limits.max_viewport_dimension);
        let h = height.clamp(1, limits.max_viewport_dimension);
        let pixmap = tiny_skia::Pixmap::new(w, h).expect("valid pixmap dimensions");
        Self {
            pixmap,
            mask_cache: ClipMaskCache::default(),
            shadow_cache: ShadowPatchCache::default(),
            image_cache: SkiaImageCache::default(),
            layer_pool: Vec::new(),
            limits,
            has_rendered: false,
        }
    }

    /// Exports the current RGBA framebuffer as a PNG image.
    pub fn save_png(&self, path: impl AsRef<std::path::Path>) -> Result<(), String> {
        self.pixmap
            .save_png(path)
            .map_err(|e| format!("Failed to save PNG: {e}"))
    }

    /// Returns the physical width of the current RGBA framebuffer in pixels.
    pub fn physical_width(&self) -> u32 {
        self.pixmap.width()
    }

    /// Returns the physical height of the current RGBA framebuffer in pixels.
    pub fn physical_height(&self) -> u32 {
        self.pixmap.height()
    }

    /// Returns `(cache_hits, mask_allocations)` for the reusable clip `Mask` LRU cache.
    pub fn clip_mask_stats(&self) -> (u64, u64) {
        self.mask_cache.stats()
    }

    fn ensure_size(&mut self, width: u32, height: u32) {
        let max_dim = self.limits.max_viewport_dimension.max(1);
        let w = width.clamp(1, max_dim);
        let h = height.clamp(1, max_dim);
        if self.pixmap.width() != w || self.pixmap.height() != h {
            self.pixmap = tiny_skia::Pixmap::new(w, h).expect("valid pixmap dimensions");
            self.layer_pool.clear();
        }
    }

    fn acquire_layer_pixmap(&mut self, w: u32, h: u32) -> Option<tiny_skia::Pixmap> {
        if let Some(mut px) = self.layer_pool.pop() {
            if px.width() == w && px.height() == h {
                px.data_mut().fill(0);
                return Some(px);
            }
        }
        tiny_skia::Pixmap::new(w, h)
    }
}

struct SkiaRenderCaches<'a> {
    mask: &'a mut ClipMaskCache,
    shadow: &'a mut ShadowPatchCache,
    image: &'a mut SkiaImageCache,
}

impl RendererBackend for TinySkiaRenderer {
    fn backend_kind(&self) -> ActiveBackendKind {
        ActiveBackendKind::TinySkiaCpuFallback
    }

    fn render_scene(&mut self, scene: &PaintScene) -> FrameRenderStats {
        let scale = scene.scale_factor.clamp(1.0, 4.0);
        let phys_w = ((scene.width as f32 * scale).round() as u32).max(1);
        let phys_h = ((scene.height as f32 * scale).round() as u32).max(1);
        self.ensure_size(phys_w, phys_h);
        self.pixmap.fill(to_skia_color(scene.clear_color, 1.0));
        self.has_rendered = true;
        let max_cmds = scene
            .primitives
            .len()
            .min(self.limits.max_draw_commands_per_frame);
        let skipped = scene.primitives.len().saturating_sub(max_cmds);
        let (clamped, verts_eq, idx_eq) =
            self.execute_skia_primitives(&scene.primitives[..max_cmds], scale, skipped);
        self.layer_pool.truncate(4);
        FrameRenderStats {
            backend_kind: ActiveBackendKind::TinySkiaCpuFallback,
            draw_primitives_submitted: max_cmds.saturating_sub(clamped.saturating_sub(skipped)),
            draw_primitives_clamped: clamped,
            vertices_generated: verts_eq,
            indices_generated: idx_eq,
        }
    }

    fn framebuffer_dimensions(&self) -> Option<(u32, u32)> {
        self.has_rendered
            .then(|| (self.pixmap.width(), self.pixmap.height()))
    }

    fn framebuffer_rgba(&mut self) -> Option<&[u8]> {
        self.has_rendered.then(|| self.pixmap.data())
    }
}

impl TinySkiaRenderer {
    fn execute_skia_primitives(
        &mut self,
        primitives: &[DrawPrimitive],
        scale: f32,
        mut clamped: usize,
    ) -> (usize, usize, usize) {
        let vp = (self.pixmap.width(), self.pixmap.height());
        let scale_tx = Affine2::scale(scale, scale);
        let (mut verts_eq, mut idx_eq) = (0_usize, 0_usize);
        let (mut stack, mut active, mut inline_op) = (Vec::new(), 0, 1.0_f32);
        for prim in primitives {
            if step_layer_op_skia(
                self,
                prim,
                &mut stack,
                &mut active,
                &mut inline_op,
                vp,
                scale,
            ) {
                continue;
            }
            let (est_v, est_i) = estimate_primitive_mesh_cost(prim);
            if verts_eq + est_v > self.limits.max_vertices_per_frame
                || idx_eq + est_i > self.limits.max_indices_per_frame
            {
                clamped += 1;
                continue;
            }
            let mut caches = SkiaRenderCaches {
                mask: &mut self.mask_cache,
                shadow: &mut self.shadow_cache,
                image: &mut self.image_cache,
            };
            let (v, i) = render_primitive_skia(
                &mut self.pixmap,
                &mut caches,
                prim,
                scale,
                scale_tx,
                inline_op,
            );
            verts_eq += v;
            idx_eq += i;
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
        (clamped, verts_eq, idx_eq)
    }
}

fn estimate_primitive_mesh_cost(prim: &DrawPrimitive) -> (usize, usize) {
    match prim {
        DrawPrimitive::StyledBox { .. } | DrawPrimitive::Image { .. } => (4, 6),
        DrawPrimitive::TextGlyphs { glyphs, .. } => (glyphs.len() * 4, glyphs.len() * 6),
        DrawPrimitive::CanvasBatch { commands, .. } => (commands.len() * 8, commands.len() * 12),
        DrawPrimitive::PushLayer { .. } | DrawPrimitive::PopLayer => (0, 0),
    }
}

fn render_primitive_skia(
    pixmap: &mut tiny_skia::Pixmap,
    caches: &mut SkiaRenderCaches<'_>,
    prim: &DrawPrimitive,
    scale: f32,
    scale_tx: Affine2,
    inline_opacity: f32,
) -> (usize, usize) {
    if let Some(mut args) = StyledBoxSkiaArgs::from_primitive(prim, scale, scale_tx) {
        args.opacity *= inline_opacity;
        render_styled_box_skia(pixmap, caches.mask, caches.shadow, args);
        return (4, 6);
    }
    match prim {
        DrawPrimitive::TextGlyphs { .. } => {
            render_text_prim_skia(pixmap, caches.mask, prim, scale, inline_opacity)
        }
        DrawPrimitive::CanvasBatch {
            viewport,
            clip,
            clip_radius,
            clip_radii,
            transform,
            opacity,
            commands,
        } => tiny_skia_canvas::render_canvas_batch_skia(
            pixmap,
            caches.mask,
            *viewport,
            (*clip, *clip_radius, *clip_radii),
            (*transform, scale, scale_tx),
            *opacity * inline_opacity,
            commands,
        ),
        DrawPrimitive::Image { .. } => render_image_prim_skia(
            pixmap,
            caches.mask,
            caches.image,
            prim,
            scale,
            scale_tx,
            inline_opacity,
        ),
        _ => (0, 0),
    }
}

fn render_text_prim_skia(
    pixmap: &mut tiny_skia::Pixmap,
    mask_cache: &mut ClipMaskCache,
    prim: &DrawPrimitive,
    scale: f32,
    inline_opacity: f32,
) -> (usize, usize) {
    let DrawPrimitive::TextGlyphs {
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
    else {
        return (0, 0);
    };
    render_text_glyphs_skia(
        pixmap,
        mask_cache,
        (*origin_x, *origin_y),
        (*clip, *clip_radius, *clip_radii),
        *transform,
        scale,
        *opacity * inline_opacity,
        *color,
        glyphs,
    )
}

fn render_image_prim_skia(
    pixmap: &mut tiny_skia::Pixmap,
    mask_cache: &mut ClipMaskCache,
    image_cache: &mut SkiaImageCache,
    prim: &DrawPrimitive,
    scale: f32,
    scale_tx: Affine2,
    inline_opacity: f32,
) -> (usize, usize) {
    let DrawPrimitive::Image {
        rect,
        uv_rect,
        clip,
        clip_radius,
        clip_radii,
        transform,
        opacity,
        border_radius,
        source,
    } = prim
    else {
        return (0, 0);
    };
    tiny_skia_images::render_image_skia(
        pixmap,
        mask_cache,
        image_cache,
        tiny_skia_images::ImageSkiaArgs {
            rect: *rect,
            uv_rect: *uv_rect,
            clip: *clip,
            clip_radius: *clip_radius,
            clip_radii: *clip_radii,
            transform: *transform,
            opacity: *opacity * inline_opacity,
            border_radius: *border_radius,
            scale,
            scale_tx,
            source,
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn render_text_glyphs_skia(
    pixmap: &mut tiny_skia::Pixmap,
    mask_cache: &mut ClipMaskCache,
    origin: (f32, f32),
    (clip, clip_radius, clip_radii): (Option<LayoutRect>, f32, [f32; 4]),
    transform: Affine2,
    scale: f32,
    opacity: f32,
    color: Color,
    glyphs: &[krui_text::RasterizedGlyph],
) -> (usize, usize) {
    if opacity <= 0.0 || glyphs.is_empty() {
        return (0, 0);
    }
    let has_round_clip =
        clip.is_some() && (clip_radius > 0.0 || clip_radii.iter().any(|&r| r > 0.0));
    let mask = has_round_clip
        .then(|| {
            mask_cache.get_or_build_4(
                pixmap.width(),
                pixmap.height(),
                clip,
                clip_radius,
                clip_radii,
                scale,
            )
        })
        .flatten();
    let params = GlyphBlitParams {
        origin_x: origin.0,
        origin_y: origin.1,
        clip,
        clip_mask: mask.map(tiny_skia::Mask::data),
        transform,
        scale,
        opacity,
        color,
    };
    rasterize_glyphs_skia(pixmap, params, glyphs);
    (glyphs.len() * 4, glyphs.len() * 6)
}

fn render_styled_box_skia(
    pixmap: &mut tiny_skia::Pixmap,
    mask_cache: &mut ClipMaskCache,
    shadow_cache: &mut ShadowPatchCache,
    a: StyledBoxSkiaArgs,
) {
    if a.rect.width <= 0.0 || a.rect.height <= 0.0 || a.opacity <= 0.0 {
        return;
    }
    let mask = if box_needs_clip_mask(&a) {
        mask_cache.get_or_build_4(
            pixmap.width(),
            pixmap.height(),
            a.clip,
            a.clip_radius,
            a.clip_radii,
            a.scale,
        )
    } else {
        None
    };
    let eff_ctx = SkiaBoxEffectCtx {
        rect: a.rect,
        scaled_transform: a.scaled_transform,
        opacity: a.opacity,
        border_width: a.border_width.max(0.0),
        corner_radii: a.corner_radii,
        mask,
    };
    if a.backdrop_blur_px > 0.5 && (a.background_color.a < 255 || a.opacity < 0.999) {
        render_backdrop_blur_skia(pixmap, &eff_ctx, a.backdrop_blur_px, a.scale);
    }
    if let Some(sh) = a.box_shadow.filter(|s| !s.inset) {
        render_box_shadow_skia(pixmap, shadow_cache, &eff_ctx, sh);
    }
    paint_styled_box_surfaces_skia(pixmap, mask, &a);
    if let Some(sh) = a.box_shadow.filter(|s| s.inset) {
        render_box_shadow_skia(pixmap, shadow_cache, &eff_ctx, sh);
    }
}

fn box_needs_clip_mask(a: &StyledBoxSkiaArgs) -> bool {
    if a.clip.is_none() {
        return false;
    }
    let hw = 0.5 * a.rect.width;
    let hh = 0.5 * a.rect.height;
    let (lcx, lcy) = (a.rect.x + hw, a.rect.y + hh);
    let (scx, scy) = a.scaled_transform.transform_point(lcx, lcy);
    let tx = a.scaled_transform;
    let mut ext_x = hw + 2.0;
    let mut ext_y = hh + 2.0;
    if let Some(sh) = a.box_shadow.filter(|s| !s.inset) {
        let pad = sh.blur_radius.max(0.0) * 2.2 + 3.0;
        ext_x += sh.spread_radius.max(0.0) + sh.offset_x.abs() + pad;
        ext_y += sh.spread_radius.max(0.0) + sh.offset_y.abs() + pad;
    }
    let rx = tx.a.abs() * ext_x + tx.c.abs() * ext_y;
    let ry = tx.b.abs() * ext_x + tx.d.abs() * ext_y;
    aabb_needs_clip_mask(
        a.clip,
        a.clip_radius,
        a.clip_radii,
        (scx - rx, scy - ry, scx + rx, scy + ry),
        a.scale,
    )
}
