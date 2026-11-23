//! Unit and headless integration tests for Step 4 (`krui-core` + `krui-winit`).

#![forbid(unsafe_code)]

use krui_core::{
    caret_rect, key_name_for_keydown, route_key, A11yId, FocusState, ImeEvent, KeyAction, KeyInfo,
    LayoutRect, Mods, RenderBackendPreference, TouchPhaseKind, ViewFragment, ViewTree,
    ViewportInsets,
};
use krui_signals::{spawn_tokio_task, FrameClock, Signal};
use krui_style::{
    CompiledStyle, Dimension, DisplayMode, FlexDirection, OverflowMode, PaintStyle, TextAlign,
    TextStyle,
};
use krui_winit::{
    compute_window_idle_control_flow, key_info_from_logical_key, sync_ime_host_with_sink,
    MultiWindowApp, RecordedImeOp, RecordedImeSink, WindowSpec, WinitImeStateCache,
    CARET_BLINK_IDLE_TIMEOUT, FRAME_PACE_120HZ,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::event_loop::ControlFlow;
use winit::keyboard::{Key, NativeKey};

fn box_style(w: f32, h: f32) -> CompiledStyle {
    let mut style = CompiledStyle::default();
    style.layout.display = DisplayMode::Flex;
    style.layout.flex_direction = FlexDirection::Column;
    style.layout.width = Dimension::Px(w);
    style.layout.height = Dimension::Px(h);
    style
}

fn aligned_input_style(w: f32, h: f32, pad: f32, align: TextAlign) -> CompiledStyle {
    let mut style = box_style(w, h);
    style.layout.padding.left = pad;
    style.layout.padding.right = pad;
    style.paint = PaintStyle::default();
    style.text = TextStyle {
        text_align: align,
        font_size: 16.0,
        ..TextStyle::default()
    };
    style
}

fn box_node(
    tree: &mut ViewTree,
    parent: Option<krui_core::NodeId>,
    tag: &'static str,
    id: Option<&str>,
    style: CompiledStyle,
    (is_btn, is_inp): (bool, bool),
) -> krui_core::NodeId {
    tree.create_compiled_node(
        parent,
        tag,
        id.map(str::to_string),
        None,
        style,
        is_btn,
        is_inp,
        false,
        false,
    )
}

fn assert_right_aligned_ime_and_overflow(tree: &mut ViewTree, rgt: krui_core::NodeId) {
    assert!(tree.set_focus_to_node(rgt));
    let rgt_empty_area = tree
        .focused_ime_cursor_area()
        .expect("right empty ime area");
    assert!(
        rgt_empty_area.x > 175.0,
        "empty right x={}",
        rgt_empty_area.x
    );

    assert!(tree.dispatch_text_input("LongOverflowTextForScrollingCheck12345"));
    let _ = tree.build_paint_scene(300, 200);
    let rgt_long = tree
        .focused_ime_cursor_area()
        .expect("right overflow ime area");
    assert!(
        rgt_long.x <= 198.0,
        "overflow caret clipped inside box x={}",
        rgt_long.x
    );
    let _ = caret_rect(
        LayoutRect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 40.0,
        },
        &aligned_input_style(200.0, 40.0, 10.0, TextAlign::Right),
        0.0,
        2.0,
    );
}

#[test]
fn input_text_align_center_and_right_align_caret_and_ime_cursor_area() {
    let mut tree = ViewTree::new();
    let root = box_node(
        &mut tree,
        None,
        "div",
        None,
        box_style(300.0, 200.0),
        (false, false),
    );
    let cen = box_node(
        &mut tree,
        Some(root),
        "input",
        Some("c"),
        aligned_input_style(200.0, 40.0, 10.0, TextAlign::Center),
        (false, true),
    );
    let rgt = box_node(
        &mut tree,
        Some(root),
        "input",
        Some("r"),
        aligned_input_style(200.0, 40.0, 10.0, TextAlign::Right),
        (false, true),
    );
    let _ = tree.build_paint_scene(300, 200);

    assert!(tree.set_focus_to_node(cen));
    let cen_empty_area = tree
        .focused_ime_cursor_area()
        .expect("center empty ime area");
    assert!(
        (85.0..=115.0).contains(&cen_empty_area.x),
        "empty center x={}",
        cen_empty_area.x
    );

    assert!(tree.dispatch_text_input("Hi"));
    let _ = tree.build_paint_scene(300, 200);
    let cen_hi_area = tree.focused_ime_cursor_area().expect("center Hi ime area");
    assert!(
        cen_hi_area.x > cen_empty_area.x,
        "Hi cursor x={} vs empty {}",
        cen_hi_area.x,
        cen_empty_area.x
    );

    assert_right_aligned_ime_and_overflow(&mut tree, rgt);
}

