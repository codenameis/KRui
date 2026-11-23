//! Non-blocking TCP client (`PreviewTcpClient`) and visual overlay composition re-exports.

pub use crate::overlay_draw::decorate_preview_paint_scene;
pub(crate) use crate::overlay_draw::decorate_tree_with_engine;

use crate::panel_wire::PanelInputEvent;
use crate::protocol::{
    encode_length_prefixed_vec, read_blocking_frame, DeviceFramePreset, FramedTcpConnection,
    InspectedNodeWire, PreviewCommand, PreviewOverlayFlags, PreviewResponse, KRUI_SERVE_ADDR_ENV,
    PREVIEW_PROTOCOL_VERSION,
};
use crate::{
    mount_compiled_document_with_registry, try_hot_patch_compiled_styles_in_place,
    try_reload_compiled_document_with_registry,
};
use krui_compiler::{
    decode_compiled_document, CompileError, CompiledDocument, IrCodecError, IR_FORMAT_VERSION,
};
use krui_core::{PaintScene, ScopeRegistry, ViewTree};
use krui_text::TextEngine;
use std::io::{self, Write};
use std::net::{Shutdown, TcpStream};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

enum ClientTransport {
    NonBlocking(FramedTcpConnection),
    Background {
        cmd_rx: mpsc::Receiver<io::Result<PreviewCommand>>,
        out_tx: Option<mpsc::SyncSender<Vec<u8>>>,
        stop: Arc<AtomicBool>,
        shutdown_stream: TcpStream,
        reader_handle: Option<JoinHandle<()>>,
        writer_handle: Option<JoinHandle<()>>,
    },
    Closed,
}

/// Non-blocking TCP client embedded in `DevFileWatcher` / `StandardWindowHost` that receives
/// live `PreviewCommand`s from `krui serve`.
pub struct PreviewTcpClient {
    transport: ClientTransport,
    last_doc: Option<CompiledDocument>,
    overlays: PreviewOverlayFlags,
    device_frame: Option<DeviceFramePreset>,
    pending_panel_inputs: Vec<PanelInputEvent>,
    disconnected: bool,
    overlay_text_engine: Option<TextEngine>,
}

impl PreviewTcpClient {
    /// Connects to `addr` (e.g. `"127.0.0.1:41731"`) in non-blocking buffered mode.
    pub fn connect(addr: &str) -> Option<Self> {
        if !cfg!(debug_assertions) {
            return None;
        }
        let stream = TcpStream::connect(addr).ok()?;
        Some(Self {
            transport: ClientTransport::NonBlocking(FramedTcpConnection::new(stream)),
            last_doc: None,
            overlays: PreviewOverlayFlags::default(),
            device_frame: DeviceFramePreset::from_env_or_args(),
            pending_panel_inputs: Vec::new(),
            disconnected: false,
            overlay_text_engine: None,
        })
    }

    /// Returns `true` when background blocking TCP reader/writer threads are driving wakeups.
    pub fn has_wakeup_listener(&self) -> bool {
        matches!(self.transport, ClientTransport::Background { .. })
    }

    /// Transitions this client to event-driven background reader/writer threads so idle windows
    /// and embedded panels sleep at 0% CPU without periodic polling timers.
    pub fn start_wakeup_listener(&mut self, wakeup: impl Fn() + Send + 'static) {
        if !matches!(self.transport, ClientTransport::NonBlocking(_)) {
            return;
        }
        let prev = std::mem::replace(&mut self.transport, ClientTransport::Closed);
        let ClientTransport::NonBlocking(conn) = prev else {
            return;
        };
        let Ok((mut writer_stream, read_buf)) = conn.into_blocking_parts() else {
            self.disconnected = true;
            return;
        };
        let (Ok(reader_stream), Ok(shutdown_stream)) =
            (writer_stream.try_clone(), writer_stream.try_clone())
        else {
            self.disconnected = true;
            return;
        };
        let _ = reader_stream.set_read_timeout(Some(Duration::from_millis(40)));
        let stop = Arc::new(AtomicBool::new(false));
        let stop_reader = Arc::clone(&stop);
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (out_tx, out_rx) = mpsc::sync_channel::<Vec<u8>>(32);
        let reader_handle =
            spawn_preview_reader_thread(reader_stream, read_buf, stop_reader, cmd_tx, wakeup);
        let writer_handle = std::thread::spawn(move || {
            while let Ok(bytes) = out_rx.recv() {
                if writer_stream.write_all(&bytes).is_err() || writer_stream.flush().is_err() {
                    break;
                }
            }
        });
        self.transport = ClientTransport::Background {
            cmd_rx,
            out_tx: Some(out_tx),
            stop,
            shutdown_stream,
            reader_handle: Some(reader_handle),
            writer_handle: Some(writer_handle),
        };
    }

