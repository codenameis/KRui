# Changelog

All notable changes and milestone checkpoints for **KRui** are documented in this file.

## [0.3.0-dev] — 2026-11-25 (Stylo 0.22 Pure CSS Parity, M3 Gallery Motion & v3.1 Tier Coexistence)

### Added & Changed
- **Single CSS Owner (`krui-stylo-bridge`, Stylo `0.22.0` + `stylo_taffy` + Taffy `0.15.1`)**: Replaced legacy `ThemePreset`/`TokenId`/`TokenTable`/`lightningcss` with full W3C cascade (`@layer`, `@property`, `@media`, `@supports`, `@container`, `light-dark()`, CSS `filter`, `:has()`). Measured `@tailwindcss/cli@4.3.5` conformance: **229 / 229 (100.0%)** in `tests/real_css_conformance.rs`.
- **Static Descriptor IR (`krui-core::static_ir`, `static_codegen`)**: Lowered compiled templates into `.rodata` slices (`StaticDocumentDesc` / `StaticNodeDesc`) with zero markup parser in release binaries.
- **v3.1 Multi-Tier Platform Crates ([`ADR-0001`](docs/decisions/0001-v3-1-tier-coexistence.md))**: Added `krui-kms` (Linux DRM DumbBuffer + `DIRTYFB` + `evdev`), `krui-engine-host` (shared `wgpu::Device` / `ExternalHandle` tokio overlay host), `krui-web` (`krui-web` DOM + CSS + `<canvas>` backend), `wear_host` in `krui-android-ffi`, and `Hangul2BulsikAutomaton` for bare kiosks.
- **Material Design 3 Component Gallery & Built-in Library (`examples/component_gallery`)**: Unified 47 built-in M3 components, 41 documented pages, 77 Material Symbols SVG components, real OS clipboard JSON export via `arboard`, `<progress>` native indicator rendering, pure-CSS 0-relayout overlay motion (`[data-open]` / `krui-ui-open-*`), and Windows release GUI subsystem (`windows_subsystem = "windows"`).
- **VS Code Live Preview (`editors/vscode-krui`)**: Replaced removed `0x0B` theme-preset command with pure-CSS `OP_CMD_SET_COLOR_SCHEME` (`system` / `light` / `dark`), `views/icons/*.krui` watch resolution, password paste handling, prebuilt `krui` lookup, and JS↔Rust opcode parity test.
- **Build Documentation**: Added [`docs/BUILD_OPTIONS.md`](docs/BUILD_OPTIONS.md) covering Cargo profiles (`dev`, `release`, `kiosk`), crate features, CLI flags, and environment variables.

## [0.2.0-m1] — 2026-10-24 (Milestone M1 Checkpoint: Desktop Shell, Interactive Widgets, Korean IME & Live Hot-Reload)

### Added
- **Interactive `<input>` (`TextInput` & `PasswordInput`) with Korean IME Support**:
  - Full `winit::event::Ime` (`Ime::Preedit` syllable composition and `Ime::Commit`) + keyboard character entry and `Backspace` editing.
  - Visual caret (cursor) rendering inside the focused `<input>` and `oninput="{handler}"` reactive callback support in `krui!` / `.krui`.
- **Visual Interaction Feedback (Hover, Focus Ring)**:
  - Pointer hover tracking (`dispatch_pointer_move`) with button & input hover highlights.
  - High-contrast focus ring (`2.5px` accent border) rendered around focused widgets during `Tab` / click navigation.
- **Scrollable List & Container (`<ul>`, `<li>`, `overflow: scroll`)**:
  - Mouse wheel scroll handling (`dispatch_scroll`), child viewport clipping, and scrollbar thumb rendering.
- **Live `.krui` Hot-Reload Dev Loop (Section 3.9 & Section 6)**:
  - `dev_hot_reload::DevFileWatcher` (`#[cfg(debug_assertions)]` only; compiled out of `--release` builds) watches local `.krui` files and live-patches styles, layouts, and static text literals in-place without restarting or losing reactive signal state.