#[test]
fn password_input_suppresses_ime_and_desktop_cursor_area() {
    let mut tree = ViewTree::new();
    let pw = tree.create_compiled_node(
        None,
        "input",
        Some("pw".into()),
        None,
        aligned_input_style(220.0, 40.0, 8.0, TextAlign::Left),
        false,
        true,
        false,
        true,
    );
    let _ = tree.build_paint_scene(300, 100);
    assert!(tree.set_focus_to_node(pw));

    let k = KeyInfo::Character {
        logical: "s".into(),
        text: Some("s".into()),
    };
    let action = tree.dispatch_routed_key(&k, Mods::default(), false);
    assert_eq!(action, KeyAction::InsertText("s".to_string()));
    assert_eq!(key_name_for_keydown(&k), "s");

    let _ = tree.dispatch_ime_event(ImeEvent::Preedit {
        text: "한".into(),
        cursor_offset: None,
    });
    assert_eq!(
        tree.dispatch_routed_key(&KeyInfo::Backspace, Mods::default(), false),
        KeyAction::Backspace
    );
    let routed = route_key(
        &KeyInfo::Backspace,
        Mods::default(),
        &FocusState {
            is_input_focused: true,
            has_ime_preedit: false,
            is_ime_enabled: false,
        },
    );
    assert_eq!(routed, KeyAction::Backspace);

    let mut sink = RecordedImeSink::default();
    let mut cache = WinitImeStateCache::default();
    sync_ime_host_with_sink(&mut sink, &tree, &mut cache);
    assert!(sink.ops.contains(&RecordedImeOp::SetAllowed(false)));
    assert!(!sink
        .ops
        .iter()
        .any(|op| matches!(op, RecordedImeOp::SetCursorArea(_))));
    assert!(A11yId::from(pw).0 > 0);
}

#[test]
fn wayland_unidentified_and_dead_key_fallback_preserves_printable_text() {
    let u = Key::Unidentified(NativeKey::Unidentified);
    let d = Key::Dead(Some('^'));
    assert_eq!(
        key_info_from_logical_key(&u, Some("g")),
        KeyInfo::Character {
            logical: "g".into(),
            text: Some("g".into())
        }
    );
    assert_eq!(
        key_info_from_logical_key(&d, Some("^")),
        KeyInfo::Character {
            logical: "^".into(),
            text: Some("^".into())
        }
    );
    assert_eq!(
        key_info_from_logical_key(&d, None),
        KeyInfo::Other(String::new())
    );
    assert_eq!(
        key_info_from_logical_key(&u, Some("\u{0007}")),
        KeyInfo::Other(String::new())
    );
}

#[test]
fn window_idle_control_flow_stops_blink_on_blur_and_after_10s() {
    let now = Instant::now();
    assert_eq!(
        compute_window_idle_control_flow(
            false,
            false,
            true,
            true,
            Duration::from_secs(2),
            false,
            now
        ),
        ControlFlow::WaitUntil(now + Duration::from_millis(500))
    );
    assert_eq!(
        compute_window_idle_control_flow(
            false,
            false,
            true,
            true,
            CARET_BLINK_IDLE_TIMEOUT,
            false,
            now
        ),
        ControlFlow::Wait
    );
    assert_eq!(
        compute_window_idle_control_flow(
            false,
            false,
            true,
            false,
            Duration::from_secs(1),
            false,
            now
        ),
        ControlFlow::Wait
    );
    assert_eq!(
        compute_window_idle_control_flow(
            true,
            false,
            false,
            false,
            Duration::from_secs(20),
            false,
            now
        ),
        ControlFlow::WaitUntil(now + FRAME_PACE_120HZ)
    );
}

#[test]
fn viewport_insets_scrolls_obscured_input_above_ime_and_clamps_on_hide() {
    let mut tree = ViewTree::new();
    let mut sc_style = box_style(320.0, 600.0);
    sc_style.layout.overflow = OverflowMode::Scroll;
    let sc = tree.create_compiled_node(
        None,
        "div",
        Some("sc".into()),
        None,
        sc_style,
        false,
        false,
        false,
        false,
    );
    let _spacer = tree.create_compiled_node(
        Some(sc),
        "div",
        None,
        None,
        box_style(300.0, 540.0),
        false,
        false,
        false,
        false,
    );
    let inp = tree.create_compiled_node(
        Some(sc),
        "input",
        Some("b_inp".into()),
        None,
        aligned_input_style(300.0, 44.0, 6.0, TextAlign::Left),
        false,
        true,
        false,
        false,
    );
    let _ = tree.build_paint_scene(320, 600);
    assert!(tree.set_focus_to_node(inp));
    assert_eq!(tree.scroll_offset_y(sc), Some(0.0));

    tree.set_viewport_insets(
        ViewportInsets {
            ime_bottom: 280.0,
            ..Default::default()
        },
        600.0,
    );
    let scrolled = tree.scroll_offset_y(sc).unwrap_or(0.0);
    assert!(
        (260.0..=280.0).contains(&scrolled),
        "expected auto-scroll above 320px keyboard, got {scrolled}"
    );

    tree.set_viewport_insets(ViewportInsets::default(), 600.0);
    assert_eq!(tree.scroll_offset_y(sc), Some(0.0));
}

