//! Public `ViewTree` stylesheet, media preference, class/inline-style, and Stylo element APIs.

pub(crate) use super::restyle_sync::{
    compute_dom_element_state, flush_pending_restyle, notify_children_mutation_inner,
    notify_selector_state_change, resolve_canvas_clear_color, sync_node_attrs_to_stylo,
    sync_node_text_to_stylo, sync_stylo_children_for_container, sync_viewport_and_flush_restyle,
};

use crate::view_tree::node::{resolve_tag_semantic_role, ElementNode};
use crate::view_tree::sparse_tables::{
    attach_compiled_node_to_parent, init_sparse_for_compiled_node,
};
use crate::view_tree::{A11yId, NodeId, ViewTree, ViewTreeInner};
use krui_signals::Effect;
use krui_style::{ColorScheme, ColorSchemePreference, CompiledStyle, ReducedMotionPreference};
use krui_stylo_bridge::{
    DomElementState, StyleSheetHandle, StyleSheetOrigin, StyloDomMutation, StyloNodeId,
};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

impl ViewTree {
    /// Registers an additional author CSS stylesheet and schedules a Stylo restyle pass.
    pub fn add_author_stylesheet(&mut self, css: &str) -> StyleSheetHandle {
        let mut inner = self.inner.borrow_mut();
        let handle = inner.stylo.add_stylesheet(StyleSheetOrigin::Author, css);
        inner.stylo_dirty = true;
        inner.fonts_keyframes_dirty = true;
        inner.paint_dirty = true;
        handle
    }

    /// Replaces document stylesheets, skipping re-parsing when `sheets` are unchanged.
    pub fn set_document_stylesheets(&mut self, sheets: &[&str]) {
        let next_hash = hash_stylesheets(sheets);
        let mut inner = self.inner.borrow_mut();
        if inner.doc_sheet_hash == Some(next_hash) {
            return;
        }
        let old = std::mem::take(&mut inner.doc_sheet_handles);
        for h in old {
            let _ = inner.stylo.remove_stylesheet(h);
        }
        for s in sheets {
            let h = inner.stylo.add_stylesheet(StyleSheetOrigin::Author, s);
            inner.doc_sheet_handles.push(h);
        }
        inner.doc_sheet_hash = Some(next_hash);
        inner.stylo_dirty = true;
        inner.fonts_keyframes_dirty = true;
        inner.paint_dirty = true;
    }

    /// Replaces all author stylesheets (used during live preview hot-patching) and restyles.
    pub fn replace_author_stylesheets(&mut self, sheets: &[&str]) {
        let next_hash = hash_stylesheets(sheets);
        let mut inner = self.inner.borrow_mut();
        inner.stylo.clear_author_stylesheets();
        inner.doc_sheet_handles.clear();
        for s in sheets {
            let h = inner.stylo.add_stylesheet(StyleSheetOrigin::Author, s);
            inner.doc_sheet_handles.push(h);
        }
        inner.doc_sheet_hash = Some(next_hash);
        inner.sparse.clear_transitions();
        inner.stylo_dirty = true;
        inner.fonts_keyframes_dirty = true;
        inner.paint_dirty = true;
        let (vw, vh) = inner.last_viewport;
        let _ = sync_viewport_and_flush_restyle(&mut inner, vw, vh);
    }

    /// Registers compile-time embedded `@font-face` font bytes under `path_or_key`.
    pub fn register_embedded_font(&mut self, path_or_key: &str, bytes: Arc<[u8]>) {
        let mut inner = self.inner.borrow_mut();
        inner.embedded_fonts.insert(path_or_key.to_string(), bytes);
        inner.fonts_keyframes_dirty = true;
        inner.paint_dirty = true;
    }

    /// Records `unstyled` document mode (HTML5 browser-UA baseline remains enabled in Stylo).
    pub fn set_unstyled(&mut self, _unstyled: bool) {
        let mut inner = self.inner.borrow_mut();
        inner.stylo.set_unstyled(true);
        inner.stylo_dirty = true;
        inner.fonts_keyframes_dirty = true;
        inner.paint_dirty = true;
    }

    /// Sets host color-scheme preference (`System`, `Light`, or `Dark`).
    pub fn set_color_scheme_preference(&mut self, pref: ColorSchemePreference) {
        let mut inner = self.inner.borrow_mut();
        if inner.color_scheme_pref == pref {
            return;
        }
        inner.color_scheme_pref = pref;
        let resolved = resolve_color_scheme(inner.color_scheme_pref, inner.system_color_scheme);
        inner.stylo.set_color_scheme(resolved);
        inner.stylo_dirty = true;
        inner.paint_dirty = true;
    }

    /// Updates OS-reported dark/light color scheme (effective when preference is `System`).
    pub fn set_system_color_scheme(&mut self, scheme: ColorScheme) {
        let mut inner = self.inner.borrow_mut();
        if inner.system_color_scheme == scheme {
            return;
        }
        inner.system_color_scheme = scheme;
        if inner.color_scheme_pref == ColorSchemePreference::System {
            inner.stylo.set_color_scheme(scheme);
            inner.stylo_dirty = true;
            inner.paint_dirty = true;
        }
    }

