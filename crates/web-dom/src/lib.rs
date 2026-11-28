//! # `krui-web`
//!
//! Native browser DOM + pure CSS + `<canvas>` slot backend for KRui.
//! Mounts the exact same compiled `.krui` UI fragments and reactive signals onto real browser
//! elements so OS fonts, IME composition, accessibility, smooth scrolling, and 60/120Hz animations
//! run directly inside the browser engine with zero software frame buffer copies.

#![forbid(unsafe_code)]

pub mod backend;
pub mod bindings;
mod bindings_paint_layout;
#[cfg(target_arch = "wasm32")]
pub mod browser_backend;
#[cfg(target_arch = "wasm32")]
mod browser_canvas2d;
#[cfg(target_arch = "wasm32")]
mod browser_events;
#[cfg(target_arch = "wasm32")]
mod browser_svg;
pub mod control_flow;
pub mod events;
pub mod mock_backend;
mod mock_dispatch;
pub mod slots;
pub mod static_ir;
pub mod tree;
pub mod ua_css;

pub use backend::{DomBackend, DomEventCallback, DomEventKind, DomMutation, DomNodeId, ListenerId};
#[cfg(target_arch = "wasm32")]
pub use browser_backend::BrowserDomBackend;
pub use krui_ui_types::{CanvasFillRule, CanvasLineCap, CanvasLineJoin};
pub use mock_backend::{MockDomBackend, MockDomNode};
pub use tree::{WebBranchMountFn, WebDomTree, WebMount, WebViewFragment};
