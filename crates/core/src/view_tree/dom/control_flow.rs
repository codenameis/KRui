//! Reactive control-flow mounters (`<if>`/`<else-if>`/`<else>` and `<for>` + `<empty>`) for `ViewTree`.
//!
//! Neither `<if>` nor `<for>` creates a Taffy layout box: they use a lightweight virtual anchor
//! node (`is_anchor() == true`, `taffy_node == None`) inside the parent's `children` list and
//! splice their active child `NodeId`s directly into the parent in exact sibling order.

use crate::view_tree::node::ElementNode;
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_signals::{untracked, Effect, Scope, Signal};
use std::cell::RefCell;
use std::rc::Rc;

/// Branch mounter closure used by [`ViewTree::mount_if`] and [`ViewTree::mount_for`].
pub type BranchMountFn = Rc<dyn Fn(&mut ViewTree, Option<NodeId>) -> Vec<NodeId>>;

struct IfBranchState {
    active_branch: Option<usize>,
    active_scope: Option<Scope>,
    active_nodes: Vec<NodeId>,
    initialized: bool,
}

impl Drop for IfBranchState {
    fn drop(&mut self) {
        if let Some(scope) = self.active_scope.take() {
            scope.dispose();
        }
    }
}

struct KeyedForEntry<T: 'static, K> {
    key: K,
    scope: Scope,
    item_sig: Signal<T>,
    nodes: Vec<NodeId>,
}

struct ForListState<T: 'static, K> {
    entries: Vec<KeyedForEntry<T, K>>,
    empty_scope: Option<Scope>,
    empty_nodes: Vec<NodeId>,
}

impl<T: 'static, K> Drop for ForListState<T, K> {
    fn drop(&mut self) {
        for entry in self.entries.drain(..) {
            entry.scope.dispose();
        }
        if let Some(escope) = self.empty_scope.take() {
            escope.dispose();
        }
    }
}

impl ViewTree {
    /// Allocates a non-layout, non-visual anchor marker node in `parent` (used to preserve sibling
    /// insertion order for `<if>` and `<for>` blocks without creating a wrapper layout box).
    pub fn create_anchor_node(&mut self, parent: Option<NodeId>) -> NodeId {
        let mut inner = self.inner.borrow_mut();
        let (slot_idx, generation) = inner.allocate_slot();
        let node_id = NodeId::new(slot_idx, generation);
        let retained = ElementNode::new_anchor(node_id, parent);

        if (slot_idx as usize) < inner.nodes.len() {
            inner.nodes[slot_idx as usize] = retained;
        } else {
            inner.nodes.push(retained);
        }
        let _ = inner.ensure_anchor(node_id);

        if let Some(pid) = parent {
            if let Some(pnode) = inner.node_mut(pid) {
                pnode.children.push(node_id);
            }
        } else {
            inner.roots.push(node_id);
        }
        node_id
    }

    /// Places `ordered_nodes` immediately before `anchor_id` in `parent`'s child list (or root list)
    /// and synchronizes Taffy's layout children in exact sibling order.
    pub fn place_nodes_before_anchor(
        &mut self,
        parent: Option<NodeId>,
        anchor_id: NodeId,
        ordered_nodes: &[NodeId],
    ) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(anchor_id).is_none() {
            return;
        }
        {
            let anchor_data = inner.ensure_anchor(anchor_id);
            anchor_data.mounted_nodes.clear();
            anchor_data.mounted_nodes.extend_from_slice(ordered_nodes);
        }

        let mut flat_nodes = Vec::with_capacity(ordered_nodes.len());
        expand_anchor_nodes_recursive(&inner, ordered_nodes, &mut flat_nodes);

