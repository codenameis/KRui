//! Platform-agnostic 2D vector drawing commands and `CanvasContext2D` recorded for
//! `<canvas>` elements, compiled `<svg>` icons, and vector form controls.

use crate::canvas_2d_extras::{rect_to_path_segments, scale_color_alpha, CanvasState};
pub use crate::canvas_2d_extras::{CanvasGradient, CanvasGradientStop, CanvasTextAlign};
use crate::media::ImageSource;
use krui_layout::LayoutRect;
use krui_style::{Affine2, Color};
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
    /// Updates the local 2D affine transform applied to subsequent canvas commands.
    SetTransform(Affine2),
    /// Pushes an arbitrary vector clip path intersected with any active ancestor clip.
    PushClip {
        segments: Vec<PathSegment>,
        fill_rule: CanvasFillRule,
    },
    /// Pops the most recently pushed canvas clip path.
    PopClip,
    /// Fills an arbitrary vector path with a linear or radial [`CanvasGradient`].
    FillGradient {
        segments: Vec<PathSegment>,
        fill_rule: CanvasFillRule,
        gradient: CanvasGradient,
        alpha: f32,
    },
    /// Draws a styled text string anchored at `(x, y)`.
    DrawText {
        text: String,
        x: f32,
        y: f32,
        font_size: f32,
        font_weight: u16,
        color: Color,
        align: CanvasTextAlign,
    },
    /// Draws a decoded or external raster image into `dst_rect` with optional source pixel crop `[sx, sy, sw, sh]`.
    DrawImage {
        source: ImageSource,
        src_rect: Option<[f32; 4]>,
        dst_rect: LayoutRect,
        alpha: f32,
    },
}

/// First-class 2D drawing context exposed to `<canvas>` elements, `<svg>` icons, and internal controls.
#[derive(Clone, Debug)]
pub struct CanvasContext2D {
    width: f32,
    height: f32,
    pub(crate) current_path: Vec<PathSegment>,
    pub(crate) commands: Vec<CanvasCommand>,
    pub(crate) state: CanvasState,
    state_stack: Vec<CanvasState>,
    emitted_transform: Affine2,
}

impl Default for CanvasContext2D {
    fn default() -> Self {
        Self {
            width: 428.0,
            height: 260.0,
            current_path: Vec::new(),
            commands: Vec::new(),
            state: CanvasState::default(),
            state_stack: Vec::new(),
            emitted_transform: Affine2::IDENTITY,
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
            state: CanvasState::default(),
            state_stack: Vec::new(),
            emitted_transform: Affine2::IDENTITY,
        }
    }

    pub(crate) fn sync_transform(&mut self) {
        if self.emitted_transform != self.state.transform {
            self.commands
                .push(CanvasCommand::SetTransform(self.state.transform));
            self.emitted_transform = self.state.transform;
        }
    }

    pub(crate) fn effective_color(&self, color: Color) -> Color {
        if (self.state.global_alpha - 1.0).abs() <= 1e-5 {
            color
        } else {
            scale_color_alpha(color, self.state.global_alpha)
        }
    }

    /// Pushes a snapshot of the current transform, clip stack depth, dash, stroke, and alpha state.
    pub fn save(&mut self) {
        self.state_stack.push(self.state.clone());
    }

    /// Restores the most recently saved drawing state and pops any clip regions pushed since `save()`.
    pub fn restore(&mut self) {
        if let Some(prev) = self.state_stack.pop() {
            while self.state.clip_depth > prev.clip_depth {
                self.commands.push(CanvasCommand::PopClip);
                self.state.clip_depth -= 1;
            }
            self.state = prev;
        }
    }