    /// Returns the currently effective `prefers-color-scheme` value (`Light` or `Dark`).
    pub fn effective_color_scheme(&self) -> ColorScheme {
        let inner = self.inner.borrow();
        resolve_color_scheme(inner.color_scheme_pref, inner.system_color_scheme)
    }

    /// Sets host `prefers-reduced-motion` policy (`System`, `Reduce`, or `NoPreference`).
    pub fn set_reduced_motion_preference(&mut self, pref: ReducedMotionPreference) {
        let mut inner = self.inner.borrow_mut();
        if inner.reduced_motion_pref != pref {
            inner.reduced_motion_pref = pref;
            inner.paint_dirty = true;
        }
    }

    /// Updates OS-reported reduced-motion flag (active when policy is `System`).
    pub fn set_os_reduced_motion(&mut self, reduced: bool) {
        let mut inner = self.inner.borrow_mut();
        if inner.os_reduced_motion != reduced {
            inner.os_reduced_motion = reduced;
            inner.paint_dirty = true;
        }
    }

    /// Returns the resolved `prefers-reduced-motion` flag for animations/transitions.
    pub fn effective_reduced_motion(&self) -> bool {
        self.inner.borrow().effective_reduced_motion()
    }

    /// Replaces CSS class names on `node_id` and notifies Stylo only when classes changed.
    pub fn set_element_classes(&mut self, node_id: NodeId, classes: &[&str]) {
        set_element_classes_inner(&mut self.inner.borrow_mut(), node_id, classes);
    }

    /// Parses and applies an inline `style="..."` declaration string to `node_id`.
    pub fn set_inline_style(&mut self, node_id: NodeId, css: &str) {
        set_inline_style_inner(&mut self.inner.borrow_mut(), node_id, css);
    }

    /// Binds a reactive inline `style="..."` formatter closure to `node_id`.
    pub fn bind_dynamic_inline_style(
        &mut self,
        node_id: NodeId,
        formatter: impl Fn() -> String + 'static,
    ) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let tree_weak = Rc::downgrade(&self.inner);
        let prev = Rc::new(RefCell::new(None::<String>));
        let eff = Effect::new(move || {
            let next = formatter();
            let changed = prev.borrow().as_deref() != Some(next.as_str());
            if !changed {
                return;
            }
            *prev.borrow_mut() = Some(next.clone());
            if let Some(rc) = tree_weak.upgrade() {
                set_inline_style_inner(&mut rc.borrow_mut(), node_id, &next);
            }
        });
        self.inner.borrow_mut().push_effect(node_id, eff);
    }

    /// Returns the count of live Stylo DOM node slots (for screen-switch leak diagnostics).
    pub fn stylo_node_count(&self) -> usize {
        self.inner.borrow().stylo.active_slot_count()
    }

    /// Returns the live resolved [`CompiledStyle`] snapshot for `node_id`.
    pub fn node_computed_style(&self, node_id: NodeId) -> Option<CompiledStyle> {
        let mut inner = self.inner.borrow_mut();
        flush_pending_restyle(&mut inner);
        inner.node(node_id).map(|n| n.style)
    }

    /// Drains accumulated non-fatal CSS parse diagnostics reported during stylesheet loads.
    pub fn take_style_diagnostics(&self) -> Vec<String> {
        self.inner
            .borrow()
            .stylo
            .take_diagnostics()
            .into_iter()
            .map(|d| format!("{}:{}: {}", d.line, d.column, d.message))
            .collect()
    }

    /// Updates viewport dimensions for `@media` / viewport unit evaluation and flushes restyles.
    pub fn sync_viewport_and_media(&mut self, width: f32, height: f32) -> bool {
        sync_viewport_and_flush_restyle(&mut self.inner.borrow_mut(), width, height)
    }

    /// Flushes any pending Stylo cascade/restyle updates immediately at the current viewport.
    pub fn restyle_now(&mut self) {
        let mut inner = self.inner.borrow_mut();
        let (vw, vh) = inner.last_viewport;
        let _ = sync_viewport_and_flush_restyle(&mut inner, vw, vh);
    }

    pub(crate) fn notify_children_selector_mutation(&mut self, parent: Option<NodeId>) {
        notify_children_mutation_inner(&mut self.inner.borrow_mut(), parent);
    }

    /// Creates a Stylo-backed retained DOM element attached under `parent` (or document `body`).
    #[allow(clippy::too_many_arguments)]
    pub fn create_element_node(
        &mut self,
        parent: Option<NodeId>,
        tag: &'static str,
        element_id: Option<String>,
        aria_label: Option<String>,
        is_button: bool,
        is_input: bool,
        is_canvas: bool,
        is_secret: bool,
    ) -> NodeId {
        let role = resolve_tag_semantic_role(tag, is_button, is_input, is_canvas, is_secret);
        let mut inner = self.inner.borrow_mut();
        let stylo_id = inner.stylo.alloc_element(tag);
        let stylo_parent = resolve_stylo_parent(&inner, parent);
        inner.stylo.apply_mutation(StyloDomMutation::AppendChild {
            parent: stylo_parent,
            child: stylo_id,
        });
        if let Some(id_val) = element_id.as_deref() {
            inner.stylo.apply_mutation(StyloDomMutation::SetId {
                node: stylo_id,
                id: Some(id_val.to_string()),
            });
        }
        let taffy_node = inner.layout_engine.create_stylo_node(stylo_id);
        let (slot_idx, generation) = inner.allocate_slot();
        let node_id = NodeId::new(slot_idx, generation);
        if role.is_focusable() {
            inner
                .a11y_engine
                .register_focusable(A11yId::from(node_id).0);
        }
        let mut retained = ElementNode::new_element(
            node_id,
            parent,
            tag,
            element_id,
            aria_label,
            role,
            is_secret,
            taffy_node,
            CompiledStyle::default(),
        );
        retained.stylo_node = Some(stylo_id);
        if (slot_idx as usize) < inner.nodes.len() {
            inner.nodes[slot_idx as usize] = retained;
        } else {
            inner.nodes.push(retained);
        }
        inner.stylo_to_view.insert(stylo_id, node_id);
        init_sparse_for_compiled_node(&mut inner, node_id, role, is_secret, is_canvas, false);
        attach_compiled_node_to_parent(&mut inner, parent, node_id, taffy_node);
        let init_state = compute_dom_element_state(&inner, node_id);
        if init_state != DomElementState::EMPTY {
            if let Some(node) = inner.node_mut(node_id) {
                node.last_stylo_state = init_state;
            }
            inner.stylo.apply_mutation(StyloDomMutation::SetState {
                node: stylo_id,
                state: init_state,
            });
        }
        inner.stylo_dirty = true;
        inner.paint_dirty = true;
        node_id
    }
}

