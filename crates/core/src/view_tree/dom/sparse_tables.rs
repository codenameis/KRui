//! Sparse secondary component tables (`TextEditData`, `ControlData`, `EventListeners`,
//! `ScrollData`, `CanvasData`, `AnchorData`) stored by `NodeId` on `ViewTreeInner` so plain
//! `<div>`/`<span>` elements carry zero unused editing, form, listener, scroll, or canvas bloat.

use crate::scene::{CanvasCommand, CanvasDraw, CanvasRedrawMode};
use crate::view_tree::form_submit::ButtonKind;
use crate::view_tree::grapheme::TextEditSnapshot;
use crate::view_tree::media_nodes::MediaData;
use crate::view_tree::node::{ElementNode, ImeState};
use crate::view_tree::transitions::ActivePaintTransition;
use crate::view_tree::{InputKind, NodeId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_signals::{Effect, Scope};
use krui_style::PaintStyle;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

/// Compact 2-byte boolean flag bitset stored on every [`ElementNode`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NodeFlags(u16);

impl NodeFlags {
    pub(crate) const ALIVE: u16 = 1 << 0;
    pub(crate) const IS_ANCHOR: u16 = 1 << 1;
    pub(crate) const IS_SECRET: u16 = 1 << 2;
    pub(crate) const IS_DISABLED: u16 = 1 << 3;
    pub(crate) const IS_CHECKED: u16 = 1 << 4;
    pub(crate) const IS_INVALID: u16 = 1 << 5;
    pub(crate) const TEXT_DIRTY: u16 = 1 << 6;
    pub(crate) const HAS_DYNAMIC_TEXT_BINDING: u16 = 1 << 7;
    pub(crate) const IS_SYNTHETIC_TEXT_RUN: u16 = 1 << 8;
    pub(crate) const VISIBILITY_HIDDEN: u16 = 1 << 9;
    pub(crate) const POINTER_EVENTS_NONE: u16 = 1 << 10;

    #[inline]
    pub(crate) const fn contains(self, mask: u16) -> bool {
        (self.0 & mask) != 0
    }

    #[inline]
    pub(crate) fn set(&mut self, mask: u16, enabled: bool) {
        if enabled {
            self.0 |= mask;
        } else {
            self.0 &= !mask;
        }
    }
}

/// Sparse text editing, selection, IME composition, undo/redo, and inner scroll state
/// allocated only for `<input>`, `<textarea>`, and actively selected text nodes.
#[derive(Clone, Debug, Default)]
pub(crate) struct TextEditData {
    pub(crate) placeholder: String,
    pub(crate) committed_value_on_focus: String,
    pub(crate) cursor_char_idx: usize,
    pub(crate) selection_anchor_char_idx: Option<usize>,
    pub(crate) preferred_x: Option<f32>,
    pub(crate) needs_caret_scroll: bool,
    pub(crate) ime_state: ImeState,
    pub(crate) undo_stack: Vec<TextEditSnapshot>,
    pub(crate) redo_stack: Vec<TextEditSnapshot>,
    pub(crate) scroll_x: f32,
    pub(crate) scroll_y: f32,
}

/// Sparse form control metadata (`<select>` options, radio groups, range bounds, validation flags).
#[derive(Clone)]
pub(crate) struct ControlData {
    pub(crate) input_kind: InputKind,
    pub(crate) title: Option<String>,
    pub(crate) label_for: Option<String>,
    pub(crate) control_name: Option<String>,
    pub(crate) range_bounds: (f32, f32, f32),
    pub(crate) is_required: bool,
    pub(crate) button_kind: ButtonKind,
    pub(crate) select_options: Vec<(String, String, bool)>,
    pub(crate) selected_option_idx: Option<usize>,
    pub(crate) initial_text: Option<String>,
    pub(crate) initial_checked: Option<bool>,
    pub(crate) on_checked_change: Option<Rc<dyn Fn(bool)>>,
    pub(crate) on_submit: Option<Rc<dyn Fn()>>,
    pub(crate) on_reset: Option<Rc<dyn Fn()>>,
}

impl Default for ControlData {
    fn default() -> Self {
        Self {
            input_kind: InputKind::Text,
            title: None,
            label_for: None,
            control_name: None,
            range_bounds: (0.0, 100.0, 1.0),
            is_required: false,
            button_kind: ButtonKind::Auto,
            select_options: Vec::new(),
            selected_option_idx: None,
            initial_text: None,
            initial_checked: None,
            on_checked_change: None,
            on_submit: None,
            on_reset: None,
        }
    }
}

/// Sparse DOM event listener table allocated only for nodes with attached callbacks.
#[derive(Clone, Default)]
pub(crate) struct EventListeners {
    pub(crate) on_click: Option<Rc<dyn Fn()>>,
    pub(crate) on_focus: Option<Rc<dyn Fn()>>,
    pub(crate) on_blur: Option<Rc<dyn Fn()>>,
    pub(crate) on_pointer_down: Option<Rc<dyn Fn(f32, f32)>>,
    pub(crate) on_pointer_up: Option<Rc<dyn Fn(f32, f32)>>,
    pub(crate) on_pointer_move: Option<Rc<dyn Fn(f32, f32)>>,
    pub(crate) on_scroll: Option<Rc<dyn Fn()>>,
    pub(crate) on_input: Option<Rc<dyn Fn(String)>>,
    pub(crate) on_change: Option<Rc<dyn Fn(String)>>,
    pub(crate) on_keydown: Option<Rc<dyn Fn(String)>>,
}

impl EventListeners {
    #[inline]
    pub(crate) fn has_interactive_pointer_or_focus(&self) -> bool {
        self.on_click.is_some()
            || self.on_focus.is_some()
            || self.on_blur.is_some()
            || self.on_pointer_down.is_some()
            || self.on_pointer_up.is_some()
            || self.on_pointer_move.is_some()
    }
}

/// Sparse scroll offsets and cached content overflow bounds for `overflow: scroll` and `<ul>` containers.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ScrollData {
    pub(crate) scroll_x: f32,
    pub(crate) scroll_y: f32,
    pub(crate) cached_max_scroll_x: f32,
    pub(crate) cached_max_scroll_y: f32,
}

