//! Adversarial review regression tests for `krui-signals` (`Signal::update` re-entrancy & unwind,
//! initial `Effect::new` write flushing, `ScopeId::ROOT` async cleanup, and cyclic `Computed` guard).

#![forbid(unsafe_code)]

use krui_signals::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn signal_update_reentrant_get_with_and_set_if_changed_never_panics() {
    reset_reactive_runtime();
    let sig = Signal::new(10_i32);

    // 1. First `.update()` shares `Rc` with `initial_value` (`strong_count == 2`, CoW path):
    //    `sig.get()` inside `update` reads current value `10` without panicking.
    sig.update(|v| {
        let cur = sig.get();
        *v = cur + 5;
    });
    assert_eq!(sig.get(), 15);

    // 2. Re-entrant `sig.update()` inside outer `sig.with()` (`borrow_depth == 1`, CoW path):
    sig.with(|borrowed| {
        sig.update(|v| {
            *v = *borrowed * 2;
        });
    });
    assert_eq!(sig.get(), 30);

    // 3. Unshared in-place `.update()` (`strong_count == 1`): `try_get()` returns `None` cleanly
    //    and `set_if_changed()` returns `false` without `"Signal type mismatch in arena"` panic.
    sig.update(|v| {
        assert_eq!(sig.try_get(), None);
        assert!(!sig.set_if_changed(99));
        *v += 1;
    });
    assert_eq!(sig.get(), 31);
}

#[test]
fn signal_update_panic_in_mutator_or_cow_clone_preserves_valid_typed_value() {
    reset_reactive_runtime();

    #[derive(Debug)]
    struct BombClone {
        value: i32,
        panic_on_clone: Arc<AtomicBool>,
    }
    impl Clone for BombClone {
        fn clone(&self) -> Self {
            if self.panic_on_clone.load(Ordering::SeqCst) {
                panic!("simulated OOM/clone panic");
            }
            Self {
                value: self.value,
                panic_on_clone: self.panic_on_clone.clone(),
            }
        }
    }

    let flag = Arc::new(AtomicBool::new(false));
    let sig = Signal::new(BombClone {
        value: 7,
        panic_on_clone: flag.clone(),
    });
    // 0. Panic inside `Clone::clone` when `borrow_depth == 0` AND `strong_count == 2` (`initial_value` shares `Rc`):
    //    `taken_in_place == true`, so `UpdateSlotGuard` must restore `slot.value` instead of leaving `Rc::new(())`!
    flag.store(true, Ordering::SeqCst);
    let res0 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sig.update(|inner| inner.value = 88);
    }));
    assert!(res0.is_err());
    flag.store(false, Ordering::SeqCst);
    assert_eq!(sig.with(|b| b.value), 7);

    // Detach from `initial_value` so `Rc` has strong_count == 1 when unborrowed.
    sig.update(|b| b.value = 11);

    // 1. Panic inside `f` while `borrow_depth == 0` (`taken_in_place == true`):
    //    must NOT leave `Rc::new(())` in the slot!
    let res1 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sig.update(|_b| panic!("mutator boom"));
    }));
    assert!(res1.is_err());
    assert_eq!(sig.with(|b| b.value), 11);

    // 2. Panic inside `Clone::clone` on the CoW path (`borrow_depth == 1`):
    //    must clear `updating_signals` and preserve the existing value so subsequent `.update()` works!
    flag.store(true, Ordering::SeqCst);
    sig.with(|b| {
        assert_eq!(b.value, 11);
        let res2 = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sig.update(|inner| inner.value = 99);
        }));
        assert!(res2.is_err());
    });
    flag.store(false, Ordering::SeqCst);
    sig.update(|b| b.value += 9);
    assert_eq!(sig.with(|b| b.value), 20);
}

#[test]
fn effect_initial_run_signal_set_flushes_downstream_effects_immediately() {
    reset_reactive_runtime();
    let derived_target = Signal::new(0_i32);
    let log = Rc::new(RefCell::new(Vec::new()));
    let log_c = log.clone();
    let _observer = Effect::new(move || {
        log_c.borrow_mut().push(derived_target.get());
    });
    assert_eq!(*log.borrow(), vec![0]);

    let source = Signal::new(42_i32);
    let _writer = Effect::new(move || {
        derived_target.set(source.get());
    });

    assert_eq!(
        *log.borrow(),
        vec![0, 42],
        "Signal::set inside initial Effect::new (outside batch) must flush pending subscribers when tracking unwinds"
    );
}

