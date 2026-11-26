//! End-to-end adversarial review regression suite locking down fixes across
//! `krui-signals`, `krui-patterns`, `krui-style`, `krui-core`, `krui-compiler`,
//! `krui-preview`, `krui-renderer`, and `krui-cli`.

#![forbid(unsafe_code)]

use accesskit::{Action, ActionRequest, Role};
use krui::*;
use std::cell::{Cell, RefCell};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn review_reg_01_cross_session_scope_dispose_aba_and_isolated_ui_tasks() {
    let s1 = ReactiveSession::new();
    let s2 = ReactiveSession::new();

    let (scope1, sig1, runs) = s1.enter(|| {
        let sig = Signal::new(1_i32);
        let runs = Rc::new(Cell::new(0_usize));
        let runs_c = Rc::clone(&runs);
        let scope = Scope::new();
        scope.run(|| {
            let _eff = Effect::new(move || {
                let _ = sig.get();
                runs_c.set(runs_c.get() + 1);
            });
        });
        (scope, sig, runs)
    });
    assert_eq!(runs.get(), 1);

    // Disposing `s1`'s Scope from inside `s2` (outside `s1.enter`) must clean up `s1`'s Effect.
    s2.enter(|| scope1.dispose());
    s1.enter(|| sig1.set(2));
    assert_eq!(runs.get(), 1);
    assert_eq!(s1.enter(|| debug_stats().live_effects), 0);

    // Resetting `s1` must retire or advance generation so `sig1` never aliases a new Signal in `s1`.
    s1.reset();
    let fresh = s1.enter(|| Signal::new(99_i32));
    s1.enter(|| {
        assert_eq!(sig1.try_get(), None);
        assert_eq!(fresh.get(), 99);
    });

    // `SessionWakeHandle::post_to_ui` queued on `s1` must not drain when polling `s2`.
    let ran_s1 = Arc::new(AtomicUsize::new(0));
    let w1 = s1.enter(krui_signals::current_wake_handle);
    let ran_c = Arc::clone(&ran_s1);
    w1.post_to_ui(move || {
        ran_c.fetch_add(1, Ordering::SeqCst);
    });
    assert!(!s2.enter(poll_async_tasks));
    assert_eq!(ran_s1.load(Ordering::SeqCst), 0);
    assert!(s1.enter(poll_async_tasks));
    assert_eq!(ran_s1.load(Ordering::SeqCst), 1);
}

#[test]
fn review_reg_02_store_and_command_cross_session_and_panic_recovery() {
    let s1 = ReactiveSession::new();
    let (hits, should_panic, cmd, store) = s1.enter(|| {
        let hits = Signal::new(0_i32);
        let should_panic = Signal::new(true);
        let cmd = Command::new(move || {
            assert!(!should_panic.get(), "deliberate command panic");
            hits.update(|v| *v += 1);
        });
        let store = Store::new(0_i32, |state, msg: i32| {
            assert!(msg != -999, "deliberate reducer panic");
            *state += msg;
            Effects::none()
        });
        (hits, should_panic, cmd, store)
    });

    // Panics inside Command::execute or Store::send must reset `running` / `draining` guards.
    let _ = catch_unwind(AssertUnwindSafe(|| cmd.execute()));
    s1.enter(|| should_panic.set(false));

    // Invoking Command and Store outside `s1.enter` must automatically enter `s1` and succeed.
    cmd.execute();
    let _ = catch_unwind(AssertUnwindSafe(|| store.send(-999)));
    store.send(5);
    store.send(7);
    s1.enter(|| {
        assert_eq!(hits.get(), 1);
        assert_eq!(store.state().get(), 12);
    });
}