    /// Connects to the address specified in `KRUI_SERVE_ADDR`, if present.
    pub fn from_env() -> Option<Self> {
        let addr = std::env::var(KRUI_SERVE_ADDR_ENV).ok()?;
        Self::connect(addr.trim())
    }

    /// Decorates `scene` with any active `CompileError` banner, debug overlays, and device bezel.
    pub fn decorate_paint_scene(
        &mut self,
        tree: &ViewTree,
        scene: &mut PaintScene,
        compile_error: Option<&CompileError>,
        last_frame_us: u128,
    ) {
        decorate_tree_with_engine(
            tree,
            scene,
            &mut self.overlay_text_engine,
            compile_error,
            self.overlays,
            self.device_frame,
            last_frame_us,
        );
    }

    /// Returns `true` if the underlying `krui serve` TCP socket has closed or errored.
    pub fn is_disconnected(&self) -> bool {
        self.disconnected
    }

    /// Drains any pending [`PanelInputEvent`] items queued from the embedded panel.
    pub fn drain_panel_inputs(&mut self) -> Vec<PanelInputEvent> {
        std::mem::take(&mut self.pending_panel_inputs)
    }

    /// Sends a [`PreviewResponse`] back to `krui serve`, marking disconnected on broken sockets.
    pub fn send_response(&mut self, resp: &PreviewResponse) -> std::io::Result<()> {
        let res = match &mut self.transport {
            ClientTransport::NonBlocking(conn) => conn.send(resp),
            ClientTransport::Background { out_tx, .. } => {
                let framed = encode_length_prefixed_vec(resp)?;
                match out_tx {
                    Some(tx) => match tx.try_send(framed) {
                        Ok(()) => Ok(()),
                        Err(mpsc::TrySendError::Full(_)) => Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "Preview TCP outbound channel full",
                        )),
                        Err(mpsc::TrySendError::Disconnected(_)) => Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "Preview TCP writer disconnected",
                        )),
                    },
                    None => Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed")),
                }
            }
            ClientTransport::Closed => Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed")),
        };
        if let Err(ref err) = res {
            if matches!(
                err.kind(),
                std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::UnexpectedEof
            ) {
                self.disconnected = true;
            }
        }
        res
    }

    fn try_recv_next_cmd(&mut self) -> io::Result<Option<PreviewCommand>> {
        match &mut self.transport {
            ClientTransport::NonBlocking(conn) => conn.try_recv::<PreviewCommand>(),
            ClientTransport::Background { cmd_rx, .. } => match cmd_rx.try_recv() {
                Ok(Ok(cmd)) => Ok(Some(cmd)),
                Ok(Err(err)) => Err(err),
                Err(mpsc::TryRecvError::Empty) => Ok(None),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err(io::Error::new(io::ErrorKind::ConnectionReset, "EOF"))
                }
            },
            ClientTransport::Closed => Err(io::Error::new(io::ErrorKind::ConnectionReset, "EOF")),
        }
    }

    /// Returns the active [`PreviewOverlayFlags`].
    pub fn overlays(&self) -> PreviewOverlayFlags {
        self.overlays
    }

    /// Returns the active [`DeviceFramePreset`], if any.
    pub fn device_frame(&self) -> Option<DeviceFramePreset> {
        self.device_frame
    }

    /// Sets the fallback `CompiledDocument` used by `PreviewCommand::Restart` before any TCP `Reload` arrives.
    pub fn set_initial_document(&mut self, doc: CompiledDocument) {
        if self.last_doc.is_none() {
            self.last_doc = Some(doc);
        }
    }

    /// Polls the non-blocking buffered TCP socket and applies any pending [`PreviewCommand`]s to `tree`,
    /// updating `compile_error` and `reload_count` and sending back [`PreviewResponse`]s.
    ///
    /// Returns `true` if any command mutated the tree, error overlay, debug overlay, or device frame.
    pub fn poll(
        &mut self,
        tree: &mut ViewTree,
        compile_error: &mut Option<CompileError>,
        reload_count: &mut u64,
    ) -> bool {
        let mut changed = false;
        loop {
            match self.try_recv_next_cmd() {
                Ok(Some(cmd)) => {
                    changed |= self.apply_command(cmd, tree, compile_error, reload_count);
                }
                Ok(None) => break,
                Err(_) => {
                    self.disconnected = true;
                    break;
                }
            }
        }
        changed
    }

    /// Applies a single [`PreviewCommand`] to `tree` and sends the corresponding [`PreviewResponse`].
    pub fn apply_command(
        &mut self,
        cmd: PreviewCommand,
        tree: &mut ViewTree,
        compile_error: &mut Option<CompileError>,
        reload_count: &mut u64,
    ) -> bool {
        match cmd {
            PreviewCommand::Hello {
                protocol_version,
                ir_version,
            } => self.handle_hello(protocol_version, ir_version),
            PreviewCommand::Reload(ir_bytes) => {
                self.handle_reload(&ir_bytes, tree, compile_error, reload_count)
            }
            PreviewCommand::CompileError(err) => {
                *compile_error = Some(err);
                let _ = self.send_response(&PreviewResponse::Ack);
                true
            }
            PreviewCommand::Restart => self.handle_restart(tree, compile_error, reload_count),
            PreviewCommand::InspectTree => {
                self.handle_inspect_tree(tree);
                false
            }
            PreviewCommand::SetOverlay { kind, enabled } => {
                self.overlays.set(kind, enabled);
                let _ = self.send_response(&PreviewResponse::Ack);
                true
            }
            PreviewCommand::SetDeviceFrame(frame) => {
                self.device_frame = frame;
                let _ = self.send_response(&PreviewResponse::Ack);
                true
            }
            PreviewCommand::PanelInput(ev) => {
                self.pending_panel_inputs.push(ev);
                true
            }
        }
    }

    fn handle_hello(&mut self, protocol_version: u32, ir_version: u32) -> bool {
        if protocol_version != PREVIEW_PROTOCOL_VERSION {
            let _ = self.send_response(&PreviewResponse::VersionMismatch {
                expected: PREVIEW_PROTOCOL_VERSION,
                found: protocol_version,
            });
            return false;
        }
        if ir_version != IR_FORMAT_VERSION {
            let _ = self.send_response(&PreviewResponse::VersionMismatch {
                expected: IR_FORMAT_VERSION,
                found: ir_version,
            });
            return false;
        }
        false
    }

    fn handle_reload(
        &mut self,
        ir_bytes: &[u8],
        tree: &mut ViewTree,
        compile_error: &mut Option<CompileError>,
        reload_count: &mut u64,
    ) -> bool {
        match decode_compiled_document(ir_bytes) {
            Ok(doc) => self.apply_decoded_reload(doc, tree, compile_error, reload_count),
            Err(IrCodecError::VersionMismatch { expected, found }) => {
                let msg = format!(
                    "IR format version mismatch (client expected v{expected}, server sent v{found}); requesting full rebuild/restart"
                );
                *compile_error = Some(CompileError::new(1, 1, msg));
                let _ = self.send_response(&PreviewResponse::VersionMismatch { expected, found });
                true
            }
            Err(codec_err) => {
                let msg = codec_err.to_string();
                *compile_error = Some(CompileError::new(1, 1, msg.clone()));
                let _ = self.send_response(&PreviewResponse::Error(msg));
                true
            }
        }
    }

    fn apply_decoded_reload(
        &mut self,
        doc: CompiledDocument,
        tree: &mut ViewTree,
        compile_error: &mut Option<CompileError>,
        reload_count: &mut u64,
    ) -> bool {
        if !cfg!(debug_assertions) {
            return false;
        }
        let reg = tree
            .preview_registry()
            .unwrap_or_else(|| Rc::new(ScopeRegistry::new()));
        let outcome = match &self.last_doc {
            Some(prev) => match try_hot_patch_compiled_styles_in_place(tree, prev, &doc, &reg) {
                Ok(Some(cnt)) => Ok(cnt),
                Ok(None) => try_reload_compiled_document_with_registry(tree, &doc, reg),
                Err(err) => Err(err),
            },
            None => try_reload_compiled_document_with_registry(tree, &doc, reg),
        };
        match outcome {
            Ok(node_count) => {
                *compile_error = None;
                self.last_doc = Some(doc);
                *reload_count += 1;
                let _ = self.send_response(&PreviewResponse::Reloaded {
                    node_count,
                    reload_count: *reload_count,
                });
                true
            }
            Err(eval_err) => {
                let msg = eval_err.to_string();
                *compile_error = Some(eval_err);
                let _ = self.send_response(&PreviewResponse::Error(msg));
                true
            }
        }
    }

    fn handle_restart(
        &mut self,
        tree: &mut ViewTree,
        compile_error: &mut Option<CompileError>,
        reload_count: &mut u64,
    ) -> bool {
        *compile_error = None;
        if let Some(doc) = self.last_doc.clone() {
            let _ = tree.take_nodes_for_preview_remount();
            let reg = tree
                .preview_registry()
                .unwrap_or_else(|| Rc::new(ScopeRegistry::new()));
            let _ = mount_compiled_document_with_registry(tree, &doc, reg);
            let node_count = tree.node_count();
            *reload_count += 1;
            let _ = self.send_response(&PreviewResponse::Reloaded {
                node_count,
                reload_count: *reload_count,
            });
            true
        } else {
            let _ = self.send_response(&PreviewResponse::Ack);
            false
        }
    }

    fn handle_inspect_tree(&mut self, tree: &ViewTree) {
        let wire_nodes = tree
            .inspect_nodes()
            .into_iter()
            .map(|n| InspectedNodeWire {
                path_key: n.path_key,
                tag: n.tag,
                element_id: n.element_id,
                depth: n.depth,
                rect: (
                    n.world_rect.x,
                    n.world_rect.y,
                    n.world_rect.width,
                    n.world_rect.height,
                ),
                text: n.text,
                source_line: n.source_line,
                source_column: n.source_column,
                padding: n.padding,
                tokens: n.tokens,
            })
            .collect();
        let _ = self.send_response(&PreviewResponse::TreeInspection(wire_nodes));
    }
}

