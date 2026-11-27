//! Styled multi-span rich paragraph measurement and shaping helpers for [`TextEngine`].

use crate::cache::{
    normalize_font_weight, quantize_font_size, quantize_letter_spacing, quantize_max_width,
    quantize_scale, quantize_style_line_height, quantize_variations, SpanStyleKey, TextCacheKey,
};
use crate::fallback::measure_fallback_dimensions;
use crate::font_stacks::SanitizedRichSpan;
use crate::rasterize::{measure_with_cosmic, LineLayoutConfig};
use crate::text_wrap_ellipsis::normalize_spans_white_space;
use crate::{build_shaped_layout, ShapedTextLayout, TextEngine, MIN_SHAPING_FONT_SIZE_PX};
use krui_style::{sanitize_untrusted_text, FontFamilyStack, StyledTextSpan, TextAlign, TextStyle};

fn prepare_styled_spans(
    spans: &[StyledTextSpan],
    style: &TextStyle,
) -> (Vec<SanitizedRichSpan>, Vec<SpanStyleKey>, String) {
    let mut clean_spans = Vec::with_capacity(spans.len());
    for sp in spans {
        clean_spans.push(SanitizedRichSpan {
            text: sanitize_untrusted_text(&sp.text),
            font_weight: sp.font_weight,
            italic: sp.italic,
            font_family: sp.font_family,
            color: sp.color,
            variations: sp.variations,
        });
    }
    let full_text = normalize_spans_white_space(&mut clean_spans, style.white_space);
    let span_keys = clean_spans
        .iter()
        .map(|sp| SpanStyleKey {
            text: sp.text.clone(),
            font_weight: normalize_font_weight(sp.font_weight),
            italic: sp.italic,
            font_family: sp.font_family,
            color: sp.color.map(|c| [c.r, c.g, c.b, c.a]),
            variations: quantize_variations(sp.variations),
        })
        .collect();
    (clean_spans, span_keys, full_text)
}

fn build_rich_cache_key(
    span_keys: Vec<SpanStyleKey>,
    style: &TextStyle,
    max_width: Option<f32>,
    eff_align: TextAlign,
    scale: f32,
    epoch: u64,
) -> TextCacheKey {
    TextCacheKey {
        spans: span_keys,
        font_size_q: quantize_font_size(style.font_size),
        line_height_q: quantize_style_line_height(style.line_height, style.font_size),
        letter_spacing_q: quantize_letter_spacing(style.letter_spacing_px),
        text_box_trim: style.text_box_trim,
        text_box_edge: style.text_box_edge,
        white_space: style.white_space,
        word_break: style.word_break,
        overflow_wrap: style.overflow_wrap,
        text_overflow_ellipsis: style.text_overflow_ellipsis,
        line_clamp: style.line_clamp,
        max_width_q: quantize_max_width(max_width),
        text_align: eff_align,
        direction: style.direction,
        scale_pct: quantize_scale(scale),
        font_face_epoch: epoch,
    }
}

impl TextEngine {
    /// Measures a multi-span inline paragraph without glyph rasterization using full [`TextStyle`].
    pub fn measure_rich_text(
        &mut self,
        spans: &[StyledTextSpan],
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> (f32, f32) {
        let (clean_spans, span_keys, full_text) = prepare_styled_spans(spans, style);
        if full_text.is_empty() {
            return (0.0, 0.0);
        }
        let eff_align = if max_width.is_some() {
            style.text_align
        } else {
            TextAlign::Left
        };
        let epoch = self.current_epoch();
        let cache_key = build_rich_cache_key(span_keys, style, max_width, eff_align, 1.0, epoch);
        if let Some(cached) = self.measure_cache.get_and_touch(&cache_key) {
            return cached;
        }
        if let Some(shaped) = self.layout_cache.peek(&cache_key) {
            return (shaped.measured_width.ceil(), shaped.measured_height.ceil());
        }
        let safe_fs = style.font_size.max(MIN_SHAPING_FONT_SIZE_PX);
        let cfg = LineLayoutConfig::from_style(style, max_width, eff_align);
        let result = self.with_font_context(|ctx| {
            let (mut mw, mut mh) = measure_with_cosmic(ctx, &clean_spans, &full_text, cfg);
            if mw == 0.0 && !full_text.trim().is_empty() {
                let lh = style.line_height.resolve_px(safe_fs, safe_fs * 1.2);
                (mw, mh) = measure_fallback_dimensions(&full_text, safe_fs, lh, cfg.max_width);
            }
            (mw.ceil(), mh.ceil())
        });
        self.measure_cache.insert(cache_key, result);
        result
    }

    /// Shapes and rasterizes a multi-span inline paragraph preserving [`TextStyle`] line box and BiDi rules.
    pub fn shape_styled_rich_text(
        &mut self,
        spans: &[StyledTextSpan],
        style: &TextStyle,
        max_width: Option<f32>,
        scale_factor: f32,
    ) -> ShapedTextLayout {
        let scale = scale_factor.clamp(1.0, 4.0);
        let (clean_spans, span_keys, full_text) = prepare_styled_spans(spans, style);
        let eff_align = if max_width.is_some() {
            style.text_align
        } else {
            TextAlign::Left
        };
        let epoch = self.current_epoch();
        let cache_key = build_rich_cache_key(span_keys, style, max_width, eff_align, scale, epoch);
        if let Some(cached) = self.layout_cache.get_and_touch(&cache_key) {
            return cached;
        }
        let cfg = LineLayoutConfig::from_style(style, max_width, eff_align);
        let mut fallback_spans = clean_spans;
        if fallback_spans.is_empty() {
            fallback_spans.push(SanitizedRichSpan {
                text: String::new(),
                font_weight: style.font_weight,
                italic: style.italic,
                font_family: FontFamilyStack::FluentUI,
                color: None,
                variations: style.variations,
            });
        }
        let result = self.with_font_context(|ctx| {
            build_shaped_layout(ctx, &fallback_spans, full_text, cfg, scale)
        });
        self.layout_cache.insert(cache_key, result.clone());
        result
    }
}
