//! Grapheme-cluster navigation, word boundary detection, single-line/multiline text normalization,
//! horizontal/vertical input scroll clamping, and undo/redo snapshot helpers.

use crate::view_tree::key_routing::CARET_RIGHT_MARGIN;
use crate::view_tree::sparse_tables::NodeEditBorrow;
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_style::{sanitize_untrusted_text, CompiledStyle};
use unicode_segmentation::UnicodeSegmentation;

/// Maximum number of undo/redo snapshots retained per `<input>` / `<textarea>` node.
pub const MAX_TEXT_UNDO_HISTORY: usize = 64;

/// Snapshot of a node's text buffer, caret position, and selection anchor for Undo/Redo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEditSnapshot {
    /// Text buffer content captured before an edit.
    pub text: String,
    /// Caret character index captured before an edit.
    pub cursor_char_idx: usize,
    /// Optional selection anchor character index captured before an edit.
    pub selection_anchor_char_idx: Option<usize>,
}

/// Records the current text/caret/selection state onto `undo_stack` and clears `redo_stack`.
pub(crate) fn push_undo_snapshot(ctx: &mut NodeEditBorrow<'_>) {
    let snap = TextEditSnapshot {
        text: ctx.node.text.clone(),
        cursor_char_idx: ctx.edit.cursor_char_idx,
        selection_anchor_char_idx: ctx.edit.selection_anchor_char_idx,
    };
    if ctx
        .edit
        .undo_stack
        .last()
        .is_some_and(|last| last.text == snap.text)
    {
        return;
    }
    if ctx.edit.undo_stack.len() >= MAX_TEXT_UNDO_HISTORY {
        ctx.edit.undo_stack.remove(0);
    }
    ctx.edit.undo_stack.push(snap);
    ctx.edit.redo_stack.clear();
}

/// Normalizes text for a single-line `<input>`, stripping BiDi overrides and replacing
/// `\r\n`, `\r`, `\n`, and `\t` with `' '` while removing other control characters.
pub(crate) fn sanitize_single_line_text(raw: &str) -> String {
    let clean = sanitize_untrusted_text(raw);
    let mut out = String::with_capacity(clean.len());
    let mut chars = clean.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                let _ = chars.next();
            }
            out.push(' ');
        } else if ch == '\n' || ch == '\t' {
            out.push(' ');
        } else if !ch.is_control() {
            out.push(ch);
        }
    }
    out
}

/// Normalizes text for a multiline `<textarea>`, stripping BiDi overrides, normalizing
/// `\r\n` and `\r` to `\n`, replacing `\t` with `' '`, and removing other control characters.
pub(crate) fn sanitize_multiline_text(raw: &str) -> String {
    let clean = sanitize_untrusted_text(raw);
    let mut out = String::with_capacity(clean.len());
    let mut chars = clean.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                let _ = chars.next();
            }
            out.push('\n');
        } else if ch == '\n' {
            out.push('\n');
        } else if ch == '\t' {
            out.push(' ');
        } else if !ch.is_control() {
            out.push(ch);
        }
    }
    out
}

