//! # `krui-core`
//!
//! Platform-agnostic core of KRui (`ViewTree`, layout integration, pointer/keyboard/IME event
//! dispatch, accessibility tree updates, and `PaintScene` assembly).
//!
//! Has **zero dependencies on `winit` or `wgpu`** so game engines, VR hosts, and mobile shells
//! can embed KRui directly by feeding input events and consuming `PaintScene`.

#![forbid(unsafe_code)]

pub mod canvas_2d;
pub mod media;
#[cfg(feature = "preview")]
pub mod registry;
pub mod scene;
pub mod view_tree;

pub use accesskit;
pub use krui_layout::LayoutRect;
pub use media::{
    compute_object_fit_placement, DecodedImage, ExternalTextureId, ImageId, ImageSource, SvgColor,
    SvgVectorOp, SvgVectorTemplate,
};
#[cfg(feature = "preview")]
pub use registry::{
    CallError, DynCommand, DynInputHandler, DynSignal, IntoRegistryValue, Registry,
    RegistryFieldExt, RegistryFieldFallback, RegistryMethodDispatchFallback, RegistryRef,
    ScopeFieldProbe, ScopeRegistry, TryFromRegistryValue, Value,
};
pub use scene::{
    validate_external_file_path, ActiveBackendKind, CanvasCommand, CanvasContext2D, CanvasDraw,
    CanvasFillRule, CanvasLineCap, CanvasLineJoin, CanvasRedrawMode, DrawPrimitive,
    ExtensibleEvent, FrameRenderStats, GpuResourceLimits, PaintScene, PathSegment,
    RenderBackendPreference, ResolvedRadialGradient, Size,
};
pub use view_tree::{
    caret_rect, key_name_for_keydown, prevent_default_key, route_key, route_key_with_repeat,
    utf16_to_utf8_offset, utf16_to_utf8_range, utf8_to_utf16_offset, utf8_to_utf16_range, A11yId,
    BranchMountFn, ButtonKind, ClickBinding, ClickTarget, EditingState, EditorActionKind,
    FocusOrigin, FocusState, HoveredCursorKind, ImeEvent, ImeState, InputKind, InputTarget,
    KeyAction, KeyInfo, KeydownOutcome, Mods, NodeId, TextEditSnapshot, TextInputHost,
    TouchPhaseKind, ViewFragment, ViewTree, ViewportInsets, A11Y_ID_OFFSET, TOUCH_SLOP_PX,
};
#[cfg(feature = "preview")]
pub use view_tree::{
    normalize_preview_path_key, InspectedNodeInfo, PreviewSavedNodes, SavedPreviewNode,
};
