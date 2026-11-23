//! # `krui-text`
//! Full CJK (Korean 한글), RTL, Color Emoji (`RGBA8`), OpenType `fvar` variable axes,
//! `@font-face` rules, W3C `line-height` + `text-box-trim`, and grapheme-aware IME geometry.

#![forbid(unsafe_code)]

mod cache;
/// Reusable grapheme-aware single-line and multiline editing buffer and IME visual geometry.
pub mod editor;
mod fallback;
mod font_metrics;
mod font_presets;
mod font_stacks;
mod font_var_raster;
mod grapheme_nav;
mod ime_geometry;
mod layout_types;
mod rasterize;
mod rich_paragraph;

use cache::{
    normalize_font_weight, quantize_font_size, quantize_max_width, quantize_scale,
    quantize_style_line_height, quantize_variations, BoundedLruCache, SpanStyleKey, TextCacheKey,
};
pub use cache::{
    notdef_warning_count, record_notdef_glyph_warning, MAX_FONT_WEIGHT, MAX_TEXT_CACHE_ENTRIES,
    MIN_FONT_WEIGHT,
};
pub use editor::{
    compute_ime_visuals, next_grapheme_char_idx, prev_grapheme_char_idx,
    snap_char_to_grapheme_boundary, ImeUnderlineSegment, ImeVisualGeometry, TextEditBuffer,
    DEFAULT_CANDIDATE_CARET_WIDTH_PX, MAX_EDIT_UNDO_HISTORY, MIN_IME_SEGMENT_WIDTH_PX,
};
use fallback::{measure_fallback_dimensions, populate_fallback_glyphs};
use font_metrics::compute_w3c_line_box;
pub use font_stacks::is_cjk_or_hangul;
use font_stacks::{FontContext, SanitizedRichSpan};
use krui_style::{
    sanitize_untrusted_text, Color, FontFaceRule, FontFamilyStack, FontVariationList, LineHeight,
    TextAlign, TextBoxEdge, TextBoxTrim, TextDirection, TextStyle,
};
pub use layout_types::{
    build_hollow_notdef_mask, RasterizedGlyph, RichTextSpan, ShapedTextLayout, TextLineMetrics,
};
use rasterize::{
    build_byte_to_char_table, measure_with_cosmic, rasterize_cosmic_runs, LineLayoutConfig,
};
pub use rasterize::{decode_swash_image, DecodedGlyphBitmap};
use std::sync::{Arc, Mutex, OnceLock};
use unicode_bidi::{BidiInfo, Level};
use unicode_segmentation::UnicodeSegmentation;

/// Minimum font size (logical px) used during text shaping and measurement.
pub const MIN_SHAPING_FONT_SIZE_PX: f32 = 8.0;
/// Fallback character advance ratio relative to `font_size` when no system fonts are installed.
pub const FALLBACK_LATIN_ADVANCE_RATIO: f32 = 0.6;
/// Wide CJK / Hangul advance multiplier relative to `FALLBACK_LATIN_ADVANCE_RATIO`.
pub const FALLBACK_CJK_ADVANCE_MULTIPLIER: f32 = 1.6;

static SHARED_FONT_CONTEXT: OnceLock<Mutex<FontContext>> = OnceLock::new();

pub(crate) fn with_shared_fonts<R>(f: impl FnOnce(&mut FontContext) -> R) -> R {
    let mutex = SHARED_FONT_CONTEXT.get_or_init(|| Mutex::new(FontContext::new()));
    f(&mut mutex.lock().unwrap_or_else(|e| e.into_inner()))
}

enum FontBackend {
    Shared,
    Isolated(Box<FontContext>),
}

/// BiDi-, CJK-, Color-Emoji-, `@font-face`-, and OpenType-`fvar`-aware text shaper and rasterizer.
pub struct TextEngine {
    backend: FontBackend,
    pub(crate) layout_cache: BoundedLruCache<TextCacheKey, ShapedTextLayout>,
    pub(crate) measure_cache: BoundedLruCache<TextCacheKey, (f32, f32)>,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    /// Initializes `TextEngine` backed by the process-wide shared `FontSystem` and `ScaleContext` pool.
    pub fn new() -> Self {
        with_shared_fonts(|_| {});
        Self {
            backend: FontBackend::Shared,
            layout_cache: BoundedLruCache::with_capacity(256),
            measure_cache: BoundedLruCache::with_capacity(256),
        }
    }

    /// Initializes `TextEngine` owning an isolated `FontSystem` and `ScaleContext` without global lock contention.
    pub fn new_isolated() -> Self {
        Self {
            backend: FontBackend::Isolated(Box::new(FontContext::new())),
            layout_cache: BoundedLruCache::with_capacity(256),
            measure_cache: BoundedLruCache::with_capacity(256),
        }
    }

