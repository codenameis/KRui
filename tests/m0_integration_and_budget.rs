//! M0 & M1 Integration, Business Logic Bindings, Protected `.krui-sys` Presets,
//! Korean IME, Scrollable List, Pattern-Agnostic Verification, and CI Frame-Budget Gate.

#![forbid(unsafe_code)]

use krui::*;

#[derive(Clone)]
struct OrderDeskViewModel {
    approved_orders: Signal<i32>,
    item_name: Signal<String>,
    quantity: Signal<i32>,
    final_total_text: Computed<String>,
    approve_order_cmd: Command,
    qty_dec_cmd: Command,
    qty_inc_cmd: Command,
}

impl OrderDeskViewModel {
    fn new_with_default_services() -> ((), Self) {
        let approved_orders = Signal::new(0_i32);
        let item_name = Signal::new("안녕하세요 KRui M2!".to_string());
        let quantity = Signal::new(3_i32);
        let final_total_text = Computed::new(move || {
            let total = i64::from(quantity.get()) * 112_200;
            let s = total.to_string();
            let rev: String = s
                .chars()
                .rev()
                .enumerate()
                .flat_map(|(i, c)| {
                    (i > 0 && i % 3 == 0)
                        .then_some(',')
                        .into_iter()
                        .chain(Some(c))
                })
                .collect();
            format!("{}원", rev.chars().rev().collect::<String>())
        });
        let approve_order_cmd = Command::new(move || approved_orders.update(|n| *n += 1));
        let qty_dec_cmd = Command::new(move || quantity.update(|q| *q = (*q - 1).max(1)));
        let qty_inc_cmd = Command::new(move || quantity.update(|q| *q += 1));
        (
            (),
            Self {
                approved_orders,
                item_name,
                quantity,
                final_total_text,
                approve_order_cmd,
                qty_dec_cmd,
                qty_inc_cmd,
            },
        )
    }

    fn mount_studio_view(&self, tree: &mut ViewTree, clock: &mut FrameClock) {
        let vm = self.clone();
        let target_x = Signal::new(0.0_f32);
        let spring_x = clock.spring_to(0.0, target_x, 220.0, 14.0);
        let vm_orders = vm.approved_orders;
        let _ = Effect::new(move || {
            if vm_orders.get() > 0 {
                target_x.set(24.0);
            }
        });
        tree.mount_with(move || {
            let vm = vm;
            krui!(
                r#"
                <div style="display: flex; flex-direction: column; width: 900px; padding: 16px; gap: 10px">
                    <div style="display: flex; flex-direction: row; gap: 8px; align-items: center; transform: translate({spring_x}px, 0px)">
                        <button id="increment-btn" onclick="{vm.approve_order_cmd}">승인: {vm.approved_orders}</button>
                        <input id="live-input" bind:value="{vm.item_name}" style="width: 240px; height: 34px" />
                        <button id="btn-qty-dec" onclick="{vm.qty_dec_cmd}">-1</button>
                        <button id="btn-qty-inc" onclick="{vm.qty_inc_cmd}">+1</button>
                        <span>{vm.final_total_text}</span>
                    </div>
                    <input id="approver-pin" type="password" value="1234" style="width: 180px; height: 32px" />
                </div>
                "#
            )
        });
    }
}

