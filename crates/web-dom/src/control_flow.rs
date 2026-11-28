//! Reactive `<if>` and keyed `<for>` control-flow reconciliation anchored at DOM comments.

use crate::backend::DomNodeId;
use crate::tree::{WebBranchMountFn, WebDomTree};
use krui_signals::{untracked, Effect, Scope, Signal};
use std::cell::RefCell;
use std::rc::Rc;

impl WebDomTree {
    /// Mounts a standalone `<if>` control-flow anchor under `parent` and attaches reactive branches.
    pub fn mount_if(
        &mut self,
        parent: Option<DomNodeId>,
        branch_fn: impl Fn() -> Option<usize> + 'static,
        branches: Vec<WebBranchMountFn>,
    ) -> DomNodeId {
        let anchor = self.inner.borrow_mut().backend.create_comment("if");
        self.record_created_node(anchor, parent, "#anchor", false, None);
        self.inner.borrow_mut().backend.append_child(parent, anchor);
        self.attach_if_to_anchor(parent, anchor, branch_fn, branches)
    }

    /// Attaches conditional reactive branches immediately before an existing `anchor` comment node.
    pub fn attach_if_to_anchor(
        &mut self,
        parent: Option<DomNodeId>,
        anchor: DomNodeId,
        branch_fn: impl Fn() -> Option<usize> + 'static,
        branches: Vec<WebBranchMountFn>,
    ) -> DomNodeId {
        let tree = self.clone();
        let branches_rc = Rc::new(branches);
        let state = Rc::new(RefCell::new(IfState::default()));
        let cleanup_state = Rc::clone(&state);
        self.set_anchor_cleanup(anchor, move || {
            let mut st = cleanup_state.borrow_mut();
            st.mounted_nodes.clear();
            if let Some(scope) = st.child_scope.take() {
                scope.dispose();
            }
        });
        let parent_scope = Scope::current();
        let _effect = Effect::new(move || {
            let next_branch = branch_fn();
            untracked(|| {
                parent_scope.run(|| {
                    reconcile_if_branch(&tree, parent, anchor, &state, next_branch, &branches_rc);
                });
            });
        });
        anchor
    }

    /// Mounts a standalone `<for>` control-flow anchor under `parent` and attaches keyed list items.
    pub fn mount_for<T, K>(
        &mut self,
        parent: Option<DomNodeId>,
        list_fn: impl Fn() -> Vec<T> + 'static,
        key_fn: impl Fn(&T) -> K + 'static,
        body_fn: impl Fn(&mut Self, Option<DomNodeId>, Signal<T>) -> Vec<DomNodeId> + 'static,
        empty_fn: Option<WebBranchMountFn>,
    ) -> DomNodeId
    where
        T: Clone + 'static,
        K: Eq + Clone + 'static,
    {
        let anchor = self.inner.borrow_mut().backend.create_comment("for");
        self.record_created_node(anchor, parent, "#anchor", false, None);
        self.inner.borrow_mut().backend.append_child(parent, anchor);
        self.attach_for_to_anchor(parent, anchor, list_fn, key_fn, body_fn, empty_fn)
    }

    /// Reconciles keyed `<for>` children before `anchor` preserving DOM node identity and focus.
    pub fn attach_for_to_anchor<T, K>(
        &mut self,
        parent: Option<DomNodeId>,
        anchor: DomNodeId,
        list_fn: impl Fn() -> Vec<T> + 'static,
        key_fn: impl Fn(&T) -> K + 'static,
        body_fn: impl Fn(&mut Self, Option<DomNodeId>, Signal<T>) -> Vec<DomNodeId> + 'static,
        empty_fn: Option<WebBranchMountFn>,
    ) -> DomNodeId
    where
        T: Clone + 'static,
        K: Eq + Clone + 'static,
    {
        let tree = self.clone();
        let body_rc = Rc::new(body_fn);
        let state: Rc<RefCell<ForState<T, K>>> = Rc::new(RefCell::new(ForState::default()));
        let cleanup_state = Rc::clone(&state);
        self.set_anchor_cleanup(anchor, move || {
            let mut st = cleanup_state.borrow_mut();
            for row in st.rows.drain(..) {
                row.scope.dispose();
            }
            st.empty_nodes.clear();
            if let Some(escope) = st.empty_scope.take() {
                escope.dispose();
            }
        });
        let parent_scope = Scope::current();
        let _effect = Effect::new(move || {
            let items = list_fn();
            untracked(|| {
                parent_scope.run(|| {
                    let next_keys: Vec<K> = items.iter().map(&key_fn).collect();
                    let mut tree_mut = tree.clone();
                    reconcile_for_items(
                        &mut tree_mut,
                        parent,
                        anchor,
                        &state,
                        items,
                        next_keys,
                        &body_rc,
                        &empty_fn,
                    );
                });
            });
        });
        anchor
    }
}

#[derive(Default)]
struct IfState {
    initialized: bool,
    current_branch: Option<usize>,
    child_scope: Option<Scope>,
    mounted_nodes: Vec<DomNodeId>,
}

