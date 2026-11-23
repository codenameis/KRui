//! Phase 1 vertical slice tests for `krui-android-ffi` (`#![forbid(unsafe_code)]`):
//! Korean IME composition + mid-composition focus change, emoji UTF-16 bridge ops,
//! password keyboard hint, soft-keyboard inset auto-scroll, touch slop/fling, surface recovery,
//! stale handle safety, and optional live TCP preview hot-reload.

#![forbid(unsafe_code)]

use krui_android_ffi::{
    create_registered_session, destroy_registered_session, with_registered_session, AndroidSession,
};
use krui_core::NodeId;
use krui_signals::Signal;
use krui_style::{CompiledStyle, Dimension, DisplayMode, FlexDirection, OverflowMode};
use std::cell::Cell;
use std::rc::Rc;

fn box_style(w: f32, h: f32) -> CompiledStyle {
    let mut style = CompiledStyle::default();
    style.layout.display = DisplayMode::Flex;
    style.layout.flex_direction = FlexDirection::Column;
    style.layout.width = Dimension::Px(w);
    style.layout.height = Dimension::Px(h);
    style.layout.flex_shrink = 0.0;
    style
}

fn input_style(w: f32, h: f32) -> CompiledStyle {
    let mut style = box_style(w, h);
    style.layout.padding.left = 8.0;
    style.layout.padding.right = 8.0;
    style
}

fn mount_ime_inputs(
    session: &mut AndroidSession,
) -> (NodeId, NodeId, NodeId, Signal<String>, Signal<String>) {
    let rs = session.reactive_session.clone();
    let tree = &mut session.tree;
    rs.enter(|| {
        let s1 = Signal::new(String::new());
        let s2 = Signal::new(String::new());
        let pw_sig = Signal::new(String::new());
        let root = tree.create_compiled_node(
            None,
            "div",
            None,
            None,
            box_style(400.0, 600.0),
            false,
            false,
            false,
            false,
        );
        let id1 = add_bound_input(tree, root, "i1", s1, false);
        let id2 = add_bound_input(tree, root, "i2", s2, false);
        let pw = add_bound_input(tree, root, "pw", pw_sig, true);
        (id1, id2, pw, s1, s2)
    })
}

fn add_bound_input(
    tree: &mut krui_core::ViewTree,
    parent: NodeId,
    id: &str,
    sig: Signal<String>,
    is_password: bool,
) -> NodeId {
    let nid = tree.create_compiled_node(
        Some(parent),
        "input",
        Some(id.to_string()),
        None,
        input_style(360.0, 44.0),
        false,
        true,
        false,
        is_password,
    );
    tree.bind_dynamic_text(nid, move || sig.get());
    tree.set_on_input(nid, move |v| sig.set(v));
    nid
}

#[test]
fn android_korean_ime_focus_move_pins_ime_target_and_password_reports_kind() {
    let mut session = AndroidSession::new(800, 1200, 2.0, false);
    assert_eq!(session.logical_size(), (400, 600));
    let (id1, id2, pw_id, s1, s2) = mount_ime_inputs(&mut session);
    session.step_frame(0.016);

    assert!(session.tree.set_focus_to_node(id1));
    assert!(session.should_show_keyboard());
    assert_eq!(session.input_kind_code(), 0);

    assert!(session.set_composing_text("ㅎ", 1));
    assert!(session.set_composing_text("하", 1));
    assert!(session.set_composing_text("한", 1));

    let rs = session.reactive_session.clone();
    assert!(rs.enter(|| session.tree.set_focus_to_node(id2)));
    assert!(session.finish_composing_text());
    assert!(session.commit_text("둘", 1));
    rs.enter(|| {
        assert_eq!(s1.get(), "한");
        assert_eq!(s2.get(), "둘");
        let _ = session.tree.set_focus_to_node(pw_id);
    });
    assert!(session.should_show_keyboard());
    assert_eq!(session.input_kind_code(), 1);
}

