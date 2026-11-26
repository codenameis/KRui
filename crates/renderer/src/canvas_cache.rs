//! Bounded LRU mesh cache for `DrawPrimitive::CanvasBatch` so static `<canvas>` / `<svg>`
//! vector paths avoid CPU `lyon` re-tessellation on subsequent frames and stay ABA-safe via `Weak`.

use crate::tessellate::{tessellate_canvas_commands_standalone, GpuVertex};
use crate::tiny_skia_backend::tiny_skia_canvas::rasterize_canvas_batch_to_decoded;
use krui_core::{CanvasCommand, DecodedImage, GpuResourceLimits};
use krui_layout::LayoutRect;
use krui_style::Affine2;
use std::sync::{Arc, Weak};

const MAX_CANVAS_CACHE_ENTRIES: usize = 64;
const MAX_CLIPPED_RASTER_ENTRIES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct CanvasMeshKey {
    commands_len: usize,
    q_viewport: [i32; 4],
    q_logical_vp: [i32; 2],
    q_transform: [i32; 6],
    q_clip: Option<[i32; 4]>,
    q_clip_radius: i32,
    q_surface_scale: i32,
    q_opacity: u16,
}

struct CachedCanvasEntry {
    key: CanvasMeshKey,
    commands_weak: Weak<[CanvasCommand]>,
    mesh: Arc<(Vec<GpuVertex>, Vec<u32>, bool)>,
    last_used_tick: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ClippedRasterKey {
    commands_len: usize,
    q_size: [i32; 2],
    q_surface_scale: i32,
}

struct CachedClippedRaster {
    key: ClippedRasterKey,
    commands_weak: Weak<[CanvasCommand]>,
    image: DecodedImage,
    last_used_tick: u64,
}

/// Bounded LRU cache of `lyon`-tessellated `<canvas>` command buffers.
pub struct CanvasMeshCache {
    entries: Vec<CachedCanvasEntry>,
    clipped_rasters: Vec<CachedClippedRaster>,
    tick: u64,
    hits: u64,
    misses: u64,
}

impl Default for CanvasMeshCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CanvasMeshCache {
    /// Creates an empty bounded `<canvas>` mesh cache.
    pub fn new() -> Self {
        Self {
            entries: Vec::with_capacity(MAX_CANVAS_CACHE_ENTRIES),
            clipped_rasters: Vec::with_capacity(MAX_CLIPPED_RASTER_ENTRIES),
            tick: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// Rasterizes and caches a `<canvas>` batch using `PushClip` into a `DecodedImage` for `wgpu` compositing.
    pub fn get_or_rasterize_clipped(
        &mut self,
        commands: &Arc<[CanvasCommand]>,
        viewport: LayoutRect,
        surface_scale: f32,
    ) -> Option<DecodedImage> {
        self.tick = self.tick.wrapping_add(1);
        let key = ClippedRasterKey {
            commands_len: commands.len(),
            q_size: [
                quantize_f32(viewport.width, 16.0),
                quantize_f32(viewport.height, 16.0),
            ],
            q_surface_scale: quantize_f32(surface_scale, 64.0),
        };
        let probe_weak = Arc::downgrade(commands);
        if let Some(entry) = self.clipped_rasters.iter_mut().find(|e| {
            e.key == key
                && e.commands_weak.strong_count() > 0
                && Weak::ptr_eq(&e.commands_weak, &probe_weak)
        }) {
            entry.last_used_tick = self.tick;
            self.hits += 1;
            return Some(entry.image.clone());
        }
        self.misses += 1;
        let image = rasterize_canvas_batch_to_decoded(viewport, surface_scale, commands)?;
        if self.clipped_rasters.len() >= MAX_CLIPPED_RASTER_ENTRIES {
            let dead = self
                .clipped_rasters
                .iter()
                .position(|e| e.commands_weak.strong_count() == 0);
            let victim = dead.or_else(|| {
                self.clipped_rasters
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, e)| e.last_used_tick)
                    .map(|(i, _)| i)
            });
            if let Some(idx) = victim {
                self.clipped_rasters.swap_remove(idx);
            }
        }
        self.clipped_rasters.push(CachedClippedRaster {
            key,
            commands_weak: probe_weak,
            image: image.clone(),
            last_used_tick: self.tick,
        });
        Some(image)
    }

