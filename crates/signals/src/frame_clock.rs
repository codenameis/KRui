//! First-class `FrameClock`, closed-form analytical spring physics, and compositor animation drivers.

pub use crate::spring::step_analytical_spring;
use crate::spring::SPRING_SETTLE_EPSILON;

use crate::session_hub::active_session;
use crate::{batch, current_scope_id, is_scope_alive, ScopeId, Signal};
use std::cell::Cell;
use std::rc::Rc;

const MIN_ANIM_DURATION_SECONDS: f32 = 0.01;
const SHAKE_VERTICAL_FREQ_RATIO: f32 = 1.3;
const SHAKE_VERTICAL_AMP_RATIO: f32 = 0.25;

pub(crate) struct ScopedAsyncPoller {
    pub(crate) scope_id: ScopeId,
    /// Polls any completed async tasks on the UI thread and returns `(keep_registered, active)`.
    pub(crate) poll: Box<dyn Fn() -> (bool, bool)>,
}

/// Registers a UI-thread poller for `Effects::task` or `Command::new_async` that drains
/// completed background callbacks on each frame tick.
///
/// Automatically removed when its owning `Scope` is disposed or when the poller returns `false`
/// (indicating it has completed or its weak reference expired).
pub fn register_async_poller(poll: impl Fn() -> bool + 'static) {
    register_async_poller_with_retention(move || {
        let active = poll();
        (active, active)
    });
}

/// Registers a UI-thread poller that explicitly distinguishes `(keep_registered, active)`:
/// - `keep_registered`: whether this poller should remain registered for future ticks.
/// - `active`: whether an async task is currently in-flight or was just drained on this tick.
pub fn register_async_poller_with_retention(poll: impl Fn() -> (bool, bool) + 'static) {
    let scope_id = current_scope_id();
    let sess = active_session();
    let mut list = sess.inner.async_pollers.borrow_mut();
    list.retain(|entry| is_scope_alive(entry.scope_id));
    list.push(ScopedAsyncPoller {
        scope_id,
        poll: Box::new(poll),
    });
}

/// Polls all registered async task queues on the currently active session's UI thread.
///
/// Releases borrows before invoking callbacks so re-entrant `register_async_poller` calls never
/// panic. Returns `true` if any task ran or remains active.
pub fn poll_async_tasks() -> bool {
    poll_active_session_tasks()
}

pub(crate) fn poll_active_session_tasks() -> bool {
    let sess = active_session();
    let mut any_active = sess.drain_ui_tasks();
    let taken = {
        let mut list = std::mem::take(&mut *sess.inner.async_pollers.borrow_mut());
        list.retain(|entry| is_scope_alive(entry.scope_id));
        list
    };

    struct PollerRestoreGuard<'a> {
        sess: &'a crate::session_hub::ReactiveSession,
        remaining: std::vec::IntoIter<ScopedAsyncPoller>,
        survivors: Vec<ScopedAsyncPoller>,
    }
    impl Drop for PollerRestoreGuard<'_> {
        fn drop(&mut self) {
            if let Ok(mut list) = self.sess.inner.async_pollers.try_borrow_mut() {
                let newly_registered = std::mem::take(&mut *list);
                self.survivors.extend(
                    self.remaining
                        .by_ref()
                        .filter(|entry| is_scope_alive(entry.scope_id)),
                );
                self.survivors.extend(
                    newly_registered
                        .into_iter()
                        .filter(|entry| is_scope_alive(entry.scope_id)),
                );
                *list = std::mem::take(&mut self.survivors);
            }
        }
    }

    let cap = taken.len();
    let mut guard = PollerRestoreGuard {
        sess: &sess,
        remaining: taken.into_iter(),
        survivors: Vec::with_capacity(cap),
    };
    loop {
        let Some(entry) = guard.remaining.next() else {
            break;
        };
        if !is_scope_alive(entry.scope_id) {
            continue;
        }
        let (keep, active) = crate::Scope::from_id(entry.scope_id).run(&entry.poll);
        if active {
            any_active = true;
        }
        if keep && is_scope_alive(entry.scope_id) {
            guard.survivors.push(entry);
        }
    }
    drop(guard);
    crate::session_hub::flush_deferred_scope_disposals();
    any_active
}

pub(crate) fn live_async_poller_count() -> usize {
    let sess = active_session();
    let count = if let Ok(mut list) = sess.inner.async_pollers.try_borrow_mut() {
        list.retain(|entry| is_scope_alive(entry.scope_id));
        list.len()
    } else if let Ok(list) = sess.inner.async_pollers.try_borrow() {
        list.iter().filter(|e| is_scope_alive(e.scope_id)).count()
    } else {
        0
    };
    count
}

/// Compositor-only 2D transform output produced by fast-path animation helpers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FastPathTransform {
    pub translate_x: f32,
    pub translate_y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotate_deg: f32,
}

