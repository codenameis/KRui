//! Focused `<input>` / `<textarea>` and selectable text editing, IME auto-commit on blur,
//! grapheme-cluster deletion, and text selection.

use crate::view_tree::form_submit::{refresh_control_validity_if_needed, select_display_label};
use crate::view_tree::grapheme::{
    push_undo_snapshot, sanitize_multiline_text, sanitize_single_line_text,
};
use crate::view_tree::node::{
    byte_to_char_idx, char_to_byte_idx, delete_active_selection_in_node, is_node_text_selectable,
    next_grapheme_char_idx, prev_grapheme_char_idx, snap_char_to_grapheme_boundary,
    sync_node_text_context,
};
use crate::view_tree::sparse_tables::NodeEditBorrow;
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_signals::batch;
use std::rc::Rc;

pub use crate::view_tree::key_routing::caret_rect;

pub(crate) type InputCallbackPair = (Option<Rc<dyn Fn(String)>>, String);

fn insert_clean_str_at_cursor_with_range(
    ctx: &mut NodeEditBorrow<'_>,
    clean: &str,
) -> (usize, usize, InputCallbackPair) {
    push_undo_snapshot(ctx);
    delete_active_selection_in_node(ctx);
    let char_count = ctx.node.text.chars().count();
    let insert_char_idx =
        snap_char_to_grapheme_boundary(&ctx.node.text, ctx.edit.cursor_char_idx.min(char_count));
    let byte_idx = char_to_byte_idx(&ctx.node.text, insert_char_idx);
    ctx.node.text.insert_str(byte_idx, clean);
    let end_char_idx = insert_char_idx + clean.chars().count();
    ctx.edit.cursor_char_idx = end_char_idx;
    ctx.edit.preferred_x = None;
    ctx.edit.needs_caret_scroll = true;
    ctx.node.set_text_dirty(true);
    (
        insert_char_idx,
        end_char_idx,
        (ctx.on_input.clone(), ctx.node.text.clone()),
    )
}

pub(crate) fn insert_clean_str_at_cursor(
    ctx: &mut NodeEditBorrow<'_>,
    clean: &str,
) -> InputCallbackPair {
    insert_clean_str_at_cursor_with_range(ctx, clean).2
}

/// Commits any remaining non-empty `ime_state.preedit` on `ime_target` when focus moves away from it,
/// recording `pending_blur_commit` `(start_char, end_char, preedit_str)` so a late platform `Ime::Commit`
/// either deduplicates identical text or replaces the auto-inserted range with converted text.
pub(crate) fn auto_commit_ime_on_blur(
    inner: &mut ViewTreeInner,
    old_focus: Option<NodeId>,
    new_focus: Option<NodeId>,
) -> Option<InputCallbackPair> {
    if old_focus == new_focus {
        return None;
    }
    let target_id = inner.ime_target.or(old_focus)?;
    if new_focus == Some(target_id) {
        return None;
    }
    if !inner
        .text_edit(target_id)
        .is_some_and(|e| !e.ime_state.preedit.is_empty())
    {
        return None;
    }
    let (cb, updated_text) = {
        let mut ctx = inner.node_edit_mut(target_id)?;
        let preedit_str = std::mem::take(&mut ctx.edit.ime_state.preedit);
        ctx.edit.ime_state.preedit_cursor = None;
        let (start_char, end_char, pair) =
            insert_clean_str_at_cursor_with_range(&mut ctx, &preedit_str);
        ctx.edit.ime_state.pending_blur_commit = Some((start_char, end_char, preedit_str));
        pair
    };
    inner.ime_target = Some(target_id);
    sync_node_text_context(inner, target_id);
    inner.paint_dirty = true;
    Some((cb, updated_text))
}

pub(crate) struct ComposedInputDisplay {
    pub(crate) display_string: String,
    pub(crate) active_caret_char: usize,
    pub(crate) preedit_range: Option<(usize, usize)>,
    pub(crate) clause_range: Option<(usize, usize)>,
}

