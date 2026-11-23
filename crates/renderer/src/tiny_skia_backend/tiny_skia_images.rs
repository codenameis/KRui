//! Raster `<img>` drawing for `TinySkiaRenderer`.

use super::tiny_skia_clip_cache::ClipMaskCache;
use super::tiny_skia_shapes::{
    build_skia_round_rect, build_skia_round_rect_4, fill_skia_path_with_rule,
    stroke_skia_path_styled, to_skia_transform,
};
use krui_core::{
    CanvasCommand, CanvasFillRule, CanvasLineCap, CanvasLineJoin, DecodedImage, ImageId,
    ImageSource, PathSegment,
};
use krui_layout::LayoutRect;
use krui_style::Affine2;

const MAX_SKIA_IMAGE_CACHE_ENTRIES: usize = 64;
const MAX_SKIA_IMAGE_CACHE_BYTES: usize = 64 * 1024 * 1024;

struct CachedSkiaImage {
    id: ImageId,
    pm_pixmap: Option<tiny_skia::Pixmap>,
    is_opaque: bool,
    byte_size: usize,
    last_tick: u64,
}

/// Bounded LRU cache of premultiplied `tiny_skia::Pixmap`s for translucent `DecodedImage`s.
#[derive(Default)]
pub(super) struct SkiaImageCache {
    entries: Vec<CachedSkiaImage>,
    total_bytes: usize,
    tick: u64,
}

impl SkiaImageCache {
    fn resolve_or_premultiply(
        &mut self,
        img: &DecodedImage,
    ) -> Option<(bool, Option<&tiny_skia::Pixmap>)> {
        self.tick = self.tick.wrapping_add(1);
        if let Some(pos) = self.entries.iter().position(|e| e.id == img.id) {
            self.entries[pos].last_tick = self.tick;
            let entry = &self.entries[pos];
            return Some((entry.is_opaque, entry.pm_pixmap.as_ref()));
        }
        let is_opaque = img.rgba8.as_chunks::<4>().0.iter().all(|px| px[3] == 255);
        let (pm_pixmap, byte_size) = if is_opaque {
            (None, 0)
        } else {
            let size = tiny_skia::IntSize::from_wh(img.width, img.height)?;
            let pm_bytes = premultiply_rgba8(&img.rgba8);
            let b_len = pm_bytes.len();
            let px = tiny_skia::Pixmap::from_vec(pm_bytes, size)?;
            (Some(px), b_len)
        };
        while !self.entries.is_empty()
            && (self.entries.len() >= MAX_SKIA_IMAGE_CACHE_ENTRIES
                || self.total_bytes.saturating_add(byte_size) > MAX_SKIA_IMAGE_CACHE_BYTES)
        {
            if let Some((victim, _)) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_tick)
            {
                let evicted = self.entries.swap_remove(victim);
                self.total_bytes = self.total_bytes.saturating_sub(evicted.byte_size);
            } else {
                break;
            }
        }
        self.total_bytes = self.total_bytes.saturating_add(byte_size);
        self.entries.push(CachedSkiaImage {
            id: img.id,
            pm_pixmap,
            is_opaque,
            byte_size,
            last_tick: self.tick,
        });
        let last = self.entries.last()?;
        Some((last.is_opaque, last.pm_pixmap.as_ref()))
    }
}

/// Input parameters for rendering a `DrawPrimitive::Image` into a `tiny-skia` pixmap.
pub(super) struct ImageSkiaArgs<'a> {
    pub rect: LayoutRect,
    pub uv_rect: [f32; 4],
    pub clip: Option<LayoutRect>,
    pub clip_radius: f32,
    pub clip_radii: [f32; 4],
    pub transform: Affine2,
    pub opacity: f32,
    pub border_radius: f32,
    pub scale: f32,
    pub scale_tx: Affine2,
    pub source: &'a ImageSource,
}

/// Renders a decoded `<img>` into `pixmap` with `uv_rect` cropping and `border_radius` clipping.
pub(super) fn render_image_skia(
    pixmap: &mut tiny_skia::Pixmap,
    mask_cache: &mut ClipMaskCache,
    image_cache: &mut SkiaImageCache,
    a: ImageSkiaArgs<'_>,
) -> (usize, usize) {
    if a.rect.width <= 0.0 || a.rect.height <= 0.0 || a.opacity <= 0.0 {
        return (0, 0);
    }
    let img = match a.source {
        ImageSource::Decoded(img) => img,
        ImageSource::External { .. } => return (0, 0),
    };
    let Some(path) = build_skia_round_rect(
        a.rect.x,
        a.rect.y,
        a.rect.width,
        a.rect.height,
        a.border_radius,
    ) else {
        return (0, 0);
    };
    let mask = mask_cache.get_or_build_4(
        pixmap.width(),
        pixmap.height(),
        a.clip,
        a.clip_radius,
        a.clip_radii,
        a.scale,
    );
    let pattern_tx = compute_uv_pattern_transform(a.rect, a.uv_rect, img);
    let path_tx = to_skia_transform(a.scale_tx * a.transform);
    draw_decoded_image_pattern(
        pixmap,
        image_cache,
        img,
        &path,
        pattern_tx,
        path_tx,
        a.opacity,
        mask,
    );
    (4, 6)
}

