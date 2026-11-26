//! Pseudo-element linkage (`::before`/`::after`), runtime HTML attributes, custom tags,
//! pointerdown local coordinate latching (`--krui-pointer-x/y/xp/yp`), hover tracking
//! (`--krui-hover-x/y`), and `ResolvedRadialGradient` resolution.

use crate::scene::{ResolvedConicGradient, ResolvedRadialGradient};
use crate::view_tree::hit_test::effective_node_world_matrix_and_rect;
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_layout::LayoutRect;
use krui_signals::Effect;
use krui_style::{resolve_radial_geometry, PaintStyle};
use krui_stylo_bridge::StyloDomMutation;
use std::rc::Rc;

/// Sparse per-node pseudo-element, custom-tag, HTML attribute, and pointer-variable state.
#[derive(Clone, Debug, Default)]
pub(crate) struct PointerAndPseudoData {
    pub(crate) pseudo_host: Option<NodeId>,
    pub(crate) is_pseudo: bool,
    pub(crate) is_before: bool,
    pub(crate) is_backdrop: bool,
    pub(crate) backdrop_pseudo: Option<NodeId>,
    pub(crate) popover_open: bool,
    pub(crate) custom_tag: Option<String>,
    pub(crate) author_inline_style: Option<String>,
    pub(crate) latched_pointer_xy: Option<(f32, f32)>,
    pub(crate) hover_pointer_xy: Option<(f32, f32)>,
    pub(crate) attrs: Vec<(String, String)>,
}

impl ViewTree {
    /// Stores the original lowercase/custom tag name (`HtmlTag::Custom`) for runtime CSS matching.
    pub fn set_custom_tag(&mut self, node_id: NodeId, custom_tag: impl Into<String>) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_none() {
            return;
        }
        inner.ensure_pseudo_pointer(node_id).custom_tag = Some(custom_tag.into());
        crate::view_tree::sync::sync_node_attrs_to_stylo(&mut inner, node_id);
    }

    /// Sets or updates a static/runtime HTML attribute on `node_id` and invalidates selectors.
    pub fn set_node_attr(
        &mut self,
        node_id: NodeId,
        name: impl Into<String>,
        value: impl Into<String>,
    ) {
        let mut inner = self.inner.borrow_mut();
        set_node_attr_inner(&mut inner, node_id, &name.into(), Some(value.into()));
    }

    /// Removes an HTML attribute from `node_id` and invalidates selectors.
    pub fn remove_node_attr(&mut self, node_id: NodeId, name: &str) {
        let mut inner = self.inner.borrow_mut();
        set_node_attr_inner(&mut inner, node_id, name, None);
    }

    /// Reactively binds a dynamic HTML attribute expression (`open`, `data-*`, `aria-*`) onto `node_id`.
    pub fn bind_dynamic_attr(
        &mut self,
        node_id: NodeId,
        name: impl Into<String>,
        eval: impl Fn() -> Option<String> + 'static,
    ) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let attr_name = name.into();
        let tree_weak = Rc::downgrade(&self.inner);
        let effect = Effect::new(move || {
            let next_val = eval();
            if let Some(rc) = tree_weak.upgrade() {
                let mut inner = rc.borrow_mut();
                set_node_attr_inner(&mut inner, node_id, &attr_name, next_val);
            }
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }

    /// Links a leading `::before` `pseudo_id` to its owner element `host_id`.
    pub fn mark_pseudo_before(&mut self, pseudo_id: NodeId, host_id: NodeId) {
        {
            let mut inner = self.inner.borrow_mut();
            if inner.node(pseudo_id).is_some() {
                inner.ensure_pseudo_pointer(pseudo_id).is_before = true;
            }
        }
        self.mark_pseudo_element(pseudo_id, host_id);
    }

    /// Links `pseudo_id` (`::before` or `::after`) to its owner element `host_id`.
    pub fn mark_pseudo_element(&mut self, pseudo_id: NodeId, host_id: NodeId) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(pseudo_id).is_none() || inner.node(host_id).is_none() {
            return;
        }
        {
            let entry = inner.ensure_pseudo_pointer(pseudo_id);
            entry.is_pseudo = true;
            entry.pseudo_host = Some(host_id);
        }
        inner.paint_dirty = true;
    }

    /// Links `backdrop_id` (`::backdrop`) to its host `<dialog>` or `[popover]` without layout impact.
    pub fn mark_backdrop_pseudo(&mut self, backdrop_id: NodeId, host_id: NodeId) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(backdrop_id).is_none() || inner.node(host_id).is_none() {
            return;
        }
        {
            let entry = inner.ensure_pseudo_pointer(backdrop_id);
            entry.is_pseudo = true;
            entry.is_backdrop = true;
            entry.pseudo_host = Some(host_id);
        }
        inner.ensure_pseudo_pointer(host_id).backdrop_pseudo = Some(backdrop_id);
        crate::view_tree::semantics::sync_semantic_visibility(&mut inner, backdrop_id);
        inner.paint_dirty = true;
    }

    /// Configures simulated pointer hardware capabilities for `@media (hover: ...)` and `(pointer: ...)`.
    pub fn set_pointer_capabilities(&mut self, can_hover: bool, pointer_fine: bool) {
        let mut inner = self.inner.borrow_mut();
        let kind = if can_hover && pointer_fine {
            krui_stylo_bridge::PointerKind::FineHover
        } else {
            krui_stylo_bridge::PointerKind::CoarseTouch
        };
        inner.stylo.set_pointer(kind);
        inner.stylo_dirty = true;
        inner.paint_dirty = true;
    }
}

