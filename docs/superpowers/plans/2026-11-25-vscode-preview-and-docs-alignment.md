# VS Code Live Preview Fixes, Docs Alignment & Build Options Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the verified VS Code Live Preview protocol disconnect bug and UX gaps (`editors/vscode-krui` + `crates/cli` + `crates/winit`), align all drifted repository docs with the current 19-crate Stylo 0.22 / 229/229 conformance architecture, and publish an authoritative build-options reference.

**Architecture:** Replace the deleted legacy `ThemePreset` (`OP_CMD_SET_THEME = 0x0B`) panel command with a pure-CSS `ColorSchemePreference` (`system` / `light` / `dark`) channel across `krui-preview` -> `krui-cli` -> `krui-winit` -> `vscode-krui`, add a Node protocol-parity test against `panel_codec.rs`, extend `--watch` target inference to `views/icons/*.krui`, and synchronize all workspace documentation (`README.md`, `ARCHITECTURE.md`, `CHANGELOG.md`, `docs/TODO.md`, `docs/HANDOFF.md`, `docs/BUILD_OPTIONS.md`, `editors/vscode-krui/README.md`, `.agents/skills/krui-harness2-loop/SKILL.md`).

**Tech Stack:** Rust (`krui-preview`, `krui-cli`, `krui-winit`), Node.js / VS Code Extension API (`editors/vscode-krui`), Markdown docs, PowerShell (`probe_krui.ps1`).

**Spec:** `AGENTS.md` (sections 0–6) and `docs/decisions/0001-v3-1-tier-coexistence.md`

## Global Constraints

- `#![forbid(unsafe_code)]` across all touched Rust crates (`krui-preview`, `krui-cli`, `krui-winit`).
- Pure CSS ownership: no `ThemePreset` / `TokenId` / `TokenTable` restoration; Rust panel controls may only override `@media (prefers-color-scheme)` (`System` / `Light` / `Dark`).
- File length target `<= 500` lines (hard gate `<= 560`), function length target `<= 60` lines (hard gate `<= 80`).
- Zero runtime markup/preview parser in release builds (`cargo tree` gate in `probe_krui.ps1`).
- Windows Cargo commands require `$env:PYTHON3 = "C:\Users\Kim Bohyun\AppData\Local\Python\bin\python3.exe"`.

## Review Focus

- **Legacy/unknown opcode tolerance**: Sending opcode `0x0B` from an older extension or switching Color Scheme in VS Code must never drop the `PanelBridgeServer` socket or kill `krui serve --panel`.
- **Sub-directory `.krui` watch targets**: Opening `examples/component_gallery/views/icons/IconHome.krui` (or `views/components/*.krui`, `views/styles/*.css`, `views/themes/*.css`) and invoking Live Preview must resolve to `examples/component_gallery/views/component_gallery.krui`.
- **Password field paste in Webview**: Pressing `Ctrl+V` / `Cmd+V` while a password `<input>` is focused (`stage` focused, `imeBridge` blurred) must commit pasted text to the preview password field without enabling IME composition.
- **Prebuilt `krui` binary fallback on Windows**: `extension.js` must prefer `krui.cliPath`, then workspace `target/debug/krui[.exe]`, before falling back to `cargo run -q -p krui-cli`, avoiding needless rebuild latency and missing `PYTHON3` failures.
- **Protocol drift prevention**: Automated test in `editors/vscode-krui` must parse `crates/cli/src/panel_codec.rs` opcode constants and fail immediately if `editors/vscode-krui/src/panel_protocol.js` ever diverges again.

---

### Task 1: Replace Dead `OP.SET_THEME` (`0x0B`) with Pure-CSS `ColorScheme` (`system` / `light` / `dark`)