/// Maps a character index `idx` from `old_text` into `new_text` by matching unchanged prefix and
/// suffix character spans so external `bind:value` updates preserve caret and selection offsets.
pub(crate) fn map_char_idx_across_text_update(old_text: &str, new_text: &str, idx: usize) -> usize {
    let old_chars: Vec<char> = old_text.chars().collect();
    let new_chars: Vec<char> = new_text.chars().collect();
    let (old_len, new_len) = (old_chars.len(), new_chars.len());
    let clamped = idx.min(old_len);
    let mut prefix = 0;
    while prefix < old_len && prefix < new_len && old_chars[prefix] == new_chars[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < (old_len - prefix)
        && suffix < (new_len - prefix)
        && old_chars[old_len - 1 - suffix] == new_chars[new_len - 1 - suffix]
    {
        suffix += 1;
    }
    let mapped = if clamped <= prefix && (prefix > 0 || clamped < old_len - suffix) {
        clamped
    } else if clamped >= old_len - suffix {
        new_len.saturating_sub(old_len - clamped)
    } else {
        new_len.saturating_sub(suffix).min(new_len)
    };
    snap_char_to_grapheme_boundary(new_text, mapped.min(new_len))
}

fn normalize_bound_text(
    role: SemanticRole,
    range_bounds: (f32, f32, f32),
    raw_text: &str,
) -> String {
    if role.is_single_line_input() {
        sanitize_single_line_text(raw_text)
    } else if role == SemanticRole::MultilineTextInput {
        sanitize_multiline_text(raw_text)
    } else if role == SemanticRole::Slider {
        crate::view_tree::controls::format_range_value(
            crate::view_tree::controls::parse_and_clamp_range_value(raw_text, range_bounds),
        )
    } else {
        sanitize_untrusted_text(raw_text)
    }
}

/// Applies a reactive `bind_dynamic_text` / `bind:value` update to `node_id`, normalizing text and
/// preserving caret, selection, and IME composition state without allocating sparse entries on plain nodes.
pub(crate) fn apply_bound_text_update(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    raw_text: &str,
    is_focused: bool,
) -> bool {
    let Some(role) = inner.node(node_id).map(|n| n.role) else {
        return false;
    };
    let bounds = inner
        .control(node_id)
        .map_or((0.0, 100.0, 1.0), |c| c.range_bounds);
    let new_text = normalize_bound_text(role, bounds, raw_text);
    sync_bound_control_meta(inner, node_id, role, &new_text, is_focused);
    if role == SemanticRole::Checkbox && (new_text == "true" || new_text == "false") {
        let next_checked = new_text == "true";
        let toggled = if let Some(node) = inner.node_mut(node_id) {
            if node.is_checked() != next_checked {
                node.set_checked(next_checked);
                node.text = new_text.clone();
                true
            } else {
                false
            }
        } else {
            false
        };
        if toggled {
            crate::view_tree::sync::notify_selector_state_change(inner, node_id);
            return true;
        }
    }
    if inner.node(node_id).is_some_and(|n| n.text == new_text) {
        return false;
    }
    if role.is_text_editable() || inner.sparse.text_edits.contains_key(&node_id) {
        apply_editable_bound_text(inner, node_id, new_text, is_focused);
    } else if let Some(node) = inner.node_mut(node_id) {
        node.text = new_text;
        node.set_text_dirty(true);
    }
    crate::view_tree::sync::notify_selector_state_change(inner, node_id);
    true
}

fn sync_bound_control_meta(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    role: SemanticRole,
    new_text: &str,
    is_focused: bool,
) {
    let is_form_ctrl = role.is_text_editable()
        || matches!(
            role,
            SemanticRole::Checkbox
                | SemanticRole::RadioButton
                | SemanticRole::Slider
                | SemanticRole::ComboBox
        );
    if !is_focused && (role.is_text_editable() || inner.sparse.text_edits.contains_key(&node_id)) {
        inner.ensure_text_edit(node_id).committed_value_on_focus = new_text.to_string();
    }
    if is_form_ctrl || inner.sparse.controls.contains_key(&node_id) {
        let ctrl = inner.ensure_control(node_id);
        if ctrl.initial_text.is_none() {
            ctrl.initial_text = Some(new_text.to_string());
        }
        if role == SemanticRole::ComboBox {
            ctrl.selected_option_idx = ctrl
                .select_options
                .iter()
                .position(|(v, _, _)| *v == new_text);
        }
    }
}

fn apply_editable_bound_text(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    new_text: String,
    is_focused: bool,
) {
    let Some(ctx) = inner.node_edit_mut(node_id) else {
        return;
    };
    let was_empty = ctx.node.text.is_empty();
    let preserve = is_focused
        || ctx.edit.selection_anchor_char_idx.is_some()
        || !ctx.edit.ime_state.preedit.is_empty();
    if preserve && !was_empty {
        let next_cur =
            map_char_idx_across_text_update(&ctx.node.text, &new_text, ctx.edit.cursor_char_idx);
        let next_anchor = ctx
            .edit
            .selection_anchor_char_idx
            .map(|a| map_char_idx_across_text_update(&ctx.node.text, &new_text, a))
            .filter(|&a| a != next_cur);
        ctx.node.text = new_text;
        ctx.edit.cursor_char_idx = next_cur;
        ctx.edit.selection_anchor_char_idx = next_anchor;
    } else {
        ctx.node.text = new_text;
        let char_len = ctx.node.text.chars().count();
        ctx.edit.cursor_char_idx = if was_empty {
            char_len
        } else {
            ctx.edit.cursor_char_idx.min(char_len)
        };
        ctx.edit.selection_anchor_char_idx = None;
    }
    if is_focused {
        ctx.edit.needs_caret_scroll = true;
        ctx.edit.preferred_x = None;
    }
    ctx.node.set_text_dirty(true);
}

/// Computes the visible inner horizontal width of a single-line `<input>` box.
pub(crate) fn input_visible_inner_width(world_rect: LayoutRect, style: &CompiledStyle) -> f32 {
    let cb = crate::view_tree::content_box::content_box_rect(world_rect, &style.layout);
    let right_extra = (CARET_RIGHT_MARGIN - style.layout.padding.right).max(0.0);
    (cb.width - right_extra).max(1.0)
}

/// Computes the visible inner vertical height of a multiline `<textarea>` box.
pub(crate) fn textarea_visible_inner_height(world_rect: LayoutRect, style: &CompiledStyle) -> f32 {
    crate::view_tree::content_box::content_box_rect(world_rect, &style.layout)
        .height
        .max(1.0)
}

/// Updates `scroll_x` by the minimal delta needed to keep `raw_cursor_x` visible inside `world_rect`.
pub(crate) fn update_input_scroll_x(
    scroll_x: f32,
    raw_cursor_x: f32,
    measured_width: f32,
    world_rect: LayoutRect,
    style: &CompiledStyle,
) -> f32 {
    if world_rect.width <= 0.0 {
        return 0.0;
    }
    let inner_w = input_visible_inner_width(world_rect, style);
    let max_scroll = (measured_width - inner_w).max(0.0);
    let mut next = scroll_x.clamp(0.0, max_scroll);
    if raw_cursor_x < next {
        next = raw_cursor_x;
    } else if raw_cursor_x > next + inner_w {
        next = raw_cursor_x - inner_w;
    }
    next.clamp(0.0, max_scroll)
}

/// Clamps `<textarea>` `scroll_y` and, when `needs_caret_scroll` is `true`, scrolls minimally so the
/// caret line `[line_top, line_top + line_height]` stays visible inside the inner content box.
pub(crate) fn update_textarea_scroll_y(
    scroll_y: f32,
    needs_caret_scroll: bool,
    line_top: f32,
    line_height: f32,
    measured_height: f32,
    world_rect: LayoutRect,
    style: &CompiledStyle,
) -> f32 {
    if world_rect.height <= 0.0 {
        return 0.0;
    }
    let inner_h = textarea_visible_inner_height(world_rect, style);
    let max_scroll = (measured_height.max(line_top + line_height) - inner_h).max(0.0);
    let mut next = scroll_y.clamp(0.0, max_scroll);
    if needs_caret_scroll {
        if line_top < next {
            next = line_top;
        } else if line_top + line_height > next + inner_h {
            next = line_top + line_height - inner_h;
        }
    }
    next.clamp(0.0, max_scroll)
}

pub(crate) fn delete_active_selection_in_node(ctx: &mut NodeEditBorrow<'_>) -> bool {
    let Some(anchor) = ctx.edit.selection_anchor_char_idx.take() else {
        return false;
    };
    let char_count = ctx.node.text.chars().count();
    let a = snap_char_to_grapheme_boundary(&ctx.node.text, anchor.min(char_count));
    let cur =
        snap_char_to_grapheme_boundary(&ctx.node.text, ctx.edit.cursor_char_idx.min(char_count));
    let (start, end) = (a.min(cur), a.max(cur));
    if start < end {
        let (b0, b1) = (
            char_to_byte_idx(&ctx.node.text, start),
            char_to_byte_idx(&ctx.node.text, end),
        );
        ctx.node.text.replace_range(b0..b1, "");
        ctx.edit.cursor_char_idx = start;
        true
    } else {
        false
    }
}

pub(crate) fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

pub(crate) fn byte_to_char_idx(s: &str, byte_idx: usize) -> usize {
    let clamped = byte_idx.min(s.len());
    s.char_indices().take_while(|&(b, _)| b < clamped).count()
}

/// Returns the character index of the grapheme cluster boundary immediately before `cur_char_idx`.
pub(crate) fn prev_grapheme_char_idx(s: &str, cur_char_idx: usize) -> usize {
    if cur_char_idx == 0 {
        return 0;
    }
    let (mut acc, mut prev) = (0, 0);
    for g in UnicodeSegmentation::graphemes(s, true) {
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
pub(crate) fn next_grapheme_char_idx(s: &str, cur_char_idx: usize) -> usize {
    let mut acc = 0;
    for g in UnicodeSegmentation::graphemes(s, true) {
        let g_chars = g.chars().count();
        if acc + g_chars > cur_char_idx {
            return acc + g_chars;
        }
        acc += g_chars;
    }
    acc
}

/// Snaps an arbitrary character index `target_char_idx` to the nearest extended grapheme cluster boundary.
pub(crate) fn snap_char_to_grapheme_boundary(s: &str, target_char_idx: usize) -> usize {
    let mut acc = 0;
    for g in UnicodeSegmentation::graphemes(s, true) {
        let g_chars = g.chars().count();
        if target_char_idx <= acc {
            return acc;
        }
        if target_char_idx < acc + g_chars {
            let (dist_start, dist_end) = (target_char_idx - acc, (acc + g_chars) - target_char_idx);
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

/// Returns the character index of the previous Unicode word boundary before `cur_char_idx`.
pub(crate) fn prev_word_char_idx(s: &str, cur_char_idx: usize) -> usize {
    if cur_char_idx == 0 {
        return 0;
    }
    let mut acc = 0;
    let mut last_word_start = None;
    let mut last_non_ws_start = None;
    for seg in UnicodeSegmentation::split_word_bounds(s) {
        let seg_chars = seg.chars().count();
        if acc >= cur_char_idx {
            break;
        }
        if is_word_segment(seg) {
            last_word_start = Some(acc);
        } else if seg.chars().any(|c| !c.is_whitespace()) {
            last_non_ws_start = Some(acc);
        }
        acc += seg_chars;
    }
    snap_char_to_grapheme_boundary(s, last_word_start.or(last_non_ws_start).unwrap_or(0))
}

/// Returns the character index of the next Unicode word boundary after `cur_char_idx`.
pub(crate) fn next_word_char_idx(s: &str, cur_char_idx: usize) -> usize {
    let char_count = s.chars().count();
    if cur_char_idx >= char_count {
        return char_count;
    }
    let mut acc = 0;
    let mut fallback_non_ws_end = None;
    for seg in UnicodeSegmentation::split_word_bounds(s) {
        let seg_chars = seg.chars().count();
        let seg_end = acc + seg_chars;
        if seg_end > cur_char_idx {
            if is_word_segment(seg) {
                return snap_char_to_grapheme_boundary(s, seg_end);
            }
            if fallback_non_ws_end.is_none() && seg.chars().any(|c| !c.is_whitespace()) {
                fallback_non_ws_end = Some(seg_end);
            }
        }
        acc = seg_end;
    }
    snap_char_to_grapheme_boundary(s, fallback_non_ws_end.unwrap_or(char_count))
}

pub(crate) use crate::view_tree::content_box::sync_node_text_context;
