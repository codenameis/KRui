//! Cursor navigation (`ArrowLeft/Right/Up/Down/Home/End`), `Shift` selection extension,
//! `Ctrl` word navigation/deletion, Undo/Redo stack execution, and `<input>` kind/scroll inspection.

use crate::view_tree::controls::step_focused_control;
use crate::view_tree::grapheme::{
    char_to_byte_idx, delete_active_selection_in_node, next_grapheme_char_idx, next_word_char_idx,
    prev_grapheme_char_idx, prev_word_char_idx, push_undo_snapshot, snap_char_to_grapheme_boundary,
    TextEditSnapshot, MAX_TEXT_UNDO_HISTORY,
};
use crate::view_tree::sparse_tables::NodeEditBorrow;
use crate::view_tree::{InputKind, NodeId, ViewTree};
use krui_a11y::SemanticRole;
use krui_signals::batch;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CursorMotion {
    GraphemeLeft,
    GraphemeRight,
    WordLeft,
    WordRight,
    LineUp,
    LineDown,
    LineStart,
    LineEnd,
}

fn logical_line_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0usize;
    let mut idx = 0usize;
    for ch in text.chars() {
        if ch == '\n' {
            spans.push((start, idx));
            start = idx + 1;
        }
        idx += 1;
    }
    spans.push((start, idx));
    spans
}

fn compute_multiline_motion_target(
    ctx: &mut NodeEditBorrow<'_>,
    cur: usize,
    motion: CursorMotion,
) -> usize {
    if matches!(motion, CursorMotion::LineUp | CursorMotion::LineDown) {
        if let Some(shaped) = ctx.node.cached_text_layout.as_ref() {
            if shaped.lines.len() > 1 {
                let x = ctx
                    .edit
                    .preferred_x
                    .unwrap_or_else(|| shaped.cursor_x_for_char(cur));
                ctx.edit.preferred_x = Some(x);
                let cur_line = shaped.line_for_char(cur);
                let idx = shaped
                    .lines
                    .iter()
                    .position(|l| (l.line_top - cur_line.line_top).abs() < 0.5)
                    .unwrap_or(0);
                if motion == CursorMotion::LineUp {
                    if idx == 0 {
                        return 0;
                    }
                    let prev = &shaped.lines[idx - 1];
                    return snap_char_to_grapheme_boundary(
                        &ctx.node.text,
                        shaped.hit_test(x, prev.line_top + prev.line_height * 0.5),
                    );
                }
                if idx + 1 >= shaped.lines.len() {
                    return ctx.node.text.chars().count();
                }
                let next = &shaped.lines[idx + 1];
                return snap_char_to_grapheme_boundary(
                    &ctx.node.text,
                    shaped.hit_test(x, next.line_top + next.line_height * 0.5),
                );
            }
        }
    }
    compute_logical_multiline_target(&ctx.node.text, cur, motion)
}

fn compute_logical_multiline_target(text: &str, cur: usize, motion: CursorMotion) -> usize {
    let spans = logical_line_spans(text);
    let line_idx = spans
        .iter()
        .position(|&(s, e)| cur >= s && cur <= e)
        .unwrap_or(spans.len().saturating_sub(1));
    let (line_start, line_end) = spans[line_idx];
    let col = cur.saturating_sub(line_start);
    match motion {
        CursorMotion::LineStart => line_start,
        CursorMotion::LineEnd => line_end,
        CursorMotion::LineUp if line_idx > 0 => {
            let (ps, pe) = spans[line_idx - 1];
            snap_char_to_grapheme_boundary(text, ps + col.min(pe - ps))
        }
        CursorMotion::LineUp => 0,
        CursorMotion::LineDown if line_idx + 1 < spans.len() => {
            let (ns, ne) = spans[line_idx + 1];
            snap_char_to_grapheme_boundary(text, ns + col.min(ne - ns))
        }
        CursorMotion::LineDown => line_end,
        _ => cur,
    }
}

