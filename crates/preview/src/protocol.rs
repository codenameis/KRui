//! TCP preview protocol (`PreviewCommand`, `PreviewResponse`), device frame presets (`DeviceFramePreset`),
//! debug overlay flags (`PreviewOverlayFlags`), and non-blocking buffered client/server transport for `krui serve`.

use krui_compiler::CompileError;
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};

/// Current wire protocol version for `krui serve` (`PreviewCommand` / `PreviewResponse`).
pub const PREVIEW_PROTOCOL_VERSION: u32 = 1;

/// Default local TCP port used by `krui serve` when `KRUI_SERVE_ADDR` is not set.
pub const DEFAULT_PREVIEW_PORT: u16 = 41731;

/// Environment variable set by `krui serve` to pass the server `127.0.0.1:<port>` address to the child app.
pub const KRUI_SERVE_ADDR_ENV: &str = "KRUI_SERVE_ADDR";

/// Environment variable set by `krui serve --frame <preset>` to activate device frame bezel rendering.
pub const KRUI_DEVICE_FRAME_ENV: &str = "KRUI_DEVICE_FRAME";

pub use crate::device_frame::{
    DeviceFramePreset, InspectedNodeWire, PreviewOverlayFlags, PreviewOverlayKind,
};

/// Command sent from `krui serve` to a connected preview client (or vice versa for `InspectTree`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PreviewCommand {
    /// Negotiates protocol and IR format version (`PREVIEW_PROTOCOL_VERSION`, `IR_FORMAT_VERSION`).
    Hello {
        /// Wire protocol version.
        protocol_version: u32,
        /// `CompiledDocument` IR format version.
        ir_version: u32,
    },
    /// `reload(ir)`: Sends a versioned `CompiledDocument` IR binary bundle (`encode_compiled_document`)
    /// to hot-reload the running `ViewTree` while preserving widget state.
    Reload(Vec<u8>),
    /// Sends a structured `CompileError` when `.krui`/`.css` fails to compile so the preview window
    /// renders a non-destructive error overlay while keeping the previous UI alive.
    CompileError(CompileError),
    /// `restart`: Resets the preview `ViewTree` without preserving previous `<input>`/scroll state.
    Restart,
    /// `inspect_tree`: Requests a snapshot of the live `ViewTree` hierarchy and layout rectangles.
    InspectTree,
    /// `set_overlay(relayout|fps|hit)`: Toggles a visual debug overlay on the preview window.
    SetOverlay {
        /// Which debug overlay to configure.
        kind: PreviewOverlayKind,
        /// Whether the overlay is enabled.
        enabled: bool,
    },
    /// Sets or clears the active [`DeviceFramePreset`] at runtime.
    SetDeviceFrame(Option<DeviceFramePreset>),
    /// Forwards an interactive input or host control event from an embedded IDE panel.
    PanelInput(crate::panel_wire::PanelInputEvent),
}

/// Response sent from the preview client back to `krui serve`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PreviewResponse {
    /// Acknowledges a `Reload` or `Restart` with the number of mounted element nodes and reload count.
    Reloaded {
        /// Number of alive element nodes in the `ViewTree`.
        node_count: usize,
        /// Cumulative reload count.
        reload_count: u64,
    },
    /// Requests a full binary rebuild and child restart because the wire protocol or IR version mismatched.
    VersionMismatch {
        /// Expected version on the client.
        expected: u32,
        /// Actual version received from the server.
        found: u32,
    },
    /// Reports an IR decode/version error (`IrCodecError`) or command error.
    Error(String),
    /// Returns the live `ViewTree` node hierarchy for `inspect_tree`.
    TreeInspection(Vec<InspectedNodeWire>),
    /// Streams a rendered framebuffer (`"KRFR"`) and live engine/IME state back to an embedded IDE panel.
    PanelFrame(crate::panel_wire::PanelFrameState),
    /// Acknowledges an overlay or device frame update.
    Ack,
}

const MAX_FRAME_LEN: usize = 32 * 1024 * 1024;
const MAX_PENDING_WRITE_BYTES: usize = MAX_FRAME_LEN * 2;

/// Encodes `msg` into a length-prefixed (`[u32 LE length][postcard payload]`) byte vector.
pub fn encode_length_prefixed_vec<T: Serialize>(msg: &T) -> io::Result<Vec<u8>> {
    let payload =
        postcard::to_stdvec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if payload.len() > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Frame exceeds 32 MiB limit",
        ));
    }
    let len = payload.len() as u32;
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Persistent per-connection non-blocking read/write buffer that safely handles fragmented TCP
/// headers/payloads, coalesced back-to-back frames, and partial socket writes without ever switching
/// the socket into blocking mode.
pub struct FramedTcpConnection {
    stream: TcpStream,
    read_buf: Vec<u8>,
    write_buf: Vec<u8>,
}

