//! Bounded LRU cache, quantized cache keys, and `.notdef` warning deduplication for `krui-text`.

use crate::MIN_SHAPING_FONT_SIZE_PX;
use krui_style::{
    FontFamilyStack, FontVariationList, LineHeight, OverflowWrapMode, TextAlign, TextBoxEdge,
    TextBoxTrim, TextDirection, TextStyle, WhiteSpaceMode, WordBreakMode,
};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::{Mutex, OnceLock};

/// Maximum number of shaped text layouts retained in each [`TextEngine`](crate::TextEngine) LRU cache.
pub const MAX_TEXT_CACHE_ENTRIES: usize = 512;

/// Minimum CSS Fonts 4 numeric font weight (`1`).
pub const MIN_FONT_WEIGHT: u16 = 1;
/// Maximum CSS Fonts 4 numeric font weight (`1000`).
pub const MAX_FONT_WEIGHT: u16 = 1000;

const FONT_SIZE_QUANT_SCALE: f32 = 100.0;
const LINE_HEIGHT_QUANT_SCALE: f32 = 1000.0;
const MAX_WIDTH_QUANT_SCALE: f32 = 4.0;
const SCALE_FACTOR_QUANT_SCALE: f32 = 100.0;

static NOTDEF_WARNED_CODEPOINTS: OnceLock<Mutex<HashSet<(FontFamilyStack, char)>>> =
    OnceLock::new();

/// Records a missing glyph (`.notdef`) for `(font_family, ch)` and emits a diagnostic warning
/// to `stderr` at most **once** per unique `(font_family, ch)` pair across the process lifetime.
pub fn record_notdef_glyph_warning(font_family: FontFamilyStack, ch: char) -> bool {
    let set_mutex = NOTDEF_WARNED_CODEPOINTS.get_or_init(|| Mutex::new(HashSet::new()));
    let mut guard = set_mutex.lock().unwrap_or_else(|e| e.into_inner());
    if guard.insert((font_family, ch)) {
        eprintln!(
            "[krui-text] missing glyph (.notdef) for U+{:04X} ({:?}) in font stack {:?}",
            ch as u32, ch, font_family
        );
        true
    } else {
        false
    }
}

/// Returns the total number of unique `(FontFamilyStack, char)` pairs that have triggered a `.notdef` warning.
pub fn notdef_warning_count() -> usize {
    let set_mutex = NOTDEF_WARNED_CODEPOINTS.get_or_init(|| Mutex::new(HashSet::new()));
    set_mutex.lock().unwrap_or_else(|e| e.into_inner()).len()
}

pub(crate) const fn normalize_font_weight(weight: u16) -> u16 {
    if weight < MIN_FONT_WEIGHT {
        MIN_FONT_WEIGHT
    } else if weight > MAX_FONT_WEIGHT {
        MAX_FONT_WEIGHT
    } else {
        weight
    }
}

