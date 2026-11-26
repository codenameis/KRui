//! End-to-end live-preview verification for `examples/component_gallery` on a strict 1 MB stack thread.

#![cfg(feature = "preview")]
#![forbid(unsafe_code)]

#[path = "../examples/component_gallery/controller/mod.rs"]
#[allow(dead_code)]
mod controller;

use controller::GalleryController;
use krui::dev_hot_reload::{
    compile_krui_source_from_path, decode_rle_rgba_frame, mount_compiled_document_with_registry,
};
use krui::*;
use krui_cli::panel_codec::{
    OP_CMD_BUFFER_UPDATE, OP_CMD_SHUTDOWN, OP_SERVER_JSON, OP_SERVER_KRFR,
};
use krui_cli::{resolve_effective_target, PanelBridgeServer, ServeOptions, ServeSession};
use krui_winit::embed_panel::EmbeddedPanelDriver;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

struct NoopHooks;
impl WindowEventHooks for NoopHooks {}

struct EditorBridgeClient {
    stream: TcpStream,
    buf: Vec<u8>,
    jsons: Vec<String>,
    frames: Vec<Vec<u8>>,
}

impl EditorBridgeClient {
    fn connect(addr: SocketAddr) -> Self {
        let stream = TcpStream::connect(addr).expect("connect panel bridge");
        stream.set_nonblocking(true).expect("nonblocking");
        Self {
            stream,
            buf: Vec::new(),
            jsons: Vec::new(),
            frames: Vec::new(),
        }
    }

    fn send_cmd(&mut self, opcode: u8, payload: &[u8]) {
        let _ = self.stream.set_nonblocking(false);
        let mut pkt = Vec::with_capacity(5 + payload.len());
        pkt.extend_from_slice(&((1 + payload.len()) as u32).to_le_bytes());
        pkt.push(opcode);
        pkt.extend_from_slice(payload);
        self.stream.write_all(&pkt).expect("write cmd");
        let _ = self.stream.set_nonblocking(true);
    }

    fn send_buffer_update(&mut self, rel_path: &str, content: &str) {
        let mut payload = Vec::with_capacity(4 + rel_path.len() + content.len());
        payload.extend_from_slice(&(rel_path.len() as u32).to_le_bytes());
        payload.extend_from_slice(rel_path.as_bytes());
        payload.extend_from_slice(content.as_bytes());
        self.send_cmd(OP_CMD_BUFFER_UPDATE, &payload);
    }

    fn drain(&mut self) {
        let mut tmp = [0_u8; 8192];
        loop {
            match self.stream.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("panel bridge socket read error: {e}"),
            }
        }
        while self.buf.len() >= 4 {
            let body_len =
                u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
            if self.buf.len() < 4 + body_len {
                break;
            }
            let opcode = self.buf[4];
            let payload = self.buf[5..4 + body_len].to_vec();
            self.buf.drain(..4 + body_len);
            if opcode == OP_SERVER_KRFR {
                self.frames.push(payload);
            } else if opcode == OP_SERVER_JSON {
                self.jsons
                    .push(String::from_utf8(payload).expect("utf8 server json"));
            }
        }
    }
}

const PUMP_TIMEOUT_SECS: u64 = 30;

