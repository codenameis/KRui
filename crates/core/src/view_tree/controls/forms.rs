//! Public `ViewTree` form & control APIs (`<label>`, `checkbox`, `radio`, `range`, `:checked`, `:invalid`).

use crate::view_tree::controls::{
    format_range_value, parse_and_clamp_range_value, select_radio_in_group,
    CHECKABLE_DEFAULT_SIZE_PX, SLIDER_DEFAULT_HEIGHT_PX, SLIDER_DEFAULT_MIN_WIDTH_PX,
};
use crate::view_tree::form_submit::{
    select_display_label, sync_select_selected_option, try_reset_enclosing_form,
    try_submit_enclosing_form, ButtonKind,
};
use crate::view_tree::node::{sync_node_text_context, ClickTarget, InputTarget};
use crate::view_tree::{A11yId, NodeId, PointerEvent, PointerTarget, ViewTree};
use krui_a11y::SemanticRole;
use krui_signals::{batch, Effect};
use krui_style::{sanitize_untrusted_text, Dimension, Edges};
use std::rc::Rc;

impl ViewTree {
    /// Attaches a focus-gained callback (`onfocus`) to `node_id`.
    pub fn set_on_focus(&mut self, node_id: NodeId, callback: impl ClickTarget) {
        let cb = callback.into_click_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_focus = Some(cb);
        }
    }

    /// Attaches a focus-lost callback (`onblur`) to `node_id`.
    pub fn set_on_blur(&mut self, node_id: NodeId, callback: impl ClickTarget) {
        let cb = callback.into_click_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_blur = Some(cb);
        }
    }

    /// Attaches a pointer-down callback (`onpointerdown`, `PointerHandler` or zero-arg closure) to `node_id`.
    pub fn set_on_pointer_down(&mut self, node_id: NodeId, callback: impl PointerTarget) {
        let cb = callback.into_pointer_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_pointer_down = Some(cb);
        }
    }

    /// Attaches a coordinate-receiving pointer-down callback (`Fn(f32, f32)`) to `node_id`.
    pub fn set_on_pointer_down_xy(
        &mut self,
        node_id: NodeId,
        callback: impl Fn(f32, f32) + 'static,
    ) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_pointer_down =
                Some(Rc::new(move |ev: PointerEvent| callback(ev.x, ev.y)));
        }
    }

    /// Attaches a pointer-up callback (`onpointerup`, `PointerHandler` or zero-arg closure) to `node_id`.
    pub fn set_on_pointer_up(&mut self, node_id: NodeId, callback: impl PointerTarget) {
        let cb = callback.into_pointer_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_pointer_up = Some(cb);
        }
    }

    /// Attaches a coordinate-receiving pointer-up callback (`Fn(f32, f32)`) to `node_id`.
    pub fn set_on_pointer_up_xy(&mut self, node_id: NodeId, callback: impl Fn(f32, f32) + 'static) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_pointer_up =
                Some(Rc::new(move |ev: PointerEvent| callback(ev.x, ev.y)));
        }
    }

    /// Attaches a pointer-move callback (`onpointermove`, `PointerHandler` or zero-arg closure) to `node_id`.
    pub fn set_on_pointer_move(&mut self, node_id: NodeId, callback: impl PointerTarget) {
        let cb = callback.into_pointer_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_pointer_move = Some(cb);
        }
    }

    /// Attaches a coordinate-receiving pointer-move callback (`Fn(f32, f32)`) to `node_id`.
    pub fn set_on_pointer_move_xy(
        &mut self,
        node_id: NodeId,
        callback: impl Fn(f32, f32) + 'static,
    ) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_pointer_move =
                Some(Rc::new(move |ev: PointerEvent| callback(ev.x, ev.y)));
        }
    }

    /// Sets the `for="..."` target element ID on a `<label>` node.
    pub fn set_label_for(&mut self, node_id: NodeId, target_id: &str) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_control(node_id).label_for = Some(target_id.trim().to_string());
        }
    }

    /// Sets the `name="..."` control group/form name on `node_id`.
    pub fn set_control_name(&mut self, node_id: NodeId, name: &str) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_control(node_id).control_name = Some(name.trim().to_string());
        }
    }

    /// Configures `node_id` as an `<input type="checkbox">` (`SemanticRole::Checkbox`).
    pub fn set_checkbox_role(&mut self, node_id: NodeId) {
        self.configure_checkable_control(node_id, SemanticRole::Checkbox, 3.0);
    }

    /// Configures `node_id` as an `<input type="radio">` (`SemanticRole::RadioButton`).
    pub fn set_radio_role(&mut self, node_id: NodeId) {
        self.configure_checkable_control(node_id, SemanticRole::RadioButton, 4.0);
    }

    fn configure_checkable_control(
        &mut self,
        node_id: NodeId,
        role: SemanticRole,
        fallback_pad_px: f32,
    ) {
        let mut inner = self.inner.borrow_mut();
        inner
            .a11y_engine
            .register_focusable(A11yId::from(node_id).0);
        inner.sparse.text_edits.remove(&node_id);
        let maybe_layout = inner.node_mut(node_id).and_then(|node| {
            node.role = role;
            if node.style.layout.width == Dimension::Auto {
                node.style.layout.width = Dimension::Px(CHECKABLE_DEFAULT_SIZE_PX);
            }
            if node.style.layout.height == Dimension::Auto {
                node.style.layout.height = Dimension::Px(CHECKABLE_DEFAULT_SIZE_PX);
            }
            if !node.style.paint.appearance_none && !node.style.layout.padding_explicit {
                node.style.layout.padding = Edges::all(fallback_pad_px);
            }
            node.taffy_node.map(|tn| (tn, node.style.layout))
        });
        if let Some((tn, lay)) = maybe_layout {
            crate::view_tree::semantics::set_effective_taffy_layout(&mut inner, node_id, tn, lay);
            sync_node_text_context(&mut inner, node_id);
            inner.paint_dirty = true;
        }
    }

    /// Configures `node_id` as an `<input type="range">` (`SemanticRole::Slider`) with `(min, max, step)`.
    pub fn set_range_bounds(&mut self, node_id: NodeId, min: f32, max: f32, step: f32) {
        let mut inner = self.inner.borrow_mut();
        inner
            .a11y_engine
            .register_focusable(A11yId::from(node_id).0);
        inner.sparse.text_edits.remove(&node_id);
        let bounds = (min, max.max(min), step.max(0.000_001));
        if inner.node(node_id).is_some() {
            inner.ensure_control(node_id).range_bounds = bounds;
        }
        let maybe_layout = inner.node_mut(node_id).and_then(|node| {
            node.role = SemanticRole::Slider;
            node.text = format_range_value(parse_and_clamp_range_value(&node.text, bounds));
            if node.style.layout.width == Dimension::Auto
                && node.style.layout.min_width == Dimension::Auto
            {
                node.style.layout.min_width = Dimension::Px(SLIDER_DEFAULT_MIN_WIDTH_PX);
            }
            if node.style.layout.height == Dimension::Auto
                && node.style.layout.min_height == Dimension::Auto
            {
                node.style.layout.min_height = Dimension::Px(SLIDER_DEFAULT_HEIGHT_PX);
            }
            node.taffy_node.map(|tn| (tn, node.style.layout))
        });
        if let Some((tn, lay)) = maybe_layout {
            crate::view_tree::semantics::set_effective_taffy_layout(&mut inner, node_id, tn, lay);
            sync_node_text_context(&mut inner, node_id);
            inner.paint_dirty = true;
        }
    }

    /// Returns the numeric value of an `<input type="range">` node, or `None` if `node_id` is stale.
    pub fn node_range_value(&self, node_id: NodeId) -> Option<f32> {
        let inner = self.inner.borrow();
        let node = inner.node(node_id)?;
        let bounds = inner
            .control(node_id)
            .map(|c| c.range_bounds)
            .unwrap_or((0.0, 100.0, 1.0));
        Some(parse_and_clamp_range_value(&node.text, bounds))
    }

    /// Sets the `checked` state of `node_id` (enforcing radio-group single selection if `node_id` is a `RadioButton`).
    pub fn set_checked(&mut self, node_id: NodeId, checked: bool) {
        let cbs = {
            let mut inner = self.inner.borrow_mut();
            let Some(role) = inner.node(node_id).map(|n| n.role) else {
                return;
            };
            {
                let ctrl = inner.ensure_control(node_id);
                if ctrl.initial_checked.is_none() {
                    ctrl.initial_checked = Some(checked);
                }
            }
            if role == SemanticRole::RadioButton && checked {
                select_radio_in_group(&mut inner, node_id)
            } else {
                let mut changed = false;
                if let Some(node) = inner.node_mut(node_id) {
                    if node.is_checked() != checked {
                        node.set_checked(checked);
                        inner.paint_dirty = true;
                        changed = true;
                    }
                }
                if changed {
                    crate::view_tree::sync::notify_selector_state_change(&mut inner, node_id);
                }
                Vec::new()
            }
        };
        for (cb, val) in cbs {
            batch(|| cb(val));
        }
    }

    /// Returns `true` if `node_id` is currently `:checked`.
    pub fn is_checked(&self, node_id: NodeId) -> bool {
        self.inner
            .borrow()
            .node(node_id)
            .is_some_and(|n| n.is_checked())
    }

    /// Binds a reactive `checked` predicate to `node_id`.
    pub fn bind_checked(&mut self, node_id: NodeId, pred: impl Fn() -> bool + 'static) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let tree = self.clone();
        let effect = Effect::new(move || {
            let checked = pred();
            let mut t = tree.clone();
            t.set_checked(node_id, checked);
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }

    /// Attaches a `checked`-change callback (`bind:checked`) to `node_id`.
    pub fn set_on_checked_change(&mut self, node_id: NodeId, callback: impl Fn(bool) + 'static) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_control(node_id).on_checked_change = Some(Rc::new(callback));
        }
    }

    /// Sets the `:invalid` validation state of `node_id`.
    pub fn set_invalid(&mut self, node_id: NodeId, invalid: bool) {
        let mut inner = self.inner.borrow_mut();
        let changed = if let Some(node) = inner.node_mut(node_id) {
            if node.is_invalid() != invalid {
                node.set_invalid(invalid);
                inner.paint_dirty = true;
                true
            } else {
                false
            }
        } else {
            false
        };
        if changed {
            crate::view_tree::sync::notify_selector_state_change(&mut inner, node_id);
        }
    }

    /// Returns `true` if `node_id` is currently marked `:invalid`.
    pub fn is_invalid(&self, node_id: NodeId) -> bool {
        self.inner
            .borrow()
            .node(node_id)
            .is_some_and(|n| n.is_invalid())
    }

    /// Attaches an `onchange` handler that receives `"true"`/`"false"` when a checkbox toggles.
    pub fn set_on_checked_input(&mut self, node_id: NodeId, callback: impl InputTarget) {
        self.set_on_input(node_id, callback);
    }

    /// Sets the `required` validation state on `node_id`.
    pub fn set_required(&mut self, node_id: NodeId, required: bool) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_control(node_id).is_required = required;
        }
    }

    /// Returns `true` if `node_id` has `required` enabled.
    pub fn is_required(&self, node_id: NodeId) -> bool {
        let inner = self.inner.borrow();
        inner.node(node_id).is_some() && inner.control(node_id).is_some_and(|c| c.is_required)
    }

    /// Binds a reactive `required` predicate to `node_id`.
    pub fn bind_required(&mut self, node_id: NodeId, pred: impl Fn() -> bool + 'static) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let tree = self.clone();
        let effect = Effect::new(move || {
            let req = pred();
            let mut t = tree.clone();
            t.set_required(node_id, req);
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }

    /// Configures `<button type="button|submit|reset">` behavior on `node_id`.
    pub fn set_button_kind(&mut self, node_id: NodeId, kind: ButtonKind) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_control(node_id).button_kind = kind;
        }
    }

    /// Attaches an `onsubmit` callback to a `<form>` node.
    pub fn set_on_submit(&mut self, node_id: NodeId, callback: impl ClickTarget) {
        let cb = callback.into_click_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_control(node_id).on_submit = Some(cb);
        }
    }

    /// Attaches an `onreset` callback to a `<form>` node.
    pub fn set_on_reset(&mut self, node_id: NodeId, callback: impl ClickTarget) {
        let cb = callback.into_click_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_control(node_id).on_reset = Some(cb);
        }
    }

    /// Validates all controls in the `<form>` enclosing `form_or_child_id` and invokes `onsubmit` if valid.
    pub fn submit_form(&mut self, form_or_child_id: NodeId) -> bool {
        let act = try_submit_enclosing_form(&mut self.inner.borrow_mut(), form_or_child_id);
        let handled = act.handled && act.form_cb.is_some();
        act.invoke();
        handled
    }

    /// Resets all controls in the `<form>` enclosing `form_or_child_id` to their initial values and invokes `onreset`.
    pub fn reset_form(&mut self, form_or_child_id: NodeId) -> bool {
        let act = try_reset_enclosing_form(&mut self.inner.borrow_mut(), form_or_child_id);
        let handled = act.handled;
        act.invoke();
        handled
    }

    /// Appends a `<option value="..." selected>` entry to a `<select>` (`SemanticRole::ComboBox`) node.
    pub fn add_select_option(&mut self, node_id: NodeId, value: &str, label: &str, selected: bool) {
        let mut inner = self.inner.borrow_mut();
        let Some((cur_text, has_dyn)) = inner
            .node(node_id)
            .map(|n| (n.text.clone(), n.has_dynamic_text_binding()))
        else {
            return;
        };
        let clean_val = sanitize_untrusted_text(value);
        let clean_lbl = sanitize_untrusted_text(label);
        let mut assign_text = None;
        {
            let ctrl = inner.ensure_control(node_id);
            let idx = ctrl.select_options.len();
            ctrl.select_options
                .push((clean_val.clone(), clean_lbl, selected));
            if cur_text == clean_val {
                ctrl.selected_option_idx = Some(idx);
            } else if selected || (ctrl.selected_option_idx.is_none() && cur_text.is_empty()) {
                ctrl.selected_option_idx = Some(idx);
                if !has_dyn || cur_text.is_empty() {
                    assign_text = Some(clean_val.clone());
                }
                if selected || ctrl.initial_text.is_none() {
                    ctrl.initial_text = Some(clean_val);
                }
            }
        }
        if let Some(val) = assign_text {
            if let Some(node) = inner.node_mut(node_id) {
                node.text = val.clone();
            }
            if let Some(ed) = inner.sparse.text_edits.get_mut(&node_id) {
                ed.committed_value_on_focus = val;
            }
        }
        if let Some(node) = inner.node_mut(node_id) {
            node.set_text_dirty(true);
        }
        sync_node_text_context(&mut inner, node_id);
        inner.paint_dirty = true;
    }

    /// Replaces the entire `<option>` list on a `<select>` (`SemanticRole::ComboBox`) node.
    pub fn set_select_options(&mut self, node_id: NodeId, options: Vec<(String, String, bool)>) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_none() {
            return;
        }
        inner.ensure_control(node_id).select_options = options
            .into_iter()
            .map(|(v, l, sel)| {
                (
                    sanitize_untrusted_text(&v),
                    sanitize_untrusted_text(&l),
                    sel,
                )
            })
            .collect();
        sync_select_selected_option(&mut inner, node_id);
        if let Some(node) = inner.node_mut(node_id) {
            node.set_text_dirty(true);
        }
        sync_node_text_context(&mut inner, node_id);
        inner.paint_dirty = true;
    }

    /// Reactively binds the `<option>` list of a `<select>` (`SemanticRole::ComboBox`) node.
    pub fn bind_select_options(
        &mut self,
        node_id: NodeId,
        eval_options: impl Fn() -> Vec<(String, String, bool)> + 'static,
    ) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let tree = self.clone();
        let effect = Effect::new(move || {
            let opts = eval_options();
            let mut t = tree.clone();
            t.set_select_options(node_id, opts);
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }

    /// Returns the human-readable `<option>` label for the currently selected value of a `<select>` node.
    pub fn node_selected_label(&self, node_id: NodeId) -> Option<String> {
        let inner = self.inner.borrow();
        let _ = inner.node(node_id)?;
        Some(select_display_label(&inner, node_id))
    }

    /// Returns the `(value, label)` pairs currently mounted on a `<select>` node.
    pub fn select_options(&self, node_id: NodeId) -> Vec<(String, String)> {
        let inner = self.inner.borrow();
        if inner.node(node_id).is_none() {
            return Vec::new();
        }
        inner
            .control(node_id)
            .map(|c| {
                c.select_options
                    .iter()
                    .map(|(v, l, _)| (v.clone(), l.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Sets the static `title` tooltip text on `node_id`.
    pub fn set_title(&mut self, node_id: NodeId, title: &str) {
        let mut inner = self.inner.borrow_mut();
        let clean = sanitize_untrusted_text(title);
        if let Some(node) = inner.node_mut(node_id) {
            if node.aria_label.is_none() && !clean.is_empty() {
                node.aria_label = Some(clean.clone());
            }
            inner.ensure_control(node_id).title = Some(clean);
        }
    }

    /// Returns the current `title` tooltip text on `node_id`, if set.
    pub fn node_title(&self, node_id: NodeId) -> Option<String> {
        let inner = self.inner.borrow();
        let _ = inner.node(node_id)?;
        inner.control(node_id).and_then(|c| c.title.clone())
    }

    /// Binds a reactive `title` tooltip formatting closure directly to `node_id`.
    pub fn bind_dynamic_title(
        &mut self,
        node_id: NodeId,
        formatter: impl Fn() -> String + 'static,
    ) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let tree = self.clone();
        let effect = Effect::new(move || {
            let text = formatter();
            let mut t = tree.clone();
            t.set_title(node_id, &text);
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }
}
