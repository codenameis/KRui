//! Platform-agnostic 2D vector drawing commands and `CanvasContext2D` recorded for
//! `<canvas>` elements, compiled `<svg>` icons, and vector form controls.

use krui_style::Color;
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
    QuadTo { cx: f32, cy: f32, x: f32, y: f32 },
    /// Draw a cubic Bezier curve with control points `(c1x, c1y)` and `(c2x, c2y)` to `(x, y)`.
    CubicTo {
        c1x: f32,
        c1y: f32,
        c2x: f32,
        c2y: f32,
        x: f32,
        y: f32,
    },
    /// Draw an elliptical arc segment centered at `(cx, cy)` with radii `(rx, ry)`.
    Arc {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        start_rad: f32,
        sweep_rad: f32,
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

/// Recorded 2D vector command executed by either `wgpu` (via `lyon_tessellation`) or `tiny-skia`.
#[derive(Clone, Debug, PartialEq)]
pub enum CanvasCommand {
    /// Filled rectangle (with optional uniform corner `radius`).
    FillRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        color: Color,
    },
    /// Stroked rectangle outline.
    StrokeRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        line_width: f32,
        color: Color,
    },
    /// Filled circle.
    FillCircle {
        cx: f32,
        cy: f32,
        radius: f32,
        color: Color,
    },
    /// Stroked line segment.
    StrokeLine {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        line_width: f32,
        line_cap: CanvasLineCap,
        color: Color,
    },
    /// Filled arbitrary vector path.
    FillPath {
        segments: Vec<PathSegment>,
        fill_rule: CanvasFillRule,
        color: Color,
    },
    /// Stroked arbitrary vector path.
    StrokePath {
        segments: Vec<PathSegment>,
        line_width: f32,
        line_cap: CanvasLineCap,
        line_join: CanvasLineJoin,
        color: Color,
    },
}

/// First-class 2D drawing context exposed to `<canvas>` elements, `<svg>` icons, and internal controls.
#[derive(Clone, Debug)]
pub struct CanvasContext2D {
    width: f32,
    height: f32,
    current_path: Vec<PathSegment>,
    commands: Vec<CanvasCommand>,
    line_cap: CanvasLineCap,
    line_join: CanvasLineJoin,
    fill_rule: CanvasFillRule,
}

impl Default for CanvasContext2D {
    fn default() -> Self {
        Self {
            width: 428.0,
            height: 260.0,
            current_path: Vec::new(),
            commands: Vec::new(),
            line_cap: CanvasLineCap::Butt,
            line_join: CanvasLineJoin::Miter,
            fill_rule: CanvasFillRule::NonZero,
        }
    }
}

impl CanvasContext2D {
    /// Creates a `CanvasContext2D` with default canvas dimensions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a `CanvasContext2D` sized to the `<canvas>` node's layout bounds `(width, height)`.
    pub fn with_size(width: f32, height: f32) -> Self {
        Self {
            width: width.max(1.0),
            height: height.max(1.0),
            current_path: Vec::new(),
            commands: Vec::new(),
            line_cap: CanvasLineCap::Butt,
            line_join: CanvasLineJoin::Miter,
            fill_rule: CanvasFillRule::NonZero,
        }
    }

    /// Sets the stroke endpoint cap style for subsequent `stroke_path`/`stroke_line` calls.
    pub fn set_line_cap(&mut self, cap: CanvasLineCap) {
        self.line_cap = cap;
    }

    /// Sets the stroke corner join style for subsequent `stroke_path`/`stroke_circle` calls.
    pub fn set_line_join(&mut self, join: CanvasLineJoin) {
        self.line_join = join;
    }

    /// Sets the path fill rule for subsequent `fill_path` calls.
    pub fn set_fill_rule(&mut self, rule: CanvasFillRule) {
        self.fill_rule = rule;
    }

    /// Returns the logical width of the `<canvas>` node being drawn.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Returns the logical height of the `<canvas>` node being drawn.
    pub fn height(&self) -> f32 {
        self.height
    }

    /// Returns the `(width, height)` of the `<canvas>` node being drawn.
    pub fn viewport_size(&self) -> (f32, f32) {
        (self.width, self.height)
    }

    /// Clears recorded path state and drawing commands.
    pub fn clear(&mut self) {
        self.current_path.clear();
        self.commands.clear();
    }

    /// Returns a slice of recorded drawing commands.
    pub fn commands(&self) -> &[CanvasCommand] {
        &self.commands
    }

    /// Consumes the drawing context and returns its recorded commands.
    pub fn into_commands(self) -> Vec<CanvasCommand> {
        self.commands
    }

    /// Begins a new vector subpath, discarding any uncommitted path segments.
    pub fn begin_path(&mut self) {
        self.current_path.clear();
    }