#[test]
fn root_scope_tokio_task_aborts_when_session_resets() {
    let sess = ReactiveSession::new();
    let dropped = Arc::new(AtomicUsize::new(0));
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let dropped_bg = dropped.clone();

    sess.enter(|| {
        assert_eq!(current_scope_id(), ScopeId::ROOT);
        struct DropCounter(Arc<AtomicUsize>);
        impl Drop for DropCounter {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let _handle = spawn_tokio_task(
            async move {
                let _guard = DropCounter(dropped_bg);
                let _ = started_tx.send(());
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            },
            |_| {},
        );
    });

    started_rx.recv().expect("root tokio task started");
    sess.reset();

    for _ in 0..80 {
        if dropped.load(Ordering::SeqCst) == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        1,
        "Tasks spawned in ScopeId::ROOT must be aborted when ReactiveSession::reset() runs"
    );
}

#[test]
fn dynamic_cyclic_computed_returns_cached_value_without_stack_overflow() {
    reset_reactive_runtime();
    let trigger = Signal::new(false);
    let c2_slot: Rc<Cell<Option<Computed<i32>>>> = Rc::new(Cell::new(None));
    let c2_for_c1 = c2_slot.clone();

    let c1 = Computed::new(move || {
        if trigger.get() {
            if let Some(c2) = c2_for_c1.get() {
                return c2.get() + 1;
            }
        }
        10
    });
    let c2 = Computed::new(move || if trigger.get() { c1.get() + 1 } else { 20 });
    c2_slot.set(Some(c2));

    assert_eq!(c1.get(), 10);
    assert_eq!(c2.get(), 20);

    // Flipping `trigger` creates a dynamic `c1 <-> c2` cycle; must terminate cleanly without stack overflow.
    trigger.set(true);
    let v1 = c1.get();
    let v2 = c2.get();
    assert!(v1 >= 10 && v2 >= 10);
}

#[test]
fn signal_update_second_call_cross_signal_effect_and_reentrant_set_never_panic_or_clobber() {
    reset_reactive_runtime();
    let sig = Signal::new(1_i32);
    let other = Signal::new(0_i32);
    let observed = Rc::new(RefCell::new(Vec::new()));
    let obs_c = observed.clone();
    let _eff = Effect::new(move || {
        obs_c.borrow_mut().push((other.get(), sig.get()));
    });
    assert_eq!(*observed.borrow(), vec![(0, 1)]);

    // Detach `sig.value` from `sig.initial_value` so `Rc::strong_count == 1` (`InPlace` path).
    sig.update(|v| *v = 2);
    assert_eq!(*observed.borrow(), vec![(0, 1), (0, 2)]);

    // 1. Cross-signal `other.set()` + 2nd-call re-entrant `sig.get()` inside `InPlace` update:
    //    must batch `_eff` until `sig` is restored and never panic!
    sig.update(|v| {
        let _ = sig.get();
        *v = 3;
        other.set(1);
    });
    assert_eq!(sig.get(), 3);
    assert_eq!(*observed.borrow(), vec![(0, 1), (0, 2), (1, 3)]);

    // 2. Re-entrant `sig.set(99)` inside `InPlace` update must supersede `*v` instead of being clobbered:
    sig.update(|v| {
        *v = 50;
        sig.set(99);
        assert_eq!(sig.get(), 99);
    });
    assert_eq!(sig.get(), 99);

    // 3. Re-entrant `sig.reset_to_initial()` inside `InPlace` update must restore `1`:
    sig.update(|v| {
        *v = 77;
        sig.reset_to_initial();
    });
    assert_eq!(sig.get(), 1);
}

#[test]
fn untracked_computed_pull_inside_effect_does_not_flush_stale_glitch() {
    reset_reactive_runtime();
    let src = Signal::new(1_i32);
    let comp = Computed::new(move || src.get() * 10);
    let observed = Rc::new(RefCell::new(Vec::new()));
    let obs_c = observed.clone();
    let _obs = Effect::new(move || {
        obs_c.borrow_mut().push(comp.get());
    });
    assert_eq!(*observed.borrow(), vec![10]);

    let pulled = Rc::new(Cell::new(0));
    let pulled_c = pulled.clone();
    let _writer = Effect::new(move || {
        src.set(2);
        let val = untracked(|| comp.get());
        pulled_c.set(val);
    });

    assert_eq!(pulled.get(), 20);
    assert_eq!(
        *observed.borrow(),
        vec![10, 20],
        "`untracked(|| comp.get())` inside an Effect must never flush `_obs` prematurely with stale value `10`"
    );
}
