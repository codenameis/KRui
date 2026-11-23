//! DOM mutations, structural invalidation, snapshot capture, and state mapping on [`StyloDom`].

use app_units::Au;
use selectors::matching::QuirksMode;
use std::sync::atomic::Ordering;
use style::error_reporting::ParseErrorReporter;
use style::invalidation::element::restyle_hints::RestyleHint;
use style::properties::parse_style_attribute;
use style::servo::attr::{AttrIdentifier, AttrValue};
use style::servo::selector_parser::SnapshotMap;
use style::servo_arc::Arc;
use style::stylesheets::{CssRuleType, UrlExtraData};
use style::values::AtomIdent;
use style::{LocalName, Namespace};
use stylo_dom::ElementState;

use crate::dom_store::{StyloDom, StyloSlot, HTML_NS_URI, SVG_NS_URI};
use crate::{DomElementState, ElementAttr, ElementNamespace, StyloDomMutation, StyloNodeId};

impl StyloDom {
    pub(crate) fn set_root_element(&mut self, id: StyloNodeId) {
        let Some(new_root) = self.resolve_live(id) else {
            return;
        };
        if !self.slots[new_root as usize].is_element {
            return;
        }
        if let Some(prev) = self.root_element.filter(|&p| p != new_root) {
            self.detach_child(prev);
        }
        self.detach_child(new_root);
        self.link_before(0, new_root, None);
        self.root_element = Some(new_root);
        self.restyle_subtree_slot(new_root);
        self.mark_ancestors_dirty(new_root);
    }

    pub(crate) fn apply_mutation(
        &mut self,
        mutation: StyloDomMutation,
        snapshots: &mut SnapshotMap,
        base_url: &UrlExtraData,
        reporter: Option<&dyn ParseErrorReporter>,
    ) {
        match mutation {
            StyloDomMutation::AppendChild { parent, child } => {
                self.insert_child(parent, child, None)
            }
            StyloDomMutation::InsertBefore {
                parent,
                child,
                before,
            } => {
                self.insert_child(parent, child, before);
            }
            StyloDomMutation::DetachNode(node) => {
                if let Some(idx) = self.resolve_live(node) {
                    self.detach_and_invalidate(idx);
                }
            }
            StyloDomMutation::DisposeSubtree(node) => {
                if let Some(idx) = self.resolve_live(node) {
                    self.detach_and_invalidate(idx);
                    self.dispose_recursive(idx);
                }
            }
            StyloDomMutation::SetTag {
                node,
                tag_name,
                namespace,
            } => {
                self.set_tag(node, &tag_name, namespace, snapshots);
            }
            StyloDomMutation::SetClasses { node, classes } => {
                self.set_classes(node, classes, snapshots);
            }
            StyloDomMutation::SetId { node, id } => self.set_id(node, id, snapshots),
            StyloDomMutation::SetAttributes { node, attrs } => {
                self.set_attributes(node, attrs, snapshots);
            }
            StyloDomMutation::SetInlineStyle { node, css } => {
                self.set_inline_style(node, &css, base_url, reporter);
            }
            StyloDomMutation::SetState { node, state } => self.set_state(node, state, snapshots),
            StyloDomMutation::SetTextNonEmpty { node, non_empty } => {
                self.set_text_non_empty(node, non_empty);
            }
        }
    }

    pub(crate) fn set_container_size(
        &mut self,
        node: StyloNodeId,
        width_px: Option<f32>,
        height_px: Option<f32>,
    ) {
        let Some(idx) = self.resolve_live(node) else {
            return;
        };
        let next = (
            width_px.map(Au::from_f32_px),
            height_px.map(Au::from_f32_px),
        );
        if self.slots[idx as usize].container_size == next {
            return;
        }
        self.slots[idx as usize].container_size = next;
        self.restyle_subtree_slot(idx);
        self.mark_ancestors_dirty(idx);
    }

    fn insert_child(
        &mut self,
        parent: StyloNodeId,
        child: StyloNodeId,
        before: Option<StyloNodeId>,
    ) {
        let (Some(p), Some(c)) = (self.resolve_live(parent), self.resolve_live(child)) else {
            return;
        };
        if p == c || !self.slots[p as usize].is_element || self.is_ancestor(c, p) {
            return;
        }
        let before_idx = before
            .and_then(|b| self.resolve_live(b))
            .filter(|&b| b != c && self.slots[b as usize].parent == Some(p));
        let old_parent = self.slots[c as usize].parent;
        self.detach_child(c);
        if self.root_element == Some(c) {
            self.root_element = None;
        }
        self.link_before(p, c, before_idx);
        if let Some(op) = old_parent.filter(|&op| op > 0) {
            self.restyle_subtree_slot(op);
            self.invalidate_relative_ancestors(Some(op));
        }
        self.restyle_subtree_slot(c);
        self.restyle_subtree_slot(p);
        self.mark_ancestors_dirty(c);
        self.invalidate_relative_ancestors(Some(p));
    }

