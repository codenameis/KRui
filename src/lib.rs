//! # KRui
//!
//! A free (MIT/Apache-2.0), performance-first, cross-platform Rust GUI framework.
//!
//! ## Architectural Pillars
//! 1. **Compile-time HTML + Reactive Templates (`krui!`, `include_krui!`)**: HTML markup and
//!    type-safe template expressions compile at build time into direct `ViewTree` node builders and
//!    signal bindings, keeping `krui-compiler` out of release binaries while Stylo 0.22 evaluates
//!    pure W3C CSS cascade, selectors, `@media`/`@layer`/`@scope`, and custom properties.
//! 2. **Fine-grained Glitch-free Signals**: Contiguous arena signals with batched propagation (`batch`)
//!    and a first-class renderer-tied `FrameClock` (`shake`, `nudge`, `spring_to`).
//! 3. **Compositor Fast-Path & Taffy 0.15**: `transform` and `opacity` `PaintSlot` updates bypass
//!    Taffy relayout and Stylo restyling completely (`0` relayouts).
//! 4. **Dual Backend (`wgpu` + `tiny-skia`)**: Auto-detects GPU support and falls back to `tiny-skia`
//!    with pure CSS styling (`krui://preset/unstyled` + custom stylesheets).

#![forbid(unsafe_code)]

#[cfg(all(feature = "native-engine", feature = "web-dom"))]
compile_error!("Features `native-engine` and `web-dom` are mutually exclusive.");

#[cfg(not(any(feature = "native-engine", feature = "web-dom")))]
compile_error!("Enable either `native-engine` (default) or `web-dom`.");

extern crate self as krui;

pub mod widgets;

pub use widgets::{
    CalendarDayCell, CameraBasis3D, CarouselController, ColorPickerController,
    ContextMenuController, ContextMenuItem, DatePickerController, DockLayoutController, DockPane,
    DragFloatController, DropPlacement, GizmoAxis, GizmoController, GizmoMode, GizmoPalette,
    SnackbarController, SplitAxis, SplitPaneController, TimePickerController, TreeNodeData,
    TreeViewController, UndoEntry, UndoHistory, Vec3Controller, VisibleTreeRow,
};
#[cfg(feature = "native-engine")]
pub use widgets::{KeyboardLayer, VirtualKeyKind, VirtualKeyboardController};

pub use krui_ui_types::{
    prevent_default_key, prevent_default_wheel, ButtonKind, CanvasCommand, CanvasContext2D,
    CanvasDraw, CanvasFillRule, CanvasGradient, CanvasGradientStop, CanvasLineCap, CanvasLineJoin,
    CanvasRedrawMode, CanvasTextAlign, ClickBinding, ClickTarget, DecodedImage, ExternalTextureId,
    ImageId, ImageSource, InputKind, InputTarget, KeydownOutcome, LayoutRect, Mods, PathSegment,
    PointerButton, PointerEvent, PointerHandler, PointerTarget, Size, SvgColor, SvgVectorOp,
    SvgVectorTemplate, WheelEvent, WheelHandler, WheelTarget,
};

