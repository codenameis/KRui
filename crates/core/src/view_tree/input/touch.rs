//! Touch gesture recognition (slop threshold vs tap, drag scrolling, and inertial fling).

use crate::view_tree::ViewTree;
use krui_signals::{FrameClock, Scope};
use std::cell::RefCell;
use std::rc::Rc;

/// Movement distance threshold (logical px) distinguishing a tap from a touch drag scroll.
pub const TOUCH_SLOP_PX: f32 = 8.0;

const MIN_FLING_SPEED_PX_PER_SEC: f32 = 40.0;
const STOP_FLING_SPEED_PX_PER_SEC: f32 = 8.0;
const MAX_FLING_SPEED_PX_PER_SEC: f32 = 6000.0;
const FLING_DECAY_PER_SEC: f32 = 6.0;
const VELOCITY_EMA_ALPHA: f32 = 0.6;
const MAX_FLING_DT_SECONDS: f32 = 0.05;

/// Platform-neutral touch contact lifecycle phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPhaseKind {
    /// Finger touched down on the surface.
    Started,
    /// Finger moved while in contact with the surface.
    Moved,
    /// Finger lifted cleanly from the surface.
    Ended,
    /// OS cancelled the gesture (e.g., palm rejection or window blur).
    Cancelled,
}

struct FlingState {
    active: bool,
    x: f32,
    y: f32,
    velocity_y: f32,
}

struct ActiveFling {
    scope: Scope,
    state: Rc<RefCell<FlingState>>,
}

/// Per-`ViewTree` single-pointer touch drag & inertial fling tracker.
#[derive(Default)]
pub(crate) struct TouchTracker {
    active_pointer: Option<u64>,
    start_pos: (f32, f32),
    last_pos: (f32, f32),
    scrolled: bool,
    velocity_y: f32,
    fling: Option<ActiveFling>,
}

impl Drop for TouchTracker {
    fn drop(&mut self) {
        self.cancel_fling();
    }
}

impl TouchTracker {
    /// Cancels and disposes any currently running inertial fling animation scope.
    pub(crate) fn cancel_fling(&mut self) {
        if let Some(active) = self.fling.take() {
            if let Ok(mut st) = active.state.try_borrow_mut() {
                st.active = false;
                st.velocity_y = 0.0;
            }
            active.scope.dispose();
        }
    }

    /// Clears active contact tracking and cancels any inertial fling (e.g. page transition / remount).
    pub(crate) fn reset(&mut self) {
        self.active_pointer = None;
        self.scrolled = false;
        self.velocity_y = 0.0;
        self.cancel_fling();
    }

