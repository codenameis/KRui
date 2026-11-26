//! # `krui-signals`
//!
//! Fine-grained, glitch-free reactive primitives (`Signal`, `Computed`, `Effect`, `Scope`, `batch`)
//! with 8-byte `!Send + !Sync` `Copy` handles, push-pull topological propagation, dynamic dependency
//! re-tracking, `UiWaker` event-loop wakeups, and analytical `FrameClock` animation drivers.

#![forbid(unsafe_code)]

mod arena_types;
mod derived;
mod frame_clock;
mod propagation;
mod runtime;
mod scope;
pub mod session_hub;
mod signal_ops;
mod spring;
mod template_scalar;
pub mod tokio_bridge;
mod waker;

pub use derived::{Computed, Effect};
pub use frame_clock::{
    poll_async_tasks, register_async_poller, register_async_poller_with_retention,
    step_analytical_spring, FastPathTransform, FrameClock, FrameTick,
};
pub use runtime::{
    batch, current_scope_id, debug_stats, is_scope_alive, reset_reactive_runtime,
    total_subscriber_executions, untracked, ComputedId, EffectId, RuntimeStats, Scope, ScopeId,
    SignalId, SubscriberId,
};
pub use session_hub::{
    current_runtime_id, current_wake_handle, ReactiveSession, RuntimeId, SessionWakeHandle,
};
pub use template_scalar::{
    IntoKruiTemplateScalar, KruiBoolCondition, KruiIterableSource, KruiLoopItem, KruiTemplateScalar,
};
pub use tokio_bridge::{
    init_async_runtime_mode, spawn_tokio_interval, spawn_tokio_stream, spawn_tokio_task,
    spawn_tokio_task_with_status, tokio_handle, tokio_runtime, AsyncRuntimeMode, AsyncTaskHandle,
    StreamSink,
};
pub use waker::{set_ui_waker, wake_ui_loop, UiWaker};

use runtime::{ensure_signal_up_to_date, flush_pending_subscribers, RUNTIME};
use signal_ops::{
    allocate_signal_slot, default_disposed_fallback, record_active_subscription,
    store_value_in_slot, SignalReadGuard,
};
use std::any::Any;
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

/// A fine-grained reactive signal holding a value of type `T`.
///
/// `Signal<T>` is an 8-byte `Copy` handle (`SignalId` + `PhantomData<*const T>`) backed by the
/// thread-local reactive arena. It is strictly `!Send` and `!Sync` so handles can never cross threads.
///
/// ```compile_fail
/// use krui_signals::Signal;
/// fn assert_send<T: Send>() {}
/// let sig = Signal::new(42_i32);
/// let _ = sig;
/// assert_send::<Signal<i32>>();
/// ```
///
/// ```compile_fail
/// use krui_signals::Signal;
/// fn assert_sync<T: Sync>() {}
/// assert_sync::<Signal<i32>>();
/// ```
pub struct Signal<T: 'static> {
    id: SignalId,
    _marker: PhantomData<*const T>,
}

impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Signal<T> {}

impl<T: 'static> PartialEq for Signal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T: 'static> Eq for Signal<T> {}

impl<T: 'static + fmt::Debug + Clone> fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Signal")
            .field("id", &self.id)
            .field("value", &self.get_untracked())
            .finish()
    }
}

impl<T: 'static + fmt::Display> fmt::Display for Signal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.with(|v| fmt::Display::fmt(v, f))
    }
}

impl std::ops::Not for Signal<bool> {
    type Output = bool;
    fn not(self) -> bool {
        !self.get()
    }
}

