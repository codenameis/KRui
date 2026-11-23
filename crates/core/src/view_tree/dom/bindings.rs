//! Reactive slot bindings (`bind_paint<T>`, `bind_layout_*`, `bind_dynamic_text`, click/input/canvas callbacks).

use crate::scene::{CanvasContext2D, CanvasDraw, CanvasRedrawMode, Size};
use crate::view_tree::focus_pointer::apply_disabled_state_change;
use crate::view_tree::grapheme::{
    apply_bound_text_update, sanitize_multiline_text, sanitize_single_line_text,
};
use crate::view_tree::node::{sync_node_text_context, ClickTarget, InputTarget};
use crate::view_tree::{A11yId, ClickBinding, NodeId, ViewTree};
use krui_a11y::SemanticRole;
use krui_signals::{Effect, FastPathTransform, Signal};
use krui_style::{sanitize_untrusted_text, Dimension, Edges, LayoutStyle, PaintStyle, Transform2D};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

fn is_form_value_role(role: SemanticRole) -> bool {
    role.is_text_editable()
        || matches!(
            role,
            SemanticRole::Checkbox
                | SemanticRole::RadioButton
                | SemanticRole::Slider
                | SemanticRole::ComboBox
        )
}

impl ViewTree {
    /// Sets static text on `node_id` (sanitizing BiDi overrides per Section 6).
    pub fn set_static_text(&mut self, node_id: NodeId, text: &str) {
        let mut inner = self.inner.borrow_mut();
        let Some(role) = inner.node(node_id).map(|n| n.role) else {
            return;
        };
        let bounds = inner
            .control(node_id)
            .map(|c| c.range_bounds)
            .unwrap_or((0.0, 100.0, 1.0));
        let clean = if role.is_single_line_input() {
            sanitize_single_line_text(text)
        } else if role == SemanticRole::MultilineTextInput {
            sanitize_multiline_text(text)
        } else if role == SemanticRole::Slider {
            crate::view_tree::controls::format_range_value(
                crate::view_tree::controls::parse_and_clamp_range_value(text, bounds),
            )
        } else {
            sanitize_untrusted_text(text)
        };
        if is_form_value_role(role) {
            let ctrl = inner.ensure_control(node_id);
            if ctrl.initial_text.is_none() {
                ctrl.initial_text = Some(clean.clone());
            }
            if role == SemanticRole::ComboBox {
                ctrl.selected_option_idx =
                    ctrl.select_options.iter().position(|(v, _, _)| *v == clean);
            }
        }
        let char_len = clean.chars().count();
        if role.is_text_editable() || inner.sparse.text_edits.contains_key(&node_id) {
            let ed = inner.ensure_text_edit(node_id);
            ed.committed_value_on_focus = clean.clone();
            ed.cursor_char_idx = char_len;
            ed.selection_anchor_char_idx = None;
            ed.preferred_x = None;
            ed.needs_caret_scroll = true;
            ed.undo_stack.clear();
            ed.redo_stack.clear();
        }
        if let Some(node) = inner.node_mut(node_id) {
            node.text = clean;
            node.set_text_dirty(true);
        }
        crate::view_tree::sync::sync_node_text_to_stylo(&mut inner, node_id);
        crate::view_tree::sync::notify_selector_state_change(&mut inner, node_id);
        sync_node_text_context(&mut inner, node_id);
        inner.paint_dirty = true;
    }

    /// Binds a reactive text formatting closure directly to `node_id`.
    pub fn bind_dynamic_text(&mut self, node_id: NodeId, formatter: impl Fn() -> String + 'static) {
        self.bind_dynamic_text_rc(node_id, Rc::new(formatter));
    }

