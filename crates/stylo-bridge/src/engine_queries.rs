//! Encapsulated style, metadata, `@keyframes`, and `@font-face` query methods on
//! [`StyloStyleEngine`], including lazy pseudo-element (`::selection`, `::placeholder`, `::backdrop`)
//! resolution without exposing Stylo types outside `krui-stylo-bridge`.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use style::properties::ComputedValues;
use style::selector_parser::PseudoElement;
use style::servo_arc::Arc;
use style::shared_lock::{SharedRwLockReadGuard, StylesheetGuards};
use style::stylist::{RuleInclusion, Stylist};
use style::values::computed::Color as StyloColor;

use krui_style::{
    Color, FontFaceRule, InheritedTextProps, KeyframesDef, LayoutStyle, PaintStyle, TextStyle,
};

use crate::computed_to_layout::extract_layout_style;
use crate::computed_to_paint::{
    absolute_color_to_krui, extract_box_meta, extract_paint_style, extract_pseudo_content,
    stylo_color_to_krui,
};
use crate::computed_to_text::extract_text_style;
use crate::dom_node::{StyloElement, TraverseNode};
use crate::dom_store::StyloDom;
use crate::engine::merge_damage;
use crate::font_face_extract::extract_font_face_rules_with;
use crate::keyframes_extract::extract_keyframes_defs;
use crate::{BoxMeta, PseudoElementKind, StyloDamage, StyloNodeId, StyloStyleEngine};

#[derive(Clone, Default)]
pub(crate) struct LazyPseudoCache {
    pub(crate) placeholder: Option<Arc<ComputedValues>>,
    pub(crate) backdrop: Option<Arc<ComputedValues>>,
}

impl LazyPseudoCache {
    fn is_empty(&self) -> bool {
        self.placeholder.is_none() && self.backdrop.is_none()
    }
}

fn to_stylo_pseudo(kind: PseudoElementKind) -> PseudoElement {
    match kind {
        PseudoElementKind::Before => PseudoElement::Before,
        PseudoElementKind::After => PseudoElement::After,
        PseudoElementKind::Selection => PseudoElement::Selection,
        PseudoElementKind::Placeholder => PseudoElement::Placeholder,
        PseudoElementKind::Backdrop => PseudoElement::Backdrop,
    }
}

impl StyloStyleEngine {
    /// Returns resolved compositor [`PaintStyle`] (folding matched `::selection`/`::placeholder`).
    #[must_use]
    pub fn paint_style(&self, node: StyloNodeId) -> Option<PaintStyle> {
        let cv = self.computed_values(node)?;
        let mut paint = extract_paint_style(&cv);
        if let Some(sel) = self.pseudo_computed_values(node, PseudoElementKind::Selection) {
            let cur = sel.get_inherited_text().clone_color();
            let fg = absolute_color_to_krui(&cur);
            let bg = stylo_color_to_krui(&sel.get_background().background_color, &cur);
            paint.selection_color = Some(fg);
            if bg.a > 0 {
                paint.selection_background = Some(bg);
            }
        }
        if let Some(ph) = self.pseudo_computed_values(node, PseudoElementKind::Placeholder) {
            let ph_col = absolute_color_to_krui(&ph.get_inherited_text().clone_color());
            let ph_opacity = ph.get_effects().opacity;
            paint.placeholder_color = Some(ph_col);
            paint.placeholder_opacity = if ph_opacity.is_finite() {
                ph_opacity.clamp(0.0, 1.0)
            } else {
                1.0
            };
        }
        Some(paint)
    }

    /// Returns resolved [`TextStyle`] and [`InheritedTextProps`] for `node` or its parent element.
    #[must_use]
    pub fn text_style(&self, node: StyloNodeId) -> Option<(TextStyle, InheritedTextProps)> {
        let cv = self.computed_values(node)?;
        Some(extract_text_style(&cv))
    }

    /// Returns resolved layout/interaction [`BoxMeta`] for `node`.
    #[must_use]
    pub fn box_meta(&self, node: StyloNodeId) -> Option<BoxMeta> {
        let cv = self.computed_values(node)?;
        Some(extract_box_meta(&cv))
    }

    /// Returns a pure-Rust [`LayoutStyle`] snapshot for `node`.
    #[must_use]
    pub fn layout_style(&self, node: StyloNodeId) -> Option<LayoutStyle> {
        let cv = self.computed_values(node)?;
        Some(extract_layout_style(&cv))
    }

