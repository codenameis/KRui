const { spawn } = require("child_process");

const OP = {
  REFRESH: 0x01,
  RESIZE: 0x02,
  POINTER_DOWN: 0x03,
  POINTER_MOVE: 0x04,
  POINTER_UP: 0x05,
  POINTER_LEAVE: 0x06,
  WHEEL: 0x07,
  KEY_DOWN: 0x08,
  IME_PREEDIT: 0x09,
  IME_COMMIT: 0x0a,
  SET_THEME: 0x0b,
  SET_DEVICE_FRAME: 0x0c,
  SET_OVERLAY: 0x0d,
  RESTART: 0x0e,
  INSPECT_CLICK: 0x0f,
  BUFFER_UPDATE: 0x10,
  BUFFER_SAVED: 0x11,
  SHUTDOWN: 0xff,
};

const MAX_PACKET_BYTES = 32 * 1024 * 1024;

function buildFramedPacket(opcode, body) {
  const payload = body || Buffer.alloc(0);
  const frame = Buffer.allocUnsafe(5 + payload.length);
  frame.writeUInt32LE(1 + payload.length, 0);
  frame[4] = opcode;
  if (payload.length > 0) {
    payload.copy(frame, 5);
  }
  return frame;
}

function encodeBufferUpdatePayload(filePath, content) {
  const pathBuf = Buffer.from(filePath, "utf8");
  const contentBuf = Buffer.from(content, "utf8");
  const payload = Buffer.allocUnsafe(4 + pathBuf.length + contentBuf.length);
  payload.writeUInt32LE(pathBuf.length, 0);
  pathBuf.copy(payload, 4);
  contentBuf.copy(payload, 4 + pathBuf.length);
  return payload;
}

function f32Pair(x, y) {
  const b = Buffer.allocUnsafe(8);
  b.writeFloatLE(Number(x || 0), 0);
  b.writeFloatLE(Number(y || 0), 4);
  return b;
}

function clampU16(v, lo, hi) {
  return Math.max(lo, Math.min(hi, Math.round(Number(v || lo))));
}

function encodePointerOrResizeCommand(msg) {
  switch (msg.type) {
    case "resize": {
      const b = Buffer.allocUnsafe(4);
      b.writeUInt16LE(clampU16(msg.width, 240, 2560), 0);
      b.writeUInt16LE(clampU16(msg.height, 200, 2160), 2);
      return { opcode: OP.RESIZE, body: b };
    }
    case "pointerDown": {
      const b = Buffer.allocUnsafe(18);
      b.writeFloatLE(Number(msg.x || 0), 0);
      b.writeFloatLE(Number(msg.y || 0), 4);
      b[8] = Number(msg.button || 0) & 0xff;
      b[9] = (msg.shift ? 1 : 0) | (msg.ctrl ? 2 : 0) | (msg.alt ? 4 : 0);
      b.writeBigUInt64LE(BigInt(Math.max(0, Math.round(Number(msg.timeMs || 0)))), 10);
      return { opcode: OP.POINTER_DOWN, body: b };
    }
    case "pointerMove":
      return { opcode: OP.POINTER_MOVE, body: f32Pair(msg.x, msg.y) };
    case "pointerUp":
      return { opcode: OP.POINTER_UP, body: f32Pair(msg.x, msg.y) };
    case "pointerLeave":
      return { opcode: OP.POINTER_LEAVE, body: null };
    case "wheel": {
      const b = Buffer.allocUnsafe(12);
      b.writeFloatLE(Number(msg.x || 0), 0);
      b.writeFloatLE(Number(msg.y || 0), 4);
      b.writeFloatLE(Number(msg.deltaY || 0), 8);
      return { opcode: OP.WHEEL, body: b };
    }
    case "inspectClick":
      return { opcode: OP.INSPECT_CLICK, body: f32Pair(msg.x, msg.y) };
    default:
      return null;
  }
}

function encodeWebviewCommand(msg) {
  const ptr = encodePointerOrResizeCommand(msg);
  if (ptr) return ptr;
  switch (msg.type) {
    case "keyDown": {
      const keyBuf = Buffer.from(String(msg.key || ""), "utf8");
      const b = Buffer.allocUnsafe(1 + keyBuf.length);
      b[0] = (msg.shift ? 1 : 0) | (msg.ctrl ? 2 : 0) | (msg.alt ? 4 : 0) | (msg.repeat ? 8 : 0);
      keyBuf.copy(b, 1);
      return { opcode: OP.KEY_DOWN, body: b };
    }
    case "imePreedit": {
      const textBuf = Buffer.from(String(msg.text || ""), "utf8");
      const b = Buffer.allocUnsafe(8 + textBuf.length);
      b.writeUInt32LE(Math.max(0, Number(msg.cursorStart || 0)), 0);
      b.writeUInt32LE(Math.max(0, Number(msg.cursorEnd || 0)), 4);
      textBuf.copy(b, 8);
      return { opcode: OP.IME_PREEDIT, body: b };
    }
    case "imeCommit":
      return { opcode: OP.IME_COMMIT, body: Buffer.from(String(msg.text || ""), "utf8") };
    case "setTheme":
      return { opcode: OP.SET_THEME, body: Buffer.from(String(msg.slug || "fluent"), "utf8") };
    case "setDeviceFrame":
      return { opcode: OP.SET_DEVICE_FRAME, body: Buffer.from(String(msg.slug || "none"), "utf8") };
    case "setOverlay": {
      const slugBuf = Buffer.from(String(msg.slug || ""), "utf8");
      const b = Buffer.allocUnsafe(1 + slugBuf.length);
      b[0] = msg.enabled ? 1 : 0;
      slugBuf.copy(b, 1);
      return { opcode: OP.SET_OVERLAY, body: b };
    }
    case "restart":
      return { opcode: OP.RESTART, body: null };
    default:
      return null;
  }
}

function forwardWebviewCommand(msg, sendPacket) {
  const encoded = encodeWebviewCommand(msg);
  if (encoded) {
    sendPacket(encoded.opcode, encoded.body);
  }
}

function terminateChildProcessTree(child) {
  if (!child || child.exitCode !== null) return;
  try {
    if (process.platform === "win32" && child.pid) {
      const killer = spawn("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
        windowsHide: true,
        stdio: "ignore",
      });
      killer.on("error", () => {});
    } else if (!child.killed) {
      child.kill();
    }
  } catch (_) {}
}

module.exports = {
  OP,
  MAX_PACKET_BYTES,
  buildFramedPacket,
  encodeBufferUpdatePayload,
  forwardWebviewCommand,
  terminateChildProcessTree,
};