/// Builds the combined display string (`text` with `ime_state.preedit` spliced at `cursor_char_idx`)
/// and returns its caret and IME preedit/clause character ranges.
pub(crate) fn compose_node_display_and_ime_ranges(
    inner: &ViewTreeInner,
    node_id: NodeId,
) -> ComposedInputDisplay {
    let Some(node) = inner.node(node_id) else {
        return ComposedInputDisplay {
            display_string: String::new(),
            active_caret_char: 0,
            preedit_range: None,
            clause_range: None,
        };
    };
    if node.role == SemanticRole::ComboBox {
        let label = select_display_label(inner, node_id);
        let len = label.chars().count();
        return ComposedInputDisplay {
            display_string: label,
            active_caret_char: len,
            preedit_range: None,
            clause_range: None,
        };
    }
    let ed = inner.text_edit(node_id);
    let raw_cur = ed.map_or(0, |e| e.cursor_char_idx);
    let preedit = ed.map_or("", |e| e.ime_state.preedit.as_str());
    let preedit_cursor = ed.and_then(|e| e.ime_state.preedit_cursor);
    let base_chars = node.text.chars().count();
    let cur_idx = snap_char_to_grapheme_boundary(&node.text, raw_cur.min(base_chars));
    let preedit_chars = preedit.chars().count();

    if node.is_secret() {
        return ComposedInputDisplay {
            display_string: "•".repeat(base_chars + preedit_chars),
            active_caret_char: cur_idx + preedit_chars,
            preedit_range: None,
            clause_range: None,
        };
    }
    if preedit.is_empty() {
        return ComposedInputDisplay {
            display_string: node.text.clone(),
            active_caret_char: cur_idx,
            preedit_range: None,
            clause_range: None,
        };
    }
    build_spliced_preedit_display(&node.text, cur_idx, preedit, preedit_chars, preedit_cursor)
}

fn build_spliced_preedit_display(
    base_text: &str,
    cur_idx: usize,
    preedit: &str,
    preedit_chars: usize,
    preedit_cursor: Option<(usize, usize)>,
) -> ComposedInputDisplay {
    let byte_idx = char_to_byte_idx(base_text, cur_idx);
    let mut combined = String::with_capacity(base_text.len() + preedit.len());
    combined.push_str(&base_text[..byte_idx]);
    combined.push_str(preedit);
    combined.push_str(&base_text[byte_idx..]);

    let preedit_range = Some((cur_idx, cur_idx + preedit_chars));
    let (active_caret_char, clause_range) = match preedit_cursor {
        Some((b_start, b_end)) => {
            let c_start = cur_idx + byte_to_char_idx(preedit, b_start);
            let c_end = cur_idx + byte_to_char_idx(preedit, b_end);
            let (start, end) = (c_start.min(c_end), c_start.max(c_end));
            (end, (start < end).then_some((start, end)))
        }
        None => (cur_idx + preedit_chars, None),
    };
    ComposedInputDisplay {
        display_string: combined,
        active_caret_char,
        preedit_range,
        clause_range,
    }
}