#[cfg(feature = "native-engine")]
pub use krui_a11y::{A11yNodeDescriptor, AccessibilityEngine, SemanticRole};
#[cfg(feature = "native-engine")]
pub use krui_core::{
    caret_rect, collect_widget_hotspots, key_name_for_keydown, next_spatial_focus, route_key,
    route_key_with_repeat, utf16_to_utf8_offset, utf16_to_utf8_range, utf8_to_utf16_offset,
    utf8_to_utf16_range, validate_external_file_path, A11yId, ActiveBackendKind, BranchMountFn,
    CalibrationPair, DrawPrimitive, DynamicSlotBuffer, DynamicSlotOverride, EditingState,
    EditorActionKind, ExtensibleEvent, FocusOrigin, FocusState, FocusableCandidate,
    FrameRenderStats, GpuResourceLimits, Hangul2BulsikAutomaton, HangulStep, HoveredCursorKind,
    ImeEvent, ImeState, KeyAction, KeyInfo, NodeId, PaintScene, PaintScenePool,
    RenderBackendPreference, SpatialNavDirection, TextEditSnapshot, TextInputHost,
    TouchCalibrationMatrix, TouchPhaseKind, ViewFragment, ViewTree, ViewportInsets,
    WidgetActionBinding, WidgetHotspot, WidgetRenderConfig, WidgetSizeClass, WidgetSnapshot,
    A11Y_ID_OFFSET, MAX_WIDGET_EDGE_PX, MAX_WIDGET_SCALE, MIN_WIDGET_SCALE, TOUCH_SLOP_PX,
};
#[cfg(all(feature = "native-engine", feature = "preview"))]
pub use krui_core::{
    normalize_preview_path_key, CallError, DynCommand, DynInputHandler, DynSignal,
    InspectedNodeInfo, IntoRegistryValue, PreviewSavedNodes, Registry, RegistryFieldExt,
    RegistryFieldFallback, RegistryMethodDispatchFallback, RegistryRef, SavedPreviewNode,
    ScopeFieldProbe, ScopeRegistry, TryFromRegistryValue, Value,
};
#[cfg(feature = "native-engine")]
pub use krui_layout::{LayoutEngine, TextContext};
pub use krui_macros::{controller, include_krui, krui, store_msg};
#[cfg(feature = "preview")]
pub use krui_patterns::StoreMsgRegistry;
pub use krui_patterns::{
    view_model, AppStateExt, AsyncCommandDone, ClickActionResult, Command, EffectDoneCallback,
    Effects, FromAppState, InputCommand, Store,
};
#[cfg(feature = "native-engine")]
pub use krui_renderer::convert_rgba8_to_rgb565_le_dithered;
#[cfg(all(feature = "native-engine", feature = "desktop"))]
pub use krui_renderer::{
    AdaptiveRenderer, DualGlyphAtlas, GpuContext, RendererBackend, TinySkiaRenderer, WgpuRenderer,
    WindowSurfacePresenter,
};
#[cfg(all(feature = "native-engine", not(feature = "desktop")))]
pub use krui_renderer::{AdaptiveRenderer, RendererBackend, TinySkiaRenderer};
pub use krui_signals::{
    batch, current_runtime_id, current_wake_handle, debug_stats, poll_async_tasks,
    register_async_poller, reset_reactive_runtime, set_ui_waker, step_analytical_spring,
    total_subscriber_executions, untracked, wake_ui_loop, Computed, ComputedId, Effect, EffectId,
    FastPathTransform, FrameClock, FrameTick, IntoKruiTemplateScalar, KruiBoolCondition,
    KruiIterableSource, KruiLoopItem, KruiTemplateScalar, ReactiveSession, RuntimeId, RuntimeStats,
    Scope, ScopeId, SessionWakeHandle, Signal, SignalId, SubscriberId, UiWaker,
};
#[cfg(not(target_arch = "wasm32"))]
pub use krui_signals::{
    init_async_runtime_mode, spawn_tokio_interval, spawn_tokio_stream, spawn_tokio_task,
    spawn_tokio_task_with_status, tokio_handle, tokio_runtime, AsyncRuntimeMode, AsyncTaskHandle,
    StreamSink,
};
pub use krui_style::{
    is_bidi_control_char, sanitize_untrusted_text, Affine2, AlignItems, AnimationDirection,
    AnimationFillMode, AnimationSpec, BoxShadow, Color, ColorDepthBits, ColorScheme,
    ColorSchemePreference, CompiledStyle, CornerRadii, CssFilterEffect, CursorStyle, DamageRect,
    DamageTrackingMode, Dimension, DisplayMode, DisplayShape, DynamicSlotId, Edges, FlexDirection,
    FontFamilyStack, GridTrack, GridTrackList, InheritedTextProps, InsetEdges, JustifyContent,
    KeyframeStop, KeyframesDef, LayoutPatchOp, LayoutStyle, LinearGradient, ObjectFit,
    OverflowMode, PaintPatchOp, PaintStyle, PositionMode, ReducedMotionPreference,
    RootProfileAttrs, TextAlign, TextDirection, TextPatchOp, TextStyle, TierProfile, Transform2D,
    TransitionCurve, TransitionMask, TransitionSpec, UserSelect, MAX_GRID_TRACKS,
};
#[cfg(feature = "native-engine")]
pub use krui_text::{
    compute_ime_visuals, DecodedGlyphBitmap, ImeUnderlineSegment, ImeVisualGeometry,
    RasterizedGlyph, RichTextSpan, ShapedTextLayout, TextEditBuffer, TextEngine, TextLineMetrics,
};
#[cfg(feature = "web-dom")]
pub use krui_web::{
    DomBackend, DomMutation, DomNodeId as NodeId, MockDomBackend,
    WebBranchMountFn as BranchMountFn, WebDomTree as ViewTree, WebMount,
    WebViewFragment as ViewFragment,
};
#[cfg(feature = "preview")]
pub use krui_winit::preview;
#[cfg(feature = "preview")]
pub use krui_winit::preview as dev_hot_reload;
#[cfg(feature = "desktop")]
pub use krui_winit::{
    apply_key_action, compute_idle_control_flow, compute_tray_aware_idle_control_flow,
    compute_window_idle_control_flow, get_clipboard_text, ime_event_from_winit,
    is_dev_hot_reload_enabled, key_info_from_logical_key, key_info_from_winit,
    pack_premultiplied_rgba_to_softbuffer_u32, resolve_close_request_command, run_app_window,
    run_app_window_with_hooks, run_app_window_with_options, set_clipboard_text,
    sync_window_ime_cursor_area, touch_phase_from_winit, AppIcon, AppIconError, CloseBehavior,
    EngineSession, FrameTelemetry, KruiMultiWindowEvent, KruiWinitEvent, MultiWindowApp,
    SoftbufferPresenter, TrayClickButton, TrayConfig, TrayEvent, TrayIdleState, TrayMenuItem,
    WindowCommand, WindowEventHooks, WindowOptions, WindowSpec, WinitTextInputHost,
    CARET_BLINK_IDLE_TIMEOUT, FRAME_PACE_120HZ,
};
#[cfg(not(target_arch = "wasm32"))]
pub use tokio;

