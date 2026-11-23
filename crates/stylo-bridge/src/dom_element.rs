//! Stylo element trait implementations (`selectors::Element` and `TElement`) over [`StyloElement`].
//!
//! # Safety Invariant
//! - All Stylo traversals run strictly single-threaded (`traverse_dom(..., None)`).
//! - This module contains **0 `unsafe {}` blocks** and **0 raw pointers**; only the
//!   5 `unsafe fn` signatures mandated by `style::dom::TElement` carry `#[allow(unsafe_code)]`.

use app_units::Au;
use euclid::default::Size2D;
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::BloomFilter;
use selectors::matching::{ElementSelectorFlags, MatchingContext, VisitedHandlingMode};
use selectors::sink::Push;
use selectors::{Element as SelectorsElement, OpaqueElement};
use std::sync::atomic::Ordering;
use style::applicable_declarations::ApplicableDeclarationBlock;
use style::context::SharedStyleContext;
use style::data::{ElementData, ElementDataMut, ElementDataRef};
use style::dom::{DomChildren, LayoutIterator, NodeInfo, TElement, TNode};
use style::properties::PropertyDeclarationBlock;
use style::selector_parser::{AttrValue, Lang, PseudoElement, SelectorImpl};
use style::servo_arc::Arc;
use style::shared_lock::Locked;
use style::values::computed::Display;
use style::values::AtomIdent;
use style::{LocalName, Namespace};
use stylo_atoms::Atom as WeakAtom;
use stylo_dom::ElementState;

use crate::dom_node::{StyloElement, StyloNode, StyloShadowRoot};
use crate::ElementNamespace;

impl<'a> SelectorsElement for StyloElement<'a> {
    type Impl = SelectorImpl;

    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(self.slot())
    }
    fn parent_element(&self) -> Option<Self> {
        StyloNode(self.0).parent_node().and_then(|n| n.as_element())
    }
    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }
    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }
    fn is_pseudo_element(&self) -> bool {
        false
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        let mut prev = StyloNode(self.0).prev_sibling();
        while let Some(n) = prev {
            if let Some(el) = n.as_element() {
                return Some(el);
            }
            prev = n.prev_sibling();
        }
        None
    }
    fn next_sibling_element(&self) -> Option<Self> {
        let mut next = StyloNode(self.0).next_sibling();
        while let Some(n) = next {
            if let Some(el) = n.as_element() {
                return Some(el);
            }
            next = n.next_sibling();
        }
        None
    }
    fn first_element_child(&self) -> Option<Self> {
        StyloNode(self.0)
            .dom_children()
            .find_map(|n| n.as_element())
    }
    fn is_html_element_in_html_document(&self) -> bool {
        self.slot().ns_kind == ElementNamespace::Html
    }
    fn has_local_name(&self, local_name: &web_atoms::LocalName) -> bool {
        &self.slot().local_name.0 == local_name
    }
    fn has_namespace(&self, ns: &web_atoms::Namespace) -> bool {
        &self.slot().namespace.0 == ns
    }
    fn is_same_type(&self, other: &Self) -> bool {
        self.slot().local_name == other.slot().local_name
            && self.slot().namespace == other.slot().namespace
    }
    fn attr_matches(
        &self,
        ns: &NamespaceConstraint<&Namespace>,
        local_name: &LocalName,
        operation: &AttrSelectorOperation<&style::values::AtomString>,
    ) -> bool {
        let html_ns = match *ns {
            NamespaceConstraint::Any => true,
            NamespaceConstraint::Specific(u) => u.0 == web_atoms::ns!(),
        };
        if !html_ns {
            return false;
        }
        if *local_name == LocalName::from("id") {
            return self
                .slot()
                .id
                .as_ref()
                .is_some_and(|id| operation.eval_str(id.0.as_ref()));
        }
        if *local_name == LocalName::from("class") {
            if self.slot().classes.is_empty() {
                return false;
            }
            let joined = self
                .slot()
                .classes
                .iter()
                .map(|c| c.0.as_ref())
                .collect::<Vec<_>>()
                .join(" ");
            return operation.eval_str(&joined);
        }
        self.slot()
            .attrs
            .iter()
            .find(|(k, _)| k == local_name)
            .is_some_and(|(_, val)| operation.eval_str(val))
    }
    fn match_non_ts_pseudo_class(
        &self,
        pc: &style::selector_parser::NonTSPseudoClass,
        _context: &mut MatchingContext<Self::Impl>,
    ) -> bool {
        let flag = pc.state_flag();
        !flag.is_empty() && self.slot().state.contains(flag)
    }
    fn match_pseudo_element(
        &self,
        pe: &PseudoElement,
        _context: &mut MatchingContext<Self::Impl>,
    ) -> bool {
        matches!(
            pe,
            PseudoElement::Before
                | PseudoElement::After
                | PseudoElement::Selection
                | PseudoElement::Placeholder
                | PseudoElement::Backdrop
        )
    }
    fn apply_selector_flags(&self, flags: ElementSelectorFlags) {
        self.slot().add_selector_flags(flags);
    }
    fn is_link(&self) -> bool {
        self.has_local_name(&web_atoms::local_name!("a"))
            && self
                .slot()
                .attrs
                .iter()
                .any(|(k, _)| *k == LocalName::from("href"))
    }
    fn is_html_slot_element(&self) -> bool {
        self.has_local_name(&web_atoms::local_name!("slot"))
    }
    fn has_id(&self, id: &AtomIdent, case_sensitivity: CaseSensitivity) -> bool {
        self.slot()
            .id
            .as_ref()
            .is_some_and(|own| case_sensitivity.eq(own.0.as_bytes(), id.0.as_bytes()))
    }
    fn has_class(&self, name: &AtomIdent, case_sensitivity: CaseSensitivity) -> bool {
        self.slot()
            .classes
            .iter()
            .any(|cls| case_sensitivity.eq(cls.0.as_bytes(), name.0.as_bytes()))
    }
    fn has_custom_state(&self, _name: &AtomIdent) -> bool {
        false
    }
    fn imported_part(&self, _name: &AtomIdent) -> Option<AtomIdent> {
        None
    }
    fn is_part(&self, _name: &AtomIdent) -> bool {
        false
    }
    fn is_empty(&self) -> bool {
        !StyloNode(self.0)
            .dom_children()
            .any(|c| c.is_element() || (c.is_text_node() && c.0.slot.text_non_empty))
    }
    fn is_root(&self) -> bool {
        StyloNode(self.0)
            .parent_node()
            .is_some_and(|p| p.0.idx == 0)
    }
    fn add_element_unique_hashes(&self, filter: &mut BloomFilter) -> bool {
        style::bloom::each_relevant_element_hash(*self, |hash| {
            filter.insert_hash(hash & selectors::bloom::BLOOM_HASH_MASK);
        });
        true
    }
}

