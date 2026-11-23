use crate::fallback::build_cosmic_notdef_glyph;
use crate::font_metrics::{compute_w3c_line_box, CachedFaceMeta, LineBoxMetrics};
use crate::font_stacks::{shape_rich_cosmic_buffer, FontContext, SanitizedRichSpan};
use crate::font_var_raster::{
    compute_glyph_atlas_cache_key, rasterize_variable_glyph, resolve_active_fvar_settings,
    varied_glyph_advance_px,
};
pub use crate::font_var_raster::{decode_swash_image, DecodedGlyphBitmap};
use crate::{record_notdef_glyph_warning, RasterizedGlyph, TextLineMetrics};
use krui_style::{
    Color, FontFamilyStack, FontVariationList, LineHeight, TextAlign, TextBoxEdge, TextBoxTrim,
    TextDirection,
};

#[derive(Clone, Copy)]
pub(crate) struct LineLayoutConfig {
    pub(crate) font_size: f32,
    pub(crate) line_height: LineHeight,
    pub(crate) text_box_trim: TextBoxTrim,
    pub(crate) text_box_edge: TextBoxEdge,
    pub(crate) max_width: Option<f32>,
    pub(crate) align: TextAlign,
    pub(crate) direction: TextDirection,
}

pub(crate) struct ShapedRunsOutput {
    pub(crate) char_x_offsets: Vec<f32>,
    pub(crate) char_line_indices: Vec<usize>,
    pub(crate) lines: Vec<TextLineMetrics>,
    pub(crate) glyphs: Vec<RasterizedGlyph>,
    pub(crate) measured_width: f32,
    pub(crate) measured_height: f32,
}

pub(crate) fn measure_with_cosmic(
    ctx: &mut FontContext,
    spans: &[SanitizedRichSpan],
    sanitized_text: &str,
    cfg: LineLayoutConfig,
) -> (f32, f32) {
    let (buffer, strut_meta) = shape_rich_cosmic_buffer(
        ctx,
        spans,
        sanitized_text,
        cfg.font_size,
        cfg.max_width,
        cfg.align,
        cfg.direction,
    );
    let runs_count = buffer.layout_runs().count();
    let trailing_nl = sanitized_text.ends_with('\n');
    let total_lines = (runs_count + usize::from(trailing_nl)).max(1);
    let (mut mw, mut mh) = (0.0_f32, 0.0_f32);
    for (idx, run) in buffer.layout_runs().enumerate() {
        let line_meta = merge_run_face_meta(ctx, &strut_meta, &run);
        let lbox = compute_w3c_line_box(
            &line_meta,
            cfg.font_size,
            cfg.line_height,
            cfg.text_box_trim,
            cfg.text_box_edge,
            idx == 0,
            idx + 1 == total_lines,
        );
        let line_w = compute_run_varied_width(ctx, spans, &run);
        mw = mw.max(line_w);
        mh += lbox.line_height;
    }
    if trailing_nl || runs_count == 0 {
        let lbox = compute_w3c_line_box(
            &strut_meta,
            cfg.font_size,
            cfg.line_height,
            cfg.text_box_trim,
            cfg.text_box_edge,
            runs_count == 0,
            true,
        );
        mh += lbox.line_height;
    }
    (mw, mh)
}

fn merge_run_face_meta(
    ctx: &mut FontContext,
    strut: &CachedFaceMeta,
    run: &cosmic_text::LayoutRun<'_>,
) -> CachedFaceMeta {
    let mut out = strut.clone();
    for g in run.glyphs.iter() {
        let gm = ctx.ensure_face_meta(g.font_id);
        out.ascent_em = out.ascent_em.max(gm.ascent_em);
        out.descent_em = out.descent_em.max(gm.descent_em);
        out.line_gap_em = out.line_gap_em.max(gm.line_gap_em);
    }
    out
}

fn compute_run_varied_width(
    ctx: &mut FontContext,
    spans: &[SanitizedRichSpan],
    run: &cosmic_text::LayoutRun<'_>,
) -> f32 {
    let mut delta = 0.0_f32;
    for g in run.glyphs.iter() {
        let (w, vars) = span_weight_and_vars(spans, g.metadata);
        let meta = ctx.ensure_face_meta(g.font_id);
        let active = resolve_active_fvar_settings(&meta, g.font_size, w, vars);
        if !active.is_empty() && !g.level.is_rtl() {
            if let Some(font) = ctx.font_system.get_font(g.font_id) {
                if let Some(new_adv) =
                    varied_glyph_advance_px(&font, g.glyph_id, g.font_size, &active)
                {
                    delta += new_adv - g.w;
                }
            }
        }
    }
    (run.line_w + delta).max(0.0)
}

