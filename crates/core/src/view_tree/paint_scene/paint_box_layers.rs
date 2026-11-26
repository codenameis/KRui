//! Non-layout decorative box layers emitted after the base node `StyledBox`:
//! 1. Bottom accent indicator strip (`border_bottom_width` / `border_bottom_color`)
//! 2. Non-layout focus/accent ring (`outline_width` / `outline_offset` / `outline_color`)

use crate::scene::{CanvasCommand, DrawPrimitive, PaintScene, PathSegment};
use crate::view_tree::clip_shape::ClipState;
use crate::view_tree::paint::{push_flat_box, NodePaintFrame};
use crate::view_tree::scroll::{compute_scrollbar_geometry, SCROLLBAR_THUMB_OPACITY_FACTOR};
use krui_layout::LayoutRect;
use krui_style::{Affine2, Color, LayoutStyle, PaintStyle};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
use std::sync::Arc;

fn has_non_uniform_per_side_border(p: &PaintStyle, l: &LayoutStyle) -> bool {
    let e = p.effective_border_edges(l);
    if e.top <= 0.0 && e.right <= 0.0 && e.bottom <= 0.0 && e.left <= 0.0 {
        return false;
    }
    let uniform_w = (e.top - e.right).abs() < 1e-4
        && (e.top - e.bottom).abs() < 1e-4
        && (e.top - e.left).abs() < 1e-4;
    let uniform_c = p
        .border_side_colors
        .is_none_or(|c| c[0] == c[1] && c[0] == c[2] && c[0] == c[3]);
    !(uniform_w && uniform_c)
}

/// Returns `true` if `p` or `l` emits any extra decorative box layer beyond base background/uniform border/shadow.
pub(crate) fn paint_has_extra_box_layers(p: &PaintStyle, l: &LayoutStyle) -> bool {
    (p.outline_width > 0.0 && p.outline_color.a > 0) || has_non_uniform_per_side_border(p, l)
}

/// Emits per-side borders and non-layout outer ring onto `scene`.
pub(crate) fn emit_extra_box_layers(
    world_rect: LayoutRect,
    clip: ClipState,
    transform: Affine2,
    opacity: f32,
    p: &PaintStyle,
    l: &LayoutStyle,
    scene: &mut PaintScene,
) {
    let corners = p.effective_corner_radii().to_array();
    if has_non_uniform_per_side_border(p, l) {
        emit_per_side_borders(world_rect, clip, transform, opacity, p, l, scene);
    }
    emit_outline_layer(world_rect, clip, transform, opacity, p, corners, scene);
}

fn normalize_box_radii(w: f32, h: f32, radii: [f32; 4]) -> [f32; 4] {
    let r = radii.map(|v| v.max(0.0));
    if w <= 0.0 || h <= 0.0 {
        return [0.0; 4];
    }
    let f = [
        w / (r[0] + r[1]).max(1e-4),
        w / (r[3] + r[2]).max(1e-4),
        h / (r[0] + r[3]).max(1e-4),
        h / (r[1] + r[2]).max(1e-4),
    ]
    .into_iter()
    .fold(1.0_f32, f32::min);
    r.map(|v| v * f)
}

fn append_corner(segs: &mut Vec<PathSegment>, cx: f32, cy: f32, r_mid: f32, start_rad: f32) {
    segs.push(PathSegment::Arc {
        cx,
        cy,
        rx: r_mid,
        ry: r_mid,
        start_rad,
        sweep_rad: FRAC_PI_4,
        x_rotation_rad: 0.0,
    });
}

