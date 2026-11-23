//! Retained raster image (`<img>`), external GPU texture (`ExternalTexture`), and
//! compiled vector `<svg>` template primitives for `krui-core`.

use crate::scene::{
    CanvasCommand, CanvasContext2D, CanvasFillRule, CanvasLineCap, CanvasLineJoin, PathSegment,
};
use krui_layout::LayoutRect;
use krui_style::{Color, ObjectFit};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static NEXT_IMAGE_ID: AtomicU64 = AtomicU64::new(1);

/// Opaque unique handle identifying a decoded [`DecodedImage`] texture in GPU/SW caches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageId(pub u64);

/// Opaque host-owned external GPU texture slot identifier (`VkImage` / `SurfaceTexture` / engine pass).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExternalTextureId(pub u32);

/// Decoded RGBA8 raster image payload shared via `Arc<[u8]>` between `ViewTree` and renderers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    /// Stable cache key for this image instance.
    pub id: ImageId,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Tightly packed row-major straight RGBA8 bytes (`width * height * 4`).
    pub rgba8: Arc<[u8]>,
}

impl DecodedImage {
    /// Decodes PNG, JPEG, or WebP bytes into a premultiplication-ready straight RGBA8 image.
    ///
    /// Enforces defensive dimension (`<= 16384x16384`) and allocation (`<= 256 MiB`) limits
    /// against crafted decompression bombs.
    pub fn from_encoded_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| format!("Failed to inspect image format: {e}"))?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16_384);
        limits.max_image_height = Some(16_384);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        let dyn_img = reader
            .decode()
            .map_err(|e| format!("Failed to decode image bytes: {e}"))?;
        let rgba = dyn_img.to_rgba8();
        let (width, height) = rgba.dimensions();
        Self::from_rgba8(width, height, rgba.into_raw())
    }

    /// Wraps raw straight RGBA8 pixels, validating dimensions and buffer byte length.
    pub fn from_rgba8(width: u32, height: u32, rgba8: Vec<u8>) -> Result<Self, String> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|px| px.checked_mul(4))
            .ok_or_else(|| "Image dimensions overflow usize".to_string())?;
        if width == 0 || height == 0 || rgba8.len() != expected {
            return Err(format!(
                "Invalid RGBA8 image buffer: expected {expected} bytes for {width}x{height}, got {}",
                rgba8.len()
            ));
        }
        Ok(Self {
            id: ImageId(NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed)),
            width,
            height,
            rgba8: Arc::from(rgba8),
        })
    }
}

/// Source backing a `DrawPrimitive::Image` draw call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageSource {
    /// CPU-decoded raster image (`<img>`).
    Decoded(DecodedImage),
    /// Host-owned external GPU texture slot (`ExternalTexture`).
    External {
        /// Registered slot identifier.
        id: ExternalTextureId,
        /// Intrinsic texture width in pixels (used for `ObjectFit` aspect ratio).
        width: u32,
        /// Intrinsic texture height in pixels.
        height: u32,
    },
}

impl ImageSource {
    /// Returns intrinsic pixel `(width, height)`.
    pub const fn intrinsic_dimensions(&self) -> (u32, u32) {
        match self {
            Self::Decoded(img) => (img.width, img.height),
            Self::External { width, height, .. } => (*width, *height),
        }
    }
}

/// Vector paint color in a compiled `<svg>` template (explicit CSS color or `currentColor`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgColor {
    /// Explicit CSS color.
    Explicit(Color),
    /// Inherited from the element's resolved CSS `color` (`PaintStyle::text_color`).
    CurrentColor,
}

impl SvgColor {
    /// Resolves `self` against the node's effective text `color`.
    pub const fn resolve(self, current_color: Color) -> Color {
        match self {
            Self::Explicit(c) => c,
            Self::CurrentColor => current_color,
        }
    }
}

