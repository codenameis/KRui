# KRui Architecture & Design Decisions

This document records the architecture of **KRui** across all 19 workspace crates and its compile-time HTML/CSS (`krui-compiler` -> slim DOM IR / `static_ir` -> `krui-stylo-bridge` + `taffy 0.15`) + live TCP hot-reload pipeline.

---

## 1. Workspace Layout (Domain-Oriented Crates)

Crates are organized strictly by domain capability (`DSL -> Slim IR / static_ir -> codegen/interpreter -> ViewTree -> Stylo Bridge -> Taffy 0.15 Layout -> PaintScene -> Renderer`), with a **single CSS owner** (`krui-stylo-bridge`) to prevent shotgun surgery:

| Crate | Domain Capability | `unsafe` Policy |
|---|---|---|
| [`signals`](crates/signals/src/lib.rs) | Session-isolated `Signal<T>`, `Computed<T>`, `Effect`, `Scope`, glitch-free `batch()`, `FrameClock`, `tokio` async bridge, and `UiWaker` | `#![forbid(unsafe_code)]` |
| [`style`](crates/style/src/lib.rs) | Pure CSS data model (`LayoutStyle`, `PaintStyle`, `TextStyle`, `ColorScheme`, `Affine2`, SVG types, text sanitization — zero `TokenId`/`ThemePreset` tables) | `#![forbid(unsafe_code)]` |
| [`stylo-bridge`](crates/stylo-bridge/src/lib.rs) | **Sole CSS owner crate**: wraps `stylo` (`0.22.0`) + `stylo_taffy` (`0.3.0-beta.4`) for W3C cascade, `@media`, `@layer`, `@supports`, `@property`, `@keyframes`, transitions, custom properties, and `TaffyStyloStyle` | `#![deny(unsafe_code)]` (`0` `unsafe {}` blocks; `5` `TElement` trait `unsafe fn`s) |
| [`compiler`](crates/compiler/src/lib.rs) | Build-time HTML parser, component expander (`<slot>` + lexical `:where([data-krui-scope~="..."])` `@layer component` scoping), `<svg>` compiler, and slim DOM-only `CompiledDocument` IR (no `lightningcss`) | `#![forbid(unsafe_code)]` |
| [`macros`](crates/macros/src/lib.rs) | Compile-time procedural macros `krui!(...)`, `include_krui!(...)`, `static_codegen` (`StaticDocumentDesc`), `#[view_model]`, `#[controller]`, and `#[store_msg]` | `#![forbid(unsafe_code)]` |
| [`layout`](crates/layout/src/lib.rs) | Taffy (`0.15.1`) Flexbox & CSS Grid layout solver consuming `krui-stylo-bridge` zero-copy `TaffyStyloStyle` and `LayoutStyle` overrides | `#![forbid(unsafe_code)]` |
| [`text`](crates/text/src/lib.rs) | `cosmic-text` (`0.12`) shaping, Unicode BiDi, CJK/Korean fallback, glyph rasterization, and `TextEditBuffer` | `#![forbid(unsafe_code)]` |
| [`a11y`](crates/a11y/src/lib.rs) | `accesskit` (`0.17`) semantic tree builder (`Role`, `Action`, `url`, focus order, password masking) | `#![forbid(unsafe_code)]` |
| [`core`](crates/core/src/lib.rs) | Platform-neutral `ViewTree` split into cohesive submodules (`view_tree/{dom,style_bridge,input,ime,controls,paint_scene}`), `static_ir`, `EditingState` + `TextInputHost`, `Hangul2BulsikAutomaton`, and `PaintScene` | `#![forbid(unsafe_code)]` |
| [`renderer`](crates/renderer/src/lib.rs) | `WgpuRenderer` (instanced SDF + Kawase dual-filter blur + premultiplied LRU texture cache + `<canvas>`) and `TinySkiaRenderer` CPU fallback | `#![forbid(unsafe_code)]` |
| [`winit`](crates/winit/src/lib.rs) | **Primary Production Host**: Desktop single/multi-window `winit` event loop, IME adapter, AccessKit bridge, and `KRUI_EMBED_PANEL` headless driver | `#![forbid(unsafe_code)]` |
| [`kms`](crates/kms/src/lib.rs) | **Experimental / Adapter Preview**: Trait-backed Linux DRM DumbBuffer (`RGB565`/`XRGB8888`) + `DIRTYFB` union-bbox commit planner, flush worker, and `evdev` kiosk router | `#![forbid(unsafe_code)]` |
| [`engine-host`](crates/engine-host/src/lib.rs) | **Primary Production Host**: In-pass 3D engine UI host (`EngineUiHost`) sharing external `wgpu::Device`/`Queue`, `tokio::runtime::Handle`, and ping-pong `PaintScenePool` | `#![forbid(unsafe_code)]` |
| [`web`](crates/web-dom/src/lib.rs) | **Experimental Prototype**: Standalone browser DOM + native CSS + `<canvas>` slot backend (`krui-web`, independent of `ViewTree`/`stylo`/`taffy`) | `#![forbid(unsafe_code)]` |
| [`patterns`](crates/patterns/src/lib.rs) | Opt-in MVVM (`Command`, `InputCommand`), MVC (`#[controller]`), and Flux/Elm/MVI (`Store<State, Msg>`, `Effects<Msg>`) | `#![forbid(unsafe_code)]` |
| [`preview`](crates/preview/src/lib.rs) | Dev-only (`preview` feature) TCP framing, event-driven `PreviewTcpClient`, `DevFileWatcher`, `"KRFR"` RLE codec, and device bezels sharing `CompiledDocument` IR | `#![forbid(unsafe_code)]` |
| [`cli`](crates/cli/src/lib.rs) | `krui serve` live preview TCP server, PID-scoped Windows lock-free staged binary rebuilds, ADB port reverse, and `--panel` IDE bridge | `#![forbid(unsafe_code)]` |
| [`android-ffi`](crates/android-ffi/src/lib.rs) | **Experimental / Adapter Preview**: Safe Android JNI + Kotlin CPU `SurfaceView` / `Choreographer` / `InputConnection` bridge (`0` `unsafe {}` blocks, `5` `#[no_mangle]` exports) | `#![deny(unsafe_code)]` |
| [`krui` (root)](src/lib.rs) | Public facade, showcase application (`krui-showcase`), starter component gallery example (`component_gallery`), and controlled 120Hz release benchmark (`krui-bench`) | `#![forbid(unsafe_code)]` |

