//! Grapheme-aware single-line and multiline [`TextEditBuffer`] with preferred-X vertical
//! cursor navigation, selection expansion, bounded Undo/Redo, and clipboard helpers.

use crate::grapheme_nav::{
    char_to_byte_idx, closest_grapheme_on_line, line_end_char_idx, line_start_char_idx,
    next_word_char_idx, normalize_editor_text, prev_word_char_idx,
};
pub use crate::grapheme_nav::{
    next_grapheme_char_idx, prev_grapheme_char_idx, snap_char_to_grapheme_boundary,
};
pub use crate::ime_geometry::{
    compute_ime_visuals, ImeUnderlineSegment, ImeVisualGeometry, DEFAULT_CANDIDATE_CARET_WIDTH_PX,
    MIN_IME_SEGMENT_WIDTH_PX,
};
use crate::ShapedTextLayout;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// Maximum number of Undo/Redo snapshots retained by a [`TextEditBuffer`].
pub const MAX_EDIT_UNDO_HISTORY: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
struct EditSnapshot {
    text: String,
    cursor: usize,
    selection_anchor: Option<usize>,
}

/// Pure-Rust grapheme-aware text editing buffer supporting single-line and multiline editing,
/// preferred-X vertical line navigation, selection ranges, Undo/Redo, and clipboard operations.
#[derive(Clone, Debug)]
pub struct TextEditBuffer {
    text: String,
    multiline: bool,
    cursor: usize,
    selection_anchor: Option<usize>,
    preferred_x: Option<f32>,
    undo_stack: Vec<EditSnapshot>,
    redo_stack: Vec<EditSnapshot>,
}

impl Default for TextEditBuffer {
    fn default() -> Self {
        Self::new_single_line()
    }
}

impl TextEditBuffer {
    /// Creates an empty single-line `TextEditBuffer`.
    pub fn new_single_line() -> Self {
        Self::with_text("", false)
    }

    /// Creates an empty multiline `TextEditBuffer`.
    pub fn new_multiline() -> Self {
        Self::with_text("", true)
    }

