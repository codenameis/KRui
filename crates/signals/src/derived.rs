//! Derived reactive signals (`Computed`, `Effect`).

use crate::arena_types::BatchDepthGuard;
use crate::propagation::{
    allocate_subscriber_slot, run_computed_tracking, run_effect_tracking, set_subscriber_callback,
    EFFECT_TOPO_DEPTH,
};
use crate::runtime::{ComputedId, EffectId, SubscriberId, RUNTIME};
use crate::Signal;
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

/// A glitch-free derived reactive signal whose value is dynamically re-tracked and recomputed
/// when its active upstream dependencies change.
///
/// Uses `PhantomData<*const T>` so `Computed<T>` is strictly `!Send` and `!Sync` (thread-local arena).
///
/// ```compile_fail
/// use krui_signals::Computed;
/// fn assert_send<T: Send>() {}
/// assert_send::<Computed<i32>>();
/// ```
pub struct Computed<T: 'static> {
    inner: Signal<T>,
    subscriber_id: SubscriberId,
    _marker: PhantomData<*const T>,
}

impl<T: 'static> Clone for Computed<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Computed<T> {}

impl<T: 'static> PartialEq for Computed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.subscriber_id == other.subscriber_id && self.inner == other.inner
    }
}

impl<T: 'static> Eq for Computed<T> {}

impl<T: 'static> From<Computed<T>> for Signal<T> {
    fn from(computed: Computed<T>) -> Self {
        computed.inner
    }
}

impl<T: 'static + fmt::Debug + Clone> fmt::Debug for Computed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Computed")
            .field("id", &self.subscriber_id)
            .field("value", &self.get_untracked())
            .finish()
    }
}

impl<T: 'static + fmt::Display> fmt::Display for Computed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.with(|v| fmt::Display::fmt(v, f))
    }
}

impl std::ops::Not for Computed<bool> {
    type Output = bool;
    fn not(self) -> bool {
        !self.get()
    }
}

impl<T: Clone + 'static> Computed<T> {
    /// Creates a derived signal from a pure computation closure, evaluating `compute` once initially
    /// and dynamically re-tracking dependencies on every re-evaluation.
    pub fn new(compute: impl Fn() -> T + 'static) -> Self {
        let sub_id = allocate_subscriber_slot(1, false);
        let initial_val = run_computed_tracking(sub_id, &compute);
        let inner_signal = Signal::new_produced(initial_val, sub_id);
        set_subscriber_callback(
            sub_id,
            Rc::new(move || {
                RUNTIME.with(|rt| rt.borrow_mut().batch_depth += 1);
                let _batch = BatchDepthGuard {
                    flush_on_zero: true,
                };
                let next_val = run_computed_tracking(sub_id, &compute);
                inner_signal.set_computed_output(next_val);
            }),
        );
        Self {
            inner: inner_signal,
            subscriber_id: sub_id,
            _marker: PhantomData,
        }
    }

    /// Creates a memoized derived signal (`Selector`) that only propagates to downstream
    /// subscribers when the newly computed value differs (`PartialEq::ne`) from the previous value.
    pub fn new_memoized(compute: impl Fn() -> T + 'static) -> Self
    where
        T: PartialEq,
    {
        let sub_id = allocate_subscriber_slot(1, false);
        let initial_val = run_computed_tracking(sub_id, &compute);
        let inner_signal = Signal::new_produced(initial_val, sub_id);
        set_subscriber_callback(
            sub_id,
            Rc::new(move || {
                RUNTIME.with(|rt| rt.borrow_mut().batch_depth += 1);
                let _batch = BatchDepthGuard {
                    flush_on_zero: true,
                };
                let next_val = run_computed_tracking(sub_id, &compute);
                inner_signal.set_computed_output_if_changed(next_val);
            }),
        );
        Self {
            inner: inner_signal,
            subscriber_id: sub_id,
            _marker: PhantomData,
        }
    }

    /// Returns the subscriber ID of this computed signal.
    pub fn id(self) -> ComputedId {
        self.subscriber_id
    }

    /// Returns `true` if this computed signal's producer and backing signal are still alive.
    pub fn is_alive(self) -> bool {
        self.inner.is_alive()
    }

    /// Reads the derived value by reference if still alive, returning `None` after scope disposal.
    pub fn try_with<R>(self, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.inner.try_with(f)
    }

    /// Reads and clones the current derived value if still alive, returning `None` after disposal.
    pub fn try_get(self) -> Option<T> {
        self.inner.try_get()
    }

    /// Reads the derived value by reference via a closure (pulling fresh state if dirty) and subscribes the caller.
    pub fn with<R>(self, f: impl FnOnce(&T) -> R) -> R {
        self.inner.with(f)
    }

    /// Reads and clones the current derived value (pulling fresh state if dirty), subscribing the caller.
    pub fn get(self) -> T {
        self.inner.get()
    }

    /// Reads and clones the current derived value (pulling fresh state if dirty) without subscribing.
    pub fn get_untracked(self) -> T {
        self.inner.get_untracked()
    }

    /// Returns the underlying `Signal<T>`.
    pub fn as_signal(self) -> Signal<T> {
        self.inner
    }
}

/// A reactive side-effect that runs immediately and re-runs whenever any `Signal` or `Computed`
/// read inside it changes.
///
/// Strictly `!Send` and `!Sync` (thread-local reactive arena).
///
/// ```compile_fail
/// use krui_signals::Effect;
/// fn assert_send<T: Send>() {}
/// assert_send::<Effect>();
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    id: SubscriberId,
    _marker: PhantomData<*const ()>,
}

impl Effect {
    /// Creates and immediately runs a new reactive effect in the current `Scope`.
    pub fn new(effect_fn: impl Fn() + 'static) -> Self {
        let sub_id = allocate_subscriber_slot(EFFECT_TOPO_DEPTH, true);
        let cb: Rc<dyn Fn()> = Rc::new(effect_fn);
        set_subscriber_callback(sub_id, cb.clone());
        run_effect_tracking(sub_id, || cb());
        Self {
            id: sub_id,
            _marker: PhantomData,
        }
    }

    /// Returns the subscriber ID of this effect.
    pub fn id(self) -> EffectId {
        self.id
    }

    /// Disposes this effect and any nested reactive primitives created during its last execution.
    pub fn dispose(self) {
        let Some(sess) = crate::session_hub::session_for_generation(self.id.generation) else {
            return;
        };
        sess.enter(|| {
            let mut disposed_now = false;
            if let Ok(mut rt_mut) = sess.inner.runtime.try_borrow_mut() {
                rt_mut.dispose_subscriber(self.id);
                disposed_now = true;
            }
            if disposed_now {
                crate::runtime::drain_deferred_drops();
            }
        });
    }
}