#[test]
fn review_reg_03_anchor_activation_keyboard_enter_not_space_and_a11y_url() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    let activated: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let act_c = Rc::clone(&activated);
    tree.set_on_link_activate(move |href| act_c.borrow_mut().push(href.to_string()));

    tree.mount_with(|| {
        krui!(
            r#"
            <div style="width: 300px; height: 120px; display: flex">
                <a id="docs-link" href="https://krui.dev/docs" style="width: 160px; height: 40px">문서 보기</a>
            </div>
            "#
        )
    });
    let (_, _) = tree.build_paint_scene(300, 120);

    assert!(tree.dispatch_click(20.0, 20.0));
    assert_eq!(activated.borrow().len(), 1);
    assert_eq!(
        tree.last_activated_link().as_deref(),
        Some("https://krui.dev/docs")
    );

    let link_id = tree.find_by_id("docs-link").expect("docs-link node");
    assert!(tree.set_focus_to_node(link_id));
    assert!(!tree.dispatch_key_space());
    assert_eq!(activated.borrow().len(), 1);
    assert!(tree.dispatch_key_activate());
    assert_eq!(activated.borrow().len(), 2);

    let update = tree.build_accessibility_tree(300.0, 120.0);
    let (a11y_id, a11y_node) = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == Role::Link)
        .expect("AccessKit Role::Link node");
    assert_eq!(a11y_node.url(), Some("https://krui.dev/docs"));
    assert_eq!(a11y_node.label(), Some("문서 보기"));
    assert!(tree.dispatch_accesskit_action(&ActionRequest {
        action: Action::Click,
        target: *a11y_id,
        data: None,
    }));
    assert_eq!(activated.borrow().len(), 3);
}

#[test]
fn review_reg_04_canvas_reentrancy_sharp_nested_clip_and_live_app_tokens() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <div style="width: 240px; height: 180px; border-radius: 20px; overflow: hidden; background-color: var(--krui-accent)">
                <div id="sharp-inner" style="width: 120px; height: 90px; overflow: hidden">
                    <canvas id="cv" width="80" height="60"></canvas>
                </div>
            </div>
            "#
        )
    });
    let cv_id = tree.find_by_id("cv").expect("canvas node");
    let tree_handle = tree.clone();
    let reentrant_found = Rc::new(Cell::new(false));
    let rf_c = Rc::clone(&reentrant_found);
    tree.set_canvas_draw(cv_id, move |ctx: &mut CanvasContext2D, _size: Size| {
        rf_c.set(tree_handle.find_by_id("sharp-inner").is_some());
        ctx.fill_rect(0.0, 0.0, 40.0, 30.0, Color::rgba(255, 255, 255, 255));
    });

    let (scene1, _) = tree.build_paint_scene(240, 180);
    assert!(reentrant_found.get());
    let canvas_clip_radius = scene1
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::CanvasBatch { clip_radius, .. } => Some(*clip_radius),
            _ => None,
        })
        .expect("canvas batch");
    assert!(
        canvas_clip_radius.abs() < 1e-3,
        "Sharp child overflow container must reset inherited 20px clip_radius to 0.0"
    );

    let custom_accent = Color::rgba(231, 49, 119, 255);
    let _ = tree.add_author_stylesheet(":root { --krui-accent: rgb(231, 49, 119); }");
    let (scene2, _) = tree.build_paint_scene(240, 180);
    let has_custom_bg = scene2.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox { background_color, .. } if *background_color == custom_accent
        )
    });
    assert!(has_custom_bg);
}

#[test]
fn review_reg_05_object_fit_none_and_nonblocking_dynamic_img_load() {
    reset_reactive_runtime();
    let tmp_png = std::env::temp_dir().join(format!("krui_reg05_{}.png", std::process::id()));
    let png_bytes: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0xF8,
        0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00,
        0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
    ];
    std::fs::write(&tmp_png, png_bytes).expect("write temp 1x1 png");
    let src_sig = Signal::new(tmp_png.to_string_lossy().into_owned());

    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <div style="width: 120px; height: 100px">
                <img src="{src_sig}" style="width: 100px; height: 80px; object-fit: none" />
            </div>
            "#
        )
    });

    let mut loaded_rect = None;
    for _ in 0..40 {
        let _ = poll_async_tasks();
        let (scene, _) = tree.build_paint_scene(120, 100);
        loaded_rect = scene.primitives.iter().find_map(|p| match p {
            DrawPrimitive::Image { rect, .. } => Some(*rect),
            _ => None,
        });
        if loaded_rect.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let _ = std::fs::remove_file(&tmp_png);
    let rect = loaded_rect.expect("async dynamic <img> must finish decoding on background pool");
    assert!((rect.width - 1.0).abs() < 1e-3 && (rect.height - 1.0).abs() < 1e-3);
    assert!((rect.x - 49.5).abs() < 1e-2 && (rect.y - 39.5).abs() < 1e-2);
}

