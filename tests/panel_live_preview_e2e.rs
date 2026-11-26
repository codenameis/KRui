//! End-to-end integration test for `krui serve --panel`, `PanelBridgeServer`,
//! `EmbeddedPanelDriver`, real `krui-renderer` `"KRFR"` streaming, Korean IME, and `--watch` resolution.

#![cfg(feature = "preview")]
#![forbid(unsafe_code)]

use krui::dev_hot_reload::{
    compile_krui_source, decode_rle_rgba_frame, mount_compiled_document_with_registry,
};
use krui::*;
use krui_cli::panel_codec::{
    OP_CMD_BUFFER_UPDATE, OP_CMD_IME_COMMIT, OP_CMD_IME_PREEDIT, OP_CMD_INSPECT_CLICK,
    OP_CMD_POINTER_DOWN, OP_CMD_POINTER_UP, OP_CMD_SET_COLOR_SCHEME, OP_CMD_SHUTDOWN,
    OP_SERVER_JSON, OP_SERVER_KRFR,
};
use krui_cli::{
    parse_cli_args, preview_binary_path_for_opts, resolve_effective_target, PanelBridgeServer,
    ServeOptions, ServeSession,
};
use krui_winit::embed_panel::EmbeddedPanelDriver;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

struct NoopHooks;
impl WindowEventHooks for NoopHooks {}

#[view_model]
#[derive(Clone)]
struct PanelE2eVm {
    #[state]
    name: String,
    #[state]
    count: i32,
    #[command]
    bump: Command,
}

impl PanelE2eVm {
    fn new() -> Self {
        let name = Signal::new(String::new());
        let count = Signal::new(0_i32);
        let c = count;
        let bump = Command::new(move || c.update(|v| *v += 1));
        Self { name, count, bump }
    }
}

struct TestEditorClient {
    stream: TcpStream,
    buf: Vec<u8>,
    jsons: Vec<String>,
    frames: Vec<Vec<u8>>,
}

impl TestEditorClient {
    fn connect(addr: SocketAddr) -> Self {
        let stream = TcpStream::connect(addr).expect("connect bridge");
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

    fn send_click(&mut self, center: (f32, f32)) {
        let mut down = Vec::with_capacity(18);
        down.extend_from_slice(&center.0.to_le_bytes());
        down.extend_from_slice(&center.1.to_le_bytes());
        down.push(0);
        down.push(0);
        down.extend_from_slice(&10_u64.to_le_bytes());
        self.send_cmd(OP_CMD_POINTER_DOWN, &down);

        let mut up = Vec::with_capacity(8);
        up.extend_from_slice(&center.0.to_le_bytes());
        up.extend_from_slice(&center.1.to_le_bytes());
        self.send_cmd(OP_CMD_POINTER_UP, &up);
    }

    fn drain(&mut self) {
        let mut tmp = [0u8; 8192];
        loop {
            match self.stream.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("editor socket read error: {e}"),
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
                    .push(String::from_utf8(payload).expect("utf8 json"));
            }
        }
    }
}

fn pump_until<F>(
    driver: &mut EmbeddedPanelDriver<NoopHooks>,
    bridge: &mut PanelBridgeServer,
    session: &mut ServeSession,
    editor: &mut TestEditorClient,
    mut cond: F,
) where
    F: FnMut(&EmbeddedPanelDriver<NoopHooks>, &TestEditorClient, &PanelBridgeServer, bool) -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline {
        let driver_ok = driver.step().is_ok();
        let _ = bridge.step(session);
        editor.drain();
        if cond(driver, editor, bridge, driver_ok) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("Timed out waiting for panel bridge condition");
}

fn node_center(driver: &EmbeddedPanelDriver<NoopHooks>, id_attr: &str) -> (f32, f32) {
    let nid = driver.session.tree.find_by_id(id_attr).expect(id_attr);
    let r = driver.session.tree.node_layout(nid).expect("node layout");
    (r.x + r.width * 0.5, r.y + r.height * 0.5)
}

