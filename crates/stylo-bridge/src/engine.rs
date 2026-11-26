//! Top-level Stylo CSS cascade engine driving stylesheet lifecycle, device metrics,
//! incremental DOM restyling, and encapsulated style/text/paint queries.

use krui_style::ColorScheme;
use selectors::matching::QuirksMode;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use style::animation::DocumentAnimationSet;
use style::context::{SharedStyleContext, StyleSystemOptions};
use style::dom::{TDocument, TNode};
use style::media_queries::MediaList;
use style::selector_parser::SnapshotMap;
use style::servo_arc::Arc;
use style::shared_lock::StylesheetGuards;
use style::stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet, UrlExtraData};
use style::stylist::Stylist;
use style::thread_state::{self, ThreadState};
use style::traversal::DomTraversal;
use style::traversal_flags::TraversalFlags;

use crate::container_queries::{
    clear_container_caches, preprocess_container_rules, sync_container_node,
    RegisteredContainerQuery,
};
use crate::device::{build_device, init_stylo_prefs, DiagnosticCollector, NoopPainters};
use crate::dom_node::{build_traverse_arena, link_traverse_arena, RecalcStyleOnly, StyloDocument};
use crate::dom_store::StyloDom;
use crate::engine_queries::LazyPseudoCache;
use crate::relative_invalidation::invalidate_relative_selectors_for_snapshots;
use crate::{
    CssDiagnostic, DeviceConfig, RestyleReport, StyleSheetHandle, StyleSheetOrigin, StyloDamage,
    StyloDomMutation, StyloNodeId,
};

const UA_SHARED_CSS: &str = include_str!("../ua/ua_shared.css");
const UA_UNSTYLED_CSS: &str = include_str!("../ua/unstyled.css");

pub(crate) struct RegisteredSheet {
    pub(crate) handle: StyleSheetHandle,
    pub(crate) origin: StyleSheetOrigin,
    pub(crate) sheet: DocumentStyleSheet,
}

/// Isolated Stylo CSS cascade engine owning sheets, device metrics, snapshots, and DOM slots.
pub struct StyloStyleEngine {
    pub(crate) dom: StyloDom,
    pub(crate) stylist: Stylist,
    pub(crate) snapshots: SnapshotMap,
    pub(crate) sheets: Vec<RegisteredSheet>,
    pub(crate) lazy_pseudos: HashMap<StyloNodeId, LazyPseudoCache>,
    pub(crate) lazy_pseudos_dirty: bool,
    diagnostics: Mutex<Vec<CssDiagnostic>>,
    next_sheet_id: u64,
    next_cq_id: u32,
    container_queries: Vec<RegisteredContainerQuery>,
    unstyled_sheet: Option<StyleSheetHandle>,
    pub(crate) config: DeviceConfig,
    base_url: UrlExtraData,
}

impl StyloStyleEngine {
    /// Creates a new Stylo CSS engine configured with media state and baseline UA stylesheets.
    #[must_use]
    pub fn new(config: DeviceConfig) -> Self {
        init_stylo_prefs();
        let base_url = UrlExtraData::from(
            url::Url::parse("krui://app/").expect("static krui base url must parse"),
        );
        let device = build_device(&config);
        let stylist = Stylist::new(device, QuirksMode::NoQuirks);
        let mut engine = Self {
            dom: StyloDom::new(),
            stylist,
            snapshots: SnapshotMap::new(),
            sheets: Vec::new(),
            lazy_pseudos: HashMap::new(),
            lazy_pseudos_dirty: true,
            diagnostics: Mutex::new(Vec::new()),
            next_sheet_id: 1,
            next_cq_id: 1,
            container_queries: Vec::new(),
            unstyled_sheet: None,
            config,
            base_url,
        };
        engine.add_stylesheet(StyleSheetOrigin::UserAgent, UA_SHARED_CSS);
        if config.unstyled {
            engine.unstyled_sheet =
                Some(engine.add_stylesheet(StyleSheetOrigin::UserAgent, UA_UNSTYLED_CSS));
        }
        engine
    }

    /// Allocates a new element node inside Stylo's generational DOM arena.
    pub fn alloc_element(&mut self, local_name: &str) -> StyloNodeId {
        self.dom.alloc_element(local_name)
    }

    /// Allocates a new text node inside Stylo's generational DOM arena.
    pub fn alloc_text(&mut self, non_empty: bool) -> StyloNodeId {
        self.dom.alloc_text(non_empty)
    }

    /// Returns the number of currently live DOM slots (excluding deleted/recycled nodes).
    #[must_use]
    pub fn active_slot_count(&self) -> usize {
        self.dom.active_slot_count()
    }

