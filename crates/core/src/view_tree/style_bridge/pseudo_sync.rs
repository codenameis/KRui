//! Synchronizes CSS-generated `::before`, `::after`, and `::backdrop` helper DOM nodes from Stylo.

use crate::view_tree::content_box::sync_node_text_context;
use crate::view_tree::keyframes_driver::sync_node_css_animations;
use crate::view_tree::layout_modes::sync_container_taffy_children;
use crate::view_tree::node::ElementNode;
use crate::view_tree::paint::resolve_node_paint_state;
use crate::view_tree::pointer_vars::{backdrop_pseudo_of, is_pseudo_before_node, is_pseudo_node};
use crate::view_tree::semantics::sync_semantic_visibility;
use crate::view_tree::transitions::commit_node_transitions_from_style;
use crate::view_tree::{unmount_node_subtree, NodeId, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_style::{sanitize_untrusted_text, CompiledStyle, DisplayMode, PaintStyle, TextStyle};
use krui_stylo_bridge::{BoxMeta, PseudoElementKind, StyloNodeId};
use taffy::NodeId as TaffyNodeId;

pub(crate) fn sync_pseudo_elements_for_node(
    inner: &mut ViewTreeInner,
    host_id: NodeId,
    host_stylo: StyloNodeId,
) {
    let Some(host) = inner.node(host_id) else {
        return;
    };
    if !host.alive() || host.is_anchor() || is_pseudo_node(inner, host_id) {
        return;
    }
    let children = host.children.clone();
    let existing_before = children
        .iter()
        .copied()
        .find(|&cid| is_pseudo_before_node(inner, cid));
    let existing_after = children
        .iter()
        .copied()
        .find(|&cid| is_pseudo_node(inner, cid) && !is_pseudo_before_node(inner, cid));
    let existing_backdrop = backdrop_pseudo_of(inner, host_id);

    sync_flow_pseudo(
        inner,
        host_id,
        host_stylo,
        PseudoElementKind::Before,
        existing_before,
        true,
    );
    sync_flow_pseudo(
        inner,
        host_id,
        host_stylo,
        PseudoElementKind::After,
        existing_after,
        false,
    );
    sync_backdrop_pseudo(inner, host_id, host_stylo, existing_backdrop);
}

fn sync_flow_pseudo(
    inner: &mut ViewTreeInner,
    host_id: NodeId,
    host_stylo: StyloNodeId,
    kind: PseudoElementKind,
    existing: Option<NodeId>,
    is_before: bool,
) {
    let content = inner.stylo.pseudo_content(host_stylo, kind);
    let layout_opt = inner.stylo.pseudo_layout_style(host_stylo, kind);
    let active = content.is_some()
        && layout_opt
            .as_ref()
            .is_some_and(|l| l.display != DisplayMode::None);
    if !active {
        if let Some(pid) = existing {
            remove_pseudo_helper(inner, host_id, pid, true);
        }
        return;
    }
    let mut layout = layout_opt.unwrap_or_default();
    let mut paint = inner
        .stylo
        .pseudo_paint_style(host_stylo, kind)
        .unwrap_or_default();
    let text = inner
        .stylo
        .pseudo_text_style(host_stylo, kind)
        .map_or_else(TextStyle::default, |(t, _)| t);
    let meta = inner.stylo.pseudo_box_meta(host_stylo, kind);
    apply_box_meta_fields(&mut layout, &mut paint, meta);

    let (pid, taffy_node, created) = match existing {
        Some(id) => {
            let tn = inner
                .node(id)
                .and_then(|n| n.taffy_node)
                .unwrap_or_else(|| inner.layout_engine.create_node(&layout));
            (id, tn, false)
        }
        None => alloc_pseudo_node(
            inner,
            host_id,
            if is_before { "::before" } else { "::after" },
            CompiledStyle {
                layout,
                text,
                paint,
            },
            is_before,
            false,
        ),
    };

    let sanitized = sanitize_untrusted_text(&content.unwrap_or_default());
    apply_flow_pseudo_state(
        inner,
        (host_id, pid, taffy_node),
        CompiledStyle {
            layout,
            text,
            paint,
        },
        meta,
        &sanitized,
        created,
    );
}

fn apply_flow_pseudo_state(
    inner: &mut ViewTreeInner,
    (host_id, pid, taffy_node): (NodeId, NodeId, TaffyNodeId),
    style: CompiledStyle,
    meta: Option<BoxMeta>,
    sanitized: &str,
    created: bool,
) {
    let prev_styles = inner
        .node(pid)
        .filter(|_| !created)
        .map(|n| (n.style.layout, n.style.text));
    let had_prev_target = inner.sparse.last_target_paints.contains_key(&pid);
    let mut text_changed = false;
    let CompiledStyle {
        layout,
        text,
        paint,
    } = style;
    if let Some(node) = inner.node_mut(pid) {
        if let Some(m) = meta {
            node.set_visibility_hidden(m.visibility_hidden);
            node.set_pointer_events_none(m.pointer_events_none);
        }
        node.style = style;
        if node.text != sanitized {
            node.text = sanitized.to_string();
            node.set_text_dirty(true);
            node.cached_text_layout = None;
            node.last_shaped_key = 0;
            text_changed = true;
        }
    }
    let layout_changed = created || prev_styles.is_none_or(|(pl, _)| pl != layout);
    let text_style_changed = created || prev_styles.is_none_or(|(_, pt)| pt != text);
    let host_flow_changed = created
        || prev_styles
            .is_some_and(|(pl, _)| (pl.position, pl.display) != (layout.position, layout.display));
    if layout_changed {
        inner.layout_engine.set_layout_style(taffy_node, &layout);
    }
    if text_changed || text_style_changed || layout_changed {
        sync_node_text_context(inner, pid);
    }
    sync_semantic_visibility(inner, pid);
    if host_flow_changed {
        sync_node_text_context(inner, host_id);
    }
    if created {
        sync_container_taffy_children(inner, Some(host_id));
        let raw = inner
            .node(pid)
            .map(|n| resolve_node_paint_state(inner, pid, n, false, false));
        if let Some(raw) = raw {
            inner.sparse.last_target_paints.insert(pid, raw);
        }
    } else if had_prev_target {
        commit_node_transitions_from_style(inner, pid);
    }
    sync_node_css_animations(inner, pid, paint.animations);
    inner.paint_dirty = true;
}

fn sync_backdrop_pseudo(
    inner: &mut ViewTreeInner,
    host_id: NodeId,
    host_stylo: StyloNodeId,
    existing: Option<NodeId>,
) {
    let is_top_layer_candidate = crate::view_tree::stacking::is_top_layer_node(inner, host_id);
    if !is_top_layer_candidate {
        if let Some(bd_id) = existing {
            clear_backdrop_link(inner, host_id);
            remove_pseudo_helper(inner, host_id, bd_id, false);
        }
        return;
    }
    let paint_opt = inner
        .stylo
        .pseudo_paint_style(host_stylo, PseudoElementKind::Backdrop);
    let meta = inner
        .stylo
        .pseudo_box_meta(host_stylo, PseudoElementKind::Backdrop);
    let visible = paint_opt.is_some_and(|p| {
        meta.is_none_or(|m| m.display != DisplayMode::None)
            && (p.background_color.a > 0
                || p.background_gradient.is_some()
                || p.radial_gradient.is_some()
                || p.backdrop_blur_px > 0.5)
    });
    if !visible {
        if let Some(bd_id) = existing {
            clear_backdrop_link(inner, host_id);
            remove_pseudo_helper(inner, host_id, bd_id, false);
        }
        return;
    }
    let paint = paint_opt.unwrap_or_default();
    let style = CompiledStyle {
        layout: Default::default(),
        text: Default::default(),
        paint,
    };
    let (bd_id, _, created) = match existing {
        Some(id) => (id, inner.node(id).and_then(|n| n.taffy_node), false),
        None => {
            let (id, tn, c) = alloc_pseudo_node(inner, host_id, "::backdrop", style, false, true);
            inner.ensure_pseudo_pointer(host_id).backdrop_pseudo = Some(id);
            (id, Some(tn), c)
        }
    };
    if let Some(node) = inner.node_mut(bd_id) {
        node.style = style;
    }
    sync_semantic_visibility(inner, bd_id);
    if created {
        inner.sparse.last_target_paints.insert(bd_id, paint);
    } else {
        commit_node_transitions_from_style(inner, bd_id);
    }
    inner.paint_dirty = true;
}

fn alloc_pseudo_node(
    inner: &mut ViewTreeInner,
    host_id: NodeId,
    tag: &'static str,
    style: CompiledStyle,
    is_before: bool,
    is_backdrop: bool,
) -> (NodeId, TaffyNodeId, bool) {
    let taffy_node = inner.layout_engine.create_node(&style.layout);
    let (slot_idx, generation) = inner.allocate_slot();
    let pid = NodeId::new(slot_idx, generation);
    let retained = ElementNode::new_element(
        pid,
        if is_backdrop { None } else { Some(host_id) },
        tag,
        None,
        None,
        SemanticRole::GenericContainer,
        false,
        taffy_node,
        style,
    );
    if (slot_idx as usize) < inner.nodes.len() {
        inner.nodes[slot_idx as usize] = retained;
    } else {
        inner.nodes.push(retained);
    }
    let entry = inner.ensure_pseudo_pointer(pid);
    entry.is_pseudo = true;
    entry.is_before = is_before;
    entry.is_backdrop = is_backdrop;
    entry.pseudo_host = Some(host_id);
    if !is_backdrop {
        if let Some(host) = inner.node_mut(host_id) {
            if is_before {
                host.children.insert(0, pid);
            } else {
                host.children.push(pid);
            }
        }
    }
    (pid, taffy_node, true)
}

fn remove_pseudo_helper(inner: &mut ViewTreeInner, host_id: NodeId, pid: NodeId, sync_taffy: bool) {
    unmount_node_subtree(inner, pid);
    if sync_taffy {
        sync_container_taffy_children(inner, Some(host_id));
        sync_node_text_context(inner, host_id);
    }
    inner.paint_dirty = true;
}

fn clear_backdrop_link(inner: &mut ViewTreeInner, host_id: NodeId) {
    if let Some(entry) = inner.sparse.pseudo_pointer.get_mut(&host_id) {
        entry.backdrop_pseudo = None;
    }
}

pub(crate) fn apply_box_meta_fields(
    layout: &mut krui_style::LayoutStyle,
    paint: &mut PaintStyle,
    meta: Option<BoxMeta>,
) {
    let Some(m) = meta else {
        return;
    };
    paint.object_fit = m.object_fit;
    paint.user_select = m.user_select;
    layout.scrollbar_width = m.scrollbar_width;
    layout.z_index = m.z_index.unwrap_or(0);
    layout.has_z_index = m.z_index.is_some();
}