#[test]
fn android_emoji_utf16_selection_and_delete_surrounding_preserve_utf8() {
    let mut session = AndroidSession::new(400, 300, 1.0, false);
    let inp = session.reactive_session.enter(|| {
        let sig = Signal::new("안녕🚀끝".to_string());
        let n = session.tree.create_compiled_node(
            None,
            "input",
            None,
            None,
            input_style(300.0, 44.0),
            false,
            true,
            false,
            false,
        );
        session.tree.set_static_text(n, "안녕🚀끝");
        session.tree.set_on_input(n, move |v| sig.set(v));
        n
    });
    session.step_frame(0.016);
    assert!(session.tree.set_focus_to_node(inp));

    assert!(session.set_selection_utf16(4, 4));
    assert!(session.delete_surrounding_text_utf16(2, 0));
    let state = session.tree.focused_editing_state().expect("editing state");
    assert_eq!(state.text, "안녕끝");
}

fn mount_scrollable_bottom_input(session: &mut AndroidSession) -> (NodeId, NodeId) {
    session.reactive_session.enter(|| {
        let mut sc_style = box_style(400.0, 600.0);
        sc_style.layout.overflow = OverflowMode::Scroll;
        let sc = session.tree.create_compiled_node(
            None, "div", None, None, sc_style, false, false, false, false,
        );
        let _spacer = session.tree.create_compiled_node(
            Some(sc),
            "div",
            None,
            None,
            box_style(380.0, 500.0),
            false,
            false,
            false,
            false,
        );
        let inp = session.tree.create_compiled_node(
            Some(sc),
            "input",
            None,
            None,
            input_style(380.0, 44.0),
            false,
            true,
            false,
            false,
        );
        let _tail = session.tree.create_compiled_node(
            Some(sc),
            "div",
            None,
            None,
            box_style(380.0, 200.0),
            false,
            false,
            false,
            false,
        );
        (sc, inp)
    })
}

#[test]
fn android_insets_changed_scrolls_obscured_input_above_ime_and_clamps_on_hide() {
    let mut session = AndroidSession::new(800, 1200, 2.0, false);
    let (sc, bottom_input) = mount_scrollable_bottom_input(&mut session);
    session.step_frame(0.016);
    assert!(session.tree.set_focus_to_node(bottom_input));
    assert!(session.on_insets_changed(0.0, 0.0, 0.0, 0.0, 520.0));
    let scrolled = session.tree.scroll_offset_y(sc).unwrap_or(0.0);
    assert!(
        scrolled >= 100.0,
        "expected keyboard inset to scroll obscured input into view, got {scrolled}"
    );
    let _ = session.on_insets_changed(0.0, 0.0, 0.0, 0.0, 0.0);
    let clamped = session.tree.scroll_offset_y(sc).unwrap_or(0.0);
    assert!(clamped <= 144.0);
}

fn mount_scrollable_touch_list(session: &mut AndroidSession, clicks: &Rc<Cell<u32>>) -> NodeId {
    session.reactive_session.enter(|| {
        let mut sc_style = box_style(300.0, 200.0);
        sc_style.layout.overflow = OverflowMode::Scroll;
        let sc = session.tree.create_compiled_node(
            None, "div", None, None, sc_style, false, false, false, false,
        );
        let btn = session.tree.create_compiled_node(
            Some(sc),
            "button",
            None,
            None,
            box_style(280.0, 80.0),
            true,
            false,
            false,
            false,
        );
        let c = Rc::clone(clicks);
        session.tree.set_on_click(btn, move || c.set(c.get() + 1));
        let _tall = session.tree.create_compiled_node(
            Some(sc),
            "div",
            None,
            None,
            box_style(280.0, 900.0),
            false,
            false,
            false,
            false,
        );
        sc
    })
}