    /// Sets or replaces the document's root element (maintaining a unique `:root` element).
    pub fn set_root_element(&mut self, root: StyloNodeId) {
        self.dom.set_root_element(root);
    }

    /// Applies a DOM, attribute, inline-style, class, or interaction state mutation.
    pub fn apply_mutation(&mut self, mutation: StyloDomMutation) {
        let disposed = matches!(mutation, StyloDomMutation::DisposeSubtree(_));
        let reporter = DiagnosticCollector(&self.diagnostics);
        self.dom.apply_mutation(
            mutation,
            &mut self.snapshots,
            &self.base_url,
            Some(&reporter),
        );
        if disposed {
            self.lazy_pseudos
                .retain(|id, _| self.dom.resolve_live(*id).is_some());
        }
    }

    /// Returns `true` if any registered stylesheet defines `@container` size queries.
    #[must_use]
    pub fn has_container_queries(&self) -> bool {
        !self.container_queries.is_empty()
    }

    /// Updates a node's laid-out container box size (CSS px) for `@container` size queries.
    ///
    /// Returns `true` if the container size or matched query state changed and scheduled descendant restyling.
    pub fn set_container_size(
        &mut self,
        node: StyloNodeId,
        width_px: Option<f32>,
        height_px: Option<f32>,
    ) -> bool {
        let cv = self.computed_values(node);
        sync_container_node(
            &mut self.dom,
            &mut self.snapshots,
            &self.stylist,
            &self.container_queries,
            cv.as_deref(),
            (node, width_px, height_px),
        )
    }

    /// Adds a stylesheet at User-Agent or Author origin and returns its stable handle.
    pub fn add_stylesheet(&mut self, origin: StyleSheetOrigin, css: &str) -> StyleSheetHandle {
        let handle = StyleSheetHandle(self.next_sheet_id);
        self.next_sheet_id += 1;
        let (rewritten, new_cq) = preprocess_container_rules(css, handle.0, &mut self.next_cq_id);
        let sheet = self.parse_stylesheet(origin, &rewritten);
        let guard = self.dom.shared_lock.read();
        self.stylist.append_stylesheet(sheet.clone(), &guard);
        drop(guard);
        self.sheets.push(RegisteredSheet {
            handle,
            origin,
            sheet,
        });
        if !new_cq.is_empty() {
            self.container_queries.extend(new_cq);
            clear_container_caches(&mut self.dom, &mut self.snapshots, &[]);
        }
        self.dom.mark_all_dirty();
        self.lazy_pseudos_dirty = true;
        handle
    }

    /// Replaces an existing stylesheet in place at its original cascade position.
    pub fn replace_stylesheet(&mut self, handle: StyleSheetHandle, css: &str) -> bool {
        let Some(pos) = self.sheets.iter().position(|entry| entry.handle == handle) else {
            return false;
        };
        let origin = self.sheets[pos].origin;
        let removed_ids = self.remove_container_queries_for_sheet(handle.0);
        let (rewritten, new_cq) = preprocess_container_rules(css, handle.0, &mut self.next_cq_id);
        let next_sheet = self.parse_stylesheet(origin, &rewritten);
        let next_same_origin = self.sheets[pos + 1..]
            .iter()
            .find(|entry| entry.origin == origin)
            .map(|entry| entry.sheet.clone());
        let guard = self.dom.shared_lock.read();
        let prev_sheet = self.sheets[pos].sheet.clone();
        self.stylist.remove_stylesheet(prev_sheet, &guard);
        if let Some(before) = next_same_origin {
            self.stylist
                .insert_stylesheet_before(next_sheet.clone(), before, &guard);
        } else {
            self.stylist.append_stylesheet(next_sheet.clone(), &guard);
        }
        drop(guard);
        self.sheets[pos].sheet = next_sheet;
        let cq_changed = !removed_ids.is_empty() || !new_cq.is_empty();
        self.container_queries.extend(new_cq);
        if cq_changed {
            clear_container_caches(&mut self.dom, &mut self.snapshots, &removed_ids);
        }
        self.dom.mark_all_dirty();
        self.lazy_pseudos_dirty = true;
        true
    }

    /// Unregisters a previously added stylesheet and marks the DOM for restyling.
    pub fn remove_stylesheet(&mut self, handle: StyleSheetHandle) -> bool {
        let Some(pos) = self.sheets.iter().position(|entry| entry.handle == handle) else {
            return false;
        };
        let entry = self.sheets.remove(pos);
        if self.unstyled_sheet == Some(handle) {
            self.unstyled_sheet = None;
        }
        let removed_ids = self.remove_container_queries_for_sheet(handle.0);
        if !removed_ids.is_empty() {
            clear_container_caches(&mut self.dom, &mut self.snapshots, &removed_ids);
        }
        let guard = self.dom.shared_lock.read();
        self.stylist.remove_stylesheet(entry.sheet, &guard);
        self.dom.mark_all_dirty();
        self.lazy_pseudos_dirty = true;
        true
    }