fn apply_cursor_motion(ctx: &mut NodeEditBorrow<'_>, motion: CursorMotion, extend_selection: bool) {
    ctx.edit.needs_caret_scroll = true;
    if !matches!(motion, CursorMotion::LineUp | CursorMotion::LineDown) {
        ctx.edit.preferred_x = None;
    }
    let char_count = ctx.node.text.chars().count();
    let cur =
        snap_char_to_grapheme_boundary(&ctx.node.text, ctx.edit.cursor_char_idx.min(char_count));
    if !extend_selection {
        if let Some(anchor) = ctx.edit.selection_anchor_char_idx.take() {
            let a = snap_char_to_grapheme_boundary(&ctx.node.text, anchor.min(char_count));
            let (sel_start, sel_end) = (a.min(cur), a.max(cur));
            if sel_start < sel_end {
                match motion {
                    CursorMotion::GraphemeLeft => return ctx.edit.cursor_char_idx = sel_start,
                    CursorMotion::GraphemeRight => return ctx.edit.cursor_char_idx = sel_end,
                    _ => {}
                }
            }
        }
    } else if ctx.edit.selection_anchor_char_idx.is_none() {
        ctx.edit.selection_anchor_char_idx = Some(cur);
    }

    ctx.edit.cursor_char_idx = match motion {
        CursorMotion::GraphemeLeft => prev_grapheme_char_idx(&ctx.node.text, cur),
        CursorMotion::GraphemeRight => next_grapheme_char_idx(&ctx.node.text, cur),
        CursorMotion::WordLeft if ctx.node.is_secret() => 0,
        CursorMotion::WordLeft => prev_word_char_idx(&ctx.node.text, cur),
        CursorMotion::WordRight if ctx.node.is_secret() => char_count,
        CursorMotion::WordRight => next_word_char_idx(&ctx.node.text, cur),
        _ if ctx.node.role == SemanticRole::MultilineTextInput => {
            compute_multiline_motion_target(ctx, cur, motion)
        }
        CursorMotion::LineUp | CursorMotion::LineStart => 0,
        _ => char_count,
    };
    if extend_selection && ctx.edit.selection_anchor_char_idx == Some(ctx.edit.cursor_char_idx) {
        ctx.edit.selection_anchor_char_idx = None;
    }
}

impl ViewTree {
    fn try_step_focused_control(&mut self, dir: i32, to_bound: Option<bool>) -> bool {
        let act = {
            let mut inner = self.inner.borrow_mut();
            step_focused_control(&mut inner, dir, to_bound)
        };
        if let Some(cbs) = act {
            cbs.invoke();
            true
        } else {
            false
        }
    }

    fn move_focused_input_cursor(&mut self, motion: CursorMotion, extend_selection: bool) -> bool {
        self.with_focused_input(|ctx| {
            if !ctx.edit.ime_state.preedit.is_empty() {
                return None;
            }
            apply_cursor_motion(ctx, motion, extend_selection);
            Some(())
        })
        .is_some()
    }

    /// Moves the text cursor one extended grapheme cluster to the left (`ArrowLeft`).
    pub fn dispatch_cursor_left(&mut self) -> bool {
        if self.try_step_focused_control(-1, None) {
            return true;
        }
        self.move_focused_input_cursor(CursorMotion::GraphemeLeft, false)
    }

    /// Moves the text cursor one extended grapheme cluster to the right (`ArrowRight`).
    pub fn dispatch_cursor_right(&mut self) -> bool {
        if self.try_step_focused_control(1, None) {
            return true;
        }
        self.move_focused_input_cursor(CursorMotion::GraphemeRight, false)
    }

    /// Moves the text cursor up (`ArrowUp`).
    pub fn dispatch_cursor_up(&mut self) -> bool {
        if self.try_step_focused_control(-2, None) {
            return true;
        }
        self.move_focused_input_cursor(CursorMotion::LineUp, false)
    }

    /// Moves the text cursor down (`ArrowDown`).
    pub fn dispatch_cursor_down(&mut self) -> bool {
        if self.try_step_focused_control(2, None) {
            return true;
        }
        self.move_focused_input_cursor(CursorMotion::LineDown, false)
    }

    /// Moves the text cursor to the beginning of the line (`Home`).
    pub fn dispatch_cursor_home(&mut self) -> bool {
        if self.try_step_focused_control(0, Some(false)) {
            return true;
        }
        self.move_focused_input_cursor(CursorMotion::LineStart, false)
    }

    /// Moves the text cursor to the end of the line (`End`).
    pub fn dispatch_cursor_end(&mut self) -> bool {
        if self.try_step_focused_control(0, Some(true)) {
            return true;
        }
        self.move_focused_input_cursor(CursorMotion::LineEnd, false)
    }

    /// Extends selection one extended grapheme cluster to the left (`Shift+ArrowLeft`).
    pub fn dispatch_select_left(&mut self) -> bool {
        self.move_focused_input_cursor(CursorMotion::GraphemeLeft, true)
    }

    /// Extends selection one extended grapheme cluster to the right (`Shift+ArrowRight`).
    pub fn dispatch_select_right(&mut self) -> bool {
        self.move_focused_input_cursor(CursorMotion::GraphemeRight, true)
    }

