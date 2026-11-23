#![forbid(unsafe_code)]

use krui::*;

fn first_box_width_for_height(scene: &PaintScene, approx_h: f32) -> f32 {
    scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox { rect, .. } if (rect.height - approx_h).abs() < 1.5 => {
                Some(rect.width)
            }
            _ => None,
        })
        .expect("expected StyledBox with matching height")
}

#[test]
fn more_than_four_clauses_list_cycling_and_negative_delay_work() {
    reset_reactive_runtime();
    let active = Signal::new(false);
    let mut sess = EngineSession::new(480, 320, RenderBackendPreference::CpuFallback);
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                #box {
                    width: 100px; height: 40px;
                    opacity: 0.0; transform: translateX(0px);
                    background-color: #000000; color: #000000;
                    border: 2px solid #000000; border-radius: 0px;
                    transition-property: opacity, transform, background-color, color, border-color, border-radius;
                    transition-duration: 200ms, 400ms;
                    transition-timing-function: linear;
                    transition-delay: -100ms, 0ms;
                }
                #box.on-true {
                    opacity: 1.0; transform: translateX(100px);
                    background-color: #ffffff; color: #ffffff;
                    border-color: #ff0000; border-radius: 20px;
                }
            </style>
            <div id="box" class="on-{active}"></div>
            "#
        )
    });
    let _ = sess.step_frame(0.0);
    let rel0 = sess.tree.relayout_passes();
    let id = sess.tree.find_by_id("box").expect("box");
    active.set(true);

    // At elapsed = 0ms: even indices (0: opacity, 4: border-color) use -100ms delay / 200ms duration -> 50% immediately!
    let p0 = sess.tree.node_effective_paint(id).expect("paint t=0");
    assert!((p0.opacity - 0.5).abs() < 0.05, "p0.opacity={}", p0.opacity);
    assert!(
        p0.border_color.r > 100 && p0.border_color.r < 155,
        "p0.bc.r={}",
        p0.border_color.r
    );

    // Advance 60ms: odd-index 6th clause `border-radius` (400ms, 0ms delay) is at 15% (~3px), proving >4 clauses!
    let _ = sess.tree.tick_transitions(0.06);
    let p_mid = sess.tree.node_effective_paint(id).expect("paint mid");
    assert!(
        (p_mid.border_radius - 3.0).abs() < 0.8,
        "p_mid.radius={}",
        p_mid.border_radius
    );
    assert_eq!(sess.tree.relayout_passes(), rel0);
}

#[test]
fn reversing_shortening_factor_scales_mid_flight_hover_return() {
    reset_reactive_runtime();
    let mut sess = EngineSession::new(320, 200, RenderBackendPreference::CpuFallback);
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                #rev {
                    width: 100px; height: 40px; opacity: 1.0;
                    background-color: #334455;
                    transition: opacity 1000ms linear;
                }
                #rev:hover { opacity: 0.0; }
            </style>
            <button id="rev">Hover</button>
            "#
        )
    });
    let _ = sess.step_frame(0.0);
    let id = sess.tree.find_by_id("rev").expect("rev");
    let rect = sess.tree.node_layout(id).expect("rect");

    // Hover #rev -> advance 300ms (opacity reaches 0.7).
    let _ = sess
        .tree
        .dispatch_pointer_move(rect.x + 10.0, rect.y + 10.0);
    let _ = sess.tree.tick_transitions(0.30);
    let p1 = sess.tree.node_effective_paint(id).expect("p1");
    assert!(
        (p1.opacity - 0.7).abs() < 0.03,
        "expected ~0.7, got {}",
        p1.opacity
    );

    // Move away mid-flight -> W3C shortening factor = 0.3, so return finishes in 300ms (not 1000ms!).
    let _ = sess
        .tree
        .dispatch_pointer_move(rect.x + 200.0, rect.y + 150.0);
    let _ = sess.tree.tick_transitions(0.29);
    let p2 = sess.tree.node_effective_paint(id).expect("p2");
    assert!(
        p2.opacity > 0.95 && sess.tree.active_transition_count() == 1,
        "op={}",
        p2.opacity
    );
    let _ = sess.tree.tick_transitions(0.02);
    assert_eq!(sess.tree.active_transition_count(), 0);
    let p3 = sess.tree.node_effective_paint(id).expect("p3");
    assert!((p3.opacity - 1.0).abs() < 1e-3);
}

