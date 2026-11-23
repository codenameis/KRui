//! # `krui-renderer`
//!
//! Dual-backend rendering engine (`wgpu` primary GPU + `tiny-skia` CPU fallback),
//! `lyon`-powered `<canvas>` 2D vector tessellator, GPU glyph texture atlas, bounded GPU resource
//! guards (Section 6), and 3D / VR-AR extensibility trait boundaries (Section 3.5, 3.7, 3.10, 3.11).

#![forbid(unsafe_code)]

mod blur_pass;
mod canvas_cache;
mod dual_atlas;
mod gpu_context;
mod image_cache;
mod presenter;
mod sdf_instance;
mod sdf_instance_aux;
mod tessellate;
mod tessellate_aa;
mod tessellate_primitives;
mod tiny_skia_backend;
mod wgpu_backend;
mod wgpu_compositor;
mod wgpu_compositor_emit;
mod wgpu_compositor_exec;
mod wgpu_compositor_media;
mod wgpu_pipeline;

use std::collections::HashMap;
use std::sync::Arc;

pub use canvas_cache::CanvasMeshCache;
pub use dual_atlas::{
    AtlasPackError, DualGlyphAtlas, GlyphSlot, COLOR_ATLAS_LAYERS, DUAL_ATLAS_SIZE,
    MASK_ATLAS_LAYERS,
};
pub use gpu_context::GpuContext;
pub use krui_core::{
    ActiveBackendKind, CanvasCommand, CanvasContext2D, CanvasDraw, CanvasFillRule, CanvasLineCap,
    CanvasLineJoin, CanvasRedrawMode, DecodedImage, DrawPrimitive, ExtensibleEvent,
    ExternalTextureId, FrameRenderStats, GpuResourceLimits, ImageId, ImageSource, PaintScene,
    PathSegment, RenderBackendPreference, Size,
};
pub use presenter::WindowSurfacePresenter;
pub use sdf_instance::SdfQuadInstance;
pub use tessellate::{
    tessellate_scene_for_surface, tessellate_scene_for_surface_batched,
    tessellate_scene_for_surface_with_scale_batched, tessellate_scene_to_mesh,
    tessellate_scene_with_atlas, GpuDrawBatch, GpuVertex,
};
pub use tiny_skia_backend::TinySkiaRenderer;
pub use wgpu_backend::WgpuRenderer;

pub const ATLAS_SIZE: u32 = 1024;

