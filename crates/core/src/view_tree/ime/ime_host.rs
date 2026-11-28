//! Platform-neutral IME abstraction (`EditingState`, `ImeEvent`, `InputKind`, `EditorActionKind`, `TextInputHost`)
//! and UTF-8 <-> UTF-16 byte/code-unit range conversion for Android (`InputConnection`) and iOS (`UITextInput`).

use crate::view_tree::grapheme::push_undo_snapshot;
use crate::view_tree::node::{
    byte_to_char_idx, char_to_byte_idx, delete_active_selection_in_node, next_grapheme_char_idx,
    prev_grapheme_char_idx, snap_char_to_grapheme_boundary,
};
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_signals::batch;
pub use krui_ui_types::InputKind;
use std::ops::Range;

/// Mobile / desktop IME action button (`Enter` / `Done` / `Search` / `Next` / `Send`) triggered by
/// [`ImeEvent::EditorAction`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorActionKind {
    /// Commits the current input value (`onchange`).
    #[default]
    Done,
    /// Submits or triggers search (`onchange`).
    Search,
    /// Advances focus to the next focusable widget (`Tab`).
    Next,
    /// Advances focus to the previous focusable widget (`Shift+Tab`).
    Previous,
    /// Sends or submits the current input (`onchange`).
    Send,
    /// Navigates / submits (`onchange`).
    Go,
}

/// Platform-neutral snapshot of an `<input>` node's text buffer, active selection, and IME composition range.
///
/// All ranges (`selection`, `composing`) are **UTF-8 byte ranges** within [`EditingState::text`]
/// (which includes any active in-progress `preedit` spliced at the cursor position).
/// Use [`utf8_to_utf16_range`] and [`utf16_to_utf8_range`] when bridging to Android JNI or iOS `UITextInput`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditingState {
    /// Complete display text buffer (committed text with any active IME preedit spliced at the cursor).
    pub text: String,
    /// Active selection or caret range in UTF-8 byte offsets within `text` (`start == end` when no selection).
    pub selection: Range<usize>,
    /// Active IME composition (`preedit` / `markedText`) range in UTF-8 byte offsets within `text`, if any.
    pub composing: Option<Range<usize>>,
}

impl Default for EditingState {
    fn default() -> Self {
        Self {
            text: String::new(),
            selection: 0..0,
            composing: None,
        }
    }
}

/// Platform-neutral IME lifecycle and composition event dispatched into [`ViewTree::dispatch_ime_event`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeEvent {
    /// Platform IME session was enabled for the focused `<input>`.
    Enabled,
    /// In-progress composition string (`preedit`) and optional UTF-8 byte cursor range within `text`.
    Preedit {
        /// Composition string owned by the OS IME.
        text: String,
        /// Optional UTF-8 byte cursor/clause range `(start, end)` relative to `text`.
        cursor_offset: Option<(usize, usize)>,
    },
    /// Mobile/desktop alias for [`ImeEvent::Preedit`] (`InputConnection.setComposingText` / `setMarkedText`).
    SetComposing {
        /// Composition string owned by the OS IME.
        text: String,
        /// Optional UTF-8 byte cursor/clause range `(start, end)` relative to `text`.
        cursor: Option<(usize, usize)>,
    },
    /// Finalized composition string committed into the target `<input>`.
    Commit(String),
    /// Finishes any active composition in-place (`InputConnection.finishComposingText` / `unmarkText`),
    /// committing the current `preedit` as final text and unpinning `ime_target`.
    FinishComposing,
    /// Deletes `before_bytes` UTF-8 bytes before the cursor/selection and `after_bytes` UTF-8 bytes
    /// after the cursor/selection (`InputConnection.deleteSurroundingText`, clamped to grapheme boundaries).
    DeleteSurrounding {
        /// Number of UTF-8 bytes to delete before the current selection/caret.
        before_bytes: usize,
        /// Number of UTF-8 bytes to delete after the current selection/caret.
        after_bytes: usize,
    },
    /// Sets the active selection or caret range in UTF-8 byte offsets within the committed text.
    SetSelection(Range<usize>),
    /// Performs a mobile/IME editor action (`Done`, `Next`, `Previous`, `Search`, `Send`, `Go`).
    EditorAction(EditorActionKind),
    /// Platform IME session was closed or disabled.
    Disabled,
}

/// Platform adapter trait connecting `krui-core`'s IME state machine to an OS window,
/// Android `InputMethodManager`, iOS `UITextInput`, or an embedded game/VR host.
pub trait TextInputHost {
    /// Enables or disables OS IME composition for the host surface.
    fn set_ime_allowed(&mut self, allowed: bool);

