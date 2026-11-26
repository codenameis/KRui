//! Viewport pointer button capture, `oncontextmenu`, `onwheel`, and OS pointer-lock dispatch.

use crate::view_tree::hit_test::{
    effective_node_world_matrix_and_rect, hit_test_hover_node, hit_test_interactive_node,
    node_contains_screen_point,
};
use crate::view_tree::key_routing::{KeydownOutcome, Mods};
use crate::view_tree::pointer_types::{clear_prevent_default_wheel, take_prevent_default_wheel};
pub use crate::view_tree::pointer_types::{
    prevent_default_wheel, PointerButton, PointerEvent, PointerHandler, PointerTarget, WheelEvent,
    WheelHandler, WheelTarget,
};
use crate::view_tree::select_popup::try_handle_select_popup_scroll;
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_signals::batch;
use std::rc::Rc;

type PointerCallback = (Rc<dyn Fn(PointerEvent)>, PointerEvent);
type WheelCallback = (Rc<dyn Fn(WheelEvent) -> bool>, WheelEvent);

pub(crate) fn compute_node_local_coords(
    inner: &ViewTreeInner,
    nid: NodeId,
    x: f32,
    y: f32,
) -> (f32, f32) {
    let (aff, rect) = effective_node_world_matrix_and_rect(inner, nid);
    if let Some((lx, ly)) = aff.inverse().map(|inv| inv.transform_point(x, y)) {
        (lx - rect.x, ly - rect.y)
    } else {
        (x - rect.x, y - rect.y)
    }
}

pub(crate) fn build_pointer_event_for_node(
    inner: &ViewTreeInner,
    nid: NodeId,
    (x, y): (f32, f32),
    delta: (f32, f32),
    button: PointerButton,
    click_count: u8,
    mods: Mods,
) -> PointerEvent {
    let (local_x, local_y) = compute_node_local_coords(inner, nid, x, y);
    PointerEvent {
        x,
        y,
        local_x,
        local_y,
        delta_x: delta.0,
        delta_y: delta.1,
        button,
        buttons: inner.interaction.held_buttons,
        click_count,
        mods,
        pointer_locked: inner.interaction.pointer_lock == Some(nid),
    }
}

impl ViewTree {
    /// Attaches an event-receiving `onpointerdown` handler (`Fn(PointerEvent)`).
    pub fn set_on_pointer_down_event(
        &mut self,
        node_id: NodeId,
        callback: impl Fn(PointerEvent) + 'static,
    ) {
        self.set_on_pointer_down(node_id, PointerHandler::new(callback));
    }

    /// Attaches an event-receiving `onpointerup` handler (`Fn(PointerEvent)`).
    pub fn set_on_pointer_up_event(
        &mut self,
        node_id: NodeId,
        callback: impl Fn(PointerEvent) + 'static,
    ) {
        self.set_on_pointer_up(node_id, PointerHandler::new(callback));
    }

    /// Attaches an event-receiving `onpointermove` handler (`Fn(PointerEvent)`).
    pub fn set_on_pointer_move_event(
        &mut self,
        node_id: NodeId,
        callback: impl Fn(PointerEvent) + 'static,
    ) {
        self.set_on_pointer_move(node_id, PointerHandler::new(callback));
    }

