//! Retained reactive DOM tree (`WebDomTree`), view fragments (`WebViewFragment`), and lifecycle scopes.

use crate::backend::{DomBackend, DomNodeId};
use crate::mock_backend::MockDomBackend;
use crate::slots::{SharedSlotRegistry, SlotHostRegistry};
use krui_signals::{ReactiveSession, Scope};
use krui_style::ColorSchemePreference;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// Reference-counted branch mounter closure used by `<if>` and `<for><empty>` constructs.
pub type WebBranchMountFn =
    std::rc::Rc<dyn Fn(&mut WebDomTree, Option<DomNodeId>) -> Vec<DomNodeId>>;

type FragmentBuilder = Box<dyn FnOnce(&mut WebDomTree, Option<DomNodeId>) -> Vec<DomNodeId>>;

/// Compiled UI fragment produced by `krui!` / `include_krui!` for `WebDomTree`.
pub struct WebViewFragment {
    builder: FragmentBuilder,
}

impl WebViewFragment {
    /// Creates a [`WebViewFragment`] wrapping a macro-emitted node builder closure.
    pub fn new(
        f: impl FnOnce(&mut WebDomTree, Option<DomNodeId>) -> Vec<DomNodeId> + 'static,
    ) -> Self {
        Self {
            builder: Box::new(f),
        }
    }

    /// Mounts this fragment into `tree` under `parent`, returning top-level DOM node ids.
    pub fn mount(self, tree: &mut WebDomTree, parent: Option<DomNodeId>) -> Vec<DomNodeId> {
        (self.builder)(tree, parent)
    }
}

/// Legacy prototype mount handle kept for backwards API compatibility.
pub struct WebMount {
    root_scope: Scope,
    mutations: Vec<crate::backend::DomMutation>,
}

impl Default for WebMount {
    fn default() -> Self {
        Self::new()
    }
}

impl WebMount {
    /// Creates an empty prototype mount handle.
    pub fn new() -> Self {
        Self {
            root_scope: Scope::new(),
            mutations: Vec::new(),
        }
    }

    /// Mounts `fragment` into a fresh mock-backed `WebDomTree` and records its mutations.
    pub fn mount(&mut self, fragment: WebViewFragment) -> Vec<DomNodeId> {
        let mut tree = WebDomTree::with_backend(Box::new(MockDomBackend::new()));
        let ids = self.root_scope.run(|| fragment.mount(&mut tree, None));
        self.mutations.extend(tree.drain_mutations());
        ids
    }

    /// Returns recorded mutations from legacy [`WebMount::mount`] calls.
    pub fn mutations(&self) -> &[crate::backend::DomMutation] {
        &self.mutations
    }

    /// Disposes reactive effects attached to this mount.
    pub fn dispose(&mut self) {
        self.root_scope.dispose();
    }
}

#[derive(Default)]
pub(crate) struct NodeMeta {
    pub(crate) parent: Option<DomNodeId>,
    pub(crate) children: Vec<DomNodeId>,
    pub(crate) anchor_mounted: Vec<DomNodeId>,
    pub(crate) anchor_cleanup: Option<Rc<dyn Fn()>>,
    pub(crate) is_value_control: bool,
    pub(crate) html_id: Option<String>,
}

pub(crate) struct WebDomInner {
    pub(crate) backend: Box<dyn DomBackend>,
    pub(crate) session: ReactiveSession,
    pub(crate) nodes: HashMap<DomNodeId, NodeMeta>,
    pub(crate) roots: Vec<DomNodeId>,
    pub(crate) root_scope: Scope,
    pub(crate) scale_factor: f32,
    pub(crate) ua_injected: bool,
    pub(crate) embedded_fonts: HashMap<String, Arc<[u8]>>,
    pub(crate) injected_sheets: Vec<String>,
    pub(crate) slots: SharedSlotRegistry,
}