pub(crate) fn set_node_attr_inner(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    name: &str,
    value: Option<String>,
) {
    if inner.node(node_id).is_none() {
        return;
    }
    let key = name.to_ascii_lowercase();
    let is_semantic_vis_attr = matches!(key.as_str(), "hidden" | "popover" | "open");
    let changed = if let Some(val) = value {
        let entry = inner.ensure_pseudo_pointer(node_id);
        if let Some(slot) = entry.attrs.iter_mut().find(|(k, _)| *k == key) {
            if slot.1 == val {
                false
            } else {
                slot.1 = val;
                true
            }
        } else {
            entry.attrs.push((key, val));
            true
        }
    } else if let Some(entry) = inner.sparse.pseudo_pointer.get_mut(&node_id) {
        let before_len = entry.attrs.len();
        entry.attrs.retain(|(k, _)| *k != key);
        entry.attrs.len() != before_len
    } else {
        false
    };
    if changed {
        if is_semantic_vis_attr {
            crate::view_tree::semantics::sync_semantic_visibility(inner, node_id);
        }
        crate::view_tree::sync::sync_node_attrs_to_stylo(inner, node_id);
        crate::view_tree::sync::notify_selector_state_change(inner, node_id);
        inner.paint_dirty = true;
    }
}

#[inline]
pub(crate) fn is_pseudo_node(inner: &ViewTreeInner, id: NodeId) -> bool {
    inner
        .sparse
        .pseudo_pointer
        .get(&id)
        .is_some_and(|e| e.is_pseudo)
}

#[inline]
pub(crate) fn is_pseudo_before_node(inner: &ViewTreeInner, id: NodeId) -> bool {
    inner
        .sparse
        .pseudo_pointer
        .get(&id)
        .is_some_and(|e| e.is_pseudo && e.is_before)
}

#[inline]
pub(crate) fn backdrop_pseudo_of(inner: &ViewTreeInner, host_id: NodeId) -> Option<NodeId> {
    inner.sparse.pseudo_pointer.get(&host_id)?.backdrop_pseudo
}

#[inline]
pub(crate) fn pseudo_host_of(inner: &ViewTreeInner, id: NodeId) -> Option<NodeId> {
    inner.sparse.pseudo_pointer.get(&id)?.pseudo_host
}

