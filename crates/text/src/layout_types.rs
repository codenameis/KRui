//! Core shaped paragraph, line metric, inline span, and rasterized glyph data structures.

use crate::ime_geometry::line_range_segments;
use krui_style::{Color, FontFamilyStack};
use std::sync::Arc;

/// Builds a hollow 1px rectangle mask (`w x h`) for `.notdef` (`glyph_id == 0`) or fallback glyphs.
pub fn build_hollow_notdef_mask(w: u32, h: u32) -> Arc<[u8]> {
    let (w, h) = (w.clamp(3, 4096), h.clamp(4, 4096));
    let mut mask = vec![0_u8; (w as usize) * (h as usize)];
    for y in 0..h {
        for x in 0..w {
            if x == 0 || x + 1 == w || y == 0 || y + 1 == h {
                mask[(y as usize) * (w as usize) + (x as usize)] = 220;
            }
        }
    }
    Arc::from(mask)
}

/// Inline styled text span within a rich multi-span paragraph ([`crate::TextEngine::shape_rich_text`]).
#[derive(Clone, Debug, PartialEq)]
pub struct RichTextSpan {
    /// Raw or pre-sanitized span text.
    pub text: String,
    /// Numeric CSS font weight (`100..=900`).
    pub font_weight: u16,
    /// Italic font posture (`font-style: italic`).
    pub italic: bool,
    /// Font family stack for this span (e.g. `FluentUI` for prose, `CosmicMono` for `<code>`).
    pub font_family: FontFamilyStack,
    /// Optional per-span foreground color override.
    pub color: Option<Color>,
}

/// A single shaped and rasterized glyph ready for GPU atlas upload or CPU `tiny-skia` blitting.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterizedGlyph {
    /// Horizontal offset in logical pixels relative to paragraph origin.
    pub x: f32,
    /// Vertical offset in logical pixels relative to paragraph origin.
    pub y: f32,
    /// Rasterized bitmap width in physical pixels.
    pub width: u32,
    /// Rasterized bitmap height in physical pixels.
    pub height: u32,
    /// Stable 64-bit glyph atlas cache key derived from `(font_id, glyph_id, font_size_bits, subpixel_bin, flags)`.
    pub cache_key: u64,
    /// True when `glyph_id == 0` (`.notdef` missing glyph box).
    pub is_notdef: bool,
    /// True when this glyph is a full-color glyph (`SwashContent::Color`) with `color_rgba` populated.
    pub is_color: bool,
    /// Source [`RichTextSpan`] index (`0` for single-span paragraphs).
    pub span_index: u16,
    /// Inclusive starting `char` index covered by this glyph cluster.
    pub char_start: u32,
    /// Exclusive ending `char` index covered by this glyph cluster.
    pub char_end: u32,
    /// Optional per-span foreground color override from [`RichTextSpan::color`].
    pub glyph_color: Option<Color>,
    /// Shared immutable `R8` alpha mask (`width * height` bytes) — always populated.
    pub alpha_mask: Arc<[u8]>,
    /// Shared immutable straight-alpha `RGBA8` bitmap (`width * height * 4` bytes) for color emojis.
    pub color_rgba: Option<Arc<[u8]>>,
}

/// Per-line layout metrics produced during multi-line text shaping.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLineMetrics {
    /// Zero-based visual line index.
    pub line_index: usize,
    /// Top Y offset in logical pixels relative to paragraph origin.
    pub line_top: f32,
    /// Line box height in logical pixels.
    pub line_height: f32,
    /// Alphabetic baseline Y offset in logical pixels relative to paragraph origin.
    pub baseline_y: f32,
    /// Font/glyph ascent above `baseline_y` in logical pixels.
    pub ascent: f32,
    /// Font/glyph descent below `baseline_y` in logical pixels (non-negative downward).
    pub descent: f32,
    /// Shaped horizontal advance width of this line in logical pixels.
    pub line_width: f32,
    /// Inclusive starting `char` index on this line.
    pub start_char: usize,
    /// Exclusive ending `char` index on this line.
    pub end_char: usize,
}

