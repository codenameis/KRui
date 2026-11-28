//! Core 2D canvas value types, path segments, and recorded draw command enum.

use crate::canvas_2d::CanvasContext2D;
pub use crate::canvas_2d_extras::{CanvasGradient, CanvasTextAlign};
use crate::media::ImageSource;
use krui_style::{Affine2, Color, LayoutRect};
use std::rc::Rc;

/// Logical 2D size (`width`, `height`) in logical pixels passed to `<canvas>` drawing callbacks.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    /// Width in logical pixels.
    pub width: f32,
    /// Height in logical pixels.
    pub height: f32,
}

impl Size {
    /// Creates a new `Size` from `width` and `height`.
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

type CanvasDrawClosure = Rc<dyn Fn(&mut CanvasContext2D, Size)>;

/// Beginner-friendly `<canvas>` 2D drawing callback wrapper (`CanvasDraw::new(move |ctx, size| ...)`).
#[derive(Clone)]
pub struct CanvasDraw {
    cb: CanvasDrawClosure,
}

impl CanvasDraw {
    /// Wraps a `<canvas>` drawing closure `Fn(&mut CanvasContext2D, Size)` in a `Clone` handle.
    pub fn new(f: impl Fn(&mut CanvasContext2D, Size) + 'static) -> Self {
        Self { cb: Rc::new(f) }
    }

    /// Invokes the underlying drawing callback with `ctx` and its current layout `size`.
    pub fn draw(&self, ctx: &mut CanvasContext2D, size: Size) {
        (self.cb)(ctx, size);
    }
}

impl<F> From<F> for CanvasDraw
where
    F: Fn(&mut CanvasContext2D, Size) + 'static,
{
    fn from(f: F) -> Self {
        Self::new(f)
    }
}

/// Redraw policy for a `<canvas>` element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasRedrawMode {
    /// Re-runs the draw closure only when reactive `Signal`s read inside it change.
    Reactive,
    /// Re-runs the draw closure on every frame tick (for game loops and real-time visualization).
    PerFrame,
}

/// A single segment in a 2D vector path recorded on a [`CanvasContext2D`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathSegment {
    /// Move the path cursor to `(x, y)` without drawing.
    MoveTo(f32, f32),
    /// Draw a straight line segment to `(x, y)`.
    LineTo(f32, f32),
    /// Draw a quadratic Bezier curve with control point `(cx, cy)` to `(x, y)`.
    QuadTo {
        /// Control point X.
        cx: f32,
        /// Control point Y.
        cy: f32,
        /// End point X.
        x: f32,
        /// End point Y.
        y: f32,
    },
    /// Draw a cubic Bezier curve with control points `(c1x, c1y)` and `(c2x, c2y)` to `(x, y)`.
    CubicTo {
        /// First control point X.
        c1x: f32,
        /// First control point Y.
        c1y: f32,
        /// Second control point X.
        c2x: f32,
        /// Second control point Y.
        c2y: f32,
        /// End point X.
        x: f32,
        /// End point Y.
        y: f32,
    },
    /// Draw an elliptical arc segment centered at `(cx, cy)` with radii `(rx, ry)`.
    Arc {
        /// Center X.
        cx: f32,
        /// Center Y.
        cy: f32,
        /// Horizontal radius.
        rx: f32,
        /// Vertical radius.
        ry: f32,
        /// Start angle in radians.
        start_rad: f32,
        /// Sweep angle in radians.
        sweep_rad: f32,
        /// X-axis rotation in radians.
        x_rotation_rad: f32,
    },
    /// Close the current subpath back to its starting point.
    Close,
}

/// Stroke endpoint cap style for 2D `<canvas>` and `<svg>` vector paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CanvasLineCap {
    /// Flat cap ending squarely at the path endpoint (web standard default).
    #[default]
    Butt,
    /// Semicircular cap extending by half the line width beyond each endpoint.
    Round,
    /// Square cap extending by half the line width beyond each endpoint.
    Square,
}

/// Stroke corner join style for 2D `<canvas>` and `<svg>` vector paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CanvasLineJoin {
    /// Sharp angled miter corner (web standard default).
    #[default]
    Miter,
    /// Circular rounded corner join.
    Round,
    /// Flat beveled corner join.
    Bevel,
}

