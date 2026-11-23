//! Subtree unmounting, slot recycling, and full-tree reset lifecycle helpers.

use crate::view_tree::sparse_tables::{collect_unmount_postorder, detach_node_from_parent};
use crate::view_tree::{A11yId, NodeId, ViewTree, ViewTreeInner};
use krui_signals::{Effect, Scope};
use krui_stylo_bridge::StyloDomMutation;

impl ViewTreeInner {
    pub(crate) fn reset_for_replace_root(
        &mut self,
        next_scope: Scope,
    ) -> (Scope, Vec<Effect>, Vec<Scope>) {
        self.touch.reset();
        let old_scope = std::mem::replace(&mut self.root_scope, next_scope);
        let mut to_dispose = Vec::new();
        for idx in 0..self.nodes.len() {
            if self.nodes[idx].alive() {
                self.generations[idx] = self.generations[idx].wrapping_add(1);
                if let Some(tid) = self.nodes[idx].text_stylo_node.take() {
                    to_dispose.push(tid);
                }
                if let Some(sid) = self.nodes[idx].stylo_node.take() {
                    to_dispose.push(sid);
                }
            }
        }
        for sid in to_dispose {
            self.stylo
                .apply_mutation(StyloDomMutation::DisposeSubtree(sid));
        }
        self.stylo_to_view.clear();
        self.propagated_canvas_node = None;
        self.keyframes.clear_all();
        self.fixed_nodes.clear();
        self.open_popovers.clear();
        self.mount_batch_depth = 0;
        self.pending_mount_selector_eval = false;
        self.stylo_dirty = true;
        let (leftover_effects, leftover_scopes) = self.sparse.clear_all_for_reset();
        self.nodes.clear();
        self.free_nodes.clear();
        self.roots.clear();
        self.overlay.select_popup = None;
        self.layout_engine = krui_layout::LayoutEngine::new();
        self.a11y_engine = krui_a11y::AccessibilityEngine::new();
        self.hovered_node = None;
        self.pressed_node = None;
        self.selecting_node = None;
        self.scrollbar_drag = None;
        self.ime_target = None;
        self.interaction.reset();
        self.paint_dirty = true;
        (old_scope, leftover_effects, leftover_scopes)
    }
}

pub(crate) fn release_node_slot(
    inner: &mut ViewTreeInner,
    id: NodeId,
) -> (Vec<Effect>, Vec<Scope>) {
    if inner.hovered_node == Some(id) {
        inner.hovered_node = None;
    }
    if inner.pressed_node == Some(id) {
        inner.pressed_node = None;
    }
    if inner.selecting_node == Some(id) {
        inner.selecting_node = None;
    }
    if inner.ime_target == Some(id) {
        inner.ime_target = None;
    }
    if inner.scrollbar_drag.is_some_and(|(sid, _)| sid == id) {
        inner.scrollbar_drag = None;
    }
    if inner.propagated_canvas_node == Some(id) {
        inner.propagated_canvas_node = None;
    }
    inner.fixed_nodes.retain(|&fid| fid != id);
    inner.open_popovers.retain(|&pid| pid != id);
    inner.interaction.release_node(id);
    if inner
        .overlay
        .select_popup
        .as_ref()
        .is_some_and(|p| p.select_id == id)
    {
        inner.overlay.select_popup = None;
    }
    inner.a11y_engine.unregister_focusable(A11yId::from(id).0);
    inner.keyframes.purge_node(id);
    let (effects, scopes) = inner.sparse.purge_node(id);
    let idx = id.index as usize;
    let (taffy_to_remove, stylo_to_dispose, text_stylo_to_dispose) = inner
        .nodes
        .get_mut(idx)
        .map(|node| {
            let sn = node.stylo_node.take();
            let ts = node.text_stylo_node.take();
            let tn = node.clear_for_unmount();
            inner.generations[idx] = inner.generations[idx].wrapping_add(1);
            inner.free_nodes.push(id.index);
            (tn, sn, ts)
        })
        .unwrap_or((None, None, None));
    if let Some(ts) = text_stylo_to_dispose {
        inner
            .stylo
            .apply_mutation(StyloDomMutation::DisposeSubtree(ts));
    }
    if let Some(sn) = stylo_to_dispose {
        inner.stylo_to_view.remove(&sn);
        inner
            .stylo
            .apply_mutation(StyloDomMutation::DisposeSubtree(sn));
    }
    if let Some(tn) = taffy_to_remove {
        inner.layout_engine.remove_node(tn);
    }
    (effects, scopes)
}

pub(crate) fn unmount_node_inner(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
) -> Option<(Vec<Effect>, Vec<Scope>)> {
    inner.node(node_id)?;
    let parent_id = inner.node(node_id).and_then(|n| n.parent);
    let to_remove = collect_unmount_postorder(inner, node_id);
    let mut all_effects = Vec::new();
    let mut all_scopes = Vec::new();
    for id in to_remove {
        detach_node_from_parent(inner, id);
        let (effs, scps) = release_node_slot(inner, id);
        all_effects.extend(effs);
        all_scopes.extend(scps);
    }
    crate::view_tree::sync::notify_children_mutation_inner(inner, parent_id);
    inner.paint_dirty = true;
    Some((all_effects, all_scopes))
}

pub(crate) fn unmount_node_subtree(inner: &mut ViewTreeInner, node_id: NodeId) {
    if let Some((effects, scopes)) = unmount_node_inner(inner, node_id) {
        for eff in effects {
            eff.dispose();
        }
        for scope in scopes {
            scope.dispose();
        }
    }
}

impl ViewTree {
    /// Unmounts `node_id`, any nested `<if>`/`<for>` anchor subtrees, and all sparse component entries.
    pub fn unmount(&mut self, node_id: NodeId) -> bool {
        let Some((effects, scopes)) = unmount_node_inner(&mut self.inner.borrow_mut(), node_id)
        else {
            return false;
        };
        for eff in effects {
            eff.dispose();
        }
        for scope in scopes {
            scope.dispose();
        }
        true
    }
}