fn span_weight_and_vars(spans: &[SanitizedRichSpan], meta_idx: usize) -> (u16, FontVariationList) {
    spans
        .get(meta_idx)
        .map_or((400, FontVariationList::EMPTY), |s| {
            (s.font_weight, s.variations)
        })
}

pub(crate) fn build_byte_to_char_table(sanitized_text: &str) -> (Vec<usize>, usize) {
    let mut byte_to_char = vec![0_usize; sanitized_text.len() + 1];
    let mut char_count = 0_usize;
    for (b_idx, ch) in sanitized_text.char_indices() {
        let next_b = b_idx + ch.len_utf8();
        for slot in &mut byte_to_char[b_idx..next_b] {
            *slot = char_count;
        }
        char_count += 1;
    }
    if let Some(last) = byte_to_char.last_mut() {
        *last = char_count;
    }
    (byte_to_char, char_count)
}

fn build_line_byte_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0_usize];
    for (b, ch) in text.char_indices() {
        if ch == '\n' {
            offsets.push(b + 1);
        }
    }
    offsets
}

pub(crate) fn rasterize_cosmic_runs(
    ctx: &mut FontContext,
    spans: &[SanitizedRichSpan],
    sanitized_text: &str,
    byte_to_char: &[usize],
    char_count: usize,
    cfg: LineLayoutConfig,
    scale: f32,
) -> ShapedRunsOutput {
    let mut out = ShapedRunsOutput {
        char_x_offsets: vec![0.0; char_count + 1],
        char_line_indices: vec![0; char_count + 1],
        lines: Vec::new(),
        glyphs: Vec::new(),
        measured_width: 0.0,
        measured_height: 0.0,
    };
    let mut visited = vec![false; char_count + 1];
    let line_offsets = build_line_byte_offsets(sanitized_text);
    let (buffer, strut_meta) = shape_rich_cosmic_buffer(
        ctx,
        spans,
        sanitized_text,
        cfg.font_size,
        cfg.max_width,
        cfg.align,
        cfg.direction,
    );
    let runs_count = buffer.layout_runs().count();
    let trailing_nl = sanitized_text.ends_with('\n');
    let total_lines = (runs_count + usize::from(trailing_nl)).max(1);
    let base_fam = spans
        .first()
        .map_or(FontFamilyStack::FluentUI, |s| s.font_family);
    let mut cur_y = 0.0_f32;
    for (line_idx, run) in buffer.layout_runs().enumerate() {
        let base_b = line_offsets.get(run.line_i).copied().unwrap_or(0);
        let line_meta = merge_run_face_meta(ctx, &strut_meta, &run);
        let lbox = compute_w3c_line_box(
            &line_meta,
            cfg.font_size,
            cfg.line_height,
            cfg.text_box_trim,
            cfg.text_box_edge,
            line_idx == 0,
            line_idx + 1 == total_lines,
        );
        rasterize_single_cosmic_run(
            ctx,
            spans,
            sanitized_text,
            byte_to_char,
            (char_count, line_idx, base_b),
            &run,
            (cur_y, lbox, cfg, scale, base_fam),
            &mut visited,
            &mut out,
        );
        cur_y += lbox.line_height;
    }
    finalize_cosmic_runs(
        sanitized_text,
        char_count,
        (&strut_meta, cfg, cur_y, runs_count == 0),
        &mut visited,
        &mut out,
    );
    out
}