/// Single scalable vector drawing operation in a [`SvgVectorTemplate`]'s `viewBox` coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum SvgVectorOp {
    /// Filled rounded rectangle.
    FillRect {
        /// Left X in viewBox coordinates.
        x: f32,
        /// Top Y in viewBox coordinates.
        y: f32,
        /// Width in viewBox coordinates.
        w: f32,
        /// Height in viewBox coordinates.
        h: f32,
        /// Corner radius in viewBox coordinates.
        radius: f32,
        /// Fill color.
        color: SvgColor,
    },
    /// Stroked rectangle.
    StrokeRect {
        /// Left X in viewBox coordinates.
        x: f32,
        /// Top Y in viewBox coordinates.
        y: f32,
        /// Width in viewBox coordinates.
        w: f32,
        /// Height in viewBox coordinates.
        h: f32,
        /// Stroke width in viewBox coordinates.
        line_width: f32,
        /// Stroke color.
        color: SvgColor,
    },
    /// Filled circle.
    FillCircle {
        /// Center X in viewBox coordinates.
        cx: f32,
        /// Center Y in viewBox coordinates.
        cy: f32,
        /// Radius in viewBox coordinates.
        radius: f32,
        /// Fill color.
        color: SvgColor,
    },
    /// Stroked circle.
    StrokeCircle {
        /// Center X in viewBox coordinates.
        cx: f32,
        /// Center Y in viewBox coordinates.
        cy: f32,
        /// Radius in viewBox coordinates.
        radius: f32,
        /// Stroke width in viewBox coordinates.
        line_width: f32,
        /// Stroke color.
        color: SvgColor,
    },
    /// Filled vector path.
    FillPath {
        /// Path segments in viewBox coordinates.
        segments: Vec<PathSegment>,
        /// SVG `fill-rule` parity (`NonZero` or `EvenOdd`).
        fill_rule: CanvasFillRule,
        /// Fill color.
        color: SvgColor,
    },
    /// Stroked vector path.
    StrokePath {
        /// Path segments in viewBox coordinates.
        segments: Vec<PathSegment>,
        /// Stroke width in viewBox coordinates.
        line_width: f32,
        /// Stroke endpoint cap style (`stroke-linecap`).
        line_cap: CanvasLineCap,
        /// Stroke corner join style (`stroke-linejoin`).
        line_join: CanvasLineJoin,
        /// Stroke color.
        color: SvgColor,
    },
}

/// Resolution-independent vector icon template compiled from `<svg>` at compile time or preview mount.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgVectorTemplate {
    /// ViewBox `(min_x, min_y, width, height)`.
    pub view_box: [f32; 4],
    /// Intrinsic CSS `width`/`height` fallback in logical px.
    pub intrinsic_size: (f32, f32),
    /// Ordered vector drawing operations inside `view_box`.
    pub ops: Arc<[SvgVectorOp]>,
}

impl SvgVectorTemplate {
    /// Materializes `self` into scaled [`CanvasCommand`]s for `world_rect` and `current_color`.
    pub fn to_canvas_commands(&self, rect: LayoutRect, current_color: Color) -> Vec<CanvasCommand> {
        let [vb_x, vb_y, vb_w, vb_h] = self.view_box;
        if vb_w <= 0.0 || vb_h <= 0.0 || rect.width <= 0.0 || rect.height <= 0.0 {
            return Vec::new();
        }
        let scale = (rect.width / vb_w).min(rect.height / vb_h);
        let ox = (rect.width - vb_w * scale) * 0.5 - vb_x * scale;
        let oy = (rect.height - vb_h * scale) * 0.5 - vb_y * scale;
        let mut ctx = CanvasContext2D::with_size(rect.width, rect.height);
        for op in self.ops.iter() {
            emit_scaled_svg_op(&mut ctx, op, scale, ox, oy, current_color);
        }
        ctx.into_commands()
    }
}

fn emit_scaled_svg_op(
    ctx: &mut CanvasContext2D,
    op: &SvgVectorOp,
    s: f32,
    ox: f32,
    oy: f32,
    current_color: Color,
) {
    match op {
        SvgVectorOp::FillRect {
            x,
            y,
            w,
            h,
            radius,
            color,
        } => {
            ctx.fill_rounded_rect(
                x * s + ox,
                y * s + oy,
                w * s,
                h * s,
                radius * s,
                color.resolve(current_color),
            );
        }
        SvgVectorOp::StrokeRect {
            x,
            y,
            w,
            h,
            line_width,
            color,
        } => {
            ctx.stroke_rect(
                x * s + ox,
                y * s + oy,
                w * s,
                h * s,
                (line_width * s).max(0.05),
                color.resolve(current_color),
            );
        }
        other => emit_scaled_svg_circle_or_path(ctx, other, s, ox, oy, current_color),
    }
}

fn emit_scaled_svg_circle_or_path(
    ctx: &mut CanvasContext2D,
    op: &SvgVectorOp,
    s: f32,
    ox: f32,
    oy: f32,
    current_color: Color,
) {
    match op {
        SvgVectorOp::FillCircle {
            cx,
            cy,
            radius,
            color,
        } => ctx.fill_circle(
            cx * s + ox,
            cy * s + oy,
            radius * s,
            color.resolve(current_color),
        ),
        SvgVectorOp::StrokeCircle {
            cx,
            cy,
            radius,
            line_width,
            color,
        } => ctx.stroke_circle(
            cx * s + ox,
            cy * s + oy,
            radius * s,
            (line_width * s).max(0.05),
            color.resolve(current_color),
        ),
        _ => emit_scaled_svg_path_op(ctx, op, s, ox, oy, current_color),
    }
}

fn emit_scaled_svg_path_op(
    ctx: &mut CanvasContext2D,
    op: &SvgVectorOp,
    s: f32,
    ox: f32,
    oy: f32,
    current_color: Color,
) {
    match op {
        SvgVectorOp::FillPath {
            segments,
            fill_rule,
            color,
        } => {
            ctx.set_fill_rule(*fill_rule);
            replay_scaled_path(ctx, segments, s, ox, oy);
            ctx.fill_path(color.resolve(current_color));
        }
        SvgVectorOp::StrokePath {
            segments,
            line_width,
            line_cap,
            line_join,
            color,
        } => {
            ctx.set_line_cap(*line_cap);
            ctx.set_line_join(*line_join);
            replay_scaled_path(ctx, segments, s, ox, oy);
            ctx.stroke_path((line_width * s).max(0.05), color.resolve(current_color));
        }
        _ => {}
    }
}

