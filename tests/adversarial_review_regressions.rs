//! Adversarial code review regression suite covering UTF-8 boundaries, widget math,
//! canvas dash termination, `::backdrop` & preview Stylo lifecycle, CSS comment scoping,
//! password IME isolation, `DamageTracker` hashing, and `winit` idle control flow.

#![forbid(unsafe_code)]

use krui::*;
use std::time::{Duration, Instant};

#[test]
fn color_picker_set_hex_multibyte_korean_input_returns_false_without_panic() {
    reset_reactive_runtime();
    let picker = ColorPickerController::from_rgba_u8([16, 32, 64, 255]);
    // "한글" is 6 UTF-8 bytes, matching the 6-byte `#RRGGBB` arm if ASCII is not checked.
    assert!(!picker.set_hex("한글"));
    assert!(!picker.set_hex("#한글"));
    // 8 UTF-8 bytes ("a한글b" = 1 + 3 + 3 + 1 = 8 bytes) matching the 8-byte `#RRGGBBAA` arm.
    assert!(!picker.set_hex("a한글b"));
    assert_eq!(picker.rgba_u8(), [16, 32, 64, 255]);
    assert!(picker.set_hex("#FF8000"));
    assert_eq!(picker.rgba_u8(), [255, 128, 0, 255]);
}

#[test]
fn dock_layout_right_and_bottom_splitters_move_in_pointer_direction() {
    reset_reactive_runtime();
    let dock = DockLayoutController::new();
    let initial_right = dock.right_split.ratio.get();
    let initial_bottom = dock.bottom_split.ratio.get();

    // Dragging the right-side Inspector divider LEFT (from x=760 to x=660 in a 1000px window)
    // must GROW the right Inspector pane, not shrink it.
    dock.right_split.begin_drag(760.0);
    dock.right_split.drag_to(660.0, 1000.0);
    dock.right_split.end_drag();
    assert!(
        dock.right_split.ratio.get() > initial_right,
        "Dragging right_split left must increase right pane ratio: before={initial_right}, after={}",
        dock.right_split.ratio.get()
    );

    // Dragging the bottom AssetBrowser divider UP (from y=740 to y=640 in a 1000px window)
    // must GROW the bottom pane, not shrink it.
    dock.bottom_split.begin_drag(740.0);
    dock.bottom_split.drag_to(640.0, 1000.0);
    dock.bottom_split.end_drag();
    assert!(
        dock.bottom_split.ratio.get() > initial_bottom,
        "Dragging bottom_split up must increase bottom pane ratio: before={initial_bottom}, after={}",
        dock.bottom_split.ratio.get()
    );
}

#[test]
fn tree_view_handles_cyclic_parents_and_extreme_scroll_values_safely() {
    reset_reactive_runtime();
    let tree = TreeViewController::new();
    tree.set_nodes(vec![
        TreeNodeData {
            id: 1,
            parent: None,
            label: "Root".into(),
            expanded: true,
        },
        TreeNodeData {
            id: 2,
            parent: Some(3),
            label: "CycleA".into(),
            expanded: true,
        },
        TreeNodeData {
            id: 3,
            parent: Some(2),
            label: "CycleB".into(),
            expanded: true,
        },
    ]);
    let (rows_norm, total_h) = tree.visible_window(0.0, 200.0, 24.0);
    assert!(total_h >= 24.0 && !rows_norm.is_empty());

    let (rows_huge, _) = tree.visible_window(1e35, 1e35, 24.0);
    assert!(rows_huge.is_empty());

    let (rows_nan, _) = tree.visible_window(f32::NAN, f32::INFINITY, 0.0);
    assert!(!rows_nan.is_empty());
}

#[test]
fn krui_bindable_signal_ignores_nan_and_infinity_strings() {
    reset_reactive_runtime();
    let f32_sig = Signal::new(42.5_f32);
    KruiBindableSignal::set_from_krui_input(&f32_sig, "NaN".to_string());
    KruiBindableSignal::set_from_krui_input(&f32_sig, "inf".to_string());
    KruiBindableSignal::set_from_krui_input(&f32_sig, "-infinity".to_string());
    assert_eq!(f32_sig.get(), 42.5);

    let f64_sig = Signal::new(10.25_f64);
    KruiBindableSignal::set_from_krui_input(&f64_sig, "NaN".to_string());
    KruiBindableSignal::set_from_krui_input(&f64_sig, "inf".to_string());
    assert_eq!(f64_sig.get(), 10.25);
}