/// Result of shaping and measuring a text paragraph, including per-character X offsets and
/// per-line metrics for sub-millisecond cursor positioning and 2D mouse hit-testing.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedTextLayout {
    /// Concatenated sanitized text of the paragraph.
    pub sanitized_text: String,
    /// Total extended grapheme cluster count.
    pub grapheme_count: usize,
    /// `true` if the paragraph contains right-to-left runs or uses an explicit RTL base direction.
    pub has_rtl: bool,
    /// `true` if the paragraph contains Korean Hangul or CJK Unified Ideographs.
    pub has_cjk_or_hangul: bool,
    /// Intrinsic paragraph width in logical pixels.
    pub measured_width: f32,
    /// Intrinsic paragraph height in logical pixels.
    pub measured_height: f32,
    /// X pixel offset before each `char` index (`0..=char_count`) on its respective line.
    pub char_x_offsets: Arc<[f32]>,
    /// Line index (`0..lines.len()`) for each `char` boundary (`0..=char_count`).
    pub char_line_indices: Arc<[usize]>,
    /// Per-line vertical and character-range metrics.
    pub lines: Arc<[TextLineMetrics]>,
    /// Shaped and rasterized glyph list.
    pub glyphs: Arc<[RasterizedGlyph]>,
}

impl ShapedTextLayout {
    /// Returns the X pixel offset for a cursor placed at `char_index`.
    pub fn cursor_x_for_char(&self, char_index: usize) -> f32 {
        if self.char_x_offsets.is_empty() {
            return 0.0;
        }
        self.char_x_offsets[char_index.min(self.char_x_offsets.len() - 1)]
    }

    /// Returns the [`TextLineMetrics`] containing `char_index`.
    pub fn line_for_char(&self, char_index: usize) -> TextLineMetrics {
        let line_idx = self
            .char_line_indices
            .get(char_index.min(self.char_line_indices.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0);
        self.lines
            .get(line_idx)
            .cloned()
            .or_else(|| self.lines.last().cloned())
            .unwrap_or(TextLineMetrics {
                line_index: 0,
                line_top: 0.0,
                line_height: self.measured_height,
                baseline_y: self.measured_height * 0.80,
                ascent: self.measured_height * 0.80,
                descent: self.measured_height * 0.20,
                line_width: self.measured_width,
                start_char: 0,
                end_char: self.char_x_offsets.len().saturating_sub(1),
            })
    }

    /// Finds the closest character index (`0..=char_count`) for a local `(x, y)` click coordinate.
    pub fn hit_test(&self, local_x: f32, local_y: f32) -> usize {
        if self.char_x_offsets.is_empty() {
            return 0;
        }
        if self.lines.len() <= 1 {
            return self.hit_test_char_index(local_x);
        }
        let Some(line) = self
            .lines
            .iter()
            .find(|l| local_y < l.line_top + l.line_height)
            .or_else(|| self.lines.last())
        else {
            return self.hit_test_char_index(local_x);
        };
        let max_idx = self.char_x_offsets.len() - 1;
        let start = line.start_char.min(max_idx);
        let end = line.end_char.min(max_idx).max(start);
        let x = local_x.max(0.0);
        (start..=end)
            .filter(|&idx| {
                idx == start || self.char_line_indices.get(idx).copied() == Some(line.line_index)
            })
            .min_by(|&a, &b| {
                (x - self.char_x_offsets[a])
                    .abs()
                    .total_cmp(&(x - self.char_x_offsets[b]).abs())
            })
            .unwrap_or(start)
    }

    /// Finds the closest character index (`0..=char_count`) for a single-line local X coordinate.
    pub fn hit_test_char_index(&self, local_x: f32) -> usize {
        if self.char_x_offsets.is_empty() {
            return 0;
        }
        let x = local_x.max(0.0);
        self.char_x_offsets
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (x - **a).abs().total_cmp(&(x - **b).abs()))
            .map_or(0, |(idx, _)| idx)
    }

    /// Computes per-line selection highlight boxes `(x, y, width, height)` in local text coordinates
    /// for the character range `start_char..end_char`.
    pub fn selection_rects_for_range(
        &self,
        start_char: usize,
        end_char: usize,
    ) -> Vec<(f32, f32, f32, f32)> {
        line_range_segments(self, start_char, end_char)
            .into_iter()
            .map(|(_, x, y, w, h)| (x, y, w, h))
            .collect()
    }

    /// Returns a clone of `self` (retained for API compatibility; base text color is external).
    pub fn with_color(&self, _color: Color) -> Self {
        self.clone()
    }
}