    /// Extends selection up (`Shift+ArrowUp`).
    pub fn dispatch_select_up(&mut self) -> bool {
        self.move_focused_input_cursor(CursorMotion::LineUp, true)
    }

    /// Extends selection down (`Shift+ArrowDown`).
    pub fn dispatch_select_down(&mut self) -> bool {
        self.move_focused_input_cursor(CursorMotion::LineDown, true)
    }

    /// Extends selection to the beginning of the `<input>` (`Shift+Home`).
    pub fn dispatch_select_home(&mut self) -> bool {
        self.move_focused_input_cursor(CursorMotion::LineStart, true)
    }

    /// Extends selection to the end of the `<input>` (`Shift+End`).
    pub fn dispatch_select_end(&mut self) -> bool {
        self.move_focused_input_cursor(CursorMotion::LineEnd, true)
    }

    /// Moves the text cursor one Unicode word to the left (`Ctrl+ArrowLeft`).
    pub fn dispatch_word_left(&mut self) -> bool {
        self.move_focused_input_cursor(CursorMotion::WordLeft, false)
    }

    /// Moves the text cursor one Unicode word to the right (`Ctrl+ArrowRight`).
    pub fn dispatch_word_right(&mut self) -> bool {
        self.move_focused_input_cursor(CursorMotion::WordRight, false)
    }

    /// Extends selection one Unicode word to the left (`Ctrl+Shift+ArrowLeft`).
    pub fn dispatch_select_word_left(&mut self) -> bool {
        self.move_focused_input_cursor(CursorMotion::WordLeft, true)
    }

    /// Extends selection one Unicode word to the right (`Ctrl+Shift+ArrowRight`).
    pub fn dispatch_select_word_right(&mut self) -> bool {
        self.move_focused_input_cursor(CursorMotion::WordRight, true)
    }

    /// Deletes the active selection or the Unicode word before the cursor (`Ctrl+Backspace`).
    pub fn dispatch_delete_word_backward(&mut self) -> bool {
        self.delete_word_or_selection(true)
    }

    /// Deletes the active selection or the Unicode word after the cursor (`Ctrl+Delete`).
    pub fn dispatch_delete_word_forward(&mut self) -> bool {
        self.delete_word_or_selection(false)
    }

    fn delete_word_or_selection(&mut self, backward: bool) -> bool {
        let Some((cb, updated_text)) = self.with_focused_input(|ctx| {
            if !ctx.edit.ime_state.preedit.is_empty() {
                return None;
            }
            push_undo_snapshot(ctx);
            if !delete_active_selection_in_node(ctx) {
                let char_count = ctx.node.text.chars().count();
                let cur = snap_char_to_grapheme_boundary(
                    &ctx.node.text,
                    ctx.edit.cursor_char_idx.min(char_count),
                );
                if backward && cur > 0 {
                    let prev_w = if ctx.node.is_secret() {
                        0
                    } else {
                        prev_word_char_idx(&ctx.node.text, cur)
                    };
                    let b0 = char_to_byte_idx(&ctx.node.text, prev_w);
                    let b1 = char_to_byte_idx(&ctx.node.text, cur);
                    ctx.node.text.replace_range(b0..b1, "");
                    ctx.edit.cursor_char_idx = prev_w;
                } else if !backward && cur < char_count {
                    let next_w = if ctx.node.is_secret() {
                        char_count
                    } else {
                        next_word_char_idx(&ctx.node.text, cur)
                    };
                    let b0 = char_to_byte_idx(&ctx.node.text, cur);
                    let b1 = char_to_byte_idx(&ctx.node.text, next_w);
                    ctx.node.text.replace_range(b0..b1, "");
                } else {
                    let _ = ctx.edit.undo_stack.pop();
                    return None;
                }
            }
            ctx.edit.preferred_x = None;
            ctx.edit.needs_caret_scroll = true;
            ctx.node.set_text_dirty(true);
            Some((ctx.on_input.clone(), ctx.node.text.clone()))
        }) else {
            return false;
        };
        if let Some(on_input) = cb {
            batch(|| on_input(updated_text));
        }
        true
    }

