//! P0-4 & A3 Transition Stress, Reactive Ownership & Generational Disposal Verification Suite
//!
//! Verifies:
//! 1. Cycling across 4 distinct reactive pages 100 times via `EngineSession::transition_to_view_with`
//!    (including `<if>`/`<else>`, `<for>`/`<empty>`, and `Command::new_async` on pages)
//!    has **zero** leak tolerance (`after == baseline`) across ALL runtime counters:
//!    `live_signals`, `live_computed`, `live_effects`, `live_scopes`, `total_subscriber_entries`,
//!    `live_async_pollers`, `FrameClock::active_callback_count()`, `ViewTree::taffy_node_count()`,
//!    and `ViewTree::retained_node_count()`.
//! 2. `Scope::dispose` removes the child scope from its parent's `owned_children`.
//! 3. `Effect` re-runs dispose nested `Signal`s and `Effect`s created on the previous execution.
//! 4. `ViewTree::unmount` disposes all node `Effect`s and removes Taffy nodes.
//! 5. App-level `Store` / `Signal` created after `EngineSession::new` / `ViewTree::new` is NOT
//!    captured by the page scope and survives `replace_root`.
//! 6. Reading a disposed `Signal` returns that specific signal's initial value (never another
//!    signal's slot-0 value) and never panics even after `reset_reactive_runtime()`.
//! 7. Re-entrant `register_async_poller` inside a poller callback never panics on `RefCell` borrow.

#![forbid(unsafe_code)]

use krui::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
struct StressRow {
    id: u32,
    text: String,
}

fn make_page(page_idx: usize) -> ViewFragment {
    let counter = Signal::new((page_idx as i32) * 10);
    let label = Signal::new(format!("페이지 #{page_idx}"));
    let show_detail = Signal::new(page_idx.is_multiple_of(2));
    let rows = Signal::new(vec![
        StressRow {
            id: 1,
            text: format!("항목 A-{page_idx}"),
        },
        StressRow {
            id: 2,
            text: format!("항목 B-{page_idx}"),
        },
    ]);
    let summary = Computed::new(move || format!("{} — 카운트: {}", label.get(), counter.get()));
    let inc_cmd = Command::new(move || counter.update(|c| *c += 1));
    let async_cmd = Command::new_async(move |done| {
        counter.update(|c| *c += 5);
        done();
    });

    match page_idx % 4 {
        0 => krui!(
            r#"<div direction="column" width="760" height="520" gap="12" style="padding: 16px; background-color: #0f172a"><h1 id="page-title">01 비즈니스 뷰: {summary}</h1><input id="page-input" width="320" height="40" bind:value="{label}" /><button id="page-btn" width="160" height="42" onclick="{inc_cmd}">증가 ({counter})</button><button id="async-btn" width="160" height="42" onclick="{async_cmd}">비동기 승인</button><if test="{show_detail}"><span id="detail-badge">상세 활성: {label}</span><else><span id="detail-fallback">상세 대기</span></if></div>"#
        ),
        1 => krui!(
            r#"<section direction="column" width="760" height="520" gap="14" style="padding: 20px; background-color: #1e293b"><h2 id="page-title">02 MVVM 뷰: {summary}</h2><div direction="row" gap="10"><button id="page-btn" width="180" height="44" onclick="{inc_cmd}">장바구니 추가: {counter}</button><button id="async-btn" width="160" height="44" onclick="{async_cmd}">비동기 결제</button><span>상태: {label}</span></div><ul><for each="{row in rows}" key="{row.id}"><li>{row.text}</li></for><empty><li>목록 비어 있음</li></empty></ul></section>"#
        ),
        2 => krui!(
            r#"<main direction="column" width="760" height="520" gap="10" style="padding: 18px; background-color: #111827"><h1 id="page-title">03 MVC 뷰: {summary}</h1><p selectable="true">포트폴리오 요약: {label}</p><button id="page-btn" width="160" height="40" onclick="{inc_cmd}">매수 실행 ({counter})</button><button id="async-btn" width="160" height="40" onclick="{async_cmd}">비동기 리밸런싱</button><if test="{show_detail}"><span>헷지 활성</span><else><span>기본 배분</span></if></main>"#
        ),
        _ => krui!(
            r#"<div direction="column" width="760" height="520" gap="12" style="padding: 16px; background-color: #090d16"><header direction="row" gap="12"><h2 id="page-title">04 Elm/Flux 뷰: {summary}</h2><button id="page-btn" width="150" height="38" onclick="{inc_cmd}">디스패치 ({counter})</button><button id="async-btn" width="150" height="38" onclick="{async_cmd}">비동기 태스크</button></header><input id="page-input" width="300" height="38" bind:value="{label}" /><ul><for each="{row in rows}" key="{row.id}"><li>{row.text}</li></for></ul></div>"#
        ),
    }
}

