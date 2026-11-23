//! Length-prefixed TCP framing and JSON status/error formatting between `krui serve --panel`
//! and IDE side-panel hosts (VS Code extension host + webview).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

pub use crate::panel_json::{
    format_building_json, format_compile_error_json, format_compile_ok_json,
    format_jump_to_source_json, format_state_sync_json, json_escape,
};

/// Maximum permitted `[u32 LE body_len]` packet size across the IDE panel TCP bridge (32 MiB).
pub const MAX_PANEL_PACKET_BYTES: usize = 32 * 1024 * 1024;
/// Server -> Editor opcode carrying raw `"KRFR"` binary RLE framebuffer bytes.
pub const OP_SERVER_KRFR: u8 = 0x01;
/// Server -> Editor opcode carrying UTF-8 JSON control/state/diagnostic messages.
pub const OP_SERVER_JSON: u8 = 0x02;

/// Editor -> Server opcode: request a fresh framebuffer emission.
pub const OP_CMD_REFRESH: u8 = 0x01;
/// Editor -> Server opcode: resize the panel viewport (`[u16 w][u16 h]`).
pub const OP_CMD_RESIZE: u8 = 0x02;
/// Editor -> Server opcode: pointer button press.
pub const OP_CMD_POINTER_DOWN: u8 = 0x03;
/// Editor -> Server opcode: pointer move or active drag.
pub const OP_CMD_POINTER_MOVE: u8 = 0x04;
/// Editor -> Server opcode: pointer button release / drag end.
pub const OP_CMD_POINTER_UP: u8 = 0x05;
/// Editor -> Server opcode: pointer exited panel canvas.
pub const OP_CMD_POINTER_LEAVE: u8 = 0x06;
/// Editor -> Server opcode: mouse wheel scroll event.
pub const OP_CMD_WHEEL: u8 = 0x07;
/// Editor -> Server opcode: non-IME keyboard or shortcut event.
pub const OP_CMD_KEY_DOWN: u8 = 0x08;
/// Editor -> Server opcode: OS IME composition preedit text update.
pub const OP_CMD_IME_PREEDIT: u8 = 0x09;
/// Editor -> Server opcode: committed IME or printable UTF-8 text.
pub const OP_CMD_IME_COMMIT: u8 = 0x0A;
/// Editor -> Server opcode: switch active `DeviceFramePreset` or desktop mode.
pub const OP_CMD_SET_DEVICE_FRAME: u8 = 0x0C;
/// Editor -> Server opcode: toggle diagnostic overlay (`relayout`, `fps`, `hit`).
pub const OP_CMD_SET_OVERLAY: u8 = 0x0D;
/// Editor -> Server opcode: remount/restart the preview UI tree.
pub const OP_CMD_RESTART: u8 = 0x0E;
/// Editor -> Server opcode: inspect widget under `(x, y)` to jump to source.
pub const OP_CMD_INSPECT_CLICK: u8 = 0x0F;
/// Editor -> Server opcode: supply unsaved `.krui`/`.css` buffer contents.
pub const OP_CMD_BUFFER_UPDATE: u8 = 0x10;
/// Editor -> Server opcode: clear unsaved buffer override on disk save.
pub const OP_CMD_BUFFER_SAVED: u8 = 0x11;
/// Editor -> Server opcode: gracefully close panel bridge and preview child.
pub const OP_CMD_SHUTDOWN: u8 = 0xFF;

const WRITE_DEADLINE_MS: u64 = 500;

