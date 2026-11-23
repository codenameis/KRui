//! Incremental Stylo cascade restyle flushing, DOM/state/attr synchronization, and `@font-face` syncing.

use crate::view_tree::content_box::sync_node_text_context;
use crate::view_tree::keyframes_driver::{resync_all_keyframes, sync_node_css_animations};
use crate::view_tree::layout_modes::sync_node_layout_mode_if_changed;
use crate::view_tree::pointer_vars::is_pseudo_node;
use crate::view_tree::pseudo_sync::{apply_box_meta_fields, sync_pseudo_elements_for_node};
use crate::view_tree::semantics::sync_semantic_visibility;
use crate::view_tree::transitions::commit_node_transitions_from_style;
use crate::view_tree::transitions_layout::{
    apply_effective_node_layout_to_taffy, commit_node_layout_transition,
};
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_style::{Color, DisplayMode, LayoutStyle, PaintStyle, PositionMode, TextStyle};
use krui_stylo_bridge::{
    DomElementState, ElementAttr, ElementNamespace, StyloDamage, StyloDomMutation, StyloNodeId,
};
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) fn sync_stylo_children_for_container(
    inner: &mut ViewTreeInner,
    container: Option<NodeId>,
) {
    let (stylo_parent, child_ids, text_child) = match container {
        Some(pid) => {
            let Some(pnode) = inner.node(pid) else {
                return;
            };
            if pnode.is_anchor() {
                sync_stylo_children_for_container(inner, pnode.parent);
                return;
            }
            let Some(ps) = pnode.stylo_node else {
                return;
            };
            (ps, pnode.children.clone(), pnode.text_stylo_node)
        }
        None => (inner.stylo_body_root, inner.roots.clone(), None),
    };
    let mut stylo_children = Vec::with_capacity(child_ids.len() + 1);
    if let Some(tid) = text_child {
        stylo_children.push(tid);
    }
    collect_stylo_children_flat(inner, &child_ids, &mut stylo_children);
    let mut next_sibling = None;
    for child in stylo_children.into_iter().rev() {
        inner.stylo.apply_mutation(StyloDomMutation::InsertBefore {
            parent: stylo_parent,
            child,
            before: next_sibling,
        });
        next_sibling = Some(child);
    }
}

fn collect_stylo_children_flat(
    inner: &ViewTreeInner,
    child_ids: &[NodeId],
    out: &mut Vec<StyloNodeId>,
) {
    for &cid in child_ids {
        let Some(cn) = inner.node(cid) else {
            continue;
        };
        if !cn.alive() || is_pseudo_node(inner, cid) {
            continue;
        }
        if cn.is_anchor() {
            let nested = cn.children.clone();
            collect_stylo_children_flat(inner, &nested, out);
        } else if let Some(cs) = cn.stylo_node {
            out.push(cs);
        }
    }
}

pub(crate) fn sync_node_text_to_stylo(inner: &mut ViewTreeInner, node_id: NodeId) {
    let Some((sid_opt, prev_tid, non_empty, is_syn, is_ed)) = inner.node(node_id).map(|n| {
        (
            n.stylo_node,
            n.text_stylo_node,
            !n.text.trim().is_empty(),
            n.is_synthetic_text_run(),
            n.role.is_text_editable(),
        )
    }) else {
        return;
    };
    if is_ed {
        return;
    }
    let Some(sid) = sid_opt else {
        return;
    };
    if is_syn {
        inner
            .stylo
            .apply_mutation(StyloDomMutation::SetTextNonEmpty {
                node: sid,
                non_empty,
            });
        inner.stylo_dirty = true;
        return;
    }
    match prev_tid {
        Some(tid) => {
            inner
                .stylo
                .apply_mutation(StyloDomMutation::SetTextNonEmpty {
                    node: tid,
                    non_empty,
                });
            inner.stylo_dirty = true;
        }
        None if non_empty => {
            let tid = inner.stylo.alloc_text(true);
            if let Some(n) = inner.node_mut(node_id) {
                n.text_stylo_node = Some(tid);
            }
            sync_stylo_children_for_container(inner, Some(node_id));
            inner.stylo_dirty = true;
        }
        None => {}
    }
}