impl Default for FastPathTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl FastPathTransform {
    /// Identity 2D transform (`translate = (0, 0)`, `scale = (1, 1)`, `rotate = 0`).
    pub const IDENTITY: Self = Self {
        translate_x: 0.0,
        translate_y: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        rotate_deg: 0.0,
    };
}

/// Frame timing information passed to `on_frame` callbacks by the renderer's vblank clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameTick {
    /// Monotonically increasing frame index.
    pub frame_number: u64,
    /// Delta time since previous frame in seconds.
    pub dt_seconds: f32,
    /// Total elapsed time since clock creation in seconds.
    pub elapsed_seconds: f32,
}

struct ScopedFrameCallback {
    scope_id: ScopeId,
    callback: Box<dyn FnMut(FrameTick)>,
}

struct ScopedActiveCheck {
    scope_id: ScopeId,
    check: Box<dyn Fn() -> bool>,
}

/// First-class per-frame animation driver tied directly to the renderer's frame cadence.
pub struct FrameClock {
    frame_number: u64,
    elapsed_seconds: f32,
    callbacks: Vec<ScopedFrameCallback>,
    active_checks: Vec<ScopedActiveCheck>,
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameClock {
    /// Creates a new `FrameClock`.
    pub fn new() -> Self {
        Self {
            frame_number: 0,
            elapsed_seconds: 0.0,
            callbacks: Vec::new(),
            active_checks: Vec::new(),
        }
    }

    /// Removes any callbacks and active checks whose owning `Scope` has been disposed.
    pub fn prune_disposed_scopes(&mut self) {
        self.callbacks
            .retain(|entry| is_scope_alive(entry.scope_id));
        self.active_checks
            .retain(|entry| is_scope_alive(entry.scope_id));
    }

    /// Returns the number of currently active (non-disposed) per-frame callbacks.
    pub fn active_callback_count(&self) -> usize {
        self.callbacks
            .iter()
            .filter(|entry| is_scope_alive(entry.scope_id))
            .count()
    }

    /// Registers a per-frame callback tied to the current `Scope` and the renderer's frame cadence.
    pub fn on_frame(&mut self, callback: impl FnMut(FrameTick) + 'static) {
        self.prune_disposed_scopes();
        self.callbacks.push(ScopedFrameCallback {
            scope_id: current_scope_id(),
            callback: Box::new(callback),
        });
    }

    /// Registers a predicate that reports whether an animation driver is currently converging.
    pub fn register_active_check(&mut self, check: impl Fn() -> bool + 'static) {
        self.prune_disposed_scopes();
        self.active_checks.push(ScopedActiveCheck {
            scope_id: current_scope_id(),
            check: Box::new(check),
        });
    }

    /// Returns `true` if any non-disposed animation driver is currently active.
    pub fn has_active_animations(&self) -> bool {
        let _ = poll_async_tasks();
        self.active_checks
            .iter()
            .any(|entry| is_scope_alive(entry.scope_id) && (entry.check)())
    }

    /// Advances the frame clock by `dt_seconds`, draining completed async tasks, pruning disposed
    /// scope drivers, and running all live frame callbacks inside a single glitch-free `batch`.
    pub fn tick(&mut self, dt_seconds: f32) -> FrameTick {
        let _ = poll_async_tasks();
        self.prune_disposed_scopes();

        let safe_dt = if dt_seconds.is_finite() && dt_seconds > 0.0 {
            dt_seconds
        } else {
            0.0
        };
        self.frame_number += 1;
        self.elapsed_seconds += safe_dt;

        let tick_info = FrameTick {
            frame_number: self.frame_number,
            dt_seconds: safe_dt,
            elapsed_seconds: self.elapsed_seconds,
        };

        batch(|| {
            for entry in &mut self.callbacks {
                if is_scope_alive(entry.scope_id) {
                    let scope = crate::Scope::from_id(entry.scope_id);
                    let cb = &mut entry.callback;
                    scope.run(|| cb(tick_info));
                }
            }
        });

        tick_info
    }

    /// Creates a fast-path horizontal/vertical `shake()` animation signal pre-wired to compositor-only
    /// `FastPathTransform` (`translate_x`, `translate_y`).
    pub fn shake(
        &mut self,
        amplitude_px: f32,
        frequency_hz: f32,
        duration_seconds: f32,
    ) -> (Signal<FastPathTransform>, Signal<bool>) {
        let transform_sig = Signal::new_frame_driven(FastPathTransform::IDENTITY);
        let trigger_sig = Signal::new(false);
        let remaining_cell = Rc::new(Cell::new(0.0_f32));

        let rem_for_check = remaining_cell.clone();
        self.register_active_check(move || {
            trigger_sig.get_untracked()
                || rem_for_check.get() > 0.0
                || transform_sig.get_untracked() != FastPathTransform::IDENTITY
        });

        self.on_frame(move |tick| {
            let mut remaining = remaining_cell.get();
            if trigger_sig.get_untracked() {
                remaining = duration_seconds.max(MIN_ANIM_DURATION_SECONDS);
                trigger_sig.set(false);
            }

            if remaining > 0.0 {
                remaining = (remaining - tick.dt_seconds).max(0.0);
                remaining_cell.set(remaining);
                if remaining == 0.0 {
                    transform_sig.set_if_changed(FastPathTransform::IDENTITY);
                    return;
                }
                let dur = duration_seconds.max(MIN_ANIM_DURATION_SECONDS);
                let progress = 1.0 - (remaining / dur);
                let decay = (1.0 - progress).powi(2);
                let phase = progress * duration_seconds * frequency_hz * std::f32::consts::TAU;
                let dx = phase.sin() * amplitude_px * decay;
                let dy = (phase * SHAKE_VERTICAL_FREQ_RATIO).cos()
                    * (amplitude_px * SHAKE_VERTICAL_AMP_RATIO)
                    * decay;
                transform_sig.set(FastPathTransform {
                    translate_x: dx,
                    translate_y: dy,
                    ..FastPathTransform::IDENTITY
                });
            } else {
                remaining_cell.set(0.0);
                transform_sig.set_if_changed(FastPathTransform::IDENTITY);
            }
        });

        (transform_sig, trigger_sig)
    }

    /// Creates a fast-path directional `nudge()` impulse animation signal pre-wired to compositor-only
    /// `FastPathTransform`.
    pub fn nudge(
        &mut self,
        offset_x: f32,
        offset_y: f32,
        duration_seconds: f32,
    ) -> (Signal<FastPathTransform>, Signal<bool>) {
        let transform_sig = Signal::new_frame_driven(FastPathTransform::IDENTITY);
        let trigger_sig = Signal::new(false);
        let remaining_cell = Rc::new(Cell::new(0.0_f32));

        let rem_for_check = remaining_cell.clone();
        self.register_active_check(move || {
            trigger_sig.get_untracked()
                || rem_for_check.get() > 0.0
                || transform_sig.get_untracked() != FastPathTransform::IDENTITY
        });

        self.on_frame(move |tick| {
            let mut remaining = remaining_cell.get();
            if trigger_sig.get_untracked() {
                remaining = duration_seconds.max(MIN_ANIM_DURATION_SECONDS);
                trigger_sig.set(false);
            }

            if remaining > 0.0 {
                remaining = (remaining - tick.dt_seconds).max(0.0);
                remaining_cell.set(remaining);
                if remaining == 0.0 {
                    transform_sig.set_if_changed(FastPathTransform::IDENTITY);
                    return;
                }
                let t = 1.0 - (remaining / duration_seconds.max(MIN_ANIM_DURATION_SECONDS));
                let envelope = (t * std::f32::consts::PI).sin();
                transform_sig.set(FastPathTransform {
                    translate_x: offset_x * envelope,
                    translate_y: offset_y * envelope,
                    ..FastPathTransform::IDENTITY
                });
            } else {
                remaining_cell.set(0.0);
                transform_sig.set_if_changed(FastPathTransform::IDENTITY);
            }
        });

        (transform_sig, trigger_sig)
    }

    /// Creates an exact closed-form analytical spring driver (`spring_to`) that smoothly animates
    /// a `f32` value toward `target` on every frame clock tick without Euler instability on large `dt`.
    pub fn spring_to(
        &mut self,
        initial: f32,
        target: impl Into<Signal<f32>>,
        stiffness: f32,
        damping: f32,
    ) -> Signal<f32> {
        let target = target.into();
        let current = Signal::new_frame_driven(initial);
        let velocity_cell = Rc::new(Cell::new(0.0_f32));

        let vel_for_check = velocity_cell.clone();
        self.register_active_check(move || {
            let goal = target.get_untracked();
            let pos = current.get_untracked();
            (goal - pos).abs() >= SPRING_SETTLE_EPSILON
                || vel_for_check.get().abs() >= SPRING_SETTLE_EPSILON
        });

        self.on_frame(move |tick| {
            let goal = target.get_untracked();
            let pos = current.get_untracked();
            let vel = velocity_cell.get();
            if (goal - pos).abs() < SPRING_SETTLE_EPSILON && vel.abs() < SPRING_SETTLE_EPSILON {
                current.set_if_changed(goal);
                velocity_cell.set(0.0);
                return;
            }
            let (next_pos, next_vel) =
                step_analytical_spring(pos, vel, goal, stiffness, damping, tick.dt_seconds);
            velocity_cell.set(next_vel);
            current.set_if_changed(next_pos);
        });

        current
    }
}