pub(crate) fn quantize_font_size(font_size: f32) -> u32 {
    (font_size.max(MIN_SHAPING_FONT_SIZE_PX) * FONT_SIZE_QUANT_SCALE).round() as u32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum QuantizedLineHeight {
    Normal,
    FactorMilli(u32),
    PxCenti(u32),
}

pub(crate) fn quantize_style_line_height(lh: LineHeight, font_size: f32) -> QuantizedLineHeight {
    match lh {
        LineHeight::Normal => QuantizedLineHeight::Normal,
        LineHeight::Factor(f) => {
            QuantizedLineHeight::FactorMilli((f.max(0.0) * LINE_HEIGHT_QUANT_SCALE).round() as u32)
        }
        LineHeight::Px(px) => {
            QuantizedLineHeight::PxCenti((px.max(0.0) * FONT_SIZE_QUANT_SCALE).round() as u32)
        }
        LineHeight::FontRelative {
            em_factor,
            px_offset,
        } => {
            let px = (font_size.max(0.0) * em_factor + px_offset).max(0.0);
            QuantizedLineHeight::PxCenti((px * FONT_SIZE_QUANT_SCALE).round() as u32)
        }
    }
}

pub(crate) fn quantize_variations(vars: FontVariationList) -> Vec<([u8; 4], i32)> {
    vars.as_slice()
        .iter()
        .map(|v| (v.tag, (v.value * 100.0).round() as i32))
        .collect()
}

pub(crate) fn quantize_max_width(max_width: Option<f32>) -> Option<u32> {
    max_width.map(|w| (w.max(1.0) * MAX_WIDTH_QUANT_SCALE).round() as u32)
}

pub(crate) fn quantize_scale(scale_factor: f32) -> u32 {
    (scale_factor.clamp(1.0, 4.0) * SCALE_FACTOR_QUANT_SCALE).round() as u32
}

pub(crate) fn quantize_letter_spacing(px: f32) -> i32 {
    if px.is_finite() {
        (px.clamp(-512.0, 512.0) * FONT_SIZE_QUANT_SCALE).round() as i32
    } else {
        0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SpanStyleKey {
    pub(crate) text: String,
    pub(crate) font_weight: u16,
    pub(crate) italic: bool,
    pub(crate) font_family: FontFamilyStack,
    pub(crate) color: Option<[u8; 4]>,
    pub(crate) variations: Vec<([u8; 4], i32)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TextCacheKey {
    pub(crate) spans: Vec<SpanStyleKey>,
    pub(crate) font_size_q: u32,
    pub(crate) line_height_q: QuantizedLineHeight,
    pub(crate) letter_spacing_q: i32,
    pub(crate) text_box_trim: TextBoxTrim,
    pub(crate) text_box_edge: TextBoxEdge,
    pub(crate) white_space: WhiteSpaceMode,
    pub(crate) word_break: WordBreakMode,
    pub(crate) overflow_wrap: OverflowWrapMode,
    pub(crate) text_overflow_ellipsis: bool,
    pub(crate) line_clamp: Option<u16>,
    pub(crate) max_width_q: Option<u32>,
    pub(crate) text_align: TextAlign,
    pub(crate) direction: TextDirection,
    pub(crate) scale_pct: u32,
    pub(crate) font_face_epoch: u64,
}

impl TextCacheKey {
    pub(crate) fn from_text_style(
        sanitized_text: String,
        style: &TextStyle,
        max_width: Option<f32>,
        effective_align: TextAlign,
        scale_factor: f32,
        font_face_epoch: u64,
    ) -> Self {
        Self {
            spans: vec![SpanStyleKey {
                text: sanitized_text,
                font_weight: normalize_font_weight(style.font_weight),
                italic: style.italic,
                font_family: style.font_family,
                color: None,
                variations: quantize_variations(style.variations),
            }],
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
            text_align: effective_align,
            direction: style.direction,
            scale_pct: quantize_scale(scale_factor),
            font_face_epoch,
        }
    }
}

struct LruEntry<V> {
    value: V,
    last_used_tick: u64,
}

/// True LRU cache bounded to `MAX_TEXT_CACHE_ENTRIES` entries.
pub(crate) struct BoundedLruCache<K, V> {
    map: HashMap<K, LruEntry<V>>,
    tick: u64,
}

impl<K: Clone + Eq + Hash, V: Clone> BoundedLruCache<K, V> {
    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self {
            map: HashMap::with_capacity(cap),
            tick: 0,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }

    pub(crate) fn get_and_touch(&mut self, key: &K) -> Option<V> {
        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        let entry = self.map.get_mut(key)?;
        entry.last_used_tick = tick;
        Some(entry.value.clone())
    }

    pub(crate) fn peek(&self, key: &K) -> Option<&V> {
        self.map.get(key).map(|e| &e.value)
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        if let Some(existing) = self.map.get_mut(&key) {
            existing.value = value;
            existing.last_used_tick = tick;
            return;
        }
        while self.map.len() >= MAX_TEXT_CACHE_ENTRIES {
            let Some(oldest_key) = self
                .map
                .iter()
                .min_by_key(|(_, entry)| entry.last_used_tick)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.map.remove(&oldest_key);
        }
        self.map.insert(
            key,
            LruEntry {
                value,
                last_used_tick: tick,
            },
        );
    }
}