fn build_scrollable_touch_tree(clicks: Rc<Cell<u32>>) -> (ViewTree, krui_core::NodeId) {
    let mut tree = ViewTree::new();
    let mut sc_style = box_style(300.0, 200.0);
    sc_style.layout.overflow = OverflowMode::Scroll;
    let sc = box_node(&mut tree, None, "div", None, sc_style, (false, false));
    let mut btn_style = box_style(280.0, 80.0);
    btn_style.layout.flex_shrink = 0.0;
    let btn = box_node(
        &mut tree,
        Some(sc),
        "button",
        None,
        btn_style,
        (true, false),
    );
    tree.set_on_click(btn, move || clicks.set(clicks.get() + 1));
    let mut tall_style = box_style(280.0, 1200.0);
    tall_style.layout.flex_shrink = 0.0;
    let _tall = box_node(&mut tree, Some(sc), "div", None, tall_style, (false, false));
    let _ = tree.build_paint_scene(300, 200);
    (tree, sc)
}

#[test]
fn touch_slop_distinguishes_tap_from_scroll_and_cleans_up_inertial_fling() {
    let clicks = Rc::new(Cell::new(0_u32));
    let (mut tree, sc) = build_scrollable_touch_tree(Rc::clone(&clicks));
    let mut clock = FrameClock::new();

    assert!(tree.dispatch_touch(&mut clock, 1, TouchPhaseKind::Started, 50.0, 30.0, 0.016));
    assert!(!tree.dispatch_touch(&mut clock, 1, TouchPhaseKind::Moved, 54.0, 33.0, 0.016));
    assert!(tree.dispatch_touch(&mut clock, 1, TouchPhaseKind::Ended, 54.0, 33.0, 0.016));
    assert_eq!(clicks.get(), 1);

    assert!(tree.dispatch_touch(&mut clock, 2, TouchPhaseKind::Started, 50.0, 120.0, 0.016));
    assert!(tree.dispatch_touch(&mut clock, 2, TouchPhaseKind::Moved, 50.0, 40.0, 0.016));
    assert!(tree.dispatch_touch(&mut clock, 2, TouchPhaseKind::Ended, 50.0, 40.0, 0.016));
    assert_eq!(
        clicks.get(),
        1,
        "dragging past 8px slop must cancel button tap"
    );

    let right_after_drag = tree.scroll_offset_y(sc).unwrap_or(0.0);
    assert!(
        right_after_drag > 50.0,
        "expected touch scroll, got {right_after_drag}"
    );
    assert!(
        clock.has_active_animations(),
        "expected inertial fling started"
    );

    for _ in 0..180 {
        let _ = clock.tick(1.0 / 60.0);
        if !clock.has_active_animations() {
            break;
        }
    }
    clock.prune_disposed_scopes();
    let after_fling = tree.scroll_offset_y(sc).unwrap_or(0.0);
    assert!(
        after_fling > right_after_drag,
        "fling did not advance scroll: {after_fling} <= {right_after_drag}"
    );
    assert_eq!(clock.active_callback_count(), 0);
    assert!(!tree.dispatch_ime_event(ImeEvent::Disabled));
}

#[test]
fn multi_window_app_isolates_sessions_shares_gpu_context_and_avoids_offscreen_alloc() {
    let mut app = MultiWindowApp::new(RenderBackendPreference::Auto);
    app.add_window(WindowSpec::new("Win A", 320, 240, |sess| {
        let s = Signal::new(10_i32);
        let root = sess.tree.create_compiled_node(
            None,
            "div",
            None,
            None,
            box_style(320.0, 240.0),
            false,
            false,
            false,
            false,
        );
        sess.tree
            .bind_dynamic_text(root, move || s.get().to_string());
        spawn_tokio_task(async { 1_u8 }, |_| {});
    }));
    app.add_window(WindowSpec::new("Win B", 320, 240, |_sess| {
        let _ = ViewFragment::new(|tree, parent| {
            vec![tree.create_compiled_node(
                parent,
                "div",
                None,
                None,
                box_style(100.0, 40.0),
                false,
                false,
                false,
                false,
            )]
        });
        let s = Signal::new(20_i32);
        assert_eq!(s.get(), 20);
    }));
    let sessions = app.build_headless_sessions();
    assert_eq!(sessions.len(), 2);
    assert_ne!(
        sessions[0].reactive_session.id(),
        sessions[1].reactive_session.id()
    );
    assert!(!sessions[0].renderer.has_offscreen_target());
    assert!(!sessions[1].renderer.has_offscreen_target());
    if let Some(shared) = app.shared_gpu_context() {
        let g0 = sessions[0].renderer.gpu_context().expect("session 0 gpu");
        let g1 = sessions[1].renderer.gpu_context().expect("session 1 gpu");
        assert!(Arc::ptr_eq(&shared, &g0));
        assert!(Arc::ptr_eq(&shared, &g1));
    }
}