fn finalize_cosmic_runs(
    text: &str,
    char_count: usize,
    (strut_meta, cfg, cur_y, was_empty): (&CachedFaceMeta, LineLayoutConfig, f32, bool),
    visited: &mut [bool],
    out: &mut ShapedRunsOutput,
) {
    let need_trailing = was_empty
        || (text.ends_with('\n') && out.lines.last().is_none_or(|l| l.start_char < char_count));
    if need_trailing {
        let idx = out.lines.len();
        let lbox = compute_w3c_line_box(
            strut_meta,
            cfg.font_size,
            cfg.line_height,
            cfg.text_box_trim,
            cfg.text_box_edge,
            idx == 0,
            true,
        );
        out.lines.push(TextLineMetrics {
            line_index: idx,
            line_top: cur_y,
            line_height: lbox.line_height,
            baseline_y: cur_y + lbox.baseline_offset,
            ascent: lbox.ascent,
            descent: lbox.descent,
            line_width: 0.0,
            start_char: char_count,
            end_char: char_count,
        });
        out.measured_height = out.measured_height.max(cur_y + lbox.line_height);
        if char_count < out.char_x_offsets.len() {
            out.char_x_offsets[char_count] = 0.0;
            out.char_line_indices[char_count] = idx;
            visited[char_count] = true;
        }
    }
    for (i, &vis) in visited.iter().enumerate().take(char_count + 1).skip(1) {
        if !vis {
            out.char_x_offsets[i] = out.char_x_offsets[i - 1];
            out.char_line_indices[i] = out.char_line_indices[i - 1];
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rasterize_single_cosmic_run(
    ctx: &mut FontContext,
    spans: &[SanitizedRichSpan],
    text: &str,
    byte_to_char: &[usize],
    (char_count, line_idx, base_b): (usize, usize, usize),
    run: &cosmic_text::LayoutRun<'_>,
    (line_top, lbox, cfg, scale, default_family): (
        f32,
        LineBoxMetrics,
        LineLayoutConfig,
        f32,
        FontFamilyStack,
    ),
    visited: &mut [bool],
    out: &mut ShapedRunsOutput,
) {
    let baseline_y = line_top + lbox.baseline_offset;
    let varied_advances = collect_varied_glyph_boxes(ctx, spans, run, cfg);
    let line_w = (run.line_w + varied_advances.1).max(0.0);
    out.measured_width = out.measured_width.max(line_w);
    out.measured_height = out.measured_height.max(line_top + lbox.line_height);
    let (mut sc, mut ec) = (char_count, 0_usize);
    for (g, &(adj_x, adj_w)) in run.glyphs.iter().zip(varied_advances.0.iter()) {
        let sb = (base_b + g.start).min(text.len());
        let eb = (base_b + g.end).min(text.len());
        let (cs, ce) = (byte_to_char[sb], byte_to_char[eb]);
        (sc, ec) = (sc.min(cs), ec.max(ce));
        record_glyph_char_offsets(
            (adj_x, adj_w, g.level.is_rtl()),
            cs,
            ce,
            line_idx,
            out,
            visited,
        );
        let span_fam = spans
            .get(g.metadata)
            .map_or(default_family, |sp| sp.font_family);
        let (req_w, span_vars) = span_weight_and_vars(spans, g.metadata);
        if let Some(rg) = rasterize_single_cosmic_glyph(
            ctx,
            text,
            g,
            (adj_x, adj_w, sb, eb, cs as u32, ce as u32),
            (cfg.font_size, baseline_y, scale, span_fam, req_w, span_vars),
        ) {
            out.glyphs.push(rg);
        }
    }
    if sc > ec {
        let c0 = byte_to_char[base_b.min(text.len())];
        (sc, ec) = (c0, c0);
        if !visited[sc] || out.char_line_indices[sc] < line_idx {
            (
                out.char_x_offsets[sc],
                out.char_line_indices[sc],
                visited[sc],
            ) = (0.0, line_idx, true);
        }
    }
    out.lines.push(TextLineMetrics {
        line_index: line_idx,
        line_top,
        line_height: lbox.line_height,
        baseline_y,
        ascent: lbox.ascent,
        descent: lbox.descent,
        line_width: line_w,
        start_char: sc,
        end_char: ec,
    });
}

fn collect_varied_glyph_boxes(
    ctx: &mut FontContext,
    spans: &[SanitizedRichSpan],
    run: &cosmic_text::LayoutRun<'_>,
    cfg: LineLayoutConfig,
) -> (Vec<(f32, f32)>, f32) {
    let mut raw = Vec::with_capacity(run.glyphs.len());
    let mut shift = 0.0_f32;
    for g in run.glyphs.iter() {
        let (w, vars) = span_weight_and_vars(spans, g.metadata);
        let meta = ctx.ensure_face_meta(g.font_id);
        let active = resolve_active_fvar_settings(&meta, g.font_size, w, vars);
        let mut gw = g.w;
        if !active.is_empty() && !g.level.is_rtl() {
            if let Some(font) = ctx.font_system.get_font(g.font_id) {
                if let Some(new_adv) =
                    varied_glyph_advance_px(&font, g.glyph_id, g.font_size, &active)
                {
                    gw = new_adv;
                }
            }
        }
        raw.push((g.x + shift, gw));
        shift += gw - g.w;
    }
    if cfg.max_width.is_some() && shift.abs() > 1e-3 {
        let align_fix = match cfg.align {
            TextAlign::Center => -0.5 * shift,
            TextAlign::Right => -shift,
            TextAlign::Left if cfg.direction == TextDirection::Rtl => -shift,
            TextAlign::Left => 0.0,
        };
        if align_fix != 0.0 {
            for (x, _) in &mut raw {
                *x += align_fix;
            }
        }
    }
    (raw, shift)
}

fn record_glyph_char_offsets(
    (gx, gw, is_rtl): (f32, f32, bool),
    c_start: usize,
    c_end: usize,
    line_idx: usize,
    out: &mut ShapedRunsOutput,
    char_visited: &mut [bool],
) {
    let char_x_offsets = &mut out.char_x_offsets;
    let char_line_indices = &mut out.char_line_indices;
    let span = (c_end.saturating_sub(c_start)).max(1) as f32;
    let step_w = gw / span;
    let glyph_right = gx + gw;
    let new_wrapped_line = char_visited[c_start] && char_line_indices[c_start] < line_idx;
    if !is_rtl && (!char_visited[c_start] || new_wrapped_line) {
        char_x_offsets[c_start] = gx;
        char_line_indices[c_start] = line_idx;
        char_visited[c_start] = true;
    }
    for (k, c_idx) in (c_start..c_end).enumerate() {
        let kf = k as f32;
        if is_rtl {
            if !char_visited[c_idx] || char_line_indices[c_idx] < line_idx {
                char_x_offsets[c_idx] = glyph_right - kf * step_w;
            }
            char_x_offsets[c_idx + 1] = glyph_right - (kf + 1.0) * step_w;
            (char_line_indices[c_idx], char_line_indices[c_idx + 1]) = (line_idx, line_idx);
            (char_visited[c_idx], char_visited[c_idx + 1]) = (true, true);
        } else {
            if k > 0 {
                char_x_offsets[c_idx] = gx + kf * step_w;
                (char_line_indices[c_idx], char_visited[c_idx]) = (line_idx, true);
            }
            char_x_offsets[c_idx + 1] = gx + (kf + 1.0) * step_w;
            (char_line_indices[c_idx + 1], char_visited[c_idx + 1]) = (line_idx, true);
        }
    }
}

fn rasterize_single_cosmic_glyph(
    ctx: &mut FontContext,
    sanitized_text: &str,
    layout_glyph: &cosmic_text::LayoutGlyph,
    (adj_x, adj_w, start_b, end_b, cs, ce): (f32, f32, usize, usize, u32, u32),
    (safe_font_size, baseline_y, scale, font_family, req_w, span_vars): (
        f32,
        f32,
        f32,
        FontFamilyStack,
        u16,
        FontVariationList,
    ),
) -> Option<RasterizedGlyph> {
    let mut zero_glyph = layout_glyph.clone();
    (zero_glyph.x, zero_glyph.y) = (0.0, 0.0);
    let physical = zero_glyph.physical((0.0, 0.0), scale);
    let face_meta = ctx.ensure_face_meta(physical.cache_key.font_id);
    let active_vars =
        resolve_active_fvar_settings(&face_meta, layout_glyph.font_size, req_w, span_vars);
    let glyph_key = compute_glyph_atlas_cache_key(physical.cache_key, &active_vars);
    let span_index = layout_glyph.metadata.min(u16::MAX as usize) as u16;
    let glyph_color = layout_glyph
        .color_opt
        .map(|c| Color::rgba(c.r(), c.g(), c.b(), c.a()));

    if physical.cache_key.glyph_id == 0 {
        if let Some(ch) = sanitized_text[start_b..end_b].chars().next() {
            record_notdef_glyph_warning(font_family, ch);
        }
        return Some(build_cosmic_notdef_glyph(
            adj_x,
            adj_w,
            safe_font_size,
            baseline_y,
            scale,
            glyph_key,
            (span_index, cs, ce, glyph_color),
        ));
    }
    let decoded_and_place = if !active_vars.is_empty() {
        let font = ctx.font_system.get_font(physical.cache_key.font_id)?;
        let img =
            rasterize_variable_glyph(&mut ctx.scale_ctx, &font, physical.cache_key, &active_vars)?;
        (decode_swash_image(&img), img.placement)
    } else {
        let img = ctx
            .swash_cache
            .get_image(&mut ctx.font_system, physical.cache_key)
            .as_ref()?;
        (decode_swash_image(img), img.placement)
    };
    let (decoded, placement) = decoded_and_place;
    let (w, h) = (placement.width, placement.height);
    if w == 0 || h == 0 {
        return None;
    }
    let gx_phys = (adj_x * scale).round() + placement.left as f32;
    Some(RasterizedGlyph {
        x: gx_phys / scale,
        y: baseline_y - (placement.top as f32) / scale,
        width: w,
        height: h,
        cache_key: glyph_key,
        is_notdef: false,
        is_color: decoded.is_color,
        span_index,
        char_start: cs,
        char_end: ce,
        glyph_color,
        alpha_mask: decoded.alpha_mask,
        color_rgba: decoded.color_rgba,
    })
}