impl ViewTree {
    /// Invokes `f` with mutable access to the currently focused `<input>` / `<textarea>` node and its
    /// sparse `TextEditData`, synchronizing Taffy `TextContext` when `text_dirty` is set.
    pub(crate) fn with_focused_input<R>(
        &mut self,
        f: impl FnOnce(&mut NodeEditBorrow<'_>) -> Option<R>,
    ) -> Option<R> {
        let mut inner = self.inner.borrow_mut();
        let ak_id = inner.a11y_engine.focused_id()?;
        let nid = NodeId::try_from(ak_id).ok()?;
        if inner
            .node(nid)
            .is_none_or(|n| n.is_disabled() || !n.role.is_text_editable())
        {
            return None;
        }
        let result = {
            let mut ctx = inner.node_edit_mut(nid)?;
            f(&mut ctx)?
        };
        if inner.node(nid).is_some_and(|n| n.text_dirty()) {
            refresh_control_validity_if_needed(&mut inner, nid);
            crate::view_tree::sync::notify_selector_state_change(&mut inner, nid);
            sync_node_text_context(&mut inner, nid);
        }
        inner.caret_visible = true;
        inner.paint_dirty = true;
        Some(result)
    }

    /// Invokes `f` with an immutable reference to the currently focused [`crate::view_tree::node::ElementNode`].
    pub(crate) fn with_focused_node<R>(
        &self,
        f: impl FnOnce(&crate::view_tree::node::ElementNode) -> Option<R>,
    ) -> Option<R> {
        let inner = self.inner.borrow();
        let nid = NodeId::try_from(inner.a11y_engine.focused_id()?).ok()?;
        f(inner.node(nid)?)
    }

    /// Invokes `f` with mutable access to `ime_target` (if pinned) or the currently focused `<input>`.
    pub(crate) fn with_ime_target_or_focused_input<R>(
        &mut self,
        f: impl FnOnce(&mut NodeEditBorrow<'_>, NodeId) -> Option<(R, bool)>,
    ) -> Option<R> {
        self.with_ime_target_or_focused_input_opts(false, f)
    }

    pub(crate) fn with_ime_target_or_focused_input_opts<R>(
        &mut self,
        is_ime_preedit: bool,
        f: impl FnOnce(&mut NodeEditBorrow<'_>, NodeId) -> Option<(R, bool)>,
    ) -> Option<R> {
        let mut inner = self.inner.borrow_mut();
        let nid = inner.ime_target.or_else(|| {
            inner
                .a11y_engine
                .focused_id()
                .and_then(|ak_id| NodeId::try_from(ak_id).ok())
        })?;
        if inner
            .node(nid)
            .is_none_or(|n| n.is_disabled() || !n.role.is_text_editable())
        {
            if inner.ime_target == Some(nid) {
                inner.ime_target = None;
            }
            return None;
        }
        let (result, pin_target) = {
            let mut ctx = inner.node_edit_mut(nid)?;
            f(&mut ctx, nid)?
        };
        if pin_target {
            inner.ime_target = Some(nid);
        } else if inner.ime_target == Some(nid) {
            inner.ime_target = None;
        }
        if inner.node(nid).is_some_and(|n| n.text_dirty()) {
            crate::view_tree::sync::notify_selector_state_change(&mut inner, nid);
            if is_ime_preedit {
                crate::view_tree::content_box::sync_node_text_context_for_ime_preedit(
                    &mut inner, nid,
                );
            } else {
                sync_node_text_context(&mut inner, nid);
            }
        }
        inner.caret_visible = true;
        inner.paint_dirty = true;
        Some(result)
    }

    /// Dispatches direct character input from `KeyEvent.text` into the focused `<input>`.
    pub fn dispatch_key_text(&mut self, key_text: &str) -> bool {
        if key_text.chars().any(|c| c.is_control()) {
            return false;
        }
        let Some((cb, updated_text)) = self.with_focused_input(|ctx| {
            if ctx.edit.ime_state.enabled || !ctx.edit.ime_state.preedit.is_empty() {
                return None;
            }
            let clean = if ctx.node.role == SemanticRole::MultilineTextInput {
                sanitize_multiline_text(key_text)
            } else {
                sanitize_single_line_text(key_text)
            };
            if clean.is_empty() {
                return None;
            }
            Some(insert_clean_str_at_cursor(ctx, &clean))
        }) else {
            return false;
        };
        if let Some(on_input) = cb {
            batch(|| on_input(updated_text));
        }
        true
    }

    /// Selects all text (`Ctrl+A`) in the currently focused selectable node (`<input>` or `user-select: text`).
    pub fn select_all(&mut self) -> bool {
        let mut inner = self.inner.borrow_mut();
        let focused = inner
            .a11y_engine
            .focused_id()
            .and_then(|ak_id| NodeId::try_from(ak_id).ok());
        let Some(nid) = focused.or(inner.interaction.selection_owner) else {
            return false;
        };
        let Some(char_count) = inner.node(nid).and_then(|node| {
            is_node_text_selectable(node)
                .then(|| node.text.chars().count())
                .filter(|&len| len > 0)
        }) else {
            return false;
        };
        let edit = inner.ensure_text_edit(nid);
        edit.selection_anchor_char_idx = Some(0);
        edit.cursor_char_idx = char_count;
        edit.needs_caret_scroll = true;
        inner.paint_dirty = true;
        true
    }

    fn clamp_selection_range(
        text: &str,
        cursor_char_idx: usize,
        selection_anchor_char_idx: Option<usize>,
    ) -> Option<(usize, usize, usize)> {
        let anchor = selection_anchor_char_idx?;
        let char_count = text.chars().count();
        let a = snap_char_to_grapheme_boundary(text, anchor.min(char_count));
        let cur = snap_char_to_grapheme_boundary(text, cursor_char_idx.min(char_count));
        let (start, end) = (a.min(cur), a.max(cur));
        (start < end).then(|| {
            (
                start,
                char_to_byte_idx(text, start),
                char_to_byte_idx(text, end),
            )
        })
    }

    /// Returns the currently selected text (`Ctrl+C`) from the focused selectable node, if any.
    ///
    /// Secret/password inputs (`is_secret == true`) never expose their text to the clipboard.
    pub fn selected_text(&self) -> Option<String> {
        let inner = self.inner.borrow();
        let focused = inner
            .a11y_engine
            .focused_id()
            .and_then(|id| NodeId::try_from(id).ok());
        let nid = focused.or(inner.interaction.selection_owner)?;
        let node = inner.node(nid)?;
        if node.is_secret() || !is_node_text_selectable(node) {
            return None;
        }
        let ed = inner.text_edit(nid)?;
        let (_, b0, b1) = Self::clamp_selection_range(
            &node.text,
            ed.cursor_char_idx,
            ed.selection_anchor_char_idx,
        )?;
        Some(node.text[b0..b1].to_string())
    }

    /// Cuts the currently selected text (`Ctrl+X`) from a focused `<input>` widget and returns the cut string.
    pub fn cut_selected_text(&mut self) -> Option<String> {
        let (cut_str, cb, updated_text) = self.with_focused_input(|ctx| {
            if ctx.node.is_secret() || !ctx.edit.ime_state.preedit.is_empty() {
                return None;
            }
            let (start, b0, b1) = Self::clamp_selection_range(
                &ctx.node.text,
                ctx.edit.cursor_char_idx,
                ctx.edit.selection_anchor_char_idx,
            )?;
            push_undo_snapshot(ctx);
            let cut = ctx.node.text[b0..b1].to_string();
            ctx.node.text.replace_range(b0..b1, "");
            ctx.edit.cursor_char_idx = start;
            ctx.edit.selection_anchor_char_idx = None;
            ctx.edit.preferred_x = None;
            ctx.edit.needs_caret_scroll = true;
            ctx.node.set_text_dirty(true);
            Some((cut, ctx.on_input.clone(), ctx.node.text.clone()))
        })?;
        if let Some(on_input) = cb {
            batch(|| on_input(updated_text));
        }
        Some(cut_str)
    }

    /// Inserts committed/pasted text at the current cursor position in the focused `<input>` widget.
    pub fn dispatch_text_input(&mut self, raw_committed: &str) -> bool {
        let Some((cb, updated_text)) = self.with_focused_input(|ctx| {
            let clean = if ctx.node.role == SemanticRole::MultilineTextInput {
                sanitize_multiline_text(raw_committed)
            } else {
                sanitize_single_line_text(raw_committed)
            };
            if clean.is_empty() {
                return None;
            }
            ctx.edit.ime_state.preedit.clear();
            ctx.edit.ime_state.preedit_cursor = None;
            ctx.edit.ime_state.pending_blur_commit = None;
            Some(insert_clean_str_at_cursor(ctx, &clean))
        }) else {
            return false;
        };
        if let Some(on_input) = cb {
            batch(|| on_input(updated_text));
        }
        true
    }

    /// Convenience wrapper around [`ViewTree::ime_preedit`] with `cursor_offset = None`.
    pub fn dispatch_ime_preedit(&mut self, preedit: &str) -> bool {
        self.ime_preedit(preedit, None)
    }

    fn delete_grapheme_or_selection(&mut self, backspace: bool) -> bool {
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
                if backspace && cur > 0 {
                    let prev_char = prev_grapheme_char_idx(&ctx.node.text, cur);
                    let start_byte = char_to_byte_idx(&ctx.node.text, prev_char);
                    let end_byte = char_to_byte_idx(&ctx.node.text, cur);
                    ctx.node.text.replace_range(start_byte..end_byte, "");
                    ctx.edit.cursor_char_idx = prev_char;
                } else if !backspace && cur < char_count {
                    let next_char = next_grapheme_char_idx(&ctx.node.text, cur);
                    let start_byte = char_to_byte_idx(&ctx.node.text, cur);
                    let end_byte = char_to_byte_idx(&ctx.node.text, next_char);
                    ctx.node.text.replace_range(start_byte..end_byte, "");
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

    /// Deletes the active selection or the extended grapheme cluster before the cursor (`Backspace`).
    pub fn dispatch_backspace(&mut self) -> bool {
        self.delete_grapheme_or_selection(true)
    }

    /// Deletes the active selection or the extended grapheme cluster after the cursor (`Delete`).
    pub fn dispatch_delete(&mut self) -> bool {
        self.delete_grapheme_or_selection(false)
    }

    /// Returns the selection anchor character index of `node_id` (`Some(anchor)` when active,
    /// or `None` when collapsed or `node_id` is stale).
    pub fn node_selection_anchor_char_idx(&self, node_id: NodeId) -> Option<usize> {
        let inner = self.inner.borrow();
        let _ = inner.node(node_id)?;
        inner
            .text_edit(node_id)
            .and_then(|e| e.selection_anchor_char_idx)
    }
}
