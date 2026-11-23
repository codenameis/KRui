//! Platform-neutral Android `TextInputHost`, UTF-16 bridge math, and InputConnection event mappers.

use krui_core::{
    utf16_to_utf8_offset, utf8_to_utf16_range, EditingState, EditorActionKind, InputKind,
    LayoutRect, TextInputHost, TouchPhaseKind,
};

/// Safe Rust-owned state mirror implementing [`TextInputHost`] for Android `InputMethodManager` / `EditorInfo`.
#[derive(Clone, Debug, Default)]
pub struct AndroidTextInputHost {
    /// Whether predictive IME composition is enabled (`false` for password fields).
    pub ime_allowed: bool,
    /// Whether an editable `<input>` is currently focused and requests the soft keyboard.
    pub keyboard_visible: bool,
    /// Input semantic kind (`Text`, `Password`, `Number`, `Email`, `Search`).
    pub input_kind: InputKind,
    /// Logical caret / preedit rectangle for IME window alignment.
    pub caret_rect: LayoutRect,
    /// Latest UTF-8 text, selection, and composing range snapshot.
    pub editing_state: EditingState,
    /// Stable node identity key (`A11yId`) of the currently focused input, if any.
    pub focused_node_key: Option<u64>,
}

impl TextInputHost for AndroidTextInputHost {
    fn set_ime_allowed(&mut self, allowed: bool) {
        self.ime_allowed = allowed;
    }

    fn set_ime_cursor_area(&mut self, rect: LayoutRect) {
        self.caret_rect = rect;
    }

    fn update_editing_state(&mut self, state: &EditingState) {
        self.editing_state = state.clone();
    }

    fn show_keyboard(&mut self, kind: InputKind) {
        self.ime_allowed = kind != InputKind::Password;
        self.keyboard_visible = true;
        self.input_kind = kind;
    }

    fn hide_keyboard(&mut self) {
        self.ime_allowed = false;
        self.keyboard_visible = false;
    }

    fn set_focused_node_identity(&mut self, node_key: Option<u64>) {
        self.focused_node_key = node_key;
    }
}

/// Maps KRui's [`InputKind`] to `KruiBridge.INPUT_KIND_*` integer constants consumed by `KruiView.kt`.
pub const fn input_kind_to_android_code(kind: InputKind) -> i32 {
    match kind {
        InputKind::Text => 0,
        InputKind::Password => 1,
        InputKind::Number => 2,
        InputKind::Email => 3,
        InputKind::Search => 5,
    }
}

/// Maps Android `EditorInfo.IME_ACTION_*` constants to KRui's [`EditorActionKind`].
pub const fn editor_action_from_android_code(action_code: i32) -> EditorActionKind {
    match action_code {
        2 => EditorActionKind::Go,
        3 => EditorActionKind::Search,
        4 => EditorActionKind::Send,
        5 => EditorActionKind::Next,
        7 => EditorActionKind::Previous,
        _ => EditorActionKind::Done,
    }
}

/// Maps Android `MotionEvent.getActionMasked()` integer codes to [`TouchPhaseKind`].
pub const fn touch_phase_from_android_action(action_masked: i32) -> Option<TouchPhaseKind> {
    match action_masked {
        0 | 5 => Some(TouchPhaseKind::Started),
        2 => Some(TouchPhaseKind::Moved),
        1 | 6 => Some(TouchPhaseKind::Ended),
        3 => Some(TouchPhaseKind::Cancelled),
        _ => None,
    }
}

/// Computes the UTF-8 byte cursor range within `text` from Android's `newCursorPosition`.
pub fn composing_cursor_bytes(text: &str, new_cursor_position: i32) -> Option<(usize, usize)> {
    if text.is_empty() {
        return None;
    }
    if new_cursor_position > 0 {
        Some((text.len(), text.len()))
    } else {
        Some((0, 0))
    }
}

/// Converts Android `InputConnection.deleteSurroundingText(beforeLength, afterLength)` UTF-16 code-unit
/// counts around `state.selection` into exact UTF-8 byte counts `(before_bytes, after_bytes)`.
pub fn compute_utf8_delete_surrounding(
    state: &EditingState,
    before_u16: i32,
    after_u16: i32,
) -> (usize, usize) {
    let sel_u8 = state.selection.clone();
    let sel_u16 = utf8_to_utf16_range(&state.text, sel_u8.clone());
    let b0_u16 = sel_u16.start.saturating_sub(before_u16.max(0) as usize);
    let a1_u16 = sel_u16.end.saturating_add(after_u16.max(0) as usize);
    let b0_u8 = utf16_to_utf8_offset(&state.text, b0_u16);
    let a1_u8 = utf16_to_utf8_offset(&state.text, a1_u16);
    (
        sel_u8.start.saturating_sub(b0_u8),
        a1_u8.saturating_sub(sel_u8.end),
    )
}