#[test]
fn android_touch_slop_distinguishes_tap_from_drag_and_starts_fling() {
    let mut session = AndroidSession::new(600, 400, 2.0, false);
    let clicks = Rc::new(Cell::new(0_u32));
    let sc = mount_scrollable_touch_list(&mut session, &clicks);
    session.step_frame(0.016);

    assert!(session.on_touch(1, 0, 100.0, 60.0, 0.016));
    assert!(!session.on_touch(1, 2, 104.0, 64.0, 0.016));
    assert!(session.on_touch(1, 1, 104.0, 64.0, 0.016));
    assert_eq!(clicks.get(), 1);

    assert!(session.on_touch(2, 0, 100.0, 260.0, 0.016));
    assert!(session.on_touch(2, 2, 100.0, 100.0, 0.016));
    assert!(session.on_touch(2, 1, 100.0, 100.0, 0.016));
    assert_eq!(clicks.get(), 1);
    let right_after = session.tree.scroll_offset_y(sc).unwrap_or(0.0);
    assert!(right_after > 30.0);
    assert!(session.poll_needs_frame());
    for _ in 0..30 {
        session.step_frame(0.016);
    }
    let after_fling = session.tree.scroll_offset_y(sc).unwrap_or(0.0);
    assert!(after_fling > right_after);
}

#[test]
fn android_surface_recovery_and_stale_handle_safety() {
    let id = create_registered_session(800, 600, 2.0, false);
    assert!(id > 0);
    let ok = with_registered_session(id, |session| {
        assert!(session.is_surface_alive());
        assert!(session.on_surface_destroyed());
        assert!(!session.is_surface_alive());
        assert!(session.step_frame(0.016));
        assert!(session.on_surface_created(1080, 1920, 3.0));
        assert!(session.is_surface_alive());
        assert_eq!(session.logical_size(), (360, 640));
    });
    assert!(ok.is_some());
    assert!(destroy_registered_session(id));
    assert!(!destroy_registered_session(id));
    assert!(!destroy_registered_session(0));
    assert!(!destroy_registered_session(-5));
    assert!(with_registered_session(id, |_| ()).is_none());
    assert!(with_registered_session(0, |_| ()).is_none());
}

#[test]
fn android_hidpi_scale_propagates_to_framebuffer_focus_key_and_panic_guard() {
    use krui_android_ffi::jni_entrypoints::catch_ffi_panic;

    let guarded = catch_ffi_panic(-7_i64, || -> i64 {
        panic!("simulated JNI boundary panic");
    });
    assert_eq!(guarded, -7);

    let mut session = AndroidSession::new(1080, 1920, 3.0, false);
    assert_eq!(session.logical_size(), (360, 640));
    assert!((session.scale_factor() - 3.0).abs() < 1e-4);

    let (id1, id2, _pw, _s1, _s2) = mount_ime_inputs(&mut session);
    session.step_frame(0.016);

    let packed = session.packed_framebuffer_size();
    let fb_w = (packed >> 32) as u32;
    let fb_h = (packed & 0xFFFF_FFFF) as u32;
    assert_eq!((fb_w, fb_h), (1080, 1920));
    let copied = session.with_framebuffer_argb(|w, h, pixels| {
        assert_eq!((w, h), (1080, 1920));
        pixels.len()
    });
    assert_eq!(copied, Some(1080 * 1920));

    let rs = session.reactive_session.clone();
    assert!(rs.enter(|| session.tree.set_focus_to_node(id1)));
    let k1 = session.focused_node_key();
    assert!(k1 != 0);
    assert!(rs.enter(|| session.tree.set_focus_to_node(id2)));
    let k2 = session.focused_node_key();
    assert!(k2 != 0 && k2 != k1);
}

#[cfg(feature = "preview")]
#[test]
fn android_preview_client_applies_tcp_hot_reload() {
    use krui_preview::{
        compile_krui_source, encode_compiled_document, PreviewCommand, PreviewTcpServer,
    };

    let mut server = PreviewTcpServer::bind("127.0.0.1:0").expect("bind preview tcp");
    let addr = server.local_addr().to_string();
    let doc =
        compile_krui_source("<div style=\"width: 300px; height: 200px\"><p>Android Live</p></div>")
            .expect("compile preview doc");
    let ir_bytes = encode_compiled_document(&doc).expect("encode ir");

    let mut session = AndroidSession::new(600, 400, 1.0, false);
    assert!(session.connect_preview(&addr));
    server.accept_pending();
    server.broadcast(&PreviewCommand::Reload(ir_bytes));
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _ = session.step_frame(0.016);
        let _ = server.poll_responses();
        if session.preview_reload_count() >= 1 {
            break;
        }
    }
    assert!(session.preview_reload_count() >= 1);
    assert!(session.tree.node_count() > 0);
}
