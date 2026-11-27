//! CSS Text 3 whitespace collapsing, `word-break: keep-all` Hangul/CJK eojeol line splitting,
//! single-line `text-overflow: ellipsis`, and multi-line `line-clamp` truncation helpers.

use crate::font_stacks::{
    populate_buffer_lines, shape_rich_cosmic_buffer, FontContext, SanitizedRichSpan,
};
use crate::rasterize::{rasterize_single_cosmic_glyph, LineLayoutConfig, ShapedRunsOutput};
use crate::RasterizedGlyph;
use cosmic_text::{Align, AttrsOwned, Buffer, Metrics, Wrap};
use krui_style::{
    FontFamilyStack, OverflowWrapMode, TextAlign, TextDirection, WhiteSpaceMode, WordBreakMode,
};
use std::ops::Range;

/// Normalizes `text` according to CSS Text 3 `white-space` collapsing rules.
pub(crate) fn normalize_white_space(text: &str, mode: WhiteSpaceMode) -> String {
    if !mode.collapses_spaces() {
        return text.to_owned();
    }
    if mode == WhiteSpaceMode::PreLine {
        return normalize_pre_line(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut in_ws = false;
    for ch in text.chars() {
        if ch.is_ascii_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(ch);
            in_ws = false;
        }
    }
    out
}

fn normalize_pre_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (idx, line) in text.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let mut in_ws = false;
        for ch in line.chars() {
            if ch == '\r' {
                continue;
            }
            if ch == ' ' || ch == '\t' {
                if !in_ws {
                    out.push(' ');
                    in_ws = true;
                }
            } else {
                out.push(ch);
                in_ws = false;
            }
        }
    }
    out
}

/// Normalizes multi-span rich paragraph text according to `mode`, returning the joined string.
pub(crate) fn normalize_spans_white_space(
    spans: &mut [SanitizedRichSpan],
    mode: WhiteSpaceMode,
) -> String {
    let mut full = String::new();
    if !mode.collapses_spaces() {
        for sp in spans.iter() {
            full.push_str(&sp.text);
        }
        return full;
    }
    let preserve_nl = mode.preserves_newlines();
    let mut prev_space = false;
    for sp in spans.iter_mut() {
        let mut clean = String::with_capacity(sp.text.len());
        for ch in sp.text.chars() {
            if ch == '\r' {
                continue;
            }
            if preserve_nl && ch == '\n' {
                clean.push('\n');
                prev_space = false;
            } else if ch.is_ascii_whitespace() {
                if !prev_space {
                    clean.push(' ');
                    prev_space = true;
                }
            } else {
                clean.push(ch);
                prev_space = false;
            }
        }
        full.push_str(&clean);
        sp.text = clean;
    }
    full
}

/// Maps KRui wrapping modes to `cosmic_text::Wrap`.
pub(crate) const fn cosmic_wrap_for_config(cfg: &LineLayoutConfig) -> Wrap {
    if !cfg.white_space.allows_wrap() {
        Wrap::None
    } else if matches!(cfg.word_break, WordBreakMode::BreakAll) {
        Wrap::Glyph
    } else if matches!(
        cfg.overflow_wrap,
        OverflowWrapMode::BreakWord | OverflowWrapMode::Anywhere
    ) {
        Wrap::WordOrGlyph
    } else {
        Wrap::Word
    }
}

/// Computes `(slice_start_byte, slice_end_byte)` lines for `word-break: keep-all` so Hangul/CJK
/// words only wrap at whitespace boundaries (unless `overflow-wrap` forces emergency glyph wrap).
pub(crate) fn compute_keep_all_line_slices(
    unwrapped_buf: &cosmic_text::Buffer,
    full_text: &str,
    max_w: f32,
    letter_spacing_px: f32,
) -> Vec<(usize, usize)> {
    let mut slices = Vec::new();
    let mut para_offsets = Vec::new();
    let mut cur_b = 0_usize;
    for raw_line in full_text.split('\n') {
        para_offsets.push((cur_b, cur_b + raw_line.len()));
        cur_b += raw_line.len() + 1;
    }
    let mut runs_by_line: Vec<Vec<(usize, usize, f32)>> = vec![Vec::new(); para_offsets.len()];
    for run in unwrapped_buf.layout_runs() {
        if let Some(bucket) = runs_by_line.get_mut(run.line_i) {
            for g in run.glyphs.iter() {
                bucket.push((g.start, g.end, (g.w + letter_spacing_px).max(0.0)));
            }
        }
    }
    for (p_idx, &(ps, pe)) in para_offsets.iter().enumerate() {
        split_single_keep_all_para(
            &full_text[ps..pe],
            ps,
            &runs_by_line[p_idx],
            max_w,
            &mut slices,
        );
    }
    if slices.is_empty() {
        slices.push((0, 0));
    }
    slices
}