#[test]
fn text_change_triggers_exactly_one_relayout_on_next_frame() {
    reset_reactive_runtime();

    let mut session = EngineSession::new(1024, 720, RenderBackendPreference::ForceTinySkia);
    let (_services, logic) = OrderDeskViewModel::new_with_default_services();
    logic.mount_studio_view(&mut session.tree, &mut session.clock);

    assert_eq!(logic.final_total_text.get(), "336,600원");

    // Frame 1: Initial layout pass runs once.
    let frame1 = session.step_frame(1.0 / 120.0);
    assert!(frame1.layout_ran);
    assert_eq!(frame1.total_relayout_passes, 1);

    // Tab 1: focuses `#increment-btn` and activates it (mutates dynamic text `{vm.approved_orders}`)
    let focused_btn = session.tree.dispatch_key_tab(false);
    assert!(focused_btn.is_some());
    assert!(session.tree.dispatch_key_activate());
    assert_eq!(logic.approved_orders.get(), 1);

    // Tab 2: focuses `#live-input` and tests Korean IME Preedit + Commit + Backspace
    let focused_input = session.tree.dispatch_key_tab(false);
    assert!(focused_input.is_some());
    assert!(session.tree.dispatch_ime_preedit("한"));
    assert!(session.tree.dispatch_text_input("한글"));
    assert_eq!(logic.item_name.get(), "안녕하세요 KRui M2!한글");
    assert!(session.tree.dispatch_backspace());
    assert_eq!(logic.item_name.get(), "안녕하세요 KRui M2!한");

    // Tab 4: focuses `#btn-qty-inc` and activates it (qty becomes 4 -> final total becomes 448,800원)
    let _ = session.tree.dispatch_key_tab(false); // #btn-qty-dec
    let _ = session.tree.dispatch_key_tab(false); // #btn-qty-inc
    assert!(session.tree.dispatch_key_activate());
    assert_eq!(logic.quantity.get(), 4);
    assert_eq!(logic.final_total_text.get(), "448,800원");

    // First frame after text changes MUST run Taffy relayout exactly once!
    let frame2 = session.step_frame(1.0 / 120.0);
    assert!(
        frame2.layout_ran,
        "Mutating text or IME preedit must mark the node dirty and trigger 1 relayout pass"
    );
    assert_eq!(frame2.total_relayout_passes, 2);

    // Section 5 & Section 6: Verify AccessKit semantic tree masks the password field
    let a11y_update = session.tree.build_accessibility_tree(1024.0, 700.0);
    let has_masked_password = a11y_update.nodes.iter().any(|(_, node)| {
        node.role() == accesskit::Role::PasswordInput
            && node.value() == Some(AccessibilityEngine::MASKED_SECRET_PLACEHOLDER)
    });
    assert!(
        has_masked_password,
        "Password input must be masked in AccessKit tree"
    );
}

#[test]
fn transform_and_opacity_animation_120_frames_runs_zero_relayouts() {
    reset_reactive_runtime();

    let mut session = EngineSession::new(1024, 720, RenderBackendPreference::ForceTinySkia);
    let (_services, logic) = OrderDeskViewModel::new_with_default_services();
    logic.mount_studio_view(&mut session.tree, &mut session.clock);

    // Frame 1: Initial layout pass
    let frame1 = session.step_frame(1.0 / 120.0);
    assert!(frame1.layout_ran);
    assert_eq!(frame1.total_relayout_passes, 1);

    // Trigger `#increment-btn` (which updates text and starts `header_shake` + `spring_x` animations)
    let _ = session.tree.dispatch_key_tab(false);
    assert!(session.tree.dispatch_key_activate());

    // Settle the single text-change relayout on the immediate next frame
    let post_text_frame = session.step_frame(1.0 / 120.0);
    assert!(post_text_frame.layout_ran);
    assert_eq!(post_text_frame.total_relayout_passes, 2);

    // Run 120 consecutive animation frames at 120Hz while `header_shake` and `spring_x` animate:
    // Because `transform` and `opacity` bypass Taffy layout completely,
    // `total_relayout_passes` MUST remain 2 (0 relayouts across all 120 frames!).
    assert!(
        session.clock.has_active_animations(),
        "Animations must actually be active before running the 120-frame fast-path loop"
    );
    let mut frame_times_us = Vec::with_capacity(120);
    let mut animated_frames = 0_usize;
    for _ in 0..120 {
        let telemetry = session.step_frame(1.0 / 120.0);
        assert!(
            !telemetry.layout_ran,
            "Transform/opacity animation must never trigger a Taffy relayout!"
        );
        assert_eq!(telemetry.total_relayout_passes, 2);
        if session.clock.has_active_animations() {
            animated_frames += 1;
        }
        frame_times_us.push(telemetry.total_frame_us);
    }
    assert!(
        animated_frames > 10,
        "Expected active spring/shake animations across multiple frames, got {animated_frames}"
    );

    frame_times_us.sort_unstable();
    let median_us = frame_times_us[frame_times_us.len() / 2];
    assert!(
        median_us <= FrameTelemetry::BUDGET_60FPS_US,
        "Frame budget regression: median frame took {median_us}us (> 16,667us)"
    );
}

