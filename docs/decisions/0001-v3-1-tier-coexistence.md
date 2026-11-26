# ADR-0001: KRui v3.1 Tier Coexistence Architecture (SPI Kiosk ~ 120FPS 3D Engine ~ Wear OS ~ Web DOM)

- **Date**: 2026-11-25
- **Status**: Accepted (Approved by user)

## Context

KRui must support:
1. High-end desktop, mobile, and a custom `wgpu` 3D engine targeting 120 FPS (60 FPS floor).
2. Raspberry Pi (`aarch64`) + small SPI TFT kiosk panels (e.g., PiTFT 3.5" 480x320) with tight boot (`<= 200ms` warm / `<= 350ms` cold total including SPI wire transfer; KRui SW init+first render `<= 120ms` warm / `<= 270ms` cold) and input (`<= 16ms`) budgets.
3. Wear OS (`watch-wear`, Vulkan-first `wgpu` or `ANativeWindow` partial CPU rendering) and Browser Web (`web` DOM backend + local `<canvas>`).
4. Pure CSS portability (100% Tailwind v4 conformance) without regressing into past hand-rolled computed-value/cascade failures.

## Decisions

1. **Keep Stylo 0.22 as sole CSS cascade/computed-value authority** in native builds, isolated behind `krui_core::view_tree::style_bridge::resolver::StyleResolver`. Plan B (build-time Stylo state-variant precomputation) is deferred conditionally until P0/intermediate Raspberry Pi real-hardware measurements fail SW boot targets.
2. **D1 Static UI Descriptors (`krui-core::static_ir`)**: Lower static element trees into `.rodata` slices (`StaticDocumentDesc` / `StaticNodeDesc`) + a single flat per-component binding dispatcher, eliminating per-node closures and shrinking `.text`/`.pdata`/`.xdata` without introducing a markup parser into release builds.
3. **Zero-Cost Platform Separation**:
   - `krui-renderer` gates `wgpu` + `pollster` behind `default = ["gpu"]`.
   - `krui` root crate gates desktop host deps behind `default = ["desktop"]`.
   - `krui-kms` uses `#![forbid(unsafe_code)]`, DRM DumbBuffer ping-pong + `DIRTYFB` split commits (`hx8357d` union-bbox mitigation) + dedicated flush thread + `evdev`.
   - `krui-engine-host` shares external `wgpu::Device`/`Queue` + `tokio::runtime::Handle` + `DirtyFlagOnly` ping-pong `PaintScenePool` + `DynamicSlotBuffer`.
   - `krui-web` renders `StaticDocumentDesc` directly to browser DOM + CSS + local `<canvas>`.
4. **Compiler-Rewritten Media & Root Profile Attributes**: Rewrite `@media (prefers-reduced-motion)` into `:root:where([data-krui-motion="reduce"])` in `krui-compiler` and inject typed `RootProfileAttrs` onto synthetic `<html>`.

## Modified/Added Crates
- `krui-style`, `krui-signals`, `krui-stylo-bridge`, `krui-compiler`, `krui-macros`, `krui-core`, `krui-renderer`, `krui-android-ffi`, `krui`
- New crates: `krui-kms`, `krui-engine-host`, `krui-web`
