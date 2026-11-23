import fs from "node:fs";
import path from "node:path";
import net from "node:net";
import zlib from "node:zlib";
import { spawn, execFileSync } from "node:child_process";

const outDir = path.resolve(process.argv[2] || "target/examples_before");
fs.mkdirSync(outDir, { recursive: true });

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) {
    c ^= buf[i];
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return (c ^ 0xffffffff) >>> 0;
}

function pngChunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const t = Buffer.from(type, "ascii");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([t, data])), 0);
  return Buffer.concat([len, t, data, crc]);
}

function encodeRgbaPng(width, height, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0;
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  const idat = zlib.deflateSync(raw);
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    pngChunk("IHDR", ihdr),
    pngChunk("IDAT", idat),
    pngChunk("IEND", Buffer.alloc(0)),
  ]);
}

function decodeKrfr(buf) {
  const w = buf.readUInt16LE(8);
  const h = buf.readUInt16LE(10);
  const rgba = Buffer.alloc(w * h * 4);
  let out = 0;
  for (let i = 12; i + 4 < buf.length; i += 5) {
    const count = buf[i] + 1;
    const r = buf[i + 1], g = buf[i + 2], b = buf[i + 3], a = buf[i + 4];
    for (let k = 0; k < count; k++) {
      rgba[out] = r; rgba[out + 1] = g; rgba[out + 2] = b; rgba[out + 3] = a;
      out += 4;
    }
  }
  return { w, h, rgba };
}

async function dumpOneExample(kruiExe, exampleName) {
  return new Promise((resolve, reject) => {
    const proc = spawn(kruiExe, ["serve", "--panel", "--port", "0", "--example", exampleName], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let connected = false;
    let done = false;
    let sock = null;
    const timer = setTimeout(() => {
      if (sock) sock.destroy();
      proc.kill();
      reject(new Error(`Timeout dumping ${exampleName}: ${stdout} ${stderr}`));
    }, 60000);
    proc.stderr.on("data", (chunk) => {
      stderr += chunk.toString("utf8");
    });
    proc.on("exit", (code) => {
      if (!done && !connected) {
        clearTimeout(timer);
        reject(new Error(`krui serve exited (${code}) for ${exampleName}: ${stderr}`));
      }
    });

    proc.stdout.on("data", (chunk) => {
      stdout += chunk.toString("utf8");
      const m = stdout.match(/PANEL_ADDR=([0-9.:]+)/);
      if (m && !connected) {
        connected = true;
        const [host, portStr] = m[1].split(":");
        sock = net.createConnection({ host, port: Number(portStr) });
        let acc = Buffer.alloc(0);
        sock.on("data", (data) => {
          acc = Buffer.concat([acc, data]);
          while (acc.length >= 4) {
            const bodyLen = acc.readUInt32LE(0);
            if (acc.length < 4 + bodyLen) break;
            const opcode = acc[4];
            const payload = acc.subarray(5, 4 + bodyLen);
            acc = acc.subarray(4 + bodyLen);
            if (opcode === 0x01 && payload.length >= 12) {
              const { w, h, rgba } = decodeKrfr(payload);
              const png = encodeRgbaPng(w, h, rgba);
              const outPath = path.join(outDir, `${exampleName}.png`);
              fs.writeFileSync(outPath, png);
              done = true;
              const bye = Buffer.from([1, 0, 0, 0, 0xff]);
              sock.write(bye, () => {
                clearTimeout(timer);
                sock.end();
                resolve({ exampleName, w, h, bytes: png.length });
              });
              return;
            }
          }
        });
        sock.on("error", (err) => { clearTimeout(timer); proc.kill(); reject(err); });
      }
    });
    proc.on("error", (err) => { clearTimeout(timer); reject(err); });
  });
}

execFileSync("cargo", ["build", "-p", "krui-cli", "--bin", "krui"], { stdio: "inherit" });
execFileSync("cargo", ["build", "--features", "preview", "--examples"], { stdio: "inherit" });
const kruiExe = path.resolve("target/debug/krui.exe");
const examples = [
  "01_business_studio",
  "02_mvvm_pattern",
  "03_mvc_pattern",
  "04_elm_flux_pattern",
  "05_mvi_pattern",
  "07_controls",
  "09_tailwind_v4",
];
for (const ex of examples) {
  const res = await dumpOneExample(kruiExe, ex);
  console.log(`[dump] ${res.exampleName}: ${res.w}x${res.h} (${res.bytes} bytes)`);
}