/// Two-way `bind:value` conversion helper supporting `Signal<String>` as well as numeric/bool `Signal<T>`.
pub trait KruiBindableSignal {
    /// Parses `raw` and updates the underlying signal if parsing succeeds.
    fn set_from_krui_input(&self, raw: String);
}

impl KruiBindableSignal for Signal<String> {
    fn set_from_krui_input(&self, raw: String) {
        self.set(raw);
    }
}

impl KruiBindableSignal for Signal<f32> {
    fn set_from_krui_input(&self, raw: String) {
        if let Ok(v) = raw.trim().parse::<f32>() {
            if v.is_finite() {
                self.set(v);
            }
        }
    }
}

impl KruiBindableSignal for Signal<f64> {
    fn set_from_krui_input(&self, raw: String) {
        if let Ok(v) = raw.trim().parse::<f64>() {
            if v.is_finite() {
                self.set(v);
            }
        }
    }
}

impl KruiBindableSignal for Signal<i32> {
    fn set_from_krui_input(&self, raw: String) {
        if let Ok(v) = raw.trim().parse::<i32>() {
            self.set(v);
        }
    }
}

impl KruiBindableSignal for Signal<i64> {
    fn set_from_krui_input(&self, raw: String) {
        if let Ok(v) = raw.trim().parse::<i64>() {
            self.set(v);
        }
    }
}

impl KruiBindableSignal for Signal<u32> {
    fn set_from_krui_input(&self, raw: String) {
        if let Ok(v) = raw.trim().parse::<u32>() {
            self.set(v);
        }
    }
}

impl KruiBindableSignal for Signal<usize> {
    fn set_from_krui_input(&self, raw: String) {
        if let Ok(v) = raw.trim().parse::<usize>() {
            self.set(v);
        }
    }
}

impl KruiBindableSignal for Signal<bool> {
    fn set_from_krui_input(&self, raw: String) {
        if let Ok(v) = raw.trim().parse::<bool>() {
            self.set(v);
        }
    }
}

/// Internal crate namespace reserved for procedural macro code generation (`krui-macros`).
///
/// Application code must not depend on `__private` directly; prefer top-level `krui::*` types.
#[doc(hidden)]
pub mod __private {
    #[cfg(feature = "native-engine")]
    pub use krui_a11y as a11y;
    #[cfg(feature = "native-engine")]
    pub use krui_core as core;
    #[cfg(feature = "native-engine")]
    pub use krui_layout as layout;
    pub use krui_patterns as patterns;
    #[cfg(feature = "native-engine")]
    pub use krui_renderer as renderer;
    pub use krui_signals as signals;
    pub use krui_style as style;
    #[cfg(feature = "native-engine")]
    pub use krui_text as text;
    #[cfg(feature = "web-dom")]
    pub use krui_web as core;
    #[cfg(feature = "desktop")]
    pub use krui_winit as winit;
    #[cfg(not(target_arch = "wasm32"))]
    pub use tokio;
}