**Files:**
- Modify: `crates/preview/src/lib.rs` (or wire protocol file defining `PreviewCommand` / `PanelInputEvent`)
- Modify: `crates/cli/src/panel_codec.rs:35-110, 311-342`
- Modify: `crates/cli/src/panel_bridge.rs:185-220, 330-380`
- Modify: `crates/winit/src/embed_panel_input.rs:269-318`
- Modify: `crates/winit/src/embed_panel.rs:335-345`
- Test: `tests/panel_live_preview_e2e.rs`

**Interfaces:**
- Consumes: `krui_core::ViewTree::set_color_scheme_preference(ColorSchemePreference)` and `ViewTree::effective_color_scheme() -> ColorScheme`.
- Produces: `OP_CMD_SET_COLOR_SCHEME: u8 = 0x0B`, `PanelEditorCmd::SetColorScheme { slug: String }`, `PanelInputEvent::SetColorScheme { slug: String }` accepting `"system"`, `"light"`, `"dark"` (and ignoring unknown/legacy slugs without dropping the TCP connection).

- [ ] **Step 1: Write the failing test in `tests/panel_live_preview_e2e.rs`**

Add assertion in `panel_live_preview_e2e_renders_inputs_korean_ime_reloads_and_inspects` (or a dedicated test `panel_color_scheme_command_updates_scheme_without_disconnect`):
```rust
#[test]
fn panel_color_scheme_command_updates_scheme_without_disconnect() {
    let mut h = setup_panel_e2e();
    // Wait for initial stateSync, send OP_CMD_SET_COLOR_SCHEME ("dark"), assert stateSync contains `"theme":"dark"` and bridge is still connected (`!bridge.should_exit()`).
    // Then send OP_CMD_SET_COLOR_SCHEME ("light"), assert stateSync contains `"theme":"light"`.
    // Then send OP_CMD_SET_COLOR_SCHEME ("legacy-fluent"), assert bridge stays connected (`!bridge.should_exit()`).
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `$env:PYTHON3 = "C:\Users\Kim Bohyun\AppData\Local\Python\bin\python3.exe"; cargo test --features preview --test panel_live_preview_e2e panel_color_scheme_command_updates_scheme_without_disconnect`
Expected: FAIL (missing `OP_CMD_SET_COLOR_SCHEME` / socket disconnect on `0x0B`).

- [ ] **Step 3: Implement `OP_CMD_SET_COLOR_SCHEME` (`0x0B`) across `preview`, `cli`, and `winit`**

1. Add `PanelInputEvent::SetColorScheme { slug: String }` in `crates/preview`.
2. Add `pub const OP_CMD_SET_COLOR_SCHEME: u8 = 0x0B;` and `PanelEditorCmd::SetColorScheme { slug: String }` in `crates/cli/src/panel_codec.rs` (`decode_control_cmd` matches `OP_CMD_SET_COLOR_SCHEME`).
3. Map `PanelEditorCmd::SetColorScheme { slug }` -> `PanelInputEvent::SetColorScheme { slug }` in `crates/cli/src/panel_bridge.rs`.
4. Handle `PanelInputEvent::SetColorScheme { slug }` in `crates/winit/src/embed_panel_input.rs` calling `session.tree.set_color_scheme_preference(pref)` for `"system"` / `"light"` / `"dark"` (`force_refresh: true`; unknown/legacy slugs are ignored safely without error).

- [ ] **Step 4: Run test to verify it passes**

Run: `$env:PYTHON3 = "C:\Users\Kim Bohyun\AppData\Local\Python\bin\python3.exe"; cargo test --features preview --test panel_live_preview_e2e`
Expected: PASS (4 passed, 0 failed).

- [ ] **Step 5: Commit**

```bash
git add crates/preview crates/cli/src/panel_codec.rs crates/cli/src/panel_bridge.rs crates/winit/src/embed_panel_input.rs tests/panel_live_preview_e2e.rs
git commit -m "fix(preview): replace removed 0x0B theme opcode with pure-CSS color-scheme command"
```

---

### Task 2: Fix `views/icons/*.krui` Watch Target Resolution in `krui-cli`

**Files:**
- Modify: `crates/cli/src/args.rs:300-315`
- Test: `tests/live_preview_component_gallery_e2e.rs:129-147`

**Interfaces:**
- Produces: `resolve_effective_target(&ServeOptions) -> (Option<String>, PathBuf)` resolving `views/icons/*.krui` to the enclosing app's root `.krui`.

- [ ] **Step 1: Add failing assertion in `tests/live_preview_component_gallery_e2e.rs`**

Append `"examples/component_gallery/views/icons/IconHome.krui"` to the `for watch in [...]` array inside `verify_target_resolution_for_component_gallery()`.

- [ ] **Step 2: Run test to verify it fails**

Run: `$env:PYTHON3 = "C:\Users\Kim Bohyun\AppData\Local\Python\bin\python3.exe"; cargo test --features preview --test live_preview_component_gallery_e2e`
Expected: FAIL (`assertion left == right failed: left: "examples/component_gallery/views/icons/IconHome.krui"`).

- [ ] **Step 3: Update `resolve_effective_target` in `crates/cli/src/args.rs:301-307`**

Match parent folder name `matches!(parent_name, Some("components" | "icons"))` so sub-components under `views/icons/` resolve to `fallback_krui_for_path`.

- [ ] **Step 4: Run test to verify it passes**

Run: `$env:PYTHON3 = "C:\Users\Kim Bohyun\AppData\Local\Python\bin\python3.exe"; cargo test --features preview --test live_preview_component_gallery_e2e`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/args.rs tests/live_preview_component_gallery_e2e.rs
git commit -m "fix(cli): resolve views/icons/*.krui watch targets to app root document"
```

---

### Task 3: Update VS Code Extension (`editors/vscode-krui`) & Add Protocol-Parity Test

**Files:**
- Modify: `editors/vscode-krui/package.json:68-100`
- Modify: `editors/vscode-krui/media/webview.html:17-26`
- Modify: `editors/vscode-krui/media/webview.js:44-47, 157-161, 247-255, 339-341`
- Modify: `editors/vscode-krui/src/panel_protocol.js:14, 116-118`
- Modify: `editors/vscode-krui/src/extension.js:90-102, 227-231, 249-251`
- Modify: `editors/vscode-krui/README.md`
- Create: `editors/vscode-krui/test/protocol_parity.test.js`

**Interfaces:**
- Consumes: `OP_CMD_SET_COLOR_SCHEME` (`0x0B`) from `crates/cli/src/panel_codec.rs`.
- Produces: `npm test` (`node --test editors/vscode-krui/test/protocol_parity.test.js`) checking JS syntax and exact opcode parity with `crates/cli/src/panel_codec.rs`.

- [ ] **Step 1: Write failing `editors/vscode-krui/test/protocol_parity.test.js`**

Write a Node built-in `node:test` test that:
1. Parses all `pub const OP_CMD_*: u8 = 0x..;` constants from `crates/cli/src/panel_codec.rs`.
2. Verifies `panel_protocol.js`'s `OP` map has identical hex values and zero unknown opcodes (specifically checking `OP.SET_COLOR_SCHEME === 0x0B` and no `OP.SET_THEME`).
3. Verifies `webview.html` color-scheme `<select>` contains `system`, `light`, `dark` and no legacy `fluent`/`liquid-glass`/`cosmic`.

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test editors/vscode-krui/test/protocol_parity.test.js`
Expected: FAIL (`OP.SET_THEME` mismatch and legacy `<option>` values).

- [ ] **Step 3: Update `vscode-krui` extension files**

1. `package.json`: rename setting `krui.defaultTheme` -> `krui.defaultColorScheme` (`enum: ["system", "light", "dark"]`, default `"system"`), add `"scripts": { "test": "node --test test/protocol_parity.test.js" }`.
2. `media/webview.html`: replace `#theme-select` options with `Scheme`: `System`, `Light`, `Dark` (`value="system"|"light"|"dark"`).
3. `src/panel_protocol.js`: rename `OP.SET_THEME` -> `OP.SET_COLOR_SCHEME: 0x0b` and handle `case "setColorScheme"`.
4. `src/extension.js`:
   - Prefer `krui.cliPath`, then prebuilt `path.join(workspaceRoot, "target", "debug", process.platform === "win32" ? "krui.exe" : "krui")` when present, then `cargo run -q -p krui-cli`.
   - Send `OP.SET_COLOR_SCHEME` when `defaultColorScheme !== "system"`.
5. `media/webview.js`:
   - Wire `#theme-select` (`setColorScheme`) and keep `system` vs `light`/`dark` in sync cleanly.
   - Attach `paste` listener to both `imeBridge` and `stage` so password inputs accept `Ctrl+V` / `Cmd+V`.
6. `editors/vscode-krui/README.md`: update feature description from legacy presets to pure CSS `Light / Dark / System` color scheme switching.

- [ ] **Step 4: Run syntax check and protocol parity test**

Run: `node --check editors/vscode-krui/src/extension.js && node --check editors/vscode-krui/src/panel_protocol.js && node --check editors/vscode-krui/media/webview.js && node --test editors/vscode-krui/test/protocol_parity.test.js`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add editors/vscode-krui
git commit -m "fix(vscode-krui): align color-scheme protocol, password paste, prebuilt CLI lookup, and parity tests"
```

---

### Task 4: Align Repository Documentation & Add `docs/BUILD_OPTIONS.md`

**Files:**
- Create: `docs/BUILD_OPTIONS.md`
- Modify: `README.md:85-100` (update crate count 16 -> 19, list `krui-kms`, `krui-engine-host`, `krui-web`, link `docs/BUILD_OPTIONS.md`)
- Modify: `ARCHITECTURE.md:1-37` (update 16 -> 19 crates table including `krui-kms`, `krui-engine-host`, `krui-web`, and `static_ir` pipeline)
- Modify: `CHANGELOG.md` (add `2026-11-25` section covering Stylo 0.22 + Taffy 0.15.1, 229/229 Tailwind v4 100% conformance, M3 47 built-ins + Component Gallery pure-CSS motion, v3.1 platform crates)
- Modify: `docs/TODO.md` (replace outdated 2026-10-26 `f9bf919` checklist with current active roadmap & backlog including Fabric.js-class 2D `<canvas>` extensions, pointer/wheel/contextmenu/pointer-lock payloads, 3D editor UI widgets, native raw `wgpu` bidirectional integration mode, and opt-in `krui-bevy` Bevy engine integration adapter)
- Modify: `docs/HANDOFF.md` (update date, 19-crate summary, remove deleted `02_mvvm_pattern`/`set_app_tokens`/`TokenId` references)
- Modify: `docs/HISTORY.md` & `docs/css-engine-and-m3-gallery-oneshot.md` (add clear archive/snapshot banners pointing to `AGENTS.md` and `docs/TODO.md`)
- Modify: `.agents/skills/krui-harness2-loop/SKILL.md` (add `stylo-bridge`, `kms`, `engine-host`, `web-dom`, remove outdated "토큰 테이블로만 테마 전환" wording)

- [ ] **Step 1: Author `docs/BUILD_OPTIONS.md`**

Document exact verified Cargo profiles (`dev`, `release`, `kiosk`), crate features (`desktop`, `kiosk`, `preview`, `gpu`), binaries/examples (`krui-showcase`, `krui-bench`, `krui`, `component_gallery`), `krui serve` CLI flags, and environment variables (`PYTHON3`, `KRUI_SERVE_ADDR`, `KRUI_EMBED_PANEL`, `KRUI_DEVICE_FRAME`, `KRUI_BENCH_*`, `CARGO_TARGET_DIR`).

- [ ] **Step 2: Update `README.md`, `ARCHITECTURE.md`, `CHANGELOG.md`, `docs/TODO.md`, `docs/HANDOFF.md`, `docs/HISTORY.md`, `docs/css-engine-and-m3-gallery-oneshot.md`, and `.agents/skills/krui-harness2-loop/SKILL.md`**

Ensure every crate count (`19` workspace members: `17` `#![forbid(unsafe_code)]` + `2` `#![deny(unsafe_code)]` with `0` `unsafe {}` blocks), example path (`examples/component_gallery`), and CSS conformance figure (`229/229 [100.0%]`) matches live code identically.

Record the approved phased roadmap cleanly in `docs/TODO.md`:
1. **Wave 1 (Immediate Fixes & Hygiene)**: VS Code Live Preview `0x0B` color-scheme protocol fix, `views/icons/*.krui` watch resolution, password webview paste, prebuilt CLI lookup, JS↔Rust opcode parity test, docs alignment, logical git checkpoint commits, and nested `@import` path resolution.
2. **Wave 2 (2D Vector Canvas & Viewport Input — Fabric.js Foundation)**: `CanvasContext2D` transform stack (`save`/`restore`/`translate`/`rotate`/`scale`/`set_transform`/`clip_path`), `set_line_dash`, gradients, `global_alpha`, `draw_text`, `draw_image`; rich `.krui` pointer/wheel/contextmenu/button/modifier payloads, pointer lock / relative delta, and dynamic CSS `cursor`.
3. **Wave 3 (Raw `wgpu` Mode, Bevy Engine Integration & 3D Editor UI)**:
   - **Raw `wgpu` Original Mode**: first-class custom `wgpu::RenderPass` / `wgpu::Device` + `Queue` + `ExternalTextureId` viewport pipeline inside KRui desktop/embedded apps (zero extra engine lock-in).
   - **Bevy Engine Integration (`krui-bevy`, opt-in crate)**: import Bevy as an embedded 3D engine/renderer inside KRui editor apps (sharing `wgpu::Device`/`Queue` + Bevy `RenderTarget` -> KRui `ExternalTextureId`) and/or mount KRui UI overlay inside a Bevy app without polluting `krui-core`.
   - **3D Engine Editor Widgets**: `SplitPane` / dockable panels, virtualized `TreeView` with drag-and-drop reparenting, `DragFloat` / `Vec3` / `ColorPicker`, `ContextMenu`, shared `Undo/Redo` stack, and transparent `<canvas>` 2D gizmo overlay over `ExternalTexture`.
4. **Wave 4 (Engine Limits & CSS Backlog)**: `conic-gradient()`, multi-stop gradients (>3 linear / >2 radial), perspective 3D non-affine projection, arbitrary `clip-path: polygon()/path()` stencil pass, M3 gallery `/specs` & `/styles` sub-pages.
5. **Wave 5 (Real-Hardware Platforms & Packaging)**: `krui-kms` real Linux `/dev/dri/card*` DRM + `evdev` on Raspberry Pi, `krui-web` `wasm32` `web-sys` adapter + `StaticDocumentDesc` mounter, `krui-ios-ffi` Swift `UITextInput` + `CAMetalLayer`, Android/Wear OS real-device verification, multi-window DnD / native file dialog / remote `AssetLoader` example, and `vscode-krui` `.vsix` packaging.

- [ ] **Step 3: Run full Harness2 verification gate**

Run: `$env:PYTHON3 = "C:\Users\Kim Bohyun\AppData\Local\Python\bin\python3.exe"; powershell -ExecutionPolicy Bypass -File .agents/skills/krui-harness2-loop/scripts/probe_krui.ps1`
Expected: `[KRui Harness2 Probe] ALL GATES PASSED SUCCESSFULLY!` (exit `0`).

- [ ] **Step 4: Commit**

```bash
git add README.md ARCHITECTURE.md CHANGELOG.md docs/ .agents/skills/krui-harness2-loop/SKILL.md
git commit -m "docs: align 19-crate architecture, roadmap, handoff, and add build options guide"
```
