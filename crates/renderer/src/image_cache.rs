//! Bounded GPU LRU cache for decoded `<img>` RGBA8 textures (`ImageGpuCache`) and host
//! `ExternalTextureRegistry` mapping [`ExternalTextureId`] -> external `wgpu::TextureView`s.

use krui_core::{DecodedImage, ExternalTextureId, ImageId};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

const MAX_IMAGE_CACHE_ENTRIES: usize = 128;
const MAX_IMAGE_CACHE_BYTES: usize = 128 * 1024 * 1024;
static SRGB_TO_LINEAR_LUT: OnceLock<[f32; 256]> = OnceLock::new();

fn srgb_lut() -> &'static [f32; 256] {
    SRGB_TO_LINEAR_LUT.get_or_init(|| {
        let mut lut = [0.0_f32; 256];
        for (i, slot) in lut.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            *slot = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
        }
        lut
    })
}

#[inline]
fn linear_to_srgb_u8(lin: f32) -> f32 {
    let x = lin.clamp(0.0, 1.0);
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

struct CachedGpuImage {
    id: ImageId,
    _tex: wgpu::Texture,
    aux_bg: Arc<wgpu::BindGroup>,
    byte_size: usize,
    last_used_tick: u64,
}

/// Bounded LRU GPU texture cache for raster `<img>` [`DecodedImage`] payloads.
#[derive(Default)]
pub(crate) struct ImageGpuCache {
    entries: Vec<CachedGpuImage>,
    total_bytes: usize,
    tick: u64,
}

impl ImageGpuCache {
    /// Uploads or returns a cached `BindGroup` (matching `aux_bgl`) for `img`.
    pub(crate) fn get_or_upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        aux_bgl: &wgpu::BindGroupLayout,
        img: &DecodedImage,
    ) -> Arc<wgpu::BindGroup> {
        self.tick = self.tick.wrapping_add(1);
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == img.id) {
            entry.last_used_tick = self.tick;
            return Arc::clone(&entry.aux_bg);
        }
        let (tex, byte_size) = upload_premultiplied_rgba8_texture(device, queue, img);
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let aux_bg = Arc::new(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("KRui Image Aux BindGroup"),
            layout: aux_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        }));
        while !self.entries.is_empty()
            && (self.entries.len() >= MAX_IMAGE_CACHE_ENTRIES
                || self.total_bytes.saturating_add(byte_size) > MAX_IMAGE_CACHE_BYTES)
        {
            if let Some((victim, _)) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_used_tick)
            {
                let evicted = self.entries.swap_remove(victim);
                self.total_bytes = self.total_bytes.saturating_sub(evicted.byte_size);
            } else {
                break;
            }
        }
        self.total_bytes = self.total_bytes.saturating_add(byte_size);
        self.entries.push(CachedGpuImage {
            id: img.id,
            _tex: tex,
            aux_bg: Arc::clone(&aux_bg),
            byte_size,
            last_used_tick: self.tick,
        });
        aux_bg
    }
}

fn premultiply_px(px: &[u8; 4]) -> [u8; 4] {
    let a = px[3] as u16;
    if a == 255 {
        *px
    } else if a == 0 {
        [0, 0, 0, 0]
    } else {
        [
            ((px[0] as u16 * a + 127) / 255) as u8,
            ((px[1] as u16 * a + 127) / 255) as u8,
            ((px[2] as u16 * a + 127) / 255) as u8,
            a as u8,
        ]
    }
}

fn prepare_upload_pixels(img: &DecodedImage, max_dim: u32) -> (u32, u32, Vec<u8>) {
    let src_w = img.width.max(1);
    let src_h = img.height.max(1);
    let expected = (src_w as usize)
        .saturating_mul(src_h as usize)
        .saturating_mul(4);
    if img.rgba8.len() < expected {
        return (1, 1, vec![0, 0, 0, 0]);
    }
    if src_w <= max_dim && src_h <= max_dim {
        let mut pm = Vec::with_capacity(expected);
        for px in img.rgba8[..expected].as_chunks::<4>().0 {
            pm.extend_from_slice(&premultiply_px(px));
        }
        return (src_w, src_h, pm);
    }
    downsample_oversized_rgba8(&img.rgba8, src_w, src_h, max_dim)
}

fn downsample_oversized_rgba8(
    rgba8: &[u8],
    src_w: u32,
    src_h: u32,
    max_dim: u32,
) -> (u32, u32, Vec<u8>) {
    let scale = (max_dim as f64 / src_w as f64).min(max_dim as f64 / src_h as f64);
    let dst_w = ((src_w as f64 * scale).round() as u32).clamp(1, max_dim);
    let dst_h = ((src_h as f64 * scale).round() as u32).clamp(1, max_dim);
    let lut = srgb_lut();
    let mut pm = Vec::with_capacity((dst_w as usize) * (dst_h as usize) * 4);
    for dy in 0..dst_h {
        let sy0 = ((dy as u64 * src_h as u64) / dst_h as u64) as usize;
        let sy1 = (((dy as u64 + 1) * src_h as u64) / dst_h as u64)
            .clamp(sy0 as u64 + 1, src_h as u64) as usize;
        for dx in 0..dst_w {
            let sx0 = ((dx as u64 * src_w as u64) / dst_w as u64) as usize;
            let sx1 = (((dx as u64 + 1) * src_w as u64) / dst_w as u64)
                .clamp(sx0 as u64 + 1, src_w as u64) as usize;
            pm.extend_from_slice(&average_straight_box(
                rgba8,
                src_w as usize,
                (sx0, sx1),
                (sy0, sy1),
                lut,
            ));
        }
    }
    (dst_w, dst_h, pm)
}