fn compute_uv_pattern_transform(
    rect: LayoutRect,
    uv_rect: [f32; 4],
    img: &DecodedImage,
) -> tiny_skia::Transform {
    let w = img.width.max(1) as f32;
    let h = img.height.max(1) as f32;
    let src_x0 = uv_rect[0] * w;
    let src_y0 = uv_rect[1] * h;
    let src_w = ((uv_rect[2] - uv_rect[0]) * w).max(1e-4);
    let src_h = ((uv_rect[3] - uv_rect[1]) * h).max(1e-4);
    let kx = rect.width / src_w;
    let ky = rect.height / src_h;
    tiny_skia::Transform::from_row(kx, 0.0, 0.0, ky, rect.x - kx * src_x0, rect.y - ky * src_y0)
}

#[allow(clippy::too_many_arguments)]
fn draw_decoded_image_pattern(
    dst: &mut tiny_skia::Pixmap,
    image_cache: &mut SkiaImageCache,
    img: &DecodedImage,
    path: &tiny_skia::Path,
    pattern_tx: tiny_skia::Transform,
    path_tx: tiny_skia::Transform,
    opacity: f32,
    mask: Option<&tiny_skia::Mask>,
) {
    let Some((is_opaque, pm_opt)) = image_cache.resolve_or_premultiply(img) else {
        return;
    };
    if is_opaque {
        if let Some(src_ref) = tiny_skia::PixmapRef::from_bytes(&img.rgba8, img.width, img.height) {
            fill_with_pixmap(dst, src_ref, path, pattern_tx, path_tx, opacity, mask);
        }
    } else if let Some(src_pm) = pm_opt {
        fill_with_pixmap(
            dst,
            src_pm.as_ref(),
            path,
            pattern_tx,
            path_tx,
            opacity,
            mask,
        );
    }
}

pub(super) type LayerCompositeClip = (
    Option<LayoutRect>,
    f32,
    [f32; 4],
    Option<(LayoutRect, [f32; 4], Affine2)>,
);

pub(super) fn composite_layer_skia(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    mask_cache: &mut ClipMaskCache,
    (clip, clip_radius, clip_radii, local_clip): LayerCompositeClip,
    opacity: f32,
    scale: f32,
) {
    if opacity <= 0.0 {
        return;
    }
    let mask = mask_cache.get_or_build_4(
        dst.width(),
        dst.height(),
        clip,
        clip_radius,
        clip_radii,
        scale,
    );
    if let Some((lrect, lradii, ltx)) = local_clip {
        let Some(path) =
            build_skia_round_rect_4(lrect.x, lrect.y, lrect.width, lrect.height, lradii)
        else {
            return;
        };
        let full_tx = Affine2::scale(scale, scale) * ltx;
        let Some(inv_tx) = full_tx.inverse() else {
            return;
        };
        fill_with_pixmap(
            dst,
            src.as_ref(),
            &path,
            to_skia_transform(inv_tx),
            to_skia_transform(full_tx),
            opacity,
            mask,
        );
        return;
    }
    let paint = tiny_skia::PixmapPaint {
        opacity: opacity.clamp(0.0, 1.0),
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    dst.draw_pixmap(
        0,
        0,
        src.as_ref(),
        &paint,
        tiny_skia::Transform::identity(),
        mask,
    );
}

fn fill_with_pixmap(
    dst: &mut tiny_skia::Pixmap,
    src: tiny_skia::PixmapRef<'_>,
    path: &tiny_skia::Path,
    pattern_tx: tiny_skia::Transform,
    path_tx: tiny_skia::Transform,
    opacity: f32,
    mask: Option<&tiny_skia::Mask>,
) {
    let paint = tiny_skia::Paint {
        shader: tiny_skia::Pattern::new(
            src,
            tiny_skia::SpreadMode::Pad,
            tiny_skia::FilterQuality::Bilinear,
            opacity.clamp(0.0, 1.0),
            pattern_tx,
        ),
        anti_alias: true,
        ..Default::default()
    };
    dst.fill_path(path, &paint, tiny_skia::FillRule::Winding, path_tx, mask);
}

fn premultiply_rgba8(rgba8: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba8.len());
    for px in rgba8.as_chunks::<4>().0 {
        let a = px[3] as u16;
        if a == 255 {
            out.extend_from_slice(px);
        } else if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            out.push(((px[0] as u16 * a + 127) / 255) as u8);
            out.push(((px[1] as u16 * a + 127) / 255) as u8);
            out.push(((px[2] as u16 * a + 127) / 255) as u8);
            out.push(a as u8);
        }
    }
    out
}

