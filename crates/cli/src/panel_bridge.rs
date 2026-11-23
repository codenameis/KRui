//! Bidirectional TCP bridge between `krui serve --panel` and embedded IDE side panels.

use crate::panel_codec::{
    encode_json_packet, encode_krfr_packet, format_building_json, format_compile_error_json,
    format_compile_ok_json, format_jump_to_source_json, format_state_sync_json, PanelEditorCmd,
    PanelSocketConn,
};
use crate::{
    build_preview_app_target, spawn_staged_after_build, try_setup_adb_reverse, ChildKillGuard,
    ServeOptions, ServeSession,
};
use krui_compiler::CompileError;
use krui_preview::{
    DeviceFramePreset, InspectedNodeWire, PanelFrameState, PanelInputEvent, PreviewCommand,
    PreviewOverlayKind, PreviewResponse,
};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const VIRTUAL_COMPILE_DEBOUNCE: Duration = Duration::from_millis(30);
const PANEL_LOOP_SLEEP: Duration = Duration::from_millis(12);
const BUILD_POLL_SLEEP: Duration = Duration::from_millis(30);

/// Non-blocking TCP bridge server accepting IDE side-panel hosts (`extension.js`) and
/// multiplexing `"KRFR"` binary framebuffers, `stateSync` / diagnostic JSON, input events,
/// and unsaved editor buffer updates onto an active [`ServeSession`].
pub struct PanelBridgeServer {
    listener: TcpListener,
    local_addr: SocketAddr,
    clients: Vec<PanelSocketConn>,
    had_client: bool,
    shutdown_requested: bool,
    active_frame: Option<DeviceFramePreset>,
    pending_inspect_click: Option<(f32, f32)>,
    dirty_virtual_since: Option<Instant>,
    last_status_packet: Option<Vec<u8>>,
    last_compile_packet: Option<Vec<u8>>,
    last_krfr_packet: Option<Vec<u8>>,
}

