//! Stylo document, shadow root, node, and traversal driver implementations over pointer-sized
//! [`StyloNode`] and [`StyloElement`] handles (`size_of == size_of::<usize>()`, required by Stylo's
//! `StyleSharingCache`). Zero `unsafe {}` blocks and zero raw pointers.

use selectors::matching::QuirksMode;
use std::cell::Cell;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use style::context::{CascadeInputs, SharedStyleContext, StyleContext};
use style::data::ElementDataMut;
use style::dom::{NodeInfo, OpaqueNode, TDocument, TElement, TNode, TShadowRoot};
use style::invalidation::element::restyle_hints::RestyleHint;
use style::matching::StyleChange;
use style::properties::ComputedValues;
use style::rule_tree::{RuleCascadeFlags, StrongRuleNode};
use style::selector_parser::PseudoElement;
use style::servo::restyle_damage::ServoRestyleDamage;
use style::shared_lock::SharedRwLock;
use style::style_resolver::{PrimaryStyle, PseudoElementResolution, StyleResolverForElement};
use style::stylist::{CascadeData, RuleInclusion};
use style::traversal::{recalc_style_at, DomTraversal};

use crate::dom_store::{StyloDom, StyloSlot};
use crate::{StyloDamage, StyloNodeId};

/// Compact pointer-sized traversal view built from [`StyloDom`] for a single `restyle()` pass.
pub(crate) struct TraverseNode<'a> {
    pub(crate) idx: u32,
    pub(crate) slot: &'a StyloSlot,
    pub(crate) shared_lock: &'a SharedRwLock,
    pub(crate) pending_hide: &'a Mutex<Vec<StyloNodeId>>,
    pub(crate) parent: Cell<Option<&'a TraverseNode<'a>>>,
    pub(crate) first_child: Cell<Option<&'a TraverseNode<'a>>>,
    pub(crate) last_child: Cell<Option<&'a TraverseNode<'a>>>,
    pub(crate) prev_sibling: Cell<Option<&'a TraverseNode<'a>>>,
    pub(crate) next_sibling: Cell<Option<&'a TraverseNode<'a>>>,
}

impl fmt::Debug for TraverseNode<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TraverseNode({})", self.idx)
    }
}

pub(crate) fn build_traverse_arena<'a>(dom: &'a StyloDom) -> Vec<TraverseNode<'a>> {
    let nodes: Vec<TraverseNode<'a>> = dom
        .slots
        .iter()
        .enumerate()
        .map(|(i, slot)| TraverseNode {
            idx: i as u32,
            slot,
            shared_lock: &dom.shared_lock,
            pending_hide: &dom.pending_hide_nodes,
            parent: Cell::new(None),
            first_child: Cell::new(None),
            last_child: Cell::new(None),
            prev_sibling: Cell::new(None),
            next_sibling: Cell::new(None),
        })
        .collect();
    nodes
}

pub(crate) fn link_traverse_arena<'a>(nodes: &'a [TraverseNode<'a>], dom: &StyloDom) {
    let get = |opt: Option<u32>| opt.and_then(|i| nodes.get(i as usize));
    for (node, slot) in nodes.iter().zip(dom.slots.iter()) {
        node.parent.set(get(slot.parent));
        node.first_child.set(get(slot.first_child));
        node.last_child.set(get(slot.last_child));
        node.prev_sibling.set(get(slot.prev_sibling));
        node.next_sibling.set(get(slot.next_sibling));
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct StyloDocument<'a>(pub(crate) &'a TraverseNode<'a>);

#[derive(Clone, Copy, Debug)]
pub(crate) struct StyloNode<'a>(pub(crate) &'a TraverseNode<'a>);

#[derive(Clone, Copy, Debug)]
pub(crate) struct StyloElement<'a>(pub(crate) &'a TraverseNode<'a>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StyloShadowRoot<'a>(pub(crate) StyloNode<'a>);

impl PartialEq for StyloNode<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0.idx == other.0.idx
    }
}
impl Eq for StyloNode<'_> {}

impl PartialEq for StyloElement<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0.idx == other.0.idx
    }
}
impl Eq for StyloElement<'_> {}