/// Decoded command sent from an IDE host to `krui serve --panel`.
#[derive(Clone, Debug, PartialEq)]
pub enum PanelEditorCmd {
    /// Requests a forced frame resend.
    RequestRefresh,
    /// Resizes the logical panel viewport when no device bezel preset is active.
    Resize { width: u16, height: u16 },
    /// Left/secondary pointer press in outer canvas coordinates.
    PointerDown {
        x: f32,
        y: f32,
        button: u8,
        shift: bool,
        ctrl: bool,
        alt: bool,
        time_ms: u64,
    },
    /// Pointer move or active drag in outer canvas coordinates.
    PointerMove { x: f32, y: f32 },
    /// Pointer release / drag end in outer canvas coordinates.
    PointerUp { x: f32, y: f32 },
    /// Cursor exited the preview canvas.
    PointerLeave,
    /// Wheel scroll (`delta_y > 0` scrolls down).
    Wheel { x: f32, y: f32, delta_y: f32 },
    /// Non-text keyboard or shortcut event.
    KeyDown {
        key: String,
        shift: bool,
        ctrl: bool,
        alt: bool,
        repeat: bool,
    },
    /// Active OS IME preedit string and UTF-8 caret/selection byte offsets.
    ImePreedit {
        cursor_start: usize,
        cursor_end: usize,
        text: String,
    },
    /// Committed IME or printable character text.
    ImeCommit { text: String },
    /// Switches device bezel preset (`none`, `iphone-15`, `pixel-8`, `rpi-800x480`).
    SetDeviceFrame { slug: String },
    /// Toggles a debug overlay (`relayout`, `fps`, `hit`).
    SetOverlay { enabled: bool, slug: String },
    /// Remounts/restarts the preview UI tree cleanly.
    Restart,
    /// Requests source location (`jumpToSource`) for widget under `(x, y)`.
    InspectClick { x: f32, y: f32 },
    /// Supplies unsaved editor buffer contents for zero-save live compilation.
    BufferUpdate { path: String, content: String },
    /// Clears dirty buffer override for `path` after disk save.
    BufferSaved { path: String },
    /// Terminates the panel bridge and preview child process cleanly.
    Shutdown,
}

/// Non-blocking framed TCP socket connection (`[u32 LE body_len][u8 opcode][payload]`)
/// used between `PanelBridgeServer` and each connected IDE panel client.
pub struct PanelSocketConn {
    stream: TcpStream,
    read_buf: Vec<u8>,
}

impl PanelSocketConn {
    /// Wraps a newly accepted client socket in non-blocking `TCP_NODELAY` mode.
    pub fn new(stream: TcpStream) -> std::io::Result<Self> {
        stream.set_nonblocking(true)?;
        let _ = stream.set_nodelay(true);
        Ok(Self {
            stream,
            read_buf: Vec::new(),
        })
    }

    /// Reads any queued bytes without blocking and returns the next decoded [`PanelEditorCmd`].
    pub fn try_recv_cmd(&mut self) -> std::io::Result<Option<PanelEditorCmd>> {
        if let Some(cmd) = try_take_buffered_cmd(&mut self.read_buf)? {
            return Ok(Some(cmd));
        }
        let mut chunk = [0u8; 8192];
        let mut saw_eof = false;
        while self.read_buf.len() <= MAX_PANEL_PACKET_BYTES + 4 {
            match self.stream.read(&mut chunk) {
                Ok(0) => {
                    saw_eof = true;
                    break;
                }
                Ok(n) => self.read_buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        if let Some(cmd) = try_take_buffered_cmd(&mut self.read_buf)? {
            return Ok(Some(cmd));
        }
        if self.read_buf.len() > MAX_PANEL_PACKET_BYTES + 4 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Panel client read buffer exceeded maximum frame size",
            ));
        }
        if saw_eof {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "IDE panel client disconnected",
            ));
        }
        Ok(None)
    }

    /// Writes a pre-framed `[u32 LE len][u8 opcode][payload]` packet reliably to the socket.
    pub fn send_raw_packet(&mut self, packet: &[u8]) -> std::io::Result<()> {
        let mut written = 0usize;
        let deadline = Instant::now() + Duration::from_millis(WRITE_DEADLINE_MS);
        while written < packet.len() {
            match self.stream.write(&packet[written..]) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        "Panel client write returned 0",
                    ));
                }
                Ok(n) => written += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "Panel client write timed out",
                        ));
                    }
                    std::thread::sleep(Duration::from_micros(200));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

