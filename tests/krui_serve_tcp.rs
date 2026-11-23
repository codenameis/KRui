//! Integration tests for `B6: krui serve (crates/cli)` and `krui-preview` TCP protocol:
//! 1. Local TCP server (`PreviewTcpServer`) <-> client (`PreviewTcpClient`) connection:
//!    - Sending `PreviewCommand::Reload(ir)` hot-reloads the live `ViewTree` and preserves `<input>` text & focus.
//!    - Sending `PreviewCommand::CompileError(err)` sets the error overlay while keeping the previous UI alive and interactive.
//!    - Sending `PreviewCommand::InspectTree` returns the live `ViewTree` node hierarchy and layout rectangles.
//!    - Sending `PreviewCommand::SetOverlay` and `PreviewCommand::SetDeviceFrame` decorates `PaintScene` with
//!      the device bezel (`iphone-15`, `pixel-8`, `rpi-800x480`), safe-area cutouts, and hit-test wireframes.
//!    - Sending `PreviewCommand::Restart` resets preserved `<input>` state back to initial values.
//! 2. `krui-cli` argument parser (`krui serve --example 02_mvvm_pattern --frame iphone-15 --overlay hit`).

#![cfg(feature = "preview")]
#![forbid(unsafe_code)]

use krui::*;
use krui_cli::{parse_cli_args, ServeOptions};
use krui_preview::{
    compile_krui_source, decorate_preview_paint_scene, encode_compiled_document,
    mount_compiled_document_with_registry, CompileError, DeviceFramePreset, PreviewCommand,
    PreviewOverlayKind, PreviewResponse, PreviewTcpClient, PreviewTcpServer,
};
use std::rc::Rc;
use std::time::{Duration, Instant};

#[view_model]
#[derive(Clone)]
struct ServeTestVm {
    #[state]
    title: String,
    #[state]
    clicks: i32,
    #[command]
    bump: Command,
}

impl ServeTestVm {
    pub fn new() -> Self {
        let title = Signal::new("라이브 서버 초기".to_string());
        let clicks = Signal::new(0_i32);
        let clicks_cb = clicks;
        let bump = Command::new(move || {
            clicks_cb.update(|c| *c += 1);
        });
        Self {
            title,
            clicks,
            bump,
        }
    }
}

