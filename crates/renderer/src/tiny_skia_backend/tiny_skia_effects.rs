//! Analytical `BoxShadow` (outer & `inset`) rasterizers and SDF helpers for `TinySkiaRenderer`.

use krui_layout::LayoutRect;
use krui_style::{Affine2, BoxShadow, Color};

pub(super) struct SkiaBoxEffectCtx<'a> {
    pub rect: LayoutRect,
    pub scaled_transform: Affine2,
    pub opacity: f32,
    pub border_width: f32,
    pub corner_radii: [f32; 4],
    pub mask: Option<&'a tiny_skia::Mask>,
}

#[inline]
pub(super) fn sd_round_rect_prenorm(px: f32, py: f32, hw: f32, hh: f32, nr: [f32; 4]) -> f32 {
    let r_sel = match (px > 0.0, py > 0.0) {
        (true, true) => nr[2],
        (true, false) => nr[1],
        (false, true) => nr[3],
        (false, false) => nr[0],
    };
    let r = r_sel.clamp(0.0, hw.min(hh).max(0.0));
    let qx = px.abs() - (hw - r);
    let qy = py.abs() - (hh - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    let dist = if ox > 0.0 && oy > 0.0 {
        (ox * ox + oy * oy).sqrt()
    } else {
        ox + oy
    };
    dist + qx.max(qy).min(0.0) - r
}

#[inline]
fn erf_approx(x: f32) -> f32 {
    let s = if x < 0.0 { -1.0 } else { 1.0 };
    let a = x.abs();
    let t = 1.0 + (0.278_393 + (0.230_389 + 0.078_108 * (a * a)) * a) * a;
    let t2 = t * t;
    s - s / (t2 * t2)
}

#[inline]
pub(super) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let denom = edge1 - edge0;
    if denom.abs() <= 1e-6 {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / denom).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(super) fn project_aabb(
    tx: Affine2,
    cx: f32,
    cy: f32,
    ext_x: f32,
    ext_y: f32,
    pw: i32,
    ph: i32,
) -> Option<(i32, i32, i32, i32)> {
    let (scx, scy) = tx.transform_point(cx, cy);
    let rx = tx.a.abs() * ext_x + tx.c.abs() * ext_y;
    let ry = tx.b.abs() * ext_x + tx.d.abs() * ext_y;
    let x0 = ((scx - rx).floor() as i32).clamp(0, pw);
    let y0 = ((scy - ry).floor() as i32).clamp(0, ph);
    let x1 = ((scx + rx).ceil() as i32).clamp(0, pw);
    let y1 = ((scy + ry).ceil() as i32).clamp(0, ph);
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
}

fn linear_axis_interval(base: f32, slope: f32, half: f32, lo: &mut f32, hi: &mut f32) -> bool {
    if slope.abs() <= 1e-6 {
        return base.abs() <= half;
    }
    let inv_s = 1.0 / slope;
    let a = (-half - base) * inv_s;
    let b = (half - base) * inv_s;
    *lo = lo.max(a.min(b));
    *hi = hi.min(a.max(b));
    *lo <= *hi
}

pub(super) fn row_linear_inner_skip(
    inv: Affine2,
    py: i32,
    (lcx, lcy): (f32, f32),
    (ihw, ihh): (f32, f32),
    (x0, x1): (i32, i32),
) -> Option<(i32, i32)> {
    if ihw <= 1.0 || ihh <= 1.0 || x0 >= x1 {
        return None;
    }
    let py_f = py as f32 + 0.5;
    let base_x = inv.c * py_f + inv.tx + 0.5 * inv.a - lcx;
    let base_y = inv.d * py_f + inv.ty + 0.5 * inv.b - lcy;
    let (mut lo, mut hi) = (x0 as f32, (x1 - 1) as f32);
    if !linear_axis_interval(base_x, inv.a, ihw, &mut lo, &mut hi)
        || !linear_axis_interval(base_y, inv.b, ihh, &mut lo, &mut hi)
    {
        return None;
    }
    let cx0 = (lo.ceil() as i32).clamp(x0, x1);
    let cx1 = (hi.floor() as i32 + 1).clamp(x0, x1);
    (cx1 > cx0).then_some((cx0, cx1))
}

pub(super) fn render_box_shadow_skia(
    pixmap: &mut tiny_skia::Pixmap,
    shadow_cache: &mut super::tiny_skia_shadow_cache::ShadowPatchCache,
    ctx: &SkiaBoxEffectCtx<'_>,
    shadow: BoxShadow,
) {
    if shadow.color.a == 0 || ctx.opacity <= 0.0 {
        return;
    }
    if shadow_cache.try_draw_cached(pixmap, ctx, shadow) {
        return;
    }
    let Some(inv) = ctx.scaled_transform.inverse() else {
        return;
    };
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let (hw, hh) = (0.5 * ctx.rect.width, 0.5 * ctx.rect.height);
    let (lcx, lcy) = (ctx.rect.x + hw, ctx.rect.y + hh);
    let px_scale = 0.5 * (inv.a.hypot(inv.b) + inv.c.hypot(inv.d)).max(1e-4);
    let blur = shadow.blur_radius.max(0.0);
    let (center, ext) = if shadow.inset {
        ((lcx, lcy), (hw + 2.0 * px_scale, hh + 2.0 * px_scale))
    } else {
        let pad = blur * 2.2 + 2.5 * px_scale;
        (
            (lcx + shadow.offset_x, lcy + shadow.offset_y),
            (
                (hw + shadow.spread_radius).max(0.0) + pad,
                (hh + shadow.spread_radius).max(0.0) + pad,
            ),
        )
    };
    if let Some(aabb) = project_aabb(
        ctx.scaled_transform,
        center.0,
        center.1,
        ext.0,
        ext.1,
        pw,
        ph,
    ) {
        rasterize_shadow_aabb(pixmap, ctx, shadow, inv, aabb, 0.5 * px_scale);
    }
}

#[derive(Clone, Copy)]
struct ShadowNormRadii {
    outer_box: [f32; 4],
    outer_sh: [f32; 4],
    pad: [f32; 4],
    cut: [f32; 4],
}

impl ShadowNormRadii {
    fn from_ctx(ctx: &SkiaBoxEffectCtx<'_>, sh: BoxShadow) -> Self {
        let (hw, hh) = (0.5 * ctx.rect.width, 0.5 * ctx.rect.height);
        let bw = ctx.border_width;
        let outer_box = crate::radii::normalize_radii_4(2.0 * hw, 2.0 * hh, ctx.corner_radii);
        let sh_hw = (hw + sh.spread_radius).max(0.0);
        let sh_hh = (hh + sh.spread_radius).max(0.0);
        let outer_sh = crate::radii::normalize_radii_4(
            2.0 * sh_hw,
            2.0 * sh_hh,
            ctx.corner_radii.map(|r| (r + sh.spread_radius).max(0.0)),
        );
        let (pad_hw, pad_hh) = ((hw - bw).max(0.0), (hh - bw).max(0.0));
        let raw_pad_r = ctx.corner_radii.map(|r| (r - bw).max(0.0));
        let pad = crate::radii::normalize_radii_4(2.0 * pad_hw, 2.0 * pad_hh, raw_pad_r);
        let cut_hw = (pad_hw - sh.spread_radius).max(0.0);
        let cut_hh = (pad_hh - sh.spread_radius).max(0.0);
        let cut = crate::radii::normalize_radii_4(
            2.0 * cut_hw,
            2.0 * cut_hh,
            raw_pad_r.map(|r| (r - sh.spread_radius).max(0.0)),
        );
        Self {
            outer_box,
            outer_sh,
            pad,
            cut,
        }
    }
}

fn rasterize_shadow_aabb(
    pixmap: &mut tiny_skia::Pixmap,
    ctx: &SkiaBoxEffectCtx<'_>,
    shadow: BoxShadow,
    inv: Affine2,
    (x0, y0, x1, y1): (i32, i32, i32, i32),
    aa: f32,
) {
    let pw = pixmap.width() as i32;
    let (hw, hh) = (0.5 * ctx.rect.width, 0.5 * ctx.rect.height);
    let (lcx, lcy) = (ctx.rect.x + hw, ctx.rect.y + hh);
    let max_r = ctx.corner_radii.into_iter().fold(0.0_f32, f32::max);
    let margin = max_r + aa * 2.0 + 1.0;
    let (ihw, ihh) = (hw - margin, hh - margin);
    let norm = ShadowNormRadii::from_ctx(ctx, shadow);
    for py in y0..y1 {
        let spans = if !shadow.inset {
            match row_linear_inner_skip(inv, py, (lcx, lcy), (ihw, ihh), (x0, x1)) {
                Some((cx0, cx1)) => [(x0, cx0), (cx1, x1)],
                None => [(x0, x1), (x1, x1)],
            }
        } else {
            [(x0, x1), (x1, x1)]
        };
        for (sx, ex) in spans {
            rasterize_shadow_row_span(pixmap, ctx, shadow, norm, inv, py, (sx, ex), pw, aa);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rasterize_shadow_row_span(
    pixmap: &mut tiny_skia::Pixmap,
    ctx: &SkiaBoxEffectCtx<'_>,
    shadow: BoxShadow,
    norm: ShadowNormRadii,
    inv: Affine2,
    py: i32,
    (sx, ex): (i32, i32),
    pw: i32,
    aa: f32,
) {
    if sx >= ex {
        return;
    }
    let (hw, hh) = (0.5 * ctx.rect.width, 0.5 * ctx.rect.height);
    let (lcx, lcy) = (ctx.rect.x + hw, ctx.rect.y + hh);
    let blur = shadow.blur_radius.max(0.0);
    let mask_bytes = ctx.mask.map(|m| m.data());
    let base_alpha = (shadow.color.a as f32 / 255.0) * ctx.opacity.clamp(0.0, 1.0);
    let pixels = pixmap.data_mut();
    let (mut ux, mut uy) = inv.transform_point(sx as f32 + 0.5, py as f32 + 0.5);
    let row_off = py * pw;
    for px in sx..ex {
        let p_idx = (row_off + px) as usize;
        let clip_cov = mask_bytes.map_or(1.0, |m| m[p_idx] as f32 / 255.0);
        if clip_cov > 0.0 {
            let (lx, ly) = (ux - lcx, uy - lcy);
            let cov = if shadow.inset {
                eval_inset_shadow_prenorm(
                    lx,
                    ly,
                    hw,
                    hh,
                    (norm.pad, norm.cut),
                    ctx.border_width,
                    shadow,
                    blur,
                    aa,
                )
            } else {
                eval_outer_shadow_prenorm(
                    lx,
                    ly,
                    hw,
                    hh,
                    (norm.outer_box, norm.outer_sh),
                    shadow,
                    blur,
                    aa,
                )
            };
            let alpha = base_alpha * clip_cov * cov;
            if alpha > (0.5 / 255.0) {
                blend_over_premul(&mut pixels[p_idx * 4..p_idx * 4 + 4], shadow.color, alpha);
            }
        }
        ux += inv.a;
        uy += inv.b;
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_outer_shadow_prenorm(
    lx: f32,
    ly: f32,
    hw: f32,
    hh: f32,
    (box_nr, sh_nr): ([f32; 4], [f32; 4]),
    sh: BoxShadow,
    blur: f32,
    aa: f32,
) -> f32 {
    let d_box = sd_round_rect_prenorm(lx, ly, hw, hh, box_nr);
    let outside = smoothstep(-aa, aa, d_box);
    if outside <= 0.0 {
        return 0.0;
    }
    let sh_hw = (hw + sh.spread_radius).max(0.0);
    let sh_hh = (hh + sh.spread_radius).max(0.0);
    let d_sh = sd_round_rect_prenorm(lx - sh.offset_x, ly - sh.offset_y, sh_hw, sh_hh, sh_nr);
    let sh_cov = if blur > 0.25 {
        0.5 - 0.5 * erf_approx(d_sh / (blur * 0.707_106_77).max(0.35))
    } else {
        smoothstep(aa, -aa, d_sh)
    };
    outside * sh_cov
}

#[allow(clippy::too_many_arguments)]
fn eval_inset_shadow_prenorm(
    lx: f32,
    ly: f32,
    hw: f32,
    hh: f32,
    (pad_nr, cut_nr): ([f32; 4], [f32; 4]),
    bw: f32,
    sh: BoxShadow,
    blur: f32,
    aa: f32,
) -> f32 {
    let (pad_hw, pad_hh) = ((hw - bw).max(0.0), (hh - bw).max(0.0));
    let inside = smoothstep(
        aa,
        -aa,
        sd_round_rect_prenorm(lx, ly, pad_hw, pad_hh, pad_nr),
    );
    if inside <= 0.0 {
        return 0.0;
    }
    let cut_hw = (pad_hw - sh.spread_radius).max(0.0);
    let cut_hh = (pad_hh - sh.spread_radius).max(0.0);
    let d_cut = sd_round_rect_prenorm(lx - sh.offset_x, ly - sh.offset_y, cut_hw, cut_hh, cut_nr);
    let sh_cov = if blur > 0.25 {
        0.5 + 0.5 * erf_approx(d_cut / (blur * 0.707_106_77).max(0.35))
    } else {
        smoothstep(-aa, aa, d_cut)
    };
    inside * sh_cov
}

#[allow(clippy::too_many_arguments)]
pub(super) fn eval_outer_shadow_cov(
    lx: f32,
    ly: f32,
    hw: f32,
    hh: f32,
    radii: [f32; 4],
    sh: BoxShadow,
    blur: f32,
    aa: f32,
) -> f32 {
    let box_nr = crate::radii::normalize_radii_4(2.0 * hw, 2.0 * hh, radii);
    let sh_hw = (hw + sh.spread_radius).max(0.0);
    let sh_hh = (hh + sh.spread_radius).max(0.0);
    let sh_nr = crate::radii::normalize_radii_4(
        2.0 * sh_hw,
        2.0 * sh_hh,
        radii.map(|r| (r + sh.spread_radius).max(0.0)),
    );
    eval_outer_shadow_prenorm(lx, ly, hw, hh, (box_nr, sh_nr), sh, blur, aa)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn eval_inset_shadow_cov(
    lx: f32,
    ly: f32,
    hw: f32,
    hh: f32,
    radii: [f32; 4],
    bw: f32,
    sh: BoxShadow,
    blur: f32,
    aa: f32,
) -> f32 {
    let (pad_hw, pad_hh) = ((hw - bw).max(0.0), (hh - bw).max(0.0));
    let raw_pad_r = radii.map(|r| (r - bw).max(0.0));
    let pad_nr = crate::radii::normalize_radii_4(2.0 * pad_hw, 2.0 * pad_hh, raw_pad_r);
    let cut_hw = (pad_hw - sh.spread_radius).max(0.0);
    let cut_hh = (pad_hh - sh.spread_radius).max(0.0);
    let cut_nr = crate::radii::normalize_radii_4(
        2.0 * cut_hw,
        2.0 * cut_hh,
        raw_pad_r.map(|r| (r - sh.spread_radius).max(0.0)),
    );
    eval_inset_shadow_prenorm(lx, ly, hw, hh, (pad_nr, cut_nr), bw, sh, blur, aa)
}

#[inline]
fn blend_over_premul(dst: &mut [u8], src: Color, alpha: f32) {
    let sa = alpha.clamp(0.0, 1.0);
    let inv = 1.0 - sa;
    dst[0] = ((src.r as f32 * sa + dst[0] as f32 * inv).clamp(0.0, 255.0) + 0.5) as u8;
    dst[1] = ((src.g as f32 * sa + dst[1] as f32 * inv).clamp(0.0, 255.0) + 0.5) as u8;
    dst[2] = ((src.b as f32 * sa + dst[2] as f32 * inv).clamp(0.0, 255.0) + 0.5) as u8;
    dst[3] = ((255.0 * sa + dst[3] as f32 * inv).clamp(0.0, 255.0) + 0.5) as u8;
}