- **Official Architecture Pattern Example Apps (Section 3.8)**:
  - Added [`examples/pattern_mvvm.rs`](examples/pattern_mvvm.rs), [`examples/pattern_mvc.rs`](examples/pattern_mvc.rs), and [`examples/pattern_elm_flux.rs`](examples/pattern_elm_flux.rs).
- **GPU Glyph Texture Atlas (`GlyphAtlasPacker`) & Intrinsic Text Sizing**:
  - Upgraded `wgpu` backend to pack `cosmic-text` / `swash` rasterized glyphs into a `1024x1024` RGBA8 texture atlas and feed intrinsic text dimensions into Taffy before layout.

## [0.1.0-m0] — 2026-10-24 (Milestone M0 Checkpoint: Core Engine Skeleton & Desktop Showcase)

### Added
- **`krui-signals`**:
  - Contiguous arena-allocated `Signal<T>`, `Computed<T>`, and `Effect` primitives (`#![forbid(unsafe_code)]`).
  - Glitch-free batched propagation (`batch(|| ...)`) ordered by topological depth so diamond dependencies never observe half-updated state.
  - First-class `FrameClock` (`on_frame`) and compositor-guaranteed animation helpers (`shake()`, `nudge()`, `spring_to()`).
- **`krui-style`**:
  - Strict structural split between `LayoutStyle` (Taffy-bound) and `PaintStyle` (compositor-bound).
  - Four compile-time design system presets (`ThemePreset::Fluent`, `Material`, `LiquidGlass`, and `Cosmic` flat zero-overdraw theme).
  - Defensive text sanitization (`sanitize_untrusted_text`, `is_bidi_control_char`) neutralizing Unicode BiDi override spoofing (`U+202A..=U+202E`, `U+2066..=U+2069`).
- **`krui-compiler` & `krui-macros`**:
  - Build-time HTML + `<style>` CSS parser (`krui!` and `include_krui!`).
  - Static specificity cascade resolution (`tag` < `.class` < `#id` < inline `style`) with zero runtime CSS parser or selector matching.
  - Partial `{signal}` interpolation in `transform`, `opacity`, layout dimensions, and text nodes compiled into direct slot bindings without runtime string re-parsing.
  - Compile-time Performance Cliff Detector warning when frame/animation signals feed layout-triggering CSS properties.
- **`krui-layout`**:
  - `taffy` (`0.7`) Flexbox and CSS Grid integration with layout-dirty tracking isolated from paint/transform updates.
- **`krui-text`**:
  - `cosmic-text` (`0.12`) advanced shaping, BiDi analysis, Korean (`한글`) / CJK / Arabic RTL / Emoji grapheme clustering, and swash glyph rasterization.
- **`krui-a11y`**:
  - `accesskit` (`0.17`) semantic tree builder, keyboard `Tab` / `Shift+Tab` focus ring navigation, and automatic masking of password/secret inputs (`••••••••`).
- **`krui-renderer`**:
  - `RendererBackend` trait boundary with `WgpuRenderer` (`wgpu` + `lyon_tessellation`) and `TinySkiaRenderer` (`tiny-skia` CPU fallback).
  - Auto-detection at startup with automatic `Cosmic` theme activation on `tiny-skia` fallback.
  - First-class `<canvas>` 2D context (`CanvasContext2D`) supporting both `Reactive` and `PerFrame` redraw modes across both backends.
  - Bounded GPU resource limits (`GpuResourceLimits`) and 3D / VR-AR extensibility trait hooks (`CustomRenderPass`, `Embedded3DViewport`, `ExtensibleEvent`).
- **`krui-mvvm` & Pattern Examples**:
  - Optional `#[view_model]` proc-macro and `Command` helper alongside integration tests demonstrating MVVM, MVC, and Elm/Flux single-state-signal patterns on the same core.