fn build_rounded_side_segments(
    side: usize,
    (wb, hb, hw): (f32, f32, f32),
    [tl, tr, br, bl]: [f32; 4],
) -> Vec<PathSegment> {
    let ((r0, c0, a0), m0, (p0, p1), (r1, c1, a1)) = match side {
        0 => (
            (tl, (tl, tl), -3.0 * FRAC_PI_4),
            (0.0, hw),
            (
                (if tl > hw + 0.25 { tl } else { 0.0 }, hw),
                (if tr > hw + 0.25 { wb - tr } else { wb }, hw),
            ),
            (tr, (wb - tr, tr), -FRAC_PI_2),
        ),
        1 => (
            (tr, (wb - tr, tr), -FRAC_PI_4),
            (wb - hw, 0.0),
            (
                (wb - hw, if tr > hw + 0.25 { tr } else { 0.0 }),
                (wb - hw, if br > hw + 0.25 { hb - br } else { hb }),
            ),
            (br, (wb - br, hb - br), 0.0),
        ),
        2 => (
            (br, (wb - br, hb - br), FRAC_PI_4),
            (wb, hb - hw),
            (
                (if br > hw + 0.25 { wb - br } else { wb }, hb - hw),
                (if bl > hw + 0.25 { bl } else { 0.0 }, hb - hw),
            ),
            (bl, (bl, hb - bl), FRAC_PI_2),
        ),
        _ => (
            (bl, (bl, hb - bl), 3.0 * FRAC_PI_4),
            (hw, hb),
            (
                (hw, if bl > hw + 0.25 { hb - bl } else { hb }),
                (hw, if tl > hw + 0.25 { tl } else { 0.0 }),
            ),
            (tl, (tl, tl), PI),
        ),
    };
    let mut s = Vec::with_capacity(4);
    if r0 > hw + 0.25 {
        append_corner(&mut s, c0.0, c0.1, r0 - hw, a0);
    } else {
        s.push(PathSegment::MoveTo(m0.0, m0.1));
    }
    if (p1.0 - p0.0).abs() + (p1.1 - p0.1).abs() > 1e-3 {
        s.push(PathSegment::LineTo(p1.0, p1.1));
    }
    if r1 > hw + 0.25 {
        append_corner(&mut s, c1.0, c1.1, r1 - hw, a1);
    }
    s
}

fn emit_per_side_borders(
    r: LayoutRect,
    clip: ClipState,
    transform: Affine2,
    opacity: f32,
    p: &PaintStyle,
    l: &LayoutStyle,
    scene: &mut PaintScene,
) {
    if r.width <= 0.0 || r.height <= 0.0 {
        return;
    }
    let edges = p.effective_border_edges(l);
    let colors = p.border_side_colors.unwrap_or([p.border_color; 4]);
    let nr = normalize_box_radii(r.width, r.height, p.effective_corner_radii().to_array());
    if nr.iter().any(|&c| c > 0.5) {
        let widths = [edges.top, edges.right, edges.bottom, edges.left];
        let mut cmds = Vec::with_capacity(4);
        for side in 0..4 {
            let (w, c) = (widths[side].min(r.width.min(r.height) * 0.5), colors[side]);
            if w > 0.0 && c.a > 0 {
                let segments = build_rounded_side_segments(side, (r.width, r.height, w * 0.5), nr);
                if !segments.is_empty() {
                    cmds.push(CanvasCommand::StrokePath {
                        segments,
                        line_width: w,
                        line_cap: crate::scene::CanvasLineCap::Butt,
                        line_join: crate::scene::CanvasLineJoin::Miter,
                        color: c,
                    });
                }
            }
        }
        if !cmds.is_empty() {
            scene.primitives.push(DrawPrimitive::CanvasBatch {
                viewport: r,
                clip: clip.rect,
                clip_radius: clip.max_radius(),
                clip_radii: clip.radii,
                transform,
                opacity,
                commands: Arc::from(cmds),
            });
        }
        return;
    }
    let (tw, rw, bw, lw) = (
        edges.top.min(r.height),
        edges.right.min(r.width),
        edges.bottom.min(r.height),
        edges.left.min(r.width),
    );
    let sides = [
        (edges.top, colors[0], (r.x, r.y, r.width, tw)),
        (
            edges.right,
            colors[1],
            (r.x + r.width - rw, r.y, rw, r.height),
        ),
        (
            edges.bottom,
            colors[2],
            (r.x, r.y + r.height - bw, r.width, bw),
        ),
        (edges.left, colors[3], (r.x, r.y, lw, r.height)),
    ];
    for (w, c, (x, y, sw, sh)) in sides {
        if w > 0.0 && c.a > 0 && sw > 0.0 && sh > 0.0 {
            push_flat_box(scene, x, y, sw, sh, clip, transform, opacity, c, 0.0);
        }
    }
}