    /// Returns `true` if this `TextEngine` owns an isolated `FontSystem` (`new_isolated()`).
    pub const fn is_isolated(&self) -> bool {
        matches!(self.backend, FontBackend::Isolated(_))
    }

    /// Returns the number of cached shaped text layouts in this `TextEngine`.
    pub fn cached_layout_count(&self) -> usize {
        self.layout_cache.len()
    }

    /// Returns the number of resolved font stacks currently cached in the backing `FontContext`.
    pub fn resolved_font_stack_count(&mut self) -> usize {
        self.with_font_context(|ctx| ctx.resolved_stacks.len())
    }

    /// Registers CSS `@font-face` rules idempotently into the backing font database.
    pub fn register_font_faces(&mut self, rules: &[FontFaceRule]) -> bool {
        self.with_font_context(|ctx| ctx.register_font_faces(rules))
    }

    pub(crate) fn with_font_context<R>(&mut self, f: impl FnOnce(&mut FontContext) -> R) -> R {
        match &mut self.backend {
            FontBackend::Shared => with_shared_fonts(f),
            FontBackend::Isolated(ctx) => f(ctx),
        }
    }

    pub(crate) fn current_epoch(&mut self) -> u64 {
        self.with_font_context(|ctx| ctx.font_face_epoch)
    }

    /// Computes the exact W3C strut line-box height for an empty control (`<input>` / `<button>`).
    pub fn measure_empty_line_height(&mut self, style: &TextStyle) -> f32 {
        let safe_fs = style.font_size.max(MIN_SHAPING_FONT_SIZE_PX);
        self.with_font_context(|ctx| {
            let (_, strut_meta) = ctx.resolve_strut(style.font_family, style.font_weight, false);
            compute_w3c_line_box(
                &strut_meta,
                safe_fs,
                style.line_height,
                style.text_box_trim,
                style.text_box_edge,
                true,
                true,
            )
            .line_height
            .ceil()
        })
    }