    /// Attaches a `oncontextmenu` callback (`PointerHandler` or zero-arg closure/`Command`) to `node_id`.
    pub fn set_on_context_menu(&mut self, node_id: NodeId, callback: impl PointerTarget) {
        let cb = callback.into_pointer_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_context_menu = Some(cb);
        }
    }

    /// Attaches an event-receiving `oncontextmenu` handler (`Fn(PointerEvent)`).
    pub fn set_on_context_menu_event(
        &mut self,
        node_id: NodeId,
        callback: impl Fn(PointerEvent) + 'static,
    ) {
        self.set_on_context_menu(node_id, PointerHandler::new(callback));
    }

    /// Attaches an `onwheel` callback (`WheelHandler` or zero-arg closure/`Command`) to `node_id`.
    pub fn set_on_wheel(&mut self, node_id: NodeId, callback: impl WheelTarget) {
        let cb = callback.into_wheel_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_wheel = Some(cb);
        }
    }

    /// Attaches an event-receiving `onwheel` handler (`Fn(WheelEvent) -> R`).
    pub fn set_on_wheel_event<F, R>(&mut self, node_id: NodeId, callback: F)
    where
        F: Fn(WheelEvent) -> R + 'static,
        R: KeydownOutcome + 'static,
    {
        self.set_on_wheel(node_id, WheelHandler::new(callback));
    }

    /// Requests OS pointer lock routing relative motion deltas exclusively to `node_id`.
    pub fn request_pointer_lock(&mut self, node_id: NodeId) -> bool {
        let mut inner = self.inner.borrow_mut();
        if inner
            .node(node_id)
            .is_none_or(|n| n.is_anchor() || n.is_disabled())
        {
            return false;
        }
        inner.interaction.pointer_lock = Some(node_id);
        true
    }

    /// Releases any active OS pointer lock.
    pub fn exit_pointer_lock(&mut self) -> bool {
        self.inner
            .borrow_mut()
            .interaction
            .pointer_lock
            .take()
            .is_some()
    }

    /// Returns the node currently holding pointer lock, if any.
    #[must_use]
    pub fn pointer_lock_node(&self) -> Option<NodeId> {
        self.inner.borrow().interaction.pointer_lock
    }

    /// Dispatches a mouse button transition (`Primary`/`Middle`/`Secondary`/`Back`/`Forward`) with modifiers.
    pub fn dispatch_pointer_button(
        &mut self,
        x: f32,
        y: f32,
        button: PointerButton,
        pressed: bool,
        mods: Mods,
        timestamp_ms: u64,
    ) -> bool {
        {
            let mut inner = self.inner.borrow_mut();
            if pressed {
                inner.interaction.held_buttons |= button.mask();
            } else {
                inner.interaction.held_buttons &= !button.mask();
            }
        }
        if button == PointerButton::Primary {
            return if pressed {
                crate::view_tree::pointer_capture::run_pointer_down(
                    self,
                    x,
                    y,
                    mods,
                    Some(timestamp_ms),
                )
            } else {
                self.dispatch_pointer_up_with_mods(x, y, mods)
            };
        }
        if pressed {
            self.dispatch_aux_button_down(x, y, button, mods)
        } else {
            self.dispatch_aux_button_up(x, y, button, mods)
        }
    }

    fn dispatch_aux_button_down(
        &mut self,
        x: f32,
        y: f32,
        button: PointerButton,
        mods: Mods,
    ) -> bool {
        let prep = {
            let mut inner = self.inner.borrow_mut();
            inner.last_pointer_pos = Some((x, y));
            if inner.stylo_dirty {
                crate::view_tree::sync::flush_pending_restyle(&mut inner);
            }
            let target = inner
                .interaction
                .pointer_lock
                .filter(|&id| inner.node(id).is_some_and(|n| !n.is_disabled()))
                .or_else(|| hit_test_interactive_node(&inner, x, y));
            target.and_then(|nid| {
                if inner.interaction.aux_button_capture.is_none() {
                    inner.interaction.aux_button_capture = Some((nid, button));
                }
                let ev =
                    build_pointer_event_for_node(&inner, nid, (x, y), (0.0, 0.0), button, 1, mods);
                inner
                    .node(nid)
                    .is_some_and(|n| !n.is_disabled())
                    .then(|| inner.listeners(nid).and_then(|l| l.on_pointer_down.clone()))
                    .flatten()
                    .map(|cb| (cb, ev))
            })
        };
        if let Some((cb, ev)) = prep {
            batch(|| cb(ev));
            true
        } else {
            false
        }
    }

    fn dispatch_aux_button_up(
        &mut self,
        x: f32,
        y: f32,
        button: PointerButton,
        mods: Mods,
    ) -> bool {
        let (up_cb, ctx_cb) = {
            let mut inner = self.inner.borrow_mut();
            inner.last_pointer_pos = Some((x, y));
            let captured = inner
                .interaction
                .aux_button_capture
                .filter(|(_, b)| *b == button)
                .map(|(nid, _)| nid);
            if captured.is_some() {
                inner.interaction.aux_button_capture = None;
            }
            let release_hit = hit_test_interactive_node(&inner, x, y);
            let target = inner
                .interaction
                .pointer_lock
                .filter(|&id| inner.node(id).is_some_and(|n| !n.is_disabled()))
                .or(captured)
                .or(release_hit);
            let up_cb = target
                .filter(|&id| inner.node(id).is_some_and(|n| !n.is_disabled()))
                .and_then(|id| {
                    let ev = build_pointer_event_for_node(
                        &inner,
                        id,
                        (x, y),
                        (0.0, 0.0),
                        button,
                        1,
                        mods,
                    );
                    inner
                        .listeners(id)
                        .and_then(|l| l.on_pointer_up.clone())
                        .map(|cb| (cb, ev))
                });
            let ctx_cb =
                if button == PointerButton::Secondary && inner.interaction.pointer_lock.is_none() {
                    let ctx_start = captured
                        .filter(|&id| node_contains_screen_point(&inner, id, x, y))
                        .or(release_hit);
                    find_context_menu_handler(&inner, ctx_start, (x, y), mods)
                } else {
                    None
                };
            (up_cb, ctx_cb)
        };
        let mut handled = false;
        if let Some((cb, ev)) = up_cb {
            batch(|| cb(ev));
            handled = true;
        }
        if let Some((cb, ev)) = ctx_cb {
            batch(|| cb(ev));
            handled = true;
        }
        handled
    }

    /// Dispatches pointer movement at `(x, y)` with keyboard modifiers and deltas.
    pub fn dispatch_pointer_move_with_mods(&mut self, x: f32, y: f32, mods: Mods) -> bool {
        let (ptr_move_cb, hover_changed) = {
            let mut inner = self.inner.borrow_mut();
            let prev = inner.last_pointer_pos.unwrap_or((x, y));
            let delta = (x - prev.0, y - prev.1);
            inner.last_pointer_pos = Some((x, y));
            if inner.stylo_dirty {
                crate::view_tree::sync::flush_pending_restyle(&mut inner);
            }
            if let Some(cap_id) = inner
                .interaction
                .pointer_capture
                .or_else(|| inner.interaction.aux_button_capture.map(|(nid, _)| nid))
            {
                inner.apply_captured_pointer_move_opts(cap_id, x, y, delta, mods)
            } else if let Some(popup_changed) =
                crate::view_tree::select_popup::try_handle_select_popup_pointer_move(
                    &mut inner, x, y,
                )
            {
                return popup_changed;
            } else {
                dispatch_unconstrained_pointer_move(&mut inner, (x, y), delta, mods)
            }
        };
        if let Some((cb, ev)) = ptr_move_cb {
            batch(|| cb(ev));
            true
        } else {
            hover_changed
        }
    }

    /// Delivers raw relative pointer deltas `(dx, dy)` exclusively to the active pointer-locked node.
    pub fn dispatch_relative_pointer_motion(&mut self, dx: f32, dy: f32, mods: Mods) -> bool {
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        let prep = {
            let inner = self.inner.borrow();
            let Some(lock_id) = inner
                .interaction
                .pointer_lock
                .filter(|&id| inner.node(id).is_some_and(|n| !n.is_disabled()))
            else {
                return false;
            };
            let (px, py) = inner.last_pointer_pos.unwrap_or((0.0, 0.0));
            let (local_x, local_y) = compute_node_local_coords(&inner, lock_id, px, py);
            let ev = PointerEvent {
                x: px,
                y: py,
                local_x,
                local_y,
                delta_x: dx,
                delta_y: dy,
                button: PointerButton::Primary,
                buttons: inner.interaction.held_buttons,
                click_count: 0,
                mods,
                pointer_locked: true,
            };
            inner
                .listeners(lock_id)
                .and_then(|l| l.on_pointer_move.clone())
                .map(|cb| (cb, ev))
        };
        if let Some((cb, ev)) = prep {
            batch(|| cb(ev));
            true
        } else {
            false
        }
    }

    /// Dispatches wheel input (`delta_x`, `delta_y`, `mods`) through `onwheel` listeners before ancestor scrolling.
    pub fn dispatch_wheel(
        &mut self,
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
        mods: Mods,
    ) -> bool {
        self.inner.borrow_mut().touch.cancel_fling();
        if let Some(popup_handled) =
            try_handle_select_popup_scroll(&mut self.inner.borrow_mut(), x, y, delta_y)
        {
            return popup_handled;
        }
        let chain = {
            let inner = self.inner.borrow();
            let start = crate::view_tree::hit_test::hit_test_any_node(&inner, x, y);
            collect_wheel_handlers(&inner, start, (x, y), (delta_x, delta_y), mods)
        };
        for (cb, ev) in chain {
            clear_prevent_default_wheel();
            let ret = batch(|| cb(ev));
            if ret || take_prevent_default_wheel() {
                return true;
            }
        }
        self.dispatch_scroll_internal(x, y, delta_y)
    }
}