impl Hash for StyloElement<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.idx.hash(state);
    }
}

impl<'a> StyloElement<'a> {
    #[inline]
    pub(crate) fn slot(&self) -> &'a StyloSlot {
        self.0.slot
    }

    pub(crate) fn ensure_data_mut(&self) -> ElementDataMut<'_> {
        let slot = self.slot();
        slot.has_style_data.store(true, Ordering::Relaxed);
        slot.style_data.borrow_mut()
    }
}

impl NodeInfo for StyloNode<'_> {
    fn is_element(&self) -> bool {
        self.0.slot.is_element
    }
    fn is_text_node(&self) -> bool {
        self.0.slot.is_text
    }
}

impl<'a> TDocument for StyloDocument<'a> {
    type ConcreteNode = StyloNode<'a>;

    fn as_node(&self) -> StyloNode<'a> {
        StyloNode(self.0)
    }
    fn is_html_document(&self) -> bool {
        true
    }
    fn quirks_mode(&self) -> QuirksMode {
        QuirksMode::NoQuirks
    }
    fn shared_lock(&self) -> &SharedRwLock {
        self.0.shared_lock
    }
}

impl<'a> TShadowRoot for StyloShadowRoot<'a> {
    type ConcreteNode = StyloNode<'a>;

    fn as_node(&self) -> StyloNode<'a> {
        self.0
    }
    fn host(&self) -> StyloElement<'a> {
        self.0
            .traversal_parent()
            .expect("shadow root must have a host")
    }
    fn style_data<'b>(&self) -> Option<&'b CascadeData>
    where
        Self: 'b,
    {
        None
    }
}

impl<'a> TNode for StyloNode<'a> {
    type ConcreteElement = StyloElement<'a>;
    type ConcreteDocument = StyloDocument<'a>;
    type ConcreteShadowRoot = StyloShadowRoot<'a>;

    fn parent_node(&self) -> Option<Self> {
        self.0.parent.get().map(StyloNode)
    }
    fn first_child(&self) -> Option<Self> {
        self.0.first_child.get().map(StyloNode)
    }
    fn last_child(&self) -> Option<Self> {
        self.0.last_child.get().map(StyloNode)
    }
    fn prev_sibling(&self) -> Option<Self> {
        self.0.prev_sibling.get().map(StyloNode)
    }
    fn next_sibling(&self) -> Option<Self> {
        self.0.next_sibling.get().map(StyloNode)
    }
    fn owner_doc(&self) -> StyloDocument<'a> {
        // Walk up to slot 0 document if needed, or return root-anchored doc handle.
        let mut cur = self.0;
        while let Some(p) = cur.parent.get() {
            cur = p;
        }
        StyloDocument(cur)
    }
    fn is_in_document(&self) -> bool {
        self.0.idx == 0 || self.0.slot.parent.is_some()
    }
    fn traversal_parent(&self) -> Option<StyloElement<'a>> {
        self.parent_node().and_then(|p| p.as_element())
    }
    fn opaque(&self) -> OpaqueNode {
        OpaqueNode(self.0.idx as usize)
    }
    fn debug_id(self) -> usize {
        self.0.idx as usize
    }
    fn as_element(&self) -> Option<StyloElement<'a>> {
        self.0.slot.is_element.then_some(StyloElement(self.0))
    }
    fn as_document(&self) -> Option<StyloDocument<'a>> {
        (self.0.idx == 0).then_some(StyloDocument(self.0))
    }
    fn as_shadow_root(&self) -> Option<StyloShadowRoot<'a>> {
        None
    }
}

pub(crate) struct RecalcStyleOnly<'a> {
    pub(crate) shared: SharedStyleContext<'a>,
    pub(crate) recorded_damage: Mutex<Vec<(StyloNodeId, StyloDamage)>>,
    pub(crate) traversed_indices: Mutex<Vec<u32>>,
}

