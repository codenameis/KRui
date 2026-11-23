//! Text click granularity (single/double/triple-click, Shift+Click) and drag-edge autoscroll helpers.

use crate::view_tree::content_box::content_box_rect;
use crate::view_tree::hit_test::effective_node_world_matrix_and_rect;
use crate::view_tree::node::{is_node_text_selectable, snap_char_to_grapheme_boundary};
use crate::view_tree::paint_text::compute_text_origin_x_offset;
use crate::view_tree::pointer_capture::SelectionGranularity;
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;

const AUTOSCROLL_GAIN: f32 = 6.0;
const AUTOSCROLL_MIN_PX_PER_SEC: f32 = 64.0;
const AUTOSCROLL_MAX_PX_PER_SEC: f32 = 640.0;

/// Returns `(start_char, end_char)` word boundaries enclosing `ch_idx`.
pub(crate) fn word_bounds_for_char(text: &str, ch_idx: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let mut idx = ch_idx.min(chars.len());
    if idx == chars.len() {
        idx -= 1;
    }
    let cls = char_word_class(chars[idx]);
    let mut start = idx;
    while start > 0 && char_word_class(chars[start - 1]) == cls {
        start -= 1;
    }
    let mut end = idx + 1;
    while end < chars.len() && char_word_class(chars[end]) == cls {
        end += 1;
    }
    (start, end)
}

/// Returns `(start_char, end_char)` line boundaries enclosing `ch_idx`.
pub(crate) fn line_bounds_for_char(text: &str, ch_idx: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let idx = ch_idx.min(len);
    let mut start = idx;
    while start > 0 && chars[start - 1] != '\n' {
        start -= 1;
    }
    let mut end = idx;
    while end < len && chars[end] != '\n' {
        end += 1;
    }
    (start, end)
}

fn char_word_class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// Maps node-local coordinates `(lx, ly)` plus scroll offsets to the snapped grapheme char index,
/// returning `(char_idx, total_char_count, text_clone, is_single_line)`.
pub(crate) fn hit_test_node_char_at_local(
    inner: &ViewTreeInner,
    nid: NodeId,
    lx: f32,
    ly: f32,
    local_rect: LayoutRect,
) -> Option<(usize, usize, String, bool)> {
    let (scroll_x, scroll_y) = inner
        .text_edit(nid)
        .map_or((0.0, 0.0), |e| (e.scroll_x, e.scroll_y));
    inner.node(nid).map(|node| {
        let cb = content_box_rect(local_rect, &node.style.layout);
        let origin_x_off = node
            .cached_text_layout
            .as_ref()
            .map(|s| {
                compute_text_origin_x_offset(
                    &node.style,
                    node.role,
                    local_rect.width,
                    s.measured_width,
                )
            })
            .unwrap_or(cb.x - local_rect.x);
        let local_x = lx - (local_rect.x + origin_x_off) + scroll_x;
        let local_y = ly - cb.y + scroll_y;
        let count = node.text.chars().count();
        let raw_clicked = node
            .cached_text_layout
            .as_ref()
            .map_or(count, |shaped| shaped.hit_test(local_x, local_y).min(count));
        (
            snap_char_to_grapheme_boundary(&node.text, raw_clicked),
            count,
            node.text.clone(),
            node.role.is_single_line_input(),
        )
    })
}

/// Hits text layout at local `(lx, ly)` and applies single/double/triple-click or `Shift+Click` selection.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_click_selection_from_local_coords(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    lx: f32,
    ly: f32,
    local_rect: LayoutRect,
    selectable: bool,
    select_all_on_click: bool,
    preserve_shift: bool,
    click_count: u8,
) {
    let Some((clicked_char, char_count, text, is_single_line)) =
        hit_test_node_char_at_local(inner, nid, lx, ly, local_rect)
    else {
        return;
    };
    let prev_anchor = preserve_shift.then(|| {
        inner
            .text_edit(nid)
            .map(|e| e.selection_anchor_char_idx.unwrap_or(e.cursor_char_idx))
            .unwrap_or(clicked_char)
    });
    if let Some(anchor) = prev_anchor {
        inner.interaction.selection_granularity = SelectionGranularity::Char;
        inner.selecting_node = Some(nid);
        set_edit_cursor_and_anchor(inner, nid, Some(anchor), clicked_char);
        return;
    }
    apply_unshifted_click_granularity(
        inner,
        nid,
        clicked_char,
        char_count,
        &text,
        is_single_line,
        selectable,
        select_all_on_click,
        click_count,
    );
}

