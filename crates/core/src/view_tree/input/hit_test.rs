//! World bounding box hit-testing, pointer hover/drag tracking, and layout rect propagation.

use crate::view_tree::clip_path_resolve::{point_in_resolved_clip_path, resolve_clip_path};
use crate::view_tree::clip_shape::{
    compute_padding_box_clip, intersect_clip_states, resolve_node_visibility, ClipState,
};
pub(crate) use crate::view_tree::hit_test_geom::update_world_rects_recursive;
use crate::view_tree::hit_test_geom::{point_inside_clip, point_inside_rounded_rect};
use crate::view_tree::node::{is_node_text_selectable, ElementNode};
use crate::view_tree::paint::resolve_node_paint_state;
use crate::view_tree::{A11yId, NodeId, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_style::{Affine2, DisplayMode, PositionMode};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitFilter {
    Interactive,
    Hover,
    Scrollable,
    AnyVisible,
}

fn effective_node_world_matrix_rect_and_clip_state(
    inner: &ViewTreeInner,
    node_id: NodeId,
) -> (Affine2, LayoutRect, ClipState) {
    let chain = collect_ancestor_chain(inner, node_id);
    let (mut scroll_offset_x, mut scroll_offset_y) = (0.0, 0.0);
    let mut accum_affine = Affine2::IDENTITY;
    let mut current_rect = LayoutRect::default();
    let mut clip_state = ClipState::NONE;
    let mut current_node_clip = ClipState::NONE;

    for cid in chain {
        let Some(n) = inner.node(cid) else {
            break;
        };
        if n.style.layout.display == DisplayMode::Contents {
            continue;
        }
        if n.style.layout.position == PositionMode::Fixed {
            scroll_offset_x = 0.0;
            scroll_offset_y = 0.0;
            clip_state = ClipState::NONE;
        } else if crate::view_tree::stacking::is_top_layer_node(inner, cid) {
            clip_state = ClipState::NONE;
        }
        current_node_clip = clip_state;
        let is_disabled = n.is_disabled();
        let is_focused =
            inner.a11y_engine.focused_id() == Some(A11yId::from(cid).0) && !is_disabled;
        let raw_paint = resolve_node_paint_state(inner, cid, n, is_disabled, is_focused);
        let eff_paint = inner.active_transition_paint(cid, raw_paint);
        let mut rect = n.world_rect;
        rect.x -= scroll_offset_x;
        rect.y -= scroll_offset_y;
        accum_affine = accum_affine
            * eff_paint.effective_affine_for_rect(rect.x, rect.y, rect.width, rect.height);
        current_rect = rect;

        let is_scroll = n.style.layout.is_scroll_container();
        if n.style.layout.clips_overflow() {
            let (pad_rect, pad_radii) = compute_padding_box_clip(rect, &eff_paint, &n.style.layout);
            let transformed_clip = transform_rect_aabb(accum_affine, pad_rect);
            clip_state = intersect_clip_states(clip_state, transformed_clip, pad_radii);
        }
        if is_scroll {
            if let Some(sc) = inner.scroll(cid) {
                scroll_offset_x += sc.scroll_x;
                scroll_offset_y += sc.scroll_y;
            }
        }
    }

    (accum_affine, current_rect, current_node_clip)
}

/// Computes the cumulative `Affine2` world transform matrix, un-transformed scrolled `world_rect`,
/// and inherited `clip: Option<LayoutRect>` for `node_id`.
pub(crate) fn effective_node_world_matrix_rect_and_clip(
    inner: &ViewTreeInner,
    node_id: NodeId,
) -> (Affine2, LayoutRect, Option<LayoutRect>) {
    let (aff, rect, cs) = effective_node_world_matrix_rect_and_clip_state(inner, node_id);
    (aff, rect, cs.rect)
}

fn collect_ancestor_chain(inner: &ViewTreeInner, node_id: NodeId) -> Vec<NodeId> {
    let mut chain = Vec::with_capacity(8);
    let mut cur = Some(node_id);
    while let Some(cid) = cur {
        let Some(n) = inner.node(cid) else {
            break;
        };
        chain.push(cid);
        cur = n.parent;
    }
    chain.reverse();
    chain
}

pub(crate) fn effective_node_world_matrix_and_rect(
    inner: &ViewTreeInner,
    node_id: NodeId,
) -> (Affine2, LayoutRect) {
    let (aff, rect, _) = effective_node_world_matrix_rect_and_clip(inner, node_id);
    (aff, rect)
}

/// Computes the axis-aligned screen-space bounding rectangle of `rect` transformed by `affine`.
pub(crate) fn transform_rect_aabb(affine: Affine2, rect: LayoutRect) -> LayoutRect {
    let (x0, y0) = affine.transform_point(rect.x, rect.y);
    let (x1, y1) = affine.transform_point(rect.x + rect.width, rect.y);
    let (x2, y2) = affine.transform_point(rect.x + rect.width, rect.y + rect.height);
    let (x3, y3) = affine.transform_point(rect.x, rect.y + rect.height);
    let min_x = x0.min(x1).min(x2).min(x3);
    let max_x = x0.max(x1).max(x2).max(x3);
    let min_y = y0.min(y1).min(y2).min(y3);
    let max_y = y0.max(y1).max(y2).max(y3);
    LayoutRect {
        x: min_x,
        y: min_y,
        width: (max_x - min_x).max(0.0),
        height: (max_y - min_y).max(0.0),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointerHit {
    Matched(NodeId),
    Occluded(NodeId),
}

/// Returns `true` when screen coordinate `(x, y)` lies inside `node_id`'s transformed and clipped hit rect.
pub(crate) fn node_contains_screen_point(
    inner: &ViewTreeInner,
    node_id: NodeId,
    x: f32,
    y: f32,
) -> bool {
    crate::view_tree::paint_scene::paint_3d_hit::node_contains_point_3d(inner, node_id, x, y)
}

/// Finds the topmost interactive or selectable node containing `(x, y)` in reverse paint order.
pub(crate) fn hit_test_interactive_node(inner: &ViewTreeInner, x: f32, y: f32) -> Option<NodeId> {
    hit_test_with_filter(inner, x, y, HitFilter::Interactive)
}

/// Finds the topmost interactive, selectable, or `:hover`-styled node containing `(x, y)`.
pub(crate) fn hit_test_hover_node(inner: &ViewTreeInner, x: f32, y: f32) -> Option<NodeId> {
    hit_test_with_filter(inner, x, y, HitFilter::Hover)
}

/// Finds the topmost scrollable container (`overflow: scroll` or `<ul>`) containing `(x, y)`.
pub(crate) fn hit_test_scrollable_node(inner: &ViewTreeInner, x: f32, y: f32) -> Option<NodeId> {
    hit_test_with_filter(inner, x, y, HitFilter::Scrollable)
}

/// Finds the deepest visible non-anchor node containing `(x, y)` (used as start node for scroll chaining).
pub(crate) fn hit_test_any_node(inner: &ViewTreeInner, x: f32, y: f32) -> Option<NodeId> {
    hit_test_with_filter(inner, x, y, HitFilter::AnyVisible)
}

fn hit_test_with_filter(
    inner: &ViewTreeInner,
    x: f32,
    y: f32,
    filter: HitFilter,
) -> Option<NodeId> {
    crate::view_tree::stacking::hit_test_stacked_scene(inner, x, y, filter)
}

pub(crate) fn subtree_scroll_and_clip(
    inner: &ViewTreeInner,
    n: &ElementNode,
    node_id: NodeId,
    world_rect: LayoutRect,
    eff_paint: krui_style::PaintStyle,
    eff_transform: Affine2,
    clip_state: ClipState,
) -> (bool, (f32, f32), ClipState) {
    let is_scroll = n.style.layout.is_scroll_container();
    let self_scroll = if is_scroll {
        inner
            .scroll(node_id)
            .map_or((0.0, 0.0), |s| (s.scroll_x, s.scroll_y))
    } else {
        (0.0, 0.0)
    };
    let clips = n.style.layout.clips_overflow();
    let next_clip = if clips {
        let (pad_rect, pad_radii) =
            compute_padding_box_clip(world_rect, &eff_paint, &n.style.layout);
        let pad_aabb = transform_rect_aabb(eff_transform, pad_rect);
        intersect_clip_states(clip_state, pad_aabb, pad_radii)
    } else {
        clip_state
    };
    (is_scroll, self_scroll, next_clip)
}

pub(crate) fn compute_node_hit_geometry(
    inner: &ViewTreeInner,
    n: &ElementNode,
    node_id: NodeId,
    scroll_offset: (f32, f32),
    clip_bounds: Option<LayoutRect>,
    parent_affine: Affine2,
    parent_opacity: f32,
) -> Option<(f32, Affine2, LayoutRect, krui_style::PaintStyle, bool)> {
    let is_disabled = n.is_disabled();
    let is_focused =
        inner.a11y_engine.focused_id() == Some(A11yId::from(node_id).0) && !is_disabled;
    let raw_paint = resolve_node_paint_state(inner, node_id, n, is_disabled, is_focused);
    let eff_paint = inner.active_transition_paint(node_id, raw_paint);
    let eff_opacity = (parent_opacity * eff_paint.opacity).clamp(0.0, 1.0);
    if eff_opacity <= 0.0 {
        return None;
    }
    let is_shielding = !n.is_pointer_events_none()
        && !n.is_visibility_hidden()
        && (eff_paint.background_color.a > 0
            || eff_paint.background_gradient.is_some()
            || eff_paint.radial_gradient.is_some()
            || eff_paint.backdrop_blur_px > 0.0);
    let mut world_rect = n.world_rect;
    world_rect.x -= scroll_offset.0;
    world_rect.y -= scroll_offset.1;
    let eff_transform = parent_affine
        * eff_paint.effective_affine_for_rect(
            world_rect.x,
            world_rect.y,
            world_rect.width,
            world_rect.height,
        );
    let clips_self = n.style.layout.clips_overflow();
    resolve_node_visibility(
        clips_self,
        n.children.is_empty(),
        world_rect,
        clip_bounds,
        eff_transform,
        &eff_paint,
    )?;
    Some((
        eff_opacity,
        eff_transform,
        world_rect,
        eff_paint,
        is_shielding,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_self_or_occluded_hit(
    inner: &ViewTreeInner,
    n: &ElementNode,
    node_id: NodeId,
    (x, y): (f32, f32),
    filter: HitFilter,
    (is_scroll, is_shielding, child_occluded): (bool, bool, Option<NodeId>),
    (clip_state, eff_transform, world_rect, eff_paint): (
        ClipState,
        Affine2,
        LayoutRect,
        &krui_style::PaintStyle,
    ),
) -> Option<PointerHit> {
    if !point_inside_clip(x, y, clip_state) {
        return child_occluded.map(PointerHit::Occluded);
    }
    if let Some(cp) = eff_paint.clip_path {
        let resolved = resolve_clip_path(cp, world_rect, eff_transform);
        if !point_in_resolved_clip_path(x, y, &resolved) {
            return child_occluded.map(PointerHit::Occluded);
        }
    }
    let Some(inv) = eff_transform.inverse() else {
        return child_occluded.map(PointerHit::Occluded);
    };
    let (lx, ly) = inv.transform_point(x, y);
    let corners = eff_paint.effective_corner_radii().to_array();
    if !point_inside_rounded_rect(lx, ly, world_rect, corners) {
        return child_occluded.map(PointerHit::Occluded);
    }
    if node_matches_role(inner, n, node_id, filter, is_scroll) {
        Some(PointerHit::Matched(node_id))
    } else if let Some(occ) = child_occluded {
        Some(PointerHit::Occluded(occ))
    } else if is_shielding {
        Some(PointerHit::Occluded(node_id))
    } else {
        None
    }
}

fn is_interactive_node(inner: &ViewTreeInner, n: &ElementNode, node_id: NodeId) -> bool {
    let has_listener = inner
        .listeners(node_id)
        .is_some_and(|l| l.has_interactive_pointer_or_focus());
    let has_label_for = inner
        .control(node_id)
        .is_some_and(|c| c.label_for.is_some());
    has_listener
        || n.role == SemanticRole::Button
        || n.role.is_focusable()
        || n._tag == "label"
        || has_label_for
        || n._tag == "summary"
        || crate::view_tree::semantics::node_has_attr(inner, node_id, "popovertarget")
        || is_node_text_selectable(n)
}

pub(crate) fn node_matches_role(
    inner: &ViewTreeInner,
    n: &ElementNode,
    node_id: NodeId,
    filter: HitFilter,
    is_scroll: bool,
) -> bool {
    let pointer_blocked = n.is_pointer_events_none() || n.is_visibility_hidden();
    match filter {
        HitFilter::AnyVisible => !pointer_blocked,
        HitFilter::Scrollable => !pointer_blocked && is_scroll,
        HitFilter::Interactive => !pointer_blocked && is_interactive_node(inner, n, node_id),
        HitFilter::Hover => !pointer_blocked,
    }
}