    /// Updates the logical caret / composition rectangle (for positioning the OS IME candidate popup).
    fn set_ime_cursor_area(&mut self, rect: LayoutRect);

    /// Pushes the latest [`EditingState`] (UTF-8 text, selection, and composing range) to the platform host.
    fn update_editing_state(&mut self, state: &EditingState);

    /// Alias for [`TextInputHost::update_editing_state`] matching the B5 host contract.
    fn set_editing_state(&mut self, state: &EditingState) {
        self.update_editing_state(state);
    }

    /// Alias for [`TextInputHost::set_ime_cursor_area`] matching the B5 host contract.
    fn set_caret_rect(&mut self, rect: LayoutRect) {
        self.set_ime_cursor_area(rect);
    }

    /// Requests the platform host to show the virtual keyboard (or enable IME with [`InputKind`] purpose).
    fn show_keyboard(&mut self, _kind: InputKind) {
        self.set_ime_allowed(true);
    }

    /// Requests the platform host to hide the virtual keyboard (or disable IME).
    fn hide_keyboard(&mut self) {
        self.set_ime_allowed(false);
    }

    /// Notifies the platform host of the currently focused editable node identity (`A11yId`),
    /// enabling explicit OS IME session resets when focus switches directly between `<input>` controls.
    fn set_focused_node_identity(&mut self, _node_key: Option<u64>) {}
}

/// Converts a UTF-8 byte offset within `text` to a UTF-16 code unit offset (for Android/iOS bridges).
pub fn utf8_to_utf16_offset(text: &str, utf8_byte_offset: usize) -> usize {
    let clamped = clamp_to_char_boundary_floor(text, utf8_byte_offset);
    text[..clamped].encode_utf16().count()
}

/// Converts a UTF-16 code unit offset within `text` back to a UTF-8 byte offset.
pub fn utf16_to_utf8_offset(text: &str, utf16_offset: usize) -> usize {
    let mut u16_count = 0usize;
    for (byte_idx, ch) in text.char_indices() {
        if u16_count >= utf16_offset {
            return byte_idx;
        }
        let ch_u16_len = ch.len_utf16();
        if u16_count + ch_u16_len > utf16_offset {
            return byte_idx;
        }
        u16_count += ch_u16_len;
    }
    text.len()
}

/// Converts a UTF-8 byte [`Range<usize>`] within `text` to a UTF-16 code unit [`Range<usize>`].
pub fn utf8_to_utf16_range(text: &str, range: Range<usize>) -> Range<usize> {
    let start = utf8_to_utf16_offset(text, range.start);
    let end = utf8_to_utf16_offset(text, range.end);
    start.min(end)..start.max(end)
}

/// Converts a UTF-16 code unit [`Range<usize>`] within `text` to a UTF-8 byte [`Range<usize>`].
pub fn utf16_to_utf8_range(text: &str, range: Range<usize>) -> Range<usize> {
    let start = utf16_to_utf8_offset(text, range.start);
    let end = utf16_to_utf8_offset(text, range.end);
    start.min(end)..start.max(end)
}

fn clamp_to_char_boundary_floor(text: &str, mut byte_idx: usize) -> usize {
    byte_idx = byte_idx.min(text.len());
    while byte_idx > 0 && !text.is_char_boundary(byte_idx) {
        byte_idx -= 1;
    }
    byte_idx
}

fn compute_surrounding_delete_char_range(
    text: &str,
    cursor_char_idx: usize,
    before_bytes: usize,
    after_bytes: usize,
) -> Option<(usize, usize)> {
    let char_count = text.chars().count();
    let cur_char = snap_char_to_grapheme_boundary(text, cursor_char_idx.min(char_count));
    let cur_byte = char_to_byte_idx(text, cur_char);
    let raw_start_byte = cur_byte.saturating_sub(before_bytes);
    let clamped_start_byte = clamp_to_char_boundary_floor(text, raw_start_byte);
    let mut start_char =
        snap_char_to_grapheme_boundary(text, byte_to_char_idx(text, clamped_start_byte));
    if start_char == cur_char && before_bytes > 0 && cur_char > 0 {
        start_char = prev_grapheme_char_idx(text, cur_char);
    }
    let mut clamped_end_byte = cur_byte.saturating_add(after_bytes).min(text.len());
    while clamped_end_byte < text.len() && !text.is_char_boundary(clamped_end_byte) {
        clamped_end_byte += 1;
    }
    let raw_end_char = byte_to_char_idx(text, clamped_end_byte);
    let snapped_end = snap_char_to_grapheme_boundary(text, raw_end_char);
    let mut end_char = if snapped_end < raw_end_char {
        next_grapheme_char_idx(text, snapped_end)
    } else {
        snapped_end
    };
    if end_char == cur_char && after_bytes > 0 && cur_char < char_count {
        end_char = next_grapheme_char_idx(text, cur_char);
    }
    (start_char < end_char).then_some((start_char, end_char))
}

