//! Helper utilities (`KruiWinitEvent`, `compute_idle_control_flow`, `key_info_from_winit`,
//! `apply_key_action`, `WindowEventHooks`) for `krui-winit`.

use crate::clipboard::{get_clipboard_text, set_clipboard_text};
use crate::session::EngineSession;
use krui_core::{HoveredCursorKind, KeyAction, KeyInfo, Mods, TouchPhaseKind, ViewTree};
use krui_signals::UiWaker;
use std::time::{Duration, Instant};
use winit::event::{KeyEvent, Modifiers, MouseButton, TouchPhase};
use winit::event_loop::{ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::CursorIcon;

/// Frame-pacing interval (8,333 microseconds = 120 Hz) used while `FrameClock` animations are active.
pub const FRAME_PACE_120HZ: Duration = Duration::from_micros(8_333);

/// Idle duration after keyboard/pointer/IME activity before caret blink redraws suspend (`0%` idle CPU).
pub const CARET_BLINK_IDLE_TIMEOUT: Duration = Duration::from_secs(10);

/// Custom user event envelope dispatched into `EventLoop::<KruiWinitEvent>` from background
/// `UiWaker` threads, live-preview file watchers, and `accesskit_winit::Adapter`.
#[derive(Debug)]
pub enum KruiWinitEvent {
    /// Wakes the UI thread to drain completed background tasks (`poll_async_tasks`) and request redraw.
    WakeUi,
    /// Accessibility activation, action request, or deactivation event forwarded by `accesskit_winit`.
    AccessKit(accesskit_winit::Event),
}

impl From<accesskit_winit::Event> for KruiWinitEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::AccessKit(event)
    }
}

pub(crate) struct WinitProxyUiWaker {
    proxy: EventLoopProxy<KruiWinitEvent>,
}

impl WinitProxyUiWaker {
    pub(crate) fn new(proxy: EventLoopProxy<KruiWinitEvent>) -> Self {
        Self { proxy }
    }
}

pub(crate) fn map_winit_theme(theme: winit::window::Theme) -> krui_style::ColorScheme {
    match theme {
        winit::window::Theme::Dark => krui_style::ColorScheme::Dark,
        winit::window::Theme::Light => krui_style::ColorScheme::Light,
    }
}

impl UiWaker for WinitProxyUiWaker {
    fn wake(&self) {
        let _ = self.proxy.send_event(KruiWinitEvent::WakeUi);
    }
}

/// Computes the optimal `winit::event_loop::ControlFlow` for the current frame.
///
/// - Never returns [`ControlFlow::Poll`] (eliminating 100% CPU busy-wait loops during async tasks).
/// - When `has_active_animations` is `true`, schedules 120 Hz frame-paced redraws via [`ControlFlow::WaitUntil`].
/// - When `is_paint_dirty` is `true`, schedules an immediate one-shot [`ControlFlow::WaitUntil`].
/// - When `has_preview_watcher` is active, wakes every `150ms`; when an `<input>` is focused, wakes every `500ms` for caret blink.
/// - Otherwise (including while background async tasks are in flight!), returns [`ControlFlow::Wait`] for **0% idle CPU**.
pub fn compute_idle_control_flow(
    has_active_animations: bool,
    is_paint_dirty: bool,
    is_input_focused: bool,
    has_preview_watcher: bool,
    now: Instant,
) -> ControlFlow {
    if has_active_animations {
        ControlFlow::WaitUntil(now + FRAME_PACE_120HZ)
    } else if is_paint_dirty {
        ControlFlow::WaitUntil(now)
    } else if has_preview_watcher {
        ControlFlow::WaitUntil(now + Duration::from_millis(150))
    } else if is_input_focused {
        ControlFlow::WaitUntil(now + Duration::from_millis(500))
    } else {
        ControlFlow::Wait
    }
}

/// Computes `ControlFlow` for a focused/unfocused window, stopping 500ms caret blink wakeups
/// when the window is unfocused or `idle_since_input >= CARET_BLINK_IDLE_TIMEOUT` (10 seconds).
pub fn compute_window_idle_control_flow(
    has_active_animations: bool,
    is_paint_dirty: bool,
    is_input_focused: bool,
    is_window_focused: bool,
    idle_since_input: Duration,
    has_preview_watcher: bool,
    now: Instant,
) -> ControlFlow {
    let should_blink =
        is_input_focused && is_window_focused && idle_since_input < CARET_BLINK_IDLE_TIMEOUT;
    compute_idle_control_flow(
        has_active_animations,
        is_paint_dirty,
        should_blink,
        has_preview_watcher,
        now,
    )
}

