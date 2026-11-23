//! Multi-page `DualGlyphAtlas` (`R8Unorm` 2DArray + premultiplied `Rgba8Unorm` 2DArray)
//! with LRU layer eviction so glyphs are never silently dropped when an atlas page fills up.

use std::collections::HashMap;

/// Side dimension (in pixels) of each layer in [`DualGlyphAtlas`].
pub const DUAL_ATLAS_SIZE: u32 = 1024;
/// Number of `1024x1024` `R8Unorm` alpha mask layers (`6 MB` total).
pub const MASK_ATLAS_LAYERS: u32 = 6;
/// Number of `1024x1024` `Rgba8Unorm` color emoji layers (`16 MB` total).
pub const COLOR_ATLAS_LAYERS: u32 = 4;

/// Atlas UV rectangle and array layer index for a packed glyph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphSlot {
    /// Normalized `[u0, v0, u1, v1]` inside `0..1`.
    pub uv: [f32; 4],
    /// `2DArray` layer index (`0..MASK_ATLAS_LAYERS` or `0..COLOR_ATLAS_LAYERS`).
    pub layer: u32,
    /// `true` when packed in the `Rgba8Unorm` color array, `false` for `R8Unorm`.
    pub is_color: bool,
}

/// Outcome when [`DualGlyphAtlas::try_pack_glyph`] cannot immediately pack a glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasPackError {
    /// Glyph has zero dimensions or exceeds the single-page `1024x1024` bound.
    InvalidOrOversized,
    /// All layers of the target array are full; caller must flush/submit GPU work, call
    /// [`DualGlyphAtlas::evict_oldest_page`], and retry.
    AllLayersFull,
}

struct AtlasLayerPage {
    pixels: Vec<u8>,
    bytes_per_px: u32,
    reserve_white_corner: bool,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    dirty_min_y: u32,
    dirty_max_y: u32,
    uploaded_rows: u32,
    last_used_epoch: u64,
}

impl AtlasLayerPage {
    fn new(bytes_per_px: u32, reserve_white_corner: bool) -> Self {
        let len = (DUAL_ATLAS_SIZE * DUAL_ATLAS_SIZE * bytes_per_px) as usize;
        let mut pixels = vec![0_u8; len];
        let (cursor_x, row_height, dirty_max_y) = if reserve_white_corner {
            fill_white_4x4(&mut pixels, bytes_per_px);
            (6, 6, 6)
        } else {
            (0, 0, 0)
        };
        Self {
            pixels,
            bytes_per_px,
            reserve_white_corner,
            cursor_x,
            cursor_y: 0,
            row_height,
            dirty_min_y: 0,
            dirty_max_y,
            uploaded_rows: 0,
            last_used_epoch: 0,
        }
    }

    fn reset(&mut self, epoch: u64) {
        let rows = self
            .dirty_max_y
            .max(self.uploaded_rows)
            .min(DUAL_ATLAS_SIZE);
        let clear_bytes = (rows * DUAL_ATLAS_SIZE * self.bytes_per_px) as usize;
        self.pixels[..clear_bytes].fill(0);
        if self.reserve_white_corner {
            fill_white_4x4(&mut self.pixels, self.bytes_per_px);
            self.cursor_x = 6;
            self.cursor_y = 0;
            self.row_height = 6;
            self.dirty_min_y = 0;
            self.dirty_max_y = 6;
        } else {
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.row_height = 0;
            self.dirty_min_y = 0;
            self.dirty_max_y = 0;
        }
        self.uploaded_rows = 0;
        self.last_used_epoch = epoch;
    }

    fn try_allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let mut next_x = self.cursor_x;
        let mut next_y = self.cursor_y;
        let mut next_row_h = self.row_height;
        if next_x + w + 2 > DUAL_ATLAS_SIZE {
            next_x = 0;
            next_y = next_y + next_row_h + 2;
            next_row_h = 0;
        }
        if next_y + h + 2 > DUAL_ATLAS_SIZE {
            return None;
        }
        self.cursor_x = next_x + w + 2;
        self.cursor_y = next_y;
        self.row_height = next_row_h.max(h);
        let bottom = (next_y + h + 2).min(DUAL_ATLAS_SIZE);
        if self.dirty_max_y <= self.dirty_min_y {
            self.dirty_min_y = next_y;
            self.dirty_max_y = bottom;
        } else {
            self.dirty_min_y = self.dirty_min_y.min(next_y);
            self.dirty_max_y = self.dirty_max_y.max(bottom);
        }
        Some((next_x + 1, next_y + 1))
    }
}