#[test]
fn canvas_dashed_stroke_with_extreme_or_non_finite_coordinates_terminates() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut ctx = CanvasContext2D::new();
        ctx.set_line_dash(&[5.0, 5.0]);
        ctx.set_line_dash_offset(1e20);
        ctx.stroke_line(0.0, 0.0, 1e9, 0.0, 2.0, Color::WHITE);
        ctx.stroke_line(0.0, 0.0, f32::INFINITY, 50.0, 2.0, Color::WHITE);
        let _ = tx.send(());
    });
    rx.recv_timeout(Duration::from_millis(500))
        .expect("CanvasContext2D dashed stroke hung in infinite loop on 1e9 / INF segment");
}

#[test]
fn dialog_backdrop_pseudo_unmounts_without_leaking_nodes_or_taffy_slots() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    let show = Signal::new(true);
    tree.mount_with(move || {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                dialog.modal-box { width: 200px; height: 120px; background-color: #ffffff; }
                dialog.modal-box::backdrop { background-color: rgba(0, 0, 0, 0.55); }
            </style>
            <div>
                <if test="{show}">
                    <dialog id="dlg" class="modal-box">모달 본문</dialog>
                </if>
            </div>
            "#
        )
    });
    let dlg = tree.find_by_id("dlg").expect("dialog mounted");
    assert!(tree.show_modal_dialog(dlg));
    let _ = tree.build_paint_scene(400, 300);

    show.set(false);
    let _ = tree.build_paint_scene(400, 300);
    let base_retained = tree.retained_node_count();
    let base_taffy = tree.taffy_node_count();
    let base_sparse = tree.sparse_component_entry_count();

    for _ in 0..20 {
        show.set(true);
        let d = tree.find_by_id("dlg").expect("dialog remounted");
        assert!(tree.show_modal_dialog(d));
        let _ = tree.build_paint_scene(400, 300);
        show.set(false);
        let _ = tree.build_paint_scene(400, 300);
    }

    assert_eq!(tree.retained_node_count(), base_retained);
    assert_eq!(tree.taffy_node_count(), base_taffy);
    assert_eq!(tree.sparse_component_entry_count(), base_sparse);
}

#[test]
fn damage_tracker_detects_in_place_canvas_command_color_change() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.set_damage_tracking_mode(DamageTrackingMode::BoundingRects {
        split_area_ratio_x10: 15,
        max_rects_per_commit: 8,
    });
    let red_mode = Signal::new(true);
    let draw = CanvasDraw::new(move |ctx, size| {
        let col = if red_mode.get() {
            Color::rgb(220, 38, 38)
        } else {
            Color::rgb(37, 99, 235)
        };
        ctx.fill_rect(0.0, 0.0, size.width, size.height, col);
    });
    tree.mount(krui!(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <canvas style="width: 120px; height: 80px;" ondraw="{draw}" />
        "#
    ));
    let mut scene = PaintScene::new(300, 200, Color::TRANSPARENT);
    let (_, first_dmg) = tree.build_paint_scene_into(300, 200, &mut scene);
    assert!(!first_dmg.is_empty());
    let (_, idle_dmg) = tree.build_paint_scene_into(300, 200, &mut scene);
    assert!(idle_dmg.is_empty());

    red_mode.set(false);
    let (_, mutated_dmg) = tree.build_paint_scene_into(300, 200, &mut scene);
    assert!(
        !mutated_dmg.is_empty(),
        "Changing canvas FillRect color with identical command count must produce non-empty damage"
    );
}

#[test]
fn password_input_ignores_ime_enabled_so_space_key_inserts_character() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <input id="pw" type="password" style="width: 180px; height: 32px;" />
        "#
    ));
    let _ = tree.build_paint_scene(300, 120);
    let pw = tree.find_by_id("pw").expect("password input");
    assert!(tree.set_focus_to_node(pw));

    // Even if a host spuriously forwards `Ime::Enabled`, secret inputs must stay non-IME
    // and clamp non-char-boundary preedit cursors without panicking.
    assert!(!tree.ime_enabled());
    assert!(tree.dispatch_key_text("a b"));
    assert_eq!(tree.node_text(pw).as_deref(), Some("a b"));
}