fn replay_scaled_path(
    ctx: &mut CanvasContext2D,
    segments: &[PathSegment],
    s: f32,
    ox: f32,
    oy: f32,
) {
    ctx.begin_path();
    for &seg in segments {
        match seg {
            PathSegment::MoveTo(x, y) => ctx.move_to(x * s + ox, y * s + oy),
            PathSegment::LineTo(x, y) => ctx.line_to(x * s + ox, y * s + oy),
            PathSegment::QuadTo { cx, cy, x, y } => {
                ctx.quad_to(cx * s + ox, cy * s + oy, x * s + ox, y * s + oy)
            }
            PathSegment::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                ctx.bezier_curve_to(
                    c1x * s + ox,
                    c1y * s + oy,
                    c2x * s + ox,
                    c2y * s + oy,
                    x * s + ox,
                    y * s + oy,
                );
            }
            PathSegment::Arc {
                cx,
                cy,
                rx,
                ry,
                start_rad,
                sweep_rad,
                x_rotation_rad,
            } => {
                ctx.ellipse_arc(
                    cx * s + ox,
                    cy * s + oy,
                    rx * s,
                    ry * s,
                    start_rad,
                    sweep_rad,
                    x_rotation_rad,
                );
            }
            PathSegment::Close => ctx.close_path(),
        }
    }
}

/// Computes destination rectangle and normalized UV crop `[u0, v0, u1, v1]` for `fit`.
pub fn compute_object_fit_placement(
    box_rect: LayoutRect,
    src_w: f32,
    src_h: f32,
    fit: ObjectFit,
) -> (LayoutRect, [f32; 4]) {
    if box_rect.width <= 0.0 || box_rect.height <= 0.0 || src_w <= 0.0 || src_h <= 0.0 {
        return (box_rect, [0.0, 0.0, 1.0, 1.0]);
    }
    match fit {
        ObjectFit::Fill => (box_rect, [0.0, 0.0, 1.0, 1.0]),
        ObjectFit::Contain | ObjectFit::ScaleDown => {
            let max_scale = if fit == ObjectFit::ScaleDown {
                1.0_f32
            } else {
                f32::INFINITY
            };
            let scale = (box_rect.width / src_w)
                .min(box_rect.height / src_h)
                .min(max_scale);
            let (dw, dh) = (src_w * scale, src_h * scale);
            (
                LayoutRect {
                    x: box_rect.x + (box_rect.width - dw) * 0.5,
                    y: box_rect.y + (box_rect.height - dh) * 0.5,
                    width: dw,
                    height: dh,
                },
                [0.0, 0.0, 1.0, 1.0],
            )
        }
        ObjectFit::Cover => {
            let box_ar = box_rect.width / box_rect.height;
            let src_ar = src_w / src_h;
            if (src_ar - box_ar).abs() < 1e-5 {
                (box_rect, [0.0, 0.0, 1.0, 1.0])
            } else if src_ar > box_ar {
                let vis_u = (box_ar / src_ar).clamp(0.0, 1.0);
                let pad = (1.0 - vis_u) * 0.5;
                (box_rect, [pad, 0.0, 1.0 - pad, 1.0])
            } else {
                let vis_v = (src_ar / box_ar).clamp(0.0, 1.0);
                let pad = (1.0 - vis_v) * 0.5;
                (box_rect, [0.0, pad, 1.0, 1.0 - pad])
            }
        }
        ObjectFit::None => compute_object_fit_none(box_rect, src_w, src_h),
    }
}

fn compute_object_fit_none(box_rect: LayoutRect, src_w: f32, src_h: f32) -> (LayoutRect, [f32; 4]) {
    let (dx, dw, u0, u1) = if src_w <= box_rect.width {
        (box_rect.x + (box_rect.width - src_w) * 0.5, src_w, 0.0, 1.0)
    } else {
        let vis_u = (box_rect.width / src_w).clamp(0.0, 1.0);
        let pad = (1.0 - vis_u) * 0.5;
        (box_rect.x, box_rect.width, pad, 1.0 - pad)
    };
    let (dy, dh, v0, v1) = if src_h <= box_rect.height {
        (
            box_rect.y + (box_rect.height - src_h) * 0.5,
            src_h,
            0.0,
            1.0,
        )
    } else {
        let vis_v = (box_rect.height / src_h).clamp(0.0, 1.0);
        let pad = (1.0 - vis_v) * 0.5;
        (box_rect.y, box_rect.height, pad, 1.0 - pad)
    };
    (
        LayoutRect {
            x: dx,
            y: dy,
            width: dw,
            height: dh,
        },
        [u0, v0, u1, v1],
    )
}