#[test]
fn resolve_effective_target_infers_example_and_host_page() {
    let gallery = ServeOptions {
        watch_path: Some(PathBuf::from(
            "examples/component_gallery/views/component_gallery.krui",
        )),
        ..Default::default()
    };
    let (ex, path) = resolve_effective_target(&gallery);
    assert_eq!(ex.as_deref(), Some("component_gallery"));
    assert_eq!(
        path,
        PathBuf::from("examples/component_gallery/views/component_gallery.krui")
    );

    let comp = ServeOptions {
        watch_path: Some(PathBuf::from(
            "examples/component_gallery/views/components/GalleryActions.krui",
        )),
        ..Default::default()
    };
    let (ex_card, path_card) = resolve_effective_target(&comp);
    assert_eq!(ex_card.as_deref(), Some("component_gallery"));
    assert_eq!(
        path_card,
        PathBuf::from("examples/component_gallery/views/component_gallery.krui")
    );

    let css = ServeOptions {
        watch_path: Some(PathBuf::from(
            "examples/component_gallery/views/component_gallery.css",
        )),
        ..Default::default()
    };
    let (ex_css, path_css) = resolve_effective_target(&css);
    assert_eq!(ex_css.as_deref(), Some("component_gallery"));
    assert_eq!(
        path_css,
        PathBuf::from("examples/component_gallery/views/component_gallery.krui")
    );

    let nav = ServeOptions {
        watch_path: Some(PathBuf::from("src/views/showcase_nav.krui")),
        ..Default::default()
    };
    let (ex_nav, path_nav) = resolve_effective_target(&nav);
    assert_eq!(ex_nav, None);
    assert_eq!(
        path_nav,
        PathBuf::from("examples/component_gallery/views/component_gallery.krui")
    );
}

#[test]
fn resolve_effective_target_and_binary_for_workspace_apps() {
    let target_dir = std::path::Path::new("target");
    let expected_exe = target_dir
        .join("debug")
        .join(format!("demo-app{}", std::env::consts::EXE_SUFFIX));

    let watch_krui = ServeOptions {
        watch_path: Some(PathBuf::from("apps/demo-app/views/app.krui")),
        ..Default::default()
    };
    let (ex, path) = resolve_effective_target(&watch_krui);
    assert_eq!(ex, None);
    assert_eq!(path, PathBuf::from("apps/demo-app/views/app.krui"));
    assert_eq!(
        preview_binary_path_for_opts(target_dir, &watch_krui),
        expected_exe
    );

    let watch_css = ServeOptions {
        watch_path: Some(PathBuf::from("apps/demo-app/views/app.css")),
        ..Default::default()
    };
    let (_, css_root) = resolve_effective_target(&watch_css);
    assert_eq!(css_root, PathBuf::from("apps/demo-app/views/app.krui"));
    assert_eq!(
        preview_binary_path_for_opts(target_dir, &watch_css),
        expected_exe
    );

    let app_flag = parse_cli_args(["serve", "--app", "demo-app"]).expect("valid --app flag");
    assert_eq!(
        preview_binary_path_for_opts(target_dir, &app_flag),
        expected_exe
    );
}

