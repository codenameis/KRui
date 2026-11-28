//! Pure, platform-agnostic key router (`route_key`, `KeyInfo`, `Mods`, `FocusState`, `KeyAction`)
//! and unified `<input>` caret rectangle geometry (`caret_rect`).

use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_style::CompiledStyle;
use krui_text::ShapedTextLayout;
pub use krui_ui_types::events::{
    clear_prevent_default_key, prevent_default_key, take_prevent_default_key, KeydownOutcome, Mods,
};

/// Minimum height (logical px) for an input caret rectangle when container space allows.
pub const MIN_CARET_HEIGHT: f32 = 12.0;
/// Right-edge safety margin (logical px) so the caret remains visible inside the input border.
pub const CARET_RIGHT_MARGIN: f32 = 5.0;

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_editable_line_caret_rect(
    world_rect: LayoutRect,
    style: &CompiledStyle,
    effective_border_width: f32,
    role: SemanticRole,
    shaped_opt: Option<&ShapedTextLayout>,
    active_cursor_char: usize,
    cursor_x: f32,
    scroll_y: f32,
    empty_line_height: f32,
    caret_width: f32,
) -> LayoutRect {
    let cb = crate::view_tree::content_box::content_box_rect(world_rect, &style.layout);
    let clip_box = crate::view_tree::content_box::text_clip_box_rect(
        world_rect,
        &style.layout,
        effective_border_width,
    );
    let edges = style.layout.effective_border_edges();
    let br = edges.right.max(effective_border_width).max(0.0);
    let min_x = cb.x;
    let right_reserve =
        crate::view_tree::content_box::CARET_RIGHT_RESERVE_PX.min(br + CARET_RIGHT_MARGIN);
    let max_x = (world_rect.x + world_rect.width - br - right_reserve).max(min_x);
    let x = (min_x + cursor_x.max(0.0)).clamp(min_x, max_x);

    let min_h = MIN_CARET_HEIGHT.min(clip_box.height.max(1.0));
    let (raw_y, raw_h) = if let Some(shaped) = shaped_opt.filter(|s| !s.lines.is_empty()) {
        let origin_y = if role.is_single_line_input() {
            cb.y + (cb.height - shaped.measured_height) * 0.5
        } else {
            cb.y - scroll_y
        };
        let ln = shaped.line_for_char(active_cursor_char);
        let h = ln.line_height.max(min_h);
        let mid = origin_y + ln.line_top + ln.line_height * 0.5;
        (mid - h * 0.5, h)
    } else {
        let h = empty_line_height.max(min_h);
        let y = if role.is_single_line_input() {
            cb.y + (cb.height - h) * 0.5
        } else {
            cb.y - scroll_y
        };
        (y, h)
    };
    let (y, height) = clamp_vertical_span(raw_y, raw_h, clip_box);
    LayoutRect {
        x,
        y,
        width: caret_width.max(1.0),
        height,
    }
}

pub(crate) fn clamp_vertical_span(raw_y: f32, raw_h: f32, clip: LayoutRect) -> (f32, f32) {
    if clip.height <= 0.0 {
        return (raw_y, raw_h.max(1.0));
    }
    let clip_bottom = clip.y + clip.height;
    let h = raw_h.clamp(1.0_f32.min(clip.height), clip.height);
    let y = raw_y.clamp(clip.y, (clip_bottom - h).max(clip.y));
    (y, h)
}

/// Computes the local-space caret rectangle (`LayoutRect`) for an `<input>` node given its
/// `world_rect`, `style`, `cursor_x` offset (relative to `content_box_rect.x`), and `caret_width`.
pub fn caret_rect(
    world_rect: LayoutRect,
    style: &CompiledStyle,
    cursor_x: f32,
    caret_width: f32,
) -> LayoutRect {
    let fs = style.text.font_size.max(8.0);
    let fallback_lh = style.text.line_height.resolve_px(fs, fs * 1.2);
    resolve_editable_line_caret_rect(
        world_rect,
        style,
        style.paint.border_width,
        SemanticRole::TextInput,
        None,
        0,
        cursor_x,
        0.0,
        fallback_lh,
        caret_width,
    )
}

/// Logical key identity passed to [`route_key`] for pure, platform-agnostic key routing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyInfo {
    Escape,
    Tab,
    Backspace,
    Delete,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Space,
    Character {
        logical: String,
        text: Option<String>,
    },
    Other(String),
}

/// Focus and IME snapshot passed to [`route_key`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FocusState {
    pub is_input_focused: bool,
    pub has_ime_preedit: bool,
    pub is_ime_enabled: bool,
}