fn build_clean_cascade_inputs(
    context: &StyleContext<StyloElement<'_>>,
    rules: &StrongRuleNode,
    style: &ComputedValues,
) -> Option<CascadeInputs> {
    if !rules.has_animation_or_transition_rules() {
        return None;
    }
    let clean = context
        .shared
        .stylist
        .rule_tree()
        .remove_animation_rules(rules);
    Some(CascadeInputs {
        rules: Some(clean),
        visited_rules: style.visited_rules().cloned(),
        flags: style.flags.for_cascade_inputs(),
        included_cascade_flags: RuleCascadeFlags::empty(),
    })
}

fn propagate_inherited_recascade_to_children<'a>(
    element: StyloElement<'a>,
    queued_children: &mut Vec<StyloNode<'a>>,
) {
    element
        .slot()
        .dirty_descendants
        .store(true, Ordering::Relaxed);
    for child_node in element.as_node().dom_children() {
        let Some(child) = child_node.as_element() else {
            continue;
        };
        if let Some(mut cdata) = child.mutate_data() {
            if cdata.has_styles() {
                cdata.hint.insert(RestyleHint::RECASCADE_SELF);
            }
        }
        if !queued_children.contains(&child_node) {
            queued_children.push(child_node);
        }
    }
}

/// Strips Stylo's Servo-side `CascadeOrigin::Transitions`/`Animations` overrides so KRui's
/// 120fps compositor receives the true post-cascade target style on every restyle pass.
fn strip_servo_animation_rules<'a>(
    element: StyloElement<'a>,
    context: &mut StyleContext<StyloElement<'a>>,
    data: &mut style::data::ElementData,
    prev_cv: Option<&ComputedValues>,
    queued_children: &mut Vec<StyloNode<'a>>,
) {
    let primary_inputs = data
        .styles
        .primary
        .as_ref()
        .and_then(|s| build_clean_cascade_inputs(context, s.rules(), s));
    if let Some(inputs) = primary_inputs {
        let clean = StyleResolverForElement::new(
            element,
            context,
            RuleInclusion::All,
            PseudoElementResolution::IfApplicable,
        )
        .cascade_style_and_visited_with_default_parents(inputs);
        data.styles.primary = Some(clean.0);
        if let Some(prev) = prev_cv {
            let diff = ServoRestyleDamage::compute_style_difference::<StyloElement<'a>>(
                prev,
                data.styles.primary(),
            );
            data.damage |= diff.damage;
            if matches!(
                diff.change,
                StyleChange::Changed {
                    reset_only: false,
                    ..
                }
            ) {
                propagate_inherited_recascade_to_children(element, queued_children);
            }
        } else {
            data.damage |= ServoRestyleDamage::REPAINT;
        }
    }

    if !data.has_styles() {
        return;
    }
    let primary_share: PrimaryStyle = data.share_primary_style();
    for pseudo in [PseudoElement::Before, PseudoElement::After] {
        let p_inputs = data
            .styles
            .pseudos
            .get(&pseudo)
            .and_then(|s| build_clean_cascade_inputs(context, s.rules(), s));
        if let Some(inputs) = p_inputs {
            let clean = StyleResolverForElement::new(
                element,
                context,
                RuleInclusion::All,
                PseudoElementResolution::IfApplicable,
            )
            .cascade_style_and_visited_for_pseudo_with_default_parents(
                inputs,
                &pseudo,
                &primary_share,
            );
            data.styles.pseudos.set(&pseudo, clean.0);
            data.damage |= ServoRestyleDamage::REPAINT;
        }
    }
}