const INITIAL_KRUISRC: &str = r#"
<style>
  .root { display: flex; flex-direction: column; width: 420px; height: 320px; padding: 16px; gap: 12px; background-color: #111827; }
  .field { width: 260px; height: 38px; background-color: #1f2937; color: #f8fafc; }
  .btn { width: 180px; height: 42px; background-color: #2563eb; color: #ffffff; }
</style>
<div class="root">
  <h2 id="hdr">Count: {vm.count} | {vm.name}</h2>
  <input id="name" class="field" bind:value="{vm.name}" />
  <input id="secret" class="field" type="password" />
  <button id="inc" class="btn" onclick="{vm.bump}">Bump</button>
</div>
"#;

struct PanelE2eHarness {
    driver: EmbeddedPanelDriver<NoopHooks>,
    bridge: PanelBridgeServer,
    session: ServeSession,
    editor: TestEditorClient,
    vm: PanelE2eVm,
    temp_krui: PathBuf,
}

impl Drop for PanelE2eHarness {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.temp_krui);
    }
}

fn setup_panel_e2e() -> PanelE2eHarness {
    let temp_krui =
        std::env::temp_dir().join(format!("krui_panel_e2e_{}.krui", std::process::id()));
    std::fs::write(&temp_krui, INITIAL_KRUISRC).expect("write temp krui");

    let session = ServeSession::start(&temp_krui, 0, None).expect("serve session");
    let bridge = PanelBridgeServer::bind("127.0.0.1:0").expect("bind bridge");
    let mut engine = EngineSession::new_isolated(420, 320, RenderBackendPreference::Auto);
    let vm = engine.reactive_session.enter(PanelE2eVm::new);
    let mut reg = ScopeRegistry::new();
    reg.insert_registry("vm", vm.clone());
    let doc = compile_krui_source(INITIAL_KRUISRC).expect("compile");
    engine.reactive_session.enter(|| {
        let _ = mount_compiled_document_with_registry(&mut engine.tree, &doc, Rc::new(reg));
    });
    let driver = EmbeddedPanelDriver::new_connected(
        engine,
        None,
        NoopHooks,
        &session.local_addr().to_string(),
    )
    .expect("panel driver");
    let editor = TestEditorClient::connect(bridge.local_addr());
    PanelE2eHarness {
        driver,
        bridge,
        session,
        editor,
        vm,
        temp_krui,
    }
}

#[test]
fn panel_live_preview_e2e_renders_inputs_korean_ime_reloads_and_inspects() {
    let mut h = setup_panel_e2e();
    pump_until(
        &mut h.driver,
        &mut h.bridge,
        &mut h.session,
        &mut h.editor,
        |_, ed, _, _| {
            !ed.frames.is_empty() && ed.jsons.iter().any(|j| j.contains(r#""kind":"stateSync""#))
        },
    );
    let (_, w, h_px, initial_rgba) =
        decode_rle_rgba_frame(&h.editor.frames[0]).expect("decode krfr");
    assert_eq!((w, h_px), (420, 320));
    let distinct = initial_rgba
        .as_chunks::<4>()
        .0
        .iter()
        .collect::<std::collections::HashSet<_>>();
    assert!(distinct.len() > 2);

    verify_inspect_and_click(
        &mut h.driver,
        &mut h.bridge,
        &mut h.session,
        &mut h.editor,
        &h.vm,
        &initial_rgba,
    );
    verify_korean_ime_and_password(
        &mut h.driver,
        &mut h.bridge,
        &mut h.session,
        &mut h.editor,
        &h.vm,
    );
    verify_virtual_reload_error_and_shutdown(
        &mut h.driver,
        &mut h.bridge,
        &mut h.session,
        &mut h.editor,
        &h.vm,
        &h.temp_krui,
    );
}

fn verify_inspect_and_click(
    driver: &mut EmbeddedPanelDriver<NoopHooks>,
    bridge: &mut PanelBridgeServer,
    session: &mut ServeSession,
    editor: &mut TestEditorClient,
    vm: &PanelE2eVm,
    initial_rgba: &[u8],
) {
    let inc_center = node_center(driver, "inc");
    let mut inspect_payload = Vec::with_capacity(8);
    inspect_payload.extend_from_slice(&inc_center.0.to_le_bytes());
    inspect_payload.extend_from_slice(&inc_center.1.to_le_bytes());
    editor.jsons.clear();
    editor.send_cmd(OP_CMD_INSPECT_CLICK, &inspect_payload);
    pump_until(&mut *driver, bridge, session, editor, |_, ed, _, _| {
        ed.jsons
            .iter()
            .any(|j| j.contains(r#""kind":"jumpToSource""#) && j.contains(r#""id":"inc""#))
    });

    editor.frames.clear();
    editor.send_click(inc_center);
    pump_until(driver, bridge, session, editor, |drv, ed, _, _| {
        drv.session.reactive_session.enter(|| vm.count.get() == 1) && !ed.frames.is_empty()
    });
    let (_, _, _, updated_rgba) = decode_rle_rgba_frame(editor.frames.last().unwrap()).unwrap();
    assert_ne!(initial_rgba, updated_rgba.as_slice());
}

fn verify_korean_ime_and_password(
    driver: &mut EmbeddedPanelDriver<NoopHooks>,
    bridge: &mut PanelBridgeServer,
    session: &mut ServeSession,
    editor: &mut TestEditorClient,
    vm: &PanelE2eVm,
) {
    editor.jsons.clear();
    editor.send_click(node_center(driver, "name"));
    pump_until(driver, bridge, session, editor, |_, ed, _, _| {
        ed.jsons
            .iter()
            .any(|j| j.contains(r#""imeFocused":true"#) && j.contains(r#""isPassword":false"#))
    });

    let mut preedit = Vec::new();
    preedit.extend_from_slice(&0_u32.to_le_bytes());
    preedit.extend_from_slice(&("한".len() as u32).to_le_bytes());
    preedit.extend_from_slice("한".as_bytes());
    editor.send_cmd(OP_CMD_IME_PREEDIT, &preedit);
    editor.send_cmd(OP_CMD_IME_COMMIT, "한글".as_bytes());
    pump_until(driver, bridge, session, editor, |drv, _, _, _| {
        drv.session
            .reactive_session
            .enter(|| vm.name.get() == "한글")
    });

    editor.jsons.clear();
    editor.send_click(node_center(driver, "secret"));
    pump_until(driver, bridge, session, editor, |_, ed, _, _| {
        ed.jsons
            .iter()
            .any(|j| j.contains(r#""imeFocused":false"#) && j.contains(r#""isPassword":true"#))
    });
}

fn verify_virtual_reload_error_and_shutdown(
    driver: &mut EmbeddedPanelDriver<NoopHooks>,
    bridge: &mut PanelBridgeServer,
    session: &mut ServeSession,
    editor: &mut TestEditorClient,
    vm: &PanelE2eVm,
    temp_krui: &std::path::Path,
) {
    let edited = INITIAL_KRUISRC.replace("Bump</button>", "Bump Live</button>");
    let path_str = temp_krui.to_string_lossy();
    let mut payload = Vec::new();
    payload.extend_from_slice(&(path_str.len() as u32).to_le_bytes());
    payload.extend_from_slice(path_str.as_bytes());
    payload.extend_from_slice(edited.as_bytes());
    editor.jsons.clear();
    editor.send_cmd(OP_CMD_BUFFER_UPDATE, &payload);
    std::thread::sleep(Duration::from_millis(45));
    pump_until(driver, bridge, session, editor, |_, ed, _, _| {
        ed.jsons.iter().any(|j| j.contains(r#""kind":"compileOk""#))
            && ed.jsons.iter().any(|j| j.contains(r#""reloadCount":2"#))
    });
    assert_eq!(
        driver.session.reactive_session.enter(|| vm.name.get()),
        "한글"
    );

    let mut bad_payload = Vec::new();
    bad_payload.extend_from_slice(&(path_str.len() as u32).to_le_bytes());
    bad_payload.extend_from_slice(path_str.as_bytes());
    bad_payload.extend_from_slice(b"<BogusComponent>invalid</BogusComponent>");
    editor.jsons.clear();
    editor.send_cmd(OP_CMD_BUFFER_UPDATE, &bad_payload);
    std::thread::sleep(Duration::from_millis(45));
    pump_until(driver, bridge, session, editor, |_, ed, _, driver_ok| {
        driver_ok
            && ed
                .jsons
                .iter()
                .any(|j| j.contains(r#""kind":"compileError""#))
    });

    editor.send_cmd(OP_CMD_SHUTDOWN, &[]);
    pump_until(driver, bridge, session, editor, |_, _, br, _| {
        br.should_exit()
    });
    assert!(bridge.should_exit());
}

#[test]
fn panel_color_scheme_command_updates_scheme_without_disconnect() {
    let mut h = setup_panel_e2e();
    pump_until(
        &mut h.driver,
        &mut h.bridge,
        &mut h.session,
        &mut h.editor,
        |_, ed, _, _| ed.jsons.iter().any(|j| j.contains(r#""kind":"stateSync""#)),
    );

    h.editor.jsons.clear();
    h.editor.send_cmd(OP_CMD_SET_COLOR_SCHEME, b"dark");
    pump_until(
        &mut h.driver,
        &mut h.bridge,
        &mut h.session,
        &mut h.editor,
        |_, ed, _, _| ed.jsons.iter().any(|j| j.contains(r#""theme":"dark""#)),
    );
    assert!(!h.bridge.should_exit());

    h.editor.jsons.clear();
    h.editor.send_cmd(OP_CMD_SET_COLOR_SCHEME, b"light");
    pump_until(
        &mut h.driver,
        &mut h.bridge,
        &mut h.session,
        &mut h.editor,
        |_, ed, _, _| ed.jsons.iter().any(|j| j.contains(r#""theme":"light""#)),
    );

    // Unknown/legacy slugs must be ignored cleanly without dropping the TCP session.
    h.editor.send_cmd(OP_CMD_SET_COLOR_SCHEME, b"fluent");
    let _ = h.driver.step();
    let _ = h.bridge.step(&mut h.session);
    assert!(!h.bridge.should_exit());

    h.editor.send_cmd(OP_CMD_SHUTDOWN, &[]);
    pump_until(
        &mut h.driver,
        &mut h.bridge,
        &mut h.session,
        &mut h.editor,
        |_, _, br, _| br.should_exit(),
    );
    assert!(h.bridge.should_exit());
}
