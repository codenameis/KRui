//! Pointer press, capture, move, drag, release, and selection-ownership routing.

use crate::view_tree::controls::{
    apply_slider_pointer_at, resolve_label_target_node, ControlActivationCallbacks,
};
use crate::view_tree::events::{
    activate_control_on_pointer_release, apply_click_caret_and_selection_opts,
};
pub(crate) use crate::view_tree::focus_transition::{
    apply_disabled_state_change, collect_enabled_focusable_order, transition_focus,
    transition_focus_with_origin, FocusTransitionCallbacks,
};
use crate::view_tree::hit_test::{
    effective_node_world_matrix_and_rect, hit_test_hover_node, hit_test_interactive_node,
    node_contains_screen_point,
};
use crate::view_tree::key_routing::Mods;
use crate::view_tree::media_nodes::invoke_post_activation_actions;
use crate::view_tree::node::{is_node_text_selectable, FocusOrigin};
use crate::view_tree::paint_text_decorations::apply_text_selection_drag;
use crate::view_tree::pointer_capture::{run_pointer_down, SelectionGranularity};
use crate::view_tree::pointer_events::{build_pointer_event_for_node, PointerButton, PointerEvent};
use crate::view_tree::scroll::apply_scrollbar_pointer_drag;
use crate::view_tree::select_popup::{
    close_select_popup, commit_select_option_index, hit_select_popup_row,
};
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_signals::batch;
use krui_style::UserSelect;
use std::rc::Rc;

pub(crate) type PointerCallback = (Rc<dyn Fn(PointerEvent)>, PointerEvent);

fn resolve_focus_target_flags(
    inner: &ViewTreeInner,
    focus_nid: NodeId,
) -> (bool, bool, bool, bool, bool, bool) {
    let has_focus_cb = inner
        .listeners(focus_nid)
        .is_some_and(|l| l.on_focus.is_some() || l.on_blur.is_some());
    inner
        .node(focus_nid)
        .map(|n| {
            (
                is_node_text_selectable(n),
                n.role.is_focusable() || has_focus_cb,
                n.role.is_text_editable(),
                n.role == SemanticRole::Slider,
                n.style.paint.user_select == UserSelect::All,
                n.is_disabled(),
            )
        })
        .unwrap_or((false, false, false, false, false, true))
}

pub(crate) fn apply_pointer_down_at_opts(
    inner: &mut ViewTreeInner,
    x: f32,
    y: f32,
    mods: Mods,
    timestamp_ms: Option<u64>,
) -> (
    FocusTransitionCallbacks,
    Option<ControlActivationCallbacks>,
    Option<PointerCallback>,
    bool,
) {
    inner.last_pointer_pos = Some((x, y));
    if inner.stylo_dirty {
        crate::view_tree::sync::flush_pending_restyle(inner);
    }
    if let Some((select_id, row_opt)) = hit_select_popup_row(inner, x, y) {
        if let Some(old_press) = inner.pressed_node.take() {
            crate::view_tree::sync::notify_selector_state_change(inner, old_press);
        }
        inner.interaction.pointer_capture = None;
        inner.interaction.pointer_inside_captured = false;
        let row_act = row_opt.and_then(|idx| commit_select_option_index(inner, select_id, idx));
        return (FocusTransitionCallbacks::default(), row_act, None, true);
    }
    let hit = hit_test_interactive_node(inner, x, y);
    let dismissed_popover =
        crate::view_tree::semantics::dismiss_light_popovers_on_pointer_down(inner, hit);
    let target_hit = hit.map(|nid| resolve_label_target_node(inner, nid).unwrap_or(nid));
    let dismissed_popup = if inner
        .overlay
        .select_popup
        .as_ref()
        .is_some_and(|p| Some(p.select_id) != target_hit)
    {
        close_select_popup(inner)
    } else {
        false
    } || dismissed_popover;
    let preserve_shift = if mods.shift {
        target_hit.filter(|&tid| inner.interaction.selection_owner == Some(tid))
    } else {
        None
    };
    inner.clear_other_text_selections(preserve_shift);
    inner.selecting_node = None;
    let Some(nid) = hit else {
        let had_sel = inner.interaction.selection_owner.take().is_some();
        inner.interaction.tab_anchor_node = None;
        inner.interaction.pointer_capture = None;
        inner.interaction.pointer_inside_captured = false;
        inner.interaction.selection_granularity = SelectionGranularity::Char;
        let _ = inner
            .interaction
            .multi_click
            .record_click(None, x, y, timestamp_ms);
        let old_press = inner.pressed_node.take();
        if let Some(pid) = old_press {
            crate::view_tree::sync::notify_selector_state_change(inner, pid);
        }
        let had = inner.a11y_engine.focused_id().is_some()
            || old_press.is_some()
            || dismissed_popup
            || had_sel;
        let focus_cbs = transition_focus_with_origin(inner, None, FocusOrigin::Pointer);
        inner.paint_dirty = true;
        return (focus_cbs, None, None, had);
    };
    apply_hit_pointer_down_opts(inner, nid, x, y, mods, timestamp_ms)
}