#[allow(clippy::too_many_arguments)]
fn apply_unshifted_click_granularity(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    clicked_char: usize,
    char_count: usize,
    text: &str,
    is_single_line: bool,
    selectable: bool,
    select_all_on_click: bool,
    click_count: u8,
) {
    if selectable && click_count == 2 {
        let (ws, we) = word_bounds_for_char(text, clicked_char);
        inner.interaction.selection_granularity = SelectionGranularity::Word {
            anchor_start: ws,
            anchor_end: we,
        };
        inner.selecting_node = Some(nid);
        set_edit_cursor_and_anchor(inner, nid, Some(ws), we);
    } else if selectable && click_count >= 3 {
        let (ls, le) = if is_single_line {
            (0, char_count)
        } else {
            line_bounds_for_char(text, clicked_char)
        };
        inner.interaction.selection_granularity = SelectionGranularity::Line {
            anchor_start: ls,
            anchor_end: le,
        };
        inner.selecting_node = Some(nid);
        set_edit_cursor_and_anchor(inner, nid, Some(ls), le);
    } else if select_all_on_click && selectable {
        inner.interaction.selection_granularity = SelectionGranularity::Char;
        set_edit_cursor_and_anchor(inner, nid, Some(0), char_count);
    } else {
        inner.interaction.selection_granularity = SelectionGranularity::Char;
        inner.selecting_node = selectable.then_some(nid);
        set_edit_cursor_and_anchor(inner, nid, selectable.then_some(clicked_char), clicked_char);
    }
}

fn set_edit_cursor_and_anchor(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    anchor: Option<usize>,
    cursor: usize,
) {
    let ed = inner.ensure_text_edit(nid);
    ed.preferred_x = None;
    ed.needs_caret_scroll = true;
    ed.selection_anchor_char_idx = anchor;
    ed.cursor_char_idx = cursor;
}

/// Computes `(dx, dy)` autoscroll step when dragging text selection outside node content bounds.
pub(crate) fn compute_drag_autoscroll_step(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    (px, py): (f32, f32),
    dt: f32,
) -> (f32, f32) {
    let (aff, rect) = effective_node_world_matrix_and_rect(inner, nid);
    let Some((lx, ly)) = aff.inverse().map(|inv| inv.transform_point(px, py)) else {
        return (0.0, 0.0);
    };
    let Some(node) = inner.node(nid) else {
        return (0.0, 0.0);
    };
    if !is_node_text_selectable(node) {
        return (0.0, 0.0);
    }
    let cb = content_box_rect(rect, &node.style.layout);
    let (left, right) = (cb.x, cb.x + cb.width);
    let (top, bottom) = (cb.y, cb.y + cb.height);
    let (mw, mh) = node
        .cached_text_layout
        .as_ref()
        .map_or((0.0, 0.0), |s| (s.measured_width, s.measured_height));
    let single_line = node.role.is_single_line_input();
    let multi_line = node.role == SemanticRole::MultilineTextInput;
    let max_sx = (mw - (right - left)).max(0.0);
    let max_sy = (mh - (bottom - top)).max(0.0);
    let ed = inner.ensure_text_edit(nid);
    let mut moved_x = 0.0;
    if single_line && max_sx > 0.0 {
        let next = (ed.scroll_x + overshoot_velocity(lx, left, right) * dt).clamp(0.0, max_sx);
        moved_x = next - ed.scroll_x;
        ed.scroll_x = next;
    }
    let mut moved_y = 0.0;
    if multi_line && max_sy > 0.0 {
        let next = (ed.scroll_y + overshoot_velocity(ly, top, bottom) * dt).clamp(0.0, max_sy);
        moved_y = next - ed.scroll_y;
        ed.scroll_y = next;
    }
    (moved_x, moved_y)
}

fn overshoot_velocity(coord: f32, min_edge: f32, max_edge: f32) -> f32 {
    if coord < min_edge {
        let over = min_edge - coord;
        -(over * AUTOSCROLL_GAIN).clamp(AUTOSCROLL_MIN_PX_PER_SEC, AUTOSCROLL_MAX_PX_PER_SEC)
    } else if coord > max_edge {
        let over = coord - max_edge;
        (over * AUTOSCROLL_GAIN).clamp(AUTOSCROLL_MIN_PX_PER_SEC, AUTOSCROLL_MAX_PX_PER_SEC)
    } else {
        0.0
    }
}
