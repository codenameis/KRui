//! Bounded LRU `ShadowPatchCache` for axis-aligned `BoxShadow` rasterization in `TinySkiaRenderer`.

use super::tiny_skia_effects::{eval_inset_shadow_cov, eval_outer_shadow_cov, SkiaBoxEffectCtx};
use krui_style::BoxShadow;

const MAX_SHADOW_CACHE_ENTRIES: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
struct ShadowCacheKey {
    w_q: u32,
    h_q: u32,
    sx_q: u32,
    sy_q: u32,
    radii_q: [u32; 4],
    bw_q: u32,
    ox_q: i32,
    oy_q: i32,
    blur_q: u32,
    spread_q: i32,
    color: [u8; 4],
    inset: bool,
}

struct CachedShadowEntry {
    key: ShadowCacheKey,
    rx_px: f32,
    ry_px: f32,
    inner_x: i32,
    inner_y: i32,
    pixmap: tiny_skia::Pixmap,
}

/// Bounded LRU cache of pre-rasterized axis-aligned `BoxShadow` patches.
#[derive(Default)]
pub(super) struct ShadowPatchCache {
    entries: Vec<CachedShadowEntry>,
}

impl ShadowPatchCache {
    pub(super) fn try_draw_cached(
        &mut self,
        dst: &mut tiny_skia::Pixmap,
        ctx: &SkiaBoxEffectCtx<'_>,
        sh: BoxShadow,
    ) -> bool {
        let tx = ctx.scaled_transform;
        if tx.b.abs() > 1e-5 || tx.c.abs() > 1e-5 || tx.a <= 1e-4 || tx.d <= 1e-4 {
            return false;
        }
        let Some((key, rx_px, ry_px, pw, ph)) = build_key_and_metrics(ctx, sh) else {
            return false;
        };
        let idx = if let Some(pos) = self.entries.iter().position(|e| e.key == key) {
            let hit = self.entries.remove(pos);
            self.entries.push(hit);
            self.entries.len() - 1
        } else {
            let Some((patch, inner_x, inner_y)) =
                rasterize_shadow_patch(ctx, sh, (rx_px, ry_px), (pw, ph))
            else {
                return false;
            };
            if self.entries.len() >= MAX_SHADOW_CACHE_ENTRIES {
                self.entries.remove(0);
            }
            self.entries.push(CachedShadowEntry {
                key,
                rx_px,
                ry_px,
                inner_x,
                inner_y,
                pixmap: patch,
            });
            self.entries.len() - 1
        };
        blit_cached_patch(dst, &self.entries[idx], ctx);
        true
    }
}

fn build_key_and_metrics(
    ctx: &SkiaBoxEffectCtx<'_>,
    sh: BoxShadow,
) -> Option<(ShadowCacheKey, f32, f32, u32, u32)> {
    let (sx, sy) = (ctx.scaled_transform.a, ctx.scaled_transform.d);
    let (hw, hh) = (0.5 * ctx.rect.width, 0.5 * ctx.rect.height);
    let px_scale = 0.5 * (1.0 / sx + 1.0 / sy);
    let blur = sh.blur_radius.max(0.0);
    let (ext_x, ext_y) = if sh.inset {
        (hw + 2.0 * px_scale, hh + 2.0 * px_scale)
    } else {
        let pad = blur * 2.2 + 2.5 * px_scale;
        (
            (hw + sh.spread_radius).max(0.0) + pad + sh.offset_x.abs(),
            (hh + sh.spread_radius).max(0.0) + pad + sh.offset_y.abs(),
        )
    };
    let rx_px = (sx * ext_x).ceil();
    let ry_px = (sy * ext_y).ceil();
    let (pw, ph) = ((2.0 * rx_px) as u32, (2.0 * ry_px) as u32);
    if pw == 0 || ph == 0 || pw > 2048 || ph > 2048 {
        return None;
    }
    let q = |v: f32| (v * 16.0).round() as i32;
    let key = ShadowCacheKey {
        w_q: q(ctx.rect.width).max(0) as u32,
        h_q: q(ctx.rect.height).max(0) as u32,
        sx_q: (sx * 256.0).round() as u32,
        sy_q: (sy * 256.0).round() as u32,
        radii_q: ctx.corner_radii.map(|r| q(r).max(0) as u32),
        bw_q: q(ctx.border_width).max(0) as u32,
        ox_q: q(sh.offset_x),
        oy_q: q(sh.offset_y),
        blur_q: q(blur).max(0) as u32,
        spread_q: q(sh.spread_radius),
        color: [sh.color.r, sh.color.g, sh.color.b, sh.color.a],
        inset: sh.inset,
    };
    Some((key, rx_px, ry_px, pw, ph))
}

fn shadow_inner_skip(
    inset: bool,
    hw: f32,
    hh: f32,
    border_radius: f32,
    (sx, sy): (f32, f32),
    aa: f32,
) -> (i32, i32) {
    if inset {
        (0, 0)
    } else {
        let ix = ((hw - border_radius - aa * 2.0 - 1.0) * sx).floor() as i32;
        let iy = ((hh - border_radius - aa * 2.0 - 1.0) * sy).floor() as i32;
        (ix, iy)
    }
}