fn emit_outline_layer(
    world_rect: LayoutRect,
    clip: ClipState,
    transform: Affine2,
    opacity: f32,
    p: &PaintStyle,
    corners: [f32; 4],
    scene: &mut PaintScene,
) {
    if p.outline_width <= 0.0 || p.outline_color.a == 0 {
        return;
    }
    let expand = p.outline_offset + p.outline_width;
    let rw = world_rect.width + 2.0 * expand;
    let rh = world_rect.height + 2.0 * expand;
    if rw <= 0.5 || rh <= 0.5 {
        return;
    }
    let radius = if p.border_radius > 0.0 {
        (p.border_radius + expand).max(0.0)
    } else {
        0.0
    };
    let expanded_corners = corners.map(|c| if c > 0.0 { (c + expand).max(0.0) } else { 0.0 });
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: LayoutRect {
            x: world_rect.x - expand,
            y: world_rect.y - expand,
            width: rw,
            height: rh,
        },
        clip: clip.rect,
        clip_radius: clip.max_radius(),
        clip_radii: clip.radii,
        transform,
        opacity,
        background_color: Color::TRANSPARENT,
        background_gradient: None,
        radial_gradient: None,
        conic_gradient: None,
        border_color: p.outline_color,
        border_width: p.outline_width,
        border_radius: radius,
        corner_radii: expanded_corners,
        box_shadow: None,
        backdrop_blur_px: 0.0,
    });
}

fn split_stack_base_shadow(
    stack: &[krui_style::BoxShadow],
    has_base: bool,
) -> (Option<usize>, Option<usize>, Option<krui_style::BoxShadow>) {
    if !has_base || stack.is_empty() {
        return (None, None, None);
    }
    if let Some(oi) = stack.iter().position(|s| !s.inset && s.color.a > 0) {
        return (Some(oi), None, Some(stack[oi]));
    }
    if let Some(ii) = stack.iter().rposition(|s| s.color.a > 0) {
        return (None, Some(ii), Some(stack[ii]));
    }
    (None, None, None)
}