fn dispatch_unconstrained_pointer_move(
    inner: &mut ViewTreeInner,
    (x, y): (f32, f32),
    delta: (f32, f32),
    mods: Mods,
) -> (Option<PointerCallback>, bool) {
    let new_hover = hit_test_hover_node(inner, x, y);
    let hover_changed =
        if crate::view_tree::pointer_capture::set_hovered_node_and_notify(inner, new_hover) {
            inner.paint_dirty = true;
            true
        } else {
            false
        };
    crate::view_tree::pointer_vars::update_hover_pointer_coords(inner, new_hover, x, y);
    let ptr_move_cb = new_hover
        .filter(|&nid| inner.node(nid).is_some_and(|n| !n.is_disabled()))
        .and_then(|nid| {
            let ev = build_pointer_event_for_node(
                inner,
                nid,
                (x, y),
                delta,
                PointerButton::Primary,
                0,
                mods,
            );
            inner
                .listeners(nid)
                .and_then(|l| l.on_pointer_move.clone())
                .map(|cb| (cb, ev))
        });
    (ptr_move_cb, hover_changed)
}

fn find_context_menu_handler(
    inner: &ViewTreeInner,
    start: Option<NodeId>,
    (x, y): (f32, f32),
    mods: Mods,
) -> Option<PointerCallback> {
    let mut cur = start;
    while let Some(nid) = cur {
        let Some(node) = inner.node(nid) else { break };
        if !node.is_disabled() {
            if let Some(cb) = inner.listeners(nid).and_then(|l| l.on_context_menu.clone()) {
                let ev = build_pointer_event_for_node(
                    inner,
                    nid,
                    (x, y),
                    (0.0, 0.0),
                    PointerButton::Secondary,
                    1,
                    mods,
                );
                return Some((cb, ev));
            }
        }
        cur = node.parent;
    }
    None
}

fn collect_wheel_handlers(
    inner: &ViewTreeInner,
    start: Option<NodeId>,
    (x, y): (f32, f32),
    (delta_x, delta_y): (f32, f32),
    mods: Mods,
) -> Vec<WheelCallback> {
    let mut out = Vec::new();
    let mut cur = start;
    while let Some(nid) = cur {
        let Some(node) = inner.node(nid) else { break };
        if !node.is_disabled() {
            if let Some(cb) = inner.listeners(nid).and_then(|l| l.on_wheel.clone()) {
                let (local_x, local_y) = compute_node_local_coords(inner, nid, x, y);
                out.push((
                    cb,
                    WheelEvent {
                        x,
                        y,
                        local_x,
                        local_y,
                        delta_x,
                        delta_y,
                        mods,
                    },
                ));
            }
        }
        cur = node.parent;
    }
    out
}
