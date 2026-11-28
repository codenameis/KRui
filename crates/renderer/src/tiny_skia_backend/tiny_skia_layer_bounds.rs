//! Conservative physical dirty-region estimation and subrect layer pooling
//! helpers for `TinySkiaRenderer` offscreen compositing.

use krui_core::DrawPrimitive;
use krui_layout::LayoutRect;
use krui_style::Affine2;

pub(super) fn acquire_pooled_layer_pixmap(
    pool: &mut Vec<(tiny_skia::Pixmap, Option<[u32; 4]>)>,
    w: u32,
    h: u32,
) -> Option<tiny_skia::Pixmap> {
    if let Some((mut px, dirty)) = pool.pop() {
        if px.width() == w && px.height() == h {
            if let Some(bounds) = dirty {
                zero_pixmap_subrect(&mut px, bounds);
            }
            return Some(px);
        }
    }
    tiny_skia::Pixmap::new(w, h)
}

fn zero_pixmap_subrect(pixmap: &mut tiny_skia::Pixmap, bounds: [u32; 4]) {
    let (pw, ph) = (pixmap.width(), pixmap.height());
    let (x0, y0) = (bounds[0].min(pw), bounds[1].min(ph));
    let (x1, y1) = (bounds[2].min(pw), bounds[3].min(ph));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let row_bytes = (pw as usize) * 4;
    let x_start = (x0 as usize) * 4;
    let x_end = (x1 as usize) * 4;
    let data = pixmap.data_mut();
    for y in y0..y1 {
        let base = (y as usize) * row_bytes;
        data[base + x_start..base + x_end].fill(0);
    }
}

pub(super) fn union_bounds(a: Option<[u32; 4]>, b: Option<[u32; 4]>) -> Option<[u32; 4]> {
    match (a, b) {
        (Some(x), Some(y)) => Some([
            x[0].min(y[0]),
            x[1].min(y[1]),
            x[2].max(y[2]),
            x[3].max(y[3]),
        ]),
        (Some(x), None) => Some(x),
        (None, y) => y,
    }
}

pub(super) fn intersect_bounds(a: [u32; 4], b: [u32; 4]) -> Option<[u32; 4]> {
    let (x0, y0) = (a[0].max(b[0]), a[1].max(b[1]));
    let (x1, y1) = (a[2].min(b[2]), a[3].min(b[3]));
    (x0 < x1 && y0 < y1).then_some([x0, y0, x1, y1])
}

pub(super) fn clip_rect_phys(
    c: LayoutRect,
    scale: f32,
    (pw, ph): (u32, u32),
    pad: f32,
) -> Option<[u32; 4]> {
    let x0 = ((c.x * scale - pad).floor() as i32).clamp(0, pw as i32) as u32;
    let y0 = ((c.y * scale - pad).floor() as i32).clamp(0, ph as i32) as u32;
    let x1 = (((c.x + c.width) * scale + pad).ceil() as i32).clamp(0, pw as i32) as u32;
    let y1 = (((c.y + c.height) * scale + pad).ceil() as i32).clamp(0, ph as i32) as u32;
    (x0 < x1 && y0 < y1).then_some([x0, y0, x1, y1])
}

fn transformed_rect_phys(
    r: LayoutRect,
    pad: f32,
    tx: Affine2,
    scale: f32,
    (pw, ph): (u32, u32),
) -> Option<[u32; 4]> {
    let pts = [
        tx.transform_point(r.x - pad, r.y - pad),
        tx.transform_point(r.x + r.width + pad, r.y - pad),
        tx.transform_point(r.x + r.width + pad, r.y + r.height + pad),
        tx.transform_point(r.x - pad, r.y + r.height + pad),
    ];
    let min_x = (pts.iter().map(|p| p.0).fold(f32::INFINITY, f32::min) * scale).floor() as i32;
    let min_y = (pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min) * scale).floor() as i32;
    let max_x = (pts.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max) * scale).ceil() as i32;
    let max_y = (pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max) * scale).ceil() as i32;
    let x0 = (min_x - 1).clamp(0, pw as i32) as u32;
    let y0 = (min_y - 1).clamp(0, ph as i32) as u32;
    let x1 = (max_x + 1).clamp(0, pw as i32) as u32;
    let y1 = (max_y + 1).clamp(0, ph as i32) as u32;
    (x0 < x1 && y0 < y1).then_some([x0, y0, x1, y1])
}

fn glyph_bounds_phys(
    ox: f32,
    oy: f32,
    glyphs: &[krui_text::RasterizedGlyph],
    tx: Affine2,
    scale: f32,
    vp: (u32, u32),
) -> Option<[u32; 4]> {
    if glyphs.is_empty() {
        return None;
    }
    let (mut lx0, mut ly0, mut lx1, mut ly1) = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for g in glyphs {
        let (gx, gy) = (ox + g.x, oy + g.y);
        lx0 = lx0.min(gx);
        ly0 = ly0.min(gy);
        lx1 = lx1.max(gx + g.width as f32);
        ly1 = ly1.max(gy + g.height as f32);
    }
    let r = LayoutRect {
        x: lx0,
        y: ly0,
        width: (lx1 - lx0).max(0.0),
        height: (ly1 - ly0).max(0.0),
    };
    transformed_rect_phys(r, 2.0, tx, scale, vp)
}

pub(super) fn estimate_prim_phys_bounds(
    prim: &DrawPrimitive,
    scale: f32,
    vp: (u32, u32),
) -> Option<[u32; 4]> {
    let (mut bounds, clip) = match prim {
        DrawPrimitive::StyledBox {
            rect,
            clip,
            transform,
            border_width,
            box_shadow,
            ..
        } => {
            let sh = box_shadow
                .filter(|s| !s.inset)
                .map(|s| {
                    s.blur_radius.max(0.0) * 2.5
                        + s.spread_radius.max(0.0)
                        + s.offset_x.abs().max(s.offset_y.abs())
                })
                .unwrap_or(0.0);
            let pad = border_width.max(0.0) + sh + 4.0;
            (
                transformed_rect_phys(*rect, pad, *transform, scale, vp),
                *clip,
            )
        }
        DrawPrimitive::Image {
            rect,
            clip,
            transform,
            ..
        } => (
            transformed_rect_phys(*rect, 4.0, *transform, scale, vp),
            *clip,
        ),
        DrawPrimitive::TextGlyphs {
            origin_x,
            origin_y,
            clip,
            transform,
            glyphs,
            ..
        } => (
            glyph_bounds_phys(*origin_x, *origin_y, glyphs, *transform, scale, vp),
            *clip,
        ),
        DrawPrimitive::CanvasBatch { clip, .. } => (Some([0, 0, vp.0, vp.1]), *clip),
        DrawPrimitive::PushLayer { .. } | DrawPrimitive::PopLayer => return None,
    };
    if let (Some(b), Some(c)) = (bounds, clip) {
        let cb = clip_rect_phys(c, scale, vp, 2.0)?;
        bounds = intersect_bounds(b, cb);
    }
    bounds
}
