//! Click, keyboard navigation, cursor movement, and focus dispatch on `ViewTree`.

use crate::view_tree::controls::{
    activate_control_node, resolve_label_target_node, ControlActivationCallbacks,
};
use crate::view_tree::focus_pointer::{
    apply_pointer_down_at_opts, collect_enabled_focusable_order, transition_focus_with_origin,
};
use crate::view_tree::form_submit::try_submit_enclosing_form;
use crate::view_tree::key_routing::{
    clear_prevent_default_key, key_name_for_keydown, route_key_with_repeat,
    take_prevent_default_key, FocusState, KeyAction, KeyInfo, Mods,
};
use crate::view_tree::media_nodes::invoke_post_activation_actions;
use crate::view_tree::node::{sync_node_text_context, FocusOrigin};
use crate::view_tree::pointer_capture::SelectionGranularity;
use crate::view_tree::pointer_events::{build_pointer_event_for_node, PointerButton};
pub(crate) use crate::view_tree::scroll::compute_max_scroll_y;
pub use crate::view_tree::scroll::{KEYBOARD_SCROLL_LINE_STEP_PX, KEYBOARD_SCROLL_PAGE_FRACTION};
use crate::view_tree::select_popup::{close_select_popup, toggle_select_popup_on_click};
use crate::view_tree::text_edit::insert_clean_str_at_cursor;
use crate::view_tree::text_selection_granularity::apply_click_selection_from_local_coords;
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_signals::batch;
use std::rc::Rc;

pub(crate) type BlurChangeCallback = (Rc<dyn Fn(String)>, String);

pub(crate) fn activate_control_on_pointer_release(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
) -> (Option<ControlActivationCallbacks>, bool) {
    let target_id = resolve_label_target_node(inner, node_id).unwrap_or(node_id);
    let is_combo = inner
        .node(target_id)
        .is_some_and(|n| !n.is_disabled() && n.role == SemanticRole::ComboBox);
    if is_combo {
        let toggled = toggle_select_popup_on_click(inner, target_id);
        (None, toggled)
    } else {
        let act = activate_control_node(inner, node_id);
        let has_act = act.is_some();
        (act, has_act)
    }
}

pub(crate) fn take_pending_blur_change(
    inner: &mut ViewTreeInner,
    old_focus: Option<NodeId>,
    new_focus: Option<NodeId>,
) -> Option<BlurChangeCallback> {
    if old_focus == new_focus {
        return None;
    }
    let old_nid = old_focus?;
    let cur_text = inner.node(old_nid)?.text.clone();
    let changed = inner.sparse.text_edits.get_mut(&old_nid).is_some_and(|ed| {
        if cur_text != ed.committed_value_on_focus {
            ed.committed_value_on_focus = cur_text.clone();
            true
        } else {
            false
        }
    });
    if changed {
        inner
            .listeners(old_nid)
            .and_then(|l| l.on_change.clone())
            .map(|cb| (cb, cur_text))
    } else {
        None
    }
}