#[test]
fn pattern_agnostic_core_supports_mvvm_mvc_and_elm_flux_equally() {
    reset_reactive_runtime();

    #[view_model]
    #[derive(Clone, Debug, PartialEq)]
    struct TodoViewModelState {
        completed_count: i32,
    }
    let vm = TodoViewModelState { completed_count: 2 }.into_view_model();
    let complete_cmd = Command::new({
        let count_sig = vm.completed_count;
        move || count_sig.update(|n| *n += 1)
    });
    let on_mvvm_click = complete_cmd;
    let mvvm_count = vm.completed_count;

    #[derive(Clone, Copy)]
    struct BankController {
        balance: Signal<i32>,
    }
    impl BankController {
        fn deposit(&self, amount: i32) {
            self.balance.update(|b| *b += amount);
        }
    }
    let controller = BankController {
        balance: Signal::new(100),
    };
    let on_mvc_click = move || controller.deposit(50);
    let mvc_balance = controller.balance;

    let elm_state = Signal::new(0_i32);
    let on_elm_click = move || elm_state.update(|s| *s += 10);

    let mut tree = ViewTree::new();
    let fragment = krui!(
        r#"
        <div style="display: flex; flex-direction: row; gap: 12px; width: 600px; height: 120px">
            <button onclick="{on_mvvm_click}">MVVM: {mvvm_count}</button>
            <button onclick="{on_mvc_click}">MVC: {mvc_balance}</button>
            <button onclick="{on_elm_click}">Elm: {elm_state}</button>
        </div>
        "#
    );
    tree.mount(fragment);

    for _ in 0..3 {
        tree.dispatch_key_tab(false);
        tree.dispatch_key_activate();
    }

    assert_eq!(vm.completed_count.get(), 3);
    assert_eq!(controller.balance.get(), 150);
    assert_eq!(elm_state.get(), 10);
}

#[test]
fn m1_cursor_navigation_focus_clearing_and_live_hot_reload() {
    reset_reactive_runtime();

    let mut session = EngineSession::new(1024, 700, RenderBackendPreference::ForceTinySkia);
    let live_text = Signal::new("가나다라".to_string());
    let on_text_change = move |v: String| live_text.set(v);

    let fragment = krui!(
        r#"
        <div style="display: flex; flex-direction: column; width: 600px; height: 400px; padding: 20px; background-color: #0f172a">
            <h1 id="title">원래 제목</h1>
            <input id="edit-box" value="가나다라" oninput="{on_text_change}" />
        </div>
        "#
    );
    session.tree.mount(fragment);
    session.step_frame(1.0 / 120.0);

    session.tree.dispatch_key_tab(false);
    assert!(session.tree.focused_input_rect().is_some());

    assert!(session.tree.dispatch_cursor_left());
    assert!(session.tree.dispatch_cursor_left());
    assert!(session.tree.dispatch_text_input("하"));
    assert_eq!(live_text.get(), "가나하다라");

    assert!(session.tree.dispatch_cursor_home());
    assert!(session.tree.dispatch_delete());
    assert_eq!(live_text.get(), "나하다라");

    session.tree.dispatch_click(500.0, 360.0);
    assert!(session.tree.focused_input_rect().is_none());

    #[cfg(feature = "preview")]
    verify_companion_css_and_live_hot_reload(&mut session.tree);
}

