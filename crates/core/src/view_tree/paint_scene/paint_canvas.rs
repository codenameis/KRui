//! `<canvas>` 2D command recording, media (`<img>`/`<svg>`) scene emission, and control indicators.

use crate::media::compute_object_fit_placement;
use crate::scene::{
    CanvasCommand, CanvasContext2D, CanvasDraw, CanvasRedrawMode, DrawPrimitive, PaintScene, Size,
};
use crate::view_tree::clip_shape::ClipState;
use crate::view_tree::content_box::content_box_rect;
use crate::view_tree::controls::{
    parse_and_clamp_range_value, SLIDER_THUMB_SIZE_PX, SLIDER_TRACK_HEIGHT_PX,
};
use crate::view_tree::paint::push_flat_box;
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_style::{Affine2, BoxShadow, Color, LayoutStyle, PaintStyle, UA_SLIDER_TRACK_COLOR};
use std::sync::Arc;

/// Evaluates dirty or per-frame `<canvas>` callbacks outside `ViewTreeInner`'s `RefCell` borrow
/// so user draw callbacks can safely read `ViewTree` queries without `BorrowMutError`.
pub(crate) fn refresh_dirty_canvases(tree: &ViewTree) {
    let pending: Vec<(NodeId, CanvasDraw, Size)> = {
        let inner = tree.inner.borrow();
        inner
            .sparse
            .canvases
            .iter()
            .filter(|(_, cv)| cv.canvas_mode == CanvasRedrawMode::PerFrame || cv.canvas_dirty)
            .filter_map(|(&nid, cv)| {
                let node = inner.node(nid)?;
                let cb = cv.canvas_callback.clone()?;
                Some((
                    nid,
                    cb,
                    Size::new(node.world_rect.width, node.world_rect.height),
                ))
            })
            .collect()
    };
    for (nid, cb, size) in pending {
        let mut ctx = CanvasContext2D::with_size(size.width, size.height);
        cb.draw(&mut ctx, size);
        let cmds: Arc<[CanvasCommand]> = Arc::from(ctx.into_commands());
        let mut inner = tree.inner.borrow_mut();
        if inner.node(nid).is_some() {
            if let Some(cv) = inner.sparse.canvases.get_mut(&nid) {
                cv.canvas_commands = cmds;
                cv.canvas_dirty = false;
            }
        }
    }
}

pub(crate) fn emit_node_canvas_primitives(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    world_rect: LayoutRect,
    clip: ClipState,
    effective_transform: Affine2,
    effective_opacity: f32,
    scene: &mut PaintScene,
) {
    if inner.node(node_id).is_none() {
        return;
    }
    crate::view_tree::paint_scene::paint_canvas_rich::emit_canvas_commands_to_scene(
        inner,
        node_id,
        world_rect,
        clip,
        effective_transform,
        effective_opacity,
        scene,
    );
}