    fn remove_container_queries_for_sheet(&mut self, sheet_id: u64) -> Vec<u32> {
        let mut removed = Vec::new();
        self.container_queries.retain(|q| {
            if q.sheet_id == sheet_id {
                removed.push(q.id);
                false
            } else {
                true
            }
        });
        removed
    }

    /// Toggles the minimal unstyled UA stylesheet (`krui://preset/unstyled`).
    pub fn set_unstyled(&mut self, unstyled: bool) {
        if self.config.unstyled == unstyled {
            return;
        }
        self.config.unstyled = unstyled;
        if unstyled {
            if self.unstyled_sheet.is_none() {
                self.unstyled_sheet =
                    Some(self.add_stylesheet(StyleSheetOrigin::UserAgent, UA_UNSTYLED_CSS));
            }
        } else if let Some(handle) = self.unstyled_sheet.take() {
            let _ = self.remove_stylesheet(handle);
        }
    }

    /// Removes all [`StyleSheetOrigin::Author`] stylesheets while keeping User-Agent sheets intact.
    pub fn clear_author_stylesheets(&mut self) {
        let author_handles: Vec<StyleSheetHandle> = self
            .sheets
            .iter()
            .filter(|s| s.origin == StyleSheetOrigin::Author)
            .map(|s| s.handle)
            .collect();
        for handle in author_handles {
            let _ = self.remove_stylesheet(handle);
        }
    }

    /// Updates viewport dimensions / DPR, invalidating `@media` queries and viewport units if changed.
    pub fn set_viewport(&mut self, width: f32, height: f32, dpr: f32) {
        if (self.config.viewport_width - width).abs() < f32::EPSILON
            && (self.config.viewport_height - height).abs() < f32::EPSILON
            && (self.config.dpr - dpr).abs() < f32::EPSILON
        {
            return;
        }
        self.config.viewport_width = width;
        self.config.viewport_height = height;
        self.config.dpr = dpr;
        self.rebuild_device();
    }

    /// Updates the document's evaluated color scheme (`light` / `dark`), marking dirty nodes.
    pub fn set_color_scheme(&mut self, color_scheme: ColorScheme) {
        if self.config.color_scheme == color_scheme {
            return;
        }
        self.config.color_scheme = color_scheme;
        self.rebuild_device();
    }

    /// Updates simulated pointer hardware fidelity (`FineHover` / `CoarseTouch`), marking dirty nodes.
    pub fn set_pointer(&mut self, pointer: crate::PointerKind) {
        if self.config.pointer == pointer {
            return;
        }
        self.config.pointer = pointer;
        self.rebuild_device();
    }

    /// Flushes stylesheet changes and performs single-threaded incremental restyling.
    pub fn restyle(&mut self) -> RestyleReport {
        {
            let _thread_guard = enter_layout_thread_state();
            let guard = self.dom.shared_lock.read();
            let guards = StylesheetGuards::same(&guard);
            self.stylist.flush(&guards);
        }

        let mut report = RestyleReport {
            nodes: self.drain_pending_hides(),
        };

        let Some(root_idx) = self.dom.root_element else {
            self.finalize_restyle_flags();
            return report;
        };

        {
            let _thread_guard = enter_layout_thread_state();
            let guard = self.dom.shared_lock.read();
            let arena = build_traverse_arena(&self.dom);
            link_traverse_arena(&arena, &self.dom);
            invalidate_relative_selectors_for_snapshots(&self.stylist, &arena, &self.snapshots);
            let doc = StyloDocument(&arena[0]);
            let mut visited = Vec::new();
            if let Some(root_el) = doc
                .as_node()
                .dom_children()
                .find(|n| n.0.idx == root_idx)
                .and_then(|n| n.as_element())
            {
                let traversal = RecalcStyleOnly {
                    shared: SharedStyleContext {
                        stylist: &self.stylist,
                        visited_styles_enabled: false,
                        options: StyleSystemOptions::default(),
                        guards: StylesheetGuards::same(&guard),
                        current_time_for_animations: 0.0,
                        traversal_flags: TraversalFlags::empty(),
                        snapshot_map: &self.snapshots,
                        animations: DocumentAnimationSet::default(),
                        registered_speculative_painters: &NoopPainters,
                    },
                    recorded_damage: Mutex::new(Vec::new()),
                    traversed_indices: Mutex::new(Vec::new()),
                };

                let token = RecalcStyleOnly::pre_traverse(root_el, traversal.shared_context());
                if token.should_traverse() {
                    style::driver::traverse_dom(&traversal, token, None);
                    if let Ok(list) = traversal.recorded_damage.lock() {
                        for (id, dmg) in list.iter() {
                            merge_damage(&mut report.nodes, *id, *dmg);
                        }
                    }
                    if let Ok(mut list) = traversal.traversed_indices.lock() {
                        visited = std::mem::take(&mut *list);
                    }
                }
            }
            crate::engine_queries::refresh_lazy_pseudos(
                &self.dom,
                &self.stylist,
                &mut self.lazy_pseudos,
                &mut self.lazy_pseudos_dirty,
                &arena,
                &guard,
                &visited,
                &mut report.nodes,
            );
        }

        self.finalize_restyle_flags();
        for (id, dmg) in self.drain_pending_hides() {
            merge_damage(&mut report.nodes, id, dmg);
        }
        crate::engine_queries::propagate_text_child_damage(&self.dom, &mut report.nodes);
        report
    }