/// Pure action decision returned by [`route_key`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyAction {
    /// Ignored (e.g. while IME composition is active or unhandled key).
    None,
    /// Clears focus and text selection (`Escape` — never exits the application).
    ClearFocus,
    /// Cycles keyboard focus (`Tab` / `Shift+Tab`).
    FocusTab { shift: bool },
    /// Deletes the selection or grapheme before the cursor (`Backspace`).
    Backspace,
    /// Deletes the selection or grapheme after the cursor (`Delete`).
    Delete,
    /// Deletes the selection or word before the cursor (`Ctrl+Backspace`).
    DeleteWordBackward,
    /// Deletes the selection or word after the cursor (`Ctrl+Delete`).
    DeleteWordForward,
    /// Moves cursor left by one grapheme (`ArrowLeft`).
    CursorLeft,
    /// Moves cursor right by one grapheme (`ArrowRight`).
    CursorRight,
    /// Moves cursor up or to start of single-line input (`ArrowUp`).
    CursorUp,
    /// Moves cursor down or to end of single-line input (`ArrowDown`).
    CursorDown,
    /// Moves cursor to the start of the input (`Home`).
    CursorHome,
    /// Moves cursor to the end of the input (`End`).
    CursorEnd,
    /// Extends selection left by one grapheme (`Shift+ArrowLeft`).
    SelectLeft,
    /// Extends selection right by one grapheme (`Shift+ArrowRight`).
    SelectRight,
    /// Extends selection up or to start of single-line input (`Shift+ArrowUp`).
    SelectUp,
    /// Extends selection down or to end of single-line input (`Shift+ArrowDown`).
    SelectDown,
    /// Extends selection to the start of the input (`Shift+Home`).
    SelectHome,
    /// Extends selection to the end of the input (`Shift+End`).
    SelectEnd,
    /// Moves cursor left by one word (`Ctrl+ArrowLeft`).
    WordLeft,
    /// Moves cursor right by one word (`Ctrl+ArrowRight`).
    WordRight,
    /// Extends selection left by one word (`Ctrl+Shift+ArrowLeft`).
    SelectWordLeft,
    /// Extends selection right by one word (`Ctrl+Shift+ArrowRight`).
    SelectWordRight,
    /// Confirms `<input>` change or activates focused button (`Enter`).
    Enter,
    /// Inserts space in `<input>` (when IME disabled) or activates focused button (`Space`).
    Space,
    /// Inserts direct character text (`KeyEvent.text`) when IME is disabled or via `AltGr`.
    InsertText(String),
    /// Selects all text (`Ctrl+A`).
    SelectAll,
    /// Copies selected text to clipboard (`Ctrl+C`).
    Copy,
    /// Cuts selected text to clipboard (`Ctrl+X`).
    Cut,
    /// Pastes clipboard text into focused input (`Ctrl+V`).
    Paste,
    /// Undoes the last text edit transaction (`Ctrl+Z`).
    Undo,
    /// Redoes the last undone text edit transaction (`Ctrl+Y` or `Ctrl+Shift+Z`).
    Redo,
    /// Scrolls the active scroll container up by one page (`PageUp`).
    PageUp,
    /// Scrolls the active scroll container down by one page (`PageDown`).
    PageDown,
    /// Scrolls the active scroll container up by one line (`ArrowUp` outside inputs).
    ScrollUp,
    /// Scrolls the active scroll container down by one line (`ArrowDown` outside inputs).
    ScrollDown,
}

/// Pure, platform-agnostic key router that maps `(event, mods, state)` to a [`KeyAction`].
///
/// Enforces KRui's keyboard invariants:
/// - While `state.has_ime_preedit` is `true`, keys are owned by the OS IME (`KeyAction::None`).
/// - `AltGr` (`Ctrl + Alt`) is treated as character input rather than a `Ctrl` shortcut.
/// - `Escape` only clears focus/selection (`KeyAction::ClearFocus`) and **never** exits the app.
/// - Number keys `1..=4` outside inputs do **not** implicitly switch themes in the framework.
/// - When `state.is_ime_enabled` is `true`, character insertion waits for `Ime::Commit` (ignoring `KeyEvent.text`),
///   except for `AltGr` character combinations which bypass IME on Windows.
pub fn route_key(event: &KeyInfo, mods: Mods, state: &FocusState) -> KeyAction {
    route_key_with_repeat(event, mods, state, false)
}

/// Pure, platform-agnostic key router that maps `(event, mods, state, repeat)` to a [`KeyAction`].
///
/// Suppresses repeated `Enter` and non-input `Space` activations (`repeat == true`) so holding
/// `Space` or `Enter` on a `<button>` activates its command only once.
pub fn route_key_with_repeat(
    event: &KeyInfo,
    mods: Mods,
    state: &FocusState,
    repeat: bool,
) -> KeyAction {
    if state.has_ime_preedit {
        return KeyAction::None;
    }
    if mods.is_shortcut_ctrl() {
        return route_ctrl_shortcut(event, mods, state);
    }
    match event {
        KeyInfo::Escape => KeyAction::ClearFocus,
        KeyInfo::Tab => KeyAction::FocusTab { shift: mods.shift },
        KeyInfo::Backspace if state.is_input_focused => KeyAction::Backspace,
        KeyInfo::Delete if state.is_input_focused => KeyAction::Delete,
        KeyInfo::ArrowLeft
        | KeyInfo::ArrowRight
        | KeyInfo::ArrowUp
        | KeyInfo::ArrowDown
        | KeyInfo::Home
        | KeyInfo::End
            if state.is_input_focused =>
        {
            route_input_navigation_key(event, mods.shift)
        }
        KeyInfo::PageUp => KeyAction::PageUp,
        KeyInfo::PageDown => KeyAction::PageDown,
        KeyInfo::ArrowUp if !state.is_input_focused && !mods.alt => KeyAction::ScrollUp,
        KeyInfo::ArrowDown if !state.is_input_focused && !mods.alt => KeyAction::ScrollDown,
        KeyInfo::Enter if !repeat => KeyAction::Enter,
        KeyInfo::Space
            if !(state.is_input_focused && state.is_ime_enabled)
                && !(!state.is_input_focused && repeat) =>
        {
            KeyAction::Space
        }
        KeyInfo::Character { logical, text } if state.is_input_focused => {
            if state.is_ime_enabled && !mods.is_altgr() {
                return KeyAction::None;
            }
            let c = text.as_deref().unwrap_or(logical.as_str());
            if !c.is_empty() && !c.chars().any(|ch| ch.is_control()) {
                KeyAction::InsertText(c.to_string())
            } else {
                KeyAction::None
            }
        }
        _ => KeyAction::None,
    }
}

