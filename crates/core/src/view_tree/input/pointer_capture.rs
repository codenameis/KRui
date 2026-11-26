//! Central pointer capture, focus-origin tracking (`:focus-visible`), selection ownership,
//! `Tab` anchor continuation, multi-click detection, and drag-outside autoscroll ticks.

use crate::view_tree::focus_pointer::{apply_pointer_down_at_opts, PointerCallback};
use crate::view_tree::hit_test::node_contains_screen_point;
use crate::view_tree::key_routing::Mods;
use crate::view_tree::node::{is_node_text_selectable, FocusOrigin, NodeId};
use crate::view_tree::paint_text_decorations::apply_text_selection_drag;
use crate::view_tree::pointer_events::{build_pointer_event_for_node, PointerButton};
use crate::view_tree::scroll::try_start_scrollbar_pointer_down;
use crate::view_tree::select_popup::hit_select_popup_row;
use crate::view_tree::text_selection_granularity::compute_drag_autoscroll_step;
use crate::view_tree::{A11yId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_signals::batch;
use krui_style::DisplayMode;

const MULTI_CLICK_MAX_MS: u64 = 500;
const MULTI_CLICK_MAX_DIST_PX: f32 = 4.0;

/// Selection expansion mode recorded on pointer-down (single / double / triple click).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SelectionGranularity {
    #[default]
    Char,
    Word {
        anchor_start: usize,
        anchor_end: usize,
    },
    Line {
        anchor_start: usize,
        anchor_end: usize,
    },
}

/// Platform-agnostic click cadence tracker for double- and triple-click selection.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MultiClickTracker {
    last_pos: (f32, f32),
    last_ms: u64,
    last_node: Option<NodeId>,
    count: u8,
}

impl MultiClickTracker {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn record_click(
        &mut self,
        node: Option<NodeId>,
        x: f32,
        y: f32,
        timestamp_ms: Option<u64>,
    ) -> u8 {
        let Some(ts) = timestamp_ms else {
            self.reset();
            return 1;
        };
        let within_time = ts.saturating_sub(self.last_ms) <= MULTI_CLICK_MAX_MS;
        let within_dist =
            (x - self.last_pos.0).hypot(y - self.last_pos.1) <= MULTI_CLICK_MAX_DIST_PX;
        if node.is_some() && node == self.last_node && within_time && within_dist {
            self.count = if self.count >= 3 { 1 } else { self.count + 1 };
        } else {
            self.count = 1;
        }
        self.last_node = node;
        self.last_pos = (x, y);
        self.last_ms = ts;
        self.count
    }
}

/// Grouped focus origin, text selection ownership, pointer capture, and multi-click state.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PointerFocusState {
    pub(crate) focus_origin: FocusOrigin,
    pub(crate) selection_owner: Option<NodeId>,
    pub(crate) tab_anchor_node: Option<NodeId>,
    pub(crate) pointer_capture: Option<NodeId>,
    pub(crate) aux_button_capture: Option<(NodeId, PointerButton)>,
    pub(crate) pointer_lock: Option<NodeId>,
    pub(crate) held_buttons: u8,
    pub(crate) pointer_inside_captured: bool,
    pub(crate) selection_granularity: SelectionGranularity,
    pub(crate) multi_click: MultiClickTracker,
}

impl PointerFocusState {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn release_node(&mut self, id: NodeId) {
        if self.selection_owner == Some(id) {
            self.selection_owner = None;
        }
        if self.tab_anchor_node == Some(id) {
            self.tab_anchor_node = None;
        }
        if self.pointer_capture == Some(id) {
            self.pointer_capture = None;
            self.pointer_inside_captured = false;
            self.selection_granularity = SelectionGranularity::Char;
        }
        if self.aux_button_capture.is_some_and(|(nid, _)| nid == id) {
            self.aux_button_capture = None;
        }
        if self.pointer_lock == Some(id) {
            self.pointer_lock = None;
        }
        if self.multi_click.last_node == Some(id) {
            self.multi_click.reset();
        }
    }
}

impl ViewTreeInner {
    pub(crate) fn is_node_focus_visible(&self, id: NodeId) -> bool {
        if self.a11y_engine.focused_id() != Some(A11yId::from(id).0) {
            return false;
        }
        self.node(id).is_some_and(|n| {
            !n.is_disabled()
                && (self.interaction.focus_origin == FocusOrigin::Keyboard
                    || n.role.is_text_editable())
        })
    }

