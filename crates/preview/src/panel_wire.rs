//! Embedded IDE panel input events and streamed `krui-renderer` framebuffer state payloads.

use crate::device_frame::PreviewOverlayFlags;
use serde::{Deserialize, Serialize};

/// Environment variable (`"1"`) set by `krui serve --panel` to run the target app headlessly and
/// stream real `krui-renderer` frames back into the IDE side panel instead of opening an OS window.
pub const KRUI_EMBED_PANEL_ENV: &str = "KRUI_EMBED_PANEL";

/// Interactive input or host control event forwarded from an embedded IDE preview panel to the live app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PanelInputEvent {
    /// Forces the running app to emit a fresh `PanelFrameState` + `KRFR` framebuffer.
    RequestRefresh,
    /// Resizes the target logical viewport (ignored for layout while a `DeviceFramePreset` is active).
    Resize { width: u16, height: u16 },
    /// Dispatches a pointer-down event in outer canvas logical coordinates.
    PointerDown {
        x: f32,
        y: f32,
        button: u8,
        shift: bool,
        ctrl: bool,
        alt: bool,
        time_ms: u64,
    },
    /// Dispatches a pointer-move (hover / drag) event in outer canvas logical coordinates.
    PointerMove { x: f32, y: f32 },
    /// Dispatches a pointer-up / drag-end event.
    PointerUp { x: f32, y: f32 },
    /// Clears pointer hover state when leaving the canvas.
    PointerLeave,
    /// Dispatches a mouse-wheel scroll delta (`delta_y > 0` = DOM scroll down).
    Wheel { x: f32, y: f32, delta_y: f32 },
    /// Dispatches a non-text key press or accelerator shortcut.
    KeyDown {
        key: String,
        shift: bool,
        ctrl: bool,
        alt: bool,
        repeat: bool,
    },
    /// Updates active OS IME preedit text in `<input>`/`<textarea>`.
    ImePreedit {
        text: String,
        cursor_start: usize,
        cursor_end: usize,
    },
    /// Commits composed or direct printable text into the focused field.
    ImeCommit { text: String },
    /// Updates pure-CSS `ColorSchemePreference` (`System`, `Light`, or `Dark`).
    SetColorScheme(krui_style::ColorSchemePreference),
}

/// Real `krui-renderer` framebuffer and engine state streamed from the live app back to `krui serve --panel`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanelFrameState {
    /// Monotonic frame sequence number.
    pub seq: u32,
    /// Rendered physical pixel width.
    pub width: u16,
    /// Rendered physical pixel height.
    pub height: u16,
    /// Active renderer backend label (`"Wgpu"` or `"TinySkia"`).
    pub backend_name: String,
    /// Active resolved CSS color scheme slug (`"light"` or `"dark"`).
    pub theme_code: String,
    /// Active device frame slug (`"iphone-15"`, `"pixel-8"`, `"rpi-800x480"`), if any.
    pub device_frame_code: Option<String>,
    /// Active preview diagnostic overlay flags.
    pub overlays: PreviewOverlayFlags,
    /// Live element count in `ViewTree`.
    pub node_count: usize,
    /// Cumulative Taffy layout pass count.
    pub relayout_passes: u64,
    /// Cumulative live-preview reload count.
    pub reload_count: u64,
    /// Frame render duration in microseconds.
    pub frame_us: u64,
    /// Hovered pointer cursor (`"default"`, `"pointer"`, `"text"`).
    pub cursor_kind: String,
    /// Whether a text input currently accepts IME composition (`!is_password`).
    pub ime_focused: bool,
    /// Whether the currently focused input is `type="password"`.
    pub is_password: bool,
    /// Outer-canvas coordinates `(x, y, w, h)` of the focused IME caret/cursor area.
    pub caret_rect: Option<(f32, f32, f32, f32)>,
    /// Compressed `"KRFR"` RGBA8 frame payload (empty when unchanged since previous non-forced frame).
    pub krfr_bytes: Vec<u8>,
}