---

## 2. Key Architectural Invariants

1. **Zero Build-Time Compiler or Preview Parser in Release**: `krui-compiler`, `krui-preview`, and `lightningcss` are excluded from default/release builds (`cargo tree -p krui -e normal,no-proc-macro -i krui-compiler` returns empty). Both proc-macro codegen and `--features preview` hot-reload consume the identical slim `CompiledDocument` IR.
2. **Single CSS Owner & Two-Track Damage Separation**: All CSS parsing, selector matching, custom-property inheritance, and transition/keyframe cascade live exclusively inside `krui-stylo-bridge`. `StyloDamage` routes paint-only changes directly to GPU/CPU fast-path compositing (`0` relayout passes over 120+ animation frames), while Tier-2 layout property transitions dirty only the affected node. Layout and hit-testing share the exact parent-to-child center-origin `Affine2` / 4x4 matrix.
3. **OS-Owned IME Pinning**: Active preedit composition pins `ime_target` until `FinishComposing` or `Commit`, preserving composed Korean/Japanese text even across mid-composition focus transitions.
4. **Non-Blocking UI Thread**: Clipboard uses native `arboard`, dynamic `<img src="{...}">` file reads/decodes run on `tokio::task::spawn_blocking` guarded by owner `Scope` disposal, and live TCP preview clients wake the event loop via blocking background listener threads (`0%` idle CPU).