fn split_single_keep_all_para(
    para: &str,
    para_base: usize,
    glyphs: &[(usize, usize, f32)],
    max_w: f32,
    out: &mut Vec<(usize, usize)>,
) {
    let words = collect_non_ws_ranges(para);
    if words.is_empty() {
        out.push((para_base, para_base + para.len()));
        return;
    }
    let range_width = |s: usize, e: usize| -> f32 {
        glyphs
            .iter()
            .filter(|&&(gs, ge, _)| ge > s && gs < e)
            .map(|&(_, _, gw)| gw)
            .sum()
    };
    let (mut cur_s, mut cur_e) = words[0];
    for &(ws, we) in words.iter().skip(1) {
        if range_width(cur_s, we) <= max_w + 0.5 {
            cur_e = we;
        } else {
            out.push((para_base + cur_s, para_base + cur_e));
            cur_s = ws;
            cur_e = we;
        }
    }
    out.push((para_base + cur_s, para_base + cur_e));
}

fn collect_non_ws_ranges(para: &str) -> Vec<(usize, usize)> {
    let mut words = Vec::new();
    let mut active_start: Option<usize> = None;
    for (b, ch) in para.char_indices() {
        if ch.is_whitespace() {
            if let Some(s) = active_start.take() {
                words.push((s, b));
            }
        } else if active_start.is_none() {
            active_start = Some(b);
        }
    }
    if let Some(s) = active_start {
        words.push((s, para.len()));
    }
    words
}

/// Shapes a `word-break: keep-all` paragraph by breaking at Hangul/CJK whitespace eojeols first.
pub(crate) fn shape_keep_all_cosmic_buffer(
    ctx: &mut FontContext,
    full_text: &str,
    cfg: LineLayoutConfig,
    safe_cosmic_lh: f32,
    strut_attrs: &AttrsOwned,
    ranged_attrs: &[(Range<usize>, AttrsOwned)],
    line_align: Option<Align>,
) -> (Buffer, Vec<usize>) {
    let mut unwrapped = Buffer::new(
        &mut ctx.font_system,
        Metrics::new(cfg.font_size, safe_cosmic_lh),
    );
    unwrapped.set_wrap(&mut ctx.font_system, Wrap::None);
    unwrapped.set_size(&mut ctx.font_system, None, None);
    let _ = populate_buffer_lines(
        &mut unwrapped,
        full_text,
        None,
        strut_attrs,
        ranged_attrs,
        None,
    );
    unwrapped.shape_until_scroll(&mut ctx.font_system, false);
    let slices = compute_keep_all_line_slices(
        &unwrapped,
        full_text,
        cfg.max_width.unwrap_or(f32::INFINITY),
        cfg.letter_spacing_px,
    );
    let mut buffer = Buffer::new(
        &mut ctx.font_system,
        Metrics::new(cfg.font_size, safe_cosmic_lh),
    );
    let emergency_wrap = if matches!(
        cfg.overflow_wrap,
        OverflowWrapMode::BreakWord | OverflowWrapMode::Anywhere
    ) {
        Wrap::Glyph
    } else {
        Wrap::None
    };
    buffer.set_wrap(&mut ctx.font_system, emergency_wrap);
    buffer.set_size(&mut ctx.font_system, cfg.max_width, None);
    let offsets = populate_buffer_lines(
        &mut buffer,
        full_text,
        Some(&slices),
        strut_attrs,
        ranged_attrs,
        line_align,
    );
    buffer.shape_until_scroll(&mut ctx.font_system, false);
    (buffer, offsets)
}