pub(super) fn build_skia_path(segments: &[PathSegment]) -> Option<tiny_skia::Path> {
    let mut pb = tiny_skia::PathBuilder::new();
    let mut has_subpath = false;
    for seg in segments {
        match *seg {
            PathSegment::MoveTo(x, y) => {
                pb.move_to(x, y);
                has_subpath = true;
            }
            PathSegment::LineTo(x, y) => {
                pb.line_to(x, y);
                has_subpath = true;
            }
            PathSegment::QuadTo { cx, cy, x, y } => {
                pb.quad_to(cx, cy, x, y);
                has_subpath = true;
            }
            PathSegment::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                pb.cubic_to(c1x, c1y, c2x, c2y, x, y);
                has_subpath = true;
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
                append_skia_arc(
                    &mut pb,
                    &mut has_subpath,
                    (cx, cy, rx, ry),
                    (start_rad, sweep_rad, x_rotation_rad),
                );
            }
            PathSegment::Close => {
                pb.close();
                has_subpath = false;
            }
        }
    }
    pb.finish()
}

fn append_skia_arc(
    pb: &mut tiny_skia::PathBuilder,
    has_subpath: &mut bool,
    (cx, cy, rx, ry): (f32, f32, f32, f32),
    (start_rad, sweep_rad, x_rotation_rad): (f32, f32, f32),
) {
    let ((sx, sy), cubics) = crate::tessellate_primitives::flatten_arc_to_cubics(
        cx,
        cy,
        rx,
        ry,
        start_rad,
        sweep_rad,
        x_rotation_rad,
    );
    if *has_subpath {
        pb.line_to(sx, sy);
    } else {
        pb.move_to(sx, sy);
        *has_subpath = true;
    }
    for [c1, c2, end] in cubics {
        pb.cubic_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
    }
}

fn to_skia_fill_rule(rule: CanvasFillRule) -> tiny_skia::FillRule {
    match rule {
        CanvasFillRule::NonZero => tiny_skia::FillRule::Winding,
        CanvasFillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
    }
}

pub(super) fn render_canvas_stroke_or_path_skia(
    pixmap: &mut tiny_skia::Pixmap,
    cmd: &CanvasCommand,
    skia_tx: tiny_skia::Transform,
    opacity: f32,
    mask: Option<&tiny_skia::Mask>,
) {
    match *cmd {
        CanvasCommand::StrokeRect {
            x,
            y,
            w,
            h,
            line_width,
            color,
        } if w > 0.0 && h > 0.0 && line_width > 0.0 => {
            if let Some(r) = tiny_skia::Rect::from_xywh(x, y, w, h) {
                let p = tiny_skia::PathBuilder::from_rect(r);
                let (cap, join) = (CanvasLineCap::Butt, CanvasLineJoin::Miter);
                stroke_skia_path_styled(
                    pixmap, &p, color, opacity, line_width, cap, join, skia_tx, mask,
                );
            }
        }
        CanvasCommand::StrokeLine {
            x1,
            y1,
            x2,
            y2,
            line_width,
            line_cap,
            color,
        } => {
            let mut pb = tiny_skia::PathBuilder::new();
            pb.move_to(x1, y1);
            pb.line_to(x2, y2);
            if let Some(p) = pb.finish() {
                let join = CanvasLineJoin::Miter;
                stroke_skia_path_styled(
                    pixmap, &p, color, opacity, line_width, line_cap, join, skia_tx, mask,
                );
            }
        }
        _ => render_canvas_path_skia(pixmap, cmd, skia_tx, opacity, mask),
    }
}

fn render_canvas_path_skia(
    pixmap: &mut tiny_skia::Pixmap,
    cmd: &CanvasCommand,
    skia_tx: tiny_skia::Transform,
    opacity: f32,
    mask: Option<&tiny_skia::Mask>,
) {
    match *cmd {
        CanvasCommand::FillPath {
            ref segments,
            fill_rule,
            color,
        } => {
            if let Some(p) = build_skia_path(segments) {
                let rule = to_skia_fill_rule(fill_rule);
                fill_skia_path_with_rule(pixmap, &p, color, opacity, rule, skia_tx, mask);
            }
        }
        CanvasCommand::StrokePath {
            ref segments,
            line_width,
            line_cap,
            line_join,
            color,
        } => {
            if let Some(p) = build_skia_path(segments) {
                stroke_skia_path_styled(
                    pixmap, &p, color, opacity, line_width, line_cap, line_join, skia_tx, mask,
                );
            }
        }
        _ => {}
    }
}
