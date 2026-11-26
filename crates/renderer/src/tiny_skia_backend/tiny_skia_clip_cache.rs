//! Reusable `ClipMaskCache` with dirty-row clearing and interior-unclipped bounding checks
//! for `TinySkiaRenderer`.

use super::tiny_skia_shapes::build_skia_round_rect_4;
use crate::radii::effective_radii_4;
use krui_layout::LayoutRect;

const MAX_CACHED_CLIP_MASKS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
struct ClipMaskKey {
    w: u32,
    h: u32,
    q_rect: [i32; 4],
    q_radii: [i32; 4],
}

struct CachedClipMask {
    key: ClipMaskKey,
    mask: tiny_skia::Mask,
    dirty_rows: (u32, u32),
    last_tick: u64,
}

#[derive(Default)]
pub(super) struct ClipMaskCache {
    entries: Vec<CachedClipMask>,
    tick: u64,
    hits: u64,
    allocations: u64,
}

impl ClipMaskCache {
    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.allocations)
    }

    pub fn get_or_build_4(
        &mut self,
        w: u32,
        h: u32,
        clip: Option<LayoutRect>,
        clip_radius: f32,
        clip_radii: [f32; 4],
        scale: f32,
    ) -> Option<&tiny_skia::Mask> {
        let rect = clip?;
        self.tick = self.tick.wrapping_add(1);
        let radii_phys = effective_radii_4(clip_radius, clip_radii).map(|r| (r * scale).max(0.0));
        let key = ClipMaskKey {
            w,
            h,
            q_rect: [
                (rect.x * scale * 16.0).round() as i32,
                (rect.y * scale * 16.0).round() as i32,
                (rect.width * scale * 16.0).round() as i32,
                (rect.height * scale * 16.0).round() as i32,
            ],
            q_radii: radii_phys.map(|r| (r * 16.0).round() as i32),
        };
        if let Some(pos) = self.entries.iter().position(|e| e.key == key) {
            self.entries[pos].last_tick = self.tick;
            self.hits += 1;
            return Some(&self.entries[pos].mask);
        }
        let mut mask = self.acquire_mask_slot(w, h)?;
        let (cw, ch) = (
            (rect.width * scale).max(0.0),
            (rect.height * scale).max(0.0),
        );
        let y0 = ((rect.y * scale).floor() as i32 - 2).clamp(0, h as i32) as u32;
        let y1 = (((rect.y + rect.height) * scale).ceil() as i32 + 2).clamp(0, h as i32) as u32;
        if cw > 0.0 && ch > 0.0 {
            if let Some(path) =
                build_skia_round_rect_4(rect.x * scale, rect.y * scale, cw, ch, radii_phys)
            {
                mask.fill_path(
                    &path,
                    tiny_skia::FillRule::Winding,
                    true,
                    tiny_skia::Transform::identity(),
                );
            }
        }
        self.entries.push(CachedClipMask {
            key,
            mask,
            dirty_rows: (y0, y1.max(y0)),
            last_tick: self.tick,
        });
        self.entries.last().map(|e| &e.mask)
    }

    fn acquire_mask_slot(&mut self, w: u32, h: u32) -> Option<tiny_skia::Mask> {
        if self.entries.len() >= MAX_CACHED_CLIP_MASKS {
            let victim = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_tick)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let mut old_entry = self.entries.swap_remove(victim);
            if old_entry.mask.width() == w && old_entry.mask.height() == h {
                let (dy0, dy1) = old_entry.dirty_rows;
                let row_start = (dy0.min(h) as usize) * (w as usize);
                let row_end = (dy1.min(h) as usize) * (w as usize);
                if row_end > row_start {
                    old_entry.mask.data_mut()[row_start..row_end].fill(0);
                }
                return Some(old_entry.mask);
            }
        }
        self.allocations += 1;
        tiny_skia::Mask::new(w, h)
    }
}

/// Returns `true` when `box_aabb_phys` could partially overlap any clipped boundary or rounded corner.
pub(super) fn aabb_needs_clip_mask(
    clip: Option<LayoutRect>,
    clip_radius: f32,
    clip_radii: [f32; 4],
    (bx0, by0, bx1, by1): (f32, f32, f32, f32),
    scale: f32,
) -> bool {
    let Some(cr) = clip else {
        return false;
    };
    let cx0 = cr.x * scale;
    let cy0 = cr.y * scale;
    let cx1 = (cr.x + cr.width) * scale;
    let cy1 = (cr.y + cr.height) * scale;
    if bx0 < cx0 + 1.0 || by0 < cy0 + 1.0 || bx1 > cx1 - 1.0 || by1 > cy1 - 1.0 {
        return true;
    }
    let [tl, tr, br, bl] = effective_radii_4(clip_radius, clip_radii).map(|r| (r * scale).max(0.0));
    (tl > 0.05 && bx0 < cx0 + tl + 1.0 && by0 < cy0 + tl + 1.0)
        || (tr > 0.05 && bx1 > cx1 - tr - 1.0 && by0 < cy0 + tr + 1.0)
        || (br > 0.05 && bx1 > cx1 - br - 1.0 && by1 > cy1 - br - 1.0)
        || (bl > 0.05 && bx0 < cx0 + bl + 1.0 && by1 > cy1 - bl - 1.0)
}