    /// Post-multiplies the current canvas transform by a translation `(dx, dy)`.
    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.state.transform = self.state.transform * Affine2::translation(dx, dy);
    }

    /// Post-multiplies the current canvas transform by a scale `(sx, sy)`.
    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.state.transform = self.state.transform * Affine2::scale(sx, sy);
    }

    /// Post-multiplies the current canvas transform by a rotation of `radians` clockwise.
    pub fn rotate(&mut self, radians: f32) {
        self.state.transform = self.state.transform * Affine2::rotation_deg(radians.to_degrees());
    }

    /// Post-multiplies the current canvas transform by affine matrix `matrix`.
    pub fn transform(&mut self, matrix: Affine2) {
        self.state.transform = self.state.transform * matrix;
    }

    /// Replaces the current canvas transform with `matrix`.
    pub fn set_transform(&mut self, matrix: Affine2) {
        self.state.transform = matrix;
    }

    /// Resets the current canvas transform back to [`Affine2::IDENTITY`].
    pub fn reset_transform(&mut self) {
        self.state.transform = Affine2::IDENTITY;
    }

    /// Returns the currently active 2D canvas transform matrix.
    pub fn current_transform(&self) -> Affine2 {
        self.state.transform
    }

    /// Sets the global alpha multiplier (`0.0..=1.0`) applied to subsequent draws.
    pub fn set_global_alpha(&mut self, alpha: f32) {
        if alpha.is_finite() {
            self.state.global_alpha = alpha.clamp(0.0, 1.0);
        }
    }

    /// Returns the currently active global alpha multiplier (`0.0..=1.0`).
    pub fn global_alpha(&self) -> f32 {
        self.state.global_alpha
    }

    /// Sets the stroke dash pattern (empty slice resets to solid strokes; odd slices are duplicated per HTML5 Canvas).
    pub fn set_line_dash(&mut self, segments: &[f32]) {
        if segments.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return;
        }
        if segments.is_empty() || segments.iter().all(|&v| v <= 1e-5) {
            self.state.line_dash.clear();
            return;
        }
        let mut pattern = segments.to_vec();
        if pattern.len() % 2 == 1 {
            pattern.extend_from_within(..);
        }
        self.state.line_dash = pattern;
    }

    /// Returns the active stroke dash pattern slice.
    pub fn line_dash(&self) -> &[f32] {
        &self.state.line_dash
    }

    /// Sets the stroke dash pattern phase offset in logical pixels.
    pub fn set_line_dash_offset(&mut self, offset: f32) {
        if offset.is_finite() {
            self.state.line_dash_offset = offset;
        }
    }

    /// Returns the active stroke dash pattern phase offset.
    pub fn line_dash_offset(&self) -> f32 {
        self.state.line_dash_offset
    }

    /// Intersects the active clip region with `current_path` (popped automatically on matching `restore()`).
    pub fn clip_path(&mut self) {
        if !self.current_path.is_empty() {
            self.sync_transform();
            self.state.clip_depth += 1;
            self.commands.push(CanvasCommand::PushClip {
                segments: self.current_path.clone(),
                fill_rule: self.state.fill_rule,
            });
        }
    }

    /// Intersects the active clip region with the rectangle `(x, y, w, h)` (popped on matching `restore()`).
    pub fn clip_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.sync_transform();
        self.state.clip_depth += 1;
        self.commands.push(CanvasCommand::PushClip {
            segments: rect_to_path_segments(x, y, w, h, 0.0),
            fill_rule: CanvasFillRule::NonZero,
        });
    }

    /// Sets the stroke endpoint cap style for subsequent `stroke_path`/`stroke_line` calls.
    pub fn set_line_cap(&mut self, cap: CanvasLineCap) {
        self.state.line_cap = cap;
    }

    /// Sets the stroke corner join style for subsequent `stroke_path`/`stroke_circle` calls.
    pub fn set_line_join(&mut self, join: CanvasLineJoin) {
        self.state.line_join = join;
    }

    /// Sets the path fill rule for subsequent `fill_path` calls.
    pub fn set_fill_rule(&mut self, rule: CanvasFillRule) {
        self.state.fill_rule = rule;
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

    /// Clears recorded path state, transform/clip state stack, and drawing commands.
    pub fn clear(&mut self) {
        self.current_path.clear();
        self.commands.clear();
        self.state = CanvasState::default();
        self.state_stack.clear();
        self.emitted_transform = Affine2::IDENTITY;
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
}
