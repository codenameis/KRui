//! Generational arena DOM store backing Stylo trait adapters without raw pointers.
//!
//! All live slots reside in `Vec<StyloSlot>`, recycled cleanly via `free_list` with monotonic
//! per-slot generations so stale `StyloNodeId` handles safely return `None` / become no-ops and
//! 100+ mount/dispose cycles maintain constant live slot memory.

use app_units::Au;
use selectors::matching::ElementSelectorFlags;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::sync::Mutex;
use style::data::{ElementData, ElementDataWrapper};
use style::invalidation::element::restyle_hints::RestyleHint;
use style::properties::PropertyDeclarationBlock;
use style::servo::attr::AttrValue;
use style::servo_arc::Arc;
use style::shared_lock::{Locked, SharedRwLock};
use style::values::AtomIdent;
use style::{LocalName, Namespace};
use stylo_dom::ElementState;

use crate::{ElementNamespace, StyloNodeId};

pub(crate) const HTML_NS_URI: &str = "http://www.w3.org/1999/xhtml";
pub(crate) const SVG_NS_URI: &str = "http://www.w3.org/2000/svg";

/// Per-node slot inside [`StyloDom`].
#[derive(Debug)]
pub(crate) struct StyloSlot {
    pub(crate) generation: u32,
    pub(crate) occupied: bool,
    pub(crate) parent: Option<u32>,
    pub(crate) first_child: Option<u32>,
    pub(crate) last_child: Option<u32>,
    pub(crate) prev_sibling: Option<u32>,
    pub(crate) next_sibling: Option<u32>,
    pub(crate) is_element: bool,
    pub(crate) is_text: bool,
    pub(crate) text_non_empty: bool,
    pub(crate) ns_kind: ElementNamespace,
    pub(crate) local_name: LocalName,
    pub(crate) namespace: Namespace,
    pub(crate) id: Option<AtomIdent>,
    pub(crate) classes: Vec<AtomIdent>,
    pub(crate) attrs: Vec<(LocalName, AttrValue)>,
    pub(crate) inline_style: Option<Arc<Locked<PropertyDeclarationBlock>>>,
    pub(crate) state: ElementState,
    pub(crate) container_size: (Option<Au>, Option<Au>),
    pub(crate) dirty_descendants: AtomicBool,
    pub(crate) has_snapshot: AtomicBool,
    pub(crate) handled_snapshot: AtomicBool,
    pub(crate) selector_flags: AtomicUsize,
    pub(crate) children_to_process: AtomicIsize,
    pub(crate) has_style_data: AtomicBool,
    pub(crate) ever_styled: AtomicBool,
    pub(crate) pending_hide_relayout: AtomicBool,
    pub(crate) style_data: ElementDataWrapper,
}

impl StyloSlot {
    fn new_document() -> Self {
        Self::blank(1, false, false, "", false)
    }

    fn blank(
        generation: u32,
        is_element: bool,
        is_text: bool,
        local_name: &str,
        text_non_empty: bool,
    ) -> Self {
        Self {
            generation,
            occupied: true,
            parent: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
            is_element,
            is_text,
            text_non_empty,
            ns_kind: ElementNamespace::Html,
            local_name: LocalName::from(local_name),
            namespace: Namespace::from(HTML_NS_URI),
            id: None,
            classes: Vec::new(),
            attrs: Vec::new(),
            inline_style: None,
            state: ElementState::empty(),
            container_size: (None, None),
            dirty_descendants: AtomicBool::new(false),
            has_snapshot: AtomicBool::new(false),
            handled_snapshot: AtomicBool::new(false),
            selector_flags: AtomicUsize::new(0),
            children_to_process: AtomicIsize::new(0),
            has_style_data: AtomicBool::new(false),
            ever_styled: AtomicBool::new(false),
            pending_hide_relayout: AtomicBool::new(false),
            style_data: ElementDataWrapper::default(),
        }
    }

    pub(crate) fn reset_free(&mut self) {
        self.occupied = false;
        self.generation = self.generation.wrapping_add(1).max(1);
        self.parent = None;
        self.first_child = None;
        self.last_child = None;
        self.prev_sibling = None;
        self.next_sibling = None;
        self.is_element = false;
        self.is_text = false;
        self.text_non_empty = false;
        self.local_name = LocalName::from("");
        self.id = None;
        self.classes.clear();
        self.attrs.clear();
        self.inline_style = None;
        self.state = ElementState::empty();
        self.container_size = (None, None);
        self.dirty_descendants.store(false, Ordering::Relaxed);
        self.has_snapshot.store(false, Ordering::Relaxed);
        self.handled_snapshot.store(false, Ordering::Relaxed);
        self.selector_flags.store(0, Ordering::Relaxed);
        self.children_to_process.store(0, Ordering::Relaxed);
        self.has_style_data.store(false, Ordering::Relaxed);
        self.ever_styled.store(false, Ordering::Relaxed);
        *self.style_data.borrow_mut() = ElementData::default();
    }

    pub(crate) fn add_selector_flags(&self, flags: ElementSelectorFlags) {
        self.selector_flags
            .fetch_or(flags.bits(), Ordering::Relaxed);
    }

    pub(crate) fn has_selector_flags(&self, flags: ElementSelectorFlags) -> bool {
        let bits = self.selector_flags.load(Ordering::Relaxed);
        ElementSelectorFlags::from_bits_truncate(bits).contains(flags)
    }
}

/// Owned DOM arena paired with Stylo's shared lock.
#[derive(Debug)]
pub(crate) struct StyloDom {
    pub(crate) slots: Vec<StyloSlot>,
    pub(crate) free_list: Vec<u32>,
    pub(crate) root_element: Option<u32>,
    pub(crate) shared_lock: SharedRwLock,
    pub(crate) pending_hide_nodes: Mutex<Vec<StyloNodeId>>,
}