    fn is_ancestor(&self, candidate_ancestor: u32, mut cursor: u32) -> bool {
        while let Some(parent) = self.slots.get(cursor as usize).and_then(|s| s.parent) {
            if parent == candidate_ancestor {
                return true;
            }
            cursor = parent;
        }
        false
    }

    fn detach_and_invalidate(&mut self, idx: u32) {
        let old_parent = self.slots[idx as usize].parent;
        if self.root_element == Some(idx) {
            self.root_element = None;
        }
        if self.slots[idx as usize]
            .ever_styled
            .swap(false, Ordering::Relaxed)
        {
            let gen = self.slots[idx as usize].generation;
            self.slots[idx as usize]
                .pending_hide_relayout
                .store(true, Ordering::Relaxed);
            self.record_pending_hide(StyloNodeId::new(idx, gen));
        }
        self.detach_child(idx);
        if let Some(parent) = old_parent.filter(|&p| p > 0) {
            self.restyle_subtree_slot(parent);
            self.mark_ancestors_dirty(parent);
            self.invalidate_relative_ancestors(Some(parent));
        }
    }

    fn dispose_recursive(&mut self, idx: u32) {
        let mut child = self.slots[idx as usize].first_child;
        while let Some(c) = child {
            child = self.slots[c as usize].next_sibling;
            self.dispose_recursive(c);
        }
        self.slots[idx as usize].reset_free();
        self.free_list.push(idx);
    }

    fn set_tag(
        &mut self,
        node: StyloNodeId,
        tag_name: &str,
        namespace: ElementNamespace,
        snapshots: &mut SnapshotMap,
    ) {
        let Some(idx) = self.resolve_live(node) else {
            return;
        };
        self.capture_snapshot(idx, false, false, true, &[], snapshots);
        let slot = &mut self.slots[idx as usize];
        slot.local_name = LocalName::from(tag_name.to_ascii_lowercase().as_str());
        slot.ns_kind = namespace;
        slot.namespace = Namespace::from(match namespace {
            ElementNamespace::Html => HTML_NS_URI,
            ElementNamespace::Svg => SVG_NS_URI,
        });
        self.restyle_subtree_slot(idx);
        self.mark_ancestors_dirty(idx);
    }

    fn set_classes(
        &mut self,
        node: StyloNodeId,
        classes: Vec<String>,
        snapshots: &mut SnapshotMap,
    ) {
        let Some(idx) = self.resolve_live(node) else {
            return;
        };
        let class_attr = [LocalName::from("class")];
        self.capture_snapshot(idx, true, false, false, &class_attr, snapshots);
        let mut normalized = Vec::new();
        for item in classes {
            for token in item.split_whitespace() {
                let atom = AtomIdent::from(token);
                if !normalized.contains(&atom) {
                    normalized.push(atom);
                }
            }
        }
        self.slots[idx as usize].classes = normalized;
        self.mark_ancestors_dirty(idx);
        self.invalidate_nth_of_siblings(idx);
    }

    fn set_id(&mut self, node: StyloNodeId, id: Option<String>, snapshots: &mut SnapshotMap) {
        let Some(idx) = self.resolve_live(node) else {
            return;
        };
        let id_attr = [LocalName::from("id")];
        self.capture_snapshot(idx, false, true, false, &id_attr, snapshots);
        self.slots[idx as usize].id = id.map(|v| AtomIdent::from(v.trim()));
        self.mark_ancestors_dirty(idx);
        self.invalidate_nth_of_siblings(idx);
    }