impl PanelBridgeServer {
    /// Binds a non-blocking TCP bridge listener on `addr` (e.g. `"127.0.0.1:0"`).
    pub fn bind(addr: &str) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let local_addr = listener.local_addr()?;
        Ok(Self {
            listener,
            local_addr,
            clients: Vec::new(),
            had_client: false,
            shutdown_requested: false,
            active_frame: None,
            pending_inspect_click: None,
            dirty_virtual_since: None,
            last_status_packet: None,
            last_compile_packet: None,
            last_krfr_packet: None,
        })
    }

    /// Returns the bound local socket address (`127.0.0.1:<port>`).
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Returns the number of currently connected IDE panel clients.
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Returns `true` when `Shutdown` was received or the IDE client closed the socket.
    pub fn should_exit(&self) -> bool {
        self.shutdown_requested || (self.had_client && self.clients.is_empty())
    }

    /// Sets the tracked device frame preset used when mapping inspect-click coordinates.
    pub fn set_active_frame(&mut self, frame: Option<DeviceFramePreset>) {
        self.active_frame = frame;
    }

    /// Broadcasts `{"kind":"building",...}` to connected clients and caches it for late joiners.
    pub fn broadcast_building(&mut self, target: &str, message: &str) {
        let pkt = encode_json_packet(&format_building_json(target, message));
        self.last_status_packet = Some(pkt.clone());
        self.broadcast_raw(&pkt);
    }

    /// Seeds initial `compileOk` / `compileError` packet from `session` startup status.
    pub fn seed_initial_compile_state(&mut self, session: &ServeSession) {
        let path_str = session.watched_krui_path().to_string_lossy().into_owned();
        let json = match session.initial_compile_error() {
            Some(err) => format_compile_error_json(err, &path_str),
            None => format_compile_ok_json(&path_str, 0),
        };
        self.last_compile_packet = Some(encode_json_packet(&json));
    }

    /// Broadcasts a compile result (`compileOk` or `compileError`) and caches it for new clients.
    pub fn broadcast_compile_result(&mut self, file: &Path, res: &Result<usize, CompileError>) {
        let file_str = file.to_string_lossy();
        let json = match res {
            Ok(bytes) => format_compile_ok_json(&file_str, *bytes),
            Err(err) => format_compile_error_json(err, &file_str),
        };
        let pkt = encode_json_packet(&json);
        self.last_compile_packet = Some(pkt.clone());
        self.broadcast_raw(&pkt);
    }

    /// Accepts waiting clients and drains commands during initial `cargo build`.
    pub fn accept_and_poll_during_build(&mut self, session: &mut ServeSession) {
        self.accept_clients();
        let cmds = self.poll_client_commands();
        for cmd in cmds {
            self.dispatch_editor_cmd(session, cmd);
        }
        self.flush_virtual_buffers(session, false);
    }

    /// Executes one non-blocking bridge tick: accepts clients, handles editor commands,
    /// flushes debounced virtual buffers, and forwards `ServeSession` frames/inspections.
    pub fn step(&mut self, session: &mut ServeSession) -> bool {
        self.accept_clients();
        let cmds = self.poll_client_commands();
        let had_cmds = !cmds.is_empty();
        for cmd in cmds {
            self.dispatch_editor_cmd(session, cmd);
        }
        self.flush_virtual_buffers(session, false);
        let responses = session.poll_responses();
        let had_responses = !responses.is_empty();
        for resp in responses {
            self.handle_preview_response(session, resp);
        }
        had_cmds || had_responses
    }

    fn accept_clients(&mut self) {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let Ok(mut conn) = PanelSocketConn::new(stream) else {
                        continue;
                    };
                    for cached in [
                        &self.last_status_packet,
                        &self.last_compile_packet,
                        &self.last_krfr_packet,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        let _ = conn.send_raw_packet(cached);
                    }
                    self.clients.push(conn);
                    self.had_client = true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }

    fn poll_client_commands(&mut self) -> Vec<PanelEditorCmd> {
        let mut cmds = Vec::new();
        let mut idx = 0;
        while idx < self.clients.len() {
            let drop_client = match drain_socket_cmds(&mut self.clients[idx], &mut cmds) {
                Ok(()) => false,
                Err(_) => true,
            };
            if drop_client {
                self.clients.swap_remove(idx);
            } else {
                idx += 1;
            }
        }
        cmds
    }

    fn dispatch_editor_cmd(&mut self, session: &mut ServeSession, cmd: PanelEditorCmd) {
        match cmd {
            PanelEditorCmd::SetDeviceFrame { slug } => {
                let frame = parse_device_frame_slug(&slug);
                self.active_frame = frame;
                session.send_command(&PreviewCommand::SetDeviceFrame(frame));
            }
            PanelEditorCmd::SetOverlay { enabled, slug } => {
                if let Some(kind) = PreviewOverlayKind::parse(&slug) {
                    session.send_command(&PreviewCommand::SetOverlay { kind, enabled });
                }
            }
            PanelEditorCmd::Restart => session.send_command(&PreviewCommand::Restart),
            PanelEditorCmd::InspectClick { x, y } => {
                self.pending_inspect_click = Some((x, y));
                session.send_command(&PreviewCommand::InspectTree);
            }
            PanelEditorCmd::BufferUpdate { path, content } => {
                session.set_virtual_buffer(path, content);
                self.dirty_virtual_since = Some(Instant::now());
            }
            PanelEditorCmd::BufferSaved { path } => {
                session.remove_virtual_buffer(Path::new(&path));
                self.dirty_virtual_since = None;
                let res = session.compile_and_broadcast();
                let target = session.watched_krui_path().to_path_buf();
                self.broadcast_compile_result(&target, &res);
            }
            PanelEditorCmd::Shutdown => self.shutdown_requested = true,
            other => {
                if let Some(ev) = editor_cmd_to_panel_input(other) {
                    session.send_command(&PreviewCommand::PanelInput(ev));
                }
            }
        }
    }

    fn flush_virtual_buffers(&mut self, session: &mut ServeSession, force: bool) {
        let Some(since) = self.dirty_virtual_since else {
            return;
        };
        if !force && since.elapsed() < VIRTUAL_COMPILE_DEBOUNCE {
            return;
        }
        self.dirty_virtual_since = None;
        let res = session.compile_and_broadcast();
        let target = session.watched_krui_path().to_path_buf();
        self.broadcast_compile_result(&target, &res);
    }

    fn handle_preview_response(&mut self, session: &ServeSession, resp: PreviewResponse) {
        match resp {
            PreviewResponse::PanelFrame(state) => self.emit_panel_frame(state),
            PreviewResponse::TreeInspection(nodes) => self.resolve_inspect_click(session, &nodes),
            _ => {}
        }
    }

    fn emit_panel_frame(&mut self, mut state: PanelFrameState) {
        let krfr = std::mem::take(&mut state.krfr_bytes);
        let state_pkt = encode_json_packet(&format_state_sync_json(&state));
        self.last_status_packet = Some(state_pkt.clone());
        self.broadcast_raw(&state_pkt);
        if !krfr.is_empty() {
            let krfr_pkt = encode_krfr_packet(&krfr);
            self.last_krfr_packet = Some(krfr_pkt.clone());
            self.broadcast_raw(&krfr_pkt);
        }
    }

    fn resolve_inspect_click(&mut self, session: &ServeSession, nodes: &[InspectedNodeWire]) {
        let Some((wx, wy)) = self.pending_inspect_click.take() else {
            return;
        };
        let mapped = match self.active_frame {
            Some(preset) => preset.map_window_to_screen_point(wx, wy),
            None => Some((wx, wy)),
        };
        let Some((sx, sy)) = mapped else {
            return;
        };
        let best = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.source_line > 0 && rect_contains(n.rect, sx, sy))
            .max_by_key(|(idx, n)| (n.depth, *idx))
            .map(|(_, n)| n);
        if let Some(node) = best {
            let json = format_jump_to_source_json(
                &session.watched_krui_path().to_string_lossy(),
                node.source_line as usize,
                node.source_column.max(1) as usize,
                &node.tag,
                node.element_id.as_deref(),
            );
            self.broadcast_raw(&encode_json_packet(&json));
        }
    }

    fn broadcast_raw(&mut self, packet: &[u8]) {
        let mut idx = 0;
        while idx < self.clients.len() {
            if self.clients[idx].send_raw_packet(packet).is_err() {
                self.clients.swap_remove(idx);
            } else {
                idx += 1;
            }
        }
    }
}

