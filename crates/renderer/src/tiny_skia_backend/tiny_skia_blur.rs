//! Multiscale separable box-blur, fixed-point bilinear upsampling, and CPU `backdrop-filter: blur()` compositing.

use super::tiny_skia_effects::{
    project_aabb, row_linear_inner_skip, sd_round_rect_prenorm, smoothstep, SkiaBoxEffectCtx,
};
use krui_style::Affine2;

struct BlurBounds {
    inv: Affine2,
    px_scale: f32,
    aabb: (i32, i32, i32, i32),
}

fn compute_blur_bounds(ctx: &SkiaBoxEffectCtx<'_>, pw: i32, ph: i32) -> Option<BlurBounds> {
    let inv = ctx.scaled_transform.inverse()?;
    let (hw, hh) = (0.5 * ctx.rect.width, 0.5 * ctx.rect.height);
    let (lcx, lcy) = (ctx.rect.x + hw, ctx.rect.y + hh);
    let px_scale = 0.5 * (inv.a.hypot(inv.b) + inv.c.hypot(inv.d)).max(1e-4);
    let aabb = project_aabb(
        ctx.scaled_transform,
        lcx,
        lcy,
        hw + 2.0 * px_scale,
        hh + 2.0 * px_scale,
        pw,
        ph,
    )?;
    Some(BlurBounds {
        inv,
        px_scale,
        aabb,
    })
}

pub(super) fn render_backdrop_blur_skia(
    pixmap: &mut tiny_skia::Pixmap,
    ctx: &SkiaBoxEffectCtx<'_>,
    blur_px: f32,
    scale: f32,
) {
    let radius = ((blur_px * scale * 0.5).round() as i32).clamp(1, 48);
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let Some(BlurBounds {
        inv,
        px_scale,
        aabb: (x0, y0, x1, y1),
    }) = compute_blur_bounds(ctx, pw, ph)
    else {
        return;
    };
    let halo = radius * 2;
    let (sx0, sy0) = ((x0 - halo).max(0), (y0 - halo).max(0));
    let (sx1, sy1) = ((x1 + halo).min(pw), (y1 + halo).min(ph));
    let (sw, sh) = ((sx1 - sx0) as usize, (sy1 - sy0) as usize);
    let origin = (sx0 as usize, sy0 as usize);
    if sw == 0 || sh == 0 || is_sub_pixmap_uniform(pixmap.data(), pw as usize, origin, (sw, sh)) {
        return;
    }
    let ds = if radius >= 4 && sw >= 16 && sh >= 16 {
        4
    } else {
        1
    };
    let (bw, bh, blurred) = blur_sub_pixmap_multiscale(
        pixmap.data(),
        pw as usize,
        origin,
        (sw, sh),
        radius as usize,
        ds,
    );
    blend_blurred_backdrop_aabb(
        pixmap,
        ctx,
        &blurred,
        (sx0, sy0, bw, bh, ds),
        inv,
        (x0, y0, x1, y1),
        0.5 * px_scale,
    );
}

fn is_sub_pixmap_uniform(
    src: &[u8],
    full_w: usize,
    (sx0, sy0): (usize, usize),
    (sw, sh): (usize, usize),
) -> bool {
    let first_off = (sy0 * full_w + sx0) * 4;
    let first = [
        src[first_off],
        src[first_off + 1],
        src[first_off + 2],
        src[first_off + 3],
    ];
    let first_u32 = u32::from_ne_bytes(first);
    let row_bytes = sw * 4;
    let mut pattern = [0_u8; 64];
    for chunk in pattern.as_chunks_mut::<4>().0 {
        chunk.copy_from_slice(&first_u32.to_ne_bytes());
    }
    for row in 0..sh {
        let s_off = ((sy0 + row) * full_w + sx0) * 4;
        let slice = &src[s_off..s_off + row_bytes];
        let mut idx = 0;
        while idx + 64 <= row_bytes {
            if slice[idx..idx + 64] != pattern {
                return false;
            }
            idx += 64;
        }
        while idx < row_bytes {
            if slice[idx..idx + 4] != first {
                return false;
            }
            idx += 4;
        }
    }
    true
}