type PointerDownPrep = (
    FocusTransitionCallbacks,
    Option<ControlActivationCallbacks>,
    Option<PointerCallback>,
    bool,
);

fn notify_pressed_pair(inner: &mut ViewTreeInner, prev: Option<NodeId>, next: Option<NodeId>) {
    for id_opt in crate::view_tree::semantics::collect_press_notification_pair(inner, prev)
        .into_iter()
        .chain(crate::view_tree::semantics::collect_press_notification_pair(inner, next))
        .flatten()
    {
        crate::view_tree::sync::notify_selector_state_change(inner, id_opt);
    }
}

fn take_pointer_up_callback(
    inner: &ViewTreeInner,
    target: Option<NodeId>,
    (x, y): (f32, f32),
    mods: Mods,
) -> Option<PointerCallback> {
    target
        .filter(|&nid| inner.node(nid).is_some_and(|n| !n.is_disabled()))
        .and_then(|nid| {
            let ev = build_pointer_event_for_node(
                inner,
                nid,
                (x, y),
                (0.0, 0.0),
                PointerButton::Primary,
                1,
                mods,
            );
            inner
                .listeners(nid)?
                .on_pointer_up
                .clone()
                .map(|cb| (cb, ev))
        })
}

fn apply_hit_pointer_down_opts(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    x: f32,
    y: f32,
    mods: Mods,
    timestamp_ms: Option<u64>,
) -> PointerDownPrep {
    let label_target = resolve_label_target_node(inner, nid);
    let focus_nid = label_target.unwrap_or(nid);
    let Some(is_disabled) = inner.node(nid).map(|n| n.is_disabled()) else {
        return (FocusTransitionCallbacks::default(), None, None, false);
    };
    let ptr_down_cb = (!is_disabled)
        .then(|| inner.listeners(nid)?.on_pointer_down.clone())
        .flatten();
    let (selectable, is_focusable, is_inp, is_slider, sel_all, target_disabled) =
        resolve_focus_target_flags(inner, focus_nid);
    let active_target = !is_disabled && !target_disabled;
    let same_owner_for_shift = mods.shift && inner.interaction.selection_owner == Some(nid);
    let old_press = inner.pressed_node;
    inner.pressed_node = (!is_disabled).then_some(nid);
    if old_press != inner.pressed_node {
        notify_pressed_pair(inner, old_press, inner.pressed_node);
    }
    if !is_disabled {
        crate::view_tree::pointer_vars::latch_pointer_down_ancestors(inner, nid, x, y);
    }
    inner.interaction.pointer_capture = (!is_disabled).then_some(nid);
    inner.interaction.pointer_inside_captured = !is_disabled;
    let click_count = if !is_disabled {
        inner
            .interaction
            .multi_click
            .record_click(Some(nid), x, y, timestamp_ms)
    } else {
        1
    };
    let new_focus = (is_focusable && active_target).then(|| {
        inner.caret_visible = true;
        focus_nid
    });
    let focus_cbs = transition_focus_with_origin(inner, new_focus, FocusOrigin::Pointer);
    inner.apply_pointer_selection_ownership(focus_nid, selectable && active_target, new_focus);
    let slider_act =
        if label_target.is_none() && !is_disabled && (is_slider || is_inp || selectable) {
            apply_hit_local_pointer_down(
                inner,
                nid,
                (x, y),
                is_slider,
                (selectable, sel_all, same_owner_for_shift, click_count),
            )
        } else {
            None
        };
    let ptr_down_ev = ptr_down_cb.as_ref().map(|_| {
        build_pointer_event_for_node(
            inner,
            nid,
            (x, y),
            (0.0, 0.0),
            PointerButton::Primary,
            click_count,
            mods,
        )
    });
    inner.paint_dirty = true;
    (focus_cbs, slider_act, ptr_down_cb.zip(ptr_down_ev), true)
}