/// Winding parity rule used when filling closed or self-intersecting vector paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CanvasFillRule {
    /// Non-zero winding rule (HTML `<canvas>` and SVG default).
    #[default]
    NonZero,
    /// Even-odd alternating parity fill rule.
    EvenOdd,
}

/// Recorded 2D vector command executed by `wgpu`, `tiny-skia`, or browser `CanvasRenderingContext2d`.
#[derive(Clone, Debug, PartialEq)]
pub enum CanvasCommand {
    /// Filled rectangle (with optional uniform corner `radius`).
    FillRect {
        /// Top-left X.
        x: f32,
        /// Top-left Y.
        y: f32,
        /// Width.
        w: f32,
        /// Height.
        h: f32,
        /// Uniform corner radius.
        radius: f32,
        /// Fill color.
        color: Color,
    },
    /// Stroked rectangle outline.
    StrokeRect {
        /// Top-left X.
        x: f32,
        /// Top-left Y.
        y: f32,
        /// Width.
        w: f32,
        /// Height.
        h: f32,
        /// Stroke width.
        line_width: f32,
        /// Stroke color.
        color: Color,
    },
    /// Filled circle.
    FillCircle {
        /// Center X.
        cx: f32,
        /// Center Y.
        cy: f32,
        /// Circle radius.
        radius: f32,
        /// Fill color.
        color: Color,
    },
    /// Stroked line segment.
    StrokeLine {
        /// Start X.
        x1: f32,
        /// Start Y.
        y1: f32,
        /// End X.
        x2: f32,
        /// End Y.
        y2: f32,
        /// Stroke width.
        line_width: f32,
        /// Cap style.
        line_cap: CanvasLineCap,
        /// Stroke color.
        color: Color,
    },
    /// Filled arbitrary vector path.
    FillPath {
        /// Path segments.
        segments: Vec<PathSegment>,
        /// Fill parity rule.
        fill_rule: CanvasFillRule,
        /// Fill color.
        color: Color,
    },
    /// Stroked arbitrary vector path.
    StrokePath {
        /// Path segments.
        segments: Vec<PathSegment>,
        /// Stroke width.
        line_width: f32,
        /// Cap style.
        line_cap: CanvasLineCap,
        /// Join style.
        line_join: CanvasLineJoin,
        /// Stroke color.
        color: Color,
    },
    /// Updates the local 2D affine transform applied to subsequent canvas commands.
    SetTransform(Affine2),
    /// Pushes an arbitrary vector clip path intersected with any active ancestor clip.
    PushClip {
        /// Clip path segments.
        segments: Vec<PathSegment>,
        /// Fill parity rule.
        fill_rule: CanvasFillRule,
    },
    /// Pops the most recently pushed canvas clip path.
    PopClip,
    /// Fills an arbitrary vector path with a linear or radial [`CanvasGradient`].
    FillGradient {
        /// Path segments.
        segments: Vec<PathSegment>,
        /// Fill parity rule.
        fill_rule: CanvasFillRule,
        /// Gradient definition.
        gradient: CanvasGradient,
        /// Global alpha multiplier.
        alpha: f32,
    },
    /// Draws a styled text string anchored at `(x, y)`.
    DrawText {
        /// String content.
        text: String,
        /// Anchor X.
        x: f32,
        /// Anchor Y.
        y: f32,
        /// Font size in logical px.
        font_size: f32,
        /// Numeric font weight.
        font_weight: u16,
        /// Text color.
        color: Color,
        /// Horizontal anchor alignment.
        align: CanvasTextAlign,
    },
    /// Draws a decoded or external raster image into `dst_rect` with optional source pixel crop.
    DrawImage {
        /// Image source.
        source: ImageSource,
        /// Optional source pixel crop `[sx, sy, sw, sh]`.
        src_rect: Option<[f32; 4]>,
        /// Destination rectangle in logical px.
        dst_rect: LayoutRect,
        /// Global alpha multiplier.
        alpha: f32,
    },
}
