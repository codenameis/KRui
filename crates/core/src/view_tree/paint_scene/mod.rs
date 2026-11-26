//! `PaintScene` assembly, box/text/canvas emission, top-layer overlays, and W3C Stacking Context ordering.

pub mod damage_tracker;
pub mod paint;
pub(crate) mod paint_3d;
pub(crate) mod paint_3d_hit;
pub(crate) mod paint_box_layers;
pub(crate) mod paint_canvas;
pub(crate) mod paint_canvas_rich;
pub(crate) mod paint_text;
pub(crate) mod paint_text_decorations;
pub(crate) mod select_popup_paint;
pub(crate) mod stacking;
pub(crate) mod tooltip_paint;
