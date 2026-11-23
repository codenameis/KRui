//! `:has(...)` relative-selector snapshot invalidation adapter.
//!
//! Stylo's `recalc_style_at` invalidates normal descendant/sibling selectors from element
//! snapshots, while relative selectors (`:has()`) require invoking [`RelativeSelectorInvalidator`]
//! on live snapshotted elements prior to `pre_traverse` so parent/ancestor/earlier-sibling anchors
//! receive `RestyleHint::RESTYLE_SELF` and ancestor `dirty_descendants` flags in the same pass.

use selectors::attr::CaseSensitivity;
use selectors::matching::QuirksMode;
use selectors::Element;
use std::marker::PhantomData;
use std::sync::atomic::Ordering;
use style::dom::{OpaqueNode, TElement};
use style::invalidation::element::element_wrapper::{ElementSnapshot, ElementWrapper};
use style::invalidation::element::invalidator::{InvalidationResult, SiblingTraversalMap};
use style::invalidation::element::relative_selector::{
    RelativeSelectorDependencyCollector, RelativeSelectorInvalidator,
};
use style::selector_parser::SnapshotMap;
use style::stylist::{CascadeData, Stylist};
use style::Atom;
use stylo_atoms::Atom as WeakAtom;

use crate::dom_node::{StyloElement, TraverseNode};

/// Runs Stylo relative-selector invalidation for every live element with an unhandled snapshot.
pub(crate) fn invalidate_relative_selectors_for_snapshots<'a>(
    stylist: &'a Stylist,
    arena: &'a [TraverseNode<'a>],
    snapshots: &SnapshotMap,
) {
    if snapshots.is_empty() {
        return;
    }

    for OpaqueNode(idx) in snapshots.keys() {
        let Some(node) = arena.get(*idx) else {
            continue;
        };
        if !node.slot.occupied || !node.slot.is_element {
            continue;
        }
        let element = StyloElement(node);
        if !element.has_data()
            || !element.has_snapshot()
            || element.handled_snapshot()
            || element.relative_selector_search_direction().is_empty()
        {
            continue;
        }

        let invalidator = RelativeSelectorInvalidator {
            element,
            quirks_mode: QuirksMode::NoQuirks,
            snapshot_table: Some(snapshots),
            invalidated: mark_relative_anchor_dirty,
            sibling_traversal_map: SiblingTraversalMap::default(),
            _marker: PhantomData,
        };
        invalidator.invalidate_relative_selectors_for_this(
            stylist,
            |el, scope, data, quirks_mode, collector| {
                collect_snapshot_relative_deps(*el, scope, data, quirks_mode, snapshots, collector);
            },
        );
    }
}

fn mark_relative_anchor_dirty<'a>(anchor: StyloElement<'a>, result: &InvalidationResult) {
    if !result.has_invalidated_self()
        && !result.has_invalidated_descendants()
        && !result.has_invalidated_siblings()
    {
        return;
    }

    if result.has_invalidated_descendants() {
        anchor
            .slot()
            .dirty_descendants
            .store(true, Ordering::Relaxed);
    }

    let mut cursor = anchor.traversal_parent();
    while let Some(parent) = cursor {
        parent
            .slot()
            .dirty_descendants
            .store(true, Ordering::Relaxed);
        cursor = parent.traversal_parent();
    }
}

fn collect_snapshot_relative_deps<'a>(
    element: StyloElement<'a>,
    scope: Option<selectors::OpaqueElement>,
    data: &'a CascadeData,
    quirks_mode: QuirksMode,
    snapshots: &SnapshotMap,
    collector: &mut RelativeSelectorDependencyCollector<'a, StyloElement<'a>>,
) {
    let wrapper = ElementWrapper::new(element, snapshots);
    let Some(snapshot) = wrapper.snapshot() else {
        return;
    };

    let state_changes = wrapper.state_changes();
    let (classes_added, classes_removed) = diff_classes(&element, snapshot);
    let (id_added, id_removed) = diff_ids(&element, snapshot);

    let map = data.relative_selector_invalidation_map();
    if let Some(id) = id_removed {
        if let Some(deps) = map.id_to_selector.get(id, quirks_mode) {
            for dep in deps {
                collector.add_dependency(dep, element, scope);
            }
        }
    }
    if let Some(id) = id_added {
        if let Some(deps) = map.id_to_selector.get(id, quirks_mode) {
            for dep in deps {
                collector.add_dependency(dep, element, scope);
            }
        }
    }

    for class in classes_added.iter().chain(classes_removed.iter()) {
        if let Some(deps) = map.class_to_selector.get(class, quirks_mode) {
            for dep in deps {
                collector.add_dependency(dep, element, scope);
            }
        }
    }

    snapshot.each_attr_changed(|attribute| {
        if let Some(deps) = map.other_attribute_affecting_selectors.get(attribute) {
            for dep in deps {
                collector.add_dependency(dep, element, scope);
            }
        }
    });

    if !state_changes.is_empty() {
        map.state_affecting_selectors.lookup_with_additional(
            element,
            quirks_mode,
            id_removed,
            &classes_removed,
            state_changes,
            |dependency| {
                if dependency.state.intersects(state_changes) {
                    collector.add_dependency(&dependency.dep, element, scope);
                }
                true
            },
        );
    }
}

fn diff_classes(
    element: &StyloElement<'_>,
    snapshot: &style::servo::selector_parser::ServoElementSnapshot,
) -> (Vec<Atom>, Vec<Atom>) {
    if !snapshot.class_changed() {
        return (Vec::new(), Vec::new());
    }
    let mut removed = Vec::new();
    let mut added = Vec::new();
    snapshot.each_class(|cls| {
        if !element.has_class(cls, CaseSensitivity::CaseSensitive) {
            removed.push(cls.0.clone());
        }
    });
    element.each_class(|cls| {
        if !snapshot.has_class(cls, CaseSensitivity::CaseSensitive) {
            added.push(cls.0.clone());
        }
    });
    (added, removed)
}

fn diff_ids<'a>(
    element: &'a StyloElement<'_>,
    snapshot: &'a style::servo::selector_parser::ServoElementSnapshot,
) -> (Option<&'a WeakAtom>, Option<&'a WeakAtom>) {
    if !snapshot.id_changed() {
        return (None, None);
    }
    let old_id = snapshot.id_attr();
    let cur_id = element.id();
    if old_id == cur_id {
        (None, None)
    } else {
        (cur_id, old_id)
    }
}