impl StyloDom {
    pub(crate) fn new() -> Self {
        Self {
            slots: vec![StyloSlot::new_document()],
            free_list: Vec::new(),
            root_element: None,
            shared_lock: SharedRwLock::new(),
            pending_hide_nodes: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn record_pending_hide(&self, id: StyloNodeId) {
        if let Ok(mut list) = self.pending_hide_nodes.lock() {
            list.push(id);
        }
    }

    pub(crate) fn active_slot_count(&self) -> usize {
        self.slots.len().saturating_sub(1 + self.free_list.len())
    }

    pub(crate) fn resolve_live(&self, id: StyloNodeId) -> Option<u32> {
        let idx = id.index();
        let slot = self.slots.get(idx as usize)?;
        (idx > 0 && slot.occupied && slot.generation == id.generation()).then_some(idx)
    }

    pub(crate) fn alloc_element(&mut self, tag_name: &str) -> StyloNodeId {
        self.alloc_slot(true, false, &tag_name.to_ascii_lowercase(), false)
    }

    pub(crate) fn alloc_text(&mut self, non_empty: bool) -> StyloNodeId {
        self.alloc_slot(false, true, "", non_empty)
    }

    fn alloc_slot(
        &mut self,
        is_element: bool,
        is_text: bool,
        local_name: &str,
        text_non_empty: bool,
    ) -> StyloNodeId {
        if let Some(idx) = self.free_list.pop() {
            let slot = &mut self.slots[idx as usize];
            let gen = slot.generation;
            *slot = StyloSlot::blank(gen, is_element, is_text, local_name, text_non_empty);
            return StyloNodeId::new(idx, gen);
        }
        let idx = u32::try_from(self.slots.len()).expect("StyloDom slot capacity overflow");
        self.slots.push(StyloSlot::blank(
            1,
            is_element,
            is_text,
            local_name,
            text_non_empty,
        ));
        StyloNodeId::new(idx, 1)
    }

    pub(crate) fn mark_all_dirty(&self) {
        for slot in &self.slots {
            if !slot.occupied || !slot.is_element {
                continue;
            }
            slot.dirty_descendants.store(true, Ordering::Relaxed);
            if slot.has_style_data.load(Ordering::Relaxed) {
                slot.style_data
                    .borrow_mut()
                    .hint
                    .insert(RestyleHint::restyle_subtree());
            }
        }
    }

    pub(crate) fn clear_all_snapshots(&self) {
        for slot in &self.slots {
            slot.has_snapshot.store(false, Ordering::Relaxed);
            slot.handled_snapshot.store(false, Ordering::Relaxed);
        }
    }

    pub(crate) fn restyle_subtree_slot(&self, idx: u32) {
        let slot = &self.slots[idx as usize];
        slot.dirty_descendants.store(true, Ordering::Relaxed);
        if slot.has_style_data.load(Ordering::Relaxed) {
            slot.style_data
                .borrow_mut()
                .hint
                .insert(RestyleHint::restyle_subtree());
        }
    }

    pub(crate) fn mark_ancestors_dirty(&self, start: u32) {
        let mut cursor = self.slots.get(start as usize).and_then(|s| s.parent);
        while let Some(pid) = cursor {
            if let Some(pslot) = self.slots.get(pid as usize) {
                pslot.dirty_descendants.store(true, Ordering::Relaxed);
                cursor = pslot.parent;
            } else {
                break;
            }
        }
    }

    pub(crate) fn invalidate_relative_ancestors(&self, mut cursor: Option<u32>) {
        let anchor_mask = ElementSelectorFlags::ANCHORS_RELATIVE_SELECTOR
            | ElementSelectorFlags::ANCHORS_RELATIVE_SELECTOR_NON_SUBJECT;
        while let Some(idx) = cursor {
            let Some(slot) = self.slots.get(idx as usize) else {
                break;
            };
            if idx == 0 {
                break;
            }
            if slot.selector_flags.load(Ordering::Relaxed) & anchor_mask.bits() != 0 {
                self.restyle_subtree_slot(idx);
                self.mark_ancestors_dirty(idx);
            }
            cursor = slot.parent;
        }
    }

    pub(crate) fn invalidate_nth_of_siblings(&self, idx: u32) {
        let Some(parent) = self
            .slots
            .get(idx as usize)
            .and_then(|s| s.parent)
            .filter(|&p| p > 0)
        else {
            return;
        };
        let nth_mask = ElementSelectorFlags::HAS_SLOW_SELECTOR_NTH_OF
            | ElementSelectorFlags::HAS_SLOW_SELECTOR_NTH
            | ElementSelectorFlags::HAS_SLOW_SELECTOR_LATER_SIBLINGS;
        let has_nth = self.slots[parent as usize]
            .selector_flags
            .load(Ordering::Relaxed)
            & nth_mask.bits()
            != 0
            || self.slots[idx as usize]
                .selector_flags
                .load(Ordering::Relaxed)
                & nth_mask.bits()
                != 0
            || {
                let mut cur = self.slots[parent as usize].first_child;
                let mut found = false;
                while let Some(cidx) = cur {
                    if self.slots[cidx as usize]
                        .selector_flags
                        .load(Ordering::Relaxed)
                        & nth_mask.bits()
                        != 0
                    {
                        found = true;
                        break;
                    }
                    cur = self.slots[cidx as usize].next_sibling;
                }
                found
            };
        if has_nth {
            self.restyle_subtree_slot(parent);
            self.mark_ancestors_dirty(parent);
        }
    }
}