    pub(crate) fn is_node_active_visual(&self, id: NodeId) -> bool {
        let direct = self.pressed_node == Some(id);
        let forwarded = !direct
            && self
                .pressed_node
                .and_then(|pid| crate::view_tree::controls::resolve_label_target_node(self, pid))
                == Some(id);
        if !direct && !forwarded {
            return false;
        }
        let Some(n) = self.node(id) else {
            return false;
        };
        if n.is_disabled() {
            return false;
        }
        if direct && self.interaction.pointer_capture == Some(id) {
            self.interaction.pointer_inside_captured
                || n.role == SemanticRole::Slider
                || self.selecting_node == Some(id)
                || self.scrollbar_drag.is_some_and(|(sid, _)| sid == id)
        } else {
            true
        }
    }

    pub(crate) fn clear_selection_on_node(&mut self, id: NodeId) -> bool {
        if let Some(ed) = self.sparse.text_edits.get_mut(&id) {
            if ed.selection_anchor_char_idx.take().is_some() {
                self.paint_dirty = true;
                return true;
            }
        }
        false
    }

    pub(crate) fn clear_other_text_selections(&mut self, keep: Option<NodeId>) -> bool {
        let mut changed = false;
        for (&id, ed) in &mut self.sparse.text_edits {
            if Some(id) != keep && ed.selection_anchor_char_idx.take().is_some() {
                changed = true;
            }
        }
        if changed {
            self.paint_dirty = true;
        }
        changed
    }

    pub(crate) fn apply_captured_pointer_move_opts(
        &mut self,
        cap_id: NodeId,
        x: f32,
        y: f32,
        delta: (f32, f32),
        mods: Mods,
    ) -> (Option<PointerCallback>, bool) {
        let inside = node_contains_screen_point(self, cap_id, x, y);
        let mut changed = false;
        if self.interaction.pointer_capture == Some(cap_id)
            && self.interaction.pointer_inside_captured != inside
        {
            self.interaction.pointer_inside_captured = inside;
            crate::view_tree::sync::notify_selector_state_change(self, cap_id);
            changed = true;
        }
        let new_hover = inside.then_some(cap_id);
        if set_hovered_node_and_notify(self, new_hover) {
            changed = true;
        }
        crate::view_tree::pointer_vars::update_hover_pointer_coords(self, self.hovered_node, x, y);
        if changed {
            self.paint_dirty = true;
        }
        let cb = self
            .node(cap_id)
            .is_some_and(|n| !n.is_disabled())
            .then(|| {
                let ev = build_pointer_event_for_node(
                    self,
                    cap_id,
                    (x, y),
                    delta,
                    PointerButton::Primary,
                    0,
                    mods,
                );
                self.listeners(cap_id)
                    .and_then(|l| l.on_pointer_move.clone())
                    .map(|cb| (cb, ev))
            })
            .flatten();
        (cb, changed)
    }

    /// Synchronizes `focus_origin`, `tab_anchor_node`, and `selection_owner` after focus transition.
    pub(crate) fn sync_focus_origin_and_selection(
        &mut self,
        new_focus: Option<NodeId>,
        origin: FocusOrigin,
    ) {
        if let Some(nid) = new_focus {
            self.interaction.focus_origin = origin;
            self.interaction.tab_anchor_node = Some(nid);
            if self.interaction.selection_owner != Some(nid) {
                if let Some(prev_sel) = self.interaction.selection_owner.take() {
                    self.clear_selection_on_node(prev_sel);
                }
                if self.node(nid).is_some_and(is_node_text_selectable) {
                    self.interaction.selection_owner = Some(nid);
                }
            }
            self.paint_dirty = true;
        } else if origin != FocusOrigin::Pointer {
            self.interaction.focus_origin = FocusOrigin::Programmatic;
        }
    }

    /// Assigns `selection_owner` and `tab_anchor_node` after pointer-down without stealing keyboard focus for non-focusable selectable nodes.
    pub(crate) fn apply_pointer_selection_ownership(
        &mut self,
        target: NodeId,
        selectable: bool,
        new_focus: Option<NodeId>,
    ) {
        self.interaction.tab_anchor_node = Some(target);
        if selectable && new_focus.is_none() {
            if self.interaction.selection_owner != Some(target) {
                if let Some(prev) = self.interaction.selection_owner.take() {
                    self.clear_selection_on_node(prev);
                }
                self.interaction.selection_owner = Some(target);
            }
            self.paint_dirty = true;
        } else if !selectable && new_focus.is_none() {
            if let Some(prev) = self.interaction.selection_owner.take() {
                self.clear_selection_on_node(prev);
            }
        }
    }

    pub(crate) fn resolve_tab_target_from_anchor(&self, shift: bool) -> Option<NodeId> {
        let anchor = self.interaction.tab_anchor_node?;
        let mut scan = TabAnchorScan {
            anchor,
            seen_anchor: false,
            first: None,
            last: None,
            last_before: None,
            first_after: None,
        };
        for &root_id in &self.roots {
            scan_tab_anchor_subtree(self, root_id, &mut scan);
        }
        if !scan.seen_anchor {
            return None;
        }
        if shift {
            scan.last_before.or(scan.last)
        } else {
            scan.first_after.or(scan.first)
        }
    }
}