    /// Undoes the most recent text edit transaction on the focused `<input>` (`Ctrl+Z`).
    pub fn dispatch_undo(&mut self) -> bool {
        let Some((cb, updated_text)) = self.with_focused_input(|ctx| {
            if !ctx.edit.ime_state.preedit.is_empty() {
                return None;
            }
            let prev = ctx.edit.undo_stack.pop()?;
            let current = TextEditSnapshot {
                text: ctx.node.text.clone(),
                cursor_char_idx: ctx.edit.cursor_char_idx,
                selection_anchor_char_idx: ctx.edit.selection_anchor_char_idx,
            };
            if ctx.edit.redo_stack.len() >= MAX_TEXT_UNDO_HISTORY {
                ctx.edit.redo_stack.remove(0);
            }
            ctx.edit.redo_stack.push(current);
            let char_len = prev.text.chars().count();
            ctx.node.text = prev.text;
            ctx.edit.cursor_char_idx = prev.cursor_char_idx.min(char_len);
            ctx.edit.selection_anchor_char_idx =
                prev.selection_anchor_char_idx.map(|a| a.min(char_len));
            ctx.edit.preferred_x = None;
            ctx.edit.needs_caret_scroll = true;
            ctx.node.set_text_dirty(true);
            Some((ctx.on_input.clone(), ctx.node.text.clone()))
        }) else {
            return false;
        };
        if let Some(on_input) = cb {
            batch(|| on_input(updated_text));
        }
        true
    }

    /// Redoes the most recently undone text edit transaction on the focused `<input>` (`Ctrl+Y` / `Ctrl+Shift+Z`).
    pub fn dispatch_redo(&mut self) -> bool {
        let Some((cb, updated_text)) = self.with_focused_input(|ctx| {
            if !ctx.edit.ime_state.preedit.is_empty() {
                return None;
            }
            let next = ctx.edit.redo_stack.pop()?;
            let current = TextEditSnapshot {
                text: ctx.node.text.clone(),
                cursor_char_idx: ctx.edit.cursor_char_idx,
                selection_anchor_char_idx: ctx.edit.selection_anchor_char_idx,
            };
            if ctx.edit.undo_stack.len() >= MAX_TEXT_UNDO_HISTORY {
                ctx.edit.undo_stack.remove(0);
            }
            ctx.edit.undo_stack.push(current);
            let char_len = next.text.chars().count();
            ctx.node.text = next.text;
            ctx.edit.cursor_char_idx = next.cursor_char_idx.min(char_len);
            ctx.edit.selection_anchor_char_idx =
                next.selection_anchor_char_idx.map(|a| a.min(char_len));
            ctx.edit.preferred_x = None;
            ctx.edit.needs_caret_scroll = true;
            ctx.node.set_text_dirty(true);
            Some((ctx.on_input.clone(), ctx.node.text.clone()))
        }) else {
            return false;
        };
        if let Some(on_input) = cb {
            batch(|| on_input(updated_text));
        }
        true
    }

    /// Sets the virtual keyboard [`InputKind`] (`Text`, `Password`, `Number`, `Email`, `Search`) on `node_id`.
    pub fn set_input_kind(&mut self, node_id: NodeId, kind: InputKind) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_none() {
            return;
        }
        inner.ensure_control(node_id).input_kind = kind;
        if kind == InputKind::Password {
            if let Some(node) = inner.node_mut(node_id) {
                node.set_secret(true);
                node.role = SemanticRole::PasswordInput;
            }
        }
    }

    /// Returns the [`InputKind`] configured on `node_id`, or `None` if `node_id` is stale.
    pub fn node_input_kind(&self, node_id: NodeId) -> Option<InputKind> {
        let inner = self.inner.borrow();
        let n = inner.node(node_id)?;
        if n.is_secret() || n.role == SemanticRole::PasswordInput {
            Some(InputKind::Password)
        } else {
            Some(
                inner
                    .control(node_id)
                    .map_or(InputKind::Text, |c| c.input_kind),
            )
        }
    }

    /// Returns the horizontal scroll offset (`scroll_x`) of `node_id`, or `None` if `node_id` is stale.
    pub fn node_scroll_x(&self, node_id: NodeId) -> Option<f32> {
        let inner = self.inner.borrow();
        let node = inner.node(node_id)?;
        if node.role.is_text_editable() {
            Some(inner.text_edit(node_id).map_or(0.0, |e| e.scroll_x))
        } else {
            Some(
                inner
                    .scroll(node_id)
                    .map_or(0.0, |s| s.scroll_x.clamp(0.0, s.cached_max_scroll_x)),
            )
        }
    }

    /// Returns the vertical scroll offset (`scroll_y`) of `node_id`, or `None` if `node_id` is stale.
    pub fn node_scroll_y(&self, node_id: NodeId) -> Option<f32> {
        let inner = self.inner.borrow();
        let node = inner.node(node_id)?;
        if node.role == SemanticRole::MultilineTextInput {
            Some(inner.text_edit(node_id).map_or(0.0, |e| e.scroll_y))
        } else {
            Some(inner.scroll(node_id).map_or(0.0, |s| s.scroll_y))
        }
    }
}