/// Single authoritative resolver for the `<input>` node that owns the active [`TextInputHost`] session.
pub(crate) fn resolve_host_input_node_id(inner: &ViewTreeInner) -> Option<NodeId> {
    if let Some(focused_nid) = inner
        .a11y_engine
        .focused_id()
        .and_then(|ak_id| NodeId::try_from(ak_id).ok())
    {
        if let Some(node) = inner.node(focused_nid) {
            if !node.is_disabled() && node.role.is_text_editable() {
                return Some(focused_nid);
            }
        }
    }
    if let Some(target_nid) = inner.ime_target {
        if let Some(node) = inner.node(target_nid) {
            let has_preedit = inner
                .text_edit(target_nid)
                .is_some_and(|e| !e.ime_state.preedit.is_empty());
            if !node.is_disabled() && has_preedit && node.role.is_text_editable() {
                return Some(target_nid);
            }
        }
    }
    None
}

pub(crate) fn build_node_editing_state(inner: &ViewTreeInner, nid: NodeId) -> Option<EditingState> {
    let node = inner.node(nid)?;
    let ed = inner.text_edit(nid);
    let raw_cur = ed.map_or(0, |e| e.cursor_char_idx);
    let sel_anchor = ed.and_then(|e| e.selection_anchor_char_idx);
    let preedit = ed.map_or("", |e| e.ime_state.preedit.as_str());
    let preedit_cursor = ed.and_then(|e| e.ime_state.preedit_cursor);

    let base_chars = node.text.chars().count();
    let cur_char = snap_char_to_grapheme_boundary(&node.text, raw_cur.min(base_chars));
    let cur_byte = char_to_byte_idx(&node.text, cur_char);

    if preedit.is_empty() {
        let sel_range = match sel_anchor {
            Some(anchor_char) => {
                let a_char =
                    snap_char_to_grapheme_boundary(&node.text, anchor_char.min(base_chars));
                let a_byte = char_to_byte_idx(&node.text, a_char);
                a_byte.min(cur_byte)..a_byte.max(cur_byte)
            }
            None => cur_byte..cur_byte,
        };
        return Some(EditingState {
            text: node.text.clone(),
            selection: sel_range,
            composing: None,
        });
    }

    let mut combined = String::with_capacity(node.text.len() + preedit.len());
    combined.push_str(&node.text[..cur_byte]);
    combined.push_str(preedit);
    combined.push_str(&node.text[cur_byte..]);

    let comp_start = cur_byte;
    let comp_end = cur_byte + preedit.len();
    let selection = match preedit_cursor {
        Some((s, e)) => {
            let cs = clamp_to_char_boundary_floor(preedit, s);
            let ce = clamp_to_char_boundary_floor(preedit, e);
            (comp_start + cs.min(ce))..(comp_start + cs.max(ce))
        }
        None => comp_end..comp_end,
    };
    Some(EditingState {
        text: combined,
        selection,
        composing: Some(comp_start..comp_end),
    })
}

impl ViewTree {
    /// Dispatches a platform-neutral [`ImeEvent`] into `ime_target` or the focused `<input>`.
    pub fn dispatch_ime_event(&mut self, event: ImeEvent) -> bool {
        match event {
            ImeEvent::Enabled => self.ime_enabled(),
            ImeEvent::Preedit {
                text,
                cursor_offset,
            } => self.ime_preedit(&text, cursor_offset),
            ImeEvent::SetComposing { text, cursor } => self.ime_preedit(&text, cursor),
            ImeEvent::Commit(text) => self.ime_commit(&text),
            ImeEvent::FinishComposing => self.ime_finish_composing(),
            ImeEvent::DeleteSurrounding {
                before_bytes,
                after_bytes,
            } => self.ime_delete_surrounding(before_bytes, after_bytes),
            ImeEvent::SetSelection(range) => self.ime_set_selection(range),
            ImeEvent::EditorAction(action) => self.ime_editor_action(action),
            ImeEvent::Disabled => self.ime_disabled(),
        }
    }