struct TabAnchorScan {
    anchor: NodeId,
    seen_anchor: bool,
    first: Option<NodeId>,
    last: Option<NodeId>,
    last_before: Option<NodeId>,
    first_after: Option<NodeId>,
}

fn scan_tab_anchor_subtree(inner: &ViewTreeInner, node_id: NodeId, scan: &mut TabAnchorScan) {
    let Some(n) = inner.node(node_id) else {
        return;
    };
    if n.is_anchor() || n.style.layout.display == DisplayMode::None {
        return;
    }
    if node_id == scan.anchor {
        scan.seen_anchor = true;
    }
    if !n.is_disabled() && n.role.is_focusable() {
        if scan.first.is_none() {
            scan.first = Some(node_id);
        }
        scan.last = Some(node_id);
        if scan.seen_anchor && node_id != scan.anchor && scan.first_after.is_none() {
            scan.first_after = Some(node_id);
        } else if !scan.seen_anchor {
            scan.last_before = Some(node_id);
        }
    }
    for &child_id in &n.children {
        scan_tab_anchor_subtree(inner, child_id, scan);
    }
}

pub(crate) fn run_pointer_down(
    tree: &ViewTree,
    x: f32,
    y: f32,
    mods: Mods,
    timestamp_ms: Option<u64>,
) -> bool {
    let mut scroll_cb = None;
    let (focus_cbs, slider_act, ptr_down_cb, changed) = {
        let mut inner = tree.inner.borrow_mut();
        if hit_select_popup_row(&inner, x, y).is_none() {
            if let Some(scb) = try_start_scrollbar_pointer_down(&mut inner, x, y) {
                inner.last_pointer_pos = Some((x, y));
                if let Some((sid, _)) = inner.scrollbar_drag {
                    inner.interaction.pointer_capture = Some(sid);
                    inner.interaction.pointer_inside_captured = true;
                }
                scroll_cb = scb;
                (Default::default(), None, None, true)
            } else {
                apply_pointer_down_at_opts(&mut inner, x, y, mods, timestamp_ms)
            }
        } else {
            apply_pointer_down_at_opts(&mut inner, x, y, mods, timestamp_ms)
        }
    };
    if let Some(cb) = scroll_cb {
        batch(|| cb());
    }
    focus_cbs.invoke();
    if let Some(act) = slider_act {
        act.invoke();
    }
    if let Some((cb, ev)) = ptr_down_cb {
        batch(|| cb(ev));
    }
    changed
}

impl ViewTree {
    /// Returns the node currently holding pointer capture, if any.
    pub fn captured_pointer_node(&self) -> Option<NodeId> {
        self.inner.borrow().interaction.pointer_capture
    }

    /// Assigns pointer capture to `node_id` if it is live and not disabled.
    pub fn set_pointer_capture(&mut self, node_id: NodeId) -> bool {
        let mut inner = self.inner.borrow_mut();
        if inner
            .node(node_id)
            .is_none_or(|n| n.is_anchor() || n.is_disabled())
        {
            return false;
        }
        inner.interaction.pointer_capture = Some(node_id);
        inner.interaction.pointer_inside_captured = inner
            .last_pointer_pos
            .is_some_and(|(x, y)| node_contains_screen_point(&inner, node_id, x, y));
        true
    }

    /// Releases active pointer capture without emitting a click event.
    pub fn release_pointer_capture(&mut self) -> bool {
        let mut inner = self.inner.borrow_mut();
        let cap_opt = inner.interaction.pointer_capture.take();
        let had = cap_opt.is_some();
        if inner.interaction.pointer_inside_captured || had {
            inner.interaction.pointer_inside_captured = false;
            if let Some(cid) = cap_opt {
                crate::view_tree::sync::notify_selector_state_change(&mut inner, cid);
            }
            inner.paint_dirty = true;
        }
        had
    }

    /// Returns the node that currently owns text selection (`<input>`, `<textarea>`, or selectable `<p>`).
    pub fn selection_owner(&self) -> Option<NodeId> {
        self.inner.borrow().interaction.selection_owner
    }

    /// Returns `true` when `node_id` matches `:focus-visible` (keyboard `Tab` or text-editable focus).
    pub fn is_focus_visible(&self, node_id: NodeId) -> bool {
        self.inner.borrow().is_node_focus_visible(node_id)
    }

    /// Dispatches a pointer-down with modifier flags (supporting `Shift+Click` selection extension).
    pub fn dispatch_pointer_down_with_mods(&mut self, x: f32, y: f32, mods: Mods) -> bool {
        run_pointer_down(self, x, y, mods, None)
    }

