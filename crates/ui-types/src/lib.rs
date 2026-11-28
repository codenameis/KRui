//! # `krui-ui-types`
//!
//! Lightweight, `#![forbid(unsafe_code)]` shared types used by both the native KRui engine
//! (`krui-core`) and the browser native DOM backend (`krui-web`), plus `krui-patterns`.
//!
//! Contains zero heavy dependencies (`stylo`, `taffy`, `cosmic-text`, `tiny-skia`, `wgpu`).

#![forbid(unsafe_code)]

pub mod canvas_2d;
pub mod canvas_2d_dash;
pub mod canvas_2d_draw;
pub mod canvas_2d_extras;
pub mod canvas_2d_types;
pub mod events;
pub mod media;
pub mod static_ir;

pub use canvas_2d::{
    CanvasCommand, CanvasContext2D, CanvasDraw, CanvasFillRule, CanvasLineCap, CanvasLineJoin,
    CanvasRedrawMode, PathSegment, Size,
};
pub use canvas_2d_extras::{CanvasGradient, CanvasGradientStop, CanvasTextAlign};
pub use events::{
    clear_prevent_default_key, clear_prevent_default_wheel, prevent_default_key,
    prevent_default_wheel, take_prevent_default_key, take_prevent_default_wheel, ButtonKind,
    ClickBinding, ClickTarget, InputKind, InputTarget, KeydownOutcome, Mods, PointerButton,
    PointerEvent, PointerHandler, PointerTarget, WheelEvent, WheelHandler, WheelTarget,
};
pub use krui_style::LayoutRect;
pub use media::{
    compute_object_fit_placement, DecodedImage, ExternalTextureId, ImageId, ImageSource, SvgColor,
    SvgVectorOp, SvgVectorTemplate,
};
pub use static_ir::{
    StaticAttrDesc, StaticButtonKind, StaticDocumentDesc, StaticInputKind, StaticNodeDesc,
    StaticNodeKind, StaticSelectOptDesc,
};
