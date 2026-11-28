//! # `krui-winit`
//!
//! Desktop `winit` window host, `softbuffer` CPU window blitter, `EventLoopProxy` `UiWaker`,
//! `accesskit_winit::Adapter`, `arboard` OS clipboard, `EngineSession`, and optional `preview` watcher.

#![forbid(unsafe_code)]

pub mod app_icon;
pub mod clipboard;
mod cursor_routing;
#[cfg(feature = "preview")]
pub mod embed_panel;
#[cfg(feature = "preview")]
mod embed_panel_input;
pub mod ime_adapter;
pub mod launch;
pub mod multi_window;
mod multi_window_runner;
#[cfg(feature = "preview")]
pub mod preview;
pub mod session;
pub mod softbuffer_presenter;
pub mod tray;
pub mod tray_host;
mod window_helpers;
pub mod window_loop;

pub use app_icon::{AppIcon, AppIconError, CloseBehavior, WindowOptions, MAX_ICON_DIMENSION};
pub use clipboard::{get_clipboard_text, set_clipboard_text};
pub use ime_adapter::{
    ime_event_from_winit, sync_ime_host_with_sink, sync_window_ime_cursor_area,
    sync_window_ime_cursor_area_cached, ImePlatformSink, RecordedImeOp, RecordedImeSink,
    WinitImeStateCache, WinitTextInputHost,
};
pub use launch::{run_app_window, run_app_window_with_hooks, run_app_window_with_options};
pub use multi_window::{KruiMultiWindowEvent, MultiWindowApp, WindowSpec};
#[cfg(feature = "preview")]
pub use preview as dev_hot_reload;
pub use session::{is_dev_hot_reload_enabled, EngineSession, FrameTelemetry};
pub use softbuffer_presenter::{pack_premultiplied_rgba_to_softbuffer_u32, SoftbufferPresenter};
pub use tray::{
    compute_tray_aware_idle_control_flow, resolve_close_request_command, TrayClickButton,
    TrayConfig, TrayEvent, TrayIdleState, TrayMenuItem, WindowCommand,
};
#[cfg(feature = "tray")]
pub use tray_host::build_native_menu;
pub use window_helpers::{
    apply_key_action, compute_idle_control_flow, compute_window_idle_control_flow,
    key_info_from_logical_key, key_info_from_winit, touch_phase_from_winit, KruiWinitEvent,
    WindowEventHooks, CARET_BLINK_IDLE_TIMEOUT, FRAME_PACE_120HZ,
};