/// Converts a `winit::event::TouchPhase` to a platform-neutral [`TouchPhaseKind`].
pub fn touch_phase_from_winit(phase: TouchPhase) -> TouchPhaseKind {
    match phase {
        TouchPhase::Started => TouchPhaseKind::Started,
        TouchPhase::Moved => TouchPhaseKind::Moved,
        TouchPhase::Ended => TouchPhaseKind::Ended,
        TouchPhase::Cancelled => TouchPhaseKind::Cancelled,
    }
}

/// Optional application hooks for customizing window keyboard shortcuts, navigation, and window titles.
pub trait WindowEventHooks {
    /// Invoked on event loop ticks; returning `Some(...)` triggers optional preview path reload + redraw.
    fn on_tick(&mut self, _session: &mut EngineSession) -> Option<Option<String>> {
        None
    }

    /// Invoked before standard key routing; returning `Some(...)` consumes the key event.
    fn on_key_down(
        &mut self,
        _session: &mut EngineSession,
        _key_info: &KeyInfo,
        _mods: Mods,
    ) -> Option<Option<String>> {
        None
    }

    /// Invoked on mouse button press/release before DOM click/drag routing.
    /// `button`: `0` = left, `1` = middle, `2` = right. Returning `Some(...)` consumes the event.
    fn on_pointer_button(
        &mut self,
        _session: &mut EngineSession,
        _x: f32,
        _y: f32,
        _button: u8,
        _pressed: bool,
        _mods: Mods,
    ) -> Option<Option<String>> {
        None
    }

    /// Invoked after pointer movement has been routed through the view tree.
    fn on_pointer_moved(
        &mut self,
        _session: &mut EngineSession,
        _x: f32,
        _y: f32,
        _mods: Mods,
    ) -> bool {
        false
    }

    /// Invoked before standard scroll routing; returning `Some(...)` consumes the wheel event.
    fn on_mouse_wheel(
        &mut self,
        _session: &mut EngineSession,
        _x: f32,
        _y: f32,
        _dy: f32,
        _mods: Mods,
    ) -> Option<Option<String>> {
        None
    }

    /// Invoked after rendering each frame; returning `Some(title)` updates the native window title.
    fn on_after_frame(
        &mut self,
        _session: &EngineSession,
        _frame_us: u128,
        _reload_count: u64,
    ) -> Option<String> {
        None
    }
}

pub(crate) struct NoopWindowHooks;
impl WindowEventHooks for NoopWindowHooks {}

pub(crate) fn mouse_button_code(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        _ => None,
    }
}

/// Converts a `winit::keyboard::Key` and optional text slice into a platform-agnostic [`KeyInfo`].
pub fn key_info_from_logical_key(logical_key: &Key, text: Option<&str>) -> KeyInfo {
    let text = text.map(|s| s.to_string());
    match logical_key {
        Key::Named(NamedKey::Escape) => KeyInfo::Escape,
        Key::Named(NamedKey::Tab) => KeyInfo::Tab,
        Key::Named(NamedKey::Backspace) => KeyInfo::Backspace,
        Key::Named(NamedKey::Delete) => KeyInfo::Delete,
        Key::Named(NamedKey::ArrowLeft) => KeyInfo::ArrowLeft,
        Key::Named(NamedKey::ArrowRight) => KeyInfo::ArrowRight,
        Key::Named(NamedKey::ArrowUp) => KeyInfo::ArrowUp,
        Key::Named(NamedKey::ArrowDown) => KeyInfo::ArrowDown,
        Key::Named(NamedKey::Home) => KeyInfo::Home,
        Key::Named(NamedKey::End) => KeyInfo::End,
        Key::Named(NamedKey::PageUp) => KeyInfo::PageUp,
        Key::Named(NamedKey::PageDown) => KeyInfo::PageDown,
        Key::Named(NamedKey::Enter) => KeyInfo::Enter,
        Key::Named(NamedKey::Space) => KeyInfo::Space,
        Key::Character(ch) => KeyInfo::Character {
            logical: ch.to_string(),
            text,
        },
        Key::Named(named) => KeyInfo::Other(format!("{named:?}")),
        Key::Unidentified(_) | Key::Dead(_) => {
            if let Some(raw) = text {
                if !raw.is_empty() && !raw.chars().all(char::is_control) {
                    return KeyInfo::Character {
                        logical: raw.clone(),
                        text: Some(raw),
                    };
                }
            }
            KeyInfo::Other(String::new())
        }
    }
}