    /// Dispatches a pointer-down with modifier flags and millisecond timestamp (enabling double/triple click).
    pub fn dispatch_pointer_down_with_mods_at_time(
        &mut self,
        x: f32,
        y: f32,
        mods: Mods,
        timestamp_ms: u64,
    ) -> bool {
        run_pointer_down(self, x, y, mods, Some(timestamp_ms))
    }

    /// Handles pointer leaving the viewport (`CursorLeft`), clearing non-captured hover state.
    pub fn dispatch_pointer_leave(&mut self) -> bool {
        let mut inner = self.inner.borrow_mut();
        let mut changed = set_hovered_node_and_notify(&mut inner, None);
        crate::view_tree::pointer_vars::update_hover_pointer_coords(
            &mut inner, None, -10_000.0, -10_000.0,
        );
        if let Some(cid) = inner.interaction.pointer_capture {
            if inner.interaction.pointer_inside_captured {
                inner.interaction.pointer_inside_captured = false;
                crate::view_tree::sync::notify_selector_state_change(&mut inner, cid);
                changed = true;
            }
        }
        if changed {
            inner.paint_dirty = true;
        }
        changed
    }

    /// Cancels active pointer capture, pressed controls, and drag states without activating clicks.
    pub fn dispatch_pointer_cancel(&mut self) -> bool {
        let mut inner = self.inner.borrow_mut();
        let changed = inner.interaction.pointer_capture.take().is_some()
            || inner.interaction.aux_button_capture.take().is_some()
            || inner.pressed_node.take().is_some()
            || inner.hovered_node.take().is_some()
            || inner.selecting_node.take().is_some()
            || inner.scrollbar_drag.take().is_some()
            || inner.interaction.pointer_inside_captured;
        inner.interaction.held_buttons = 0;
        inner.interaction.pointer_inside_captured = false;
        inner.interaction.selection_granularity = SelectionGranularity::Char;
        if changed {
            inner.paint_dirty = true;
        }
        changed
    }

    /// Returns `true` while an active pointer-captured text selection drag is in progress.
    pub fn is_text_drag_selecting(&self) -> bool {
        let inner = self.inner.borrow();
        let Some(nid) = inner.selecting_node else {
            return false;
        };
        inner.interaction.pointer_capture == Some(nid)
            && inner.node(nid).is_some_and(is_node_text_selectable)
    }

    /// Advances edge autoscroll and extends text selection while pointer drag pauses outside bounds.
    pub fn tick_pointer_drag_autoscroll(&mut self, dt_secs: f32) -> bool {
        let dt = dt_secs.clamp(0.0, 0.1);
        if dt <= 0.0 {
            return false;
        }
        let mut inner = self.inner.borrow_mut();
        let Some(nid) = inner.selecting_node else {
            return false;
        };
        let Some((px, py)) = inner.last_pointer_pos else {
            return false;
        };
        let (dx, dy) = compute_drag_autoscroll_step(&mut inner, nid, (px, py), dt);
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        let _ = apply_text_selection_drag(&mut inner, px, py);
        inner.paint_dirty = true;
        true
    }
}

fn collect_hover_chain(inner: &ViewTreeInner, start: Option<NodeId>) -> Vec<NodeId> {
    let mut chain = Vec::with_capacity(8);
    let mut cur = start;
    for _ in 0..64 {
        let Some(nid) = cur else { break };
        chain.push(nid);
        cur = inner.node(nid).and_then(|n| n.parent);
    }
    chain
}

/// Updates `inner.hovered_node` and invalidates selectors for every ancestor/leaf entering or
/// leaving the hovered ancestor chain (needed for CSS `group-hover` / ancestor `:hover` rules).
pub(crate) fn set_hovered_node_and_notify(
    inner: &mut ViewTreeInner,
    new_hover: Option<NodeId>,
) -> bool {
    if inner.hovered_node == new_hover {
        return false;
    }
    let old_chain = collect_hover_chain(inner, inner.hovered_node);
    let new_chain = collect_hover_chain(inner, new_hover);
    inner.hovered_node = new_hover;
    for &nid in old_chain.iter().chain(new_chain.iter()) {
        if !(old_chain.contains(&nid) && new_chain.contains(&nid)) {
            crate::view_tree::transitions::commit_node_transitions_from_style(inner, nid);
        }
    }
    for &nid in &old_chain {
        if !new_chain.contains(&nid) {
            crate::view_tree::sync::notify_selector_state_change(inner, nid);
        }
    }
    for &nid in &new_chain {
        if !old_chain.contains(&nid) {
            crate::view_tree::sync::notify_selector_state_change(inner, nid);
        }
    }
    true
}