fn pump_until<F>(
    driver: &mut EmbeddedPanelDriver<NoopHooks>,
    bridge: &mut PanelBridgeServer,
    session: &mut ServeSession,
    editor: &mut EditorBridgeClient,
    mut cond: F,
) where
    F: FnMut(
        &EmbeddedPanelDriver<NoopHooks>,
        &EditorBridgeClient,
        &PanelBridgeServer,
        bool,
    ) -> bool,
{
    // Liveness timeout only (not a perf budget): one debug-build gallery reload recompiles the 3.9 MB document twice.
    let deadline = Instant::now() + Duration::from_secs(PUMP_TIMEOUT_SECS);
    while Instant::now() < deadline {
        let driver_ok = driver.step().is_ok();
        let _ = bridge.step(session);
        editor.drain();
        if cond(driver, editor, bridge, driver_ok) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!(
        "Timed out waiting for M3 gallery live preview condition. Last JSONs: {:?}",
        editor.jsons
    );
}

fn verify_target_resolution_for_component_gallery() {
    let expected_root = PathBuf::from("examples/component_gallery/views/component_gallery.krui");
    for watch in [
        "examples/component_gallery/views/component_gallery.krui",
        "examples/component_gallery/views/component_gallery.css",
        "examples/component_gallery/views/themes/material.css",
        "examples/component_gallery/views/themes/tailwind.css",
        "examples/component_gallery/views/components/GalleryActions.krui",
        "examples/component_gallery/views/components/GalleryDrawer.krui",
        "examples/component_gallery/views/icons/IconHome.krui",
    ] {
        let opts = ServeOptions {
            watch_path: Some(PathBuf::from(watch)),
            ..Default::default()
        };
        let (ex, root) = resolve_effective_target(&opts);
        assert_eq!(ex.as_deref(), Some("component_gallery"));
        assert_eq!(root, expected_root);
    }
}

fn run_component_gallery_panel_roundtrip_on_current_thread() {
    verify_target_resolution_for_component_gallery();
    let watch_path = Path::new("examples/component_gallery/views/component_gallery.krui");
    let mut session = ServeSession::start(watch_path, 0, None).expect("serve session for gallery");
    let mut bridge = PanelBridgeServer::bind("127.0.0.1:0").expect("bind bridge");
    bridge.seed_initial_compile_state(&session);

    let mut engine = EngineSession::new_isolated(1280, 840, RenderBackendPreference::Auto);
    let ctrl = engine.reactive_session.enter(GalleryController::new);
    let mut reg = ScopeRegistry::new();
    reg.insert_registry("controller", ctrl);
    let src = std::fs::read_to_string(watch_path).expect("read component_gallery.krui");
    let doc =
        compile_krui_source_from_path(watch_path, &src).expect("compile component_gallery.krui");
    engine.reactive_session.enter(|| {
        let _ = mount_compiled_document_with_registry(&mut engine.tree, &doc, Rc::new(reg));
    });

    let mut driver = EmbeddedPanelDriver::new_connected(
        engine,
        None,
        NoopHooks,
        &session.local_addr().to_string(),
    )
    .expect("embedded panel driver");
    let mut editor = EditorBridgeClient::connect(bridge.local_addr());

    pump_until(
        &mut driver,
        &mut bridge,
        &mut session,
        &mut editor,
        |_, ed, _, _| {
            !ed.frames.is_empty() && ed.jsons.iter().any(|j| j.contains(r#""kind":"stateSync""#))
        },
    );
    let (_, w, h_px, initial_rgba) =
        decode_rle_rgba_frame(&editor.frames[0]).expect("decode m3 krfr frame");
    assert_eq!((w, h_px), (1280, 840));
    let distinct: HashSet<&[u8; 4]> = initial_rgba.as_chunks::<4>().0.iter().collect();
    assert!(distinct.len() > 2, "expected non-blank rendered m3 frame");

    verify_virtual_css_and_component_reload(&mut driver, &mut bridge, &mut session, &mut editor);

    editor.send_cmd(OP_CMD_SHUTDOWN, &[]);
    pump_until(
        &mut driver,
        &mut bridge,
        &mut session,
        &mut editor,
        |_, _, br, _| br.should_exit(),
    );
}

fn verify_virtual_css_and_component_reload(
    driver: &mut EmbeddedPanelDriver<NoopHooks>,
    bridge: &mut PanelBridgeServer,
    session: &mut ServeSession,
    editor: &mut EditorBridgeClient,
) {
    let css_path = "examples/component_gallery/views/component_gallery.css";
    let mut css_src = std::fs::read_to_string(css_path).expect("read component_gallery.css");
    css_src.push_str("\n/* live css hot-reload verification */\n");
    editor.jsons.clear();
    editor.send_buffer_update(css_path, &css_src);
    pump_until(driver, bridge, session, editor, |_, ed, _, _| {
        ed.jsons.iter().any(|j| j.contains(r#""kind":"compileOk""#))
            && ed.jsons.iter().any(|j| j.contains(r#""reloadCount":2"#))
    });
    assert!(
        !editor
            .jsons
            .iter()
            .any(|j| j.contains(r#""kind":"compileError""#)),
        "CSS hot-reload produced compileError: {:?}",
        editor.jsons
    );

    let comp_dir = Path::new("examples/component_gallery/views/components");
    let first_comp = std::fs::read_dir(comp_dir)
        .expect("read components dir")
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some("krui"))
        .expect("at least one component .krui");
    let comp_rel = first_comp.to_string_lossy().replace('\\', "/");
    let mut comp_src = std::fs::read_to_string(&first_comp).expect("read component .krui");
    comp_src.push_str("\n<!-- live component hot-reload verification -->\n");
    editor.jsons.clear();
    editor.send_buffer_update(&comp_rel, &comp_src);
    pump_until(driver, bridge, session, editor, |_, ed, _, _| {
        ed.jsons.iter().any(|j| j.contains(r#""kind":"compileOk""#))
            && ed.jsons.iter().any(|j| j.contains(r#""reloadCount":3"#))
    });

    let (_, _, _, before_kit_rgba) =
        decode_rle_rgba_frame(editor.frames.last().expect("frame before kit swap"))
            .expect("decode before kit swap");
    let kit_path = "examples/component_gallery/views/themes/material.css";
    let mut kit_src = std::fs::read_to_string(kit_path).expect("read themes/material.css");
    kit_src.push_str(
        "\n[data-theme=\"material\"] {\n  --md-sys-color-surface: #14362b;\n  --krui-bg: #14362b;\n}\n",
    );
    editor.jsons.clear();
    editor.frames.clear();
    editor.send_buffer_update(kit_path, &kit_src);
    pump_until(driver, bridge, session, editor, |_, ed, _, _| {
        ed.jsons.iter().any(|j| j.contains(r#""kind":"compileOk""#))
            && ed.jsons.iter().any(|j| j.contains(r#""reloadCount":4"#))
            && !ed.frames.is_empty()
    });
    assert!(
        !editor
            .jsons
            .iter()
            .any(|j| j.contains(r#""kind":"compileError""#)),
        "Theme CSS hot-reload produced compileError: {:?}",
        editor.jsons
    );
    let (_, _, _, after_kit_rgba) =
        decode_rle_rgba_frame(editor.frames.last().expect("frame after kit swap"))
            .expect("decode after kit swap");
    assert_ne!(
        before_kit_rgba, after_kit_rgba,
        "Swapping themes/material.css tokens and root background must produce visible framebuffer changes"
    );
}

#[test]
fn component_gallery_live_preview_survives_1mb_main_stack_and_reloads_css_and_components() {
    let handle = std::thread::Builder::new()
        .name("strict-1mb-main".to_string())
        .stack_size(1024 * 1024)
        .spawn(run_component_gallery_panel_roundtrip_on_current_thread)
        .expect("spawn 1MB test thread");
    handle
        .join()
        .expect("component gallery live preview must not overflow a 1 MB stack thread");
}