fn fill_white_4x4(pixels: &mut [u8], bpp: u32) {
    for y in 0..4 {
        let start = (y * DUAL_ATLAS_SIZE * bpp) as usize;
        let len = (4 * bpp) as usize;
        pixels[start..start + len].fill(255);
    }
}

fn fnv1a_glyph_hash(w: u32, h: u32, is_color: bool, data: &[u8]) -> u64 {
    let mut hash: u64 = if is_color {
        0xcbf2_9ce4_8422_2325 ^ 0x9e37_79b9
    } else {
        0xcbf2_9ce4_8422_2325
    };
    for &b in w.to_le_bytes().iter().chain(h.to_le_bytes().iter()) {
        hash = (hash ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    for &b in data {
        hash = (hash ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Multi-page `R8Unorm` + `Rgba8Unorm` texture-array glyph packer and LRU cache.
pub struct DualGlyphAtlas {
    mask_pages: Vec<AtlasLayerPage>,
    color_pages: Vec<AtlasLayerPage>,
    mask_cache: HashMap<u64, GlyphSlot>,
    color_cache: HashMap<u64, GlyphSlot>,
    epoch: u64,
    evictions: u64,
}

impl Default for DualGlyphAtlas {
    fn default() -> Self {
        Self::new()
    }
}

impl DualGlyphAtlas {
    /// Creates a new `DualGlyphAtlas` with `MASK_ATLAS_LAYERS` `R8` pages and `COLOR_ATLAS_LAYERS` `RGBA8` pages.
    pub fn new() -> Self {
        let mask_pages = (0..MASK_ATLAS_LAYERS)
            .map(|idx| AtlasLayerPage::new(1, idx == 0))
            .collect();
        let color_pages = (0..COLOR_ATLAS_LAYERS)
            .map(|_| AtlasLayerPage::new(4, false))
            .collect();
        Self {
            mask_pages,
            color_pages,
            mask_cache: HashMap::with_capacity(1024),
            color_cache: HashMap::with_capacity(256),
            epoch: 1,
            evictions: 0,
        }
    }

    /// Advances the LRU usage epoch at the start of a frame or mid-frame flush.
    pub fn advance_epoch(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// Total number of cached monochrome + color glyphs currently resident across all layers.
    pub fn cached_glyph_count(&self) -> usize {
        self.mask_cache.len() + self.color_cache.len()
    }

    /// Total number of LRU layer evictions performed since creation.
    pub fn eviction_count(&self) -> u64 {
        self.evictions
    }

    /// Looks up or packs a glyph (`R8` mask or straight `RGBA8` color bitmap) into an available layer.
    pub fn try_pack_glyph(
        &mut self,
        cache_key: u64,
        w: u32,
        h: u32,
        is_color: bool,
        raw_bytes: &[u8],
    ) -> Result<GlyphSlot, AtlasPackError> {
        if w == 0 || h == 0 || w + 4 > DUAL_ATLAS_SIZE || h + 4 > DUAL_ATLAS_SIZE {
            return Err(AtlasPackError::InvalidOrOversized);
        }
        let key = if cache_key != 0 {
            cache_key
        } else {
            fnv1a_glyph_hash(w, h, is_color, raw_bytes)
        };
        if let Some(slot) = self.lookup_cached_slot(key, is_color) {
            return Ok(slot);
        }
        self.allocate_and_blit_slot(key, w, h, is_color, raw_bytes)
    }

    fn lookup_cached_slot(&mut self, key: u64, is_color: bool) -> Option<GlyphSlot> {
        let slot = if is_color {
            *self.color_cache.get(&key)?
        } else {
            *self.mask_cache.get(&key)?
        };
        let pages = if is_color {
            &mut self.color_pages
        } else {
            &mut self.mask_pages
        };
        if let Some(p) = pages.get_mut(slot.layer as usize) {
            p.last_used_epoch = self.epoch;
        }
        Some(slot)
    }

    fn allocate_and_blit_slot(
        &mut self,
        key: u64,
        w: u32,
        h: u32,
        is_color: bool,
        raw_bytes: &[u8],
    ) -> Result<GlyphSlot, AtlasPackError> {
        let pages = if is_color {
            &mut self.color_pages
        } else {
            &mut self.mask_pages
        };
        for (layer_idx, page) in pages.iter_mut().enumerate() {
            let Some((gx, gy)) = page.try_allocate(w, h) else {
                continue;
            };
            page.last_used_epoch = self.epoch;
            if is_color {
                blit_premultiplied_rgba(&mut page.pixels, gx, gy, w, h, raw_bytes);
            } else {
                blit_mask_r8(&mut page.pixels, gx, gy, w, h, raw_bytes);
            }
            let inv = 1.0 / DUAL_ATLAS_SIZE as f32;
            let slot = GlyphSlot {
                uv: [
                    gx as f32 * inv,
                    gy as f32 * inv,
                    (gx + w) as f32 * inv,
                    (gy + h) as f32 * inv,
                ],
                layer: layer_idx as u32,
                is_color,
            };
            if is_color {
                self.color_cache.insert(key, slot);
            } else {
                self.mask_cache.insert(key, slot);
            }
            return Ok(slot);
        }
        Err(AtlasPackError::AllLayersFull)
    }

    /// Evicts and clears the least-recently-used layer in the target array, returning the evicted layer index.
    pub fn evict_oldest_page(&mut self, is_color: bool) -> u32 {
        self.epoch = self.epoch.wrapping_add(1);
        self.evictions += 1;
        let pages = if is_color {
            &mut self.color_pages
        } else {
            &mut self.mask_pages
        };
        let victim = pages
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| p.last_used_epoch)
            .map(|(i, _)| i)
            .unwrap_or(0);
        pages[victim].reset(self.epoch);
        let victim_u32 = victim as u32;
        if is_color {
            self.color_cache.retain(|_, s| s.layer != victim_u32);
        } else {
            self.mask_cache.retain(|_, s| s.layer != victim_u32);
        }
        victim_u32
    }

    /// Uploads all dirty shelf rows across both `R8` and `RGBA8` `2DArray` GPU textures.
    pub fn upload_dirty_pages(
        &mut self,
        queue: &wgpu::Queue,
        mask_tex: &wgpu::Texture,
        color_tex: &wgpu::Texture,
    ) {
        for (layer, page) in self.mask_pages.iter_mut().enumerate() {
            upload_layer_rows(queue, mask_tex, layer as u32, page);
        }
        for (layer, page) in self.color_pages.iter_mut().enumerate() {
            upload_layer_rows(queue, color_tex, layer as u32, page);
        }
    }
}

fn blit_mask_r8(dst: &mut [u8], gx: u32, gy: u32, w: u32, h: u32, src: &[u8]) {
    for row in 0..h {
        let src_off = (row * w) as usize;
        let dst_off = ((gy + row) * DUAL_ATLAS_SIZE + gx) as usize;
        let copy_len = (src_off + w as usize)
            .min(src.len())
            .saturating_sub(src_off);
        if copy_len > 0 {
            dst[dst_off..dst_off + copy_len].copy_from_slice(&src[src_off..src_off + copy_len]);
        }
    }
}

fn blit_premultiplied_rgba(dst: &mut [u8], gx: u32, gy: u32, w: u32, h: u32, src: &[u8]) {
    for row in 0..h {
        for col in 0..w {
            let s_idx = ((row * w + col) * 4) as usize;
            let Some(px) = src.get(s_idx..s_idx + 4) else {
                break;
            };
            let a = px[3] as u16;
            let d_idx = (((gy + row) * DUAL_ATLAS_SIZE + gx + col) * 4) as usize;
            dst[d_idx] = ((px[0] as u16 * a + 127) / 255) as u8;
            dst[d_idx + 1] = ((px[1] as u16 * a + 127) / 255) as u8;
            dst[d_idx + 2] = ((px[2] as u16 * a + 127) / 255) as u8;
            dst[d_idx + 3] = px[3];
        }
    }
}

fn upload_layer_rows(
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    layer: u32,
    page: &mut AtlasLayerPage,
) {
    if page.dirty_max_y <= page.dirty_min_y {
        return;
    }
    let y0 = page.dirty_min_y.min(DUAL_ATLAS_SIZE - 1);
    let y1 = page.dirty_max_y.clamp(y0 + 1, DUAL_ATLAS_SIZE);
    let rows = y1 - y0;
    let row_bytes = DUAL_ATLAS_SIZE * page.bytes_per_px;
    let start = (y0 * row_bytes) as usize;
    let end = (y1 * row_bytes) as usize;
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: 0,
                y: y0,
                z: layer,
            },
            aspect: wgpu::TextureAspect::All,
        },
        &page.pixels[start..end],
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(row_bytes),
            rows_per_image: Some(rows),
        },
        wgpu::Extent3d {
            width: DUAL_ATLAS_SIZE,
            height: rows,
            depth_or_array_layers: 1,
        },
    );
    page.uploaded_rows = page.uploaded_rows.max(y1);
    page.dirty_min_y = 0;
    page.dirty_max_y = 0;
}