    fn set_attributes(
        &mut self,
        node: StyloNodeId,
        attrs: Vec<ElementAttr>,
        snapshots: &mut SnapshotMap,
    ) {
        let Some(idx) = self.resolve_live(node) else {
            return;
        };
        let mut changed = Vec::new();
        for attr in &attrs {
            let name = LocalName::from(attr.name.to_ascii_lowercase().as_str());
            if !changed.contains(&name) {
                changed.push(name);
            }
        }
        for (existing, _) in &self.slots[idx as usize].attrs {
            if !changed.contains(existing) {
                changed.push(existing.clone());
            }
        }
        let class_changed = changed.iter().any(|n| *n == LocalName::from("class"));
        let id_changed = changed.iter().any(|n| *n == LocalName::from("id"));
        self.capture_snapshot(idx, class_changed, id_changed, true, &changed, snapshots);
        self.slots[idx as usize].attrs = apply_attr_list(&mut self.slots[idx as usize], attrs);
        self.mark_ancestors_dirty(idx);
        self.invalidate_nth_of_siblings(idx);
    }

    fn set_inline_style(
        &mut self,
        node: StyloNodeId,
        css: &str,
        base_url: &UrlExtraData,
        reporter: Option<&dyn ParseErrorReporter>,
    ) {
        let Some(idx) = self.resolve_live(node) else {
            return;
        };
        let rewritten = crate::servo_ext::rewrite_servo_extensions(css);
        self.slots[idx as usize].inline_style = (!rewritten.trim().is_empty()).then(|| {
            let parsed = parse_style_attribute(
                rewritten.as_ref(),
                base_url,
                reporter,
                QuirksMode::NoQuirks,
                CssRuleType::Style,
            );
            Arc::new(self.shared_lock.wrap(parsed))
        });
        let slot = &self.slots[idx as usize];
        if slot.has_style_data.load(Ordering::Relaxed) {
            slot.style_data
                .borrow_mut()
                .hint
                .insert(RestyleHint::RESTYLE_STYLE_ATTRIBUTE | RestyleHint::RESTYLE_SELF);
        }
        self.mark_ancestors_dirty(idx);
    }

    fn set_state(
        &mut self,
        node: StyloNodeId,
        state: DomElementState,
        snapshots: &mut SnapshotMap,
    ) {
        let Some(idx) = self.resolve_live(node) else {
            return;
        };
        self.capture_snapshot(idx, false, false, false, &[], snapshots);
        self.slots[idx as usize].state = map_dom_state(state);
        self.mark_ancestors_dirty(idx);
        self.invalidate_nth_of_siblings(idx);
    }

    fn set_text_non_empty(&mut self, node: StyloNodeId, non_empty: bool) {
        let Some(idx) = self.resolve_live(node) else {
            return;
        };
        if self.slots[idx as usize].text_non_empty == non_empty {
            return;
        }
        self.slots[idx as usize].text_non_empty = non_empty;
        if let Some(parent) = self.slots[idx as usize].parent.filter(|&p| p > 0) {
            self.restyle_subtree_slot(parent);
            self.mark_ancestors_dirty(parent);
        }
    }

    fn capture_snapshot(
        &mut self,
        node: u32,
        class_changed: bool,
        id_changed: bool,
        other_changed: bool,
        changed_attr_names: &[LocalName],
        snapshots: &mut SnapshotMap,
    ) {
        let slot = &self.slots[node as usize];
        if !slot.has_style_data.load(Ordering::Relaxed) {
            return;
        }
        let opaque = style::dom::OpaqueNode(node as usize);
        let snap = snapshots.entry(opaque).or_default();
        if snap.state.is_none() {
            snap.state = Some(slot.state);
        }
        if snap.attrs.is_none() {
            snap.attrs = Some(snapshot_slot_attrs(slot));
        }
        snap.class_changed |= class_changed;
        snap.id_changed |= id_changed;
        snap.other_attributes_changed |= other_changed;
        for attr in changed_attr_names {
            if !snap.changed_attrs.contains(attr) {
                snap.changed_attrs.push(attr.clone());
            }
        }
        slot.has_snapshot.store(true, Ordering::Relaxed);
        slot.handled_snapshot.store(false, Ordering::Relaxed);
    }

    fn detach_child(&mut self, child: u32) {
        let (parent, prev, next) = (
            self.slots[child as usize].parent.take(),
            self.slots[child as usize].prev_sibling.take(),
            self.slots[child as usize].next_sibling.take(),
        );
        if let Some(p) = prev {
            self.slots[p as usize].next_sibling = next;
        }
        if let Some(n) = next {
            self.slots[n as usize].prev_sibling = prev;
        }
        if let Some(parent_idx) = parent {
            if self.slots[parent_idx as usize].first_child == Some(child) {
                self.slots[parent_idx as usize].first_child = next;
            }
            if self.slots[parent_idx as usize].last_child == Some(child) {
                self.slots[parent_idx as usize].last_child = prev;
            }
        }
    }