/// Applies single-line `text-overflow: ellipsis` or final-line `line-clamp` ellipsis if triggered.
pub(crate) fn maybe_apply_line_ellipsis(
    ctx: &mut FontContext,
    spans: &[SanitizedRichSpan],
    cfg: LineLayoutConfig,
    (line_idx, max_lines, runs_count, glyph_start): (usize, usize, usize, usize),
    (baseline_y, scale, default_family): (f32, f32, FontFamilyStack),
    out: &mut ShapedRunsOutput,
) {
    let (line_w, end_c) = out
        .lines
        .last()
        .map_or((0.0, 0), |l| (l.line_width, l.end_char as u32));
    let clamped_more =
        cfg.line_clamp.is_some() && line_idx + 1 == max_lines && runs_count > max_lines;
    let width_ellip =
        cfg.text_overflow_ellipsis && cfg.clamp_width.is_some_and(|cw| line_w > cw + 0.5);
    if clamped_more || width_ellip {
        let new_w = truncate_line_and_append_ellipsis(
            ctx,
            spans,
            cfg,
            (baseline_y, scale, default_family, end_c),
            cfg.clamp_width.unwrap_or(line_w),
            glyph_start,
            &mut out.glyphs,
        );
        if let Some(last) = out.lines.last_mut() {
            last.line_width = new_w;
        }
    }
}

/// Truncates `glyphs[line_glyph_start..]` to fit `"…"` within `avail_w` and appends `"…"`.
pub(crate) fn truncate_line_and_append_ellipsis(
    ctx: &mut FontContext,
    spans: &[SanitizedRichSpan],
    cfg: LineLayoutConfig,
    (baseline_y, scale, default_family, end_char): (f32, f32, FontFamilyStack, u32),
    avail_w: f32,
    line_glyph_start: usize,
    glyphs: &mut Vec<RasterizedGlyph>,
) -> f32 {
    let (ellip_w, mut ellip_glyphs) = shape_ellipsis_glyphs(
        ctx,
        spans,
        cfg,
        (baseline_y, scale, default_family, end_char),
    );
    let budget = (avail_w - ellip_w).max(0.0);
    let mut keep_count = 0_usize;
    let mut prefix_end_x = 0.0_f32;
    for g in &glyphs[line_glyph_start..] {
        let right = g.x + (g.width as f32) / scale;
        if right <= budget + 0.5 {
            keep_count += 1;
            prefix_end_x = prefix_end_x.max(right);
        } else {
            break;
        }
    }
    glyphs.truncate(line_glyph_start + keep_count);
    for mut eg in ellip_glyphs.drain(..) {
        eg.x += prefix_end_x;
        glyphs.push(eg);
    }
    (prefix_end_x + ellip_w).max(0.0)
}

fn shape_ellipsis_glyphs(
    ctx: &mut FontContext,
    spans: &[SanitizedRichSpan],
    cfg: LineLayoutConfig,
    (baseline_y, scale, default_family, end_char): (f32, f32, FontFamilyStack, u32),
) -> (f32, Vec<RasterizedGlyph>) {
    let ellip_str = "…";
    let ref_span = spans.last().or_else(|| spans.first());
    let ellip_span = [SanitizedRichSpan {
        text: ellip_str.to_owned(),
        font_weight: ref_span.map_or(400, |s| s.font_weight),
        italic: ref_span.is_some_and(|s| s.italic),
        font_family: ref_span.map_or(default_family, |s| s.font_family),
        color: ref_span.and_then(|s| s.color),
        variations: ref_span.map_or_default(|s| s.variations),
    }];
    let mut ecfg = cfg;
    ecfg.max_width = None;
    ecfg.clamp_width = None;
    ecfg.align = TextAlign::Left;
    ecfg.direction = TextDirection::Ltr;
    ecfg.white_space = WhiteSpaceMode::NoWrap;
    let (buf, _) = shape_rich_cosmic_buffer(ctx, &ellip_span, ellip_str, ecfg);
    let mut out = Vec::new();
    let mut width = 0.0_f32;
    if let Some(run) = buf.0.layout_runs().next() {
        for g in run.glyphs.iter() {
            if let Some(rg) = rasterize_single_cosmic_glyph(
                ctx,
                ellip_str,
                g,
                (width, g.w, 0, ellip_str.len(), end_char, end_char),
                (
                    cfg.font_size,
                    baseline_y,
                    scale,
                    ellip_span[0].font_family,
                    ellip_span[0].font_weight,
                    ellip_span[0].variations,
                ),
            ) {
                out.push(rg);
            }
            width += g.w;
        }
    }
    (width.max(cfg.font_size * 0.5), out)
}