    fn clear_matching_fling(&mut self, target: &Rc<RefCell<FlingState>>) {
        let is_match = self
            .fling
            .as_ref()
            .is_some_and(|active| Rc::ptr_eq(&active.state, target));
        if is_match {
            self.fling = None;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TouchDirectDrag {
    None,
    PointerDrag,
    CapturedMove,
}

fn classify_touch_direct_drag(inner: &crate::view_tree::ViewTreeInner) -> TouchDirectDrag {
    if inner.scrollbar_drag.is_some() {
        return TouchDirectDrag::PointerDrag;
    }
    if inner.selecting_node.is_some_and(|nid| {
        inner
            .node(nid)
            .is_some_and(|n| n.role == krui_a11y::SemanticRole::Slider)
    }) {
        return TouchDirectDrag::PointerDrag;
    }
    if inner.interaction.pointer_capture.is_some_and(|cap| {
        inner
            .listeners(cap)
            .is_some_and(|l| l.on_pointer_move.is_some())
    }) {
        return TouchDirectDrag::CapturedMove;
    }
    TouchDirectDrag::None
}

impl ViewTree {
    /// Dispatches a platform-neutral touch event with 8px drag slop and inertial fling on release.
    pub fn dispatch_touch(
        &mut self,
        clock: &mut FrameClock,
        pointer_id: u64,
        phase: TouchPhaseKind,
        x: f32,
        y: f32,
        dt_seconds: f32,
    ) -> bool {
        if phase != TouchPhaseKind::Cancelled && (!x.is_finite() || !y.is_finite()) {
            return false;
        }
        match phase {
            TouchPhaseKind::Started => self.handle_touch_started(pointer_id, x, y),
            TouchPhaseKind::Moved => self.handle_touch_moved(pointer_id, x, y, dt_seconds),
            TouchPhaseKind::Ended => self.handle_touch_ended(clock, pointer_id, x, y),
            TouchPhaseKind::Cancelled => self.handle_touch_cancelled(pointer_id),
        }
    }

    fn handle_touch_started(&mut self, pointer_id: u64, x: f32, y: f32) -> bool {
        {
            let mut inner = self.inner.borrow_mut();
            inner.touch.cancel_fling();
            if inner.touch.active_pointer.is_some() {
                return false;
            }
            inner.touch.active_pointer = Some(pointer_id);
            inner.touch.start_pos = (x, y);
            inner.touch.last_pos = (x, y);
            inner.touch.scrolled = false;
            inner.touch.velocity_y = 0.0;
        }
        self.dispatch_pointer_down(x, y)
    }

    fn handle_touch_moved(&mut self, pointer_id: u64, x: f32, y: f32, dt_seconds: f32) -> bool {
        let (direct_mode, crossed_slop, dy, is_scrolling) = {
            let mut inner = self.inner.borrow_mut();
            if inner.touch.active_pointer != Some(pointer_id) {
                return false;
            }
            let direct = classify_touch_direct_drag(&inner);
            if direct != TouchDirectDrag::None {
                inner.touch.last_pos = (x, y);
                inner.touch.velocity_y = 0.0;
                (direct, false, 0.0, false)
            } else {
                let dx_start = x - inner.touch.start_pos.0;
                let dy_start = y - inner.touch.start_pos.1;
                let dist = (dx_start * dx_start + dy_start * dy_start).sqrt();
                let dy = y - inner.touch.last_pos.1;
                inner.touch.last_pos = (x, y);
                let mut crossed = false;
                if !inner.touch.scrolled && dist > TOUCH_SLOP_PX {
                    inner.touch.scrolled = true;
                    if let Some(pid) = inner.pressed_node.take() {
                        crate::view_tree::sync::notify_selector_state_change(&mut inner, pid);
                    }
                    inner.selecting_node = None;
                    inner.scrollbar_drag = None;
                    inner.interaction.pointer_capture = None;
                    inner.interaction.pointer_inside_captured = false;
                    inner.paint_dirty = true;
                    crossed = true;
                }
                if inner.touch.scrolled && dt_seconds.is_finite() && dt_seconds > 0.0001 {
                    let raw_v = (dy / dt_seconds)
                        .clamp(-MAX_FLING_SPEED_PX_PER_SEC, MAX_FLING_SPEED_PX_PER_SEC);
                    inner.touch.velocity_y = inner.touch.velocity_y * (1.0 - VELOCITY_EMA_ALPHA)
                        + raw_v * VELOCITY_EMA_ALPHA;
                }
                (TouchDirectDrag::None, crossed, dy, inner.touch.scrolled)
            }
        };
        match direct_mode {
            TouchDirectDrag::PointerDrag => return self.dispatch_pointer_drag(x, y),
            TouchDirectDrag::CapturedMove => return self.dispatch_pointer_move(x, y),
            TouchDirectDrag::None => {}
        }
        if !is_scrolling {
            return false;
        }
        self.dispatch_scroll_internal(x, y, dy) || crossed_slop
    }

    fn handle_touch_ended(
        &mut self,
        clock: &mut FrameClock,
        pointer_id: u64,
        x: f32,
        y: f32,
    ) -> bool {
        let (matched, was_scrolled, fling_v) = {
            let mut inner = self.inner.borrow_mut();
            if inner.touch.active_pointer != Some(pointer_id) {
                return false;
            }
            inner.touch.active_pointer = None;
            let was_scrolled = inner.touch.scrolled;
            let v = inner.touch.velocity_y;
            inner.touch.scrolled = false;
            inner.touch.velocity_y = 0.0;
            if was_scrolled {
                if let Some(pid) = inner.pressed_node.take() {
                    crate::view_tree::sync::notify_selector_state_change(&mut inner, pid);
                }
                inner.selecting_node = None;
                inner.scrollbar_drag = None;
                inner.interaction.pointer_capture = None;
                inner.interaction.pointer_inside_captured = false;
                inner.paint_dirty = true;
            }
            (true, was_scrolled, v)
        };
        if !matched {
            return false;
        }
        if !was_scrolled {
            return self.dispatch_pointer_up_at(x, y);
        }
        if fling_v.abs() >= MIN_FLING_SPEED_PX_PER_SEC {
            spawn_touch_fling(self, clock, x, y, fling_v);
        }
        true
    }

    fn handle_touch_cancelled(&mut self, pointer_id: u64) -> bool {
        let mut inner = self.inner.borrow_mut();
        if inner.touch.active_pointer != Some(pointer_id) {
            return false;
        }
        inner.touch.reset();
        if let Some(pid) = inner.pressed_node.take() {
            crate::view_tree::sync::notify_selector_state_change(&mut inner, pid);
        }
        inner.selecting_node = None;
        inner.scrollbar_drag = None;
        inner.interaction.pointer_capture = None;
        inner.interaction.pointer_inside_captured = false;
        inner.paint_dirty = true;
        true
    }
}

fn spawn_touch_fling(tree: &ViewTree, clock: &mut FrameClock, x: f32, y: f32, velocity_y: f32) {
    let scope = Scope::new();
    let state = Rc::new(RefCell::new(FlingState {
        active: true,
        x,
        y,
        velocity_y,
    }));
    {
        let mut inner = tree.inner.borrow_mut();
        inner.touch.cancel_fling();
        inner.touch.fling = Some(ActiveFling {
            scope,
            state: Rc::clone(&state),
        });
    }
    let mut tree_cb = tree.clone();
    let st_cb = Rc::clone(&state);
    let st_active = Rc::clone(&state);
    scope.run(|| {
        clock.register_active_check(move || st_active.borrow().active);
        clock.on_frame(move |tick| {
            let dt = tick.dt_seconds.clamp(0.0, MAX_FLING_DT_SECONDS);
            let (is_active, vy, px, py) = {
                let st = st_cb.borrow();
                (st.active, st.velocity_y, st.x, st.y)
            };
            if !is_active || dt <= 0.0 {
                return;
            }
            let moved = (vy * dt).abs() > 0.01 && tree_cb.dispatch_scroll_internal(px, py, vy * dt);
            let next_v = vy * (-FLING_DECAY_PER_SEC * dt).exp();
            let done = !moved || next_v.abs() < STOP_FLING_SPEED_PX_PER_SEC;
            {
                let mut st = st_cb.borrow_mut();
                if done {
                    st.active = false;
                    st.velocity_y = 0.0;
                } else {
                    st.velocity_y = next_v;
                }
            }
            if done {
                tree_cb
                    .inner
                    .borrow_mut()
                    .touch
                    .clear_matching_fling(&st_cb);
                scope.dispose();
            }
        });
    });
}