impl<T: 'static> Signal<T> {
    /// Creates a new reactive signal initialized to `initial` in the current `Scope`.
    pub fn new(initial: T) -> Self {
        Self::new_internal(initial, false, None)
    }

    /// Creates a signal marked as driven by the per-frame animation clock.
    pub fn new_frame_driven(initial: T) -> Self {
        Self::new_internal(initial, true, None)
    }

    pub(crate) fn new_produced(initial: T, producer: SubscriberId) -> Self {
        Self::new_internal(initial, false, Some(producer))
    }

    fn new_internal(initial: T, is_frame_driven: bool, producer: Option<SubscriberId>) -> Self {
        let initial_rc: Rc<dyn Any> = Rc::new(initial);
        let value_rc = initial_rc.clone();
        let id = RUNTIME.with(|rt| {
            let mut rt = rt.borrow_mut();
            allocate_signal_slot(&mut rt, value_rc, initial_rc, producer, is_frame_driven)
        });
        Self {
            id,
            _marker: PhantomData,
        }
    }

    /// Returns the underlying `SignalId`.
    pub const fn id(self) -> SignalId {
        self.id
    }

    /// Returns `true` if this signal's slot is still alive (not disposed).
    pub fn is_alive(self) -> bool {
        RUNTIME.with(|rt| rt.borrow().is_signal_alive(self.id))
    }

    /// Returns `true` if this signal is driven by the per-frame animation clock.
    pub fn is_frame_driven(self) -> bool {
        RUNTIME.with(|rt| {
            let rt = rt.borrow();
            rt.is_signal_alive(self.id) && rt.signals[self.id.index as usize].is_frame_driven
        })
    }

    fn resolve_disposed_value(self) -> Rc<dyn Any> {
        default_disposed_fallback::<T>().expect(
            "Cannot call .get()/.with() on a disposed non-primitive Signal; use .try_get() or .try_with()",
        )
    }

    /// Borrows a live signal's value without cloning, returning `None` if its `Scope` was disposed.
    pub fn try_with<R>(self, f: impl FnOnce(&T) -> R) -> Option<R> {
        if !self.is_alive() {
            return None;
        }
        Some(self.with(f))
    }

    /// Reads the signal's value via a closure and subscribes the currently executing `Computed` or `Effect`.
    ///
    /// Releases the internal `RUNTIME` borrow before invoking `f` so re-entrant reads/writes never panic.
    pub fn with<R>(self, f: impl FnOnce(&T) -> R) -> R {
        ensure_signal_up_to_date(self.id);
        let (val_rc, is_live) = RUNTIME.with(|rt| {
            let mut rt = rt.borrow_mut();
            if !rt.is_signal_alive(self.id) {
                return (self.resolve_disposed_value(), false);
            }
            record_active_subscription(&mut rt, self.id);
            let slot = &mut rt.signals[self.id.index as usize];
            slot.borrow_depth += 1;
            (slot.value.clone(), true)
        });

        let _read_guard = is_live.then_some(SignalReadGuard { id: self.id });
        let val_ref = val_rc
            .downcast_ref::<T>()
            .expect("Signal type mismatch in arena");
        f(val_ref)
    }

    /// Reads the signal's value without subscribing the currently active effect or computed.
    pub fn with_untracked<R>(self, f: impl FnOnce(&T) -> R) -> R {
        ensure_signal_up_to_date(self.id);
        let (val_rc, is_live) = RUNTIME.with(|rt| {
            let mut rt = rt.borrow_mut();
            if rt.is_signal_alive(self.id) {
                let slot = &mut rt.signals[self.id.index as usize];
                slot.borrow_depth += 1;
                (slot.value.clone(), true)
            } else {
                (self.resolve_disposed_value(), false)
            }
        });

        let _read_guard = is_live.then_some(SignalReadGuard { id: self.id });
        let val_ref = val_rc
            .downcast_ref::<T>()
            .expect("Signal type mismatch in arena");
        f(val_ref)
    }

    /// Replaces the signal's value and schedules dependent subscribers.
    ///
    /// Reuses the underlying heap allocation in-place when unshared, and safely no-ops if disposed.
    pub fn set(self, new_value: T) {
        self.write_and_notify(new_value, false);
    }

    pub(crate) fn set_computed_output(self, new_value: T) {
        self.write_and_notify(new_value, true);
    }

    fn write_and_notify(self, new_value: T, allow_during_borrow: bool) {
        let should_flush = RUNTIME.with(|rt| {
            let mut rt = rt.borrow_mut();
            if !rt.is_signal_alive(self.id) {
                return false;
            }
            let slot = &mut rt.signals[self.id.index as usize];
            store_value_in_slot(slot, new_value);
            if !allow_during_borrow && slot.borrow_depth > 0 {
                slot.deferred_notify = true;
                return false;
            }
            rt.notify_signal_changed(self.id);
            rt.batch_depth == 0 && rt.tracking_stack.is_empty()
        });

        if should_flush {
            flush_pending_subscribers();
        }
    }

    /// Replaces the signal's value **only if `new_value != current_value` (`PartialEq`)**.
    pub fn set_if_changed(self, new_value: T) -> bool
    where
        T: PartialEq,
    {
        self.write_if_changed_internal(new_value, false)
    }

    pub(crate) fn set_computed_output_if_changed(self, new_value: T) -> bool
    where
        T: PartialEq,
    {
        self.write_if_changed_internal(new_value, true)
    }

    fn write_if_changed_internal(self, new_value: T, allow_during_borrow: bool) -> bool
    where
        T: PartialEq,
    {
        let current_rc = RUNTIME.with(|rt| {
            let rt = rt.borrow();
            rt.is_signal_alive(self.id)
                .then(|| rt.signals[self.id.index as usize].value.clone())
        });
        let Some(cur_any) = current_rc else {
            return false;
        };
        let cur_ref = cur_any
            .downcast_ref::<T>()
            .expect("Signal type mismatch in arena");
        if *cur_ref == new_value {
            return false;
        }
        drop(cur_any);
        self.write_and_notify(new_value, allow_during_borrow);
        true
    }

    /// Restores this signal losslessly back to its original initial value and notifies subscribers.
    ///
    /// Preserves arbitrary structs and strings of any length without truncation.
    pub fn reset_to_initial(self) {
        let should_flush = RUNTIME.with(|rt| {
            let mut rt = rt.borrow_mut();
            if !rt.is_signal_alive(self.id) {
                return false;
            }
            let slot = &mut rt.signals[self.id.index as usize];
            slot.value = slot.initial_value.clone();
            if slot.borrow_depth > 0 {
                slot.deferred_notify = true;
                return false;
            }
            rt.notify_signal_changed(self.id);
            rt.batch_depth == 0 && rt.tracking_stack.is_empty()
        });

        if should_flush {
            flush_pending_subscribers();
        }
    }

    /// Mutates the signal's value in-place with zero cloning when unshared, and schedules subscribers.
    ///
    /// Falls back cleanly to CoW if an active `with()` borrow exists, and restores ownership via
    /// an RAII guard even if `f` unwinds.
    pub fn update(self, f: impl FnOnce(&mut T))
    where
        T: Clone,
    {
        let Some((val_rc, taken_in_place)) = take_signal_rc_for_update(self.id) else {
            return;
        };
        let Ok(typed_rc) = Rc::downcast::<T>(val_rc) else {
            clear_updating_flag(self.id);
            return;
        };

        let mut restore = UpdateSlotGuard::new(self.id, taken_in_place);
        run_mutator_with_restore(typed_rc, f, &mut restore);
        drop(restore);
        finish_signal_update(self.id);
    }
}