/// Sparse `<canvas>` 2D command buffer.
pub(crate) struct CanvasData {
    pub(crate) canvas_callback: Option<CanvasDraw>,
    pub(crate) canvas_mode: CanvasRedrawMode,
    pub(crate) canvas_commands: Arc<[CanvasCommand]>,
    pub(crate) canvas_dirty: bool,
}

impl Default for CanvasData {
    fn default() -> Self {
        Self {
            canvas_callback: None,
            canvas_mode: CanvasRedrawMode::Reactive,
            canvas_commands: Arc::from([]),
            canvas_dirty: false,
        }
    }
}

/// Sparse `<if>` / `<for>` control-flow anchor state tracking spliced child nodes and owned item scopes.
#[derive(Default)]
pub(crate) struct AnchorData {
    pub(crate) mounted_nodes: Vec<NodeId>,
    pub(crate) owned_scopes: Vec<Scope>,
}

/// Secondary component storage indexed by generational [`NodeId`].
pub(crate) type PaintSlotReapplier = Rc<dyn Fn(&mut PaintStyle)>;

#[derive(Default)]
pub(crate) struct SparseComponents {
    pub(crate) text_edits: HashMap<NodeId, TextEditData>,
    pub(crate) controls: HashMap<NodeId, ControlData>,
    pub(crate) listeners: HashMap<NodeId, EventListeners>,
    pub(crate) scrolls: HashMap<NodeId, ScrollData>,
    pub(crate) canvases: HashMap<NodeId, CanvasData>,
    pub(crate) media: HashMap<NodeId, MediaData>,
    pub(crate) anchors: HashMap<NodeId, AnchorData>,
    pub(crate) effects: HashMap<NodeId, Vec<Effect>>,
    pub(crate) layout_slot_nodes: HashSet<NodeId>,
    pub(crate) paint_slot_reappliers: HashMap<NodeId, Vec<PaintSlotReapplier>>,
    pub(crate) classes: HashMap<NodeId, Vec<String>>,
    pub(crate) last_target_paints: HashMap<NodeId, PaintStyle>,
    pub(crate) active_transitions: HashMap<NodeId, ActivePaintTransition>,
    pub(crate) last_target_layouts: HashMap<NodeId, (krui_style::LayoutStyle, f32)>,
    pub(crate) active_layout_transitions:
        HashMap<NodeId, crate::view_tree::transitions_layout::ActiveLayoutTransition>,
    pub(crate) last_cascaded_animations: HashMap<NodeId, krui_style::AnimationListRef>,
    pub(crate) rich_spans: HashMap<NodeId, Arc<[krui_style::StyledTextSpan]>>,
    pub(crate) pseudo_pointer:
        HashMap<NodeId, crate::view_tree::pointer_vars::PointerAndPseudoData>,
    pub(crate) on_link_activate: Option<Rc<dyn Fn(String)>>,
    pub(crate) last_activated_link: Option<String>,
}