fn blend_blurred_backdrop_aabb(
    pixmap: &mut tiny_skia::Pixmap,
    ctx: &SkiaBoxEffectCtx<'_>,
    sub: &[u8],
    (sx0, sy0, bw, bh, ds): (i32, i32, usize, usize, usize),
    inv: Affine2,
    (x0, y0, x1, y1): (i32, i32, i32, i32),
    aa: f32,
) {
    let pw = pixmap.width() as i32;
    let (hw, hh) = (0.5 * ctx.rect.width, 0.5 * ctx.rect.height);
    let (lcx, lcy) = (ctx.rect.x + hw, ctx.rect.y + hh);
    let box_nr = crate::radii::normalize_radii_4(2.0 * hw, 2.0 * hh, ctx.corner_radii);
    let max_r = box_nr.into_iter().fold(0.0_f32, f32::max);
    let (ihw, ihh) = (hw - max_r - aa * 2.0 - 1.0, hh - max_r - aa * 2.0 - 1.0);
    let mask_bytes = ctx.mask.map(|m| m.data());
    let op = ctx.opacity.clamp(0.0, 1.0);
    let pixels = pixmap.data_mut();
    for py in y0..y1 {
        let inner = mask_bytes
            .is_none()
            .then(|| row_linear_inner_skip(inv, py, (lcx, lcy), (ihw, ihh), (x0, x1)))
            .flatten();
        match inner {
            Some((ix0, ix1)) => {
                blend_blur_edge_span(
                    pixels,
                    (pw, py, sx0, sy0),
                    (x0, ix0),
                    (sub, bw, bh, ds),
                    (inv, (lcx, lcy, hw, hh), box_nr, aa, op, None),
                );
                blend_blur_inner_span(
                    pixels,
                    (pw, py, sx0, sy0),
                    (ix0, ix1),
                    (sub, bw, bh, ds),
                    op,
                );
                blend_blur_edge_span(
                    pixels,
                    (pw, py, sx0, sy0),
                    (ix1, x1),
                    (sub, bw, bh, ds),
                    (inv, (lcx, lcy, hw, hh), box_nr, aa, op, None),
                );
            }
            None => blend_blur_edge_span(
                pixels,
                (pw, py, sx0, sy0),
                (x0, x1),
                (sub, bw, bh, ds),
                (inv, (lcx, lcy, hw, hh), box_nr, aa, op, mask_bytes),
            ),
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn blend_blur_edge_span(
    pixels: &mut [u8],
    (pw, py, sx0, sy0): (i32, i32, i32, i32),
    (span_x0, span_x1): (i32, i32),
    (sub, bw, bh, ds): (&[u8], usize, usize, usize),
    (inv, (lcx, lcy, hw, hh), box_nr, aa, op, mask_bytes): (
        Affine2,
        (f32, f32, f32, f32),
        [f32; 4],
        f32,
        f32,
        Option<&[u8]>,
    ),
) {
    if span_x0 >= span_x1 {
        return;
    }
    let (mut ux, mut uy) = inv.transform_point(span_x0 as f32 + 0.5, py as f32 + 0.5);
    let ly_idx = (py - sy0) as usize;
    let row_off = py * pw;
    for px in span_x0..span_x1 {
        let p_idx = (row_off + px) as usize;
        let clip_cov = mask_bytes.map_or(1.0, |m| m[p_idx] as f32 / 255.0);
        if clip_cov > 0.0 {
            let d_box = sd_round_rect_prenorm(ux - lcx, uy - lcy, hw, hh, box_nr);
            let w = smoothstep(aa, -aa, d_box) * clip_cov * op;
            if w > (0.5 / 255.0) {
                let src_pm = sample_blurred_bilinear(sub, bw, bh, ds, (px - sx0) as usize, ly_idx);
                blend_premul_src_over(&mut pixels[p_idx * 4..p_idx * 4 + 4], &src_pm, w);
            }
        }
        ux += inv.a;
        uy += inv.b;
    }
}

fn blend_blur_inner_span(
    pixels: &mut [u8],
    (pw, py, sx0, sy0): (i32, i32, i32, i32),
    (span_x0, span_x1): (i32, i32),
    (sub, bw, bh, ds): (&[u8], usize, usize, usize),
    op: f32,
) {
    if span_x0 >= span_x1 {
        return;
    }
    let ly_idx = (py - sy0) as usize;
    let row_off = py * pw;
    for px in span_x0..span_x1 {
        let p_idx = (row_off + px) as usize;
        let src_pm = sample_blurred_bilinear(sub, bw, bh, ds, (px - sx0) as usize, ly_idx);
        let d = p_idx * 4;
        if op >= 0.999 && src_pm[3] == 255 {
            pixels[d..d + 4].copy_from_slice(&src_pm);
        } else {
            blend_premul_src_over(&mut pixels[d..d + 4], &src_pm, op);
        }
    }
}

#[inline]
fn blend_premul_src_over(dst: &mut [u8], src_pm: &[u8], weight: f32) {
    let w = weight.clamp(0.0, 1.0);
    let sa = (src_pm[3] as f32 / 255.0) * w;
    let inv = 1.0 - sa;
    dst[0] = ((src_pm[0] as f32 * w + dst[0] as f32 * inv).clamp(0.0, 255.0) + 0.5) as u8;
    dst[1] = ((src_pm[1] as f32 * w + dst[1] as f32 * inv).clamp(0.0, 255.0) + 0.5) as u8;
    dst[2] = ((src_pm[2] as f32 * w + dst[2] as f32 * inv).clamp(0.0, 255.0) + 0.5) as u8;
    dst[3] = ((src_pm[3] as f32 * w + dst[3] as f32 * inv).clamp(0.0, 255.0) + 0.5) as u8;
}

pub(super) fn blur_sub_pixmap_multiscale(
    src: &[u8],
    full_w: usize,
    (sx0, sy0): (usize, usize),
    (sw, sh): (usize, usize),
    radius: usize,
    ds: usize,
) -> (usize, usize, Vec<u8>) {
    if ds == 1 {
        let mut sub = extract_sub_pixmap(src, full_w, (sx0, sy0), (sw, sh));
        let mut scratch = vec![0_u8; sw * sh * 4];
        for _ in 0..2 {
            box_blur_horizontal(&sub, &mut scratch, sw, sh, radius);
            box_blur_vertical(&scratch, &mut sub, sw, sh, radius);
        }
        return (sw, sh, sub);
    }
    let (dw, dh) = (sw.div_ceil(ds), sh.div_ceil(ds));
    let mut small = downsample_box_sub(src, full_w, (sx0, sy0), (sw, sh), (dw, dh), ds);
    let mut scratch = vec![0_u8; dw * dh * 4];
    let r_small = (radius / ds).max(1);
    for _ in 0..2 {
        box_blur_horizontal(&small, &mut scratch, dw, dh, r_small);
        box_blur_vertical(&scratch, &mut small, dw, dh, r_small);
    }
    (dw, dh, small)
}

#[inline]
pub(super) fn sample_blurred_bilinear(
    buf: &[u8],
    bw: usize,
    bh: usize,
    ds: usize,
    lx: usize,
    ly: usize,
) -> [u8; 4] {
    if ds <= 1 {
        let idx = (ly.min(bh - 1) * bw + lx.min(bw - 1)) * 4;
        return [buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]];
    }
    let ds_i = ds as i32;
    let qx = (((lx as i32 * 2 + 1) * 128) / ds_i - 128).clamp(0, ((bw - 1) * 256) as i32);
    let qy = (((ly as i32 * 2 + 1) * 128) / ds_i - 128).clamp(0, ((bh - 1) * 256) as i32);
    let (x0, wx) = ((qx >> 8) as usize, (qx & 255) as u32);
    let (y0, wy) = ((qy >> 8) as usize, (qy & 255) as u32);
    let x1 = if wx == 0 { x0 } else { (x0 + 1).min(bw - 1) };
    let y1 = if wy == 0 { y0 } else { (y0 + 1).min(bh - 1) };
    let (inv_wx, inv_wy) = (256 - wx, 256 - wy);
    let (p00, p10) = ((y0 * bw + x0) * 4, (y0 * bw + x1) * 4);
    let (p01, p11) = ((y1 * bw + x0) * 4, (y1 * bw + x1) * 4);
    [
        bilerp_u8(
            buf[p00],
            buf[p10],
            buf[p01],
            buf[p11],
            (wx, inv_wx),
            (wy, inv_wy),
        ),
        bilerp_u8(
            buf[p00 + 1],
            buf[p10 + 1],
            buf[p01 + 1],
            buf[p11 + 1],
            (wx, inv_wx),
            (wy, inv_wy),
        ),
        bilerp_u8(
            buf[p00 + 2],
            buf[p10 + 2],
            buf[p01 + 2],
            buf[p11 + 2],
            (wx, inv_wx),
            (wy, inv_wy),
        ),
        bilerp_u8(
            buf[p00 + 3],
            buf[p10 + 3],
            buf[p01 + 3],
            buf[p11 + 3],
            (wx, inv_wx),
            (wy, inv_wy),
        ),
    ]
}

#[inline]
fn bilerp_u8(
    c00: u8,
    c10: u8,
    c01: u8,
    c11: u8,
    (wx, inv_wx): (u32, u32),
    (wy, inv_wy): (u32, u32),
) -> u8 {
    let top = c00 as u32 * inv_wx + c10 as u32 * wx;
    let bot = c01 as u32 * inv_wx + c11 as u32 * wx;
    ((top * inv_wy + bot * wy + 32_768) >> 16) as u8
}

fn downsample_box_sub(
    src: &[u8],
    full_w: usize,
    (sx0, sy0): (usize, usize),
    (sw, sh): (usize, usize),
    (dw, dh): (usize, usize),
    ds: usize,
) -> Vec<u8> {
    let mut out = vec![0_u8; dw * dh * 4];
    for dy in 0..dh {
        let y_start = dy * ds;
        let y_end = (y_start + ds).min(sh);
        for dx in 0..dw {
            let x_start = dx * ds;
            let x_end = (x_start + ds).min(sw);
            let mut acc = [0_u32; 4];
            let count = ((y_end - y_start) * (x_end - x_start)).max(1) as u32;
            for yy in y_start..y_end {
                let row_base = ((sy0 + yy) * full_w + sx0) * 4;
                for xx in x_start..x_end {
                    let p = row_base + xx * 4;
                    acc[0] += src[p] as u32;
                    acc[1] += src[p + 1] as u32;
                    acc[2] += src[p + 2] as u32;
                    acc[3] += src[p + 3] as u32;
                }
            }
            let d = (dy * dw + dx) * 4;
            out[d] = ((acc[0] + count / 2) / count) as u8;
            out[d + 1] = ((acc[1] + count / 2) / count) as u8;
            out[d + 2] = ((acc[2] + count / 2) / count) as u8;
            out[d + 3] = ((acc[3] + count / 2) / count) as u8;
        }
    }
    out
}

fn extract_sub_pixmap(
    src: &[u8],
    full_w: usize,
    (sx0, sy0): (usize, usize),
    (sw, sh): (usize, usize),
) -> Vec<u8> {
    let mut out = vec![0_u8; sw * sh * 4];
    for row in 0..sh {
        let s_off = ((sy0 + row) * full_w + sx0) * 4;
        let d_off = row * sw * 4;
        out[d_off..d_off + sw * 4].copy_from_slice(&src[s_off..s_off + sw * 4]);
    }
    out
}

fn box_blur_horizontal(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    let kernel = (2 * r + 1) as u32;
    for y in 0..h {
        let row_off = y * w * 4;
        let mut acc = [0_u32; 4];
        for k in 0..=2 * r {
            let cx = k.saturating_sub(r).min(w - 1);
            let p = row_off + cx * 4;
            for c in 0..4 {
                acc[c] += src[p + c] as u32;
            }
        }
        for x in 0..w {
            let out_p = row_off + x * 4;
            for c in 0..4 {
                dst[out_p + c] = ((acc[c] + kernel / 2) / kernel) as u8;
            }
            let rem_x = x.saturating_sub(r);
            let add_x = (x + r + 1).min(w - 1);
            for c in 0..4 {
                acc[c] = acc[c] + src[row_off + add_x * 4 + c] as u32
                    - src[row_off + rem_x * 4 + c] as u32;
            }
        }
    }
}

fn box_blur_vertical(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    let kernel = (2 * r + 1) as u32;
    for x in 0..w {
        let mut acc = [0_u32; 4];
        for k in 0..=2 * r {
            let cy = k.saturating_sub(r).min(h - 1);
            let p = (cy * w + x) * 4;
            for c in 0..4 {
                acc[c] += src[p + c] as u32;
            }
        }
        for y in 0..h {
            let out_p = (y * w + x) * 4;
            for c in 0..4 {
                dst[out_p + c] = ((acc[c] + kernel / 2) / kernel) as u8;
            }
            let rem_y = y.saturating_sub(r);
            let add_y = (y + r + 1).min(h - 1);
            for c in 0..4 {
                acc[c] = acc[c] + src[(add_y * w + x) * 4 + c] as u32
                    - src[(rem_y * w + x) * 4 + c] as u32;
            }
        }
    }
}