impl FramedTcpConnection {
    /// Wraps a `TcpStream` in non-blocking mode with empty read/write buffers.
    pub fn new(stream: TcpStream) -> Self {
        let _ = stream.set_nodelay(true);
        let _ = stream.set_nonblocking(true);
        Self {
            stream,
            read_buf: Vec::with_capacity(4096),
            write_buf: Vec::with_capacity(4096),
        }
    }

    /// Queues a serialized message and flushes as many bytes as the non-blocking socket currently accepts.
    pub fn send<T: Serialize>(&mut self, msg: &T) -> io::Result<()> {
        self.flush_write_buf()?;
        let framed = encode_length_prefixed_vec(msg)?;
        if self.write_buf.len().saturating_add(framed.len()) > MAX_PENDING_WRITE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Preview TCP outbound buffer full",
            ));
        }
        self.write_buf.extend_from_slice(&framed);
        self.flush_write_buf()
    }

    /// Flushes pending outbound bytes in `write_buf` until empty or `WouldBlock`.
    pub fn flush_write_buf(&mut self) -> io::Result<()> {
        while !self.write_buf.is_empty() {
            match self.stream.write(&self.write_buf) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "TCP write returned 0",
                    ))
                }
                Ok(n) => {
                    self.write_buf.drain(..n);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Reads available bytes from the non-blocking socket into `read_buf` and decodes the next
    /// complete frame if all `4 + len` bytes have arrived.
    pub fn try_recv<T: for<'de> Deserialize<'de>>(&mut self) -> io::Result<Option<T>> {
        let _ = self.flush_write_buf();
        if let Some(decoded) = self.try_decode_buffered_frame()? {
            return Ok(Some(decoded));
        }
        let mut scratch = [0u8; 8192];
        loop {
            if self.read_buf.len() > MAX_FRAME_LEN + 4 {
                break;
            }
            match self.stream.read(&mut scratch) {
                Ok(0) => {
                    if self.read_buf.is_empty() {
                        return Err(io::Error::new(io::ErrorKind::ConnectionReset, "EOF"));
                    }
                    break;
                }
                Ok(n) => self.read_buf.extend_from_slice(&scratch[..n]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        self.try_decode_buffered_frame()
    }

    fn try_decode_buffered_frame<T: for<'de> Deserialize<'de>>(&mut self) -> io::Result<Option<T>> {
        decode_frame_from_buf(&mut self.read_buf)
    }

    /// Clones the underlying `TcpStream` so a background reader thread can block for socket readiness
    /// and wake the UI event loop without periodic timers.
    pub fn try_clone_stream(&self) -> io::Result<TcpStream> {
        self.stream.try_clone()
    }

    /// Flushes outbound bytes, switches the underlying `TcpStream` back into blocking mode,
    /// and returns `(stream, pending_read_buf)` for dedicated background reader/writer threads.
    pub fn into_blocking_parts(mut self) -> io::Result<(TcpStream, Vec<u8>)> {
        let _ = self.flush_write_buf();
        self.stream.set_nonblocking(false)?;
        Ok((self.stream, self.read_buf))
    }
}

/// Decodes one complete `[u32 LE length][postcard payload]` frame from `read_buf` if present.
pub fn decode_frame_from_buf<T: for<'de> Deserialize<'de>>(
    read_buf: &mut Vec<u8>,
) -> io::Result<Option<T>> {
    if read_buf.len() < 4 {
        return Ok(None);
    }
    let len = u32::from_le_bytes([read_buf[0], read_buf[1], read_buf[2], read_buf[3]]) as usize;
    if len > MAX_FRAME_LEN {
        read_buf.clear();
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Frame exceeds 32 MiB limit",
        ));
    }
    if read_buf.len() < 4 + len {
        return Ok(None);
    }
    let decoded = postcard::from_bytes(&read_buf[4..4 + len])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    read_buf.drain(..4 + len);
    Ok(Some(decoded))
}

/// Blocks on `stream` until the next length-prefixed frame arrives in `read_buf` or `stop` is set.
pub fn read_blocking_frame<T: for<'de> Deserialize<'de>>(
    stream: &mut TcpStream,
    read_buf: &mut Vec<u8>,
    stop: &std::sync::atomic::AtomicBool,
) -> io::Result<Option<T>> {
    loop {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(None);
        }
        if let Some(decoded) = decode_frame_from_buf(read_buf)? {
            return Ok(Some(decoded));
        }
        let mut scratch = [0u8; 8192];
        match stream.read(&mut scratch) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::ConnectionReset, "EOF")),
            Ok(n) => read_buf.extend_from_slice(&scratch[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::Interrupted
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Writes a length-prefixed (`[u32 LE length][postcard payload]`) message to `stream`.
pub fn write_length_prefixed<T: Serialize>(stream: &mut TcpStream, msg: &T) -> io::Result<()> {
    let framed = encode_length_prefixed_vec(msg)?;
    stream.write_all(&framed)?;
    stream.flush()
}

/// Non-blocking TCP server hosted by `krui serve` that accepts preview app connections and broadcasts
/// [`PreviewCommand`]s (`reload(ir)`, `CompileError`, `restart`, `inspect_tree`, `set_overlay`).
pub struct PreviewTcpServer {
    listener: TcpListener,
    local_addr: SocketAddr,
    clients: Vec<FramedTcpConnection>,
    latest_ir_bundle: Option<Vec<u8>>,
    latest_error: Option<CompileError>,
    latest_frame: Option<DeviceFramePreset>,
    latest_overlays: PreviewOverlayFlags,
}

impl PreviewTcpServer {
    /// Binds a non-blocking `PreviewTcpServer` on `addr` (e.g. `"127.0.0.1:0"` or `"127.0.0.1:41731"`).
    pub fn bind(addr: &str) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let local_addr = listener.local_addr()?;
        Ok(Self {
            listener,
            local_addr,
            clients: Vec::new(),
            latest_ir_bundle: None,
            latest_error: None,
            latest_frame: None,
            latest_overlays: PreviewOverlayFlags::default(),
        })
    }

    /// Returns the bound local [`SocketAddr`].
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Returns the number of currently connected preview clients.
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Sets the initial [`DeviceFramePreset`] sent to newly connected preview clients.
    pub fn set_device_frame(&mut self, frame: Option<DeviceFramePreset>) {
        self.latest_frame = frame;
        self.broadcast(&PreviewCommand::SetDeviceFrame(frame));
    }

    /// Accepts any pending incoming client connections and replays the latest protocol handshake,
    /// device frame, active debug overlays, and IR/error state.
    pub fn accept_pending(&mut self) {
        while let Ok((stream, _)) = self.listener.accept() {
            let mut conn = FramedTcpConnection::new(stream);
            let _ = conn.send(&PreviewCommand::Hello {
                protocol_version: PREVIEW_PROTOCOL_VERSION,
                ir_version: krui_compiler::IR_FORMAT_VERSION,
            });
            if let Some(frame) = self.latest_frame {
                let _ = conn.send(&PreviewCommand::SetDeviceFrame(Some(frame)));
            }
            for (kind, enabled) in [
                (PreviewOverlayKind::Relayout, self.latest_overlays.relayout),
                (PreviewOverlayKind::Fps, self.latest_overlays.fps),
                (PreviewOverlayKind::Hit, self.latest_overlays.hit),
            ] {
                if enabled {
                    let _ = conn.send(&PreviewCommand::SetOverlay {
                        kind,
                        enabled: true,
                    });
                }
            }
            if let Some(err) = &self.latest_error {
                let _ = conn.send(&PreviewCommand::CompileError(err.clone()));
            } else if let Some(ir) = &self.latest_ir_bundle {
                let _ = conn.send(&PreviewCommand::Reload(ir.clone()));
            }
            self.clients.push(conn);
        }
    }

    /// Broadcasts `cmd` to all connected preview clients, caching the latest `Reload`, `CompileError`,
    /// `SetOverlay`, or `SetDeviceFrame` for future clients and dropping disconnected sockets.
    pub fn broadcast(&mut self, cmd: &PreviewCommand) {
        self.accept_pending();
        match cmd {
            PreviewCommand::Reload(ir) => {
                self.latest_ir_bundle = Some(ir.clone());
                self.latest_error = None;
            }
            PreviewCommand::CompileError(err) => {
                self.latest_error = Some(err.clone());
            }
            PreviewCommand::SetOverlay { kind, enabled } => {
                self.latest_overlays.set(*kind, *enabled);
            }
            PreviewCommand::SetDeviceFrame(frame) => {
                self.latest_frame = *frame;
            }
            _ => {}
        }
        self.clients.retain_mut(|conn| conn.send(cmd).is_ok());
    }

    /// Polls all connected clients for any available [`PreviewResponse`] frames without blocking.
    pub fn poll_responses(&mut self) -> Vec<PreviewResponse> {
        self.accept_pending();
        let mut out = Vec::new();
        self.clients.retain_mut(|conn| loop {
            match conn.try_recv::<PreviewResponse>() {
                Ok(Some(resp)) => out.push(resp),
                Ok(None) => return true,
                Err(_) => return false,
            }
        });
        out
    }
}