pub(crate) fn node_or_pseudo_uses_radial_gradient(inner: &ViewTreeInner, cid: NodeId) -> bool {
    let uses = |p: &PaintStyle| p.radial_gradient.is_some() || p.conic_gradient.is_some();
    let Some(n) = inner.node(cid) else {
        return false;
    };
    uses(&n.style.paint)
        || n.children.iter().any(|&pid| {
            is_pseudo_node(inner, pid) && inner.node(pid).is_some_and(|pn| uses(&pn.style.paint))
        })
}

pub(crate) fn sync_effective_inline_style(inner: &mut ViewTreeInner, node_id: NodeId) {
    let Some(stylo_id) = inner.node(node_id).and_then(|n| n.stylo_node) else {
        return;
    };
    let inject_ptr = node_or_pseudo_uses_radial_gradient(inner, node_id);
    let mut css = String::new();
    if let Some(entry) = inner.sparse.pseudo_pointer.get(&node_id) {
        if inject_ptr {
            if let Some((lx, ly)) = entry.latched_pointer_xy {
                css.push_str(&format!(
                    "--krui-pointer-x: {lx:.3}px; --krui-pointer-y: {ly:.3}px; "
                ));
            }
            if let Some((hx, hy)) = entry.hover_pointer_xy {
                css.push_str(&format!(
                    "--krui-hover-x: {hx:.3}px; --krui-hover-y: {hy:.3}px; "
                ));
            }
        }
        if let Some(author) = entry.author_inline_style.as_deref() {
            css.push_str(author);
        }
    }
    inner
        .stylo
        .apply_mutation(StyloDomMutation::SetInlineStyle {
            node: stylo_id,
            css,
        });
    inner.stylo_dirty = true;
    inner.paint_dirty = true;
}

/// Latches pointerdown-local `(lx, ly)` coordinates onto `hit` and its ancestors without affecting
/// layout, and snapshots any zero-transition `:active` state so fast release animates cleanly.
pub(crate) fn latch_pointer_down_ancestors(inner: &mut ViewTreeInner, hit: NodeId, x: f32, y: f32) {
    let mut cur = Some(hit);
    let mut chain = Vec::with_capacity(8);
    for _ in 0..64 {
        let Some(cid) = cur else { break };
        chain.push(cid);
        cur = inner.node(cid).and_then(|n| n.parent);
    }
    for cid in chain {
        let (aff, rect) = effective_node_world_matrix_and_rect(inner, cid);
        if let Some(inv) = aff.inverse() {
            let (lx, ly) = inv.transform_point(x, y);
            inner.ensure_pseudo_pointer(cid).latched_pointer_xy = Some((lx - rect.x, ly - rect.y));
            if node_or_pseudo_uses_radial_gradient(inner, cid) {
                sync_effective_inline_style(inner, cid);
            }
        }
    }
    crate::view_tree::sync::flush_pending_restyle(inner);
    snapshot_zero_transition_active_subtree(inner, hit);
}

fn snapshot_zero_transition_active_subtree(inner: &mut ViewTreeInner, host_id: NodeId) {
    let mut targets = vec![host_id];
    if let Some(host) = inner.node(host_id) {
        for &cid in &host.children {
            if is_pseudo_node(inner, cid) {
                targets.push(cid);
            }
        }
    }
    for tid in targets {
        if !inner.sparse.last_target_paints.contains_key(&tid) {
            continue;
        }
        let Some(active_paint) = inner.node(tid).map(|n| n.style.paint) else {
            continue;
        };
        if !active_paint.transition.is_active() {
            inner.sparse.last_target_paints.insert(tid, active_paint);
            inner.sparse.active_transitions.remove(&tid);
        }
    }
}