#[cfg(feature = "preview")]
fn verify_companion_css_and_live_hot_reload(tree: &mut ViewTree) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(1);
    let unique_id = format!(
        "{}_{}",
        std::process::id(),
        UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let temp_krui = std::env::temp_dir().join(format!("krui_hot_reload_{unique_id}.krui"));
    let temp_css = std::env::temp_dir().join(format!("krui_hot_reload_{unique_id}.css"));
    std::fs::write(&temp_css, ".box { background-color: #0f172a; }").expect("write initial css");
    std::fs::write(
        &temp_krui,
        r#"<div class="box"><h1 id="title">원래 제목</h1><input id="edit-box" value="가나다라" /></div>"#,
    )
    .expect("write initial temp krui");

    let mut watcher =
        dev_hot_reload::DevFileWatcher::new(&temp_krui).expect("watcher should attach");
    assert_eq!(watcher.reload_count(), 0);
    assert_eq!(watcher.watched_css_paths().len(), 1);

    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&temp_css, ".box { background-color: #ef4444; }").expect("write updated css");
    std::fs::write(
        &temp_krui,
        r#"<div class="box"><span id="added-badge">신규</span><h1 id="title">새 제목</h1><input id="edit-box" value="가나다라" /></div>"#,
    )
    .expect("write updated temp krui");

    let patched = watcher.poll_and_patch(tree);
    assert_eq!(patched, Some(4));
    assert_eq!(tree.node_count(), 4);
    assert!(tree.find_by_id("added-badge").is_some());
    assert_eq!(watcher.reload_count(), 1);

    let (scene, _) = tree.build_paint_scene(1024, 700);
    let has_red_box = scene.primitives.iter().any(|p| match p {
        DrawPrimitive::StyledBox {
            background_color, ..
        } => *background_color == Color::rgb(239, 68, 68),
        _ => false,
    });
    assert!(has_red_box);

    let _ = std::fs::remove_file(&temp_krui);
    let _ = std::fs::remove_file(&temp_css);
}

#[test]
fn developer_controlled_text_selection() {
    reset_reactive_runtime();

    let mut session = EngineSession::new(600, 400, RenderBackendPreference::ForceTinySkia);
    let val = Signal::new("드래그복사테스트".to_string());
    let on_inp = move |s: String| val.set(s);

    let fragment = krui!(
        r#"
        <div style="display: flex; flex-direction: column; width: 600px; height: 400px; padding: 20px">
            <span id="sel-span" selectable="true" style="height: 36px">선택가능한텍스트</span>
            <span id="lock-span" selectable="false" style="height: 36px">선택잠금텍스트</span>
            <input id="inp" value="드래그복사테스트" oninput="{on_inp}" style="width: 300px; height: 40px" />
        </div>
        "#
    );
    session.tree.mount(fragment);
    let _ = session.step_frame(1.0 / 120.0);

    session.tree.dispatch_click(40.0, 30.0);
    assert!(session.tree.select_all());
    assert_eq!(
        session.tree.selected_text().as_deref(),
        Some("선택가능한텍스트")
    );

    session.tree.dispatch_click(40.0, 68.0);
    assert!(!session.tree.select_all());
    assert_eq!(session.tree.selected_text(), None);

    session.tree.dispatch_click(25.0, 110.0);
    session.tree.dispatch_pointer_drag(300.0, 110.0);
    session.tree.end_pointer_drag();
    let cut = session
        .tree
        .cut_selected_text()
        .expect("should cut selected input text");
    assert!(!cut.is_empty());
}

#[test]
fn phase6_bindings_transform_inheritance_and_page_transitions() {
    reset_reactive_runtime();

    let mut session = EngineSession::new(800, 600, RenderBackendPreference::ForceTinySkia);
    let parent_tx = Signal::new(50.0_f32);
    let parent_opacity = Signal::new(0.5_f32);
    let text_sig = Signal::new("초기값".to_string());
    let qty_sig = Signal::new(1_i32);

    let dec_cmd = Command::new(move || qty_sig.update(|q| *q -= 1)).when(move || qty_sig.get() > 1);

    #[controller]
    #[derive(Clone, Copy)]
    struct InlineController {
        qty: Signal<i32>,
    }
    #[controller]
    impl InlineController {
        fn add_amount(&self, delta: i32) {
            self.qty.update(|q| *q += delta);
        }
    }
    let controller = InlineController { qty: qty_sig };

    let fragment = krui!(
        r#"
        <div direction="row" gap="16" width="760" height="240" style="transform: translate({parent_tx}px, 0px); opacity: {parent_opacity}">
            <input id="twoway-input" width="220" height="44" bind:value="{text_sig}" />
            <button id="dec-btn" width="120" height="44" onclick="{dec_cmd}">감소</button>
            <button id="add-btn" width="120" height="44" onclick="{controller.add_amount(5)}">증가</button>
        </div>
        "#
    );
    session.tree.mount(fragment);

    verify_translated_bindings_and_transition(&mut session, text_sig, qty_sig);
}