/// Retained fine-grained reactive DOM tree matching `krui-core::ViewTree`'s public surface.
#[derive(Clone)]
pub struct WebDomTree {
    pub(crate) inner: Rc<RefCell<WebDomInner>>,
}

impl Default for WebDomTree {
    fn default() -> Self {
        Self::new()
    }
}

impl WebDomTree {
    /// Creates a `WebDomTree` using [`crate::BrowserDomBackend`] on DOM targets or [`MockDomBackend`].
    pub fn new() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(browser) = crate::browser_backend::BrowserDomBackend::try_from_document() {
                return Self::with_backend(Box::new(browser));
            }
        }
        Self::with_backend(Box::new(MockDomBackend::new()))
    }

    /// Creates a `WebDomTree` backed by an explicit [`DomBackend`] (useful for unit tests).
    pub fn with_backend(backend: Box<dyn DomBackend>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(WebDomInner {
                backend,
                session: ReactiveSession::current(),
                nodes: HashMap::new(),
                roots: Vec::new(),
                root_scope: Scope::new(),
                scale_factor: 1.0,
                ua_injected: false,
                embedded_fonts: HashMap::new(),
                injected_sheets: Vec::new(),
                slots: Rc::new(RefCell::new(SlotHostRegistry::default())),
            })),
        }
    }

    /// Runs `f` with shared read access to the underlying [`DomBackend`].
    pub fn with_backend_ref<R>(&self, f: impl FnOnce(&dyn DomBackend) -> R) -> R {
        let guard = self.inner.borrow();
        f(guard.backend.as_ref())
    }

    /// Runs `f` with exclusive access to the underlying [`DomBackend`].
    pub fn with_backend_mut<R>(&mut self, f: impl FnOnce(&mut dyn DomBackend) -> R) -> R {
        let mut guard = self.inner.borrow_mut();
        f(guard.backend.as_mut())
    }

    /// Returns the live browser `web_sys::Element` for `id`, if mounted under [`crate::BrowserDomBackend`].
    #[cfg(target_arch = "wasm32")]
    pub fn browser_element(&self, id: DomNodeId) -> Option<web_sys::Element> {
        self.with_backend_ref(|b| {
            b.as_any()
                .and_then(|a| a.downcast_ref::<crate::browser_backend::BrowserDomBackend>())
                .and_then(|br| br.element_by_id(id))
        })
    }

    /// Returns the first top-level root node id, if mounted.
    pub fn root(&self) -> Option<DomNodeId> {
        self.inner.borrow().roots.first().copied()
    }

    /// Mounts `fragment` under the tree's root scope.
    pub fn mount(&mut self, fragment: WebViewFragment) -> Vec<DomNodeId> {
        let scope = self.inner.borrow().root_scope;
        let mut tree = self.clone();
        scope.run(|| fragment.mount(&mut tree, None))
    }

    /// Evaluates `build` and mounts the resulting `WebViewFragment` inside `root_scope`.
    pub fn mount_with(&mut self, build: impl FnOnce() -> WebViewFragment) -> Vec<DomNodeId> {
        let scope = self.inner.borrow().root_scope;
        let fragment = scope.run(build);
        let mut tree = self.clone();
        scope.run(|| fragment.mount(&mut tree, None))
    }

    /// Clears mounted nodes, resets `root_scope`, and mounts `fragment`.
    pub fn replace_root(&mut self, fragment: WebViewFragment) -> Vec<DomNodeId> {
        self.replace_root_with(|| fragment)
    }

    /// Clears mounted nodes, resets `root_scope`, and mounts the fragment produced by `build`.
    pub fn replace_root_with(&mut self, build: impl FnOnce() -> WebViewFragment) -> Vec<DomNodeId> {
        let old_roots = {
            let mut guard = self.inner.borrow_mut();
            guard.root_scope.dispose();
            guard.root_scope = Scope::new();
            std::mem::take(&mut guard.roots)
        };
        for root_id in old_roots {
            self.remove_subtree(root_id);
        }
        self.mount_with(build)
    }

    /// Updates root `:root[data-krui-scheme]` preference (`System`, `Light`, `Dark`).
    pub fn set_color_scheme_preference(&mut self, pref: ColorSchemePreference) {
        self.inner
            .borrow_mut()
            .backend
            .set_color_scheme_preference(pref);
    }

    /// Pauses or resumes all CSS animations and transitions cleanly.
    pub fn set_animations_paused(&mut self, paused: bool) {
        self.inner
            .borrow_mut()
            .backend
            .set_animations_paused(paused);
    }

    /// Records the viewport HiDPI scale factor and triggers any bound `<canvas>` redraws.
    pub fn set_scale_factor(&mut self, scale_factor: f32) {
        if scale_factor.is_finite() && scale_factor > 0.0 {
            self.inner.borrow_mut().scale_factor = scale_factor;
        }
    }

    /// Registers an imperative slot host (`on_mount`, `on_unmount`) for elements with `id == html_id`.
    pub fn register_slot_host(
        &mut self,
        html_id: &str,
        on_mount: impl Fn(DomNodeId) + 'static,
        on_unmount: impl Fn(DomNodeId) + 'static,
    ) {
        let slots = Rc::clone(&self.inner.borrow().slots);
        slots.borrow_mut().register(html_id, on_mount, on_unmount);
        let existing: Vec<DomNodeId> = self
            .inner
            .borrow()
            .nodes
            .iter()
            .filter_map(|(&nid, meta)| (meta.html_id.as_deref() == Some(html_id)).then_some(nid))
            .collect();
        for nid in existing {
            crate::slots::notify_slot_mounted(&slots, nid, html_id);
        }
    }

    /// Begins a mount batch (no-op in native DOM since browsers coalesce layout frames automatically).
    pub fn begin_mount_batch(&mut self) {}

    /// Ends a mount batch.
    pub fn end_mount_batch(&mut self) {}

    /// Registers embedded font bytes under `krui://inline/<key>` for `@font-face` Blob resolution.
    pub fn register_embedded_font(&mut self, key: &str, bytes: Arc<[u8]>) {
        let mut guard = self.inner.borrow_mut();
        guard.backend.register_embedded_font(key, &bytes);
        guard.embedded_fonts.insert(key.to_string(), bytes);
    }

    /// Requests immediate redraw of `<canvas>` node `id`.
    pub fn request_canvas_redraw(&self, id: DomNodeId) {
        self.inner.borrow_mut().backend.request_canvas_redraw(id);
    }

    /// Drains recorded mutations when backed by [`MockDomBackend`].
    pub fn drain_mutations(&mut self) -> Vec<crate::backend::DomMutation> {
        self.inner.borrow_mut().backend.drain_mutations()
    }

    pub(crate) fn record_created_node(
        &mut self,
        id: DomNodeId,
        parent: Option<DomNodeId>,
        _tag: &str,
        is_value_control: bool,
        html_id: Option<&str>,
    ) {
        let mut guard = self.inner.borrow_mut();
        guard.nodes.insert(
            id,
            NodeMeta {
                parent,
                children: Vec::new(),
                anchor_mounted: Vec::new(),
                anchor_cleanup: None,
                is_value_control,
                html_id: html_id.map(str::to_string),
            },
        );
        match parent {
            Some(pid) => {
                if let Some(p) = guard.nodes.get_mut(&pid) {
                    p.children.push(id);
                }
            }
            None => guard.roots.push(id),
        }
    }

    pub(crate) fn set_anchor_cleanup(&mut self, anchor: DomNodeId, cleanup: impl Fn() + 'static) {
        if let Some(meta) = self.inner.borrow_mut().nodes.get_mut(&anchor) {
            meta.anchor_cleanup = Some(Rc::new(cleanup));
        }
    }

    pub(crate) fn clear_anchor_mounted(&mut self, anchor: DomNodeId) {
        if let Some(meta) = self.inner.borrow_mut().nodes.get_mut(&anchor) {
            meta.anchor_mounted.clear();
        }
    }

    pub(crate) fn place_nodes_before_anchor(
        &mut self,
        parent: Option<DomNodeId>,
        anchor: DomNodeId,
        direct_nodes: &[DomNodeId],
    ) {
        let flat = {
            let mut guard = self.inner.borrow_mut();
            if let Some(meta) = guard.nodes.get_mut(&anchor) {
                meta.anchor_mounted = direct_nodes.to_vec();
            }
            let mut out = Vec::with_capacity(direct_nodes.len());
            for &nid in direct_nodes {
                expand_anchor_nodes(&guard.nodes, nid, &mut out);
            }
            out
        };
        for fid in flat {
            self.insert_node_in_tree(parent, fid, Some(anchor));
        }
    }

    pub(crate) fn insert_node_in_tree(
        &mut self,
        parent: Option<DomNodeId>,
        child: DomNodeId,
        anchor: Option<DomNodeId>,
    ) {
        let mut guard = self.inner.borrow_mut();
        match anchor {
            Some(anc) => guard.backend.insert_before(parent, child, anc),
            None => guard.backend.append_child(parent, child),
        }
        guard.nodes.entry(child).or_default().parent = parent;
        if let Some(pid) = parent {
            if let Some(p) = guard.nodes.get_mut(&pid) {
                p.children.retain(|&c| c != child);
                if let Some(pos) = anchor.and_then(|a| p.children.iter().position(|&c| c == a)) {
                    p.children.insert(pos, child);
                } else {
                    p.children.push(child);
                }
            }
        } else {
            guard.roots.retain(|&r| r != child);
            if let Some(pos) = anchor.and_then(|a| guard.roots.iter().position(|&r| r == a)) {
                guard.roots.insert(pos, child);
            } else {
                guard.roots.push(child);
            }
        }
    }

    pub(crate) fn remove_subtree(&mut self, root_id: DomNodeId) {
        let (subtree, parent, slots) = {
            let guard = self.inner.borrow();
            let mut order = Vec::new();
            collect_subtree_ids(&guard.nodes, root_id, &mut order);
            let parent = guard.nodes.get(&root_id).and_then(|m| m.parent);
            (order, parent, Rc::clone(&guard.slots))
        };
        for &nid in subtree.iter().rev() {
            crate::slots::notify_slot_unmounting(&slots, nid);
        }
        let mut cleanups_to_run = Vec::new();
        {
            let mut guard = self.inner.borrow_mut();
            match parent {
                Some(pid) => {
                    if let Some(p) = guard.nodes.get_mut(&pid) {
                        p.children.retain(|&c| c != root_id);
                    }
                }
                None => guard.roots.retain(|&r| r != root_id),
            }
            for &nid in &subtree {
                if let Some(meta) = guard.nodes.remove(&nid) {
                    if let Some(cleanup) = meta.anchor_cleanup {
                        cleanups_to_run.push(cleanup);
                    }
                }
            }
            for &nid in subtree.iter().rev() {
                guard.backend.remove_node(nid);
            }
        }
        for cleanup in cleanups_to_run {
            cleanup();
        }
    }
}

fn expand_anchor_nodes(
    nodes: &HashMap<DomNodeId, NodeMeta>,
    id: DomNodeId,
    out: &mut Vec<DomNodeId>,
) {
    if let Some(meta) = nodes.get(&id) {
        for &mounted_child in &meta.anchor_mounted {
            expand_anchor_nodes(nodes, mounted_child, out);
        }
    }
    out.push(id);
}

fn collect_subtree_ids(
    nodes: &HashMap<DomNodeId, NodeMeta>,
    id: DomNodeId,
    out: &mut Vec<DomNodeId>,
) {
    out.push(id);
    if let Some(meta) = nodes.get(&id) {
        for &mounted in &meta.anchor_mounted {
            collect_subtree_ids(nodes, mounted, out);
        }
        for &child in &meta.children {
            collect_subtree_ids(nodes, child, out);
        }
    }
}