    fn bind_dynamic_text_rc(&mut self, node_id: NodeId, formatter: Rc<dyn Fn() -> String>) {
        {
            let mut inner = self.inner.borrow_mut();
            let Some(node) = inner.node_mut(node_id) else {
                return;
            };
            node.set_has_dynamic_text_binding(true);
        }
        let tree_weak = Rc::downgrade(&self.inner);
        let effect = Effect::new(move || {
            let raw_text = formatter();
            if let Some(rc) = tree_weak.upgrade() {
                let mut inner = rc.borrow_mut();
                let is_focused = inner.a11y_engine.focused_id() == Some(A11yId::from(node_id).0);
                if apply_bound_text_update(&mut inner, node_id, &raw_text, is_focused) {
                    crate::view_tree::sync::sync_node_text_to_stylo(&mut inner, node_id);
                    sync_node_text_context(&mut inner, node_id);
                    crate::view_tree::sync::notify_selector_state_change(&mut inner, node_id);
                    inner.paint_dirty = true;
                }
            }
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }

    /// Sets a static `placeholder` string on an `<input>` node.
    pub fn set_placeholder(&mut self, node_id: NodeId, placeholder: &str) {
        let mut inner = self.inner.borrow_mut();
        if let Some(node) = inner.node_mut(node_id) {
            node.set_text_dirty(true);
            inner.ensure_text_edit(node_id).placeholder = sanitize_untrusted_text(placeholder);
            sync_node_text_context(&mut inner, node_id);
            crate::view_tree::sync::notify_selector_state_change(&mut inner, node_id);
            inner.paint_dirty = true;
        }
    }

    /// Returns the current `placeholder` string on `node_id`, or `None` if `node_id` is stale.
    pub fn node_placeholder(&self, node_id: NodeId) -> Option<String> {
        let inner = self.inner.borrow();
        let _ = inner.node(node_id)?;
        Some(
            inner
                .text_edit(node_id)
                .map(|e| e.placeholder.clone())
                .unwrap_or_default(),
        )
    }

    /// Binds a reactive `placeholder` formatting closure to an `<input>` node.
    pub fn bind_dynamic_placeholder(
        &mut self,
        node_id: NodeId,
        formatter: impl Fn() -> String + 'static,
    ) {
        self.bind_dynamic_placeholder_rc(node_id, Rc::new(formatter));
    }

    fn bind_dynamic_placeholder_rc(&mut self, node_id: NodeId, formatter: Rc<dyn Fn() -> String>) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let tree_weak = Rc::downgrade(&self.inner);
        let effect = Effect::new(move || {
            let new_ph = sanitize_untrusted_text(&formatter());
            if let Some(rc) = tree_weak.upgrade() {
                let mut inner = rc.borrow_mut();
                if inner.node(node_id).is_none() {
                    return;
                }
                let changed = {
                    let ed = inner.ensure_text_edit(node_id);
                    if ed.placeholder != new_ph {
                        ed.placeholder = new_ph;
                        true
                    } else {
                        false
                    }
                };
                if changed {
                    if let Some(node) = inner.node_mut(node_id) {
                        node.set_text_dirty(true);
                    }
                    sync_node_text_context(&mut inner, node_id);
                    crate::view_tree::sync::notify_selector_state_change(&mut inner, node_id);
                    inner.paint_dirty = true;
                }
            }
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }

    /// Sets the static `disabled` state of `node_id`.
    pub fn set_disabled(&mut self, node_id: NodeId, disabled: bool) {
        let focus_cbs = {
            let mut inner = self.inner.borrow_mut();
            apply_disabled_state_change(&mut inner, node_id, disabled)
        };
        if let Some(cbs) = focus_cbs {
            cbs.invoke();
        }
    }

    /// Binds a reactive `disabled` predicate to `node_id`.
    pub fn bind_disabled(&mut self, node_id: NodeId, pred: impl Fn() -> bool + 'static) {
        self.bind_disabled_rc(node_id, Rc::new(pred));
    }

    fn bind_disabled_rc(&mut self, node_id: NodeId, pred: Rc<dyn Fn() -> bool>) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let tree_weak = Rc::downgrade(&self.inner);
        let effect = Effect::new(move || {
            let disabled = pred();
            let focus_cbs = tree_weak.upgrade().and_then(|rc| {
                let mut inner = rc.borrow_mut();
                apply_disabled_state_change(&mut inner, node_id, disabled)
            });
            if let Some(cbs) = focus_cbs {
                cbs.invoke();
            }
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }

    /// Generic compositor fast-path binding that updates a `PaintStyle` slot without triggering Taffy layout.
    pub fn bind_paint<T: Copy + 'static>(
        &mut self,
        node_id: NodeId,
        sig: Signal<T>,
        apply: fn(&mut PaintStyle, T),
    ) {
        self.bind_paint_eval(node_id, move || sig.get(), apply);
    }

    /// Closure-based compositor fast-path binding that updates a `PaintStyle` slot without triggering Taffy layout.
    pub fn bind_paint_eval<T: Copy + 'static>(
        &mut self,
        node_id: NodeId,
        eval: impl Fn() -> T + 'static,
        apply: fn(&mut PaintStyle, T),
    ) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let tree_weak = Rc::downgrade(&self.inner);
        let last_val = Rc::new(Cell::new(None::<T>));
        let last_reapply = Rc::clone(&last_val);
        self.inner
            .borrow_mut()
            .sparse
            .paint_slot_reappliers
            .entry(node_id)
            .or_default()
            .push(Rc::new(move |paint| {
                if let Some(v) = last_reapply.get() {
                    apply(paint, v);
                }
            }));
        let effect = Effect::new(move || {
            let v = eval();
            last_val.set(Some(v));
            if let Some(rc) = tree_weak.upgrade() {
                let mut inner = rc.borrow_mut();
                if let Some(node) = inner.node_mut(node_id) {
                    apply(&mut node.style.paint, v);
                    inner.paint_dirty = true;
                }
            }
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }

    /// Binds `transform.translate_x` to `sig` on the compositor fast-path.
    pub fn bind_paint_translate_x(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_paint(node_id, sig, |p, v| p.transform.translate_x = v);
    }

    /// Binds `transform.translate_y` to `sig` on the compositor fast-path.
    pub fn bind_paint_translate_y(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_paint(node_id, sig, |p, v| p.transform.translate_y = v);
    }

    /// Binds `transform.scale_x` to `sig` on the compositor fast-path.
    pub fn bind_paint_scale_x(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_paint(node_id, sig, |p, v| p.transform.scale_x = v);
    }

    /// Binds `transform.scale_y` to `sig` on the compositor fast-path.
    pub fn bind_paint_scale_y(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_paint(node_id, sig, |p, v| p.transform.scale_y = v);
    }

    /// Binds uniform `transform.scale_x` and `transform.scale_y` to `sig` on the compositor fast-path.
    pub fn bind_paint_scale_uniform(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_paint(node_id, sig, |p, v| {
            p.transform.scale_x = v;
            p.transform.scale_y = v;
        });
    }

    /// Binds `transform.rotate_deg` to `sig` on the compositor fast-path.
    pub fn bind_paint_rotate_deg(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_paint(node_id, sig, |p, v| p.transform.rotate_deg = v);
    }

    /// Binds `opacity` (`0.0..=1.0`) to `sig` on the compositor fast-path.
    pub fn bind_paint_opacity(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_paint(node_id, sig, |p, v| p.opacity = v.clamp(0.0, 1.0));
    }

    /// Binds a full `FastPathTransform` to `sig` on the compositor fast-path.
    pub fn bind_paint_fast_transform(&mut self, node_id: NodeId, sig: Signal<FastPathTransform>) {
        self.bind_paint(node_id, sig, |p, ft| p.transform = Transform2D::from(ft));
    }

    /// Binds `width` in pixels to `sig`.
    pub fn bind_layout_width_px(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_layout_mutation(node_id, sig, |l, v| l.width = Dimension::Px(v));
    }

    /// Binds `height` in pixels to `sig`.
    pub fn bind_layout_height_px(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_layout_mutation(node_id, sig, |l, v| l.height = Dimension::Px(v));
    }

    /// Binds `top` inset in pixels to `sig`.
    pub fn bind_layout_top_px(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_layout_mutation(node_id, sig, |l, v| l.inset.top = Dimension::Px(v));
    }

    /// Binds `left` inset in pixels to `sig`.
    pub fn bind_layout_left_px(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_layout_mutation(node_id, sig, |l, v| l.inset.left = Dimension::Px(v));
    }

    /// Binds `right` inset in pixels to `sig`.
    pub fn bind_layout_right_px(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_layout_mutation(node_id, sig, |l, v| l.inset.right = Dimension::Px(v));
    }

    /// Binds `bottom` inset in pixels to `sig`.
    pub fn bind_layout_bottom_px(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_layout_mutation(node_id, sig, |l, v| l.inset.bottom = Dimension::Px(v));
    }

    /// Binds uniform `margin` in pixels to `sig`.
    pub fn bind_layout_margin_px(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_layout_mutation(node_id, sig, |l, v| l.margin = Edges::all(v));
    }

    /// Binds uniform `padding` in pixels to `sig`.
    pub fn bind_layout_padding_px(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_layout_mutation(node_id, sig, |l, v| l.padding = Edges::all(v));
    }

    /// Binds flex `gap` in pixels to `sig`.
    pub fn bind_layout_gap_px(&mut self, node_id: NodeId, sig: Signal<f32>) {
        self.bind_layout_mutation(node_id, sig, |l, v| l.gap = v);
    }

    fn bind_layout_mutation(
        &mut self,
        node_id: NodeId,
        sig: Signal<f32>,
        mutate: fn(&mut LayoutStyle, f32),
    ) {
        self.bind_layout_eval(node_id, move || sig.get(), mutate);
    }

    /// Closure-based reactive layout slot binding that updates `LayoutStyle` and marks the Taffy node dirty.
    pub fn bind_layout_eval(
        &mut self,
        node_id: NodeId,
        eval: impl Fn() -> f32 + 'static,
        mutate: fn(&mut LayoutStyle, f32),
    ) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let tree_weak = Rc::downgrade(&self.inner);
        let effect = Effect::new(move || {
            let v = eval();
            if let Some(rc) = tree_weak.upgrade() {
                let mut inner = rc.borrow_mut();
                inner.sparse.layout_slot_nodes.insert(node_id);
                let maybe_update = inner.node_mut(node_id).map(|node| {
                    mutate(&mut node.style.layout, v);
                    (
                        node.style.paint.transition,
                        node.style.layout,
                        node.style.text.font_size,
                        node.taffy_node,
                    )
                });
                if let Some((spec, raw_lay, raw_fs, taffy_opt)) = maybe_update {
                    let eff = crate::view_tree::transitions_layout::commit_node_layout_transition(
                        &mut inner, node_id, spec, raw_lay, raw_fs,
                    );
                    if let Some((eff_lay, eff_fs)) = eff {
                        crate::view_tree::transitions_layout::apply_effective_node_layout_to_taffy(
                            &mut inner, node_id, eff_lay, eff_fs,
                        );
                    } else if let Some(taffy_node) = taffy_opt {
                        crate::view_tree::semantics::set_effective_taffy_layout(
                            &mut inner, node_id, taffy_node, raw_lay,
                        );
                    }
                    inner.paint_dirty = true;
                }
            }
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }

    /// Attaches a click callback or [`ClickTarget`] to `node_id`.
    pub fn set_on_click(&mut self, node_id: NodeId, callback: impl ClickTarget) {
        let cb: Rc<dyn Fn()> = callback.into_click_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_click = Some(cb);
        }
    }

    /// Binds a [`ClickBinding`] (such as an MVVM `Command` with optional `can_execute` `:disabled`
    /// synchronization or a click callback) to `node_id`.
    pub fn bind_click(&mut self, node_id: NodeId, binding: impl Into<ClickBinding>) {
        let b: ClickBinding = binding.into();
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        if let Some(can_exec) = b.can_execute {
            let tree_weak = Rc::downgrade(&self.inner);
            let effect = Effect::new(move || {
                let enabled = can_exec.get();
                let focus_cbs = tree_weak.upgrade().and_then(|rc| {
                    let mut inner = rc.borrow_mut();
                    apply_disabled_state_change(&mut inner, node_id, !enabled)
                });
                if let Some(cbs) = focus_cbs {
                    cbs.invoke();
                }
            });
            self.inner.borrow_mut().push_effect(node_id, effect);
        }
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_click = Some(b.action);
        }
    }

    /// Attaches a text input change callback (`oninput`) or [`InputTarget`] to an `<input>` node.
    pub fn set_on_input(&mut self, node_id: NodeId, callback: impl InputTarget) {
        let cb: Rc<dyn Fn(String)> = callback.into_input_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_input = Some(cb);
        }
    }

    /// Attaches a committed value change callback (`onchange`) to an `<input>` node.
    pub fn set_on_change(&mut self, node_id: NodeId, callback: impl InputTarget) {
        let cb: Rc<dyn Fn(String)> = callback.into_input_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_change = Some(cb);
        }
    }