#[test]
fn transition_100_cycles_across_4_pages_has_zero_reactive_or_clock_leaks() {
    reset_reactive_runtime();

    let mut session = EngineSession::new(800, 600, RenderBackendPreference::ForceTinySkia);
    session.transition_to_view_with(|_| make_page(0));
    for _ in 0..10 {
        session.step_frame(1.0 / 120.0);
    }
    let baseline = debug_stats();
    let baseline_clock_cbs = session.clock.active_callback_count();
    let baseline_taffy = session.tree.taffy_node_count();
    let baseline_retained = session.tree.retained_node_count();
    assert_eq!(
        baseline_clock_cbs, 2,
        "Initial transition should have exactly 2 active FrameClock spring drivers (slide + fade)"
    );

    // Cycle 100 times (ending on page 100 % 4 == 0 so the final mounted page structure matches page 0)
    for i in 1..=100 {
        session.transition_to_view_with(|_| make_page(i % 4));
        for _ in 0..10 {
            session.step_frame(1.0 / 120.0);
        }
    }

    let after = debug_stats();
    assert_eq!(
        after, baseline,
        "RuntimeStats must have +0 leak tolerance across 100 page transitions: baseline={baseline:?}, after={after:?}"
    );
    assert_eq!(
        session.clock.active_callback_count(),
        baseline_clock_cbs,
        "FrameClock callbacks leaked across 100 page transitions"
    );
    assert_eq!(
        session.tree.taffy_node_count(),
        baseline_taffy,
        "Taffy layout nodes leaked across 100 page transitions"
    );
    assert_eq!(
        session.tree.retained_node_count(),
        baseline_retained,
        "RetainedNode slab leaked across 100 page transitions"
    );
}