fn apply_hit_local_pointer_down(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    (x, y): (f32, f32),
    is_slider: bool,
    (selectable, sel_all, same_owner_for_shift, click_count): (bool, bool, bool, u8),
) -> Option<ControlActivationCallbacks> {
    let (aff, rect) = effective_node_world_matrix_and_rect(inner, nid);
    let (lx, ly) = aff.inverse()?.transform_point(x, y);
    if is_slider {
        let act = apply_slider_pointer_at(inner, nid, lx, rect);
        inner.selecting_node = Some(nid);
        return act;
    }
    apply_click_caret_and_selection_opts(
        inner,
        nid,
        lx,
        ly,
        rect,
        selectable,
        sel_all,
        same_owner_for_shift,
        click_count,
    );
    None
}

struct PointerUpPrep {
    ptr_up_cb: Option<PointerCallback>,
    ctrl_act: Option<ControlActivationCallbacks>,
    click_cb: Option<Rc<dyn Fn()>>,
    fwd_click_cb: Option<Rc<dyn Fn()>>,
    matched_press: Option<NodeId>,
    changed: bool,
}

fn resolve_pointer_up_inner(
    inner: &mut ViewTreeInner,
    x: f32,
    y: f32,
    mods: Mods,
) -> PointerUpPrep {
    inner.last_pointer_pos = Some((x, y));
    let cap = inner.interaction.pointer_capture.take();
    inner.interaction.pointer_inside_captured = false;
    let had_scroll_drag = inner.scrollbar_drag.take().is_some();
    let pressed = inner.pressed_node.take();
    notify_pressed_pair(inner, pressed, None);
    if let Some(nid) = inner.selecting_node.take() {
        if let Some(ed) = inner.sparse.text_edits.get_mut(&nid) {
            if ed.selection_anchor_char_idx == Some(ed.cursor_char_idx) {
                ed.selection_anchor_char_idx = None;
            }
        }
    }
    inner.interaction.selection_granularity = SelectionGranularity::Char;
    let in_popup = hit_select_popup_row(inner, x, y).is_some();
    let release_hit = (!in_popup)
        .then(|| hit_test_interactive_node(inner, x, y))
        .flatten();
    let hover_hit = (!in_popup)
        .then(|| hit_test_hover_node(inner, x, y))
        .flatten();
    if crate::view_tree::pointer_capture::set_hovered_node_and_notify(inner, hover_hit) {
        inner.paint_dirty = true;
    }
    crate::view_tree::pointer_vars::update_hover_pointer_coords(inner, inner.hovered_node, x, y);
    let ptr_up_cb = take_pointer_up_callback(inner, cap.or(release_hit), (x, y), mods);
    let matched_press = pressed.filter(|&pid| {
        !in_popup && release_hit == Some(pid) && node_contains_screen_point(inner, pid, x, y)
    });
    let (ctrl_act, toggled) = match matched_press {
        Some(pid) => activate_control_on_pointer_release(inner, pid),
        None => (None, false),
    };
    let click_cb = matched_press
        .filter(|&pid| inner.node(pid).is_some_and(|n| !n.is_disabled()))
        .and_then(|pid| inner.listeners(pid)?.on_click.clone());
    let fwd_click_cb = matched_press
        .filter(|&pid| inner.node(pid).is_some_and(|n| !n.is_disabled()))
        .and_then(|pid| crate::view_tree::semantics::take_forwarded_label_click(inner, pid));
    let changed = had_scroll_drag
        || cap.is_some()
        || pressed.is_some()
        || ptr_up_cb.is_some()
        || toggled
        || click_cb.is_some()
        || fwd_click_cb.is_some();
    if pressed.is_some() || cap.is_some() {
        inner.paint_dirty = true;
    }
    PointerUpPrep {
        ptr_up_cb,
        ctrl_act,
        click_cb,
        fwd_click_cb,
        matched_press,
        changed,
    }
}