impl SparseComponents {
    /// Clears cached target paints/layouts and running transitions so state snaps immediately.
    pub(crate) fn clear_transitions(&mut self) {
        self.last_target_paints.clear();
        self.active_transitions.clear();
        self.last_target_layouts.clear();
        self.active_layout_transitions.clear();
    }

    /// Removes every secondary component entry for `id`, returning its owned `Effect`s and `Scope`s.
    pub(crate) fn purge_node(&mut self, id: NodeId) -> (Vec<Effect>, Vec<Scope>) {
        self.text_edits.remove(&id);
        self.controls.remove(&id);
        self.listeners.remove(&id);
        self.scrolls.remove(&id);
        self.canvases.remove(&id);
        let media_scope = self.media.remove(&id).and_then(|mut m| m.load_scope.take());
        self.layout_slot_nodes.remove(&id);
        self.paint_slot_reappliers.remove(&id);
        self.classes.remove(&id);
        self.last_target_paints.remove(&id);
        self.active_transitions.remove(&id);
        self.last_target_layouts.remove(&id);
        self.active_layout_transitions.remove(&id);
        self.last_cascaded_animations.remove(&id);
        self.rich_spans.remove(&id);
        self.pseudo_pointer.remove(&id);
        let effects = self.effects.remove(&id).unwrap_or_default();
        let mut scopes = self
            .anchors
            .remove(&id)
            .map(|a| a.owned_scopes)
            .unwrap_or_default();
        if let Some(s) = media_scope {
            scopes.push(s);
        }
        (effects, scopes)
    }

    /// Clears all sparse component tables during full-tree replacement, returning leftover `Effect`s and `Scope`s.
    pub(crate) fn clear_all_for_reset(&mut self) -> (Vec<Effect>, Vec<Scope>) {
        self.text_edits.clear();
        self.controls.clear();
        self.listeners.clear();
        self.scrolls.clear();
        self.canvases.clear();
        let media_scopes: Vec<Scope> = self
            .media
            .drain()
            .filter_map(|(_, mut m)| m.load_scope.take())
            .collect();
        self.layout_slot_nodes.clear();
        self.paint_slot_reappliers.clear();
        self.classes.clear();
        self.rich_spans.clear();
        self.pseudo_pointer.clear();
        self.clear_transitions();
        self.last_cascaded_animations.clear();
        self.last_activated_link = None;
        let effects = self
            .effects
            .drain()
            .flat_map(|(_, effs)| effs.into_iter())
            .collect();
        let mut scopes: Vec<Scope> = self
            .anchors
            .drain()
            .flat_map(|(_, a)| a.owned_scopes.into_iter())
            .collect();
        scopes.extend(media_scopes);
        (effects, scopes)
    }

    /// Returns the number of live entries across secondary component tables.
    pub(crate) fn behavior_entry_count(&self) -> usize {
        self.text_edits.len()
            + self.controls.len()
            + self.listeners.len()
            + self.scrolls.len()
            + self.canvases.len()
            + self.media.len()
            + self.anchors.len()
    }

    /// Returns the total number of live entries across all sparse secondary component tables.
    pub(crate) fn total_entry_count(&self) -> usize {
        self.behavior_entry_count()
            + self.effects.len()
            + self.layout_slot_nodes.len()
            + self.paint_slot_reappliers.len()
            + self.classes.len()
            + self.rich_spans.len()
            + self.pseudo_pointer.len()
    }
}