    /// Drains accumulated non-fatal CSS parse diagnostics reported during stylesheet loads.
    pub fn take_diagnostics(&self) -> Vec<CssDiagnostic> {
        self.diagnostics
            .lock()
            .map(|mut d| std::mem::take(&mut *d))
            .unwrap_or_default()
    }

    fn drain_pending_hides(&self) -> HashMap<StyloNodeId, StyloDamage> {
        let mut out = HashMap::new();
        let drained = self
            .dom
            .pending_hide_nodes
            .lock()
            .map(|mut list| std::mem::take(&mut *list))
            .unwrap_or_default();
        for id in drained {
            if let Some(slot) = self.dom.slots.get(id.index() as usize) {
                slot.pending_hide_relayout.store(false, Ordering::Relaxed);
            }
            out.insert(id, StyloDamage::Relayout);
        }
        out
    }

    fn finalize_restyle_flags(&mut self) {
        self.dom.clear_all_snapshots();
        self.snapshots.clear();
    }

    fn rebuild_device(&mut self) {
        let prev_root_fs = self.stylist.device().root_font_size().px();
        let prev_root_lh = self.stylist.device().root_line_height().px();
        let device = build_device(&self.config);
        device.set_root_font_size(prev_root_fs);
        device.set_root_line_height(prev_root_lh);
        let changed = {
            let guard = self.dom.shared_lock.read();
            let guards = StylesheetGuards::same(&guard);
            self.stylist.set_device(device, &guards)
        };
        if !changed.is_empty() {
            self.stylist.force_stylesheet_origins_dirty(changed);
        }
        self.dom.mark_all_dirty();
        self.lazy_pseudos_dirty = true;
    }

    fn parse_stylesheet(&self, origin: StyleSheetOrigin, css: &str) -> DocumentStyleSheet {
        let media = Arc::new(self.dom.shared_lock.wrap(MediaList::empty()));
        let origin = match origin {
            StyleSheetOrigin::UserAgent => Origin::UserAgent,
            StyleSheetOrigin::Author => Origin::Author,
        };
        let reporter = DiagnosticCollector(&self.diagnostics);
        let rewritten = crate::servo_ext::rewrite_servo_extensions(css);
        let sheet = Stylesheet::from_str(
            rewritten.as_ref(),
            self.base_url.clone(),
            origin,
            media,
            self.dom.shared_lock.clone(),
            None,
            Some(&reporter),
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );
        DocumentStyleSheet(Arc::new(sheet))
    }
}

pub(crate) fn merge_damage(
    map: &mut HashMap<StyloNodeId, StyloDamage>,
    id: StyloNodeId,
    next: StyloDamage,
) {
    map.entry(id)
        .and_modify(|cur| {
            if *cur == StyloDamage::Repaint && next == StyloDamage::Relayout {
                *cur = StyloDamage::Relayout;
            }
        })
        .or_insert(next);
}

struct LayoutThreadGuard(bool);

impl Drop for LayoutThreadGuard {
    fn drop(&mut self) {
        if self.0 {
            thread_state::exit(ThreadState::LAYOUT);
        }
    }
}

fn enter_layout_thread_state() -> LayoutThreadGuard {
    if thread_state::get().contains(ThreadState::LAYOUT) {
        LayoutThreadGuard(false)
    } else {
        thread_state::enter(ThreadState::LAYOUT);
        LayoutThreadGuard(true)
    }
}