        if let Some(pid) = parent {
            sync_parent_children_before_anchor(&mut inner, pid, anchor_id, flat_nodes);
        } else {
            sync_root_children_before_anchor(&mut inner, anchor_id, flat_nodes);
        }
        inner.paint_dirty = true;
    }

    /// Mounts a reactive `<if>` / `<else-if>` / `<else>` block directly into `parent` without
    /// creating a wrapper layout box.
    pub fn mount_if(
        &mut self,
        parent: Option<NodeId>,
        branch_selector: impl Fn() -> Option<usize> + 'static,
        branch_mounters: Vec<BranchMountFn>,
    ) -> NodeId {
        let anchor_id = self.create_anchor_node(parent);
        let state = Rc::new(RefCell::new(IfBranchState {
            active_branch: None,
            active_scope: None,
            active_nodes: Vec::new(),
            initialized: false,
        }));

        let parent_scope = Scope::current();
        let tree_weak = Rc::downgrade(&self.inner);
        let effect = Effect::new(move || {
            let next_branch = branch_selector();
            untracked(|| {
                parent_scope.run(|| {
                    let Some(inner_rc) = tree_weak.upgrade() else {
                        return;
                    };
                    let mut tree = ViewTree { inner: inner_rc };
                    reconcile_if_branch(
                        &mut tree,
                        parent,
                        anchor_id,
                        &mut state.borrow_mut(),
                        next_branch,
                        &branch_mounters,
                    );
                })
            });
        });

        self.inner.borrow_mut().push_effect(anchor_id, effect);
        anchor_id
    }

    /// Mounts a reactive keyed `<for each="{item in list}" key="{item.id}">` list (plus optional
    /// `<empty>` fallback) directly into `parent` without creating a wrapper layout box.
    pub fn mount_for<T, K>(
        &mut self,
        parent: Option<NodeId>,
        items_fn: impl Fn() -> Vec<T> + 'static,
        key_fn: impl Fn(&T) -> K + 'static,
        item_mounter: impl Fn(&mut ViewTree, Option<NodeId>, Signal<T>) -> Vec<NodeId> + 'static,
        empty_mounter: Option<BranchMountFn>,
    ) -> NodeId
    where
        T: Clone + 'static,
        K: Eq + Clone + 'static,
    {
        let anchor_id = self.create_anchor_node(parent);
        let state = Rc::new(RefCell::new(ForListState::<T, K> {
            entries: Vec::new(),
            empty_scope: None,
            empty_nodes: Vec::new(),
        }));
        let item_mounter = Rc::new(item_mounter);

        let parent_scope = Scope::current();
        let tree_weak = Rc::downgrade(&self.inner);
        let effect = Effect::new(move || {
            let items = items_fn();
            untracked(|| {
                parent_scope.run(|| {
                    let Some(inner_rc) = tree_weak.upgrade() else {
                        return;
                    };
                    let mut tree = ViewTree { inner: inner_rc };
                    reconcile_for_list(
                        &mut tree,
                        parent,
                        anchor_id,
                        &mut state.borrow_mut(),
                        items,
                        &key_fn,
                        &item_mounter,
                        empty_mounter.as_ref(),
                    );
                })
            });
        });

        self.inner.borrow_mut().push_effect(anchor_id, effect);
        anchor_id
    }
}

fn expand_anchor_nodes_recursive(inner: &ViewTreeInner, nodes: &[NodeId], out: &mut Vec<NodeId>) {
    for &nid in nodes {
        let Some(n) = inner.node(nid) else {
            continue;
        };
        if n.is_anchor() {
            if let Some(anc) = inner.anchor(nid) {
                expand_anchor_nodes_recursive(inner, &anc.mounted_nodes, out);
            }
        }
        out.push(nid);
    }
}

fn set_anchor_owned_scopes(tree: &mut ViewTree, anchor_id: NodeId, scopes: Vec<Scope>) {
    let mut inner = tree.inner.borrow_mut();
    if inner.node(anchor_id).is_some() {
        inner.ensure_anchor(anchor_id).owned_scopes = scopes;
    }
}