impl<'a> DomTraversal<StyloElement<'a>> for RecalcStyleOnly<'a> {
    fn process_preorder<F>(
        &self,
        context: &mut StyleContext<StyloElement<'a>>,
        node: StyloNode<'a>,
        mut note_child: F,
    ) where
        F: FnMut(StyloNode<'a>),
    {
        if let Some(element) = node.as_element() {
            let mut queued_children = Vec::new();
            let relayout_opt = {
                let mut data = element.ensure_data_mut();
                let prev_cv = data.styles.get_primary().cloned();
                let prev_before = data
                    .styles
                    .pseudos
                    .get(&style::selector_parser::PseudoElement::Before)
                    .cloned();
                let prev_after = data
                    .styles
                    .pseudos
                    .get(&style::selector_parser::PseudoElement::After)
                    .cloned();
                recalc_style_at(self, context, element, &mut data, |child| {
                    queued_children.push(child);
                });
                strip_servo_animation_rules(
                    element,
                    context,
                    &mut data,
                    prev_cv.as_deref(),
                    &mut queued_children,
                );
                let next_cv = data.styles.get_primary().cloned();
                if data.has_styles() {
                    if let Ok(mut list) = self.traversed_indices.lock() {
                        list.push(element.0.idx);
                    }
                }
                evaluate_element_damage(
                    element,
                    &mut data,
                    prev_cv.as_deref(),
                    prev_before.as_deref(),
                    prev_after.as_deref(),
                    next_cv.as_deref(),
                )
            };
            if let Some(dmg) = relayout_opt {
                let id = StyloNodeId::new(element.0.idx, element.slot().generation);
                self.recorded_damage
                    .lock()
                    .expect("damage log lock")
                    .push((id, dmg));
            }
            for child_node in queued_children {
                note_child(child_node);
            }
        }
    }

    fn process_postorder(
        &self,
        _context: &mut StyleContext<StyloElement<'a>>,
        _node: StyloNode<'a>,
    ) {
    }

    fn needs_postorder_traversal() -> bool {
        false
    }

    fn shared_context(&self) -> &SharedStyleContext<'a> {
        &self.shared
    }
}

fn evaluate_element_damage(
    element: StyloElement<'_>,
    data: &mut style::data::ElementData,
    prev_cv: Option<&style::properties::ComputedValues>,
    prev_before: Option<&style::properties::ComputedValues>,
    prev_after: Option<&style::properties::ComputedValues>,
    next_cv: Option<&style::properties::ComputedValues>,
) -> Option<StyloDamage> {
    use style::selector_parser::PseudoElement;
    let slot = element.slot();
    let next_before = data
        .styles
        .pseudos
        .get(&PseudoElement::Before)
        .map(|c| &**c);
    let next_after = data.styles.pseudos.get(&PseudoElement::After).map(|c| &**c);
    let first_styled = data.has_styles() && !slot.ever_styled.swap(true, Ordering::Relaxed);
    let pending_hide = slot.pending_hide_relayout.swap(false, Ordering::Relaxed);
    let custom_props_changed =
        prev_cv.map(|c| c.custom_properties()) != next_cv.map(|c| c.custom_properties());
    let pseudo_changed = prev_before.map(std::ptr::from_ref) != next_before.map(std::ptr::from_ref)
        || prev_after.map(std::ptr::from_ref) != next_after.map(std::ptr::from_ref);
    let pseudo_relayout = pseudo_changed
        && (crate::computed_to_paint::needs_taffy_relayout(prev_before, next_before, false)
            || crate::computed_to_paint::needs_taffy_relayout(prev_after, next_after, false));
    let has_element_children = element.as_node().dom_children().any(|c| c.is_element());
    let taffy_dirty = (!data.damage.is_empty() || custom_props_changed)
        && crate::computed_to_paint::needs_taffy_relayout(prev_cv, next_cv, has_element_children);
    let relayout = first_styled || pending_hide || taffy_dirty || pseudo_relayout;
    let has_damage = !data.damage.is_empty() || custom_props_changed || pseudo_changed;
    if !(relayout || has_damage) {
        return None;
    }
    data.damage = ServoRestyleDamage::empty();
    Some(if relayout {
        StyloDamage::Relayout
    } else {
        StyloDamage::Repaint
    })
}