fn wait_for_responses(server: &mut PreviewTcpServer, min_count: usize) -> Vec<PreviewResponse> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut collected = Vec::new();
    while Instant::now() < deadline && collected.len() < min_count {
        collected.extend(server.poll_responses());
        if collected.len() < min_count {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    collected
}

#[test]
fn b6_krui_serve_tcp_reload_error_overlay_inspect_tree_and_device_frame() {
    let vm = ServeTestVm::new();
    let mut scope_reg = ScopeRegistry::new();
    scope_reg.insert_registry("vm", vm.clone());
    let reg: RegistryRef = Rc::new(scope_reg);

    let initial_src = r#"
        <style>
            .root { display: flex; flex-direction: column; width: 393px; height: 400px; padding: 16px; gap: 10px; background-color: #0f172a; }
            .btn { width: 180px; height: 42px; background-color: #2563eb; color: #ffffff; }
        </style>
        <div class="root">
            <h2 id="hdr">{vm.title} ({vm.clicks})</h2>
            <input id="editor" width="240" height="38" />
            <button id="act-btn" class="btn" onclick="{vm.bump}">증가</button>
        </div>
    "#;
    let doc_v1 = compile_krui_source(initial_src).expect("compile v1");
    let mut tree = ViewTree::new();
    mount_compiled_document_with_registry(&mut tree, &doc_v1, reg);
    let _ = tree.build_paint_scene(393, 852);

    let editor_rect = tree
        .find_by_id("editor")
        .and_then(|id| tree.node_layout(id))
        .unwrap();
    tree.dispatch_click(editor_rect.x + 10.0, editor_rect.y + 10.0);
    tree.dispatch_text_input("유지될텍스트");
    let ed_id = tree.find_by_id("editor").unwrap();
    assert_eq!(tree.node_text(ed_id).as_deref(), Some("유지될텍스트"));

    let mut server = PreviewTcpServer::bind("127.0.0.1:0").expect("bind PreviewTcpServer");
    let mut client = PreviewTcpClient::connect(&server.local_addr().to_string()).unwrap();
    client.set_initial_document(doc_v1);
    server.accept_pending();
    assert_eq!(server.client_count(), 1);

    let (mut compile_error, mut reload_count) = (None, 0_u64);
    verify_tcp_reload_and_error_overlay(
        &mut server,
        &mut client,
        &mut tree,
        &mut compile_error,
        &mut reload_count,
        &vm,
    );
    verify_tcp_overlays_inspect_and_restart(
        &mut server,
        &mut client,
        &mut tree,
        &mut compile_error,
        &mut reload_count,
    );
}

fn verify_tcp_reload_and_error_overlay(
    server: &mut PreviewTcpServer,
    client: &mut PreviewTcpClient,
    tree: &mut ViewTree,
    compile_error: &mut Option<CompileError>,
    reload_count: &mut u64,
    vm: &ServeTestVm,
) {
    let updated_src = r#"
        <style>
            .root { display: flex; flex-direction: column; width: 393px; height: 440px; padding: 16px; gap: 10px; background-color: #1e293b; }
            .btn { width: 200px; height: 44px; background-color: #10b981; color: #ffffff; }
        </style>
        <div class="root">
            <h2 id="hdr">[핫리로드됨] {vm.title} ({vm.clicks})</h2>
            <span id="badge">새 배지 노드</span>
            <input id="editor" width="240" height="38" />
            <button id="act-btn" class="btn" onclick="{vm.bump}">증가 v2</button>
        </div>
    "#;
    let doc_v2 = compile_krui_source(updated_src).expect("compile v2");
    server.broadcast(&PreviewCommand::Reload(
        encode_compiled_document(&doc_v2).expect("encode v2 IR"),
    ));
    wait_until_client_reload(client, tree, compile_error, reload_count);
    assert_eq!(*reload_count, 1);
    assert!(compile_error.is_none());
    let responses = wait_for_responses(server, 1);
    assert!(matches!(
        responses.first(),
        Some(PreviewResponse::Reloaded {
            node_count: 5,
            reload_count: 1
        })
    ));

    let _ = tree.build_paint_scene(393, 852);
    let hdr_id = tree.find_by_id("hdr").expect("#hdr after reload");
    assert_eq!(
        tree.node_text(hdr_id).as_deref(),
        Some("[핫리로드됨] 라이브 서버 초기 (0)")
    );
    let ed_id_v2 = tree.find_by_id("editor").expect("#editor after reload");
    assert_eq!(tree.node_text(ed_id_v2).as_deref(), Some("유지될텍스트"));
    assert_eq!(tree.focused_node_id(), Some(ed_id_v2));

    verify_tcp_error_overlay_preserves_reactivity(
        server,
        client,
        tree,
        compile_error,
        reload_count,
        vm,
        hdr_id,
    );
}

fn wait_until_client_reload(
    client: &mut PreviewTcpClient,
    tree: &mut ViewTree,
    compile_error: &mut Option<CompileError>,
    reload_count: &mut u64,
) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && *reload_count == 0 {
        let _ = client.poll(tree, compile_error, reload_count);
        if *reload_count == 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn verify_tcp_error_overlay_preserves_reactivity(
    server: &mut PreviewTcpServer,
    client: &mut PreviewTcpClient,
    tree: &mut ViewTree,
    compile_error: &mut Option<CompileError>,
    reload_count: &mut u64,
    vm: &ServeTestVm,
    hdr_id: krui::NodeId,
) {
    let broken_src = r#"<div class="root"><UnknownComponent>문법 오류</UnknownComponent></div>"#;
    let syntax_err = compile_krui_source(broken_src).expect_err("unknown tag must fail");
    server.broadcast(&PreviewCommand::CompileError(syntax_err.clone()));
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && compile_error.is_none() {
        let _ = client.poll(tree, compile_error, reload_count);
        if compile_error.is_none() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    assert_eq!(compile_error.as_ref(), Some(&syntax_err));
    let _ = wait_for_responses(server, 1);

    let btn_rect = tree
        .find_by_id("act-btn")
        .and_then(|id| tree.node_layout(id))
        .expect("#act-btn must remain alive");
    assert!(tree.dispatch_click(btn_rect.x + 15.0, btn_rect.y + 15.0));
    assert_eq!(vm.clicks.get(), 1);
    assert_eq!(
        tree.node_text(hdr_id).as_deref(),
        Some("[핫리로드됨] 라이브 서버 초기 (1)")
    );
}

fn verify_tcp_overlays_inspect_and_restart(
    server: &mut PreviewTcpServer,
    client: &mut PreviewTcpClient,
    tree: &mut ViewTree,
    compile_error: &mut Option<CompileError>,
    reload_count: &mut u64,
) {
    server.broadcast(&PreviewCommand::SetOverlay {
        kind: PreviewOverlayKind::Hit,
        enabled: true,
    });
    server.broadcast(&PreviewCommand::SetDeviceFrame(Some(
        DeviceFramePreset::Iphone15,
    )));
    let _ = wait_for_responses(server, 0);
    std::thread::sleep(Duration::from_millis(25));
    let _ = client.poll(tree, compile_error, reload_count);
    let _ = wait_for_responses(server, 2);
    assert!(client.overlays().hit);
    assert_eq!(client.device_frame(), Some(DeviceFramePreset::Iphone15));

    let (mut scene, _) = tree.build_paint_scene(393, 852);
    let base_prim_count = scene.primitives.len();
    let mut text_engine = TextEngine::new();
    let inspected = tree.inspect_nodes();
    decorate_preview_paint_scene(
        &mut scene,
        &mut text_engine,
        &inspected,
        compile_error.as_ref(),
        client.overlays(),
        client.device_frame(),
        tree.relayout_passes(),
        450,
    );
    assert!(scene.primitives.len() > base_prim_count + 4);
    assert_eq!(
        (scene.width, scene.height),
        DeviceFramePreset::Iphone15.outer_window_size()
    );
    verify_tree_inspect_and_restart_commands(server, client, tree, compile_error, reload_count);
}

fn verify_tree_inspect_and_restart_commands(
    server: &mut PreviewTcpServer,
    client: &mut PreviewTcpClient,
    tree: &mut ViewTree,
    compile_error: &mut Option<CompileError>,
    reload_count: &mut u64,
) {
    server.broadcast(&PreviewCommand::InspectTree);
    std::thread::sleep(Duration::from_millis(25));
    let _ = client.poll(tree, compile_error, reload_count);
    let inspect_resps = wait_for_responses(server, 1);
    match inspect_resps.first() {
        Some(PreviewResponse::TreeInspection(nodes)) => {
            assert_eq!(nodes.len(), 5);
            assert!(nodes
                .iter()
                .any(|n| n.element_id.as_deref() == Some("editor") && n.text == "유지될텍스트"));
        }
        other => panic!("Expected TreeInspection response, got {other:?}"),
    }

    server.broadcast(&PreviewCommand::Restart);
    std::thread::sleep(Duration::from_millis(25));
    let _ = client.poll(tree, compile_error, reload_count);
    let _ = wait_for_responses(server, 1);
    assert!(compile_error.is_none());
    let ed_id_restarted = tree.find_by_id("editor").expect("#editor after restart");
    assert_eq!(tree.node_text(ed_id_restarted).as_deref(), Some(""));
}

#[test]
fn b6_krui_cli_argument_parsing_supports_serve_example_frame_and_overlays() {
    use krui_cli::CliParseError;

    let raw_args = [
        "serve",
        "--example",
        "02_mvvm_pattern",
        "--frame",
        "iphone-15",
        "--overlay",
        "fps",
        "--overlay",
        "hit",
        "--port",
        "42000",
        "--no-spawn",
    ];
    let opts = parse_cli_args(raw_args).expect("valid CLI args");
    assert_eq!(
        opts,
        ServeOptions {
            example: Some("02_mvvm_pattern".to_string()),
            app: None,
            frame: Some(DeviceFramePreset::Iphone15),
            watch_path: None,
            port: 42000,
            overlays: vec![PreviewOverlayKind::Fps, PreviewOverlayKind::Hit],
            no_spawn: true,
            host: "127.0.0.1".to_string(),
            android: false,
            panel: false,
        }
    );

    verify_host_android_and_frame_cli_args();
    verify_serve_session_watch_and_read_errors();
    assert!(matches!(
        parse_cli_args(["--help"]),
        Err(CliParseError::HelpRequested(_))
    ));
    assert!(matches!(
        parse_cli_args(["serve", "--unknown-flag"]),
        Err(CliParseError::InvalidArgs(_))
    ));
}

fn verify_host_android_and_frame_cli_args() {
    use krui_cli::{default_krui_path_for_target, try_setup_adb_reverse_with_bin};

    let android_opts = parse_cli_args([
        "serve",
        "--host",
        "0.0.0.0",
        "--android",
        "--watch",
        "examples/component_gallery/views/component_gallery.krui",
    ])
    .expect("valid host and android args");
    assert_eq!(android_opts.host, "0.0.0.0");
    assert!(android_opts.android);

    let inline_host = parse_cli_args(["serve", "--host=127.0.0.1"]).expect("inline host allowed");
    assert_eq!(inline_host.host, "127.0.0.1");
    assert!(parse_cli_args(["serve", "--host="]).is_err());
    assert!(try_setup_adb_reverse_with_bin("nonexistent-adb-binary-for-test", 41731).is_err());
    assert_eq!(
        parse_cli_args(["serve", "--frame=pixel-8"]).unwrap().frame,
        Some(DeviceFramePreset::Pixel8)
    );
    assert_eq!(
        parse_cli_args(["serve", "--frame", "rpi-800x480"])
            .unwrap()
            .frame,
        Some(DeviceFramePreset::Rpi800x480)
    );
    for ex in [None, Some("component_gallery")] {
        assert!(default_krui_path_for_target(ex).is_file());
    }
}

fn verify_serve_session_watch_and_read_errors() {
    use krui_cli::ServeSession;

    let missing_path = std::env::temp_dir().join("krui_non_existent_watch_file.krui");
    let _ = std::fs::remove_file(&missing_path);
    assert!(ServeSession::start(&missing_path, 0, None).is_err());

    let temp_dir = std::env::temp_dir().join("krui_b_fix_3_watch_dir");
    let views_dir = temp_dir.join("views");
    let src_dir = temp_dir.join("src");
    let _ = std::fs::create_dir_all(&views_dir);
    let _ = std::fs::create_dir_all(&src_dir);
    let temp_krui = views_dir.join("watch_test.krui");
    let temp_rs = src_dir.join("main.rs");
    std::fs::write(
        &temp_krui,
        r#"<div class="box"><span>정상 초기 화면</span></div>"#,
    )
    .unwrap();
    std::fs::write(&temp_rs, "fn main() {}\n").unwrap();
    let mut session = ServeSession::start_with_host(&temp_krui, "127.0.0.1", 0, None).unwrap();
    assert!(!session.poll_rust_changes());
    std::thread::sleep(Duration::from_millis(15));
    std::fs::write(&temp_rs, "fn main() { /* modified */ }\n").unwrap();
    assert!(session.poll_rust_changes());
    let mut watcher = preview::DevFileWatcher::new(&temp_krui).unwrap();
    let mut tree = ViewTree::new();
    assert!(watcher.force_reload(&mut tree).is_some());
    std::fs::remove_file(&temp_krui).unwrap();
    assert!(watcher.force_reload(&mut tree).is_none());
    let read_err = watcher.last_compile_error().expect("read failure error");
    assert!(read_err.file.is_some() && read_err.message.contains("Failed to read"));
    assert!(session.poll_file_changes().unwrap().is_err());
    std::fs::write(
        &temp_krui,
        r#"<div class="box"><span>복구된 화면</span></div>"#,
    )
    .unwrap();
    assert!(watcher.force_reload(&mut tree).is_some());
    assert!(watcher.last_compile_error().is_none());
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn watcher_lazy_overlay_text_engine_and_drop_join() {
    let dir = std::env::temp_dir().join(format!("krui_lazy_watch_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let krui_file = dir.join("app.krui");
    std::fs::write(&krui_file, "<div><p>Clean</p></div>").unwrap();

    let mut watcher = preview::DevFileWatcher::new(&krui_file).unwrap();
    let tree = ViewTree::new();
    let mut scene = PaintScene::new(320, 240, Color::BLACK);
    watcher.decorate_paint_scene(&tree, &mut scene, 100);
    assert!(!watcher.has_initialized_overlay_text_engine());

    watcher.set_overlay(preview::PreviewOverlayKind::Fps, true);
    watcher.decorate_paint_scene(&tree, &mut scene, 100);
    assert!(watcher.has_initialized_overlay_text_engine());

    watcher.start_background_worker(|| {});
    let start = std::time::Instant::now();
    drop(watcher);
    assert!(start.elapsed() < Duration::from_secs(1));
    let _ = std::fs::remove_dir_all(&dir);
}