fn reconcile_if_branch(
    tree: &mut ViewTree,
    parent: Option<NodeId>,
    anchor_id: NodeId,
    st: &mut IfBranchState,
    next_branch: Option<usize>,
    branch_mounters: &[BranchMountFn],
) {
    if st.initialized && st.active_branch == next_branch {
        return;
    }
    st.initialized = true;
    st.active_branch = next_branch;
    let old_scope = st.active_scope.take();
    for old_nid in std::mem::take(&mut st.active_nodes) {
        tree.unmount(old_nid);
    }
    if let Some(scope) = old_scope {
        scope.dispose();
    }
    if let Some(idx) = next_branch.filter(|&i| i < branch_mounters.len()) {
        let mounter = branch_mounters[idx].clone();
        let new_scope = Scope::new();
        let mounted = new_scope.run(|| mounter(tree, parent));
        tree.place_nodes_before_anchor(parent, anchor_id, &mounted);
        set_anchor_owned_scopes(tree, anchor_id, vec![new_scope]);
        st.active_scope = Some(new_scope);
        st.active_nodes = mounted;
    } else {
        set_anchor_owned_scopes(tree, anchor_id, Vec::new());
        tree.place_nodes_before_anchor(parent, anchor_id, &[]);
    }
    tree.notify_children_selector_mutation(parent);
}

#[allow(clippy::too_many_arguments)]
fn reconcile_for_list<T, K>(
    tree: &mut ViewTree,
    parent: Option<NodeId>,
    anchor_id: NodeId,
    st: &mut ForListState<T, K>,
    items: Vec<T>,
    key_fn: &impl Fn(&T) -> K,
    item_mounter: &Rc<impl Fn(&mut ViewTree, Option<NodeId>, Signal<T>) -> Vec<NodeId> + 'static>,
    empty_mounter: Option<&BranchMountFn>,
) where
    T: Clone + 'static,
    K: Eq + Clone + 'static,
{
    if items.is_empty() {
        apply_empty_for_list(tree, parent, anchor_id, st, empty_mounter);
        tree.notify_children_selector_mutation(parent);
        return;
    }
    clear_empty_for_branch(tree, st);
    let keyed_new: Vec<(K, T)> = items.into_iter().map(|it| (key_fn(&it), it)).collect();
    let mut old_entries: Vec<Option<KeyedForEntry<T, K>>> =
        st.entries.drain(..).map(Some).collect();
    prune_removed_for_entries(tree, &mut old_entries, &keyed_new);
    let mut next_entries = Vec::with_capacity(keyed_new.len());
    for (key, item_val) in keyed_new {
        if let Some(entry) = take_matching_for_entry(&mut old_entries, &key) {
            entry.item_sig.set(item_val);
            next_entries.push(entry);
        } else {
            next_entries.push(mount_single_for_entry(
                tree,
                parent,
                key,
                item_val,
                item_mounter,
            ));
        }
    }
    dispose_for_entries(tree, old_entries.into_iter().flatten().collect());
    let ordered_ids: Vec<NodeId> = next_entries
        .iter()
        .flat_map(|e| e.nodes.iter().copied())
        .collect();
    let scopes: Vec<Scope> = next_entries.iter().map(|e| e.scope).collect();
    set_anchor_owned_scopes(tree, anchor_id, scopes);
    tree.place_nodes_before_anchor(parent, anchor_id, &ordered_ids);
    st.entries = next_entries;
    tree.notify_children_selector_mutation(parent);
}

fn clear_empty_for_branch<T, K>(tree: &mut ViewTree, st: &mut ForListState<T, K>) {
    if let Some(escope) = st.empty_scope.take() {
        for nid in std::mem::take(&mut st.empty_nodes) {
            tree.unmount(nid);
        }
        escope.dispose();
    }
}

fn prune_removed_for_entries<T: 'static, K: Eq>(
    tree: &mut ViewTree,
    old_entries: &mut [Option<KeyedForEntry<T, K>>],
    keyed_new: &[(K, T)],
) {
    for slot in old_entries {
        if slot
            .as_ref()
            .is_some_and(|e| !keyed_new.iter().any(|(k, _)| *k == e.key))
        {
            if let Some(removed) = slot.take() {
                dispose_single_for_entry(tree, removed);
            }
        }
    }
}