fn hash_glyph_mask(w: u32, h: u32, alpha_mask: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in &w.to_le_bytes() {
        hash = (hash ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    for &b in &h.to_le_bytes() {
        hash = (hash ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    for &b in alpha_mask {
        hash = (hash ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Persistent shelf-based `R8Unorm` (1 byte per pixel) texture atlas packer and glyph cache.
///
/// Reserves the top-left `4x4` region for opaque coverage (`255`) so solid vector geometry and
/// textured glyphs can be drawn in a single unified `wgpu` render pass without pipeline switches.
pub struct GlyphAtlasPacker {
    pub pixels: Vec<u8>,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    pub dirty_min_y: u32,
    pub dirty_max_y: u32,
    pub dirty_rows: u32,
    pub uploaded_rows: u32,
    pub overflow_pending: bool,
    pub epoch: u64,
    glyph_cache: HashMap<u64, [f32; 4]>,
}

impl Default for GlyphAtlasPacker {
    fn default() -> Self {
        Self::new()
    }
}

impl GlyphAtlasPacker {
    pub fn new() -> Self {
        let mut pixels = vec![0_u8; (ATLAS_SIZE * ATLAS_SIZE) as usize];
        for y in 0..4 {
            let row_start = (y * ATLAS_SIZE) as usize;
            pixels[row_start..row_start + 4].fill(255);
        }
        Self {
            pixels,
            cursor_x: 6,
            cursor_y: 0,
            row_height: 6,
            dirty_min_y: 0,
            dirty_max_y: 6,
            dirty_rows: 6,
            uploaded_rows: 0,
            overflow_pending: false,
            epoch: 1,
            glyph_cache: HashMap::with_capacity(512),
        }
    }

    /// Evicts all glyphs at a frame boundary if `overflow_pending` was triggered, incrementing `epoch`.
    pub fn begin_frame(&mut self) {
        if self.overflow_pending {
            self.reset_frame();
        }
    }

    pub fn reset_frame(&mut self) {
        let rows_to_clear = self.dirty_rows.max(self.uploaded_rows).min(ATLAS_SIZE);
        let bytes_to_clear = (rows_to_clear * ATLAS_SIZE) as usize;
        self.pixels[..bytes_to_clear].fill(0);
        for y in 0..4 {
            let row_start = (y * ATLAS_SIZE) as usize;
            self.pixels[row_start..row_start + 4].fill(255);
        }
        self.cursor_x = 6;
        self.cursor_y = 0;
        self.row_height = 6;
        self.dirty_min_y = 0;
        self.dirty_max_y = 6;
        self.dirty_rows = 6;
        self.uploaded_rows = 0;
        self.overflow_pending = false;
        self.epoch = self.epoch.wrapping_add(1);
        self.glyph_cache.clear();
    }

    /// Returns the number of unique glyphs currently cached in the atlas.
    pub fn cached_glyph_count(&self) -> usize {
        self.glyph_cache.len()
    }

    /// Looks up or packs a glyph alpha mask by its stable `cache_key` (no per-byte hashing!).
    ///
    /// Never clears the atlas mid-frame so earlier glyph UVs emitted in the same frame remain valid.
    pub fn pack_glyph_by_key(
        &mut self,
        cache_key: u64,
        w: u32,
        h: u32,
        alpha_mask: &[u8],
    ) -> Option<[f32; 4]> {
        if w == 0 || h == 0 || w + 4 > ATLAS_SIZE || h + 4 > ATLAS_SIZE {
            return None;
        }
        let key = if cache_key != 0 {
            cache_key
        } else {
            hash_glyph_mask(w, h, alpha_mask)
        };
        if let Some(&uv) = self.glyph_cache.get(&key) {
            return Some(uv);
        }
        if self.cursor_x + w + 2 > ATLAS_SIZE {
            self.cursor_x = 0;
            self.cursor_y += self.row_height + 2;
            self.row_height = 0;
        }
        if self.cursor_y + h + 2 > ATLAS_SIZE {
            self.overflow_pending = true;
            return None;
        }
        let (gx, gy) = (self.cursor_x + 1, self.cursor_y + 1);
        self.cursor_x += w + 2;
        self.row_height = self.row_height.max(h);
        self.record_dirty_shelf(gy, h);
        self.blit_glyph_rows(gx, gy, w, h, alpha_mask);
        let inv = 1.0 / ATLAS_SIZE as f32;
        let uv = [
            gx as f32 * inv,
            gy as f32 * inv,
            (gx + w) as f32 * inv,
            (gy + h) as f32 * inv,
        ];
        self.glyph_cache.insert(key, uv);
        Some(uv)
    }

    fn record_dirty_shelf(&mut self, gy: u32, h: u32) {
        let new_bottom = (gy + h + 2).min(ATLAS_SIZE);
        if self.dirty_max_y <= self.dirty_min_y {
            self.dirty_min_y = gy;
            self.dirty_max_y = new_bottom;
        } else {
            self.dirty_min_y = self.dirty_min_y.min(gy);
            self.dirty_max_y = self.dirty_max_y.max(new_bottom);
        }
        self.dirty_rows = self.dirty_rows.max(new_bottom);
    }

    fn blit_glyph_rows(&mut self, gx: u32, gy: u32, w: u32, h: u32, alpha_mask: &[u8]) {
        for row in 0..h {
            let src_row = (row * w) as usize;
            let dst_row = ((gy + row) * ATLAS_SIZE + gx) as usize;
            let copy_len = (src_row + w as usize)
                .min(alpha_mask.len())
                .saturating_sub(src_row);
            if copy_len > 0 {
                self.pixels[dst_row..dst_row + copy_len]
                    .copy_from_slice(&alpha_mask[src_row..src_row + copy_len]);
            }
        }
    }

    /// Looks up or packs a glyph alpha mask into the atlas and returns normalized `[u0, v0, u1, v1]`.
    pub fn pack_glyph(&mut self, w: u32, h: u32, alpha_mask: &[u8]) -> Option<[f32; 4]> {
        let key = hash_glyph_mask(w, h, alpha_mask);
        self.pack_glyph_by_key(key, w, h, alpha_mask)
    }
}

/// Architectural boundary trait for KRui rendering backends (Section 3.5 & Section 12).
pub trait RendererBackend {
    /// Returns which backend implementation is active.
    fn backend_kind(&self) -> ActiveBackendKind;

    /// Returns the shared `Arc<GpuContext>` when backed by `wgpu`, or `None` on CPU fallback.
    fn gpu_context(&self) -> Option<Arc<GpuContext>> {
        None
    }

    /// Returns `true` if this backend currently holds an allocated offscreen render target.
    fn has_offscreen_target(&self) -> bool {
        false
    }

    /// Renders `scene` and returns frame statistics.
    fn render_scene(&mut self, scene: &PaintScene) -> FrameRenderStats;

    /// Waits for all submitted GPU work on this backend to complete (no-op on CPU backends).
    fn finish_gpu_work(&mut self) {}

    /// Returns the physical `(width, height)` dimensions of the latest rendered framebuffer.
    fn framebuffer_dimensions(&self) -> Option<(u32, u32)> {
        None
    }

    /// Returns the latest rendered RGBA8 framebuffer (for window presentation, snapshots, and tests),
    /// or `None` if no frame has been rendered yet or readback is unavailable.
    fn framebuffer_rgba(&mut self) -> Option<&[u8]>;
}

/// High-level renderer wrapper that probes `wgpu` capability at startup and automatically
/// falls back to `TinySkiaRenderer` without selecting or mutating CSS themes.
pub struct AdaptiveRenderer {
    backend: Box<dyn RendererBackend>,
}

impl AdaptiveRenderer {
    /// Creates an `AdaptiveRenderer`, returning `Err` if `ForceWgpu` is requested and no GPU is available.
    pub fn try_new(
        width: u32,
        height: u32,
        preference: RenderBackendPreference,
        limits: GpuResourceLimits,
    ) -> Result<Self, String> {
        let backend: Box<dyn RendererBackend> = match preference {
            RenderBackendPreference::ForceTinySkia | RenderBackendPreference::CpuFallback => {
                Box::new(TinySkiaRenderer::new(width, height, limits))
            }
            RenderBackendPreference::ForceWgpu => {
                let wgpu_renderer =
                    WgpuRenderer::try_new(width, height, limits).ok_or_else(|| {
                        "ForceWgpu requested, but no wgpu adapter is available".to_string()
                    })?;
                Box::new(wgpu_renderer)
            }
            RenderBackendPreference::Auto => {
                if let Some(wgpu_renderer) = WgpuRenderer::try_new(width, height, limits) {
                    Box::new(wgpu_renderer)
                } else {
                    Box::new(TinySkiaRenderer::new(width, height, limits))
                }
            }
        };

        Ok(Self { backend })
    }

    /// Creates a GPU-backed `AdaptiveRenderer` sharing an existing [`GpuContext`] without
    /// allocating an offscreen framebuffer until [`Self::render_scene`] is explicitly called.
    pub fn from_shared_gpu_context(
        context: Arc<GpuContext>,
        width: u32,
        height: u32,
        limits: GpuResourceLimits,
    ) -> Self {
        let backend: Box<dyn RendererBackend> = Box::new(WgpuRenderer::from_context(
            context,
            wgpu::TextureFormat::Rgba8Unorm,
            width,
            height,
            limits,
        ));
        Self { backend }
    }

    pub fn new(
        width: u32,
        height: u32,
        preference: RenderBackendPreference,
        limits: GpuResourceLimits,
    ) -> Self {
        Self::try_new(width, height, preference, limits).unwrap_or_else(|_| {
            Self::try_new(
                width,
                height,
                RenderBackendPreference::ForceTinySkia,
                limits,
            )
            .expect("TinySkia fallback never fails")
        })
    }

    pub fn backend_kind(&self) -> ActiveBackendKind {
        self.backend.backend_kind()
    }

    /// Returns the shared `Arc<GpuContext>` if the active backend is `WgpuRenderer`.
    pub fn gpu_context(&self) -> Option<Arc<GpuContext>> {
        self.backend.gpu_context()
    }

    /// Returns `true` if the active backend currently holds an allocated offscreen target.
    pub fn has_offscreen_target(&self) -> bool {
        self.backend.has_offscreen_target()
    }

    pub fn render_scene(&mut self, scene: &PaintScene) -> FrameRenderStats {
        self.backend.render_scene(scene)
    }

    /// Waits for all submitted GPU work on the active backend to complete.
    pub fn finish_gpu_work(&mut self) {
        self.backend.finish_gpu_work();
    }

    /// Returns the physical `(width, height)` dimensions of the latest rendered framebuffer.
    pub fn framebuffer_dimensions(&self) -> Option<(u32, u32)> {
        self.backend.framebuffer_dimensions()
    }

    pub fn framebuffer_rgba(&mut self) -> Option<&[u8]> {
        self.backend.framebuffer_rgba()
    }
}