fn assert_scene_boxes_translated_with_opacity(scene: &PaintScene) {
    let (mut cur_op, mut stack, mut boxes) = (1.0_f32, Vec::new(), 0_usize);
    for p in &scene.primitives {
        match p {
            DrawPrimitive::PushLayer { opacity, .. } => {
                stack.push(cur_op);
                cur_op *= *opacity;
            }
            DrawPrimitive::PopLayer => cur_op = stack.pop().unwrap_or(1.0),
            DrawPrimitive::StyledBox {
                transform, opacity, ..
            } => {
                boxes += 1;
                assert!((transform.tx - 50.0).abs() < 0.01 && cur_op * *opacity <= 0.501);
            }
            _ => {}
        }
    }
    assert!(boxes > 0);
}

fn verify_translated_bindings_and_transition(
    session: &mut EngineSession,
    text_sig: Signal<String>,
    qty_sig: Signal<i32>,
) {
    let (scene, _) = session.tree.build_paint_scene(800, 600);
    assert_scene_boxes_translated_with_opacity(&scene);

    session.tree.dispatch_click(310.0, 20.0);
    assert_eq!(qty_sig.get(), 1);

    session.tree.dispatch_click(450.0, 20.0);
    assert_eq!(qty_sig.get(), 6);

    session.tree.dispatch_click(310.0, 20.0);
    assert_eq!(qty_sig.get(), 5);

    session.tree.dispatch_click(100.0, 20.0);
    session.tree.dispatch_cursor_end();
    session.tree.dispatch_text_input("바인딩");
    assert_eq!(text_sig.get(), "초기값바인딩");

    let next_fragment = krui!(
        r#"
        <div direction="column" width="600" height="400">
            <h1>페이지 2 전환 완료</h1>
        </div>
        "#
    );
    session.transition_to_view(next_fragment);
    assert!(session.clock.has_active_animations());
    for _ in 0..180 {
        let _ = session.step_frame(1.0 / 120.0);
    }
    assert!(!session.clock.has_active_animations());
}

#[test]
fn a8_idle_control_flow_canvas_draw_size_and_external_wgpu_view() {
    use std::time::{Duration, Instant};
    use winit::event_loop::ControlFlow;

    reset_reactive_runtime();

    let now = Instant::now();
    assert_eq!(
        compute_idle_control_flow(false, false, false, false, now),
        ControlFlow::Wait
    );
    assert_eq!(
        compute_idle_control_flow(false, false, true, false, now),
        ControlFlow::WaitUntil(now + Duration::from_millis(500))
    );
    assert_eq!(
        compute_idle_control_flow(true, false, false, false, now),
        ControlFlow::WaitUntil(now + Duration::from_micros(8_333))
    );

    let mut tree = ViewTree::new();
    let observed_size = Signal::new(Size::new(0.0, 0.0));
    let draw = CanvasDraw::new(move |ctx, size| {
        observed_size.set(size);
        ctx.fill_rect(0.0, 0.0, size.width, size.height, Color::rgb(16, 185, 129));
    });
    let fragment = krui!(
        r#"
        <div direction="column" width="400" height="240">
            <canvas id="c" width="280" height="140" ondraw="{draw}" />
        </div>
        "#
    );
    tree.mount(fragment);
    let (scene, _) = tree.build_paint_scene(400, 240);
    assert_eq!(observed_size.get(), Size::new(280.0, 140.0));

    verify_external_wgpu_render_to_view(&scene);
}

fn verify_external_wgpu_render_to_view(scene: &PaintScene) {
    let Some(ctx) =
        krui::GpuContext::try_new_with_power_preference(wgpu::PowerPreference::LowPower)
    else {
        return;
    };
    let mut renderer = WgpuRenderer::from_context(
        ctx,
        wgpu::TextureFormat::Rgba8Unorm,
        400,
        240,
        GpuResourceLimits::default(),
    );
    let ext_tex = renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("A8 External Texture"),
        size: wgpu::Extent3d {
            width: 400,
            height: 240,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let ext_view = ext_tex.create_view(&wgpu::TextureViewDescriptor::default());
    let stats = renderer.render_to_view(scene, &ext_view, (400, 240), 1.0);
    assert!(stats.vertices_generated > 0);
}