/// Returns the canonical key name string passed to `onkeydown` handlers for `event`.
pub fn key_name_for_keydown(event: &KeyInfo) -> String {
    match event {
        KeyInfo::Escape => "Escape".to_string(),
        KeyInfo::Tab => "Tab".to_string(),
        KeyInfo::Backspace => "Backspace".to_string(),
        KeyInfo::Delete => "Delete".to_string(),
        KeyInfo::ArrowLeft => "ArrowLeft".to_string(),
        KeyInfo::ArrowRight => "ArrowRight".to_string(),
        KeyInfo::ArrowUp => "ArrowUp".to_string(),
        KeyInfo::ArrowDown => "ArrowDown".to_string(),
        KeyInfo::Home => "Home".to_string(),
        KeyInfo::End => "End".to_string(),
        KeyInfo::PageUp => "PageUp".to_string(),
        KeyInfo::PageDown => "PageDown".to_string(),
        KeyInfo::Enter => "Enter".to_string(),
        KeyInfo::Space => "Space".to_string(),
        KeyInfo::Character { logical, .. } => logical.clone(),
        KeyInfo::Other(name) => name.clone(),
    }
}

fn route_ctrl_shortcut(event: &KeyInfo, mods: Mods, state: &FocusState) -> KeyAction {
    if let KeyInfo::Character { logical, .. } = event {
        return match logical.to_ascii_lowercase().as_str() {
            "a" => KeyAction::SelectAll,
            "c" => KeyAction::Copy,
            "x" => KeyAction::Cut,
            "v" => KeyAction::Paste,
            "z" if state.is_input_focused => {
                if mods.shift {
                    KeyAction::Redo
                } else {
                    KeyAction::Undo
                }
            }
            "y" if state.is_input_focused && !mods.shift => KeyAction::Redo,
            _ => KeyAction::None,
        };
    }
    if !state.is_input_focused {
        return KeyAction::None;
    }
    match event {
        KeyInfo::Backspace => KeyAction::DeleteWordBackward,
        KeyInfo::Delete => KeyAction::DeleteWordForward,
        KeyInfo::ArrowLeft => {
            if mods.shift {
                KeyAction::SelectWordLeft
            } else {
                KeyAction::WordLeft
            }
        }
        KeyInfo::ArrowRight => {
            if mods.shift {
                KeyAction::SelectWordRight
            } else {
                KeyAction::WordRight
            }
        }
        KeyInfo::ArrowUp | KeyInfo::Home => {
            if mods.shift {
                KeyAction::SelectHome
            } else {
                KeyAction::CursorHome
            }
        }
        KeyInfo::ArrowDown | KeyInfo::End => {
            if mods.shift {
                KeyAction::SelectEnd
            } else {
                KeyAction::CursorEnd
            }
        }
        _ => KeyAction::None,
    }
}

fn route_input_navigation_key(event: &KeyInfo, shift: bool) -> KeyAction {
    match (event, shift) {
        (KeyInfo::ArrowLeft, false) => KeyAction::CursorLeft,
        (KeyInfo::ArrowLeft, true) => KeyAction::SelectLeft,
        (KeyInfo::ArrowRight, false) => KeyAction::CursorRight,
        (KeyInfo::ArrowRight, true) => KeyAction::SelectRight,
        (KeyInfo::ArrowUp, false) => KeyAction::CursorUp,
        (KeyInfo::ArrowUp, true) => KeyAction::SelectUp,
        (KeyInfo::ArrowDown, false) => KeyAction::CursorDown,
        (KeyInfo::ArrowDown, true) => KeyAction::SelectDown,
        (KeyInfo::Home, false) => KeyAction::CursorHome,
        (KeyInfo::Home, true) => KeyAction::SelectHome,
        (KeyInfo::End, false) => KeyAction::CursorEnd,
        (KeyInfo::End, true) => KeyAction::SelectEnd,
        _ => KeyAction::None,
    }
}