#[test]
fn review_reg_06_deep_nested_push_layer_overflow_stack_integrity() {
    let mut scene = PaintScene::new(120, 120, Color::BLACK);
    let bounds = LayoutRect {
        x: 10.0,
        y: 10.0,
        width: 100.0,
        height: 100.0,
    };
    for _ in 0..22 {
        scene.primitives.push(DrawPrimitive::PushLayer {
            bounds,
            clip: None,
            clip_radius: 0.0,
            clip_radii: [0.0; 4],
            local_clip: None,
            clip_path: None,
            perspective: None,
            opacity: 0.95,
            filter: krui_style::CssFilterEffect::IDENTITY,
        });
    }
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: bounds,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        background_color: Color::rgba(40, 140, 240, 255),
        background_gradient: None,
        radial_gradient: None,
        conic_gradient: None,
        border_color: Color::TRANSPARENT,
        border_width: 0.0,
        border_radius: 8.0,
        corner_radii: [0.0; 4],
        box_shadow: None,
        backdrop_blur_px: 0.0,
    });
    for _ in 0..22 {
        scene.primitives.push(DrawPrimitive::PopLayer);
    }

    let mut cpu = TinySkiaRenderer::new(120, 120, GpuResourceLimits::default());
    let _ = cpu.render_scene(&scene);
    assert_eq!(cpu.framebuffer_dimensions(), Some((120, 120)));

    if let Some(mut gpu) = WgpuRenderer::try_new(120, 120, GpuResourceLimits::default()) {
        let _ = gpu.render_scene(&scene);
        assert_eq!(gpu.framebuffer_dimensions(), Some((120, 120)));
    }
}