/// Split mutable borrow bundle combining an [`ElementNode`] with its [`TextEditData`] and input listener.
pub(crate) struct NodeEditBorrow<'a> {
    pub(crate) node: &'a mut ElementNode,
    pub(crate) edit: &'a mut TextEditData,
    pub(crate) on_input: Option<Rc<dyn Fn(String)>>,
}

impl ViewTreeInner {
    #[inline]
    pub(crate) fn text_edit(&self, id: NodeId) -> Option<&TextEditData> {
        self.sparse.text_edits.get(&id)
    }

    #[inline]
    pub(crate) fn ensure_text_edit(&mut self, id: NodeId) -> &mut TextEditData {
        self.sparse.text_edits.entry(id).or_default()
    }

    #[inline]
    pub(crate) fn control(&self, id: NodeId) -> Option<&ControlData> {
        self.sparse.controls.get(&id)
    }

    #[inline]
    pub(crate) fn ensure_control(&mut self, id: NodeId) -> &mut ControlData {
        self.sparse.controls.entry(id).or_default()
    }

    #[inline]
    pub(crate) fn ensure_pseudo_pointer(
        &mut self,
        id: NodeId,
    ) -> &mut crate::view_tree::pointer_vars::PointerAndPseudoData {
        self.sparse.pseudo_pointer.entry(id).or_default()
    }

    #[inline]
    pub(crate) fn listeners(&self, id: NodeId) -> Option<&EventListeners> {
        self.sparse.listeners.get(&id)
    }

    #[inline]
    pub(crate) fn ensure_listeners(&mut self, id: NodeId) -> &mut EventListeners {
        self.sparse.listeners.entry(id).or_default()
    }

    #[inline]
    pub(crate) fn scroll(&self, id: NodeId) -> Option<&ScrollData> {
        self.sparse.scrolls.get(&id)
    }

    #[inline]
    pub(crate) fn ensure_scroll(&mut self, id: NodeId) -> &mut ScrollData {
        self.sparse.scrolls.entry(id).or_default()
    }

    #[inline]
    pub(crate) fn ensure_canvas(&mut self, id: NodeId) -> &mut CanvasData {
        self.sparse.canvases.entry(id).or_default()
    }

    #[inline]
    pub(crate) fn anchor(&self, id: NodeId) -> Option<&AnchorData> {
        self.sparse.anchors.get(&id)
    }

    #[inline]
    pub(crate) fn ensure_anchor(&mut self, id: NodeId) -> &mut AnchorData {
        self.sparse.anchors.entry(id).or_default()
    }

    #[inline]
    pub(crate) fn push_effect(&mut self, id: NodeId, effect: Effect) {
        if self.node(id).is_some() {
            self.sparse.effects.entry(id).or_default().push(effect);
        } else {
            effect.dispose();
        }
    }

    #[inline]
    pub(crate) fn node_edit_mut(&mut self, id: NodeId) -> Option<NodeEditBorrow<'_>> {
        let idx = id.index as usize;
        let node = self
            .nodes
            .get_mut(idx)
            .filter(|n| n.alive() && n.id.generation == id.generation)?;
        let edit = self.sparse.text_edits.entry(id).or_default();
        let on_input = self
            .sparse
            .listeners
            .get(&id)
            .and_then(|l| l.on_input.clone());
        Some(NodeEditBorrow {
            node,
            edit,
            on_input,
        })
    }
}

impl ViewTree {
    /// Returns the number of live entries across the 6 secondary behavior tables
    /// (`TextEditData`, `ControlData`, `EventListeners`, `ScrollData`, `CanvasData`, `AnchorData`).
    pub fn sparse_behavior_entry_count(&self) -> usize {
        self.inner.borrow().sparse.behavior_entry_count()
    }
}