fn push_indicator_rect(
    scene: &mut PaintScene,
    rect: LayoutRect,
    clip: ClipState,
    transform: Affine2,
    opacity: f32,
    color: Color,
    radius: f32,
) {
    push_flat_box(
        scene,
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        clip,
        transform,
        opacity,
        color,
        radius,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_control_indicator_primitives(
    inner: &ViewTreeInner,
    node_id: NodeId,
    world_rect: LayoutRect,
    layout: &LayoutStyle,
    clip: ClipState,
    tf: Affine2,
    op: f32,
    paint: &PaintStyle,
    scene: &mut PaintScene,
) {
    if paint.appearance_none {
        return;
    }
    let Some(node) = inner.node(node_id) else {
        return;
    };
    let cb = content_box_rect(world_rect, layout);
    let has_checked_bg = paint.background_color.a > 0;
    match node.role {
        SemanticRole::Checkbox if node.is_checked() => {
            let mark_color = if paint.author_accent_color || !has_checked_bg {
                push_indicator_rect(
                    scene,
                    world_rect,
                    clip,
                    tf,
                    op,
                    paint.resolve_accent_color(),
                    paint.effective_corner_radii().max_radius(),
                );
                paint.resolve_accent_foreground()
            } else {
                paint.text_color
            };
            emit_checkbox_indicator(world_rect, cb, clip, tf, op, mark_color, scene);
        }
        SemanticRole::RadioButton if node.is_checked() => {
            emit_radio_indicator(world_rect, cb, has_checked_bg, clip, tf, op, paint, scene);
        }
        SemanticRole::Slider => {
            let bounds = inner
                .control(node_id)
                .map_or((0.0, 100.0, 1.0), |c| c.range_bounds);
            emit_slider_primitives(&node.text, bounds, world_rect, clip, tf, op, paint, scene);
        }
        SemanticRole::ComboBox => {
            emit_combo_chevron(world_rect, clip, tf, op, paint.text_color, scene);
        }
        _ => emit_progress_indicator(inner, node_id, cb, clip, tf, op, paint, scene),
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_radio_indicator(
    world_rect: LayoutRect,
    cb: LayoutRect,
    has_checked_bg: bool,
    clip: ClipState,
    tf: Affine2,
    op: f32,
    paint: &PaintStyle,
    scene: &mut PaintScene,
) {
    let diam = (cb.width.min(cb.height) * 0.78).max(2.0);
    let rect = LayoutRect {
        x: cb.x + (cb.width - diam) * 0.5,
        y: cb.y + (cb.height - diam) * 0.5,
        width: diam,
        height: diam,
    };
    let dot_color = if has_checked_bg && paint.author_accent_color {
        push_indicator_rect(
            scene,
            world_rect,
            clip,
            tf,
            op,
            paint.resolve_accent_color(),
            paint.effective_corner_radii().max_radius(),
        );
        paint.resolve_accent_foreground()
    } else if !has_checked_bg || paint.author_accent_color {
        paint.resolve_accent_color()
    } else {
        paint.text_color
    };
    push_indicator_rect(scene, rect, clip, tf, op, dot_color, diam * 0.5);
}

#[allow(clippy::too_many_arguments)]
fn emit_progress_indicator(
    inner: &ViewTreeInner,
    node_id: NodeId,
    cb: LayoutRect,
    clip: ClipState,
    tf: Affine2,
    op: f32,
    paint: &PaintStyle,
    scene: &mut PaintScene,
) {
    if let Some(ratio) = crate::view_tree::semantics::progress_fill_ratio(inner, node_id) {
        let fill_w = cb.width * ratio;
        if fill_w > 0.5 {
            let fill_rect = LayoutRect {
                width: fill_w,
                ..cb
            };
            push_indicator_rect(
                scene,
                fill_rect,
                clip,
                tf,
                op,
                paint.resolve_accent_color(),
                paint.effective_corner_radii().max_radius(),
            );
        }
    }
}

fn emit_checkbox_indicator(
    world_rect: LayoutRect,
    cb: LayoutRect,
    clip: ClipState,
    tf: Affine2,
    op: f32,
    color: Color,
    scene: &mut PaintScene,
) {
    let (w, h) = (world_rect.width, world_rect.height);
    let (ox, oy) = (cb.x - world_rect.x, cb.y - world_rect.y);
    let (cw, ch) = (cb.width.max(2.0), cb.height.max(2.0));
    let mut ctx = CanvasContext2D::with_size(w, h);
    ctx.set_line_cap(crate::scene::CanvasLineCap::Round);
    ctx.set_line_join(crate::scene::CanvasLineJoin::Round);
    ctx.begin_path();
    ctx.move_to(ox + cw * 0.14, oy + ch * 0.52);
    ctx.line_to(ox + cw * 0.40, oy + ch * 0.78);
    ctx.line_to(ox + cw * 0.86, oy + ch * 0.22);
    ctx.stroke_path((cw.min(ch) * 0.16).max(1.5), color);
    scene.primitives.push(DrawPrimitive::CanvasBatch {
        viewport: world_rect,
        clip: clip.rect,
        clip_radius: clip.max_radius(),
        clip_radii: clip.radii,
        transform: tf,
        opacity: op,
        commands: Arc::from(ctx.into_commands()),
    });
}

fn emit_combo_chevron(
    world_rect: LayoutRect,
    clip: ClipState,
    tf: Affine2,
    op: f32,
    color: Color,
    scene: &mut PaintScene,
) {
    let (w, h) = (world_rect.width, world_rect.height);
    let cx = (w - 11.0).max(w * 0.5);
    let cy = h * 0.5;
    let mut ctx = CanvasContext2D::with_size(w, h);
    ctx.set_line_cap(crate::scene::CanvasLineCap::Round);
    ctx.set_line_join(crate::scene::CanvasLineJoin::Round);
    ctx.begin_path();
    ctx.move_to(cx - 3.5, cy - 1.5);
    ctx.line_to(cx, cy + 2.5);
    ctx.line_to(cx + 3.5, cy - 1.5);
    ctx.stroke_path(1.5, color);
    scene.primitives.push(DrawPrimitive::CanvasBatch {
        viewport: world_rect,
        clip: clip.rect,
        clip_radius: clip.max_radius(),
        clip_radii: clip.radii,
        transform: tf,
        opacity: op * 0.78,
        commands: Arc::from(ctx.into_commands()),
    });
}

#[allow(clippy::too_many_arguments)]
fn emit_slider_primitives(
    text: &str,
    (min, max, step): (f32, f32, f32),
    world_rect: LayoutRect,
    clip: ClipState,
    tf: Affine2,
    op: f32,
    paint: &PaintStyle,
    scene: &mut PaintScene,
) {
    let val = parse_and_clamp_range_value(text, (min, max, step));
    let ratio = ((val - min) / (max.max(min) - min).max(0.000_001)).clamp(0.0, 1.0);
    let pad = (SLIDER_THUMB_SIZE_PX * 0.5).min(world_rect.width * 0.2);
    let track_x = world_rect.x + pad;
    let track_w = (world_rect.width - pad * 2.0).max(2.0);
    let tr = SLIDER_TRACK_HEIGHT_PX * 0.5;
    let track_color = if paint.background_color.a > 0 {
        paint.background_color
    } else {
        UA_SLIDER_TRACK_COLOR
    };
    let accent = paint.resolve_accent_color();
    let track_rect = LayoutRect {
        x: track_x,
        y: world_rect.y + (world_rect.height - SLIDER_TRACK_HEIGHT_PX) * 0.5,
        width: track_w,
        height: SLIDER_TRACK_HEIGHT_PX,
    };
    push_indicator_rect(scene, track_rect, clip, tf, op, track_color, tr);
    let fill_w = (track_w * ratio).max(0.0);
    if fill_w > 0.5 {
        let fill_rect = LayoutRect {
            width: fill_w,
            ..track_rect
        };
        push_indicator_rect(scene, fill_rect, clip, tf, op, accent, tr);
    }
    let thumb_rect = LayoutRect {
        x: track_x + fill_w - SLIDER_THUMB_SIZE_PX * 0.5,
        y: world_rect.y + (world_rect.height - SLIDER_THUMB_SIZE_PX) * 0.5,
        width: SLIDER_THUMB_SIZE_PX,
        height: SLIDER_THUMB_SIZE_PX,
    };
    push_indicator_rect(
        scene,
        thumb_rect,
        clip,
        tf,
        op,
        accent,
        SLIDER_THUMB_SIZE_PX * 0.5,
    );
}

/// Emits `DrawPrimitive::Image` and/or `DrawPrimitive::CanvasBatch` for `<img>`, `ExternalTexture`, and `<svg>`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_node_media_primitives(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    layout: &LayoutStyle,
    world_rect: LayoutRect,
    clip: ClipState,
    tf: Affine2,
    op: f32,
    eff_paint: &PaintStyle,
    scene: &mut PaintScene,
) {
    let content_box = content_box_rect(world_rect, layout);
    if content_box.width <= 0.0 || content_box.height <= 0.0 {
        return;
    }
    let image_source = inner
        .media(node_id)
        .and_then(|m| m.image_source.as_ref().cloned());
    if let Some(source) = image_source {
        let (sw, sh) = source.intrinsic_dimensions();
        let (dest_rect, uv_rect) =
            compute_object_fit_placement(content_box, sw as f32, sh as f32, eff_paint.object_fit);
        scene.primitives.push(DrawPrimitive::Image {
            rect: dest_rect,
            uv_rect,
            clip: clip.rect,
            clip_radius: clip.max_radius(),
            clip_radii: clip.radii,
            transform: tf,
            opacity: op,
            border_radius: eff_paint.border_radius,
            source,
        });
    }
    emit_node_svg_commands(
        inner,
        node_id,
        content_box,
        clip,
        tf,
        op,
        eff_paint.text_color,
        scene,
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_node_svg_commands(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    content_box: LayoutRect,
    clip: ClipState,
    tf: Affine2,
    op: f32,
    fg: Color,
    scene: &mut PaintScene,
) {
    let commands = {
        let Some(media) = inner.sparse.media.get_mut(&node_id) else {
            return;
        };
        let Some(svg) = media.svg_template.as_ref() else {
            return;
        };
        let qw = (content_box.width.max(0.0) * 100.0).round() as u32;
        let qh = (content_box.height.max(0.0) * 100.0).round() as u32;
        let cache_hit = media
            .svg_cached
            .as_ref()
            .filter(|(cw, ch, cc, _)| *cw == qw && *ch == qh && *cc == fg)
            .map(|(_, _, _, cmds)| cmds.clone());
        match cache_hit {
            Some(cmds) => cmds,
            None => {
                let cmds: Arc<[CanvasCommand]> = Arc::from(svg.to_canvas_commands(content_box, fg));
                media.svg_cached = Some((qw, qh, fg, cmds.clone()));
                cmds
            }
        }
    };
    if !commands.is_empty() {
        scene.primitives.push(DrawPrimitive::CanvasBatch {
            viewport: content_box,
            clip: clip.rect,
            clip_radius: clip.max_radius(),
            clip_radii: clip.radii,
            transform: tf,
            opacity: op,
            commands,
        });
    }
}

/// Expands `aabb` to enclose all outer CSS `box-shadow` layers on `paint`.
pub(crate) fn pad_aabb_for_paint_shadows(
    aabb: LayoutRect,
    paint: &krui_style::PaintStyle,
) -> LayoutRect {
    if paint.shadow_stack.is_empty() {
        return pad_aabb_for_box_shadow(aabb, paint.box_shadow);
    }
    let (mut min_x, mut min_y) = (aabb.x, aabb.y);
    let (mut max_x, mut max_y) = (aabb.x + aabb.width, aabb.y + aabb.height);
    for &sh in paint.shadow_stack.as_slice() {
        if !sh.inset {
            let r = pad_aabb_for_box_shadow(aabb, Some(sh));
            min_x = min_x.min(r.x);
            min_y = min_y.min(r.y);
            max_x = max_x.max(r.x + r.width);
            max_y = max_y.max(r.y + r.height);
        }
    }
    LayoutRect {
        x: min_x,
        y: min_y,
        width: (max_x - min_x).max(0.0),
        height: (max_y - min_y).max(0.0),
    }
}

/// Expands an AABB to enclose any CSS `box_shadow` extent so offscreen-rect boxes whose shadow
/// enters the clip viewport are not prematurely culled.
pub(crate) fn pad_aabb_for_box_shadow(aabb: LayoutRect, shadow: Option<BoxShadow>) -> LayoutRect {
    let Some(s) = shadow else {
        return aabb;
    };
    let blur_and_spread = s.blur_radius.max(0.0) + s.spread_radius.max(0.0);
    let pad_left = (blur_and_spread - s.offset_x).max(0.0);
    let pad_right = (blur_and_spread + s.offset_x).max(0.0);
    let pad_top = (blur_and_spread - s.offset_y).max(0.0);
    let pad_bottom = (blur_and_spread + s.offset_y).max(0.0);
    LayoutRect {
        x: aabb.x - pad_left,
        y: aabb.y - pad_top,
        width: aabb.width + pad_left + pad_right,
        height: aabb.height + pad_top + pad_bottom,
    }
}

pub(super) fn emit_node_indicator_or_canvas_primitives(
    inner: &mut ViewTreeInner,
    f: &crate::view_tree::paint::NodePaintFrame,
    is_canvas: bool,
    scene: &mut PaintScene,
) {
    emit_control_indicator_primitives(
        inner,
        f.node_id,
        f.world_rect,
        &f.layout,
        f.clip,
        f.effective_transform,
        f.effective_opacity,
        &f.effective_paint,
        scene,
    );
    emit_node_media_primitives(
        inner,
        f.node_id,
        &f.layout,
        f.world_rect,
        f.clip,
        f.effective_transform,
        f.effective_opacity,
        &f.effective_paint,
        scene,
    );
    if is_canvas {
        emit_node_canvas_primitives(
            inner,
            f.node_id,
            f.world_rect,
            f.clip,
            f.effective_transform,
            f.effective_opacity,
            scene,
        );
    }
}