#[test]
fn zero_alpha_shadow_stack_preserves_layer_indices_during_transition() {
    reset_reactive_runtime();
    let active = Signal::new(false);
    let mut sess = EngineSession::new(320, 200, RenderBackendPreference::CpuFallback);
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                #ring {
                    width: 120px; height: 48px; background-color: #ffffff;
                    box-shadow: 0 0 0 0 rgba(59, 130, 246, 0), 0 4px 10px 0 rgba(0, 0, 0, 0.4);
                    transition: box-shadow 200ms linear;
                }
                #ring.ring-true {
                    box-shadow: 0 0 0 4px rgba(59, 130, 246, 1), 0 8px 16px 0 rgba(0, 0, 0, 0.8);
                }
            </style>
            <button id="ring" class="ring-{active}">Ring</button>
            "#
        )
    });
    let _ = sess.step_frame(0.0);
    let id = sess.tree.find_by_id("ring").expect("ring");
    active.set(true);
    let _ = sess.tree.tick_transitions(0.10);
    let mid = sess.tree.node_effective_paint(id).expect("mid paint");
    assert_eq!(mid.shadow_stack.len, 2);
    let ring_layer = mid.shadow_stack.layers[0];
    let drop_layer = mid.shadow_stack.layers[1];
    assert!(
        (ring_layer.spread_radius - 2.0).abs() < 0.25,
        "spread={}",
        ring_layer.spread_radius
    );
    assert!(
        ring_layer.color.a > 100 && ring_layer.color.a < 160,
        "alpha={}",
        ring_layer.color.a
    );
    assert!(
        (drop_layer.blur_radius - 13.0).abs() < 0.5,
        "blur={}",
        drop_layer.blur_radius
    );
}

#[test]
fn tier2_layout_transition_interpolates_width_locally_while_tier1_has_zero_relayouts() {
    reset_reactive_runtime();
    let wide = Signal::new(false);
    let mut sess = EngineSession::new(480, 240, RenderBackendPreference::CpuFallback);
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                /* @krui-ignore-warnings */
                #row { display: flex; flex-direction: row; width: 480px; height: 80px; }
                #t2 {
                    width: 100px; height: 48px; background-color: #223344;
                    transition: width 300ms linear;
                }
                #t2.wide-true { width: 220px; }
                #sib { width: 60px; height: 48px; background-color: #556677; }
            </style>
            <div id="row">
                <div id="t2" class="wide-{wide}"></div>
                <div id="sib"></div>
            </div>
            "#
        )
    });
    let _ = sess.step_frame(0.0);
    let sib_id = sess.tree.find_by_id("sib").expect("sib");
    let sib_x0 = sess.tree.node_layout(sib_id).expect("sib0").x;
    wide.set(true);

    // Mid-flight at 150ms -> width ~= 160px and sibling shifts right!
    let _ = sess.tree.tick_transitions(0.15);
    let (mid_scene, did_relayout) = sess.tree.build_paint_scene(480, 240);
    assert!(
        did_relayout,
        "Tier-2 width transition must dirty localized Taffy node"
    );
    let mid_w = first_box_width_for_height(&mid_scene, 48.0);
    assert!((mid_w - 160.0).abs() < 5.0, "expected ~160px, got {mid_w}");
    let sib_x_mid = sess.tree.node_layout(sib_id).expect("sib mid").x;
    assert!((sib_x_mid - (sib_x0 + 60.0)).abs() < 5.0);

    // Finish transition -> lands at 220px and settles cleanly to 0 relayouts afterward.
    let _ = sess.tree.tick_transitions(0.20);
    let _ = sess.step_frame(1.0 / 60.0);
    assert_eq!(sess.tree.active_transition_count(), 0);
    let t2_id = sess.tree.find_by_id("t2").expect("t2");
    assert!((sess.tree.node_layout(t2_id).expect("end").width - 220.0).abs() < 1.0);
    let settled_rel = sess.tree.relayout_passes();
    let _ = sess.step_frame(1.0 / 60.0);
    assert_eq!(sess.tree.relayout_passes(), settled_rel);
}

#[test]
fn keyframes_ignore_important_merge_equal_offsets_use_interval_easing_and_implicit_ends() {
    reset_reactive_runtime();
    let run = Signal::new(false);
    let mut sess = EngineSession::new(320, 200, RenderBackendPreference::CpuFallback);
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                @keyframes pulse_mid {
                    from { opacity: 1.0 !important; opacity: 0.2; }
                    0% { border-radius: 4px; animation-timing-function: steps(2, jump-end); }
                    50% { opacity: 1.0; border-radius: 12px; }
                }
                #kf {
                    width: 80px; height: 40px; opacity: 0.2; border-radius: 0px;
                    background-color: #112233;
                }
                #kf.run-true { animation: pulse_mid 400ms linear forwards; }
            </style>
            <div id="kf" class="run-{run}"></div>
            "#
        )
    });
    let _ = sess.step_frame(0.0);
    let id = sess.tree.find_by_id("kf").expect("kf");
    run.set(true);

    // At 100ms (t=0.25 overall, midpoint of [0%, 50%] with `steps(2, jump-end)` -> quantized to step 1 = 0.5).
    let _ = sess.tree.tick_transitions(0.10);
    let p_step1 = sess.tree.node_effective_paint(id).expect("step1");
    assert!(
        (p_step1.opacity - 0.6).abs() < 0.04,
        "opacity={}",
        p_step1.opacity
    );
    assert!(
        (p_step1.border_radius - 8.0).abs() < 0.5,
        "br={}",
        p_step1.border_radius
    );

    // At 300ms (t=0.75 overall, inside [50%, implicit 100%] returning toward base opacity 0.2).
    let _ = sess.tree.tick_transitions(0.20);
    let p_ret = sess.tree.node_effective_paint(id).expect("ret");
    assert!(
        (p_ret.opacity - 0.6).abs() < 0.05,
        "ret opacity={}",
        p_ret.opacity
    );
}

