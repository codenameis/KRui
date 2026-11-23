//! Extended grapheme cluster, word-boundary, line-boundary, and vertical column navigation helpers.

use crate::{ShapedTextLayout, TextLineMetrics};
use krui_style::sanitize_untrusted_text;
use unicode_segmentation::UnicodeSegmentation;

pub(crate) fn normalize_editor_text(raw: &str, multiline: bool) -> String {
    let mut clean_crlf = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                let _ = chars.next();
            }
            clean_crlf.push(if multiline { '\n' } else { ' ' });
        } else if ch == '\n' {
            clean_crlf.push(if multiline { '\n' } else { ' ' });
        } else if ch == '\t' {
            clean_crlf.push(' ');
        } else {
            clean_crlf.push(ch);
        }
    }
    sanitize_untrusted_text(&clean_crlf)
}

pub(crate) fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map_or(s.len(), |(b, _)| b)
}

/// Returns the character index of the grapheme cluster boundary immediately before `cur_char_idx`.
pub fn prev_grapheme_char_idx(s: &str, cur_char_idx: usize) -> usize {
    if cur_char_idx == 0 {
        return 0;
    }
    let (mut acc, mut prev) = (0, 0);
    for g in s.graphemes(true) {
        let g_chars = g.chars().count();
        if acc + g_chars >= cur_char_idx {
            return acc;
        }
        prev = acc;
        acc += g_chars;
    }
    prev
}

/// Returns the character index of the grapheme cluster boundary immediately after `cur_char_idx`.
pub fn next_grapheme_char_idx(s: &str, cur_char_idx: usize) -> usize {
    let mut acc = 0;
    for g in s.graphemes(true) {
        let g_chars = g.chars().count();
        if acc + g_chars > cur_char_idx {
            return acc + g_chars;
        }
        acc += g_chars;
    }
    acc
}

/// Snaps an arbitrary character index to the nearest extended grapheme cluster boundary.
pub fn snap_char_to_grapheme_boundary(s: &str, target_char_idx: usize) -> usize {
    let mut acc = 0;
    for g in s.graphemes(true) {
        let g_chars = g.chars().count();
        if target_char_idx <= acc {
            return acc;
        }
        if target_char_idx < acc + g_chars {
            let dist_start = target_char_idx - acc;
            let dist_end = (acc + g_chars) - target_char_idx;
            return if dist_start <= dist_end {
                acc
            } else {
                acc + g_chars
            };
        }
        acc += g_chars;
    }
    acc
}

fn is_word_segment(seg: &str) -> bool {
    seg.chars().any(|c| c.is_alphanumeric() || c == '_')
}

pub(crate) fn prev_word_char_idx(s: &str, cur_char_idx: usize) -> usize {
    if cur_char_idx == 0 {
        return 0;
    }
    let mut acc = 0;
    let mut last_word = None;
    let mut last_non_ws = None;
    for seg in s.split_word_bounds() {
        let seg_chars = seg.chars().count();
        if acc >= cur_char_idx {
            break;
        }
        if is_word_segment(seg) {
            last_word = Some(acc);
        } else if seg.chars().any(|c| !c.is_whitespace()) {
            last_non_ws = Some(acc);
        }
        acc += seg_chars;
    }
    snap_char_to_grapheme_boundary(s, last_word.or(last_non_ws).unwrap_or(0))
}

pub(crate) fn next_word_char_idx(s: &str, cur_char_idx: usize) -> usize {
    let total = s.chars().count();
    if cur_char_idx >= total {
        return total;
    }
    let mut acc = 0;
    let mut fallback = None;
    for seg in s.split_word_bounds() {
        let seg_end = acc + seg.chars().count();
        if seg_end > cur_char_idx {
            if is_word_segment(seg) {
                return snap_char_to_grapheme_boundary(s, seg_end);
            }
            if fallback.is_none() && seg.chars().any(|c| !c.is_whitespace()) {
                fallback = Some(seg_end);
            }
        }
        acc = seg_end;
    }
    snap_char_to_grapheme_boundary(s, fallback.unwrap_or(total))
}

pub(crate) fn line_start_char_idx(s: &str, cur_char_idx: usize) -> usize {
    let mut last_start = 0;
    for (idx, ch) in s.chars().enumerate() {
        if idx >= cur_char_idx {
            break;
        }
        if ch == '\n' {
            last_start = idx + 1;
        }
    }
    last_start
}

pub(crate) fn line_end_char_idx(s: &str, cur_char_idx: usize) -> usize {
    for (idx, ch) in s.chars().enumerate().skip(cur_char_idx) {
        if ch == '\n' {
            return idx;
        }
    }
    s.chars().count()
}

fn collect_grapheme_boundaries(s: &str) -> Vec<usize> {
    let mut bounds = vec![0];
    let mut acc = 0;
    for g in s.graphemes(true) {
        acc += g.chars().count();
        bounds.push(acc);
    }
    bounds
}

pub(crate) fn closest_grapheme_on_line(
    text: &str,
    layout: &ShapedTextLayout,
    line: &TextLineMetrics,
    desired_x: f32,
) -> usize {
    let max_idx = layout.char_x_offsets.len().saturating_sub(1);
    let start = line.start_char.min(max_idx);
    let end = line.end_char.min(max_idx).max(start);
    let boundaries = collect_grapheme_boundaries(text);

    let best = boundaries
        .into_iter()
        .filter(|&idx| {
            idx >= start
                && idx <= end
                && (idx == start
                    || layout.char_line_indices.get(idx).copied() == Some(line.line_index))
        })
        .min_by(|&a, &b| {
            (desired_x - layout.cursor_x_for_char(a))
                .abs()
                .total_cmp(&(desired_x - layout.cursor_x_for_char(b)).abs())
        })
        .unwrap_or(start);

    snap_char_to_grapheme_boundary(text, best)
}
