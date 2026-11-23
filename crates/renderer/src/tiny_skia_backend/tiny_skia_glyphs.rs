//! Glyph blitting and premultiplied alpha blending for `TinySkiaRenderer`.

use krui_layout::LayoutRect;
use krui_style::{Affine2, Color};
use krui_text::RasterizedGlyph;

pub(super) struct GlyphBlitParams<'a> {
    pub origin_x: f32,
    pub origin_y: f32,
    pub clip: Option<LayoutRect>,
    pub clip_mask: Option<&'a [u8]>,
    pub transform: Affine2,
    pub scale: f32,
    pub opacity: f32,
    pub color: Color,
}

fn compute_clip_pixel_bounds(
    pw: i32,
    ph: i32,
    clip: Option<LayoutRect>,
    scale: f32,
) -> (i32, i32, i32, i32) {
    if let Some(c) = clip {
        let x0 = (c.x * scale).floor() as i32;
        let y0 = (c.y * scale).floor() as i32;
        let x1 = ((c.x + c.width) * scale).ceil() as i32;
        let y1 = ((c.y + c.height) * scale).ceil() as i32;
        (
            x0.clamp(0, pw),
            y0.clamp(0, ph),
            x1.clamp(0, pw),
            y1.clamp(0, ph),
        )
    } else {
        (0, 0, pw, ph)
    }
}

pub(super) fn rasterize_glyphs_skia(
    pixmap: &mut tiny_skia::Pixmap,
    params: GlyphBlitParams<'_>,
    glyphs: &[RasterizedGlyph],
) {
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let bounds = compute_clip_pixel_bounds(pw, ph, params.clip, params.scale);
    if bounds.2 <= bounds.0 || bounds.3 <= bounds.1 {
        return;
    }
    let pixels = pixmap.data_mut();
    if params.transform.is_pure_translation() {
        blit_translated_glyphs(pixels, pw, bounds, &params, glyphs);
    } else {
        blit_affine_glyphs(pixels, pw, bounds, &params, glyphs);
    }
}

fn sample_glyph_pixel(
    g: &RasterizedGlyph,
    pixel_idx: usize,
    default_fg: Color,
    opacity: f32,
) -> Option<(Color, f32)> {
    if let Some(rgba) = &g.color_rgba {
        let base = pixel_idx * 4;
        let px = rgba.get(base..base + 4)?;
        if px[3] == 0 {
            return None;
        }
        let src = Color {
            r: px[0],
            g: px[1],
            b: px[2],
            a: 255,
        };
        Some((src, (px[3] as f32 / 255.0) * opacity))
    } else {
        let cov = g.alpha_mask.get(pixel_idx).copied().unwrap_or(0);
        if cov == 0 {
            return None;
        }
        let fg = g.glyph_color.unwrap_or(default_fg);
        let alpha = (cov as f32 / 255.0) * (fg.a as f32 / 255.0) * opacity;
        Some((fg, alpha))
    }
}

fn blit_translated_glyphs(
    pixels: &mut [u8],
    pw: i32,
    (clip_x0, clip_y0, clip_x1, clip_y1): (i32, i32, i32, i32),
    params: &GlyphBlitParams<'_>,
    glyphs: &[RasterizedGlyph],
) {
    for g in glyphs {
        let gx = ((params.origin_x + params.transform.tx + g.x) * params.scale).round() as i32;
        let gy = ((params.origin_y + params.transform.ty + g.y) * params.scale).round() as i32;
        for row in 0..g.height as i32 {
            let py = gy + row;
            if py < clip_y0 || py >= clip_y1 {
                continue;
            }
            for col in 0..g.width as i32 {
                let px = gx + col;
                if px < clip_x0 || px >= clip_x1 {
                    continue;
                }
                let idx = (row as u32 * g.width + col as u32) as usize;
                blend_clipped_glyph_px(pixels, pw, px, py, g, idx, params);
            }
        }
    }
}

fn blit_affine_glyphs(
    pixels: &mut [u8],
    pw: i32,
    (clip_x0, clip_y0, clip_x1, clip_y1): (i32, i32, i32, i32),
    params: &GlyphBlitParams<'_>,
    glyphs: &[RasterizedGlyph],
) {
    let eff_tx = Affine2::scale(params.scale, params.scale) * params.transform;
    let Some(inv) = eff_tx.inverse() else {
        return;
    };
    for g in glyphs {
        if g.width == 0 || g.height == 0 {
            continue;
        }
        let (lx, ly) = (params.origin_x + g.x, params.origin_y + g.y);
        let (gw, gh) = (
            g.width as f32 / params.scale,
            g.height as f32 / params.scale,
        );
        let (x0, y0) = eff_tx.transform_point(lx, ly);
        let (x1, y1) = eff_tx.transform_point(lx + gw, ly);
        let (x2, y2) = eff_tx.transform_point(lx + gw, ly + gh);
        let (x3, y3) = eff_tx.transform_point(lx, ly + gh);
        let min_x = (x0.min(x1).min(x2).min(x3).floor() as i32).max(clip_x0);
        let max_x = (x0.max(x1).max(x2).max(x3).ceil() as i32).min(clip_x1);
        let min_y = (y0.min(y1).min(y2).min(y3).floor() as i32).max(clip_y0);
        let max_y = (y0.max(y1).max(y2).max(y3).ceil() as i32).min(clip_y1);

        for py in min_y..max_y {
            for px in min_x..max_x {
                let (ux, uy) = inv.transform_point(px as f32 + 0.5, py as f32 + 0.5);
                let col = ((ux - lx) * params.scale).floor() as i32;
                let row = ((uy - ly) * params.scale).floor() as i32;
                if col >= 0 && col < g.width as i32 && row >= 0 && row < g.height as i32 {
                    let idx = (row as u32 * g.width + col as u32) as usize;
                    blend_clipped_glyph_px(pixels, pw, px, py, g, idx, params);
                }
            }
        }
    }
}

fn blend_clipped_glyph_px(
    pixels: &mut [u8],
    pw: i32,
    px: i32,
    py: i32,
    g: &RasterizedGlyph,
    idx: usize,
    params: &GlyphBlitParams<'_>,
) {
    let Some((src, alpha)) = sample_glyph_pixel(g, idx, params.color, params.opacity) else {
        return;
    };
    let mask_cov = params
        .clip_mask
        .and_then(|m| m.get((py * pw + px) as usize))
        .map_or(1.0, |&v| v as f32 / 255.0);
    if mask_cov > 0.0 {
        let dst_idx = ((py * pw + px) * 4) as usize;
        blend_premultiplied_rgba(&mut pixels[dst_idx..dst_idx + 4], src, alpha * mask_cov);
    }
}

fn blend_premultiplied_rgba(dst: &mut [u8], src: Color, alpha: f32) {
    let (sa, da) = (alpha.clamp(0.0, 1.0), dst[3] as f32);
    let inv_sa = 1.0 - sa;
    dst[0] = (src.r as f32 * sa + dst[0] as f32 * inv_sa)
        .round()
        .clamp(0.0, 255.0) as u8;
    dst[1] = (src.g as f32 * sa + dst[1] as f32 * inv_sa)
        .round()
        .clamp(0.0, 255.0) as u8;
    dst[2] = (src.b as f32 * sa + dst[2] as f32 * inv_sa)
        .round()
        .clamp(0.0, 255.0) as u8;
    dst[3] = (sa * 255.0 + da * inv_sa).round().clamp(0.0, 255.0) as u8;
}