fn rasterize_shadow_patch(
    ctx: &SkiaBoxEffectCtx<'_>,
    sh: BoxShadow,
    (rx_px, ry_px): (f32, f32),
    (pw, ph): (u32, u32),
) -> Option<(tiny_skia::Pixmap, i32, i32)> {
    let mut patch = tiny_skia::Pixmap::new(pw, ph)?;
    let (sx, sy) = (ctx.scaled_transform.a, ctx.scaled_transform.d);
    let (hw, hh) = (0.5 * ctx.rect.width, 0.5 * ctx.rect.height);
    let aa = 0.25 * (1.0 / sx + 1.0 / sy);
    let blur = sh.blur_radius.max(0.0);
    let base_a = sh.color.a as f32 / 255.0;
    let max_r = ctx.corner_radii.into_iter().fold(0.0_f32, f32::max);
    let (inner_x, inner_y) = shadow_inner_skip(sh.inset, hw, hh, max_r, (sx, sy), aa);
    let (cx_i, cy_i) = (rx_px as i32, ry_px as i32);
    let data = patch.data_mut();
    for py in 0..ph as i32 {
        let ly = (py as f32 + 0.5 - ry_px) / sy;
        let in_core_y = inner_x > 1 && inner_y > 1 && (py - cy_i).abs() < inner_y;
        for px in 0..pw as i32 {
            if in_core_y && (px - cx_i).abs() < inner_x {
                continue;
            }
            let lx = (px as f32 + 0.5 - rx_px) / sx;
            let cov = if sh.inset {
                eval_inset_shadow_cov(
                    lx,
                    ly,
                    hw,
                    hh,
                    ctx.corner_radii,
                    ctx.border_width,
                    sh,
                    blur,
                    aa,
                )
            } else {
                eval_outer_shadow_cov(lx, ly, hw, hh, ctx.corner_radii, sh, blur, aa)
            };
            let a = base_a * cov;
            if a > (0.5 / 255.0) {
                let d = ((py as u32 * pw + px as u32) * 4) as usize;
                data[d] = (sh.color.r as f32 * a).round() as u8;
                data[d + 1] = (sh.color.g as f32 * a).round() as u8;
                data[d + 2] = (sh.color.b as f32 * a).round() as u8;
                data[d + 3] = (255.0 * a).round() as u8;
            }
        }
    }
    Some((patch, inner_x, inner_y))
}

fn blit_cached_patch(
    dst: &mut tiny_skia::Pixmap,
    entry: &CachedShadowEntry,
    ctx: &SkiaBoxEffectCtx<'_>,
) {
    let (lcx, lcy) = (
        ctx.rect.x + 0.5 * ctx.rect.width,
        ctx.rect.y + 0.5 * ctx.rect.height,
    );
    let (scx, scy) = ctx.scaled_transform.transform_point(lcx, lcy);
    let ox = (scx - entry.rx_px).round() as i32;
    let oy = (scy - entry.ry_px).round() as i32;
    let (dw, dh) = (dst.width() as i32, dst.height() as i32);
    let (pw, ph) = (entry.pixmap.width() as i32, entry.pixmap.height() as i32);
    let (sy0, sy1) = ((-oy).max(0), (dh - oy).min(ph));
    let (sx0, sx1) = ((-ox).max(0), (dw - ox).min(pw));
    if sx0 >= sx1 || sy0 >= sy1 {
        return;
    }
    let op_256 = (ctx.opacity.clamp(0.0, 1.0) * 256.0).round() as u32;
    let (cx, cy) = (entry.rx_px as i32, entry.ry_px as i32);
    let mask_bytes = ctx.mask.map(|m| m.data());
    let src_data = entry.pixmap.data();
    let dst_data = dst.data_mut();
    for py in sy0..sy1 {
        let spans = if entry.inner_x > 1 && entry.inner_y > 1 && (py - cy).abs() < entry.inner_y {
            [
                (sx0, (cx - entry.inner_x + 1).clamp(sx0, sx1)),
                ((cx + entry.inner_x).clamp(sx0, sx1), sx1),
            ]
        } else {
            [(sx0, sx1), (sx1, sx1)]
        };
        for (x_start, x_end) in spans {
            blit_shadow_span(
                dst_data,
                src_data,
                mask_bytes,
                (ox, oy, dw, pw),
                py,
                (x_start, x_end),
                op_256,
            );
        }
    }
}

fn blit_shadow_span(
    dst_data: &mut [u8],
    src_data: &[u8],
    mask_bytes: Option<&[u8]>,
    (ox, oy, dw, pw): (i32, i32, i32, i32),
    py: i32,
    (x_start, x_end): (i32, i32),
    op_256: u32,
) {
    let dy_off = (oy + py) * dw;
    let sy_off = py * pw;
    for px in x_start..x_end {
        let s = ((sy_off + px) * 4) as usize;
        let sa0 = src_data[s + 3] as u32;
        if sa0 == 0 {
            continue;
        }
        let d_px = (dy_off + ox + px) as usize;
        let m = mask_bytes.map_or(255_u32, |mb| mb[d_px] as u32);
        if m == 0 {
            continue;
        }
        let w256 = (op_256 * (m + (m >> 7))) >> 8;
        let sa = (sa0 * w256) >> 8;
        if sa == 0 {
            continue;
        }
        let inv = 255 - sa;
        let d = d_px * 4;
        dst_data[d] = (((src_data[s] as u32 * w256) >> 8)
            + ((dst_data[d] as u32 * inv + 127) / 255))
            .min(255) as u8;
        dst_data[d + 1] = (((src_data[s + 1] as u32 * w256) >> 8)
            + ((dst_data[d + 1] as u32 * inv + 127) / 255))
            .min(255) as u8;
        dst_data[d + 2] = (((src_data[s + 2] as u32 * w256) >> 8)
            + ((dst_data[d + 2] as u32 * inv + 127) / 255))
            .min(255) as u8;
        dst_data[d + 3] = (sa + ((dst_data[d + 3] as u32 * inv + 127) / 255)).min(255) as u8;
    }
}