impl ViewTree {
    /// Marks the interactive node under `(x, y)` as pressed (`:active`), transitions focus,
    /// positions the input caret, or selects an option inside an open Top-Layer `<select>` popup.
    pub fn dispatch_pointer_down(&mut self, x: f32, y: f32) -> bool {
        run_pointer_down(self, x, y, Mods::default(), None)
    }

    /// Releases the pointer at the last known pointer position.
    pub fn dispatch_pointer_up(&mut self) -> bool {
        let (rx, ry) = self
            .inner
            .borrow()
            .last_pointer_pos
            .unwrap_or((-10_000.0, -10_000.0));
        self.dispatch_pointer_up_at(rx, ry)
    }

    /// Releases the pointer at `(x, y)`, firing `onpointerup` on capture and `onclick` inside pressed bounds.
    pub fn dispatch_pointer_up_at(&mut self, x: f32, y: f32) -> bool {
        self.dispatch_pointer_up_with_mods(x, y, Mods::default())
    }

    /// Releases the primary pointer at `(x, y)` with keyboard modifier flags `mods`.
    pub fn dispatch_pointer_up_with_mods(&mut self, x: f32, y: f32, mods: Mods) -> bool {
        let prep = resolve_pointer_up_inner(&mut self.inner.borrow_mut(), x, y, mods);
        if let Some((cb, ev)) = prep.ptr_up_cb {
            batch(|| cb(ev));
        }
        if let Some(act) = prep.ctrl_act {
            act.invoke();
        }
        let mut clicked = false;
        if let Some(cb) = prep.click_cb {
            batch(|| cb());
            clicked = true;
        }
        if let Some(cb) = prep.fwd_click_cb {
            batch(|| cb());
            clicked = true;
        }
        if let Some(pid) = prep.matched_press {
            clicked |= invoke_post_activation_actions(self, pid);
        }
        clicked || prep.changed
    }

    /// Dispatches pointer movement at `(x, y)` to active capture or hover targets and `onpointermove`.
    pub fn dispatch_pointer_move(&mut self, x: f32, y: f32) -> bool {
        self.dispatch_pointer_move_with_mods(x, y, Mods::default())
    }

    /// Updates the active text selection range, slider value, or scrollbar thumb while dragging the mouse.
    pub fn dispatch_pointer_drag(&mut self, x: f32, y: f32) -> bool {
        let scroll_drag = {
            let mut inner = self.inner.borrow_mut();
            inner.last_pointer_pos = Some((x, y));
            apply_scrollbar_pointer_drag(&mut inner, x, y)
        };
        if let Some((scroll_cb, changed)) = scroll_drag {
            if let Some(cb) = scroll_cb {
                batch(|| cb());
            }
            return changed;
        }
        let slider_drag = {
            let mut inner = self.inner.borrow_mut();
            try_apply_slider_drag(&mut inner, x, y)
        };
        if let Some(act) = slider_drag {
            act.invoke();
            return true;
        }
        let mut inner = self.inner.borrow_mut();
        apply_text_selection_drag(&mut inner, x, y)
    }

    /// Finishes a mouse drag gesture on mouse button release.
    pub fn end_pointer_drag(&mut self) {
        let has_active = {
            let inner = self.inner.borrow();
            inner.pressed_node.is_some() || inner.interaction.pointer_capture.is_some()
        };
        if has_active {
            let _ = self.dispatch_pointer_up();
            return;
        }
        let mut inner = self.inner.borrow_mut();
        inner.scrollbar_drag = None;
        inner.interaction.selection_granularity = SelectionGranularity::Char;
        if let Some(nid) = inner.selecting_node.take() {
            if let Some(ed) = inner.sparse.text_edits.get_mut(&nid) {
                if ed.selection_anchor_char_idx == Some(ed.cursor_char_idx) {
                    ed.selection_anchor_char_idx = None;
                }
            }
        }
    }
}

fn try_apply_slider_drag(
    inner: &mut ViewTreeInner,
    x: f32,
    y: f32,
) -> Option<ControlActivationCallbacks> {
    let nid = inner.selecting_node?;
    if !inner
        .node(nid)
        .is_some_and(|n| n.role == SemanticRole::Slider)
    {
        return None;
    }
    let (world_affine, local_rect) = effective_node_world_matrix_and_rect(inner, nid);
    let (lx, _ly) = world_affine.inverse()?.transform_point(x, y);
    apply_slider_pointer_at(inner, nid, lx, local_rect)
}
