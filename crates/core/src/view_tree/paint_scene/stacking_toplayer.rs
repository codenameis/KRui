//! HTML5 Top-Layer discovery, environment computation, `::backdrop` emission, and painting
//! for active `<dialog open>` and `[popover]` overlays above document Stacking Contexts.

use super::{emit_stacking_context, node_is_scroll_and_clip, StackEnv};
use crate::scene::{DrawPrimitive, PaintScene};
use crate::view_tree::clip_shape::ClipState;
use crate::view_tree::paint::resolve_node_paint_state;
use crate::view_tree::pointer_vars::{backdrop_pseudo_of, is_pseudo_node};
use crate::view_tree::semantics::{
    is_modal_dialog_open, is_node_semantically_hidden, is_popover_open, node_has_attr,
};
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_layout::LayoutRect;
use krui_style::{Affine2, DisplayMode, PositionMode};

pub(super) struct TopLayerEntry {
    pub(super) node_id: NodeId,
    pub(super) scroll: (f32, f32),
    pub(super) affine: Affine2,
    pub(super) is_dialog: bool,
}

fn has_hidden_ancestor(inner: &ViewTreeInner, mut cur: Option<NodeId>) -> bool {
    while let Some(aid) = cur {
        let Some(anc) = inner.node(aid) else {
            return true;
        };
        if !anc.alive() {
            return true;
        }
        if !anc.is_anchor()
            && (anc.style.layout.display == DisplayMode::None
                || is_node_semantically_hidden(inner, aid))
        {
            return true;
        }
        cur = anc.parent;
    }
    false
}

/// Returns `true` when `node_id` is an active Top-Layer `<dialog open>` or open `[popover]`.
pub(crate) fn is_top_layer_node(inner: &ViewTreeInner, node_id: NodeId) -> bool {
    let Some(n) = inner.node(node_id) else {
        return false;
    };
    if !n.alive()
        || n.is_anchor()
        || is_pseudo_node(inner, node_id)
        || n.style.layout.display == DisplayMode::None
        || is_node_semantically_hidden(inner, node_id)
    {
        return false;
    }
    let is_open_dlg = is_modal_dialog_open(inner, node_id);
    let is_open_pop = node_has_attr(inner, node_id, "popover") && is_popover_open(inner, node_id);
    (is_open_dlg || is_open_pop) && !has_hidden_ancestor(inner, n.parent)
}

pub(super) fn collect_top_layer_entries(inner: &ViewTreeInner) -> Vec<TopLayerEntry> {
    let has_dialog = inner
        .nodes
        .iter()
        .any(|n| n.alive() && !n.is_anchor() && n._tag == "dialog");
    if inner.open_popovers.is_empty() && !has_dialog {
        return Vec::new();
    }
    let mut out = Vec::new();
    if has_dialog {
        for r_idx in 0..inner.roots.len() {
            scan_dialogs_dom_order(inner, inner.roots[r_idx], &mut out);
        }
    }
    for &pid in &inner.open_popovers {
        if is_top_layer_node(inner, pid) {
            let (scroll, affine) = compute_top_layer_env(inner, pid);
            out.push(TopLayerEntry {
                node_id: pid,
                scroll,
                affine,
                is_dialog: false,
            });
        }
    }
    out
}

fn scan_dialogs_dom_order(inner: &ViewTreeInner, id: NodeId, out: &mut Vec<TopLayerEntry>) {
    let Some(n) = inner.node(id).filter(|n| n.alive()) else {
        return;
    };
    if !n.is_anchor()
        && (n.style.layout.display == DisplayMode::None || is_node_semantically_hidden(inner, id))
    {
        return;
    }
    if !n.is_anchor() && !is_pseudo_node(inner, id) && is_modal_dialog_open(inner, id) {
        let (scroll, affine) = compute_top_layer_env(inner, id);
        out.push(TopLayerEntry {
            node_id: id,
            scroll,
            affine,
            is_dialog: true,
        });
    }
    let child_len = n.children.len();
    for i in 0..child_len {
        if let Some(cid) = inner.node(id).and_then(|n| n.children.get(i).copied()) {
            scan_dialogs_dom_order(inner, cid, out);
        }
    }
}