    /// Returns `(tracks_inline_size, tracks_block_size)` from CSS `container-type` on `node`.
    #[must_use]
    pub fn container_type_axes(&self, node: StyloNodeId) -> (bool, bool) {
        use style::values::specified::box_::ContainerType;
        let Some(cv) = self.computed_values(node) else {
            return (false, false);
        };
        let ct = cv.get_box().clone_container_type();
        (
            ct.intersects(ContainerType::INLINE_SIZE | ContainerType::SIZE),
            ct.contains(ContainerType::SIZE),
        )
    }

    /// Returns resolved pseudo-element [`PaintStyle`] (`::before`, `::after`, `::selection`,
    /// `::placeholder`, `::backdrop`).
    #[must_use]
    pub fn pseudo_paint_style(
        &self,
        node: StyloNodeId,
        pseudo: PseudoElementKind,
    ) -> Option<PaintStyle> {
        let cv = self.pseudo_computed_values(node, pseudo)?;
        Some(extract_paint_style(&cv))
    }

    /// Returns resolved pseudo-element [`LayoutStyle`].
    #[must_use]
    pub fn pseudo_layout_style(
        &self,
        node: StyloNodeId,
        pseudo: PseudoElementKind,
    ) -> Option<LayoutStyle> {
        let cv = self.pseudo_computed_values(node, pseudo)?;
        Some(extract_layout_style(&cv))
    }

    /// Returns resolved pseudo-element [`BoxMeta`].
    #[must_use]
    pub fn pseudo_box_meta(&self, node: StyloNodeId, pseudo: PseudoElementKind) -> Option<BoxMeta> {
        let cv = self.pseudo_computed_values(node, pseudo)?;
        Some(extract_box_meta(&cv))
    }

    /// Returns resolved pseudo-element [`TextStyle`] and [`InheritedTextProps`].
    #[must_use]
    pub fn pseudo_text_style(
        &self,
        node: StyloNodeId,
        pseudo: PseudoElementKind,
    ) -> Option<(TextStyle, InheritedTextProps)> {
        let cv = self.pseudo_computed_values(node, pseudo)?;
        Some(extract_text_style(&cv))
    }

    /// Returns resolved generated `content` text for `::before` or `::after` (`None` if not generated).
    #[must_use]
    pub fn pseudo_content(&self, node: StyloNodeId, pseudo: PseudoElementKind) -> Option<String> {
        let cv = self.pseudo_computed_values(node, pseudo)?;
        extract_pseudo_content(&cv)
    }

    /// Extracts all active `@keyframes` animation definitions across registered stylesheets.
    #[must_use]
    pub fn keyframes_defs(&self) -> Vec<KeyframesDef> {
        let guard = self.dom.shared_lock.read();
        let root_cv = self
            .dom
            .root_element
            .and_then(|idx| self.dom.slots.get(idx as usize))
            .map(|slot| StyloNodeId::new(idx_from_slot(self, slot), slot.generation))
            .and_then(|id| self.computed_values(id));
        let sheets: Vec<_> = self.sheets.iter().map(|s| s.sheet.clone()).collect();
        extract_keyframes_defs(&self.stylist, &sheets, root_cv.as_deref(), &guard)
    }

    /// Extracts all active `@font-face` rules declared in registered stylesheets.
    #[must_use]
    pub fn font_face_rules(&self) -> Vec<FontFaceRule> {
        self.font_face_rules_with(|_| None)
    }

    /// Extracts all active `@font-face` rules, resolving non-`data:` URLs via `resolve_embedded`.
    #[must_use]
    pub fn font_face_rules_with<F>(&self, resolve_embedded: F) -> Vec<FontFaceRule>
    where
        F: Fn(&str) -> Option<std::sync::Arc<[u8]>>,
    {
        let guard = self.dom.shared_lock.read();
        extract_font_face_rules_with(&self.stylist, &guard, &resolve_embedded)
    }

    /// Resolves the document canvas clear background color from `:root` (or `TRANSPARENT`).
    #[must_use]
    pub fn canvas_background(&self, root_node: StyloNodeId) -> Color {
        let Some(cv) = self.computed_values(root_node) else {
            return Color::TRANSPARENT;
        };
        let cur = cv.get_inherited_text().clone_color();
        let bg = &cv.get_background().background_color;
        if *bg == StyloColor::TRANSPARENT_BLACK {
            Color::TRANSPARENT
        } else {
            stylo_color_to_krui(bg, &cur)
        }
    }