    /// Attaches a keydown callback (`onkeydown`) to `node_id`.
    pub fn set_on_keydown(&mut self, node_id: NodeId, callback: impl InputTarget) {
        let cb: Rc<dyn Fn(String)> = callback.into_input_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_keydown = Some(cb);
        }
    }

    /// Attaches a reactive `<canvas>` 2D drawing callback (`CanvasDraw`) to `node_id`.
    pub fn set_canvas_draw(&mut self, node_id: NodeId, draw_cb: impl Into<CanvasDraw>) {
        self.set_canvas_draw_with_mode(node_id, CanvasRedrawMode::Reactive, draw_cb);
    }

    /// Attaches a `<canvas>` 2D drawing callback with an explicit `CanvasRedrawMode`.
    pub fn set_canvas_draw_with_mode(
        &mut self,
        node_id: NodeId,
        mode: CanvasRedrawMode,
        draw_cb: impl Into<CanvasDraw>,
    ) {
        let draw_cb: CanvasDraw = draw_cb.into();
        {
            let mut inner = self.inner.borrow_mut();
            let Some(node) = inner.node_mut(node_id) else {
                return;
            };
            node.role = SemanticRole::Canvas;
            let cv = inner.ensure_canvas(node_id);
            cv.canvas_mode = mode;
            cv.canvas_callback = Some(draw_cb.clone());
            cv.canvas_dirty = true;
        }
        if mode == CanvasRedrawMode::Reactive {
            let tree_weak = Rc::downgrade(&self.inner);
            let effect = Effect::new(move || {
                let size_opt = tree_weak
                    .upgrade()
                    .and_then(|rc| {
                        let inner = rc.borrow();
                        inner
                            .node(node_id)
                            .map(|n| (n.world_rect.width, n.world_rect.height))
                    })
                    .filter(|&(w, h)| w > 0.0 && h > 0.0)
                    .map(|(w, h)| Size::new(w, h));
                let probe_size = size_opt.unwrap_or(Size::new(440.0, 260.0));
                let mut ctx = CanvasContext2D::with_size(probe_size.width, probe_size.height);
                draw_cb.draw(&mut ctx, probe_size);
                if let Some(rc) = tree_weak.upgrade() {
                    let mut inner = rc.borrow_mut();
                    if inner.node(node_id).is_some() {
                        let cv = inner.ensure_canvas(node_id);
                        cv.canvas_commands = Arc::from(ctx.commands().to_vec());
                        cv.canvas_dirty = size_opt.is_none();
                        inner.paint_dirty = true;
                    }
                }
            });
            self.inner.borrow_mut().push_effect(node_id, effect);
        }
    }
}
