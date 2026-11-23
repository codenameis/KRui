//! `DisplayMode::Contents`, `PositionMode::Fixed`, rich inline `StyledTextSpan` paragraph
//! bindings, and subtree mount batching helpers for `ViewTree`.

use crate::view_tree::content_box::sync_node_text_context;
use crate::view_tree::node::is_node_text_selectable;
use crate::view_tree::{HoveredCursorKind, NodeId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_signals::Effect;
use krui_style::{sanitize_untrusted_text, DisplayMode, PositionMode, StyledTextSpan};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

impl ViewTree {
    /// Begins a subtree mount batch, deferring Stylo restyle passes until
    /// the matching outermost [`ViewTree::end_mount_batch`] call completes.
    pub fn begin_mount_batch(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.mount_batch_depth = inner.mount_batch_depth.saturating_add(1);
    }

    /// Ends the active mount batch and flushes any pending Stylo cascade updates.
    pub fn end_mount_batch(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.mount_batch_depth = inner.mount_batch_depth.saturating_sub(1);
        if inner.mount_batch_depth == 0 {
            inner.pending_mount_selector_eval = false;
            let (vw, vh) = inner.last_viewport;
            crate::view_tree::sync::sync_viewport_and_flush_restyle(&mut inner, vw, vh);
        }
    }

    /// Attaches static styled rich paragraph spans to `node_id` for unified single-paragraph wrapping.
    pub fn set_rich_spans(&mut self, node_id: NodeId, spans: Vec<StyledTextSpan>) {
        let mut inner = self.inner.borrow_mut();
        apply_rich_spans_inner(&mut inner, node_id, spans);
    }

    /// Reactively updates multi-span rich paragraph runs on `node_id` from `eval`.
    pub fn bind_dynamic_rich_spans(
        &mut self,
        node_id: NodeId,
        eval: impl Fn() -> Vec<StyledTextSpan> + 'static,
    ) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let tree_weak = Rc::downgrade(&self.inner);
        let effect = Effect::new(move || {
            let next_spans = eval();
            if let Some(rc) = tree_weak.upgrade() {
                let mut inner = rc.borrow_mut();
                apply_rich_spans_inner(&mut inner, node_id, next_spans);
            }
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }
}

fn apply_rich_spans_inner(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    mut spans: Vec<StyledTextSpan>,
) {
    let mut plain = String::new();
    for sp in &mut spans {
        sp.text = sanitize_untrusted_text(&sp.text);
        plain.push_str(&sp.text);
    }
    if spans.is_empty() {
        inner.sparse.rich_spans.remove(&node_id);
    } else {
        inner.sparse.rich_spans.insert(node_id, Arc::from(spans));
    }
    if let Some(node) = inner.node_mut(node_id) {
        node.text = plain;
        node.cached_text_layout = None;
        node.set_text_dirty(true);
    } else {
        return;
    }
    sync_node_text_context(inner, node_id);
    inner.paint_dirty = true;
}

pub(crate) fn hash_rich_spans(spans: &[StyledTextSpan]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    spans.len().hash(&mut h);
    for sp in spans {
        sp.text.hash(&mut h);
        sp.font_weight.hash(&mut h);
        sp.italic.hash(&mut h);
        sp.font_family.hash(&mut h);
        sp.color.map(|c| [c.r, c.g, c.b, c.a]).hash(&mut h);
    }
    h.finish()
}

fn has_contents_ancestor(inner: &ViewTreeInner, start: NodeId) -> bool {
    let mut cur = Some(start);
    while let Some(cid) = cur {
        let Some(n) = inner.node(cid) else {
            break;
        };
        if n.style.layout.display == DisplayMode::Contents {
            return true;
        }
        cur = n.parent;
    }
    false
}

fn find_concrete_layout_container(inner: &ViewTreeInner, start: Option<NodeId>) -> Option<NodeId> {
    let mut cur = start;
    while let Some(cid) = cur {
        let n = inner.node(cid)?;
        if n.style.layout.display != DisplayMode::Contents {
            return Some(cid);
        }
        cur = n.parent;
    }
    None
}

fn collect_flow_taffy_children(
    inner: &ViewTreeInner,
    dom_ids: &[NodeId],
    out: &mut Vec<taffy::NodeId>,
) {
    for &cid in dom_ids {
        let Some(n) = inner.node(cid) else {
            continue;
        };
        if n.is_anchor() || n.style.layout.position == PositionMode::Fixed {
            continue;
        }
        if n.style.layout.display == DisplayMode::Contents {
            collect_flow_taffy_children(inner, &n.children, out);
        } else if let Some(tn) = n.taffy_node {
            out.push(tn);
        }
    }
}

pub(crate) fn sync_root_and_fixed_taffy(inner: &mut ViewTreeInner) {
    let roots = inner.roots.clone();
    let mut flow_roots = Vec::with_capacity(roots.len());
    collect_flow_taffy_children(inner, &roots, &mut flow_roots);
    let ViewTreeInner {
        fixed_nodes,
        nodes,
        layout_engine,
        ..
    } = inner;
    fixed_nodes.retain(|id| {
        nodes
            .get(id.index as usize)
            .filter(|n| n.alive() && n.id.generation == id.generation)
            .is_some_and(|n| !n.is_anchor() && n.style.layout.position == PositionMode::Fixed)
    });
    let fixed_taffy: Vec<taffy::NodeId> = fixed_nodes
        .iter()
        .filter_map(|id| {
            nodes
                .get(id.index as usize)
                .filter(|n| n.alive() && n.id.generation == id.generation)
                .and_then(|n| n.taffy_node)
        })
        .collect();
    layout_engine.set_root_and_fixed_children(&flow_roots, &fixed_taffy);
}

pub(crate) fn sync_container_taffy_children(inner: &mut ViewTreeInner, container: Option<NodeId>) {
    let Some(pid) = find_concrete_layout_container(inner, container) else {
        sync_root_and_fixed_taffy(inner);
        return;
    };
    let Some((parent_taffy, child_ids)) = inner
        .node(pid)
        .and_then(|p| Some((p.taffy_node?, p.children.clone())))
    else {
        return;
    };
    let mut taffy_children = Vec::with_capacity(child_ids.len());
    collect_flow_taffy_children(inner, &child_ids, &mut taffy_children);
    inner
        .layout_engine
        .set_children(Some(parent_taffy), &taffy_children);
}

pub(crate) fn sync_after_dom_attach(
    inner: &mut ViewTreeInner,
    parent: Option<NodeId>,
    node_id: NodeId,
    taffy_node: taffy::NodeId,
) {
    let (disp, pos) = inner
        .node(node_id)
        .map_or((DisplayMode::Flex, PositionMode::Relative), |n| {
            (n.style.layout.display, n.style.layout.position)
        });
    if pos == PositionMode::Fixed {
        if !inner.fixed_nodes.contains(&node_id) {
            inner.fixed_nodes.push(node_id);
        }
        sync_root_and_fixed_taffy(inner);
        return;
    }
    let Some(pid) = parent else {
        sync_root_and_fixed_taffy(inner);
        return;
    };
    if disp == DisplayMode::Contents || has_contents_ancestor(inner, pid) {
        sync_container_taffy_children(inner, Some(pid));
    } else if let Some(parent_taffy) = inner.node(pid).and_then(|p| p.taffy_node) {
        inner.layout_engine.append_child(parent_taffy, taffy_node);
    }
}

pub(crate) fn sync_after_dom_detach(
    inner: &mut ViewTreeInner,
    parent: Option<NodeId>,
    was_fixed: bool,
) {
    if was_fixed || parent.is_none() {
        sync_root_and_fixed_taffy(inner);
    }
    if let Some(pid) = parent {
        if has_contents_ancestor(inner, pid) {
            sync_container_taffy_children(inner, Some(pid));
        }
    }
}

pub(crate) fn sync_flow_children_effective_layout(inner: &mut ViewTreeInner, container_id: NodeId) {
    let child_ids = inner
        .node(container_id)
        .map(|n| n.children.clone())
        .unwrap_or_default();
    for cid in child_ids {
        sync_flow_child_recursive(inner, cid);
    }
}

fn sync_flow_child_recursive(inner: &mut ViewTreeInner, cid: NodeId) {
    let Some((is_anchor, disp, tn_opt, lay)) = inner.node(cid).filter(|n| n.alive()).map(|n| {
        (
            n.is_anchor(),
            n.style.layout.display,
            n.taffy_node,
            n.style.layout,
        )
    }) else {
        return;
    };
    if is_anchor {
        let mounted = inner
            .anchor(cid)
            .map(|a| a.mounted_nodes.clone())
            .unwrap_or_default();
        for mid in mounted {
            sync_flow_child_recursive(inner, mid);
        }
        return;
    }
    if disp == DisplayMode::Contents {
        sync_flow_children_effective_layout(inner, cid);
    } else if let Some(tn) = tn_opt {
        crate::view_tree::semantics::set_effective_taffy_layout(inner, cid, tn, lay);
    }
}

pub(crate) fn sync_node_layout_mode_if_changed(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    (prev_d, prev_p): (DisplayMode, PositionMode),
    (next_d, next_p): (DisplayMode, PositionMode),
) {
    let fixed_changed =
        prev_p != next_p && (prev_p == PositionMode::Fixed || next_p == PositionMode::Fixed);
    let contents_changed =
        prev_d != next_d && (prev_d == DisplayMode::Contents || next_d == DisplayMode::Contents);
    if prev_d != next_d || contents_changed {
        sync_flow_children_effective_layout(inner, node_id);
    }
    if !fixed_changed && !contents_changed {
        return;
    }
    if next_p == PositionMode::Fixed {
        if !inner.fixed_nodes.contains(&node_id) {
            inner.fixed_nodes.push(node_id);
        }
    } else {
        inner.fixed_nodes.retain(|&id| id != node_id);
    }
    let parent = inner.node(node_id).and_then(|n| n.parent);
    sync_root_and_fixed_taffy(inner);
    if parent.is_some() {
        sync_container_taffy_children(inner, parent);
    }
}

pub(crate) fn collect_visible_subtree_text(
    inner: &ViewTreeInner,
    root_id: NodeId,
) -> Option<String> {
    let mut buf = String::new();
    append_visible_subtree_text(inner, root_id, &mut buf);
    let trimmed = buf.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn append_visible_subtree_text(inner: &ViewTreeInner, nid: NodeId, out: &mut String) {
    let Some(n) = inner.node(nid) else {
        return;
    };
    if n.is_anchor() || n.style.layout.display == DisplayMode::None || n.is_secret() {
        return;
    }
    let t = n.text.trim();
    if !t.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(t);
    }
    for &c in &n.children {
        append_visible_subtree_text(inner, c, out);
    }
}

impl ViewTree {
    /// Returns the appropriate OS cursor shape (`Default`, `Pointer`, `Text`) for the hovered element.
    pub fn hovered_cursor_kind(&self) -> HoveredCursorKind {
        let inner = self.inner.borrow();
        if inner
            .overlay
            .select_popup
            .as_ref()
            .is_some_and(|p| p.pointer_hovered)
        {
            return HoveredCursorKind::Pointer;
        }
        let Some(hid) = inner.hovered_node else {
            return HoveredCursorKind::Default;
        };
        let Some(node) = inner.node(hid) else {
            return HoveredCursorKind::Default;
        };
        if node.is_disabled() {
            return HoveredCursorKind::Default;
        }
        let has_click = inner.listeners(hid).is_some_and(|l| l.on_click.is_some());
        let has_label_for = inner.control(hid).is_some_and(|c| c.label_for.is_some());
        if is_node_text_selectable(node) || node.role.is_text_editable() {
            HoveredCursorKind::Text
        } else if matches!(
            node.role,
            SemanticRole::Button
                | SemanticRole::Checkbox
                | SemanticRole::RadioButton
                | SemanticRole::Slider
                | SemanticRole::ComboBox
                | SemanticRole::Link
        ) || node._tag == "label"
            || has_label_for
            || has_click
        {
            HoveredCursorKind::Pointer
        } else {
            HoveredCursorKind::Default
        }
    }
}