pub(crate) fn sync_node_attrs_to_stylo(inner: &mut ViewTreeInner, node_id: NodeId) {
    let Some(stylo_id) = inner.node(node_id).and_then(|n| n.stylo_node) else {
        return;
    };
    let entry = inner.sparse.pseudo_pointer.get(&node_id);
    if let Some(tag) = entry.and_then(|e| e.custom_tag.clone()) {
        inner.stylo.apply_mutation(StyloDomMutation::SetTag {
            node: stylo_id,
            tag_name: tag,
            namespace: ElementNamespace::Html,
        });
    }
    let attrs: Vec<ElementAttr> = entry
        .map(|e| {
            e.attrs
                .iter()
                .map(|(k, v)| ElementAttr {
                    name: k.clone(),
                    value: v.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    inner.stylo.apply_mutation(StyloDomMutation::SetAttributes {
        node: stylo_id,
        attrs,
    });
    inner.stylo_dirty = true;
}

pub(crate) fn notify_selector_state_change(inner: &mut ViewTreeInner, node_id: NodeId) {
    let mut cur = Some(node_id);
    while let Some(id) = cur {
        let Some((stylo_id, prev_state, parent)) = inner
            .node(id)
            .filter(|n| n.alive())
            .map(|n| (n.stylo_node, n.last_stylo_state, n.parent))
        else {
            break;
        };
        if let Some(sid) = stylo_id {
            let next_state = compute_dom_element_state(inner, id);
            if next_state != prev_state {
                if let Some(n) = inner.node_mut(id) {
                    n.last_stylo_state = next_state;
                }
                inner.stylo.apply_mutation(StyloDomMutation::SetState {
                    node: sid,
                    state: next_state,
                });
                inner.stylo_dirty = true;
                inner.paint_dirty = true;
            }
        }
        cur = parent;
    }
}

pub(crate) fn notify_children_mutation_inner(inner: &mut ViewTreeInner, parent: Option<NodeId>) {
    sync_stylo_children_for_container(inner, parent);
    inner.stylo_dirty = true;
    inner.paint_dirty = true;
}

pub(crate) fn sync_viewport_and_flush_restyle(inner: &mut ViewTreeInner, vw: f32, vh: f32) -> bool {
    let mut viewport_changed = false;
    if vw > 0.0 && vh > 0.0 {
        inner.stylo.set_viewport(vw, vh, inner.scale_factor);
        let (prev_w, prev_h) = inner.last_viewport;
        if (prev_w - vw).abs() > f32::EPSILON || (prev_h - vh).abs() > f32::EPSILON {
            inner.last_viewport = (vw, vh);
            inner.stylo_dirty = true;
            inner.paint_dirty = true;
            viewport_changed = true;
        }
    }
    flush_pending_restyle(inner);
    viewport_changed
}

pub(crate) fn flush_pending_restyle(inner: &mut ViewTreeInner) {
    if !inner.stylo_dirty && !inner.fonts_keyframes_dirty {
        return;
    }
    let sync_kf_fonts = inner.fonts_keyframes_dirty;
    let report = inner.stylo.restyle();
    inner.stylo_dirty = false;
    inner.fonts_keyframes_dirty = false;
    let mut damaged: Vec<_> = report.nodes.into_iter().collect();
    damaged.sort_by_key(|(sid, _)| inner.stylo_to_view.get(sid).map_or(u32::MAX, |n| n.index));
    for (sid, dmg) in damaged {
        apply_stylo_node_damage(inner, sid, dmg);
    }
    if sync_kf_fonts {
        sync_keyframes_and_fonts(inner);
    }
}

fn apply_stylo_node_damage(inner: &mut ViewTreeInner, sid: StyloNodeId, dmg: StyloDamage) {
    let Some(&vid) = inner.stylo_to_view.get(&sid) else {
        return;
    };
    if inner.node(vid).is_none_or(|n| !n.alive() || n.is_anchor()) {
        return;
    }
    let is_synthetic = inner.node(vid).is_some_and(|n| n.is_synthetic_text_run());
    let stylo_paint_opt = inner.stylo.paint_style(sid);
    let mut paint = stylo_paint_opt.unwrap_or_default();
    let (text, _inh) = inner.stylo.text_style(sid).unwrap_or_default();
    let mut layout = if is_synthetic {
        LayoutStyle {
            display: DisplayMode::Inline,
            ..LayoutStyle::default()
        }
    } else {
        inner.stylo.layout_style(sid).unwrap_or_default()
    };
    let meta = if is_synthetic {
        None
    } else {
        inner.stylo.box_meta(sid)
    };
    if is_synthetic {
        paint = PaintStyle {
            text_color: paint.text_color,
            ..PaintStyle::default()
        };
    } else {
        apply_box_meta_fields(&mut layout, &mut paint, meta);
    }
    if let Some(reappliers) = inner.sparse.paint_slot_reappliers.get(&vid) {
        for reapply in reappliers {
            reapply(&mut paint);
        }
    }

    let is_initial = !inner.sparse.last_target_paints.contains_key(&vid);
    let has_layout_slot = inner.sparse.layout_slot_nodes.contains(&vid);
    let (prev_dp, _prev_layout, taffy_opt, text_changed) =
        write_styled_fields(inner, vid, dmg, has_layout_slot, paint, text, layout, meta);

    if is_initial {
        inner.sparse.last_target_paints.insert(vid, paint);
        inner
            .sparse
            .last_target_layouts
            .insert(vid, (layout, text.font_size));
        sync_semantic_visibility(inner, vid);
    } else {
        commit_node_transitions_from_style(inner, vid);
    }

    if dmg == StyloDamage::Relayout {
        commit_relayout_damage(
            inner,
            vid,
            prev_dp,
            layout,
            paint.transition,
            text.font_size,
            taffy_opt,
            is_initial,
            has_layout_slot,
        );
    }
    if text_changed || dmg == StyloDamage::Relayout {
        sync_node_text_context(inner, vid);
    }
    if stylo_paint_opt.is_some() {
        sync_node_css_animations(inner, vid, paint.animations);
    }
    sync_pseudo_elements_for_node(inner, vid, sid);
    inner.paint_dirty = true;
}

#[allow(clippy::too_many_arguments)]
fn write_styled_fields(
    inner: &mut ViewTreeInner,
    vid: NodeId,
    dmg: StyloDamage,
    has_layout_slot: bool,
    paint: PaintStyle,
    text: TextStyle,
    layout: LayoutStyle,
    meta: Option<krui_stylo_bridge::BoxMeta>,
) -> (
    (DisplayMode, PositionMode),
    LayoutStyle,
    Option<taffy::NodeId>,
    bool,
) {
    let node = inner.node_mut(vid).expect("checked alive above");
    let prev_dp = (node.style.layout.display, node.style.layout.position);
    let prev_l = node.style.layout;
    let t_changed = node.style.text != text;
    if t_changed {
        node.cached_text_layout = None;
        node.last_shaped_key = 0;
    }
    if let Some(m) = meta {
        node.set_visibility_hidden(m.visibility_hidden);
        node.set_pointer_events_none(m.pointer_events_none);
    }
    node.style.paint = paint;
    node.style.text = text;
    if dmg == StyloDamage::Relayout && !has_layout_slot {
        node.style.layout = layout;
    } else {
        if let (Some(prev_edges), Some(next_edges)) =
            (node.style.layout.border_edges, layout.border_edges)
        {
            if (next_edges.bottom - prev_edges.bottom).abs() > 1e-4 {
                node.style.paint.border_bottom_override = Some(next_edges.bottom);
            }
        }
        node.style.layout.z_index = layout.z_index;
        node.style.layout.has_z_index = layout.has_z_index;
        node.style.layout.scrollbar_width = layout.scrollbar_width;
    }
    (prev_dp, prev_l, node.taffy_node, t_changed)
}

#[allow(clippy::too_many_arguments)]
fn commit_relayout_damage(
    inner: &mut ViewTreeInner,
    vid: NodeId,
    prev_dp: (DisplayMode, PositionMode),
    next_layout: LayoutStyle,
    transition_spec: krui_style::TransitionSpec,
    font_size: f32,
    taffy_opt: Option<taffy::NodeId>,
    is_initial: bool,
    has_layout_slot: bool,
) {
    if !has_layout_slot {
        inner
            .keyframes
            .update_base_layout_if_tracked(vid, next_layout);
    }
    let next_dp = inner
        .node(vid)
        .map_or((DisplayMode::Flex, PositionMode::Relative), |n| {
            (n.style.layout.display, n.style.layout.position)
        });
    sync_node_layout_mode_if_changed(inner, vid, prev_dp, next_dp);
    sync_semantic_visibility(inner, vid);
    let transition_sampled = if !is_initial {
        commit_node_layout_transition(inner, vid, transition_spec, next_layout, font_size)
    } else {
        None
    };
    if let Some((sampled_layout, sampled_fs)) = transition_sampled {
        apply_effective_node_layout_to_taffy(inner, vid, sampled_layout, sampled_fs);
    } else if let Some(tn) = taffy_opt {
        if !has_layout_slot && !inner.keyframes.has_node(vid) {
            inner.layout_engine.mark_dirty(tn);
        }
    }
}

fn sync_keyframes_and_fonts(inner: &mut ViewTreeInner) {
    let kf_defs = inner.stylo.keyframes_defs();
    inner.keyframes.definitions.clear();
    for def in kf_defs {
        inner.keyframes.definitions.insert(def.name.clone(), def);
    }
    resync_all_keyframes(inner);

    let ff_rules = inner
        .stylo
        .font_face_rules_with(|url| resolve_embedded_font(&inner.embedded_fonts, url));
    if inner.text_engine.register_font_faces(&ff_rules) {
        let mut touched_taffy = Vec::new();
        for n in &mut inner.nodes {
            if n.alive() && !n.is_anchor() {
                n.cached_text_layout = None;
                n.last_shaped_key = 0;
                if let Some(tn) = n.taffy_node {
                    touched_taffy.push(tn);
                }
            }
        }
        for tn in touched_taffy {
            inner.layout_engine.mark_dirty(tn);
        }
        inner.paint_dirty = true;
    }
}

pub(crate) fn resolve_canvas_clear_color(inner: &ViewTreeInner) -> (Color, Option<NodeId>) {
    let (color, sid_opt) = inner
        .stylo
        .resolve_canvas_background(inner.stylo_doc_root, inner.stylo_body_root);
    let prop_node = sid_opt.and_then(|sid| inner.stylo_to_view.get(&sid).copied());
    (color, prop_node)
}

pub(crate) fn compute_dom_element_state(inner: &ViewTreeInner, node_id: NodeId) -> DomElementState {
    let mut state = DomElementState::EMPTY;
    if ancestor_chain_contains(inner, inner.hovered_node, node_id) {
        state = state.union(DomElementState::HOVER);
    }
    let active_target = inner
        .pressed_node
        .filter(|&p| inner.is_node_active_visual(p));
    if inner.is_node_active_visual(node_id)
        || ancestor_chain_contains(inner, active_target, node_id)
    {
        state = state.union(DomElementState::ACTIVE);
    }
    let focused_id = inner
        .a11y_engine
        .focused_id()
        .and_then(|aid| NodeId::try_from(aid).ok());
    if focused_id == Some(node_id) {
        state = state.union(DomElementState::FOCUS);
        if inner.is_node_focus_visible(node_id) {
            state = state.union(DomElementState::FOCUS_VISIBLE);
        }
    }
    if ancestor_chain_contains(inner, focused_id, node_id) {
        state = state.union(DomElementState::FOCUS_WITHIN);
    }
    state = add_node_control_states(inner, node_id, state);
    state
}

fn add_node_control_states(
    inner: &ViewTreeInner,
    node_id: NodeId,
    mut state: DomElementState,
) -> DomElementState {
    if let Some(n) = inner.node(node_id) {
        state = state.union(if n.is_disabled() {
            DomElementState::DISABLED
        } else {
            DomElementState::ENABLED
        });
        if n.is_checked() {
            state = state.union(DomElementState::CHECKED);
        }
        state = state.union(if n.is_invalid() {
            DomElementState::INVALID
        } else {
            DomElementState::VALID
        });
        if n.role.is_text_editable() && n.text.is_empty() {
            state = state.union(DomElementState::PLACEHOLDER_SHOWN);
        }
    }
    if let Some(ctrl) = inner.control(node_id) {
        if ctrl.is_required {
            state = state.union(DomElementState::REQUIRED);
        }
    }
    let pp = inner.sparse.pseudo_pointer.get(&node_id);
    let has_attr = |k: &str| pp.is_some_and(|p| p.attrs.iter().any(|(ak, _)| ak == k));
    if has_attr("indeterminate") {
        state = state.union(DomElementState::INDETERMINATE);
    }
    state = state.union(if has_attr("readonly") {
        DomElementState::READ_ONLY
    } else {
        DomElementState::READ_WRITE
    });
    if pp.is_some_and(|p| p.popover_open) || has_attr("open") {
        state = state.union(DomElementState::OPEN);
    }
    if has_attr("modal") {
        state = state.union(DomElementState::MODAL);
    }
    state
}

fn ancestor_chain_contains(
    inner: &ViewTreeInner,
    mut cursor: Option<NodeId>,
    target: NodeId,
) -> bool {
    while let Some(id) = cursor {
        if id == target {
            return true;
        }
        cursor = inner.node(id).and_then(|n| n.parent);
    }
    false
}

fn resolve_embedded_font(fonts: &HashMap<String, Arc<[u8]>>, url: &str) -> Option<Arc<[u8]>> {
    let trimmed = url.trim_start_matches("./");
    fonts.get(url).or_else(|| fonts.get(trimmed)).cloned()
}