fn compute_top_layer_env(inner: &ViewTreeInner, node_id: NodeId) -> ((f32, f32), Affine2) {
    let mut chain = Vec::new();
    let mut cur = inner.node(node_id).and_then(|n| n.parent);
    while let Some(pid) = cur {
        chain.push(pid);
        cur = inner.node(pid).and_then(|n| n.parent);
    }
    chain.reverse();
    let (mut scroll, mut affine) = ((0.0_f32, 0.0_f32), Affine2::IDENTITY);
    for cid in chain {
        let Some(n) = inner.node(cid) else { break };
        if n.is_anchor() || n.style.layout.display == DisplayMode::Contents {
            continue;
        }
        if n.style.layout.position == PositionMode::Fixed {
            scroll = (0.0, 0.0);
        }
        let rect = LayoutRect {
            x: n.world_rect.x - scroll.0,
            y: n.world_rect.y - scroll.1,
            width: n.world_rect.width,
            height: n.world_rect.height,
        };
        let raw = resolve_node_paint_state(inner, cid, n, n.is_disabled(), false);
        let eff = inner.active_transition_paint(cid, raw);
        affine = affine * eff.effective_affine_for_rect(rect.x, rect.y, rect.width, rect.height);
        if node_is_scroll_and_clip(&n.style.layout, n.role).0 {
            if let Some(sc) = inner.scroll(cid) {
                scroll.0 += sc.scroll_x;
                scroll.1 += sc.scroll_y;
            }
        }
    }
    (scroll, affine)
}

pub(super) fn emit_top_layer_pass(
    inner: &mut ViewTreeInner,
    viewport: (u32, u32),
    scene: &mut PaintScene,
) {
    for entry in collect_top_layer_entries(inner) {
        emit_top_layer_backdrop(inner, &entry, viewport, scene);
        let env = StackEnv {
            scroll: entry.scroll,
            clip: ClipState::NONE,
            affine: entry.affine,
            opacity: 1.0,
            inherited_color: None,
            parent_is_flex_or_grid: false,
            any_top_layer: true,
            any_neg_z: true,
        };
        emit_stacking_context(inner, entry.node_id, env, true, scene);
    }
}

fn emit_top_layer_backdrop(
    inner: &mut ViewTreeInner,
    entry: &TopLayerEntry,
    viewport: (u32, u32),
    scene: &mut PaintScene,
) {
    let vp = LayoutRect {
        x: 0.0,
        y: 0.0,
        width: viewport.0 as f32,
        height: viewport.1 as f32,
    };
    let custom = backdrop_pseudo_of(inner, entry.node_id).and_then(|bd_id| {
        let bd_node = inner.node(bd_id)?;
        let raw = resolve_node_paint_state(inner, bd_id, bd_node, false, false);
        let eff = inner.resolve_transitioned_node_paint(bd_id, raw);
        let vis = eff.background_color.a > 0
            || eff.background_gradient.is_some()
            || eff.radial_gradient.is_some()
            || eff.backdrop_blur_px > 0.5;
        vis.then_some(eff)
    });
    if let Some(eff) = custom {
        push_backdrop_quad(
            vp,
            eff.opacity,
            eff.background_color,
            eff.background_gradient,
            eff.border_color,
            eff.border_width,
            eff.backdrop_blur_px,
            scene,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn push_backdrop_quad(
    rect: LayoutRect,
    opacity: f32,
    background_color: krui_style::Color,
    background_gradient: Option<krui_style::LinearGradient>,
    border_color: krui_style::Color,
    border_width: f32,
    backdrop_blur_px: f32,
    scene: &mut PaintScene,
) {
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity,
        background_color,
        background_gradient,
        radial_gradient: None,
        border_color,
        border_width,
        border_radius: 0.0,
        corner_radii: [0.0; 4],
        box_shadow: None,
        backdrop_blur_px: backdrop_blur_px.max(0.0),
    });
}
