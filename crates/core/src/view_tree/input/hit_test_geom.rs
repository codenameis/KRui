//! Rounded-corner SDF hit-test containment, clip-shape point checks, and layout rect propagation.

use crate::view_tree::clip_shape::{compute_padding_box_clip, normalize_radii_4_core, ClipState};
use crate::view_tree::events::compute_max_scroll_y;
use crate::view_tree::scroll::compute_max_scroll_x;
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_style::{Affine2, DisplayMode, LayoutStyle, PaintStyle, PositionMode};

/// Returns `true` when `pt` falls inside the padding box clip rect when `clips_self` is active.
pub(crate) fn point_inside_node_clip_pad(
    pt: (f32, f32),
    clips_self: bool,
    world_rect: LayoutRect,
    eff_paint: &PaintStyle,
    layout: &LayoutStyle,
    eff_transform: Affine2,
) -> bool {
    if !clips_self {
        return true;
    }
    let (pad_rect, pad_radii) = compute_padding_box_clip(world_rect, eff_paint, layout);
    let Some(inv) = eff_transform.inverse() else {
        return false;
    };
    let (lx, ly) = inv.transform_point(pt.0, pt.1);
    point_inside_rounded_rect(lx, ly, pad_rect, pad_radii)
}

/// Returns `true` when local coordinate `(px, py)` lies inside `rect` including its 4 rounded corners.
pub(crate) fn point_inside_rounded_rect(
    px: f32,
    py: f32,
    rect: LayoutRect,
    radii: [f32; 4],
) -> bool {
    if !rect.contains(px, py) {
        return false;
    }
    if radii == [0.0; 4] {
        return true;
    }
    let nr = normalize_radii_4_core(rect.width, rect.height, radii);
    let lx = px - rect.x;
    let ly = py - rect.y;
    let w = rect.width;
    let h = rect.height;
    let (r, cx, cy) = if lx < nr[0] && ly < nr[0] {
        (nr[0], nr[0], nr[0])
    } else if lx > w - nr[1] && ly < nr[1] {
        (nr[1], w - nr[1], nr[1])
    } else if lx > w - nr[2] && ly > h - nr[2] {
        (nr[2], w - nr[2], h - nr[2])
    } else if lx < nr[3] && ly > h - nr[3] {
        (nr[3], nr[3], h - nr[3])
    } else {
        return true;
    };
    (lx - cx).hypot(ly - cy) <= r + 1e-3
}

/// Returns `true` when screen coordinate `(x, y)` lies inside `clip`'s rounded padding-box bounds.
#[inline]
pub(crate) fn point_inside_clip(x: f32, y: f32, clip: ClipState) -> bool {
    let Some(rect) = clip.rect else {
        return true;
    };
    point_inside_rounded_rect(x, y, rect, clip.radii)
}

/// Recursively propagates local Taffy rects into `world_rect` coordinates, invalidates cached
/// wrapped text layouts when computed width changes, and caches `max_scroll_x/y` for scroll containers.
pub(crate) fn update_world_rects_recursive(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    parent_x: f32,
    parent_y: f32,
) {
    let Some((taffy_node, child_len, is_scroll, disp, pos)) = inner.node(node_id).and_then(|n| {
        if n.is_anchor() {
            return None;
        }
        Some((
            n.taffy_node?,
            n.children.len(),
            n.style.layout.is_scroll_container(),
            n.style.layout.display,
            n.style.layout.position,
        ))
    }) else {
        return;
    };
    let local = inner.layout_engine.local_rect(taffy_node);
    let (world, child_origin) = if disp == DisplayMode::Contents {
        (
            LayoutRect {
                x: parent_x,
                y: parent_y,
                width: 0.0,
                height: 0.0,
            },
            (parent_x, parent_y),
        )
    } else if pos == PositionMode::Fixed {
        (local, (local.x, local.y))
    } else {
        let w = LayoutRect {
            x: parent_x + local.x,
            y: parent_y + local.y,
            width: local.width,
            height: local.height,
        };
        (w, (w.x, w.y))
    };
    let mut size_changed = false;
    if let Some(node) = inner.node_mut(node_id) {
        let width_changed = (node.world_rect.width - world.width).abs() > 0.05;
        size_changed = width_changed || (node.world_rect.height - world.height).abs() > 0.01;
        let is_single_line = matches!(
            node.role,
            SemanticRole::Button | SemanticRole::TextInput | SemanticRole::PasswordInput
        );
        if width_changed && !is_single_line && node.cached_text_layout.is_some() {
            node.cached_text_layout = None;
            node.set_text_dirty(true);
        }
        node.world_rect = world;
    }
    if size_changed {
        if let Some(cv) = inner.sparse.canvases.get_mut(&node_id) {
            cv.canvas_dirty = true;
        }
    }
    for i in 0..child_len {
        let Some(child_id) = inner.node(node_id).and_then(|n| n.children.get(i).copied()) else {
            break;
        };
        update_world_rects_recursive(inner, child_id, child_origin.0, child_origin.1);
    }
    if is_scroll {
        let max_scroll_x = compute_max_scroll_x(inner, node_id);
        let max_scroll_y = compute_max_scroll_y(inner, node_id);
        let sc = inner.ensure_scroll(node_id);
        sc.cached_max_scroll_x = max_scroll_x;
        sc.cached_max_scroll_y = max_scroll_y;
        sc.scroll_x = sc.scroll_x.clamp(0.0, max_scroll_x);
        sc.scroll_y = sc.scroll_y.clamp(0.0, max_scroll_y);
    }
}