fn reconcile_if_branch(
    tree: &WebDomTree,
    parent: Option<DomNodeId>,
    anchor: DomNodeId,
    state: &Rc<RefCell<IfState>>,
    next_branch: Option<usize>,
    branches_rc: &Rc<Vec<WebBranchMountFn>>,
) {
    let (old_scope, old_nodes) = {
        let mut st = state.borrow_mut();
        if st.initialized && st.current_branch == next_branch {
            return;
        }
        st.initialized = true;
        st.current_branch = next_branch;
        (st.child_scope.take(), std::mem::take(&mut st.mounted_nodes))
    };
    if let Some(scope) = old_scope {
        scope.dispose();
    }
    let mut tree_mut = tree.clone();
    for old_id in old_nodes {
        tree_mut.remove_subtree(old_id);
    }
    tree_mut.clear_anchor_mounted(anchor);
    let Some(mount_fn) = next_branch.and_then(|idx| branches_rc.get(idx)) else {
        return;
    };
    let child_scope = Scope::new();
    let mounted = child_scope.run(|| mount_fn(&mut tree_mut, parent));
    tree_mut.place_nodes_before_anchor(parent, anchor, &mounted);
    let mut st = state.borrow_mut();
    st.child_scope = Some(child_scope);
    st.mounted_nodes = mounted;
}

struct KeyedRow<T: 'static, K> {
    key: K,
    scope: Scope,
    item_sig: Signal<T>,
    nodes: Vec<DomNodeId>,
}

struct ForState<T: 'static, K> {
    rows: Vec<KeyedRow<T, K>>,
    empty_scope: Option<Scope>,
    empty_nodes: Vec<DomNodeId>,
}

impl<T: 'static, K> Default for ForState<T, K> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            empty_scope: None,
            empty_nodes: Vec::new(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn reconcile_for_items<T, K>(
    tree: &mut WebDomTree,
    parent: Option<DomNodeId>,
    anchor: DomNodeId,
    state: &Rc<RefCell<ForState<T, K>>>,
    items: Vec<T>,
    next_keys: Vec<K>,
    body_rc: &Rc<
        impl Fn(&mut WebDomTree, Option<DomNodeId>, Signal<T>) -> Vec<DomNodeId> + 'static,
    >,
    empty_fn: &Option<WebBranchMountFn>,
) where
    T: Clone + 'static,
    K: Eq + Clone + 'static,
{
    let (mut old_rows, old_empty_scope, old_empty_nodes) = {
        let mut st = state.borrow_mut();
        (
            std::mem::take(&mut st.rows),
            st.empty_scope.take(),
            std::mem::take(&mut st.empty_nodes),
        )
    };
    if let Some(es) = old_empty_scope {
        es.dispose();
    }
    for eid in old_empty_nodes {
        tree.remove_subtree(eid);
    }

    if items.is_empty() {
        clear_rows_and_mount_empty(tree, parent, anchor, state, old_rows, empty_fn);
        return;
    }

    let mut next_rows = Vec::with_capacity(items.len());
    let mut direct_nodes = Vec::new();
    for (item, key) in items.into_iter().zip(next_keys) {
        let reused_idx = old_rows.iter().position(|r| r.key == key);
        let row = if let Some(idx) = reused_idx {
            let existing = old_rows.swap_remove(idx);
            existing.item_sig.set(item);
            existing
        } else {
            let scope = Scope::new();
            let (item_sig, nodes) = scope.run(|| {
                let sig = Signal::new(item);
                let mounted = body_rc(tree, parent, sig);
                (sig, mounted)
            });
            KeyedRow {
                key,
                scope,
                item_sig,
                nodes,
            }
        };
        direct_nodes.extend_from_slice(&row.nodes);
        next_rows.push(row);
    }
    for leftover in old_rows {
        leftover.scope.dispose();
        for nid in leftover.nodes {
            tree.remove_subtree(nid);
        }
    }
    tree.place_nodes_before_anchor(parent, anchor, &direct_nodes);
    state.borrow_mut().rows = next_rows;
}

fn clear_rows_and_mount_empty<T: 'static, K>(
    tree: &mut WebDomTree,
    parent: Option<DomNodeId>,
    anchor: DomNodeId,
    state: &Rc<RefCell<ForState<T, K>>>,
    old_rows: Vec<KeyedRow<T, K>>,
    empty_fn: &Option<WebBranchMountFn>,
) {
    for row in old_rows {
        row.scope.dispose();
        for nid in row.nodes {
            tree.remove_subtree(nid);
        }
    }
    let mut empty_nodes = Vec::new();
    let mut empty_scope = None;
    if let Some(efn) = empty_fn {
        let child_scope = Scope::new();
        let nodes = child_scope.run(|| efn(tree, parent));
        tree.place_nodes_before_anchor(parent, anchor, &nodes);
        empty_nodes = nodes;
        empty_scope = Some(child_scope);
    } else {
        tree.clear_anchor_mounted(anchor);
    }
    let mut st = state.borrow_mut();
    st.empty_scope = empty_scope;
    st.empty_nodes = empty_nodes;
}