fn take_matching_for_entry<T: 'static, K: Eq>(
    old_entries: &mut [Option<KeyedForEntry<T, K>>],
    key: &K,
) -> Option<KeyedForEntry<T, K>> {
    old_entries.iter_mut().find_map(|slot| {
        slot.as_ref()
            .is_some_and(|e| &e.key == key)
            .then(|| slot.take())
            .flatten()
    })
}

fn sync_parent_children_before_anchor(
    inner: &mut ViewTreeInner,
    pid: NodeId,
    anchor_id: NodeId,
    flat_nodes: Vec<NodeId>,
) {
    if inner.node(pid).and_then(|p| p.taffy_node).is_none() {
        return;
    }
    if let Some(pnode) = inner.node_mut(pid) {
        pnode.children.retain(|cid| !flat_nodes.contains(cid));
        let pos = pnode
            .children
            .iter()
            .position(|&c| c == anchor_id)
            .unwrap_or(pnode.children.len());
        pnode.children.splice(pos..pos, flat_nodes);
    }
    crate::view_tree::layout_modes::sync_container_taffy_children(inner, Some(pid));
    crate::view_tree::layout_modes::sync_root_and_fixed_taffy(inner);
}

fn sync_root_children_before_anchor(
    inner: &mut ViewTreeInner,
    anchor_id: NodeId,
    flat_nodes: Vec<NodeId>,
) {
    inner.roots.retain(|cid| !flat_nodes.contains(cid));
    let pos = inner
        .roots
        .iter()
        .position(|&c| c == anchor_id)
        .unwrap_or(inner.roots.len());
    inner.roots.splice(pos..pos, flat_nodes);
    crate::view_tree::layout_modes::sync_root_and_fixed_taffy(inner);
}

fn apply_empty_for_list<T: 'static, K>(
    tree: &mut ViewTree,
    parent: Option<NodeId>,
    anchor_id: NodeId,
    st: &mut ForListState<T, K>,
    empty_mounter: Option<&BranchMountFn>,
) {
    dispose_for_entries(tree, std::mem::take(&mut st.entries));
    if let Some(empty_fn) = empty_mounter {
        if st.empty_scope.is_none() {
            let escope = Scope::new();
            let enodes = escope.run(|| empty_fn(tree, parent));
            tree.place_nodes_before_anchor(parent, anchor_id, &enodes);
            set_anchor_owned_scopes(tree, anchor_id, vec![escope]);
            st.empty_scope = Some(escope);
            st.empty_nodes = enodes;
        }
    } else {
        set_anchor_owned_scopes(tree, anchor_id, Vec::new());
        tree.place_nodes_before_anchor(parent, anchor_id, &[]);
    }
    tree.notify_children_selector_mutation(parent);
}

fn mount_single_for_entry<T: Clone + 'static, K>(
    tree: &mut ViewTree,
    parent: Option<NodeId>,
    key: K,
    item_val: T,
    item_mounter: &Rc<impl Fn(&mut ViewTree, Option<NodeId>, Signal<T>) -> Vec<NodeId> + 'static>,
) -> KeyedForEntry<T, K> {
    let iscope = Scope::new();
    let mounter = item_mounter.clone();
    let (item_sig, nodes) = iscope.run(|| {
        let sig = Signal::new(item_val);
        let mounted = mounter(tree, parent, sig);
        (sig, mounted)
    });
    KeyedForEntry {
        key,
        scope: iscope,
        item_sig,
        nodes,
    }
}

fn dispose_single_for_entry<T: 'static, K>(tree: &mut ViewTree, entry: KeyedForEntry<T, K>) {
    for nid in entry.nodes {
        tree.unmount(nid);
    }
    entry.scope.dispose();
}

fn dispose_for_entries<T: 'static, K>(tree: &mut ViewTree, entries: Vec<KeyedForEntry<T, K>>) {
    for entry in entries {
        dispose_single_for_entry(tree, entry);
    }
}