    /// Creates a `TextEditBuffer` initialized with `raw` (sanitized for single-line or multiline)
    /// and places the cursor at the end of the text.
    pub fn with_text(raw: &str, multiline: bool) -> Self {
        let text = normalize_editor_text(raw, multiline);
        let cursor = text.chars().count();
        Self {
            text,
            multiline,
            cursor,
            selection_anchor: None,
            preferred_x: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Returns the sanitized UTF-8 buffer contents.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns `true` if this buffer allows `\n` line breaks and vertical line navigation.
    pub fn is_multiline(&self) -> bool {
        self.multiline
    }

    /// Returns `true` if the text buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Returns the number of Unicode scalar values (`char`s) in the buffer.
    pub fn char_len(&self) -> usize {
        self.text.chars().count()
    }

    /// Returns the number of extended grapheme clusters in the buffer.
    pub fn grapheme_len(&self) -> usize {
        self.text.graphemes(true).count()
    }

    /// Returns the current caret `char` index (`0..=char_len()`), always on a grapheme boundary.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Returns the selection anchor `char` index when an active selection exists.
    pub fn selection_anchor(&self) -> Option<usize> {
        self.selection_anchor.filter(|&a| a != self.cursor)
    }

    /// Returns the ordered `start..end` `char` range of the active selection, if non-empty.
    pub fn selection_range(&self) -> Option<Range<usize>> {
        let anchor = self.selection_anchor()?;
        Some(anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    /// Returns the selected UTF-8 substring slice when a non-empty selection is active.
    pub fn selected_text(&self) -> Option<&str> {
        let range = self.selection_range()?;
        let b0 = char_to_byte_idx(&self.text, range.start);
        let b1 = char_to_byte_idx(&self.text, range.end);
        Some(&self.text[b0..b1])
    }

    /// Returns the preserved horizontal pixel column (`preferred_x`) used during vertical movement.
    pub fn preferred_x(&self) -> Option<f32> {
        self.preferred_x
    }

    /// Returns `true` if at least one Undo snapshot is available.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Returns `true` if at least one Redo snapshot is available.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Replaces the entire buffer contents, snapping the cursor to the end and clearing Undo/Redo.
    pub fn set_text(&mut self, raw: &str) {
        self.text = normalize_editor_text(raw, self.multiline);
        self.cursor = self.char_len();
        self.selection_anchor = None;
        self.preferred_x = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Moves the cursor to `char_idx` (snapped to the nearest grapheme boundary) and resets `preferred_x`.
    pub fn set_cursor(&mut self, char_idx: usize, extend_selection: bool) {
        let target = snap_char_to_grapheme_boundary(&self.text, char_idx.min(self.char_len()));
        self.apply_cursor_move(target, extend_selection);
        self.preferred_x = None;
    }

    /// Collapses any active selection without moving the caret.
    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    /// Selects the entire buffer (`0..char_len()`).
    pub fn select_all(&mut self) {
        let len = self.char_len();
        if len > 0 {
            self.selection_anchor = Some(0);
            self.cursor = len;
            self.preferred_x = None;
        }
    }

    /// Moves the cursor left by one extended grapheme cluster (or collapses to selection start).
    pub fn move_left(&mut self, extend_selection: bool) -> bool {
        self.preferred_x = None;
        if !extend_selection {
            if let Some(range) = self.selection_range() {
                self.selection_anchor = None;
                self.cursor = range.start;
                return true;
            }
        }
        let target = prev_grapheme_char_idx(&self.text, self.cursor);
        self.apply_cursor_move(target, extend_selection)
    }

    /// Moves the cursor right by one extended grapheme cluster (or collapses to selection end).
    pub fn move_right(&mut self, extend_selection: bool) -> bool {
        self.preferred_x = None;
        if !extend_selection {
            if let Some(range) = self.selection_range() {
                self.selection_anchor = None;
                self.cursor = range.end;
                return true;
            }
        }
        let target = next_grapheme_char_idx(&self.text, self.cursor);
        self.apply_cursor_move(target, extend_selection)
    }

    /// Moves the cursor to the previous Unicode word boundary.
    pub fn move_word_left(&mut self, extend_selection: bool) -> bool {
        self.preferred_x = None;
        let target = prev_word_char_idx(&self.text, self.cursor);
        self.apply_cursor_move(target, extend_selection)
    }

    /// Moves the cursor to the next Unicode word boundary.
    pub fn move_word_right(&mut self, extend_selection: bool) -> bool {
        self.preferred_x = None;
        let target = next_word_char_idx(&self.text, self.cursor);
        self.apply_cursor_move(target, extend_selection)
    }

    /// Moves the cursor to the start of the current `\n`-delimited line.
    pub fn move_home(&mut self, extend_selection: bool) -> bool {
        self.preferred_x = None;
        let target = line_start_char_idx(&self.text, self.cursor);
        self.apply_cursor_move(target, extend_selection)
    }

    /// Moves the cursor to the end of the current `\n`-delimited line.
    pub fn move_end(&mut self, extend_selection: bool) -> bool {
        self.preferred_x = None;
        let target = line_end_char_idx(&self.text, self.cursor);
        self.apply_cursor_move(target, extend_selection)
    }

    /// Moves the cursor vertically by `delta_lines` across `layout`, preserving the initial
    /// horizontal pixel column (`preferred_x`) when passing through shorter intermediate lines.
    pub fn move_vertical(
        &mut self,
        delta_lines: isize,
        layout: &ShapedTextLayout,
        extend_selection: bool,
    ) -> bool {
        if !self.multiline || delta_lines == 0 || layout.lines.is_empty() {
            return false;
        }
        let cur_line = layout.line_for_char(self.cursor);
        let desired_x = self
            .preferred_x
            .unwrap_or_else(|| layout.cursor_x_for_char(self.cursor));
        self.preferred_x = Some(desired_x);

        let max_line = layout.lines.len() as isize - 1;
        let target_line_idx =
            (cur_line.line_index as isize + delta_lines).clamp(0, max_line) as usize;
        if target_line_idx == cur_line.line_index {
            return false;
        }
        let target_line = &layout.lines[target_line_idx];
        let best_char = closest_grapheme_on_line(&self.text, layout, target_line, desired_x);
        self.apply_cursor_move(best_char, extend_selection)
    }

    /// Inserts `raw` at the cursor (replacing any active selection) and records an Undo snapshot.
    pub fn insert_text(&mut self, raw: &str) -> bool {
        let clean = normalize_editor_text(raw, self.multiline);
        if clean.is_empty() && self.selection_range().is_none() {
            return false;
        }
        self.push_undo_snapshot();
        let _ = self.delete_selection_without_snapshot();
        let byte_idx = char_to_byte_idx(&self.text, self.cursor);
        self.text.insert_str(byte_idx, &clean);
        self.cursor += clean.chars().count();
        self.preferred_x = None;
        true
    }

    /// Deletes the active selection or the extended grapheme cluster immediately before the cursor.
    pub fn backspace(&mut self) -> bool {
        self.preferred_x = None;
        if self.selection_range().is_some() {
            self.push_undo_snapshot();
            return self.delete_selection_without_snapshot();
        }
        if self.cursor == 0 {
            return false;
        }
        self.push_undo_snapshot();
        let prev = prev_grapheme_char_idx(&self.text, self.cursor);
        let b0 = char_to_byte_idx(&self.text, prev);
        let b1 = char_to_byte_idx(&self.text, self.cursor);
        self.text.replace_range(b0..b1, "");
        self.cursor = prev;
        true
    }

    /// Deletes the active selection or the extended grapheme cluster immediately after the cursor.
    pub fn delete_forward(&mut self) -> bool {
        self.preferred_x = None;
        if self.selection_range().is_some() {
            self.push_undo_snapshot();
            return self.delete_selection_without_snapshot();
        }
        if self.cursor >= self.char_len() {
            return false;
        }
        self.push_undo_snapshot();
        let next = next_grapheme_char_idx(&self.text, self.cursor);
        let b0 = char_to_byte_idx(&self.text, self.cursor);
        let b1 = char_to_byte_idx(&self.text, next);
        self.text.replace_range(b0..b1, "");
        true
    }

    /// Restores the most recent Undo snapshot, pushing the current state onto the Redo stack.
    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo_stack.pop() else {
            return false;
        };
        let current = self.current_snapshot();
        push_bounded(&mut self.redo_stack, current);
        self.restore_snapshot(prev);
        true
    }

    /// Re-applies the most recent Redo snapshot, pushing the current state onto the Undo stack.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        let current = self.current_snapshot();
        push_bounded(&mut self.undo_stack, current);
        self.restore_snapshot(next);
        true
    }

    /// Copies the currently selected substring (`Ctrl+C`), returning `None` if nothing is selected.
    pub fn copy_selection(&self) -> Option<String> {
        self.selected_text().map(str::to_owned)
    }

    /// Cuts the currently selected substring (`Ctrl+X`), recording an Undo snapshot.
    pub fn cut_selection(&mut self) -> Option<String> {
        let copied = self.copy_selection()?;
        self.push_undo_snapshot();
        let _ = self.delete_selection_without_snapshot();
        self.preferred_x = None;
        Some(copied)
    }

    /// Pastes `clipboard_text` at the cursor (`Ctrl+V`), normalizing newlines and recording Undo.
    pub fn paste(&mut self, clipboard_text: &str) -> bool {
        self.insert_text(clipboard_text)
    }

    fn current_snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            selection_anchor: self.selection_anchor,
        }
    }

    fn restore_snapshot(&mut self, snap: EditSnapshot) {
        self.text = snap.text;
        self.cursor = snap_char_to_grapheme_boundary(&self.text, snap.cursor.min(self.char_len()));
        self.selection_anchor = snap
            .selection_anchor
            .map(|a| snap_char_to_grapheme_boundary(&self.text, a.min(self.char_len())))
            .filter(|&a| a != self.cursor);
        self.preferred_x = None;
    }

    fn push_undo_snapshot(&mut self) {
        let snap = self.current_snapshot();
        if self
            .undo_stack
            .last()
            .is_some_and(|last| last.text == snap.text)
        {
            return;
        }
        push_bounded(&mut self.undo_stack, snap);
        self.redo_stack.clear();
    }

    fn delete_selection_without_snapshot(&mut self) -> bool {
        let Some(range) = self.selection_range() else {
            self.selection_anchor = None;
            return false;
        };
        let b0 = char_to_byte_idx(&self.text, range.start);
        let b1 = char_to_byte_idx(&self.text, range.end);
        self.text.replace_range(b0..b1, "");
        self.cursor = range.start;
        self.selection_anchor = None;
        true
    }

    fn apply_cursor_move(&mut self, target: usize, extend_selection: bool) -> bool {
        let prev_cursor = self.cursor;
        let prev_anchor = self.selection_anchor;
        if extend_selection {
            let anchor = self.selection_anchor.unwrap_or(self.cursor);
            self.cursor = target;
            self.selection_anchor = (anchor != target).then_some(anchor);
        } else {
            self.cursor = target;
            self.selection_anchor = None;
        }
        self.cursor != prev_cursor || self.selection_anchor != prev_anchor
    }
}

fn push_bounded(stack: &mut Vec<EditSnapshot>, snap: EditSnapshot) {
    if stack.len() >= MAX_EDIT_UNDO_HISTORY {
        stack.remove(0);
    }
    stack.push(snap);
}
