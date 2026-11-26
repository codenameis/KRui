const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { OP } = require("../src/panel_protocol");

test("VS Code OP constants match Rust panel_codec.rs OP_CMD_* 1-to-1", () => {
  const codecPath = path.resolve(__dirname, "../../../crates/cli/src/panel_codec.rs");
  const codecSrc = fs.readFileSync(codecPath, "utf8");
  const rustOps = {};
  const re = /pub const OP_CMD_([A-Z0-9_]+):\s*u8\s*=\s*0x([0-9a-fA-F]+);/g;
  let match;
  while ((match = re.exec(codecSrc)) !== null) {
    rustOps[match[1]] = Number.parseInt(match[2], 16);
  }
  assert.ok(Object.keys(rustOps).length >= 15, "expected OP_CMD_* constants in panel_codec.rs");
  assert.deepStrictEqual(OP, rustOps);
  assert.strictEqual(OP.SET_COLOR_SCHEME, 0x0b);
});

test("webview.html uses pure-CSS System/Light/Dark scheme options", () => {
  const htmlPath = path.resolve(__dirname, "../media/webview.html");
  const html = fs.readFileSync(htmlPath, "utf8");
  for (const expected of ['value="system"', 'value="light"', 'value="dark"']) {
    assert.ok(html.includes(expected), `missing ${expected} in webview.html`);
  }
  for (const legacy of ['value="fluent"', 'value="liquid-glass"', 'value="cosmic"']) {
    assert.ok(!html.includes(legacy), `legacy option ${legacy} must stay removed`);
  }
});