/// Updates live hover local `(lx, ly)` along the hovered ancestor chain and marks dirty only if used.
pub(crate) fn update_hover_pointer_coords(
    inner: &mut ViewTreeInner,
    hovered: Option<NodeId>,
    x: f32,
    y: f32,
) {
    let mut active_chain = Vec::with_capacity(8);
    let mut cur = hovered;
    for _ in 0..64 {
        let Some(cid) = cur else { break };
        active_chain.push(cid);
        cur = inner.node(cid).and_then(|n| n.parent);
    }
    let stale: Vec<NodeId> = inner
        .sparse
        .pseudo_pointer
        .iter()
        .filter_map(|(&id, e)| {
            (e.hover_pointer_xy.is_some() && !active_chain.contains(&id)).then_some(id)
        })
        .collect();
    for id in stale {
        if let Some(e) = inner.sparse.pseudo_pointer.get_mut(&id) {
            e.hover_pointer_xy = None;
        }
        if node_or_pseudo_uses_radial_gradient(inner, id) {
            sync_effective_inline_style(inner, id);
        }
    }
    for cid in active_chain {
        let (aff, rect) = effective_node_world_matrix_and_rect(inner, cid);
        if let Some(inv) = aff.inverse() {
            let (lx, ly) = inv.transform_point(x, y);
            let next_xy = (lx - rect.x, ly - rect.y);
            inner.ensure_pseudo_pointer(cid).hover_pointer_xy = Some(next_xy);
            if node_or_pseudo_uses_radial_gradient(inner, cid) {
                sync_effective_inline_style(inner, cid);
            }
        }
    }
}

/// Resolves radial gradient center and semi-axes in logical px relative to box center.
pub(crate) fn resolve_radial_gradient_for_node(
    inner: &ViewTreeInner,
    node_id: NodeId,
    world_rect: LayoutRect,
    paint: &PaintStyle,
) -> Option<ResolvedRadialGradient> {
    let grad = paint.radial_gradient?;
    let coord_source = pseudo_host_of(inner, node_id).unwrap_or(node_id);
    let entry = inner.sparse.pseudo_pointer.get(&coord_source);
    let latched_xy = entry.and_then(|e| e.latched_pointer_xy);
    let hover_xy = entry.and_then(|e| e.hover_pointer_xy);
    let (dx, dy, rx, ry) = resolve_radial_geometry(
        &grad,
        paint.bg_position,
        paint.bg_size_scale,
        (world_rect.width, world_rect.height),
        (latched_xy, hover_xy),
    );
    Some(ResolvedRadialGradient {
        center_dx: dx,
        center_dy: dy,
        radius_x: rx,
        radius_y: ry,
        inner_color: grad.inner_color,
        outer_color: grad.outer_color,
        inner_stop: grad.inner_stop,
        outer_stop: grad.outer_stop,
        extra_stops: grad.extra_stops,
        repeating: grad.repeating,
    })
}

/// Resolves conic gradient center offset in logical px relative to box center.
pub(crate) fn resolve_conic_gradient_for_node(
    inner: &ViewTreeInner,
    node_id: NodeId,
    world_rect: LayoutRect,
    paint: &PaintStyle,
) -> Option<ResolvedConicGradient> {
    let grad = paint.conic_gradient?;
    let coord_source = pseudo_host_of(inner, node_id).unwrap_or(node_id);
    let entry = inner.sparse.pseudo_pointer.get(&coord_source);
    let latched = entry.and_then(|e| e.latched_pointer_xy);
    let hover = entry.and_then(|e| e.hover_pointer_xy);
    let cx = grad
        .center_x
        .resolve_px(world_rect.width, latched.map(|p| p.0), hover.map(|p| p.0));
    let cy = grad
        .center_y
        .resolve_px(world_rect.height, latched.map(|p| p.1), hover.map(|p| p.1));
    Some(ResolvedConicGradient {
        center_dx: cx - 0.5 * world_rect.width,
        center_dy: cy - 0.5 * world_rect.height,
        from_angle_deg: grad.from_angle_deg,
        stops: grad.stops,
        repeating: grad.repeating,
    })
}