/// Converts a `winit::event::KeyEvent` into a platform-agnostic [`KeyInfo`].
pub fn key_info_from_winit(event: &KeyEvent) -> KeyInfo {
    key_info_from_logical_key(&event.logical_key, event.text.as_deref())
}

/// Executes a [`KeyAction`] produced by `route_key` against `tree` (and the system clipboard).
pub fn apply_key_action(tree: &mut ViewTree, action: KeyAction) -> bool {
    match action {
        KeyAction::None => false,
        KeyAction::ClearFocus => tree.clear_focus(),
        KeyAction::FocusTab { shift } => tree.dispatch_key_tab(shift).is_some(),
        KeyAction::Backspace => tree.dispatch_backspace(),
        KeyAction::Delete => tree.dispatch_delete(),
        KeyAction::DeleteWordBackward => tree.dispatch_delete_word_backward(),
        KeyAction::DeleteWordForward => tree.dispatch_delete_word_forward(),
        KeyAction::CursorLeft => tree.dispatch_cursor_left(),
        KeyAction::CursorRight => tree.dispatch_cursor_right(),
        KeyAction::CursorUp => tree.dispatch_cursor_up(),
        KeyAction::CursorDown => tree.dispatch_cursor_down(),
        KeyAction::CursorHome => tree.dispatch_cursor_home(),
        KeyAction::CursorEnd => tree.dispatch_cursor_end(),
        KeyAction::SelectLeft => tree.dispatch_select_left(),
        KeyAction::SelectRight => tree.dispatch_select_right(),
        KeyAction::SelectUp => tree.dispatch_select_up(),
        KeyAction::SelectDown => tree.dispatch_select_down(),
        KeyAction::SelectHome => tree.dispatch_select_home(),
        KeyAction::SelectEnd => tree.dispatch_select_end(),
        KeyAction::WordLeft => tree.dispatch_word_left(),
        KeyAction::WordRight => tree.dispatch_word_right(),
        KeyAction::SelectWordLeft => tree.dispatch_select_word_left(),
        KeyAction::SelectWordRight => tree.dispatch_select_word_right(),
        KeyAction::Enter => tree.dispatch_key_enter(),
        KeyAction::Space => tree.dispatch_key_space(),
        KeyAction::InsertText(txt) => tree.dispatch_text_input(&txt),
        KeyAction::SelectAll => tree.select_all(),
        KeyAction::Copy => tree.selected_text().is_some_and(|txt| {
            set_clipboard_text(&txt);
            true
        }),
        KeyAction::Cut => tree.cut_selected_text().is_some_and(|txt| {
            set_clipboard_text(&txt);
            true
        }),
        KeyAction::Paste => {
            let clip = get_clipboard_text();
            !clip.is_empty() && tree.dispatch_text_input(&clip)
        }
        KeyAction::Undo => tree.dispatch_undo(),
        KeyAction::Redo => tree.dispatch_redo(),
        KeyAction::PageUp => tree.dispatch_page_up(),
        KeyAction::PageDown => tree.dispatch_page_down(),
        KeyAction::ScrollUp => tree.dispatch_scroll_up(),
        KeyAction::ScrollDown => tree.dispatch_scroll_down(),
    }
}

pub(crate) fn run_headless_window_session(title: &str, session: &mut EngineSession) {
    let first = session.step_frame(1.0 / 120.0);
    let tree = &mut session.tree;
    session.reactive_session.enter(|| {
        let _ = tree.dispatch_key_tab(false);
        let _ = tree.dispatch_key_activate();
        let _ = tree.dispatch_key_tab(false);
        let _ = tree.dispatch_text_input("A4");
    });
    for _ in 0..120 {
        let _ = session.step_frame(1.0 / 120.0);
    }
    println!(
        "[{title}] Headless 120-frame OK: nodes={}, relayouts={}, first_frame_us={}us",
        session.tree.node_count(),
        first.total_relayout_passes,
        first.total_frame_us
    );
}

pub(crate) fn cursor_icon_from_kind(kind: HoveredCursorKind) -> CursorIcon {
    match kind {
        HoveredCursorKind::Text => CursorIcon::Text,
        HoveredCursorKind::Pointer => CursorIcon::Pointer,
        HoveredCursorKind::Default => CursorIcon::Default,
    }
}

pub(crate) fn mods_from_winit(mods: &Modifiers) -> Mods {
    let s = mods.state();
    Mods {
        ctrl: s.control_key(),
        alt: s.alt_key(),
        shift: s.shift_key(),
        super_key: s.super_key(),
    }
}
