//! Fallback `.notdef` box synthesis and fontless measurement for `krui-text`.

use crate::font_stacks::is_cjk_or_hangul;
use crate::rasterize::ShapedRunsOutput;
use crate::{
    build_hollow_notdef_mask, record_notdef_glyph_warning, RasterizedGlyph, TextLineMetrics,
    FALLBACK_CJK_ADVANCE_MULTIPLIER, FALLBACK_LATIN_ADVANCE_RATIO,
};
use krui_style::{Color, FontFamilyStack};

pub(crate) fn measure_fallback_dimensions(
    sanitized_text: &str,
    safe_font_size: f32,
    line_height_px: f32,
    max_width: Option<f32>,
) -> (f32, f32) {
    let advance = safe_font_size * FALLBACK_LATIN_ADVANCE_RATIO;
    let raw_w: f32 = sanitized_text
        .chars()
        .map(|c| {
            if is_cjk_or_hangul(c) {
                advance * FALLBACK_CJK_ADVANCE_MULTIPLIER
            } else {
                advance
            }
        })
        .sum();
    let limit = max_width.filter(|&w| w > 0.0 && raw_w > w);
    let mw = limit.unwrap_or(raw_w);
    let mh = limit.map_or(line_height_px, |l| {
        (raw_w / l).ceil().max(1.0) * line_height_px
    });
    (mw, mh)
}

pub(crate) fn build_cosmic_notdef_glyph(
    gx_logical: f32,
    gw_logical: f32,
    safe_font_size: f32,
    baseline_y: f32,
    scale: f32,
    glyph_key: u64,
    (span_index, char_start, char_end, glyph_color): (u16, u32, u32, Option<Color>),
) -> RasterizedGlyph {
    let w = ((gw_logical * scale).round() as u32).max(6);
    let h = ((safe_font_size * 0.75 * scale).round() as u32).max(8);
    let gx_phys = (gx_logical * scale).round();
    RasterizedGlyph {
        x: gx_phys / scale,
        y: baseline_y - (h as f32) / scale,
        width: w,
        height: h,
        cache_key: glyph_key ^ 0xFFFF_0000_0000_0000,
        is_notdef: true,
        is_color: false,
        span_index,
        char_start,
        char_end,
        glyph_color,
        alpha_mask: build_hollow_notdef_mask(w, h),
        color_rgba: None,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn populate_fallback_glyphs(
    sanitized_text: &str,
    safe_font_size: f32,
    line_height_px: f32,
    scale: f32,
    font_family: FontFamilyStack,
    has_rtl: bool,
    max_width: Option<f32>,
    out: &mut ShapedRunsOutput,
) {
    let adv = safe_font_size * FALLBACK_LATIN_ADVANCE_RATIO;
    let gw = ((safe_font_size * 0.5 * scale).round() as u32).max(4);
    let gh = ((safe_font_size * 0.75 * scale).round() as u32).max(6);
    let wrap_limit = max_width.filter(|&w| w > 0.0);
    let (mut cx, mut line_idx, mut line_start) = (0.0_f32, 0_usize, 0_usize);
    out.lines.clear();
    let total_chars = sanitized_text.chars().count();
    for (idx, g) in sanitized_text.chars().enumerate() {
        let wide = is_cjk_or_hangul(g);
        let step = if wide {
            adv * FALLBACK_CJK_ADVANCE_MULTIPLIER
        } else {
            adv
        };
        if g == '\n' || wrap_limit.is_some_and(|lim| cx > 0.0 && cx + step > lim) {
            push_fallback_line(
                out,
                line_idx,
                safe_font_size,
                line_height_px,
                cx,
                line_start,
                idx,
            );
            (line_idx, line_start, cx) = (line_idx + 1, idx + usize::from(g == '\n'), 0.0);
            if g == '\n' {
                (out.char_x_offsets[idx], out.char_x_offsets[idx + 1]) = (0.0, 0.0);
                (out.char_line_indices[idx], out.char_line_indices[idx + 1]) = (line_idx, line_idx);
                continue;
            }
        }
        (out.char_x_offsets[idx], out.char_line_indices[idx]) = (cx, line_idx);
        if !g.is_whitespace() {
            record_notdef_glyph_warning(font_family, g);
            let w = if wide { gw * 2 } else { gw };
            out.glyphs.push(build_fallback_notdef_glyph(
                cx,
                line_idx,
                line_height_px,
                scale,
                w,
                gh,
                idx as u32,
            ));
        }
        cx += step;
        (out.char_x_offsets[idx + 1], out.char_line_indices[idx + 1]) = (cx, line_idx);
    }
    finalize_fallback_runs(
        out,
        line_idx,
        safe_font_size,
        line_height_px,
        cx,
        line_start,
        total_chars,
        has_rtl,
    );
}

#[allow(clippy::too_many_arguments)]
fn finalize_fallback_runs(
    out: &mut ShapedRunsOutput,
    line_idx: usize,
    safe_font_size: f32,
    line_height_px: f32,
    cx: f32,
    line_start: usize,
    total_chars: usize,
    has_rtl: bool,
) {
    push_fallback_line(
        out,
        line_idx,
        safe_font_size,
        line_height_px,
        cx,
        line_start,
        total_chars,
    );
    if has_rtl && line_idx == 0 && cx > 0.0 {
        for x in &mut out.char_x_offsets {
            *x = (cx - *x).max(0.0);
        }
    }
    out.measured_height = out
        .measured_height
        .max((line_idx + 1) as f32 * line_height_px);
}

fn build_fallback_notdef_glyph(
    cx: f32,
    line_idx: usize,
    line_height_px: f32,
    scale: f32,
    w: u32,
    gh: u32,
    char_idx: u32,
) -> RasterizedGlyph {
    let line_top = line_idx as f32 * line_height_px;
    RasterizedGlyph {
        x: cx,
        y: line_top + (line_height_px - (gh as f32 / scale)) * 0.5,
        width: w,
        height: gh,
        cache_key: 0xDEAD_0000_0000_0000 | ((w as u64) << 16) | (gh as u64),
        is_notdef: true,
        is_color: false,
        span_index: 0,
        char_start: char_idx,
        char_end: char_idx + 1,
        glyph_color: None,
        alpha_mask: build_hollow_notdef_mask(w, gh),
        color_rgba: None,
    }
}

fn push_fallback_line(
    out: &mut ShapedRunsOutput,
    line_idx: usize,
    safe_font_size: f32,
    line_height_px: f32,
    cx: f32,
    start_char: usize,
    end_char: usize,
) {
    let ascent = safe_font_size * 0.80;
    let descent = safe_font_size * 0.20;
    let line_top = line_idx as f32 * line_height_px;
    let half_lead = (line_height_px - (ascent + descent)) * 0.5;
    out.lines.push(TextLineMetrics {
        line_index: line_idx,
        line_top,
        line_height: line_height_px,
        baseline_y: line_top + half_lead + ascent,
        ascent,
        descent,
        line_width: cx,
        start_char,
        end_char,
    });
    out.measured_width = out.measured_width.max(cx);
}
