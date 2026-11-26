//! Bounded GPU LRU cache of 256x1 premultiplied RGBA8 gradient lookup textures (`t_aux`).

use crate::gpu_context::GpuContext;
use krui_style::{rasterize_lut_rgba8, GradientStopsRef};
use std::sync::Arc;

const LUT_WIDTH: u32 = 256;
const MAX_LUT_CACHE_ENTRIES: usize = 64;

struct CachedGradientLut {
    key: u64,
    _tex: wgpu::Texture,
    aux_bg: Arc<wgpu::BindGroup>,
    last_tick: u64,
}

/// Bounded LRU cache mapping normalized multi-stop / conic gradient ramps to `Rgba8Unorm` 256x1 textures.
#[derive(Default)]
pub(crate) struct GradientLutCache {
    entries: Vec<CachedGradientLut>,
    tick: u64,
}

impl GradientLutCache {
    /// Returns a cached `aux_bgl` bind group for `(stops, max_pos)` or uploads a 256x1 LUT texture.
    pub(crate) fn get_or_upload(
        &mut self,
        ctx: &GpuContext,
        stops: GradientStopsRef,
        max_pos: f32,
    ) -> Arc<wgpu::BindGroup> {
        self.tick = self.tick.wrapping_add(1);
        let span = max_pos.clamp(1e-4, 1.0);
        let key = stops.cache_key().wrapping_mul(0x0100_0000_01b3) ^ (span.to_bits() as u64);
        if let Some(pos) = self.entries.iter().position(|e| e.key == key) {
            self.entries[pos].last_tick = self.tick;
            return Arc::clone(&self.entries[pos].aux_bg);
        }
        let rgba = stops.with_stops(|s| rasterize_lut_rgba8(s, LUT_WIDTH as usize, span));
        let (tex, aux_bg) = upload_lut_texture(ctx, &rgba);
        if self.entries.len() >= MAX_LUT_CACHE_ENTRIES {
            if let Some((victim, _)) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_tick)
            {
                self.entries.swap_remove(victim);
            }
        }
        let ret = Arc::clone(&aux_bg);
        self.entries.push(CachedGradientLut {
            key,
            _tex: tex,
            aux_bg,
            last_tick: self.tick,
        });
        ret
    }
}

fn upload_lut_texture(ctx: &GpuContext, rgba: &[u8]) -> (wgpu::Texture, Arc<wgpu::BindGroup>) {
    let tex = ctx.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("KRui Gradient LUT 256x1"),
        size: wgpu::Extent3d {
            width: LUT_WIDTH,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    ctx.queue().write_texture(
        wgpu::ImageCopyTexture {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(LUT_WIDTH * 4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: LUT_WIDTH,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let bg = Arc::new(ctx.device().create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("KRui Gradient LUT BindGroup"),
        layout: &ctx.aux_bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
    }));
    (tex, bg)
}
