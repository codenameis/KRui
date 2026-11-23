//! Multi-line selection and IME preedit/clause underline geometry over [`ShapedTextLayout`].

use crate::{ShapedTextLayout, TextLineMetrics};
use std::ops::Range;

/// Minimum horizontal width (logical px) for a visible selection or IME underline segment.
pub const MIN_IME_SEGMENT_WIDTH_PX: f32 = 2.0;

/// Default caret width (logical px) for collapsed IME candidate window anchor rects.
pub const DEFAULT_CANDIDATE_CARET_WIDTH_PX: f32 = 2.0;

/// A single per-line IME composition underline segment in local paragraph coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImeUnderlineSegment {
    /// Zero-based visual line index inside [`ShapedTextLayout::lines`].
    pub line_index: usize,
    /// Left X offset in logical pixels relative to paragraph origin.
    pub x: f32,
    /// Top Y offset of the line in logical pixels relative to paragraph origin.
    pub y: f32,
    /// Horizontal span width in logical pixels (`>= 2.0`).
    pub width: f32,
    /// Line height in logical pixels (used to position underlines at the bottom of the line box).
    pub height_hint: f32,
    /// `true` when this segment belongs to the active converted clause (`target_clause_char_range`).
    pub is_target_clause: bool,
}

/// Multi-line IME preedit underline segments and candidate window anchor rectangle.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ImeVisualGeometry {
    /// Per-line underline segments split accurately across soft-wrapped and `\n`-broken lines.
    pub underline_segments: Vec<ImeUnderlineSegment>,
    /// Candidate window anchor rect `(x, y, width, height)` in local text coordinates on the active line.
    pub candidate_cursor_rect: (f32, f32, f32, f32),
}

pub(crate) fn line_range_segments(
    layout: &ShapedTextLayout,
    start_char: usize,
    end_char: usize,
) -> Vec<(usize, f32, f32, f32, f32)> {
    let (s, e) = (start_char.min(end_char), start_char.max(end_char));
    if s == e || layout.char_x_offsets.is_empty() {
        return Vec::new();
    }
    let max_idx = layout.char_x_offsets.len() - 1;
    let (s, e) = (s.min(max_idx), e.min(max_idx));
    if s == e {
        return Vec::new();
    }
    if layout.lines.is_empty() {
        let (mut x_min, mut x_max) = (layout.char_x_offsets[s], layout.char_x_offsets[s]);
        for &x in &layout.char_x_offsets[s..=e] {
            x_min = x_min.min(x);
            x_max = x_max.max(x);
        }
        return vec![(
            0,
            x_min,
            0.0,
            (x_max - x_min).max(MIN_IME_SEGMENT_WIDTH_PX),
            layout.measured_height,
        )];
    }

    let mut out = Vec::new();
    for line in layout.lines.iter() {
        if let Some(seg) = single_line_range_segment(layout, line, s, e, max_idx) {
            out.push(seg);
        }
    }
    out
}

fn single_line_range_segment(
    layout: &ShapedTextLayout,
    line: &TextLineMetrics,
    s: usize,
    e: usize,
    max_idx: usize,
) -> Option<(usize, f32, f32, f32, f32)> {
    let ls = s.max(line.start_char).min(max_idx);
    let le = e.min(line.end_char).min(max_idx);
    if ls >= le {
        return None;
    }
    let line_start_idx = line.start_char.min(max_idx);
    let line_left_x =
        if layout.char_line_indices.get(line_start_idx).copied() == Some(line.line_index) {
            layout.char_x_offsets[line_start_idx]
        } else {
            0.0
        };

    let (mut x_min, mut x_max) = (f32::INFINITY, f32::NEG_INFINITY);
    for idx in ls..=le {
        if layout.char_line_indices.get(idx).copied() == Some(line.line_index) {
            let x = layout.char_x_offsets[idx];
            (x_min, x_max) = (x_min.min(x), x_max.max(x));
        } else if idx == le && idx == line.end_char {
            let right_edge = line_left_x + line.line_width;
            (x_min, x_max) = (x_min.min(right_edge), x_max.max(right_edge));
        } else if idx == ls {
            let x = layout.char_x_offsets[idx];
            (x_min, x_max) = (x_min.min(x), x_max.max(x));
        }
    }
    (x_min.is_finite() && x_max.is_finite()).then_some((
        line.line_index,
        x_min,
        line.line_top,
        (x_max - x_min).max(MIN_IME_SEGMENT_WIDTH_PX),
        line.line_height,
    ))
}

fn append_ime_segments(
    out: &mut Vec<ImeUnderlineSegment>,
    layout: &ShapedTextLayout,
    range: Range<usize>,
    is_target_clause: bool,
) {
    if range.start >= range.end {
        return;
    }
    for (line_index, x, y, width, height_hint) in
        line_range_segments(layout, range.start, range.end)
    {
        out.push(ImeUnderlineSegment {
            line_index,
            x,
            y,
            width,
            height_hint,
            is_target_clause,
        });
    }
}

fn resolve_candidate_cursor_rect(
    layout: &ShapedTextLayout,
    underline_segments: &[ImeUnderlineSegment],
    pre_start: usize,
) -> (f32, f32, f32, f32) {
    if let Some(seg) = underline_segments.iter().find(|seg| seg.is_target_clause) {
        (seg.x, seg.y, seg.width, seg.height_hint)
    } else if let Some(seg) = underline_segments.last() {
        (seg.x, seg.y, seg.width, seg.height_hint)
    } else {
        let line = layout.line_for_char(pre_start);
        (
            layout.cursor_x_for_char(pre_start),
            line.line_top,
            DEFAULT_CANDIDATE_CARET_WIDTH_PX,
            line.line_height,
        )
    }
}

/// Computes multi-line IME preedit underline segments and candidate window anchor geometry.
///
/// Splits `preedit_char_range` and optional `target_clause_char_range` cleanly across wrapped
/// and `\n`-delimited lines so `<textarea>` and multi-line editors never collapse IME underlines
/// onto line `0`.
pub fn compute_ime_visuals(
    layout: &ShapedTextLayout,
    preedit_char_range: Range<usize>,
    target_clause_char_range: Option<Range<usize>>,
) -> ImeVisualGeometry {
    let max_idx = layout.char_x_offsets.len().saturating_sub(1);
    let pre_start = preedit_char_range.start.min(max_idx);
    let pre_end = preedit_char_range.end.min(max_idx).max(pre_start);

    let clause = target_clause_char_range.and_then(|r| {
        let cs = r.start.clamp(pre_start, pre_end);
        let ce = r.end.clamp(cs, pre_end);
        (cs < ce).then_some(cs..ce)
    });

    let mut underline_segments = Vec::new();
    if pre_start < pre_end {
        if let Some(cl) = &clause {
            append_ime_segments(&mut underline_segments, layout, pre_start..cl.start, false);
            append_ime_segments(&mut underline_segments, layout, cl.start..cl.end, true);
            append_ime_segments(&mut underline_segments, layout, cl.end..pre_end, false);
        } else {
            append_ime_segments(&mut underline_segments, layout, pre_start..pre_end, false);
        }
    }

    let candidate_cursor_rect =
        resolve_candidate_cursor_rect(layout, &underline_segments, pre_start);
    ImeVisualGeometry {
        underline_segments,
        candidate_cursor_rect,
    }
}