    /// Resolves the W3C propagated canvas clear background color across `<html>`/`<body>`/`:root`.
    #[must_use]
    pub fn resolve_canvas_background(
        &self,
        doc_html: StyloNodeId,
        doc_body: StyloNodeId,
    ) -> (Color, Option<StyloNodeId>) {
        if let Some(body_idx) = self.dom.resolve_live(doc_body) {
            if let Some(found) = self.find_propagated_html_or_body_bg(body_idx) {
                return found;
            }
        }
        let doc_bg = self.canvas_background(doc_html);
        if doc_bg.a > 0 {
            return (doc_bg, None);
        }
        let body_bg = self.canvas_background(doc_body);
        if body_bg.a > 0 {
            return (body_bg, None);
        }
        (Color::WHITE, None)
    }

    fn find_propagated_html_or_body_bg(
        &self,
        doc_body_idx: u32,
    ) -> Option<(Color, Option<StyloNodeId>)> {
        let html_tag = style::LocalName::from("html");
        let body_tag = style::LocalName::from("body");
        let mut cur = self.dom.slots[doc_body_idx as usize].first_child;
        while let Some(cidx) = cur {
            let cslot = &self.dom.slots[cidx as usize];
            if cslot.is_element && cslot.local_name == html_tag {
                let cid = StyloNodeId::new(cidx, cslot.generation);
                let hbg = self.canvas_background(cid);
                if hbg.a > 0 {
                    return Some((hbg, Some(cid)));
                }
                let mut gc = cslot.first_child;
                while let Some(gc_idx) = gc {
                    let gslot = &self.dom.slots[gc_idx as usize];
                    if gslot.is_element && gslot.local_name == body_tag {
                        let bid = StyloNodeId::new(gc_idx, gslot.generation);
                        let bbg = self.canvas_background(bid);
                        if bbg.a > 0 {
                            return Some((bbg, Some(bid)));
                        }
                    }
                    gc = gslot.next_sibling;
                }
            } else if cslot.is_element && cslot.local_name == body_tag {
                let bid = StyloNodeId::new(cidx, cslot.generation);
                let bbg = self.canvas_background(bid);
                if bbg.a > 0 {
                    return Some((bbg, Some(bid)));
                }
            }
            cur = cslot.next_sibling;
        }
        None
    }

    pub(crate) fn computed_values(&self, node: StyloNodeId) -> Option<Arc<ComputedValues>> {
        let mut idx = self.dom.resolve_live(node)?;
        if self.dom.slots[idx as usize].is_text {
            idx = self.dom.slots[idx as usize].parent?;
        }
        let slot = &self.dom.slots[idx as usize];
        if !slot.has_style_data.load(Ordering::Relaxed) {
            return None;
        }
        slot.style_data.borrow().styles.get_primary().cloned()
    }

    pub(crate) fn pseudo_computed_values(
        &self,
        node: StyloNodeId,
        kind: PseudoElementKind,
    ) -> Option<Arc<ComputedValues>> {
        let idx = self.dom.resolve_live(node)?;
        let slot = &self.dom.slots[idx as usize];
        if !slot.has_style_data.load(Ordering::Relaxed) {
            return None;
        }
        let pseudo = to_stylo_pseudo(kind);
        if pseudo.is_eager() {
            return slot
                .style_data
                .borrow()
                .styles
                .pseudos
                .get(&pseudo)
                .cloned();
        }
        let lazy = self.lazy_pseudos.get(&node)?;
        match kind {
            PseudoElementKind::Placeholder => lazy.placeholder.clone(),
            PseudoElementKind::Backdrop => lazy.backdrop.clone(),
            _ => None,
        }
    }
}

fn lazy_pseudo_pair_eq(a: Option<&Arc<ComputedValues>>, b: Option<&Arc<ComputedValues>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(ca), Some(cb)) => {
            Arc::ptr_eq(ca, cb) || extract_paint_style(ca) == extract_paint_style(cb)
        }
        _ => false,
    }
}