    /// Shapes `raw_text` without glyph rasterization using full [`TextStyle`] (`LineHeight`, `TextBoxTrim`, `fvar`).
    pub fn measure_styled_text(
        &mut self,
        raw_text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> (f32, f32) {
        let sanitized_text = sanitize_untrusted_text(raw_text);
        if sanitized_text.is_empty() {
            return (0.0, 0.0);
        }
        let eff_align = if max_width.is_some() {
            style.text_align
        } else {
            TextAlign::Left
        };
        let epoch = self.current_epoch();
        let cache_key = TextCacheKey::from_text_style(
            sanitized_text.clone(),
            style,
            max_width,
            eff_align,
            1.0,
            epoch,
        );
        if let Some(cached) = self.measure_cache.get_and_touch(&cache_key) {
            return cached;
        }
        if let Some(shaped) = self.layout_cache.peek(&cache_key) {
            return (shaped.measured_width.ceil(), shaped.measured_height.ceil());
        }
        let result = self.with_font_context(|ctx| {
            compute_uncached_measure(ctx, &sanitized_text, style, max_width, eff_align)
        });
        self.measure_cache.insert(cache_key, result);
        result
    }

    /// Shapes `raw_text` without glyph rasterization using a unitless `line_height_multiplier`.
    pub fn measure_text(
        &mut self,
        raw_text: &str,
        font_size: f32,
        line_height_multiplier: f32,
        font_weight: u16,
        font_family: FontFamilyStack,
        max_width: Option<f32>,
    ) -> (f32, f32) {
        let style = TextStyle {
            font_size,
            font_weight,
            line_height: LineHeight::Factor(line_height_multiplier),
            font_family,
            ..TextStyle::default()
        };
        self.measure_styled_text(raw_text, &style, max_width)
    }

    /// Shapes and rasterizes `raw_text` using full [`TextStyle`] (`LineHeight`, `TextBoxTrim`, `fvar`, `TextAlign`).
    pub fn shape_styled_text(
        &mut self,
        raw_text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
        scale_factor: f32,
    ) -> ShapedTextLayout {
        let scale = scale_factor.clamp(1.0, 4.0);
        let sanitized_text = sanitize_untrusted_text(raw_text);
        let eff_align = if max_width.is_some() {
            style.text_align
        } else {
            TextAlign::Left
        };
        let epoch = self.current_epoch();
        let cache_key = TextCacheKey::from_text_style(
            sanitized_text.clone(),
            style,
            max_width,
            eff_align,
            scale,
            epoch,
        );
        if let Some(cached) = self.layout_cache.get_and_touch(&cache_key) {
            return cached;
        }
        let spans = [SanitizedRichSpan {
            text: sanitized_text.clone(),
            font_weight: style.font_weight,
            italic: false,
            font_family: style.font_family,
            color: None,
            variations: style.variations,
        }];
        let cfg = LineLayoutConfig {
            font_size: style.font_size.max(MIN_SHAPING_FONT_SIZE_PX),
            line_height: style.line_height,
            text_box_trim: style.text_box_trim,
            text_box_edge: style.text_box_edge,
            max_width,
            align: eff_align,
            direction: style.direction,
        };
        let result = self
            .with_font_context(|ctx| build_shaped_layout(ctx, &spans, sanitized_text, cfg, scale));
        self.layout_cache.insert(cache_key, result.clone());
        result
    }

    /// Shapes, BiDi-analyzes, and rasterizes `raw_text` using the default `FluentUI` font stack.
    pub fn shape_and_rasterize(
        &mut self,
        raw_text: &str,
        font_size: f32,
        line_height_multiplier: f32,
        font_weight: u16,
        color: Color,
        max_width: Option<f32>,
    ) -> ShapedTextLayout {
        self.shape_and_rasterize_with_scale(
            raw_text,
            font_size,
            line_height_multiplier,
            font_weight,
            FontFamilyStack::FluentUI,
            color,
            max_width,
            1.0,
        )
    }

    /// Shapes, BiDi-analyzes, and rasterizes `raw_text` using the specified `FontFamilyStack`.
    #[allow(clippy::too_many_arguments)]
    pub fn shape_and_rasterize_with_family(
        &mut self,
        raw_text: &str,
        font_size: f32,
        line_height_multiplier: f32,
        font_weight: u16,
        font_family: FontFamilyStack,
        color: Color,
        max_width: Option<f32>,
    ) -> ShapedTextLayout {
        self.shape_and_rasterize_with_scale(
            raw_text,
            font_size,
            line_height_multiplier,
            font_weight,
            font_family,
            color,
            max_width,
            1.0,
        )
    }

    /// Shapes, BiDi-analyzes, and rasterizes `raw_text` at the given HiDPI `scale_factor` (`1.0..=4.0`).
    #[allow(clippy::too_many_arguments)]
    pub fn shape_and_rasterize_with_scale(
        &mut self,
        raw_text: &str,
        font_size: f32,
        line_height_multiplier: f32,
        font_weight: u16,
        font_family: FontFamilyStack,
        _color: Color,
        max_width: Option<f32>,
        scale_factor: f32,
    ) -> ShapedTextLayout {
        self.shape_text(
            raw_text,
            font_size,
            line_height_multiplier,
            font_weight,
            font_family,
            max_width,
            TextAlign::Left,
            TextDirection::Auto,
            scale_factor,
        )
    }

    /// Shapes and rasterizes a single-span paragraph with explicit alignment, direction, and HiDPI `scale_factor`.
    #[allow(clippy::too_many_arguments)]
    pub fn shape_text(
        &mut self,
        raw_text: &str,
        font_size: f32,
        line_height: f32,
        font_weight: u16,
        font_family: FontFamilyStack,
        max_width: Option<f32>,
        align: TextAlign,
        direction: TextDirection,
        scale_factor: f32,
    ) -> ShapedTextLayout {
        let style = TextStyle {
            font_size,
            font_weight,
            line_height: LineHeight::Factor(line_height),
            font_family,
            text_align: align,
            direction,
            ..TextStyle::default()
        };
        self.shape_styled_text(raw_text, &style, max_width, scale_factor)
    }

    /// Shapes and rasterizes a multi-span inline paragraph using `cosmic-text` `AttrsList`.
    #[allow(clippy::too_many_arguments)]
    pub fn shape_rich_text(
        &mut self,
        spans: &[RichTextSpan],
        base_font_size: f32,
        line_height: f32,
        max_width: Option<f32>,
        align: TextAlign,
        direction: TextDirection,
        scale_factor: f32,
    ) -> ShapedTextLayout {
        let scale = scale_factor.clamp(1.0, 4.0);
        let epoch = self.current_epoch();
        let (clean_spans, span_keys, full_text) = sanitize_rich_spans(spans);
        let lh = LineHeight::Factor(line_height);
        let cache_key = TextCacheKey {
            spans: span_keys,
            font_size_q: quantize_font_size(base_font_size),
            line_height_q: quantize_style_line_height(lh, base_font_size),
            text_box_trim: TextBoxTrim::None,
            text_box_edge: TextBoxEdge::AUTO,
            max_width_q: quantize_max_width(max_width),
            text_align: align,
            direction,
            scale_pct: quantize_scale(scale),
            font_face_epoch: epoch,
        };
        if let Some(cached) = self.layout_cache.get_and_touch(&cache_key) {
            return cached;
        }
        let cfg = LineLayoutConfig {
            font_size: base_font_size.max(MIN_SHAPING_FONT_SIZE_PX),
            line_height: lh,
            text_box_trim: TextBoxTrim::None,
            text_box_edge: TextBoxEdge::AUTO,
            max_width,
            align,
            direction,
        };
        let result = self
            .with_font_context(|ctx| build_shaped_layout(ctx, &clean_spans, full_text, cfg, scale));
        self.layout_cache.insert(cache_key, result.clone());
        result
    }
}

fn sanitize_rich_spans(
    spans: &[RichTextSpan],
) -> (Vec<SanitizedRichSpan>, Vec<SpanStyleKey>, String) {
    let mut clean_spans = Vec::with_capacity(spans.len());
    let mut span_keys = Vec::with_capacity(spans.len());
    let mut full_text = String::new();
    for sp in spans {
        let clean = sanitize_untrusted_text(&sp.text);
        full_text.push_str(&clean);
        span_keys.push(SpanStyleKey {
            text: clean.clone(),
            font_weight: normalize_font_weight(sp.font_weight),
            italic: sp.italic,
            font_family: sp.font_family,
            color: sp.color.map(|c| [c.r, c.g, c.b, c.a]),
            variations: quantize_variations(FontVariationList::EMPTY),
        });
        clean_spans.push(SanitizedRichSpan {
            text: clean,
            font_weight: sp.font_weight,
            italic: sp.italic,
            font_family: sp.font_family,
            color: sp.color,
            variations: FontVariationList::EMPTY,
        });
    }
    (clean_spans, span_keys, full_text)
}

fn compute_uncached_measure(
    ctx: &mut FontContext,
    sanitized_text: &str,
    style: &TextStyle,
    max_width: Option<f32>,
    eff_align: TextAlign,
) -> (f32, f32) {
    let safe_font_size = style.font_size.max(MIN_SHAPING_FONT_SIZE_PX);
    let single = [SanitizedRichSpan {
        text: sanitized_text.to_owned(),
        font_weight: style.font_weight,
        italic: false,
        font_family: style.font_family,
        color: None,
        variations: style.variations,
    }];
    let cfg = LineLayoutConfig {
        font_size: safe_font_size,
        line_height: style.line_height,
        text_box_trim: style.text_box_trim,
        text_box_edge: style.text_box_edge,
        max_width,
        align: eff_align,
        direction: style.direction,
    };
    let (mut mw, mut mh) = measure_with_cosmic(ctx, &single, sanitized_text, cfg);
    if mw == 0.0 && !sanitized_text.trim().is_empty() {
        let fallback_lh = style
            .line_height
            .resolve_px(safe_font_size, safe_font_size * 1.2);
        (mw, mh) =
            measure_fallback_dimensions(sanitized_text, safe_font_size, fallback_lh, max_width);
    }
    (mw.ceil(), mh.ceil())
}

fn detect_has_rtl(sanitized_text: &str, direction: TextDirection) -> bool {
    let default_level = match direction {
        TextDirection::Auto => None,
        TextDirection::Ltr => Some(Level::ltr()),
        TextDirection::Rtl => Some(Level::rtl()),
    };
    direction == TextDirection::Rtl || BidiInfo::new(sanitized_text, default_level).has_rtl()
}

pub(crate) fn build_shaped_layout(
    ctx: &mut FontContext,
    spans: &[SanitizedRichSpan],
    sanitized_text: String,
    cfg: LineLayoutConfig,
    scale: f32,
) -> ShapedTextLayout {
    let grapheme_count = sanitized_text.graphemes(true).count();
    let has_rtl = detect_has_rtl(&sanitized_text, cfg.direction);
    let has_cjk_or_hangul = sanitized_text.chars().any(is_cjk_or_hangul);
    let (byte_to_char, char_count) = build_byte_to_char_table(&sanitized_text);
    let mut runs_out = rasterize_cosmic_runs(
        ctx,
        spans,
        &sanitized_text,
        &byte_to_char,
        char_count,
        cfg,
        scale,
    );
    let base_fam = spans
        .first()
        .map_or(FontFamilyStack::FluentUI, |s| s.font_family);
    if runs_out.glyphs.is_empty() && !sanitized_text.trim().is_empty() {
        let fallback_lh = cfg
            .line_height
            .resolve_px(cfg.font_size, cfg.font_size * 1.2);
        populate_fallback_glyphs(
            &sanitized_text,
            cfg.font_size,
            fallback_lh,
            scale,
            base_fam,
            has_rtl,
            cfg.max_width,
            &mut runs_out,
        );
    }
    ShapedTextLayout {
        sanitized_text,
        grapheme_count,
        has_rtl,
        has_cjk_or_hangul,
        measured_width: runs_out.measured_width,
        measured_height: runs_out.measured_height,
        char_x_offsets: Arc::from(runs_out.char_x_offsets),
        char_line_indices: Arc::from(runs_out.char_line_indices),
        lines: Arc::from(runs_out.lines),
        glyphs: Arc::from(runs_out.glyphs),
    }
}