fn try_take_buffered_cmd(read_buf: &mut Vec<u8>) -> std::io::Result<Option<PanelEditorCmd>> {
    if read_buf.len() < 4 {
        return Ok(None);
    }
    let body_len = u32::from_le_bytes(read_buf[..4].try_into().unwrap_or([0; 4])) as usize;
    if !(1..=MAX_PANEL_PACKET_BYTES).contains(&body_len) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid panel frame length {body_len}"),
        ));
    }
    let total = 4 + body_len;
    if read_buf.len() < total {
        return Ok(None);
    }
    let opcode = read_buf[4];
    let payload = read_buf[5..total].to_vec();
    read_buf.drain(..total);
    decode_editor_cmd(opcode, &payload)
        .map(Some)
        .map_err(|msg| std::io::Error::new(std::io::ErrorKind::InvalidData, msg))
}

/// Decodes one editor command frame `(opcode, payload)` into [`PanelEditorCmd`].
pub fn decode_editor_cmd(opcode: u8, payload: &[u8]) -> Result<PanelEditorCmd, String> {
    match opcode {
        OP_CMD_REFRESH => Ok(PanelEditorCmd::RequestRefresh),
        OP_CMD_RESIZE => Ok(PanelEditorCmd::Resize {
            width: read_u16(payload, 0)?,
            height: read_u16(payload, 2)?,
        }),
        OP_CMD_POINTER_DOWN | OP_CMD_POINTER_MOVE | OP_CMD_POINTER_UP | OP_CMD_POINTER_LEAVE
        | OP_CMD_WHEEL | OP_CMD_INSPECT_CLICK => decode_pointer_cmd(opcode, payload),
        OP_CMD_KEY_DOWN | OP_CMD_IME_PREEDIT | OP_CMD_IME_COMMIT => {
            decode_text_cmd(opcode, payload)
        }
        _ => decode_control_cmd(opcode, payload),
    }
}

fn decode_pointer_cmd(opcode: u8, payload: &[u8]) -> Result<PanelEditorCmd, String> {
    match opcode {
        OP_CMD_POINTER_DOWN => {
            let mods = *payload.get(9).ok_or("Short PointerDown mods")?;
            Ok(PanelEditorCmd::PointerDown {
                x: read_f32(payload, 0)?,
                y: read_f32(payload, 4)?,
                button: *payload.get(8).ok_or("Short PointerDown button")?,
                shift: (mods & 1) != 0,
                ctrl: (mods & 2) != 0,
                alt: (mods & 4) != 0,
                time_ms: read_u64(payload, 10)?,
            })
        }
        OP_CMD_POINTER_MOVE => Ok(PanelEditorCmd::PointerMove {
            x: read_f32(payload, 0)?,
            y: read_f32(payload, 4)?,
        }),
        OP_CMD_POINTER_UP => Ok(PanelEditorCmd::PointerUp {
            x: read_f32(payload, 0)?,
            y: read_f32(payload, 4)?,
        }),
        OP_CMD_POINTER_LEAVE => Ok(PanelEditorCmd::PointerLeave),
        OP_CMD_WHEEL => Ok(PanelEditorCmd::Wheel {
            x: read_f32(payload, 0)?,
            y: read_f32(payload, 4)?,
            delta_y: read_f32(payload, 8)?,
        }),
        OP_CMD_INSPECT_CLICK => Ok(PanelEditorCmd::InspectClick {
            x: read_f32(payload, 0)?,
            y: read_f32(payload, 4)?,
        }),
        _ => Err(format!("Unexpected pointer opcode 0x{opcode:02x}")),
    }
}

fn decode_text_cmd(opcode: u8, payload: &[u8]) -> Result<PanelEditorCmd, String> {
    match opcode {
        OP_CMD_KEY_DOWN => {
            let mods = *payload.first().ok_or("Short KeyDown header")?;
            Ok(PanelEditorCmd::KeyDown {
                shift: (mods & 1) != 0,
                ctrl: (mods & 2) != 0,
                alt: (mods & 4) != 0,
                repeat: (mods & 8) != 0,
                key: read_utf8(&payload[1..])?,
            })
        }
        OP_CMD_IME_PREEDIT => Ok(PanelEditorCmd::ImePreedit {
            cursor_start: read_u32(payload, 0)? as usize,
            cursor_end: read_u32(payload, 4)? as usize,
            text: read_utf8(payload.get(8..).ok_or("Short ImePreedit payload")?)?,
        }),
        OP_CMD_IME_COMMIT => Ok(PanelEditorCmd::ImeCommit {
            text: read_utf8(payload)?,
        }),
        _ => Err(format!("Unexpected text opcode 0x{opcode:02x}")),
    }
}