#[test]
fn ordered_transforms_move_hit_test_and_korean_ime_cursor_during_keyframes() {
    reset_reactive_runtime();
    let clicks = Signal::new(0_u32);
    let on_btn = move || clicks.update(|c| *c += 1);
    let mut sess = EngineSession::new(480, 320, RenderBackendPreference::CpuFallback);
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                #root { width: 480px; height: 320px; padding: 20px; gap: 12px; display: flex; flex-direction: column; }
                #ord_a { width: 100px; height: 40px; background-color: #123456; transform: translateX(-50%) rotate(90deg); }
                #ord_b { width: 100px; height: 40px; background-color: #123456; transform: rotate(90deg) translateX(-50%); }
                @keyframes slide_right {
                    from { transform: translateX(0px); }
                    to { transform: translateX(120px); }
                }
                #btn, #edt {
                    width: 120px; height: 36px; background-color: #223344; color: #ffffff;
                    animation: slide_right 200ms linear forwards;
                }
            </style>
            <div id="root">
                <div id="ord_a"></div>
                <div id="ord_b"></div>
                <button id="btn" onclick="{on_btn}">Hit</button>
                <input id="edt" value="가" />
            </div>
            "#
        )
    });
    let _ = sess.step_frame(0.0);
    let ord_a = sess
        .tree
        .node_layout(sess.tree.find_by_id("ord_a").expect("a"))
        .expect("ra");
    let ord_b = sess
        .tree
        .node_layout(sess.tree.find_by_id("ord_b").expect("b"))
        .expect("rb");
    assert!((ord_a.x - ord_b.x).abs() > 20.0 || (ord_a.y - ord_b.y).abs() > 20.0);

    let btn_id = sess.tree.find_by_id("btn").expect("btn");
    let btn0 = sess.tree.node_layout(btn_id).expect("btn0");
    let edt_id = sess.tree.find_by_id("edt").expect("edt");
    assert!(sess.tree.set_focus_to_node(edt_id));
    assert!(sess.tree.dispatch_ime_event(ImeEvent::Enabled));
    assert!(sess.tree.dispatch_ime_event(ImeEvent::Preedit {
        text: "한".into(),
        cursor_offset: Some((0, 3)),
    }));
    let ime0 = sess.tree.focused_ime_cursor_area().expect("ime0");

    // Advance 100ms (+60px translateX): Korean IME cursor and button hit_test move together!
    let _ = sess.tree.tick_transitions(0.10);
    let ime_mid = sess.tree.focused_ime_cursor_area().expect("ime mid");
    assert!(
        (ime_mid.x - (ime0.x + 60.0)).abs() < 2.0,
        "ime0.x={}, mid.x={}",
        ime0.x,
        ime_mid.x
    );
    let btn_mid = sess.tree.node_layout(btn_id).expect("btn mid");
    assert!((btn_mid.x - (btn0.x + 60.0)).abs() < 2.0);
    let _ = sess.tree.dispatch_click(btn0.x + 10.0, btn0.y + 18.0);
    assert_eq!(clicks.get(), 0);
    assert!(sess.tree.dispatch_click(btn_mid.x + 60.0, btn_mid.y + 18.0));
    assert_eq!(clicks.get(), 1);
}

#[test]
fn positive_animation_and_transition_delays_keep_idle_paint_clean_until_wake() {
    reset_reactive_runtime();
    let go = Signal::new(false);
    let mut sess = EngineSession::new(320, 200, RenderBackendPreference::CpuFallback);
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                @keyframes late_fade {
                    from { opacity: 0.2; }
                    to { opacity: 1.0; }
                }
                #delayed_kf {
                    width: 80px; height: 40px; opacity: 1.0;
                    animation: late_fade 400ms linear 2000ms forwards;
                }
                #delayed_tr {
                    width: 80px; height: 40px; opacity: 1.0;
                    transition: opacity 300ms linear 1500ms;
                }
                #delayed_tr.go-true { opacity: 0.1; }
            </style>
            <div>
                <div id="delayed_kf"></div>
                <div id="delayed_tr" class="go-{go}"></div>
            </div>
            "#
        )
    });
    let _ = sess.step_frame(0.0);
    go.set(true);
    let _ = sess.step_frame(0.0);
    assert!(!sess.tree.is_paint_dirty());

    // Advance 500ms: both tracks remain inside positive delay -> paint_dirty stays false!
    let active = sess.tree.tick_transitions(0.50);
    assert_eq!(active, 2);
    assert!(
        !sess.tree.is_paint_dirty(),
        "must stay 0 redraws during positive delay"
    );
    let wake = sess.tree.next_animation_wake_ms().expect("wake deadline");
    assert!((980..=1020).contains(&wake), "wake={wake}");

    // Advance 1050ms past #delayed_tr's 1500ms delay -> wakes & dirties paint, wake=0ms.
    let _ = sess.tree.tick_transitions(1.05);
    assert!(sess.tree.is_paint_dirty());
    assert_eq!(sess.tree.next_animation_wake_ms(), Some(0));
}