fn drain_socket_cmds(
    conn: &mut PanelSocketConn,
    out: &mut Vec<PanelEditorCmd>,
) -> std::io::Result<()> {
    while let Some(cmd) = conn.try_recv_cmd()? {
        out.push(cmd);
    }
    Ok(())
}

fn rect_contains(rect: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    x >= rect.0 && y >= rect.1 && x <= rect.0 + rect.2 && y <= rect.1 + rect.3
}

fn parse_device_frame_slug(slug: &str) -> Option<DeviceFramePreset> {
    let trimmed = slug.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") || trimmed == "desktop" {
        None
    } else {
        DeviceFramePreset::parse(trimmed)
    }
}

fn editor_cmd_to_panel_input(cmd: PanelEditorCmd) -> Option<PanelInputEvent> {
    match cmd {
        PanelEditorCmd::RequestRefresh => Some(PanelInputEvent::RequestRefresh),
        PanelEditorCmd::Resize { width, height } => Some(PanelInputEvent::Resize { width, height }),
        PanelEditorCmd::PointerDown {
            x,
            y,
            button,
            shift,
            ctrl,
            alt,
            time_ms,
        } => Some(PanelInputEvent::PointerDown {
            x,
            y,
            button,
            shift,
            ctrl,
            alt,
            time_ms,
        }),
        PanelEditorCmd::PointerMove { x, y } => Some(PanelInputEvent::PointerMove { x, y }),
        PanelEditorCmd::PointerUp { x, y } => Some(PanelInputEvent::PointerUp { x, y }),
        PanelEditorCmd::PointerLeave => Some(PanelInputEvent::PointerLeave),
        PanelEditorCmd::Wheel { x, y, delta_y } => Some(PanelInputEvent::Wheel { x, y, delta_y }),
        PanelEditorCmd::KeyDown {
            key,
            shift,
            ctrl,
            alt,
            repeat,
        } => Some(PanelInputEvent::KeyDown {
            key,
            shift,
            ctrl,
            alt,
            repeat,
        }),
        PanelEditorCmd::ImePreedit {
            cursor_start,
            cursor_end,
            text,
        } => Some(PanelInputEvent::ImePreedit {
            text,
            cursor_start,
            cursor_end,
        }),
        PanelEditorCmd::ImeCommit { text } => Some(PanelInputEvent::ImeCommit { text }),
        _ => None,
    }
}