struct UpdateSlotGuard {
    sig_id: SignalId,
    taken_in_place: bool,
    restored_val: Option<Rc<dyn Any>>,
}

impl UpdateSlotGuard {
    fn new(sig_id: SignalId, taken_in_place: bool) -> Self {
        Self {
            sig_id,
            taken_in_place,
            restored_val: None,
        }
    }
}

impl Drop for UpdateSlotGuard {
    fn drop(&mut self) {
        let val = self.restored_val.take();
        let taken = self.taken_in_place;
        let id = self.sig_id;
        let _ = RUNTIME.try_with(|rt| {
            let mut rt_mut = rt.borrow_mut();
            if let Some(pos) = rt_mut.updating_signals.iter().position(|s| *s == id) {
                rt_mut.updating_signals.swap_remove(pos);
            }
            if rt_mut.is_signal_alive(id) {
                if let Some(v) = val {
                    rt_mut.signals[id.index as usize].value = v;
                } else if taken {
                    // Preserve validity if `f` unwound before replacing `restored_val`.
                }
            }
        });
    }
}

fn take_signal_rc_for_update(id: SignalId) -> Option<(Rc<dyn Any>, bool)> {
    RUNTIME.with(|rt| {
        let mut rt_mut = rt.borrow_mut();
        if !rt_mut.is_signal_alive(id) || rt_mut.updating_signals.contains(&id) {
            return None;
        }
        rt_mut.updating_signals.push(id);
        let slot = &mut rt_mut.signals[id.index as usize];
        if slot.borrow_depth == 0 {
            let taken = std::mem::replace(&mut slot.value, arena_types::empty_any_rc());
            Some((taken, true))
        } else {
            Some((slot.value.clone(), false))
        }
    })
}

fn clear_updating_flag(id: SignalId) {
    RUNTIME.with(|rt| {
        let mut rt_mut = rt.borrow_mut();
        if let Some(pos) = rt_mut.updating_signals.iter().position(|s| *s == id) {
            rt_mut.updating_signals.swap_remove(pos);
        }
    });
}

fn run_mutator_with_restore<T: Clone + 'static>(
    mut typed_rc: Rc<T>,
    f: impl FnOnce(&mut T),
    restore: &mut UpdateSlotGuard,
) {
    if let Some(unique_ref) = Rc::get_mut(&mut typed_rc) {
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(unique_ref)));
        restore.restored_val = Some(typed_rc);
        if let Err(payload) = res {
            std::panic::resume_unwind(payload);
        }
    } else {
        let mut cow_copy = (*typed_rc).clone();
        drop(typed_rc);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut cow_copy)));
        restore.restored_val = Some(Rc::new(cow_copy));
        if let Err(payload) = res {
            std::panic::resume_unwind(payload);
        }
    }
}

fn finish_signal_update(id: SignalId) {
    let should_flush = RUNTIME.with(|rt| {
        let mut rt = rt.borrow_mut();
        if !rt.is_signal_alive(id) {
            return false;
        }
        if rt.signals[id.index as usize].borrow_depth > 0 {
            rt.signals[id.index as usize].deferred_notify = true;
            return false;
        }
        rt.notify_signal_changed(id);
        rt.batch_depth == 0 && rt.tracking_stack.is_empty()
    });
    if should_flush {
        flush_pending_subscribers();
    }
}

impl<T: Clone + 'static> Signal<T> {
    /// Clones and returns the current value, subscribing the active `Computed` or `Effect`.
    pub fn get(self) -> T {
        self.with(T::clone)
    }

    /// Clones and returns the current value without subscribing.
    pub fn get_untracked(self) -> T {
        self.with_untracked(T::clone)
    }

    /// Returns `Some(value)` if this signal is alive, or `None` if its owning `Scope` was disposed.
    pub fn try_get(self) -> Option<T> {
        self.try_with(T::clone)
    }
}