    /// Returns `(cache_hits, cache_misses)` for telemetry and deterministic cache-hit tests.
    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }

    /// Looks up or tessellates `commands` for `(viewport, clip, clip_radius, transform, opacity, scale)`.
    #[allow(clippy::too_many_arguments)]
    pub fn get_or_tessellate(
        &mut self,
        commands: &Arc<[CanvasCommand]>,
        viewport: LayoutRect,
        clip: Option<LayoutRect>,
        clip_radius: f32,
        surface_scale: f32,
        transform: Affine2,
        opacity: f32,
        (logical_vw, logical_vh): (f32, f32),
        limits: GpuResourceLimits,
    ) -> Arc<(Vec<GpuVertex>, Vec<u32>, bool)> {
        self.tick = self.tick.wrapping_add(1);
        let key = build_canvas_key(
            commands.len(),
            viewport,
            clip,
            clip_radius,
            surface_scale,
            transform,
            opacity,
            logical_vw,
            logical_vh,
        );
        let probe_weak = Arc::downgrade(commands);
        if let Some(entry) = self.entries.iter_mut().find(|e| {
            e.key == key
                && e.commands_weak.strong_count() > 0
                && Weak::ptr_eq(&e.commands_weak, &probe_weak)
        }) {
            entry.last_used_tick = self.tick;
            self.hits += 1;
            return entry.mesh.clone();
        }

        self.misses += 1;
        let tess = tessellate_canvas_commands_standalone(
            commands,
            viewport,
            transform,
            opacity,
            surface_scale,
            (logical_vw, logical_vh),
            limits,
        );
        let arc_mesh = Arc::new(tess);
        self.insert_evicting_lru(key, probe_weak, arc_mesh.clone());
        arc_mesh
    }

    fn insert_evicting_lru(
        &mut self,
        key: CanvasMeshKey,
        commands_weak: Weak<[CanvasCommand]>,
        mesh: Arc<(Vec<GpuVertex>, Vec<u32>, bool)>,
    ) {
        if self.entries.len() >= MAX_CANVAS_CACHE_ENTRIES {
            let dead = self
                .entries
                .iter()
                .position(|e| e.commands_weak.strong_count() == 0);
            let victim = dead.or_else(|| {
                self.entries
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, e)| e.last_used_tick)
                    .map(|(i, _)| i)
            });
            if let Some(victim_idx) = victim {
                self.entries.swap_remove(victim_idx);
            }
        }
        self.entries.push(CachedCanvasEntry {
            key,
            commands_weak,
            mesh,
            last_used_tick: self.tick,
        });
    }
}

fn quantize_f32(v: f32, scale: f32) -> i32 {
    if v.is_finite() {
        (v * scale).round() as i32
    } else {
        0
    }
}

#[allow(clippy::too_many_arguments)]
fn build_canvas_key(
    commands_len: usize,
    viewport: LayoutRect,
    clip: Option<LayoutRect>,
    clip_radius: f32,
    surface_scale: f32,
    transform: Affine2,
    opacity: f32,
    logical_vw: f32,
    logical_vh: f32,
) -> CanvasMeshKey {
    CanvasMeshKey {
        commands_len,
        q_viewport: [
            quantize_f32(viewport.x, 16.0),
            quantize_f32(viewport.y, 16.0),
            quantize_f32(viewport.width, 16.0),
            quantize_f32(viewport.height, 16.0),
        ],
        q_logical_vp: [
            quantize_f32(logical_vw, 16.0),
            quantize_f32(logical_vh, 16.0),
        ],
        q_transform: [
            quantize_f32(transform.a, 1024.0),
            quantize_f32(transform.b, 1024.0),
            quantize_f32(transform.c, 1024.0),
            quantize_f32(transform.d, 1024.0),
            quantize_f32(transform.tx, 16.0),
            quantize_f32(transform.ty, 16.0),
        ],
        q_clip: clip.map(|c| {
            [
                quantize_f32(c.x, 16.0),
                quantize_f32(c.y, 16.0),
                quantize_f32(c.width, 16.0),
                quantize_f32(c.height, 16.0),
            ]
        }),
        q_clip_radius: quantize_f32(clip_radius, 16.0),
        q_surface_scale: quantize_f32(surface_scale, 64.0),
        q_opacity: (opacity.clamp(0.0, 1.0) * 1000.0).round() as u16,
    }
}