pub(crate) fn init_sparse_for_compiled_node(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    role: SemanticRole,
    is_secret: bool,
    is_canvas: bool,
    is_scroll: bool,
) {
    if role.is_text_editable() {
        let _ = inner.ensure_text_edit(node_id);
    }
    if is_secret {
        inner.ensure_control(node_id).input_kind = InputKind::Password;
    }
    if is_canvas {
        inner.ensure_canvas(node_id).canvas_dirty = true;
    }
    if is_scroll {
        let _ = inner.ensure_scroll(node_id);
    }
}

pub(crate) fn attach_compiled_node_to_parent(
    inner: &mut ViewTreeInner,
    parent: Option<NodeId>,
    node_id: NodeId,
    taffy_node: taffy::NodeId,
) {
    if let Some(pid) = parent {
        if inner.node(pid).and_then(|p| p.taffy_node).is_some() {
            if let Some(pnode) = inner.node_mut(pid) {
                pnode.children.push(node_id);
            }
            crate::view_tree::layout_modes::sync_after_dom_attach(
                inner,
                Some(pid),
                node_id,
                taffy_node,
            );
        } else {
            inner
                .a11y_engine
                .unregister_focusable(crate::view_tree::A11yId::from(node_id).0);
            let _ = inner.sparse.purge_node(node_id);
            let idx = node_id.index as usize;
            let (orphan_sn, orphan_ts) = inner
                .nodes
                .get_mut(idx)
                .map(|orphan| {
                    let sn = orphan.stylo_node.take();
                    let ts = orphan.text_stylo_node.take();
                    let _ = orphan.clear_for_unmount();
                    inner.generations[idx] = inner.generations[idx].wrapping_add(1);
                    inner.free_nodes.push(node_id.index);
                    (sn, ts)
                })
                .unwrap_or((None, None));
            if let Some(ts) = orphan_ts {
                inner
                    .stylo
                    .apply_mutation(krui_stylo_bridge::StyloDomMutation::DisposeSubtree(ts));
            }
            if let Some(sn) = orphan_sn {
                inner.stylo_to_view.remove(&sn);
                inner
                    .stylo
                    .apply_mutation(krui_stylo_bridge::StyloDomMutation::DisposeSubtree(sn));
            }
            inner.layout_engine.remove_node(taffy_node);
        }
    } else {
        inner.roots.push(node_id);
        crate::view_tree::layout_modes::sync_after_dom_attach(inner, None, node_id, taffy_node);
    }
}

pub(crate) fn collect_unmount_postorder(inner: &ViewTreeInner, root_id: NodeId) -> Vec<NodeId> {
    let mut visited = Vec::new();
    collect_unmount_recursive(inner, root_id, &mut visited);
    visited
}

fn collect_unmount_recursive(inner: &ViewTreeInner, cur: NodeId, out: &mut Vec<NodeId>) {
    let Some(n) = inner.node(cur) else {
        return;
    };
    if out.contains(&cur) {
        return;
    };
    if let Some(anchor) = inner.anchor(cur) {
        for &mounted in &anchor.mounted_nodes {
            collect_unmount_recursive(inner, mounted, out);
        }
    }
    for &child in &n.children {
        collect_unmount_recursive(inner, child, out);
    }
    if !out.contains(&cur) {
        out.push(cur);
    }
}

pub(crate) fn detach_node_from_parent(inner: &mut ViewTreeInner, node_id: NodeId) {
    let Some((target_taffy, parent_id, was_fixed)) = inner.node(node_id).map(|n| {
        (
            n.taffy_node,
            n.parent,
            n.style.layout.position == krui_style::PositionMode::Fixed,
        )
    }) else {
        return;
    };
    inner.fixed_nodes.retain(|&id| id != node_id);
    if let Some(tn) = target_taffy {
        let parent_taffy = parent_id.and_then(|pid| inner.node(pid)?.taffy_node);
        if parent_id.is_none() || parent_taffy.is_some() {
            inner.layout_engine.remove_child(parent_taffy, tn);
        }
    }
    if let Some(pid) = parent_id {
        if let Some(pnode) = inner.node_mut(pid) {
            pnode.children.retain(|&c| c != node_id);
        }
    } else {
        inner.roots.retain(|&r| r != node_id);
    }
    crate::view_tree::layout_modes::sync_after_dom_detach(inner, parent_id, was_fixed);
}