    /// Moves the current path cursor to `(x, y)`.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.current_path.push(PathSegment::MoveTo(x, y));
    }

    /// Adds a straight line segment to `(x, y)`.
    pub fn line_to(&mut self, x: f32, y: f32) {
        self.current_path.push(PathSegment::LineTo(x, y));
    }

    /// Adds a quadratic Bezier curve with control point `(cx, cy)` to `(x, y)`.
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.current_path.push(PathSegment::QuadTo { cx, cy, x, y });
    }

    /// Adds a cubic Bezier curve with control points `(c1x, c1y)` and `(c2x, c2y)` to `(x, y)`.
    pub fn bezier_curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.current_path.push(PathSegment::CubicTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
        });
    }

    /// Adds an elliptical arc segment centered at `(cx, cy)` with radii `(rx, ry)`.
    #[allow(clippy::too_many_arguments)]
    pub fn ellipse_arc(
        &mut self,
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        start_rad: f32,
        sweep_rad: f32,
        x_rotation_rad: f32,
    ) {
        self.current_path.push(PathSegment::Arc {
            cx,
            cy,
            rx,
            ry,
            start_rad,
            sweep_rad,
            x_rotation_rad,
        });
    }

    /// Adds a circular arc centered at `(cx, cy)` with `radius` from `start_rad` to `end_rad`.
    pub fn arc(&mut self, cx: f32, cy: f32, radius: f32, start_rad: f32, end_rad: f32) {
        self.ellipse_arc(cx, cy, radius, radius, start_rad, end_rad - start_rad, 0.0);
    }

    /// Closes the current subpath back to its initial point.
    pub fn close_path(&mut self) {
        self.current_path.push(PathSegment::Close);
    }

    /// Fills the current path with `color`.
    pub fn fill_path(&mut self, color: Color) {
        if !self.current_path.is_empty() {
            self.commands.push(CanvasCommand::FillPath {
                segments: self.current_path.clone(),
                fill_rule: self.fill_rule,
                color,
            });
        }
    }

    /// Strokes the current path with `line_width` and `color`.
    pub fn stroke_path(&mut self, line_width: f32, color: Color) {
        if !self.current_path.is_empty() {
            self.commands.push(CanvasCommand::StrokePath {
                segments: self.current_path.clone(),
                line_width: line_width.max(0.05),
                line_cap: self.line_cap,
                line_join: self.line_join,
                color,
            });
        }
    }

    /// Fills a sharp rectangle at `(x, y)` of size `(w, h)`.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.fill_round_rect(x, y, w, h, 0.0, color);
    }

    /// Fills a rounded rectangle at `(x, y)` of size `(w, h)` with uniform corner `radius`.
    pub fn fill_round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
        self.commands.push(CanvasCommand::FillRect {
            x,
            y,
            w,
            h,
            radius,
            color,
        });
    }

    /// Alias of [`Self::fill_round_rect`] for CSS-style naming consistency.
    pub fn fill_rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
        self.fill_round_rect(x, y, w, h, radius, color);
    }

    /// Strokes a rectangle outline at `(x, y)` of size `(w, h)`.
    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, line_width: f32, color: Color) {
        self.commands.push(CanvasCommand::StrokeRect {
            x,
            y,
            w,
            h,
            line_width: line_width.max(0.05),
            color,
        });
    }

    /// Fills a circle centered at `(cx, cy)` with `radius`.
    pub fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, color: Color) {
        self.commands.push(CanvasCommand::FillCircle {
            cx,
            cy,
            radius,
            color,
        });
    }

    /// Strokes a circle centered at `(cx, cy)` with `radius` and `line_width`.
    pub fn stroke_circle(&mut self, cx: f32, cy: f32, radius: f32, line_width: f32, color: Color) {
        if radius <= 0.0 {
            return;
        }
        let segments = vec![
            PathSegment::MoveTo(cx + radius, cy),
            PathSegment::Arc {
                cx,
                cy,
                rx: radius,
                ry: radius,
                start_rad: 0.0,
                sweep_rad: std::f32::consts::TAU,
                x_rotation_rad: 0.0,
            },
            PathSegment::Close,
        ];
        self.commands.push(CanvasCommand::StrokePath {
            segments,
            line_width: line_width.max(0.05),
            line_cap: self.line_cap,
            line_join: self.line_join,
            color,
        });
    }

    /// Strokes a straight line segment from `(x1, y1)` to `(x2, y2)`.
    pub fn stroke_line(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        line_width: f32,
        color: Color,
    ) {
        self.commands.push(CanvasCommand::StrokeLine {
            x1,
            y1,
            x2,
            y2,
            line_width: line_width.max(0.05),
            line_cap: self.line_cap,
            color,
        });
    }
}