    /// Deletes `before_bytes` UTF-8 bytes before the cursor and `after_bytes` UTF-8 bytes after
    /// the cursor on the focused `<input>` (snapped to extended grapheme cluster boundaries).
    pub fn ime_delete_surrounding(&mut self, before_bytes: usize, after_bytes: usize) -> bool {
        if before_bytes == 0 && after_bytes == 0 {
            return false;
        }
        let Some((cb, updated_text)) = self.with_focused_input(|ctx| {
            if !ctx.edit.ime_state.preedit.is_empty() {
                return None;
            }
            push_undo_snapshot(ctx);
            let sel_deleted = delete_active_selection_in_node(ctx);
            if let Some((start_char, end_char)) = compute_surrounding_delete_char_range(
                &ctx.node.text,
                ctx.edit.cursor_char_idx,
                before_bytes,
                after_bytes,
            ) {
                let b0 = char_to_byte_idx(&ctx.node.text, start_char);
                let b1 = char_to_byte_idx(&ctx.node.text, end_char);
                ctx.node.text.replace_range(b0..b1, "");
                ctx.edit.cursor_char_idx = start_char;
                ctx.edit.selection_anchor_char_idx = None;
            } else if !sel_deleted {
                let _ = ctx.edit.undo_stack.pop();
                return None;
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

    /// Sets the active selection or caret range in UTF-8 byte offsets within the focused `<input>`'s
    /// committed text (snapped to extended grapheme cluster boundaries).
    pub fn ime_set_selection(&mut self, range: Range<usize>) -> bool {
        self.with_focused_input(|ctx| {
            let start_byte = clamp_to_char_boundary_floor(&ctx.node.text, range.start);
            let end_byte = clamp_to_char_boundary_floor(&ctx.node.text, range.end);
            let start_char = snap_char_to_grapheme_boundary(
                &ctx.node.text,
                byte_to_char_idx(&ctx.node.text, start_byte),
            );
            let end_char = snap_char_to_grapheme_boundary(
                &ctx.node.text,
                byte_to_char_idx(&ctx.node.text, end_byte),
            );
            if start_char == end_char {
                ctx.edit.selection_anchor_char_idx = None;
                ctx.edit.cursor_char_idx = end_char;
            } else {
                ctx.edit.selection_anchor_char_idx = Some(start_char.min(end_char));
                ctx.edit.cursor_char_idx = start_char.max(end_char);
            }
            ctx.edit.preferred_x = None;
            ctx.edit.needs_caret_scroll = true;
            Some(())
        })
        .is_some()
    }

    /// Executes an [`EditorActionKind`] (`Done`, `Search`, `Send`, `Go` -> commits composition & triggers
    /// `onchange`; `Next` -> `Tab`; `Previous` -> `Shift+Tab`).
    pub fn ime_editor_action(&mut self, action: EditorActionKind) -> bool {
        let _ = self.ime_finish_composing();
        match action {
            EditorActionKind::Next => self.dispatch_key_tab(false).is_some(),
            EditorActionKind::Previous => self.dispatch_key_tab(true).is_some(),
            EditorActionKind::Done
            | EditorActionKind::Search
            | EditorActionKind::Send
            | EditorActionKind::Go => self.dispatch_key_enter(),
        }
    }

    /// Returns the current [`EditingState`] of the active host `<input>` node, if any.
    pub fn focused_editing_state(&self) -> Option<EditingState> {
        let inner = self.inner.borrow();
        let nid = resolve_host_input_node_id(&inner)?;
        build_node_editing_state(&inner, nid)
    }

    /// Returns the [`InputKind`] of the active host `<input>` node, if any.
    pub fn focused_input_kind(&self) -> Option<InputKind> {
        let inner = self.inner.borrow();
        let nid = resolve_host_input_node_id(&inner)?;
        let node = inner.node(nid)?;
        Some(
            if node.is_secret() || node.role == SemanticRole::PasswordInput {
                InputKind::Password
            } else {
                inner.control(nid).map_or(InputKind::Text, |c| c.input_kind)
            },
        )
    }

    /// Returns the opaque node identity (`A11yId`) of the active host `<input>` or `ime_target`.
    pub fn focused_ime_node_key(&self) -> Option<u64> {
        let inner = self.inner.borrow();
        resolve_host_input_node_id(&inner).map(|nid| crate::view_tree::A11yId::from(nid).0)
    }

    /// Synchronizes a [`TextInputHost`] with the `ViewTree`'s active `<input>` focus, [`InputKind`],
    /// caret area, and [`EditingState`].
    pub fn sync_text_input_host<H: TextInputHost + ?Sized>(&self, host: &mut H) {
        host.set_focused_node_identity(self.focused_ime_node_key());
        if let (Some(kind), Some(rect), Some(state)) = (
            self.focused_input_kind(),
            self.focused_ime_cursor_area(),
            self.focused_editing_state(),
        ) {
            host.show_keyboard(kind);
            host.set_caret_rect(rect);
            host.set_editing_state(&state);
        } else {
            host.hide_keyboard();
        }
    }
}