impl ViewTreeInner {
    pub(crate) fn effective_reduced_motion(&self) -> bool {
        match self.reduced_motion_pref {
            ReducedMotionPreference::System => self.os_reduced_motion,
            ReducedMotionPreference::Reduce => true,
            ReducedMotionPreference::NoPreference => false,
        }
    }
}

fn resolve_stylo_parent(inner: &ViewTreeInner, mut cur: Option<NodeId>) -> StyloNodeId {
    while let Some(pid) = cur {
        let Some(pnode) = inner.node(pid) else {
            break;
        };
        if !pnode.is_anchor() {
            if let Some(sid) = pnode.stylo_node {
                return sid;
            }
        }
        cur = pnode.parent;
    }
    inner.stylo_body_root
}

pub(crate) fn set_element_classes_inner(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    classes: &[&str],
) {
    let changed = {
        let entry = inner.sparse.classes.entry(node_id).or_default();
        if entry.len() == classes.len() && entry.iter().zip(classes).all(|(a, b)| a == b) {
            false
        } else {
            *entry = classes.iter().map(|s| (*s).to_string()).collect();
            true
        }
    };
    if !changed {
        return;
    }
    if let Some(stylo_id) = inner.node(node_id).and_then(|n| n.stylo_node) {
        let owned = inner
            .sparse
            .classes
            .get(&node_id)
            .cloned()
            .unwrap_or_default();
        inner.stylo.apply_mutation(StyloDomMutation::SetClasses {
            node: stylo_id,
            classes: owned,
        });
        inner.stylo_dirty = true;
        inner.paint_dirty = true;
    }
}

pub(crate) fn set_inline_style_inner(inner: &mut ViewTreeInner, node_id: NodeId, css: &str) {
    let Some(stylo_id) = inner.node(node_id).and_then(|n| n.stylo_node) else {
        return;
    };
    let trimmed = css.trim();
    if trimmed.is_empty() {
        if let Some(entry) = inner.sparse.pseudo_pointer.get_mut(&node_id) {
            entry.author_inline_style = None;
            crate::view_tree::pointer_vars::sync_effective_inline_style(inner, node_id);
        } else {
            inner
                .stylo
                .apply_mutation(StyloDomMutation::SetInlineStyle {
                    node: stylo_id,
                    css: String::new(),
                });
            inner.stylo_dirty = true;
            inner.paint_dirty = true;
        }
        return;
    }
    inner.ensure_pseudo_pointer(node_id).author_inline_style = Some(css.to_string());
    crate::view_tree::pointer_vars::sync_effective_inline_style(inner, node_id);
}

fn resolve_color_scheme(pref: ColorSchemePreference, sys: ColorScheme) -> ColorScheme {
    match pref {
        ColorSchemePreference::System => sys,
        ColorSchemePreference::Light => ColorScheme::Light,
        ColorSchemePreference::Dark => ColorScheme::Dark,
    }
}

fn hash_stylesheets(sheets: &[&str]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    sheets.len().hash(&mut h);
    for s in sheets {
        s.hash(&mut h);
    }
    h.finish()
}