pub(crate) fn run_panel_serve(opts: ServeOptions, watch_path: PathBuf) -> Result<(), String> {
    use std::io::Write;
    let mut session = ServeSession::start_with_host(&watch_path, &opts.host, opts.port, opts.frame)
        .map_err(|e| format!("Failed to start `krui serve --panel`: {e}"))?;
    let mut bridge = PanelBridgeServer::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to bind panel bridge socket: {e}"))?;
    bridge.set_active_frame(opts.frame);
    bridge.seed_initial_compile_state(&session);
    println!("[krui serve] PANEL_ADDR={}", bridge.local_addr());
    let _ = std::io::stdout().flush();

    if opts.android {
        let _ = try_setup_adb_reverse(session.local_addr().port());
    }
    for &ov in &opts.overlays {
        session.send_command(&PreviewCommand::SetOverlay {
            kind: ov,
            enabled: true,
        });
    }
    let mut child_guard = ChildKillGuard(None);
    rebuild_panel_child_async(
        &opts,
        &watch_path,
        &mut session,
        &mut bridge,
        &mut child_guard,
        true,
    )?;

    while !bridge.should_exit() {
        let active = bridge.step(&mut session);
        if let Some(err) = crate::process_stage::poll_exited_preview_child(&mut child_guard.0) {
            let ce = CompileError::new(1, 1, err);
            bridge.broadcast_compile_result(&watch_path, &Err(ce));
        }
        if let Some(change) = session.poll_file_changes() {
            bridge.broadcast_compile_result(&watch_path, &change);
        }
        if session.poll_rust_changes() {
            rebuild_panel_child_async(
                &opts,
                &watch_path,
                &mut session,
                &mut bridge,
                &mut child_guard,
                false,
            )?;
        }
        if !active {
            std::thread::sleep(PANEL_LOOP_SLEEP);
        }
    }
    Ok(())
}

fn rebuild_panel_child_async(
    opts: &ServeOptions,
    watch_path: &Path,
    session: &mut ServeSession,
    bridge: &mut PanelBridgeServer,
    child_guard: &mut ChildKillGuard,
    initial: bool,
) -> Result<(), String> {
    if opts.no_spawn || opts.android {
        return Ok(());
    }
    let app_pkg = crate::args::infer_app_package_from_opts(opts);
    let target = opts
        .example
        .as_deref()
        .or(app_pkg.as_deref())
        .unwrap_or("krui-showcase");
    let msg = if initial {
        format!("Compiling `{target}` with `--features preview`...")
    } else {
        format!("Rebuilding `{target}`...")
    };
    bridge.broadcast_building(target, &msg);
    let build_opts = opts.clone();
    let handle = std::thread::spawn(move || build_preview_app_target(&build_opts));
    while !handle.is_finished() {
        bridge.accept_and_poll_during_build(session);
        if bridge.should_exit() {
            break;
        }
        std::thread::sleep(BUILD_POLL_SLEEP);
    }
    let build_res = handle
        .join()
        .map_err(|_| "Preview build worker thread panicked".to_string())?;
    if bridge.should_exit() {
        return Ok(());
    }
    match build_res {
        Ok(()) => {
            if let Some(mut old_child) = child_guard.0.take() {
                let _ = old_child.kill();
                let _ = old_child.wait();
            }
            let spawned = spawn_staged_after_build(opts, session.local_addr())
                .map_err(|e| format!("Failed to spawn staged panel preview process: {e}"))?;
            child_guard.0 = Some(spawned);
            Ok(())
        }
        Err(err) => {
            let ce = CompileError::new(1, 1, err);
            bridge.broadcast_compile_result(watch_path, &Err(ce));
            Ok(())
        }
    }
}
