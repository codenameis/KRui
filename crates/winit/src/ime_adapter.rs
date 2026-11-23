//! `TextInputHost` adapter, platform sink abstraction, and event conversion for `winit::window::Window`.

use krui_core::{EditingState, ImeEvent, InputKind, LayoutRect, TextInputHost, ViewTree};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::Ime;
use winit::window::{ImePurpose, Window};

/// Cached OS IME state for a platform window so redundant OS IME calls are skipped and direct
/// `<input>` A -> `<input>` B transitions reset the OS IME session cleanly.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WinitImeStateCache {
    pub allowed: Option<bool>,
    pub kind: Option<InputKind>,
    pub cursor_area: Option<LayoutRect>,
    pub active_node: Option<u64>,
    pub reset_count: u64,
}

impl WinitImeStateCache {
    /// Clears the cached cursor rectangle (e.g. on `WindowEvent::ScaleFactorChanged` or device-frame switch)
    /// so the next sync re-submits physical coordinates at the updated DPI.
    pub fn invalidate_cursor_area(&mut self) {
        self.cursor_area = None;
    }
}

/// Recorded OS IME command emitted by [`WinitTextInputHost`] when backed by [`RecordedImeSink`].
#[derive(Clone, Debug, PartialEq)]
pub enum RecordedImeOp {
    SetAllowed(bool),
    SetPurpose(InputKind),
    SetCursorArea(LayoutRect),
}

/// Minimal OS IME sink implemented for `&winit::window::Window` and headless test recorders.
pub trait ImePlatformSink {
    fn set_ime_allowed(&mut self, allowed: bool);
    fn set_ime_purpose(&mut self, kind: InputKind);
    fn set_ime_cursor_area(&mut self, rect: LayoutRect);
}

/// Converts a logical viewport IME cursor rectangle into physical pixel `(x, y, width, height)` for `scale_factor`.
pub fn logical_to_physical_ime_area(rect: LayoutRect, scale_factor: f64) -> (i32, i32, u32, u32) {
    let sf = scale_factor.max(1.0);
    let px = (rect.x as f64 * sf).round() as i32;
    let py = (rect.y as f64 * sf).round() as i32;
    let pw = ((rect.width.max(1.0) as f64) * sf).round().max(1.0) as u32;
    let ph = ((rect.height.max(1.0) as f64) * sf).round().max(1.0) as u32;
    (px, py, pw, ph)
}

impl ImePlatformSink for &Window {
    fn set_ime_allowed(&mut self, allowed: bool) {
        Window::set_ime_allowed(self, allowed);
    }

    fn set_ime_purpose(&mut self, kind: InputKind) {
        Window::set_ime_purpose(
            self,
            match kind {
                InputKind::Password => ImePurpose::Password,
                _ => ImePurpose::Normal,
            },
        );
    }

    fn set_ime_cursor_area(&mut self, rect: LayoutRect) {
        let (px, py, pw, ph) = logical_to_physical_ime_area(rect, self.scale_factor());
        Window::set_ime_cursor_area(
            self,
            PhysicalPosition::new(px, py),
            PhysicalSize::new(pw, ph),
        );
    }
}

/// Headless [`ImePlatformSink`] recording exact OS IME calls for deterministic testing.
#[derive(Clone, Debug, Default)]
pub struct RecordedImeSink {
    pub ops: Vec<RecordedImeOp>,
}

impl ImePlatformSink for &mut RecordedImeSink {
    fn set_ime_allowed(&mut self, allowed: bool) {
        self.ops.push(RecordedImeOp::SetAllowed(allowed));
    }

    fn set_ime_purpose(&mut self, kind: InputKind) {
        self.ops.push(RecordedImeOp::SetPurpose(kind));
    }

    fn set_ime_cursor_area(&mut self, rect: LayoutRect) {
        self.ops.push(RecordedImeOp::SetCursorArea(rect));
    }
}

/// Adapter implementing [`TextInputHost`] for a desktop [`winit::window::Window`] or test [`ImePlatformSink`].
pub struct WinitTextInputHost<'a, S: ImePlatformSink = &'a Window> {
    sink: S,
    last_state: Option<EditingState>,
    cache: Option<&'a mut WinitImeStateCache>,
    content_offset: (f32, f32),
    suppress_cursor_for_password: bool,
}

impl<'a> WinitTextInputHost<'a, &'a Window> {
    /// Wraps a reference to a [`winit::window::Window`] as a [`TextInputHost`].
    pub fn new(window: &'a Window) -> Self {
        Self::with_sink(window, None)
    }

    /// Wraps a reference to a [`winit::window::Window`] with a [`WinitImeStateCache`] to skip redundant OS calls.
    pub fn with_cache(window: &'a Window, cache: &'a mut WinitImeStateCache) -> Self {
        Self::with_sink(window, Some(cache))
    }
}

impl<'a, S: ImePlatformSink> WinitTextInputHost<'a, S> {
    /// Wraps an arbitrary [`ImePlatformSink`] with an optional [`WinitImeStateCache`].
    pub fn with_sink(sink: S, cache: Option<&'a mut WinitImeStateCache>) -> Self {
        Self {
            sink,
            last_state: None,
            cache,
            content_offset: (0.0, 0.0),
            suppress_cursor_for_password: false,
        }
    }