fn lazy_cache_eq(prev: Option<&LazyPseudoCache>, next: &LazyPseudoCache) -> bool {
    let Some(p) = prev else {
        return next.is_empty();
    };
    lazy_pseudo_pair_eq(p.placeholder.as_ref(), next.placeholder.as_ref())
        && lazy_pseudo_pair_eq(p.backdrop.as_ref(), next.backdrop.as_ref())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn refresh_lazy_pseudos<'a>(
    dom: &StyloDom,
    stylist: &Stylist,
    lazy_pseudos: &mut HashMap<StyloNodeId, LazyPseudoCache>,
    lazy_pseudos_dirty: &mut bool,
    arena: &'a [TraverseNode<'a>],
    guard: &SharedRwLockReadGuard,
    traversed: &[u32],
    damage_out: &mut HashMap<StyloNodeId, StyloDamage>,
) {
    let guards = StylesheetGuards::same(guard);
    let dirty_all = std::mem::take(lazy_pseudos_dirty);
    let indices: Vec<u32> = if dirty_all {
        lazy_pseudos.retain(|id, _| dom.resolve_live(*id).is_some());
        arena
            .iter()
            .filter(|t| t.slot.occupied && t.slot.is_element)
            .map(|t| t.idx)
            .collect()
    } else {
        let mut seen = HashSet::new();
        traversed
            .iter()
            .copied()
            .chain(
                arena
                    .iter()
                    .filter(|t| {
                        t.slot.occupied
                            && t.slot.is_element
                            && t.slot.has_snapshot.load(Ordering::Relaxed)
                    })
                    .map(|t| t.idx),
            )
            .filter(|idx| seen.insert(*idx))
            .collect()
    };

    for idx in indices {
        refresh_lazy_pseudo_slot(
            idx,
            dirty_all,
            lazy_pseudos,
            arena,
            stylist,
            &guards,
            damage_out,
        );
    }
}

fn refresh_lazy_pseudo_slot<'a>(
    idx: u32,
    dirty_all: bool,
    lazy_pseudos: &mut HashMap<StyloNodeId, LazyPseudoCache>,
    arena: &'a [TraverseNode<'a>],
    stylist: &Stylist,
    guards: &StylesheetGuards<'_>,
    damage_out: &mut HashMap<StyloNodeId, StyloDamage>,
) {
    let Some(t_node) = arena.get(idx as usize) else {
        return;
    };
    if !t_node.slot.occupied || !t_node.slot.is_element {
        return;
    }
    let node_id = StyloNodeId::new(idx, t_node.slot.generation);
    let Some(primary) = t_node
        .slot
        .has_style_data
        .load(Ordering::Relaxed)
        .then(|| {
            t_node
                .slot
                .style_data
                .borrow()
                .styles
                .get_primary()
                .cloned()
        })
        .flatten()
    else {
        if lazy_pseudos.remove(&node_id).is_some() && !dirty_all {
            merge_damage(damage_out, node_id, StyloDamage::Repaint);
        }
        return;
    };
    let el = StyloElement(t_node);
    let probe = |pseudo: &PseudoElement| {
        stylist.lazily_compute_pseudo_element_style(
            guards,
            el,
            pseudo,
            RuleInclusion::All,
            &primary,
            true,
            None,
        )
    };
    let entry = LazyPseudoCache {
        placeholder: probe(&PseudoElement::Placeholder),
        backdrop: probe(&PseudoElement::Backdrop),
    };
    let changed = !lazy_cache_eq(lazy_pseudos.get(&node_id), &entry);
    if entry.is_empty() {
        lazy_pseudos.remove(&node_id);
    } else {
        lazy_pseudos.insert(node_id, entry);
    }
    if changed && !dirty_all {
        merge_damage(damage_out, node_id, StyloDamage::Repaint);
    }
}

fn idx_from_slot(engine: &StyloStyleEngine, slot: &crate::dom_store::StyloSlot) -> u32 {
    let base = engine.dom.slots.as_ptr() as usize;
    let cur = std::ptr::from_ref(slot) as usize;
    ((cur - base) / std::mem::size_of::<crate::dom_store::StyloSlot>()) as u32
}

impl StyloStyleEngine {
    /// Returns `true` if `node` is a Stylo text node rather than an element.
    #[must_use]
    pub fn is_text_node(&self, node: StyloNodeId) -> bool {
        self.dom
            .resolve_live(node)
            .is_some_and(|idx| self.dom.slots[idx as usize].is_text)
    }
}

pub(crate) fn propagate_text_child_damage(
    dom: &crate::dom_store::StyloDom,
    damage_out: &mut HashMap<StyloNodeId, StyloDamage>,
) {
    let parent_damage: Vec<(StyloNodeId, StyloDamage)> =
        damage_out.iter().map(|(&id, &d)| (id, d)).collect();
    for (parent_id, dmg) in parent_damage {
        let Some(pidx) = dom.resolve_live(parent_id) else {
            continue;
        };
        let mut cur = dom.slots[pidx as usize].first_child;
        while let Some(cidx) = cur {
            let cslot = &dom.slots[cidx as usize];
            if cslot.is_text {
                merge_damage(damage_out, StyloNodeId::new(cidx, cslot.generation), dmg);
            }
            cur = cslot.next_sibling;
        }
    }
}