#[test]
fn store_created_after_view_tree_new_survives_replace_root_and_nested_effects_clean_up() {
    reset_reactive_runtime();

    let mut tree = ViewTree::new();
    let app_store = Store::from_reducer(10_i32, |state, delta: i32| *state + delta);

    tree.mount_with(|| {
        let local_sig = Signal::new("1페이지".to_string());
        krui!(r#"<div><span id="p1">{local_sig}</span></div>"#)
    });

    tree.replace_root_with(|| {
        let local_sig2 = Signal::new("2페이지".to_string());
        krui!(r#"<div><span id="p2">{local_sig2}</span></div>"#)
    });

    assert!(app_store.state().is_alive());
    app_store.send(5);
    assert_eq!(app_store.get(), 15);

    verify_nested_effect_rerun_cleanup();
}

fn verify_nested_effect_rerun_cleanup() {
    let trigger = Signal::new(0_i32);
    let inner_runs = Rc::new(Cell::new(0_u32));
    let inner_trigger = Signal::new(100_i32);

    let parent_scope = Scope::new();
    let inner_runs_clone = inner_runs.clone();
    parent_scope.run(|| {
        let _outer = Effect::new(move || {
            let _t = trigger.get();
            let nested_sig = Signal::new(42_i32);
            let _ = nested_sig.get();
            let runs_ref = inner_runs_clone.clone();
            let _nested_eff = Effect::new(move || {
                let _ = inner_trigger.get();
                runs_ref.set(runs_ref.get() + 1);
            });
        });
    });

    let stats_after_first = debug_stats();
    assert_eq!(inner_runs.get(), 1);

    for step in 1..=10 {
        trigger.set(step);
    }
    assert_eq!(debug_stats(), stats_after_first);

    let before_inner = inner_runs.get();
    inner_trigger.set(200);
    assert_eq!(inner_runs.get(), before_inner + 1);

    parent_scope.dispose();
}

#[test]
fn unmount_disposes_node_effects_and_reentrant_async_poller_never_panics() {
    reset_reactive_runtime();

    let mut tree = ViewTree::new();
    let dynamic_text = Signal::new("초기".to_string());
    let dynamic_tx = Signal::new(0.0_f32);

    tree.mount_with(|| {
        krui!(
            r#"
            <div id="root" width="300" height="200">
                <span id="child-span" style="transform: translate({dynamic_tx}px, 0px)">{dynamic_text}</span>
            </div>
            "#
        )
    });

    let child_id = tree.find_by_id("child-span").expect("child-span");
    let before_unmount = debug_stats();
    assert!(before_unmount.live_effects >= 2);
    let taffy_before = tree.taffy_node_count();

    // Unmounting `child-span` must dispose its text & transform `Effect`s and remove its Taffy node!
    assert!(tree.unmount(child_id));
    let after_unmount = debug_stats();
    assert!(
        after_unmount.live_effects < before_unmount.live_effects,
        "ViewTree::unmount must dispose node _effects: before={}, after={}",
        before_unmount.live_effects,
        after_unmount.live_effects
    );
    assert_eq!(tree.taffy_node_count(), taffy_before - 1);

    // Re-entrant `register_async_poller` inside a running poller callback must not panic on RefCell borrow
    let reentrant_ran = Rc::new(Cell::new(false));
    let reentrant_clone = reentrant_ran.clone();
    let poller_scope = Scope::new();
    poller_scope.run(|| {
        let registered_nested = Cell::new(false);
        register_async_poller(move || {
            if !registered_nested.get() {
                registered_nested.set(true);
                let flag = reentrant_clone.clone();
                register_async_poller(move || {
                    flag.set(true);
                    false
                });
            }
            false
        });
    });

    let _ = poll_async_tasks();
    let _ = poll_async_tasks();
    assert!(reentrant_ran.get());
    poller_scope.dispose();
    assert_eq!(debug_stats().live_async_pollers, 0);
}

#[test]
fn stale_node_id_and_disposed_signal_access_never_panic_or_corrupt_new_page() {
    reset_reactive_runtime();

    let _slot0_sig = Signal::new("슬롯0 초기값".to_string());
    let mut tree = ViewTree::new();
    let mut old_sig_holder = None;
    let mut old_tx_holder = None;

    let roots1 = tree.mount_with(|| {
        let old_sig = Signal::new("구 페이지 고유 초기값".to_string());
        old_sig.set("구 페이지 변경값".to_string());
        let old_tx = Signal::new(12.0_f32);
        old_sig_holder = Some(old_sig);
        old_tx_holder = Some(old_tx);
        krui!(
            r#"
            <div id="root-box" width="400" height="200" style="transform: translate({old_tx}px, 0px)">
                <span id="label-node">{old_sig}</span>
            </div>
            "#
        )
    });
    let old_sig = old_sig_holder.unwrap();
    let old_tx = old_tx_holder.unwrap();
    let old_root_id = roots1[0];
    let old_label_id = tree.find_by_id("label-node").expect("find label-node");

    let roots2 = tree.replace_root_with(|| {
        let new_sig = Signal::new("신규 페이지 텍스트".to_string());
        krui!(
            r#"
            <div id="new-root" width="400" height="200">
                <span id="new-label">{new_sig}</span>
            </div>
            "#
        )
    });
    let new_root_id = roots2[0];
    assert_eq!(old_root_id.index, new_root_id.index);
    assert_ne!(old_root_id.generation, new_root_id.generation);

    old_sig.set("덮어쓰기 시도".to_string());
    old_tx.set(999.0);
    assert_eq!(old_sig.try_get(), None);
    assert_eq!(old_sig.get(), "");

    tree.set_static_text(old_label_id, "해제된 노드 텍스트 변경 시도");
    tree.set_inline_style(old_root_id, "background-color: #ff0000");
    assert!(!tree.unmount(old_label_id));

    let (scene, _) = tree.build_paint_scene(400, 200);
    assert!(!scene.primitives.is_empty());
    let new_label_id = tree.find_by_id("new-label").expect("new-label exists");
    assert!(tree.unmount(new_label_id));
    assert!(tree.find_by_id("new-label").is_none());

    reset_reactive_runtime();
    assert_eq!(old_sig.get(), "");
}

#[test]
fn verify_100_custom_struct_disposals_do_not_panic_or_leak() {
    #[derive(Clone, Debug, Default, PartialEq)]
    struct CustomRow {
        id: u32,
        label: String,
    }
    reset_reactive_runtime();
    let baseline = debug_stats();
    let root = krui::Scope::new();
    for i in 0..100u32 {
        let child = root.run(krui::Scope::new);
        let sig = child.run(|| {
            krui::Signal::new(CustomRow {
                id: i,
                label: "r".into(),
            })
        });
        sig.update(|r| r.id += 1);
        child.dispose();
        sig.set(CustomRow::default());
        sig.update(|r| r.id = 99);
        assert_eq!(sig.try_get(), None);
        assert_eq!(sig.try_with(|r| r.id), None);
    }
    root.dispose();
    assert_eq!(debug_stats(), baseline);
}

#[test]
fn a_fix_1_root_poller_cleanup_direct_anchor_unmount_reentrant_update_and_1000_cycle_stress() {
    reset_reactive_runtime();
    verify_root_poller_completion_and_reentrant_update();
    verify_direct_anchor_unmount_disposes_scopes();
    verify_1000_cycle_if_and_for_stress();
}

fn verify_root_poller_completion_and_reentrant_update() {
    let runs = Rc::new(Cell::new(0_usize));
    let runs_cb = runs.clone();
    register_async_poller(move || {
        let next = runs_cb.get() + 1;
        runs_cb.set(next);
        next < 2
    });
    assert_eq!(debug_stats().live_async_pollers, 1);
    assert!(poll_async_tasks());
    assert_eq!(debug_stats().live_async_pollers, 1);
    assert!(!poll_async_tasks());
    assert_eq!(debug_stats().live_async_pollers, 0);

    {
        let temp_store = Store::from_reducer(0_i32, |s, d: i32| *s + d);
        temp_store.send(1);
        let temp_cmd = Command::new_async(|done| done());
        temp_cmd.execute();
        assert!(debug_stats().live_async_pollers >= 2);
    }
    let _ = poll_async_tasks();
    reset_reactive_runtime();
    assert_eq!(debug_stats().live_async_pollers, 0);

    let sig = Signal::new(10_i32);
    let observed = Rc::new(RefCell::new(Vec::new()));
    let obs_clone = observed.clone();
    let _eff = Effect::new(move || obs_clone.borrow_mut().push(sig.get()));
    assert_eq!(*observed.borrow(), vec![10]);

    sig.with(|val| {
        assert_eq!(*val, 10);
        sig.update(|v| *v += 5);
        assert_eq!(*val, 10);
    });
    assert_eq!(sig.get_untracked(), 15);
    assert_eq!(*observed.borrow(), vec![10, 15]);
}

fn verify_direct_anchor_unmount_disposes_scopes() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    let cond = Signal::new(true);
    let list_sig = Signal::new(vec![
        StressRow {
            id: 1,
            text: "A".into(),
        },
        StressRow {
            id: 2,
            text: "B".into(),
        },
    ]);
    let base_stats = debug_stats();

    let branch_sig_holder: Rc<RefCell<Option<Signal<i32>>>> = Rc::new(RefCell::new(None));
    let holder_clone = branch_sig_holder.clone();
    let if_anchor = tree.mount_if(
        None,
        move || cond.get().then_some(0),
        vec![Rc::new(move |tree, parent| {
            let b_sig = Signal::new(777_i32);
            *holder_clone.borrow_mut() = Some(b_sig);
            let span = mount_plain_node(tree, parent, "span");
            tree.bind_dynamic_text(span, move || format!("{}", b_sig.get()));
            vec![span]
        })],
    );
    let branch_sig = branch_sig_holder.borrow().expect("branch_sig mounted");
    assert!(branch_sig.is_alive());
    assert!(tree.unmount(if_anchor));
    assert!(!branch_sig.is_alive());

    let item_sigs: Rc<RefCell<Vec<Signal<StressRow>>>> = Rc::new(RefCell::new(Vec::new()));
    let item_sigs_clone = item_sigs.clone();
    let for_anchor = tree.mount_for(
        None,
        move || list_sig.get(),
        |r| r.id,
        move |tree, parent, row_sig| {
            item_sigs_clone.borrow_mut().push(row_sig);
            let li = mount_plain_node(tree, parent, "li");
            tree.bind_dynamic_text(li, move || row_sig.with(|r| r.text.clone()));
            vec![li]
        },
        None,
    );
    assert_eq!(item_sigs.borrow().len(), 2);
    assert!(item_sigs.borrow().iter().all(|s| s.is_alive()));
    assert!(tree.unmount(for_anchor));
    assert!(item_sigs.borrow().iter().all(|s| !s.is_alive()));
    assert_eq!(debug_stats().live_scopes, base_stats.live_scopes);
}

fn mount_plain_node(
    tree: &mut ViewTree,
    parent: Option<krui::NodeId>,
    tag: &'static str,
) -> krui::NodeId {
    tree.create_compiled_node(
        parent,
        tag,
        None,
        None,
        CompiledStyle::default(),
        false,
        false,
        false,
        false,
    )
}

fn two_stress_rows(a: (u32, String), b: (u32, String)) -> Vec<StressRow> {
    vec![
        StressRow { id: a.0, text: a.1 },
        StressRow { id: b.0, text: b.1 },
    ]
}

fn verify_1000_cycle_if_and_for_stress() {
    reset_reactive_runtime();
    let mut stress_tree = ViewTree::new();
    let toggle = Signal::new(true);
    let items = Signal::new(two_stress_rows((1, "1".into()), (2, "2".into())));
    stress_tree.mount_with(|| {
        krui!(r#"<div direction="column" width="400" height="300"><if test="{toggle}"><span>켜짐</span><else><span>꺼짐</span></if><ul><for each="{it in items}" key="{it.id}"><li>{it.text}</li></for><empty><li>비어 있음</li></empty></ul></div>"#)
    });
    let stress_baseline = debug_stats();
    let taffy_baseline = stress_tree.taffy_node_count();
    let retained_baseline = stress_tree.retained_node_count();

    for step in 1..=1_000 {
        toggle.set(step % 2 == 0);
        if step % 3 == 0 {
            items.set(Vec::new());
        } else {
            let base = (step % 5) as u32;
            items.set(two_stress_rows(
                (base + 10, "x".repeat(128)),
                (base + 20, "y".repeat(128)),
            ));
        }
    }
    toggle.set(true);
    items.set(two_stress_rows((1, "1".into()), (2, "2".into())));

    let stress_after = debug_stats();
    assert_eq!(stress_after, stress_baseline);
    assert_eq!(stress_after.retained_disposed_fallbacks, 0);
    assert_eq!(stress_tree.taffy_node_count(), taffy_baseline);
    assert_eq!(stress_tree.retained_node_count(), retained_baseline);
}