    /// Translates `ViewTree` logical caret coordinates by `offset` (e.g. `DeviceFramePreset::content_origin()`).
    pub fn with_content_offset(mut self, offset: (f32, f32)) -> Self {
        self.content_offset = offset;
        self
    }

    /// Returns the most recent [`EditingState`] pushed by [`ViewTree::sync_text_input_host`], if any.
    pub fn last_editing_state(&self) -> Option<&EditingState> {
        self.last_state.as_ref()
    }
}

impl<S: ImePlatformSink> TextInputHost for WinitTextInputHost<'_, S> {
    fn set_focused_node_identity(&mut self, node_key: Option<u64>) {
        if let Some(cache) = &mut self.cache {
            if let (Some(prev), Some(next)) = (cache.active_node, node_key) {
                if prev != next && cache.allowed == Some(true) {
                    self.sink.set_ime_allowed(false);
                    cache.allowed = None;
                    cache.kind = None;
                    cache.cursor_area = None;
                    cache.reset_count += 1;
                }
            }
            cache.active_node = node_key;
        }
    }

    fn set_ime_allowed(&mut self, allowed: bool) {
        if let Some(cache) = &mut self.cache {
            if cache.allowed == Some(allowed) {
                return;
            }
            cache.allowed = Some(allowed);
            if !allowed {
                cache.kind = None;
                cache.cursor_area = None;
                cache.active_node = None;
            }
        }
        self.sink.set_ime_allowed(allowed);
    }

    fn set_ime_cursor_area(&mut self, rect: LayoutRect) {
        if self.suppress_cursor_for_password {
            return;
        }
        let translated = LayoutRect {
            x: rect.x + self.content_offset.0,
            y: rect.y + self.content_offset.1,
            width: rect.width,
            height: rect.height,
        };
        if let Some(cache) = &mut self.cache {
            if cache.cursor_area == Some(translated) {
                return;
            }
            cache.cursor_area = Some(translated);
        }
        self.sink.set_ime_cursor_area(translated);
    }

    fn update_editing_state(&mut self, state: &EditingState) {
        self.last_state = Some(state.clone());
    }

    fn show_keyboard(&mut self, kind: InputKind) {
        if kind == InputKind::Password {
            self.suppress_cursor_for_password = true;
            self.set_ime_allowed(false);
            return;
        }
        self.suppress_cursor_for_password = false;
        self.set_ime_allowed(true);
        if let Some(cache) = &mut self.cache {
            if cache.kind == Some(kind) {
                return;
            }
            cache.kind = Some(kind);
        }
        self.sink.set_ime_purpose(kind);
    }

    fn hide_keyboard(&mut self) {
        self.suppress_cursor_for_password = false;
        self.set_ime_allowed(false);
    }
}

/// Converts a `winit::event::Ime` into a platform-neutral [`ImeEvent`].
pub fn ime_event_from_winit(ime: Ime) -> ImeEvent {
    match ime {
        Ime::Enabled => ImeEvent::Enabled,
        Ime::Preedit(text, cursor_offset) => ImeEvent::Preedit {
            text,
            cursor_offset,
        },
        Ime::Commit(text) => ImeEvent::Commit(text),
        Ime::Disabled => ImeEvent::Disabled,
    }
}

/// Synchronizes the OS IME allowed state, candidate popup position, and [`EditingState`] with the focused `<input>`.
pub fn sync_window_ime_cursor_area(win: &Window, tree: &ViewTree) {
    let mut host = WinitTextInputHost::new(win);
    tree.sync_text_input_host(&mut host);
}

/// Synchronizes the OS IME state using `cache` so unchanged IME cursor/allowed state does not spam the OS window.
pub fn sync_window_ime_cursor_area_cached(
    win: &Window,
    tree: &ViewTree,
    cache: &mut WinitImeStateCache,
) {
    sync_window_ime_cursor_area_cached_with_offset(win, tree, cache, (0.0, 0.0));
}

/// Synchronizes the OS IME state with an outer-window logical content offset (`DeviceFramePreset::content_origin()`).
pub fn sync_window_ime_cursor_area_cached_with_offset(
    win: &Window,
    tree: &ViewTree,
    cache: &mut WinitImeStateCache,
    content_offset: (f32, f32),
) {
    let mut host = WinitTextInputHost::with_cache(win, cache).with_content_offset(content_offset);
    tree.sync_text_input_host(&mut host);
}

/// Synchronizes a [`ViewTree`]'s IME host state into an arbitrary [`ImePlatformSink`] (including test recorders).
pub fn sync_ime_host_with_sink<S: ImePlatformSink>(
    sink: S,
    tree: &ViewTree,
    cache: &mut WinitImeStateCache,
) {
    sync_ime_host_with_sink_and_offset(sink, tree, cache, (0.0, 0.0));
}

/// Synchronizes a [`ViewTree`]'s IME host state into `sink` with an explicit outer-window `content_offset`.
pub fn sync_ime_host_with_sink_and_offset<S: ImePlatformSink>(
    sink: S,
    tree: &ViewTree,
    cache: &mut WinitImeStateCache,
    content_offset: (f32, f32),
) {
    let mut host =
        WinitTextInputHost::with_sink(sink, Some(cache)).with_content_offset(content_offset);
    tree.sync_text_input_host(&mut host);
}