fn decode_control_cmd(opcode: u8, payload: &[u8]) -> Result<PanelEditorCmd, String> {
    match opcode {
        OP_CMD_SET_DEVICE_FRAME => Ok(PanelEditorCmd::SetDeviceFrame {
            slug: read_utf8(payload)?,
        }),
        OP_CMD_SET_OVERLAY => {
            let enabled = *payload.first().ok_or("Short SetOverlay flag")? != 0;
            Ok(PanelEditorCmd::SetOverlay {
                enabled,
                slug: read_utf8(&payload[1..])?,
            })
        }
        OP_CMD_RESTART => Ok(PanelEditorCmd::Restart),
        OP_CMD_BUFFER_UPDATE => {
            let path_len = read_u32(payload, 0)? as usize;
            let end = 4usize
                .checked_add(path_len)
                .ok_or("BufferUpdate path overflow")?;
            let path_slice = payload.get(4..end).ok_or("Short BufferUpdate path")?;
            let content_slice = payload.get(end..).ok_or("Short BufferUpdate content")?;
            Ok(PanelEditorCmd::BufferUpdate {
                path: read_utf8(path_slice)?,
                content: read_utf8(content_slice)?,
            })
        }
        OP_CMD_BUFFER_SAVED => Ok(PanelEditorCmd::BufferSaved {
            path: read_utf8(payload)?,
        }),
        OP_CMD_SHUTDOWN => Ok(PanelEditorCmd::Shutdown),
        other => Err(format!("Unknown panel editor opcode 0x{other:02x}")),
    }
}

/// Frames an encoded `"KRFR"` payload (`0x01`) for streaming over the panel socket.
pub fn encode_krfr_packet(krfr_bytes: &[u8]) -> Vec<u8> {
    encode_server_packet(OP_SERVER_KRFR, krfr_bytes)
}

/// Frames a UTF-8 JSON message (`0x02`) for streaming over the panel socket.
pub fn encode_json_packet(json: &str) -> Vec<u8> {
    encode_server_packet(OP_SERVER_JSON, json.as_bytes())
}

fn encode_server_packet(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let body_len = (1 + payload.len()) as u32;
    let mut out = Vec::with_capacity(5 + payload.len());
    out.extend_from_slice(&body_len.to_le_bytes());
    out.push(opcode);
    out.extend_from_slice(payload);
    out
}

fn read_u16(buf: &[u8], off: usize) -> Result<u16, String> {
    let slice = buf.get(off..off + 2).ok_or("Short u16 payload")?;
    Ok(u16::from_le_bytes(slice.try_into().unwrap_or([0; 2])))
}

fn read_u32(buf: &[u8], off: usize) -> Result<u32, String> {
    let slice = buf.get(off..off + 4).ok_or("Short u32 payload")?;
    Ok(u32::from_le_bytes(slice.try_into().unwrap_or([0; 4])))
}

fn read_f32(buf: &[u8], off: usize) -> Result<f32, String> {
    let slice = buf.get(off..off + 4).ok_or("Short f32 payload")?;
    Ok(f32::from_le_bytes(slice.try_into().unwrap_or([0; 4])))
}

fn read_u64(buf: &[u8], off: usize) -> Result<u64, String> {
    let slice = buf.get(off..off + 8).ok_or("Short u64 payload")?;
    Ok(u64::from_le_bytes(slice.try_into().unwrap_or([0; 8])))
}

fn read_utf8(buf: &[u8]) -> Result<String, String> {
    std::str::from_utf8(buf)
        .map(str::to_string)
        .map_err(|_| "Invalid UTF-8 in panel command".to_string())
}