fn spawn_preview_reader_thread(
    mut reader_stream: TcpStream,
    mut read_buf: Vec<u8>,
    stop: Arc<AtomicBool>,
    cmd_tx: mpsc::Sender<io::Result<PreviewCommand>>,
    wakeup: impl Fn() + Send + 'static,
) -> JoinHandle<()> {
    std::thread::spawn(move || loop {
        match read_blocking_frame::<PreviewCommand>(&mut reader_stream, &mut read_buf, &stop) {
            Ok(Some(cmd)) => {
                if cmd_tx.send(Ok(cmd)).is_err() {
                    break;
                }
                krui_signals::wake_ui_loop();
                wakeup();
            }
            Ok(None) => break,
            Err(err) => {
                let _ = cmd_tx.send(Err(err));
                krui_signals::wake_ui_loop();
                wakeup();
                break;
            }
        }
    })
}

impl Drop for PreviewTcpClient {
    fn drop(&mut self) {
        if let ClientTransport::Background {
            out_tx,
            stop,
            shutdown_stream,
            reader_handle,
            writer_handle,
            ..
        } = &mut self.transport
        {
            stop.store(true, Ordering::Release);
            let _ = out_tx.take();
            let _ = shutdown_stream.shutdown(Shutdown::Both);
            if let Some(h) = reader_handle.take() {
                let _ = h.join();
            }
            if let Some(h) = writer_handle.take() {
                let _ = h.join();
            }
        }
    }
}