pub(crate) fn mark_focus_entered(inner: &mut ViewTreeInner, new_focus: Option<NodeId>) {
    let Some(nid) = new_focus else {
        return;
    };
    let Some((role, cur_text)) = inner.node(nid).map(|n| (n.role, n.text.clone())) else {
        return;
    };
    if role.is_text_editable() || inner.sparse.text_edits.contains_key(&nid) {
        inner.ensure_text_edit(nid).committed_value_on_focus = cur_text;
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_click_caret_and_selection_opts(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    lx: f32,
    ly: f32,
    local_rect: LayoutRect,
    selectable: bool,
    select_all_on_click: bool,
    preserve_shift: bool,
    click_count: u8,
) {
    apply_click_selection_from_local_coords(
        inner,
        nid,
        lx,
        ly,
        local_rect,
        selectable,
        select_all_on_click,
        preserve_shift,
        click_count,
    );
}

impl ViewTree {
    /// Clears focus and any active text selection from all widgets (or closes an open `<select>`
    /// dropdown popup first without losing `<select>` focus).
    pub fn clear_focus(&mut self) -> bool {
        let (focus_cbs, changed, popup_only) = {
            let mut inner = self.inner.borrow_mut();
            if crate::view_tree::semantics::handle_escape_top_layer(&mut inner) {
                return true;
            }
            let had_focus = inner.a11y_engine.focused_id().is_some();
            let had_sel_owner = inner.interaction.selection_owner.is_some();
            let focus_cbs =
                transition_focus_with_origin(&mut inner, None, FocusOrigin::Programmatic);
            let mut changed = had_focus || had_sel_owner || focus_cbs.ime_blur_input.is_some();
            inner.interaction.reset();
            inner.selecting_node = None;
            for ed in inner.sparse.text_edits.values_mut() {
                if ed.selection_anchor_char_idx.take().is_some() {
                    changed = true;
                }
            }
            if changed {
                inner.paint_dirty = true;
            }
            (focus_cbs, changed, false)
        };
        if popup_only {
            return true;
        }
        focus_cbs.invoke();
        changed
    }

    /// Dispatches a complete pointer click (press + release) at `(x, y)` in logical window coordinates.
    pub fn dispatch_click(&mut self, x: f32, y: f32) -> bool {
        let (
            focus_cbs,
            slider_act,
            ptr_down_cb,
            ptr_up_cb,
            ctrl_act,
            click_cb,
            fwd_cb,
            pressed,
            changed,
        ) = {
            let mut inner = self.inner.borrow_mut();
            resolve_dispatch_click_inner(&mut inner, x, y)
        };
        if popup_or_early_focus_invoke(focus_cbs, slider_act, ptr_down_cb, ptr_up_cb, ctrl_act) {
            return true;
        }
        let mut clicked = false;
        if let Some(cb) = click_cb {
            batch(|| cb());
            clicked = true;
        }
        if let Some(cb) = fwd_cb {
            batch(|| cb());
            clicked = true;
        }
        if let Some(pid) = pressed {
            clicked |= invoke_post_activation_actions(self, pid);
        }
        clicked || changed
    }
}

#[allow(clippy::type_complexity)]
fn resolve_dispatch_click_inner(
    inner: &mut ViewTreeInner,
    x: f32,
    y: f32,
) -> (
    crate::view_tree::focus_pointer::FocusTransitionCallbacks,
    Option<ControlActivationCallbacks>,
    Option<crate::view_tree::focus_pointer::PointerCallback>,
    Option<crate::view_tree::focus_pointer::PointerCallback>,
    Option<ControlActivationCallbacks>,
    Option<Rc<dyn Fn()>>,
    Option<Rc<dyn Fn()>>,
    Option<NodeId>,
    bool,
) {
    let (focus_cbs, slider_act, ptr_down_cb, down_changed) =
        apply_pointer_down_at_opts(inner, x, y, Mods::default(), None);
    let pressed = inner.pressed_node.take();
    for pid in crate::view_tree::semantics::collect_press_notification_pair(inner, pressed)
        .into_iter()
        .flatten()
    {
        crate::view_tree::sync::notify_selector_state_change(inner, pid);
    }
    inner.interaction.pointer_capture = None;
    inner.interaction.pointer_inside_captured = false;
    let (ctrl_act, toggled) = match pressed {
        Some(nid) => activate_control_on_pointer_release(inner, nid),
        None => (None, false),
    };
    let (ptr_up_cb, click_cb) = pressed
        .filter(|&nid| inner.node(nid).is_some_and(|n| !n.is_disabled()))
        .map(|nid| {
            let ptr_up = inner
                .listeners(nid)
                .and_then(|l| l.on_pointer_up.clone())
                .map(|cb| {
                    let ev = build_pointer_event_for_node(
                        inner,
                        nid,
                        (x, y),
                        (0.0, 0.0),
                        PointerButton::Primary,
                        1,
                        Mods::default(),
                    );
                    (cb, ev)
                });
            let click = inner.listeners(nid).and_then(|l| l.on_click.clone());
            (ptr_up, click)
        })
        .unwrap_or((None, None));
    let fwd_cb = pressed
        .filter(|&nid| inner.node(nid).is_some_and(|n| !n.is_disabled()))
        .and_then(|nid| crate::view_tree::semantics::take_forwarded_label_click(inner, nid));
    (
        focus_cbs,
        slider_act,
        ptr_down_cb,
        ptr_up_cb,
        ctrl_act,
        click_cb,
        fwd_cb,
        pressed,
        down_changed || toggled,
    )
}

fn popup_or_early_focus_invoke(
    focus_cbs: crate::view_tree::focus_pointer::FocusTransitionCallbacks,
    slider_act: Option<ControlActivationCallbacks>,
    ptr_down_cb: Option<crate::view_tree::focus_pointer::PointerCallback>,
    ptr_up_cb: Option<crate::view_tree::focus_pointer::PointerCallback>,
    ctrl_act: Option<ControlActivationCallbacks>,
) -> bool {
    focus_cbs.invoke();
    if let Some(act) = slider_act {
        act.invoke();
    }
    if let Some((cb, ev)) = ptr_down_cb {
        batch(|| cb(ev));
    }
    if let Some((cb, ev)) = ptr_up_cb {
        batch(|| cb(ev));
    }
    if let Some(act) = ctrl_act {
        act.invoke();
    }
    false
}

impl ViewTree {
    /// Cycles keyboard focus (`Tab` or `Shift+Tab`, Section 5) across enabled focusable controls.
    pub fn dispatch_key_tab(&mut self, shift: bool) -> Option<NodeId> {
        let (focus_cbs, new_focus) = {
            let mut inner = self.inner.borrow_mut();
            let prev_ak = inner.a11y_engine.focused_id();
            let order = collect_enabled_focusable_order(&inner);
            inner.a11y_engine.sync_focusable_order(order);
            let next_nid = if prev_ak.is_none() {
                inner.resolve_tab_target_from_anchor(shift)
            } else {
                None
            }
            .or_else(|| {
                let next_ak = if shift {
                    inner.a11y_engine.focus_previous()
                } else {
                    inner.a11y_engine.focus_next()
                };
                inner.a11y_engine.set_focus(prev_ak);
                next_ak.and_then(|id| NodeId::try_from(id).ok())
            });
            let focus_cbs =
                transition_focus_with_origin(&mut inner, next_nid, FocusOrigin::Keyboard);
            inner.caret_visible = true;
            inner.paint_dirty = true;
            (focus_cbs, next_nid)
        };
        focus_cbs.invoke();
        new_focus
    }

    /// Dispatches a `keydown` event with key name `key` to the currently focused node (or root).
    pub fn dispatch_key_down(&mut self, key: &str) -> bool {
        let cb = {
            let inner = self.inner.borrow();
            let focused_nid = inner
                .a11y_engine
                .focused_id()
                .and_then(|id| NodeId::try_from(id).ok());
            focused_nid
                .filter(|&nid| inner.node(nid).is_some_and(|n| !n.is_disabled()))
                .and_then(|nid| inner.listeners(nid)?.on_keydown.clone())
                .or_else(|| {
                    let rid = *inner.roots.first()?;
                    inner
                        .node(rid)
                        .filter(|n| !n.is_disabled())
                        .and_then(|_| inner.listeners(rid)?.on_keydown.clone())
                })
        };
        if let Some(handler) = cb {
            clear_prevent_default_key();
            let k = key.to_string();
            batch(|| handler(k));
            take_prevent_default_key()
        } else {
            false
        }
    }

    /// Activates (`Enter` / `Space`) the currently focused widget (Section 5).
    pub fn dispatch_key_activate(&mut self) -> bool {
        let (nid, ctrl_act, handler) = {
            let mut inner = self.inner.borrow_mut();
            let Some(ak_id) = inner.a11y_engine.focused_id() else {
                return false;
            };
            let Ok(nid) = NodeId::try_from(ak_id) else {
                return false;
            };
            let act = activate_control_node(&mut inner, nid);
            let click_cb = inner
                .node(nid)
                .filter(|n| !n.is_disabled())
                .and_then(|_| inner.listeners(nid)?.on_click.clone());
            (nid, act, click_cb)
        };
        let mut handled = false;
        if let Some(act) = ctrl_act {
            act.invoke();
            handled = true;
        }
        if let Some(cb) = handler {
            batch(|| cb());
            handled = true;
        }
        handled |= invoke_post_activation_actions(self, nid);
        handled
    }

    /// Handles the `Enter` key.
    pub fn dispatch_key_enter(&mut self) -> bool {
        if self.has_active_ime_preedit() {
            return false;
        }
        let (focused_nid, is_multiline, multiline_cb, is_input_focused, change_cb) =
            prepare_enter_action(&mut self.inner.borrow_mut());
        if is_multiline {
            if let Some((Some(cb), val)) = multiline_cb {
                batch(|| cb(val));
            }
            return true;
        }
        if is_input_focused {
            let mut handled = false;
            if let Some((cb, val)) = change_cb {
                batch(|| cb(val));
                handled = true;
            }
            if let Some(id) = focused_nid {
                let form_act = try_submit_enclosing_form(&mut self.inner.borrow_mut(), id);
                handled |= form_act.handled;
                form_act.invoke();
            }
            handled
        } else {
            self.dispatch_key_activate()
        }
    }

    /// Handles the `Space` key.
    pub fn dispatch_key_space(&mut self) -> bool {
        if self.has_active_ime_preedit() {
            return false;
        }
        let (is_input_focused, is_link_focused) = self
            .with_focused_node(|n| Some((n.role.is_text_editable(), n.role == SemanticRole::Link)))
            .unwrap_or((false, false));
        if is_input_focused {
            self.dispatch_key_text(" ")
        } else if is_link_focused {
            false
        } else {
            self.dispatch_key_activate()
        }
    }

    /// Routes a key event through `onkeydown` and `route_key_with_repeat`.
    pub fn dispatch_routed_key(
        &mut self,
        key_info: &KeyInfo,
        mods: Mods,
        repeat: bool,
    ) -> KeyAction {
        let is_password = self.focused_input_kind() == Some(crate::InputKind::Password);
        if !is_password && self.has_active_ime_preedit() {
            return KeyAction::None;
        }
        let key_name = key_name_for_keydown(key_info);
        if !key_name.is_empty() && self.dispatch_key_down(&key_name) {
            return KeyAction::None;
        }
        if matches!(key_info, KeyInfo::Escape) && self.exit_pointer_lock() {
            return KeyAction::None;
        }
        let is_arrow_consuming_control = self
            .with_focused_node(|n| {
                Some(
                    !n.is_disabled()
                        && (n.role.is_text_editable()
                            || matches!(
                                n.role,
                                SemanticRole::RadioButton
                                    | SemanticRole::Slider
                                    | SemanticRole::ComboBox
                            )),
                )
            })
            .unwrap_or(false);
        let focus_state = FocusState {
            is_input_focused: is_arrow_consuming_control,
            has_ime_preedit: !is_password && self.has_active_ime_preedit(),
            is_ime_enabled: !is_password
                && self.focused_ime_state().map(|s| s.enabled).unwrap_or(false),
        };
        route_key_with_repeat(key_info, mods, &focus_state, repeat)
    }

    /// Cleans up active IME composition, popup overlays, and pointer state when the OS window loses focus.
    pub fn dispatch_window_focus_lost(&mut self) -> bool {
        let finished_ime = self.ime_finish_composing();
        let mut inner = self.inner.borrow_mut();
        let mut changed = finished_ime;
        if close_select_popup(&mut inner) {
            changed = true;
        }
        if let Some(tid) = inner.ime_target.take() {
            if let Some(ed) = inner.sparse.text_edits.get_mut(&tid) {
                ed.ime_state.preedit.clear();
                ed.ime_state.preedit_cursor = None;
                ed.ime_state.pending_blur_commit = None;
            }
            if let Some(node) = inner.node_mut(tid) {
                node.set_text_dirty(true);
            }
            changed = true;
        }
        let old_press = inner.pressed_node.take();
        let hover_cleared =
            crate::view_tree::pointer_capture::set_hovered_node_and_notify(&mut inner, None);
        changed |= inner.interaction.pointer_capture.take().is_some()
            || inner.interaction.aux_button_capture.take().is_some()
            || inner.interaction.pointer_lock.take().is_some()
            || inner.interaction.pointer_inside_captured
            || old_press.is_some()
            || hover_cleared
            || inner.selecting_node.take().is_some()
            || inner.scrollbar_drag.take().is_some();
        inner.interaction.held_buttons = 0;
        if let Some(pid) = old_press {
            crate::view_tree::sync::notify_selector_state_change(&mut inner, pid);
        }
        inner.interaction.pointer_inside_captured = false;
        inner.interaction.selection_granularity = SelectionGranularity::Char;
        inner.last_pointer_pos = None;
        if inner.caret_visible {
            inner.caret_visible = false;
            changed = true;
        }
        if changed {
            inner.paint_dirty = true;
        }
        changed
    }
}

type EnterActionPrep = (
    Option<NodeId>,
    bool,
    Option<(Option<Rc<dyn Fn(String)>>, String)>,
    bool,
    Option<BlurChangeCallback>,
);

fn prepare_enter_action(inner: &mut ViewTreeInner) -> EnterActionPrep {
    let Some(id) = inner
        .a11y_engine
        .focused_id()
        .and_then(|ak| NodeId::try_from(ak).ok())
    else {
        return (None, false, None, false, None);
    };
    let Some((role, is_disabled, cur_text)) = inner
        .node(id)
        .map(|n| (n.role, n.is_disabled(), n.text.clone()))
    else {
        return (None, false, None, false, None);
    };
    if role == SemanticRole::MultilineTextInput && !is_disabled {
        let pair = inner
            .node_edit_mut(id)
            .map(|mut ctx| insert_clean_str_at_cursor(&mut ctx, "\n"));
        sync_node_text_context(inner, id);
        inner.caret_visible = true;
        inner.paint_dirty = true;
        return (Some(id), true, pair, false, None);
    }
    let is_inp = role.is_single_line_input();
    let cb = if is_inp && !is_disabled {
        let changed = inner.sparse.text_edits.get_mut(&id).is_some_and(|ed| {
            if cur_text != ed.committed_value_on_focus {
                ed.committed_value_on_focus = cur_text.clone();
                true
            } else {
                false
            }
        });
        if changed {
            inner
                .listeners(id)
                .and_then(|l| l.on_change.clone())
                .map(|f| (f, cur_text))
        } else {
            None
        }
    } else {
        None
    };
    (Some(id), false, None, is_inp, cb)
}