impl<'a> TElement for StyloElement<'a> {
    type ConcreteNode = StyloNode<'a>;
    type TraversalChildrenIterator = DomChildren<StyloNode<'a>>;

    fn as_node(&self) -> StyloNode<'a> {
        StyloNode(self.0)
    }
    fn traversal_children(&self) -> LayoutIterator<Self::TraversalChildrenIterator> {
        LayoutIterator(StyloNode(self.0).dom_children())
    }
    fn is_html_element(&self) -> bool {
        self.slot().ns_kind == ElementNamespace::Html
    }
    fn is_mathml_element(&self) -> bool {
        false
    }
    fn is_svg_element(&self) -> bool {
        self.slot().ns_kind == ElementNamespace::Svg
            || self.has_local_name(&web_atoms::local_name!("svg"))
    }
    fn style_attribute(
        &self,
    ) -> Option<style::servo_arc::ArcBorrow<'_, Locked<PropertyDeclarationBlock>>> {
        self.slot()
            .inline_style
            .as_ref()
            .map(|arc| arc.borrow_arc())
    }
    fn animation_rule(
        &self,
        _: &SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }
    fn transition_rule(
        &self,
        _: &SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }
    fn state(&self) -> ElementState {
        self.slot().state
    }
    fn has_part_attr(&self) -> bool {
        false
    }
    fn exports_any_part(&self) -> bool {
        false
    }
    fn id(&self) -> Option<&WeakAtom> {
        self.slot().id.as_ref().map(|id| &id.0)
    }
    fn each_class<F>(&self, mut callback: F)
    where
        F: FnMut(&AtomIdent),
    {
        for cls in &self.slot().classes {
            callback(cls);
        }
    }
    fn each_custom_state<F>(&self, _callback: F)
    where
        F: FnMut(&AtomIdent),
    {
    }
    fn each_attr_name<F>(&self, mut callback: F)
    where
        F: FnMut(&LocalName),
    {
        if self.slot().id.is_some() {
            callback(&LocalName::from("id"));
        }
        if !self.slot().classes.is_empty() {
            callback(&LocalName::from("class"));
        }
        for (name, _) in &self.slot().attrs {
            callback(name);
        }
    }
    fn has_dirty_descendants(&self) -> bool {
        self.slot().dirty_descendants.load(Ordering::Relaxed)
    }
    fn has_snapshot(&self) -> bool {
        self.slot().has_snapshot.load(Ordering::Relaxed)
    }
    fn handled_snapshot(&self) -> bool {
        self.slot().handled_snapshot.load(Ordering::Relaxed)
    }
    #[allow(unsafe_code)]
    /// # Safety
    /// Stylo traversal invokes this strictly sequentially in single-threaded mode (`pool: None`).
    unsafe fn set_handled_snapshot(&self) {
        // SAFETY: traversal is single-threaded; atomic store has no aliasing.
        self.slot().handled_snapshot.store(true, Ordering::Relaxed);
    }
    #[allow(unsafe_code)]
    /// # Safety
    /// Stylo traversal invokes this strictly sequentially in single-threaded mode (`pool: None`).
    unsafe fn set_dirty_descendants(&self) {
        // SAFETY: traversal is single-threaded; atomic store has no aliasing.
        self.slot().dirty_descendants.store(true, Ordering::Relaxed);
    }
    #[allow(unsafe_code)]
    /// # Safety
    /// Stylo traversal invokes this strictly sequentially in single-threaded mode (`pool: None`).
    unsafe fn unset_dirty_descendants(&self) {
        // SAFETY: traversal is single-threaded; atomic store has no aliasing.
        self.slot()
            .dirty_descendants
            .store(false, Ordering::Relaxed);
    }
    fn store_children_to_process(&self, n: isize) {
        self.slot().children_to_process.store(n, Ordering::Relaxed);
    }
    fn did_process_child(&self) -> isize {
        self.slot()
            .children_to_process
            .fetch_sub(1, Ordering::Relaxed)
            - 1
    }
    #[allow(unsafe_code)]
    /// # Safety
    /// Stylo invokes `ensure_data` only on the active traversal element in single-threaded mode.
    unsafe fn ensure_data(&self) -> ElementDataMut<'_> {
        // SAFETY: delegates to Stylo's `ElementDataWrapper` atomic-flag + debug RefCell guard.
        self.ensure_data_mut()
    }
    #[allow(unsafe_code)]
    /// # Safety
    /// Stylo invokes `clear_data` only during single-threaded subtree teardown (`display: none`).
    unsafe fn clear_data(&self) {
        // SAFETY: single-threaded teardown flags pending hide relayout before resetting `ElementData`.
        let slot = self.slot();
        if slot.ever_styled.swap(false, Ordering::Relaxed) {
            slot.pending_hide_relayout.store(true, Ordering::Relaxed);
            if let Ok(mut list) = self.0.pending_hide.lock() {
                list.push(crate::StyloNodeId::new(self.0.idx, slot.generation));
            }
        }
        slot.has_style_data.store(false, Ordering::Relaxed);
        *slot.style_data.borrow_mut() = ElementData::default();
    }
    fn has_data(&self) -> bool {
        self.slot().has_style_data.load(Ordering::Relaxed)
    }
    fn borrow_data(&self) -> Option<ElementDataRef<'_>> {
        self.has_data().then(|| self.slot().style_data.borrow())
    }
    fn mutate_data(&self) -> Option<ElementDataMut<'_>> {
        self.has_data().then(|| self.slot().style_data.borrow_mut())
    }
    fn skip_item_display_fixup(&self) -> bool {
        false
    }
    fn may_have_animations(&self) -> bool {
        false
    }
    fn has_animations(&self, _: &SharedStyleContext) -> bool {
        false
    }
    fn has_css_animations(&self, _: &SharedStyleContext, _: Option<PseudoElement>) -> bool {
        false
    }
    fn has_css_transitions(&self, _: &SharedStyleContext, _: Option<PseudoElement>) -> bool {
        false
    }
    fn shadow_root(&self) -> Option<StyloShadowRoot<'a>> {
        None
    }
    fn containing_shadow(&self) -> Option<StyloShadowRoot<'a>> {
        None
    }
    fn lang_attr(&self) -> Option<AttrValue> {
        self.slot()
            .attrs
            .iter()
            .find(|(k, _)| *k == LocalName::from("lang"))
            .map(|(_, v)| AttrValue::from(&**v))
    }
    fn match_element_lang(&self, _: Option<Option<AttrValue>>, _: &Lang) -> bool {
        false
    }
    fn is_html_document_body_element(&self) -> bool {
        self.has_local_name(&web_atoms::local_name!("body"))
    }
    fn synthesize_presentational_hints_for_legacy_attributes<V>(
        &self,
        _: VisitedHandlingMode,
        _: &mut V,
    ) where
        V: Push<ApplicableDeclarationBlock>,
    {
    }
    fn local_name(&self) -> &web_atoms::LocalName {
        &self.slot().local_name.0
    }
    fn namespace(&self) -> &web_atoms::Namespace {
        &self.slot().namespace.0
    }
    fn query_container_size(&self, _display: &Display) -> Size2D<Option<Au>> {
        let (w, h) = self.slot().container_size;
        Size2D::new(w, h)
    }
    fn has_selector_flags(&self, flags: ElementSelectorFlags) -> bool {
        self.slot().has_selector_flags(flags)
    }
    fn relative_selector_search_direction(&self) -> ElementSelectorFlags {
        let bits = self.slot().selector_flags.load(Ordering::Relaxed);
        ElementSelectorFlags::from_bits_truncate(bits)
            & ElementSelectorFlags::RELATIVE_SELECTOR_SEARCH_DIRECTION_ANCESTOR_SIBLING
    }
    fn get_attr(&self, attr: &LocalName, _namespace: &Namespace) -> Option<String> {
        if *attr == LocalName::from("id") {
            return self.slot().id.as_ref().map(|id| id.0.to_string());
        }
        if *attr == LocalName::from("class") {
            if self.slot().classes.is_empty() {
                return None;
            }
            return Some(
                self.slot()
                    .classes
                    .iter()
                    .map(|c| c.0.as_ref())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
        self.slot()
            .attrs
            .iter()
            .find(|(k, _)| k == attr)
            .map(|(_, v)| v.to_string())
    }
}
