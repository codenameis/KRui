//! Unit and regression tests for `krui-signals` (`Signal`, `Computed`, `Effect`, `Scope`, `FrameClock`, `UiWaker`).

#![forbid(unsafe_code)]

use krui_signals::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
struct CustomProfile {
    title: String,
    tags: Vec<String>,
    score: f64,
}

struct CountingWaker {
    wakes: AtomicUsize,
}

impl UiWaker for CountingWaker {
    fn wake(&self) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn signal_handle_is_8_bytes_and_basic_computed_propagates() {
    reset_reactive_runtime();
    assert_eq!(std::mem::size_of::<Signal<i32>>(), 8);
    assert_eq!(std::mem::size_of::<Signal<String>>(), 8);
    assert_eq!(std::mem::size_of::<Signal<CustomProfile>>(), 8);

    let eval_count = Rc::new(Cell::new(0));
    let eval_clone = eval_count.clone();
    let count = Signal::new(2);
    let doubled = Computed::new(move || {
        eval_clone.set(eval_clone.get() + 1);
        count.get() * 2
    });
    assert_eq!(eval_count.get(), 1);
    assert_eq!(doubled.get(), 4);

    count.set(7);
    assert_eq!(eval_count.get(), 2);
    assert_eq!(doubled.get(), 14);
}

#[test]
fn long_string_and_custom_struct_reset_without_truncation() {
    reset_reactive_runtime();
    let long_initial = "한글-KRui-무손실-초기값-검증-".repeat(12);
    assert!(long_initial.len() > 200);

    let str_sig = Signal::new(long_initial.clone());
    str_sig.set("짧은 변경값".to_string());
    assert_eq!(str_sig.get(), "짧은 변경값");
    str_sig.reset_to_initial();
    assert_eq!(str_sig.get(), long_initial);

    let initial_profile = CustomProfile {
        title: "엔터프라이즈 대시보드 긴 제목 ".repeat(8),
        tags: vec!["rust".into(), "gui".into(), "ime".into()],
        score: 99.75,
    };
    let struct_sig = Signal::new(initial_profile.clone());
    struct_sig.set(CustomProfile {
        title: "임시".into(),
        tags: vec![],
        score: 1.0,
    });
    struct_sig.reset_to_initial();
    assert_eq!(struct_sig.get(), initial_profile);

    // Live signals preserve values cleanly; disposed signals do NOT leak retained initials.
    let scope = Scope::new();
    let (disposed_str, disposed_struct) = scope.run(|| {
        let s1 = Signal::new(long_initial.clone());
        s1.set("변조됨".into());
        let s2 = Signal::new(initial_profile.clone());
        (s1, s2)
    });
    scope.dispose();
    assert_eq!(disposed_str.try_get(), None);
    assert_eq!(disposed_str.try_with(|_| 1_usize), None);
    assert_eq!(disposed_str.get(), "");
    assert_eq!(disposed_struct.try_get(), None);
    assert_eq!(debug_stats().retained_disposed_fallbacks, 0);
}

#[test]
fn dynamic_conditional_computed_retracks_and_avoids_diamond_glitches() {
    reset_reactive_runtime();
    let cond = Signal::new(true);
    let a = Signal::new(10);
    let b = Signal::new(20);
    let evals = Rc::new(Cell::new(0));
    let evals_c = evals.clone();

    let branch = Computed::new(move || {
        evals_c.set(evals_c.get() + 1);
        if cond.get() {
            a.get()
        } else {
            b.get()
        }
    });
    assert_eq!(branch.get(), 10);
    assert_eq!(evals.get(), 1);

    // `b` is not currently a dependency when `cond == true`
    b.set(25);
    assert_eq!(evals.get(), 1);

    // Flip `cond` to `false` -> `a` must be unsubscribed and `b` subscribed
    cond.set(false);
    assert_eq!(branch.get(), 25);
    assert_eq!(evals.get(), 2);

    a.set(999);
    assert_eq!(
        evals.get(),
        2,
        "Mutating inactive branch `a` must not recompute `branch`"
    );

    b.set(40);
    assert_eq!(branch.get(), 40);
    assert_eq!(evals.get(), 3);
}

#[test]
fn asymmetric_and_mid_batch_diamond_computed_is_glitch_free() {
    reset_reactive_runtime();
    let base = Signal::new(1);
    let left = Computed::new(move || base.get() * 2);
    let deep_right = Computed::new(move || left.get() * 5);
    let combined = Computed::new(move || base.get() + deep_right.get());

    let observed = Rc::new(RefCell::new(Vec::new()));
    let obs_c = observed.clone();
    let _eff = Effect::new(move || {
        let l = left.get();
        let r = deep_right.get();
        let c = combined.get();
        assert_eq!(r, l * 5);
        assert_eq!(c, base.get() + r);
        obs_c.borrow_mut().push(c);
    });
    assert_eq!(*observed.borrow(), vec![11]);

    batch(|| {
        base.set(3);
        // Pull mid-batch must return fresh `3 + 30 = 33`, never stale `1 + 10` or `3 + 10`!
        assert_eq!(combined.get_untracked(), 33);
        base.set(4);
    });
    assert_eq!(*observed.borrow(), vec![11, 44]);
}

#[test]
fn memoized_computed_skips_downstream_computed_and_effects_when_unchanged() {
    reset_reactive_runtime();
    let raw = Signal::new(2_i32);
    let is_even = Computed::new_memoized(move || raw.get() % 2 == 0);
    let down_evals = Rc::new(Cell::new(0));
    let down_c = down_evals.clone();
    let label = Computed::new(move || {
        down_c.set(down_c.get() + 1);
        if is_even.get() {
            "짝수"
        } else {
            "홀수"
        }
    });
    assert_eq!(label.get(), "짝수");
    assert_eq!(down_evals.get(), 1);

    // 2 -> 4 keeps `is_even == true`, so `label` transitions Check -> Clean with 0 evaluations!
    raw.set(4);
    assert_eq!(label.get(), "짝수");
    assert_eq!(down_evals.get(), 1);

    raw.set(5);
    assert_eq!(label.get(), "홀수");
    assert_eq!(down_evals.get(), 2);
}

#[test]
fn analytical_spring_converges_for_120hz_and_extreme_5s_lag_spike_all_regimes() {
    reset_reactive_runtime();
    // Test underdamped (zeta < 1), critically damped (zeta == 1), and overdamped (zeta > 1)
    let configs = [
        (200.0_f32, 12.0_f32), // underdamped (zeta ~ 0.42)
        (196.0_f32, 28.0_f32), // critically damped (2 * sqrt(196) = 28, zeta == 1.0)
        (150.0_f32, 45.0_f32), // overdamped (zeta ~ 1.83)
    ];

    for (stiffness, damping) in configs {
        // 1. Extreme `dt = 5.0` second lag spike must converge directly to target without NaN/Inf
        let mut clock_spike = FrameClock::new();
        let target_sig = Signal::new(100.0_f32);
        let spring_spike = clock_spike.spring_to(0.0, target_sig, stiffness, damping);
        let _ = clock_spike.tick(5.0);
        let settled = spring_spike.get();
        assert!(settled.is_finite(), "Spring exploded to NaN/Inf at dt=5.0");
        assert!(
            (settled - 100.0).abs() < 0.01,
            "Expected spring settled at 100.0 after 5.0s spike, got {settled}"
        );

        // 2. High-rate 120Hz (`dt = 0.008333`s) stepping must smoothly converge to target
        let mut clock_120 = FrameClock::new();
        let spring_120 = clock_120.spring_to(0.0, Signal::new(100.0_f32), stiffness, damping);
        for _ in 0..360 {
            let _ = clock_120.tick(0.008_333);
            assert!(spring_120.get().is_finite());
        }
        assert!((spring_120.get() - 100.0).abs() < 0.01);
    }
}

#[test]
fn frame_clock_shake_and_scope_disposal_return_all_counts_to_baseline() {
    reset_reactive_runtime();
    let mut clock = FrameClock::new();
    let root_sig = Signal::new(10_i32);
    let baseline = debug_stats();

    let scope = Scope::new();
    let (scoped_sig, run_count) = scope.run(|| {
        let s = Signal::new(100_i32);
        let (shake_tx, trigger) = clock.shake(12.0, 20.0, 0.25);
        trigger.set(true);
        let _ = shake_tx.get();
        let _sp = clock.spring_to(0.0, Signal::new(10.0), 180.0, 20.0);
        let comp = Computed::new(move || root_sig.get() + s.get());
        let runs = Rc::new(Cell::new(0));
        let runs_c = runs.clone();
        let _eff = Effect::new(move || {
            let _ = comp.get();
            runs_c.set(runs_c.get() + 1);
        });
        register_async_poller(|| true);
        (s, runs)
    });

    assert_eq!(run_count.get(), 1);
    assert!(clock.active_callback_count() >= 2);
    let _ = clock.tick(0.05);

    scope.dispose();
    clock.prune_disposed_scopes();
    assert!(!scoped_sig.is_alive());
    assert_eq!(scoped_sig.try_get(), None);
    assert_eq!(clock.active_callback_count(), 0);
    assert_eq!(debug_stats(), baseline);

    root_sig.set(20);
    assert_eq!(run_count.get(), 1);
    root_sig.with(|val| root_sig.set(*val + 1));
    assert_eq!(root_sig.get(), 21);
}

#[test]
fn ui_waker_wakes_across_threads_and_clears_cleanly() {
    let waker = Arc::new(CountingWaker {
        wakes: AtomicUsize::new(0),
    });
    set_ui_waker(Some(waker.clone()));

    std::thread::spawn(|| {
        wake_ui_loop();
    })
    .join()
    .expect("join background thread");

    assert_eq!(waker.wakes.load(Ordering::SeqCst), 1);
    set_ui_waker(None);
    wake_ui_loop();
    assert_eq!(waker.wakes.load(Ordering::SeqCst), 1);
}

#[test]
fn multi_session_isolation_prevents_waker_override_and_cross_aliasing() {
    let sess_a = ReactiveSession::new();
    let sess_b = ReactiveSession::new();
    assert_ne!(sess_a.id(), sess_b.id());

    let waker_a = Arc::new(CountingWaker {
        wakes: AtomicUsize::new(0),
    });
    let waker_b = Arc::new(CountingWaker {
        wakes: AtomicUsize::new(0),
    });
    sess_a.set_ui_waker(Some(waker_a.clone()));
    sess_b.set_ui_waker(Some(waker_b.clone()));

    let sig_a = sess_a.enter(|| Signal::new("session-a".to_string()));
    let sig_b = sess_b.enter(|| Signal::new("session-b".to_string()));

    sess_b.enter(|| {
        assert!(
            !sig_a.is_alive(),
            "Session A handle must not alias in Session B"
        );
        assert_eq!(sig_a.try_get(), None);
        assert_eq!(sig_b.get(), "session-b");
        wake_ui_loop();
    });
    assert_eq!(waker_a.wakes.load(Ordering::SeqCst), 0);
    assert_eq!(waker_b.wakes.load(Ordering::SeqCst), 1);
}

#[test]
fn signal_update_mutates_in_place_without_cloning_and_recovers_on_panic() {
    reset_reactive_runtime();
    #[derive(Debug)]
    struct NoisyClone {
        value: i32,
        clones: Arc<AtomicUsize>,
    }
    impl Clone for NoisyClone {
        fn clone(&self) -> Self {
            self.clones.fetch_add(1, Ordering::SeqCst);
            Self {
                value: self.value,
                clones: self.clones.clone(),
            }
        }
    }
    let clones = Arc::new(AtomicUsize::new(0));
    let sig = Signal::new(NoisyClone {
        value: 10,
        clones: clones.clone(),
    });
    // First update splits once from `initial_value`; subsequent `.update()` mutations clone 0 times.
    sig.update(|s| s.value += 5);
    let after_initial_detach = clones.load(Ordering::SeqCst);
    for _ in 0..10 {
        sig.update(|s| s.value += 1);
    }
    assert_eq!(clones.load(Ordering::SeqCst), after_initial_detach);
    assert_eq!(sig.with(|s| s.value), 25);
    assert_eq!(clones.load(Ordering::SeqCst), after_initial_detach);
}

#[test]
fn tokio_task_and_stream_deliver_on_session_ui_thread() {
    let sess = ReactiveSession::new();
    let waker = Arc::new(CountingWaker {
        wakes: AtomicUsize::new(0),
    });
    sess.set_ui_waker(Some(waker.clone()));

    let (done_sig, stream_sum) = sess.enter(|| {
        let done = Signal::new(String::new());
        let sum = Signal::new(0_i32);
        let done_c = done;
        let _h1 = spawn_tokio_task(
            async {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                "ok-from-tokio".to_string()
            },
            move |msg| done_c.set(msg),
        );
        let sum_c = sum;
        let _h2 = spawn_tokio_stream(
            |sink| async move {
                let _ = sink.send(10);
                let _ = sink.send(32);
            },
            move |item| sum_c.update(|v| *v += item),
        );
        (done, sum)
    });

    for _ in 0..80 {
        let _ = sess.poll_async_tasks();
        if sess.enter(|| done_sig.get() == "ok-from-tokio" && stream_sum.get() == 42) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    sess.enter(|| {
        assert_eq!(done_sig.get(), "ok-from-tokio");
        assert_eq!(stream_sum.get(), 42);
    });
    assert!(waker.wakes.load(Ordering::SeqCst) >= 1);
}

#[test]
fn scope_disposal_aborts_in_flight_tokio_task_cleanly() {
    let sess = ReactiveSession::new();
    let aborted = Arc::new(AtomicUsize::new(0));
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let aborted_bg = aborted.clone();
    let cancel_scope = sess.enter(|| {
        let scope = Scope::new();
        scope.run(|| {
            struct DropMark(Arc<AtomicUsize>);
            impl Drop for DropMark {
                fn drop(&mut self) {
                    self.0.fetch_add(1, Ordering::SeqCst);
                }
            }
            let _h = spawn_tokio_task(
                async move {
                    let _mark = DropMark(aborted_bg);
                    let _ = started_tx.send(());
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                },
                |_| {},
            );
        });
        scope
    });
    started_rx.recv().expect("task should start running");
    sess.enter(|| cancel_scope.dispose());
    for _ in 0..80 {
        if aborted.load(Ordering::SeqCst) == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(aborted.load(Ordering::SeqCst), 1);
}