    fn link_before(&mut self, parent: u32, child: u32, before: Option<u32>) {
        if let Some(b) = before {
            let prev = self.slots[b as usize].prev_sibling;
            self.slots[b as usize].prev_sibling = Some(child);
            self.slots[child as usize].next_sibling = Some(b);
            self.slots[child as usize].prev_sibling = prev;
            self.slots[child as usize].parent = Some(parent);
            if let Some(p) = prev {
                self.slots[p as usize].next_sibling = Some(child);
            } else {
                self.slots[parent as usize].first_child = Some(child);
            }
            return;
        }
        let old_last = self.slots[parent as usize].last_child;
        if let Some(last_idx) = old_last {
            self.slots[last_idx as usize].next_sibling = Some(child);
        }
        self.slots[child as usize].parent = Some(parent);
        self.slots[child as usize].prev_sibling = old_last;
        self.slots[child as usize].next_sibling = None;
        if self.slots[parent as usize].first_child.is_none() {
            self.slots[parent as usize].first_child = Some(child);
        }
        self.slots[parent as usize].last_child = Some(child);
    }
}

fn apply_attr_list(slot: &mut StyloSlot, attrs: Vec<ElementAttr>) -> Vec<(LocalName, AttrValue)> {
    let mut deduped: Vec<(LocalName, AttrValue)> = Vec::new();
    for attr in attrs {
        let lower = attr.name.to_ascii_lowercase();
        if lower == "id" {
            slot.id = Some(AtomIdent::from(attr.value.trim()));
        } else if lower == "class" {
            slot.classes = attr.value.split_whitespace().map(AtomIdent::from).collect();
        } else {
            let key = LocalName::from(lower.as_str());
            if let Some((_, existing)) = deduped.iter_mut().find(|(k, _)| *k == key) {
                *existing = AttrValue::String(attr.value);
            } else {
                deduped.push((key, AttrValue::String(attr.value)));
            }
        }
    }
    deduped
}

fn attr_ident(local_name: LocalName) -> AttrIdentifier {
    AttrIdentifier {
        name: local_name.clone(),
        local_name,
        namespace: Namespace::from(""),
        prefix: None,
    }
}

fn snapshot_slot_attrs(slot: &StyloSlot) -> Vec<(AttrIdentifier, AttrValue)> {
    let mut out = Vec::with_capacity(slot.attrs.len() + 2);
    if let Some(id) = &slot.id {
        out.push((
            attr_ident(LocalName::from("id")),
            AttrValue::Atom(id.0.clone()),
        ));
    }
    if !slot.classes.is_empty() {
        let joined = slot
            .classes
            .iter()
            .map(|c| c.0.as_ref())
            .collect::<Vec<_>>()
            .join(" ");
        out.push((
            attr_ident(LocalName::from("class")),
            AttrValue::from_serialized_tokenlist(joined),
        ));
    }
    for (name, val) in &slot.attrs {
        out.push((attr_ident(name.clone()), val.clone()));
    }
    out
}

fn map_dom_state(state: DomElementState) -> ElementState {
    const TABLE: &[(DomElementState, ElementState)] = &[
        (DomElementState::HOVER, ElementState::HOVER),
        (DomElementState::FOCUS, ElementState::FOCUS),
        (DomElementState::FOCUS_VISIBLE, ElementState::FOCUSRING),
        (DomElementState::FOCUS_WITHIN, ElementState::FOCUS_WITHIN),
        (DomElementState::ACTIVE, ElementState::ACTIVE),
        (DomElementState::DISABLED, ElementState::DISABLED),
        (DomElementState::ENABLED, ElementState::ENABLED),
        (DomElementState::CHECKED, ElementState::CHECKED),
        (DomElementState::INDETERMINATE, ElementState::INDETERMINATE),
        (
            DomElementState::PLACEHOLDER_SHOWN,
            ElementState::PLACEHOLDER_SHOWN,
        ),
        (DomElementState::READ_ONLY, ElementState::READONLY),
        (DomElementState::READ_WRITE, ElementState::READWRITE),
        (DomElementState::REQUIRED, ElementState::REQUIRED),
        (DomElementState::VALID, ElementState::VALID),
        (DomElementState::INVALID, ElementState::INVALID),
        (DomElementState::OPEN, ElementState::OPEN),
        (DomElementState::MODAL, ElementState::MODAL),
    ];
    let mut out = ElementState::empty();
    for (src, dst) in TABLE {
        if state.contains(*src) {
            out.insert(*dst);
        }
    }
    out
}