/// Emits outer shadows, base styled box, and inset shadows onto `scene`.
pub(super) fn emit_base_and_shadow_boxes(
    f: &NodePaintFrame,
    has_base_visuals: bool,
    scene: &mut PaintScene,
) {
    let corners = f.effective_paint.effective_corner_radii().to_array();
    let per_side = has_non_uniform_per_side_border(&f.effective_paint, &f.layout);
    let stack = f.effective_paint.shadow_stack.as_slice();
    let has_stack = !stack.is_empty();
    let (skip_outer, skip_inset, stack_base_sh) = split_stack_base_shadow(stack, has_base_visuals);
    if has_stack {
        for (i, &sh) in stack.iter().enumerate().rev() {
            if !sh.inset && sh.color.a > 0 && Some(i) != skip_outer {
                push_shadow_only_box(f, corners, sh, scene);
            }
        }
    } else if let Some(sec) = f.effective_paint.secondary_shadow.filter(|s| !s.inset) {
        push_shadow_only_box(f, corners, sec, scene);
    }
    if has_base_visuals {
        let eff_edges = f.effective_paint.effective_border_edges(&f.layout);
        let (bw, bc) = if per_side || eff_edges.top <= 0.0 {
            (0.0, Color::TRANSPARENT)
        } else {
            (
                eff_edges.top,
                f.effective_paint
                    .border_side_colors
                    .map_or(f.effective_paint.border_color, |c| c[0]),
            )
        };
        scene.primitives.push(DrawPrimitive::StyledBox {
            rect: f.world_rect,
            clip: f.clip.rect,
            clip_radius: f.clip.max_radius(),
            clip_radii: f.clip.radii,
            transform: f.effective_transform,
            opacity: f.effective_opacity,
            background_color: f.effective_paint.background_color,
            background_gradient: f.effective_paint.background_gradient,
            radial_gradient: f.radial_gradient,
            conic_gradient: f.conic_gradient,
            border_color: bc,
            border_width: bw,
            border_radius: f.effective_paint.border_radius,
            corner_radii: corners,
            box_shadow: if has_stack {
                stack_base_sh
            } else {
                f.effective_paint.box_shadow
            },
            backdrop_blur_px: f.effective_paint.backdrop_blur_px.max(0.0),
        });
    }
    if has_stack {
        for (i, &sh) in stack.iter().enumerate().rev() {
            if sh.inset && sh.color.a > 0 && Some(i) != skip_inset {
                push_shadow_only_box(f, corners, sh, scene);
            }
        }
    } else if let Some(sec) = f.effective_paint.secondary_shadow.filter(|s| s.inset) {
        push_shadow_only_box(f, corners, sec, scene);
    }
}

fn push_shadow_only_box(
    f: &NodePaintFrame,
    corners: [f32; 4],
    shadow: krui_style::BoxShadow,
    scene: &mut PaintScene,
) {
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: f.world_rect,
        clip: f.clip.rect,
        clip_radius: f.clip.max_radius(),
        clip_radii: f.clip.radii,
        transform: f.effective_transform,
        opacity: f.effective_opacity,
        background_color: Color::TRANSPARENT,
        background_gradient: None,
        radial_gradient: None,
        conic_gradient: None,
        border_color: Color::TRANSPARENT,
        border_width: 0.0,
        border_radius: f.effective_paint.border_radius,
        corner_radii: corners,
        box_shadow: Some(shadow),
        backdrop_blur_px: 0.0,
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_scrollbar_thumb(
    scene: &mut PaintScene,
    world_rect: LayoutRect,
    scroll_y: f32,
    max_scroll_y: f32,
    clip: ClipState,
    effective_transform: Affine2,
    effective_opacity: f32,
    scrollbar_width: krui_style::ScrollbarWidth,
    scrollbar_color: Option<(Color, Color)>,
) {
    let Some(geom) =
        compute_scrollbar_geometry(world_rect, scroll_y, max_scroll_y, scrollbar_width)
    else {
        return;
    };
    if let Some((thumb_col, track_col)) = scrollbar_color {
        if track_col.a > 0 {
            push_flat_box(
                scene,
                geom.track_rect.x,
                geom.track_rect.y,
                geom.track_rect.width,
                geom.track_rect.height,
                clip,
                effective_transform,
                effective_opacity,
                track_col,
                geom.thumb_radius,
            );
        }
        push_flat_box(
            scene,
            geom.thumb_rect.x,
            geom.thumb_rect.y,
            geom.thumb_rect.width,
            geom.thumb_rect.height,
            clip,
            effective_transform,
            effective_opacity,
            thumb_col,
            geom.thumb_radius,
        );
        return;
    }
    push_flat_box(
        scene,
        geom.thumb_rect.x,
        geom.thumb_rect.y,
        geom.thumb_rect.width,
        geom.thumb_rect.height,
        clip,
        effective_transform,
        effective_opacity * SCROLLBAR_THUMB_OPACITY_FACTOR,
        krui_style::UA_SCROLLBAR_THUMB_COLOR,
        geom.thumb_radius,
    );
}