#[cfg(feature = "preview")]
#[test]
fn review_reg_07_compiler_root_tokens_strict_errors_svg_depth_and_utf8_col() {
    use krui_preview::compile_krui_source;

    let doc = compile_krui_source(
        r#"
        <style>
            :root { --krui-accent: #112233; }
        </style>
        <div>
            <svg width="24" height="24">
                <g />
                <rect x="0" y="0" width="24" height="24" />
            </svg>
        </div>
        "#,
    )
    .expect("valid :root token override and self-closing <g/> inside <svg>");
    assert!(!doc.stylesheets.is_empty());

    // Self-closing `<svg />` inside `<svg>` must not corrupt outer `</svg>` depth tracking:
    // it must report `Unsupported SVG element <svg>` at the inner `<svg />`, NOT `Unclosed <svg>`.
    let nested_svg_err = compile_krui_source(
        "<div><svg width=\"24\" height=\"24\"><svg width=\"12\" height=\"12\" /><rect width=\"24\" height=\"24\" /></svg></div>",
    )
    .expect_err("nested <svg/> rejected cleanly");
    assert!(nested_svg_err.message.contains("Unsupported SVG element"));

    let rem_src = r#"<style>.a { width: 12rem; }</style><div class="a"></div>"#;
    assert!(
        compile_krui_source(rem_src).is_ok()
            && krui::dev_hot_reload::compile_krui_source_strict(rem_src).is_ok(),
        "rem length unit is valid standard CSS"
    );
    let mut diag_tree = ViewTree::new();
    let _ = diag_tree.take_style_diagnostics();
    let invalid_css =
        "div { border-width: -3px; padding: 4px -2px; width: -10px; gap: -4px; font-size: -12px; }";
    let _ = diag_tree.add_author_stylesheet(invalid_css);
    assert!(
        !diag_tree.take_style_diagnostics().is_empty(),
        "Expected Stylo diagnostics for negative lengths: {invalid_css}"
    );
    compile_krui_source(r#"<div style="margin: -4px; top: -8px"></div>"#)
        .expect("negative margin and inset remain valid CSS");

    let custom_doc = compile_krui_source("<div>가나다<bogus/></div>").expect("lowercase fallback");
    assert!(custom_doc.warnings.iter().any(|w| {
        w.kind == krui::dev_hot_reload::CompileWarningKind::UnknownHtmlElement
            && w.signal_expr == "1:9"
    }));
    let utf8_err = compile_krui_source("<div>가나다<Bogus/></div>").expect_err("unknown component");
    assert_eq!(utf8_err.line, 1);
    assert_eq!(utf8_err.column, 9);
}

#[cfg(feature = "preview")]
fn verify_krfr_and_coalesced_panel_socket() {
    use krui_cli::panel_codec::{PanelEditorCmd, PanelSocketConn, OP_CMD_REFRESH, OP_CMD_SHUTDOWN};
    use krui_preview::decode_rle_rgba_frame;
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};

    // Truncated header claiming 4000x4000 RGBA with only 1 RLE run must be rejected (`Err`).
    let bad_krfr = [
        b'K', b'R', b'F', b'R', 1, 0, 0, 0, 0xA0, 0x0F, 0xA0, 0x0F, 0, 255, 0, 0, 255,
    ];
    assert!(decode_rle_rgba_frame(&bad_krfr).is_err());

    // Coalesced `[Refresh][Shutdown]` + immediate client EOF must yield BOTH commands in order.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client_sock = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (srv_sock, _) = listener.accept().unwrap();
    let mut panel_conn = PanelSocketConn::new(srv_sock).unwrap();
    let coalesced = [1, 0, 0, 0, OP_CMD_REFRESH, 1, 0, 0, 0, OP_CMD_SHUTDOWN];
    client_sock.write_all(&coalesced).unwrap();
    drop(client_sock);
    std::thread::sleep(std::time::Duration::from_millis(15));
    assert_eq!(
        panel_conn.try_recv_cmd().unwrap(),
        Some(PanelEditorCmd::RequestRefresh)
    );
    assert_eq!(
        panel_conn.try_recv_cmd().unwrap(),
        Some(PanelEditorCmd::Shutdown)
    );
    assert!(panel_conn.try_recv_cmd().is_err());
}

#[cfg(feature = "preview")]
#[test]
fn review_reg_08_krfr_bomb_coalesced_panel_cmds_and_restart_wakeup() {
    use krui_preview::{
        compile_krui_source, encode_compiled_document, PreviewCommand, PreviewResponse,
        PreviewTcpClient, PreviewTcpServer,
    };

    verify_krfr_and_coalesced_panel_socket();

    // `PreviewTcpClient::start_wakeup_listener` wakes immediately and `Restart` reports real `node_count`.
    reset_reactive_runtime();
    let mut server = PreviewTcpServer::bind("127.0.0.1:0").unwrap();
    let mut client = PreviewTcpClient::connect(&server.local_addr().to_string()).unwrap();
    let wakes = Arc::new(AtomicUsize::new(0));
    let wakes_c = Arc::clone(&wakes);
    client.start_wakeup_listener(move || {
        wakes_c.fetch_add(1, Ordering::SeqCst);
    });
    server.accept_pending();

    let doc = compile_krui_source("<div><p>하나</p><p>둘</p></div>").unwrap();
    server.broadcast(&PreviewCommand::Reload(
        encode_compiled_document(&doc).unwrap(),
    ));
    let mut tree = ViewTree::new();
    let mut err_slot = None;
    let mut reload_count = 0_u64;
    for _ in 0..30 {
        if client.poll(&mut tree, &mut err_slot, &mut reload_count) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(wakes.load(Ordering::SeqCst) >= 1);
    server.broadcast(&PreviewCommand::Restart);
    for _ in 0..30 {
        let _ = client.poll(&mut tree, &mut err_slot, &mut reload_count);
        let resps = server.poll_responses();
        if let Some(PreviewResponse::Reloaded { node_count, .. }) = resps.last() {
            assert!(*node_count >= 3);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("Expected Reloaded response with accurate node_count after Restart");
}