#[test]
fn component_css_comment_containing_apostrophe_does_not_leak_global_styles() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <template name="ScopedBadge">
            <style>
                .badge-box {
                    /* author's scoped note */
                    background-color: #ef4444;
                    width: 60px;
                    height: 24px;
                }
                .leak-target {
                    background-color: #ef4444;
                }
            </style>
            <div class="badge-box"><slot /></div>
        </template>
        <div>
            <ScopedBadge>배지</ScopedBadge>
            <div id="outside" class="leak-target" style="width: 60px; height: 24px;">외부</div>
        </div>
        "#
    ));
    let _ = tree.build_paint_scene(300, 200);
    let outside = tree.find_by_id("outside").unwrap();
    let paint = tree.node_effective_paint(outside).unwrap();
    assert_ne!(
        paint.background_color,
        Color::parse_css("#ef4444").unwrap(),
        "Component CSS rule after `/* author's ... */` comment must not leak to outside elements"
    );
}

#[cfg(feature = "desktop")]
#[test]
fn dirty_idle_control_flow_waits_for_os_redraw_instead_of_zero_ns_busy_spin() {
    let now = Instant::now();
    // When `is_paint_dirty` is true and no animations/timers are running, `win.request_redraw()`
    // wakes the event loop; `ControlFlow` must NEVER be `WaitUntil(now)` (0ns spin when minimized/occluded).
    let flow = compute_window_idle_control_flow(
        false,
        true,
        false,
        false,
        Duration::from_secs(30),
        false,
        now,
    );
    assert_eq!(flow, winit::event_loop::ControlFlow::Wait);
}

#[cfg(feature = "preview")]
#[test]
fn preview_preflight_accepts_empty_for_list_with_bool_numeric_and_item_fields() {
    reset_reactive_runtime();
    let mut reg = ScopeRegistry::new();
    reg.insert_value("items", Value::List(Vec::new()));
    let doc = krui_compiler::compile_krui_source(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <div>
            <for each="{item in items}" key="{item.id}">
                <button disabled="{!item.enabled}" style="opacity: {item.alpha};">
                    {item.title}
                </button>
            </for>
        </div>
        "#,
    )
    .expect("compile krui");

    krui_preview::validate_compiled_document_against_registry(&doc, &reg).expect(
        "Empty `<for>` list must pass preflight validation without fake Str(\"\") type errors",
    );
}

#[cfg(feature = "preview")]
#[test]
fn preview_remount_disposes_old_stylo_nodes_and_preserves_first_child_selector() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    let reg: RegistryRef = std::rc::Rc::new(ScopeRegistry::new());
    let src_v1 = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            .item:first-child { background-color: #22c55e; }
        </style>
        <div id="first" class="item" style="width: 80px; height: 24px;">하나</div>
    "#;
    let src_v2 = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            .item:first-child { background-color: #22c55e; }
        </style>
        <div id="first" class="item" style="width: 80px; height: 24px;">하나-수정됨</div>
        <div id="second" class="item" style="width: 80px; height: 24px;">둘</div>
    "#;
    let doc1 = krui_compiler::compile_krui_source(src_v1).unwrap();
    let doc2 = krui_compiler::compile_krui_source(src_v2).unwrap();

    krui_preview::mount_compiled_document_with_registry(&mut tree, &doc1, reg.clone());
    let _ = tree.build_paint_scene(300, 200);

    krui_preview::try_reload_compiled_document_with_registry(&mut tree, &doc2, reg.clone())
        .unwrap();
    let _ = tree.build_paint_scene(300, 200);
    let baseline_stylo = tree.stylo_node_count();

    for i in 0..8 {
        let d = if i % 2 == 0 { &doc1 } else { &doc2 };
        krui_preview::try_reload_compiled_document_with_registry(&mut tree, d, reg.clone())
            .unwrap();
        let _ = tree.build_paint_scene(300, 200);
    }

    assert_eq!(
        tree.stylo_node_count(),
        baseline_stylo,
        "Preview remounts must dispose previous Stylo DOM subtrees instead of leaking slots"
    );
    let first_id = tree.find_by_id("first").unwrap();
    let bg = tree
        .node_effective_paint(first_id)
        .unwrap()
        .background_color;
    assert_eq!(
        bg,
        Color::parse_css("#22c55e").unwrap(),
        "After preview remounts, `#first` must still be the `:first-child` of `body` in Stylo"
    );
}