fn average_straight_box(
    rgba8: &[u8],
    stride_px: usize,
    (sx0, sx1): (usize, usize),
    (sy0, sy1): (usize, usize),
    lut: &[f32; 256],
) -> [u8; 4] {
    let (mut lr, mut lg, mut lb, mut la, mut cnt) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
    for sy in sy0..sy1 {
        for sx in sx0..sx1 {
            let i = (sy * stride_px + sx) * 4;
            let a = rgba8[i + 3] as f32 / 255.0;
            lr += lut[rgba8[i] as usize] * a;
            lg += lut[rgba8[i + 1] as usize] * a;
            lb += lut[rgba8[i + 2] as usize] * a;
            la += a;
            cnt += 1.0;
        }
    }
    let inv = 1.0 / cnt.max(1.0);
    encode_premul_srgb_from_linear(lr * inv, lg * inv, lb * inv, la * inv)
}

#[inline]
fn encode_premul_srgb_from_linear(lr: f32, lg: f32, lb: f32, a_avg: f32) -> [u8; 4] {
    if a_avg <= 1e-6 {
        return [0, 0, 0, 0];
    }
    let a = a_avg.clamp(0.0, 1.0);
    let r_s = linear_to_srgb_u8(lr / a);
    let g_s = linear_to_srgb_u8(lg / a);
    let b_s = linear_to_srgb_u8(lb / a);
    [
        (r_s * a * 255.0).round().clamp(0.0, 255.0) as u8,
        (g_s * a * 255.0).round().clamp(0.0, 255.0) as u8,
        (b_s * a * 255.0).round().clamp(0.0, 255.0) as u8,
        (a * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

fn downsample_premul_srgb_2x2(src: &[u8], w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let lut = srgb_lut();
    let mut dst = Vec::with_capacity((nw as usize) * (nh as usize) * 4);
    for dy in 0..nh {
        let sy0 = (dy * 2).min(h - 1) as usize;
        let sy1 = (dy * 2 + 1).min(h - 1) as usize;
        for dx in 0..nw {
            let sx0 = (dx * 2).min(w - 1) as usize;
            let sx1 = (dx * 2 + 1).min(w - 1) as usize;
            let mut sum = [0.0_f32; 4];
            for sy in [sy0, sy1] {
                for sx in [sx0, sx1] {
                    let i = (sy * w as usize + sx) * 4;
                    let a_u8 = src[i + 3];
                    if a_u8 > 0 {
                        let a = a_u8 as f32 / 255.0;
                        let inv_a = 255.0 / a_u8 as f32;
                        let ch = |v: u8| lut[((v as f32 * inv_a).round() as usize).min(255)] * a;
                        sum[0] += ch(src[i]);
                        sum[1] += ch(src[i + 1]);
                        sum[2] += ch(src[i + 2]);
                        sum[3] += a;
                    }
                }
            }
            dst.extend_from_slice(&encode_premul_srgb_from_linear(
                sum[0] * 0.25,
                sum[1] * 0.25,
                sum[2] * 0.25,
                sum[3] * 0.25,
            ));
        }
    }
    (nw, nh, dst)
}

fn upload_mip_level(
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    mip_level: u32,
    w: u32,
    h: u32,
    bytes: &[u8],
) {
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: tex,
            mip_level,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
}

fn upload_premultiplied_rgba8_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    img: &DecodedImage,
) -> (wgpu::Texture, usize) {
    let max_dim = device.limits().max_texture_dimension_2d.max(1);
    let (mut cur_w, mut cur_h, mut cur_bytes) = prepare_upload_pixels(img, max_dim);
    let mip_level_count = cur_w.max(cur_h).ilog2() + 1;
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("KRui Cached Image Texture"),
        size: wgpu::Extent3d {
            width: cur_w,
            height: cur_h,
            depth_or_array_layers: 1,
        },
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut total_bytes = cur_bytes.len();
    upload_mip_level(queue, &tex, 0, cur_w, cur_h, &cur_bytes);
    for mip in 1..mip_level_count {
        let (nw, nh, next_bytes) = downsample_premul_srgb_2x2(&cur_bytes, cur_w, cur_h);
        total_bytes = total_bytes.saturating_add(next_bytes.len());
        upload_mip_level(queue, &tex, mip, nw, nh, &next_bytes);
        cur_w = nw;
        cur_h = nh;
        cur_bytes = next_bytes;
    }
    (tex, total_bytes)
}

/// Host registry mapping [`ExternalTextureId`] slots to `wgpu::BindGroup`s bound to `aux_bgl`.
#[derive(Default)]
pub(crate) struct ExternalTextureRegistry {
    slots: HashMap<ExternalTextureId, Arc<wgpu::BindGroup>>,
}

impl ExternalTextureRegistry {
    pub(crate) fn register(
        &mut self,
        device: &wgpu::Device,
        aux_bgl: &wgpu::BindGroupLayout,
        id: ExternalTextureId,
        view: &wgpu::TextureView,
    ) {
        let bg = Arc::new(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("KRui ExternalTexture BindGroup"),
            layout: aux_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            }],
        }));
        self.slots.insert(id, bg);
    }

    pub(crate) fn unregister(&mut self, id: ExternalTextureId) {
        self.slots.remove(&id);
    }

    pub(crate) fn get(&self, id: ExternalTextureId) -> Option<Arc<wgpu::BindGroup>> {
        self.slots.get(&id).cloned()
    }
}
