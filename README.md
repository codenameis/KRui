# KRui

**A free, performance-first, cross-platform Rust GUI framework.**  
*Real HTML + CSS authoring syntax compiled at build time into native Rust DOM + Stylo (`0.22.0`) + Taffy (`0.15.1`) — zero webview, zero runtime HTML parser, dual-licensed MIT / Apache-2.0.*

---

## Core Principles

1. **Native-grade CJK / Korean IME first**: OS-owned preedit composition, candidate window anchoring, and `ime_target` focus pinning across desktop and Android.
2. **Performance over everything**: 120Hz (`<= 8,333us` p50 & p99) frame budget. `transform` and `opacity` animations bypass Taffy layout completely (`0` relayouts).
3. **Real HTML + CSS syntax (`krui!`, `include_krui!`)**: Familiar authoring syntax compiled into a slim DOM IR, styled by a single CSS owner crate (`krui-stylo-bridge`) via Stylo (`0.22.0`) + `stylo_taffy` + Taffy (`0.15.1`), with state-preserving TCP hot-reload (`krui serve` / `krui serve --panel`).
4. **Zero runtime markup/preview parser in release**: No HTML parser, no preview interpreter, and no `lightningcss` dependency ship in release binaries.
5. **No webview in app runtime**: Every pixel is drawn by KRui's own `wgpu` (GPU) or `tiny-skia` (CPU fallback) renderer using pure CSS `[data-theme]` / `@media (prefers-color-scheme)` / `light-dark()` stylesheets.
6. **Integrated reactive + pattern library**: Use `Signal<T>` / `Computed<T>` / `Effect` + `tokio` async tasks directly, or opt-in `krui-patterns` (`#[view_model]`, `Command`, `#[controller]`, `Store<State, Msg>`).
7. **Measured pure-CSS conformance**: Target 100% unmodified external CSS compatibility. Current measured conformance on the real `@tailwindcss/cli@4.3.5` fixture suite (`tests/fixtures/real_css/tailwind_v4/app.krui` — 12 app nodes + 9 edge-case probes vs headless Chromium oracle, `tests/real_css_conformance.rs`): **229/229 (100.0%, measured 2026-11-25)** with zero remaining known gaps (`tests/fixtures/real_css/KNOWN_GAPS.toml`); complex `clip-path`, `conic-gradient()`, and 3D perspective remain tracked in `docs/ENGINE_LIMITS.toml`.

---

## Platform Support Tiers

| Tier | Crates / Targets | Maturity & Current Scope |
|---|---|---|
| **Primary Production Tier** | `krui-winit` (`Win / macOS / Linux`), `krui-renderer` (`wgpu` + `TinySkia` CPU fallback), `krui-engine-host` | Full desktop windowing, OS-owned CJK/Korean IME, AccessKit a11y, multi-window, 120Hz fast-path compositing, and `krui serve` TCP hot-reload. |
| **Experimental / Adapter Preview** | `krui-kms`, `krui-android-ffi`, `krui-web` | `krui-kms`: trait-backed DRM `DIRTYFB` planner, flush worker, and `evdev`/Hangul router (host app supplies concrete OS device descriptors). `krui-android-ffi`: safe JNI + CPU `SurfaceView` framebuffer bridge. `krui-web`: standalone browser DOM prototype outside the `ViewTree`/`Stylo` pipeline. |

---

## Quick Start

### 1. Run the Interactive Desktop Showcase (or Headless Verification Suite)

```bash
# Launch the interactive desktop window (winit + wgpu / tiny-skia):
cargo run -p krui --bin krui-showcase

# Run the automated 120Hz frame-budget & multi-theme verification suite:
cargo run -p krui --bin krui-showcase -- --headless

# Run the Material 3 / Tailwind pure-CSS Component Gallery headless gate:
cargo run --example component_gallery -- --headless

# Run the full build & feature options reference: docs/BUILD_OPTIONS.md
# Launch live TCP hot-reload preview:
cargo run -p krui-cli -- serve --example component_gallery
```

See [Build Options & Flags](docs/BUILD_OPTIONS.md) and [Roadmap](docs/TODO.md) for profiles (`dev`, `release`, `kiosk`), features, and multi-platform targets.

### 2. Run the Full Test & Harness2 Verification Suite

```bash
cargo test --workspace
pwsh -NoProfile -ExecutionPolicy Bypass -File .agents/skills/krui-harness2-loop/scripts/probe_krui.ps1
```

---

## Example Usage

```rust
use krui::*;

let mut session = EngineSession::new(960, 640, RenderBackendPreference::Auto);
// 반응형 상태 및 Fast-Path 흔들림(shake) 애니메이션 생성
let count = Signal::new(0);
let (shake_tx, trigger_shake) = session.clock.shake(10.0, 22.0, 0.35);

// 구체 타입 Command로 이벤트 연결 (트레이트 객체 노출 없음)
let on_press = Command::new(move || {
    count.update(|n| *n += 1);
    trigger_shake.set(true);
});

session.tree.mount_with(|| {
    krui!(r#"
        <style>
            .card {
                display: flex;
                flex-direction: column;
                padding: 24px;
                gap: 12px;
                background-color: #1e293b;
                border-radius: 12px;
            }
        </style>
        <div class="card" style="transform: {shake_tx}">
            <h1>안녕하세요 KRui!</h1>
            <button onclick="{on_press}">클릭 횟수: {count}</button>
        </div>
    "#)
});
```

---

## unsafe 목록

19개 워크스페이스 크레이트 중 17개 크레이트(`krui`, `krui-core`, `krui-signals`, `krui-style`, `krui-compiler`, `krui-macros`, `krui-layout`, `krui-text`, `krui-a11y`, `krui-renderer`, `krui-winit`, `krui-kms`, `krui-engine-host`, `krui-web`, `krui-patterns`, `krui-preview`, `krui-cli`)는 모두 `#![forbid(unsafe_code)]`를 강제합니다. 예외 2개 크레이트인 [`crates/android-ffi`](crates/android-ffi/README.md)와 [`crates/stylo-bridge`](crates/stylo-bridge/src/lib.rs) 역시 `#![deny(unsafe_code)]` 아래 **직접 작성한 `unsafe {}` 블록과 raw pointer가 0줄**이며, 각각 JNI 심볼 바인딩용 `#[no_mangle]` 7개 및 `style::dom::TElement` 트레이트 계약상 필수인 `unsafe fn` 시그니처 5개(크레이트당 한도 10개 이내)만 사용합니다.

| 크레이트 | 파일:라인 | 구분 | 줄 수 | 안전성 근거 (`// SAFETY:`) |
|---|---|---|---|---|
| `krui-android-ffi` | `crates/android-ffi/src/jni_entrypoints.rs:67` | `#[no_mangle]` (`nativeSessionOp`) | 1 | `catch_unwind` + 정수 세션 핸들 테이블만 사용 (`unsafe` 블록 0줄). |
| `krui-android-ffi` | `crates/android-ffi/src/jni_entrypoints.rs:92` | `#[no_mangle]` (`nativeMotionOrInsets`) | 1 | `catch_unwind` + 원시 수치 인자만 사용 (`unsafe` 블록 0줄). |
| `krui-android-ffi` | `crates/android-ffi/src/jni_entrypoints.rs:123` | `#[no_mangle]` (`nativeTextOp`) | 1 | `is_null()` 검사 + safe `JNIEnv::get_string` + `catch_unwind`. |
| `krui-android-ffi` | `crates/android-ffi/src/jni_entrypoints.rs:151` | `#[no_mangle]` (`nativeImeRangeOp`) | 1 | `catch_unwind` + UTF-16 정수 범위 인자만 사용 (`unsafe` 블록 0줄). |
| `krui-android-ffi` | `crates/android-ffi/src/jni_entrypoints.rs:177` | `#[no_mangle]` (`nativeCopyFramebuffer`) | 1 | `is_null()`·길이 검사 + safe `JNIEnv::set_int_array_region` + `catch_unwind`. |
| `krui-android-ffi` | `crates/android-ffi/src/jni_entrypoints.rs:247` | `#[no_mangle]` (`nativeWidgetOp`) | 1 | `catch_unwind` + 정수 위젯 핸들 테이블만 사용 (`unsafe` 블록 0줄). |
| `krui-android-ffi` | `crates/android-ffi/src/jni_entrypoints.rs:266` | `#[no_mangle]` (`nativeCopyWidgetData`) | 1 | `is_null()`·길이 검사 + safe `JNIEnv::set_int_array_region` + `catch_unwind`. |
| `krui-stylo-bridge` | `crates/stylo-bridge/src/dom_element.rs:294` | `unsafe fn set_handled_snapshot` (`TElement`) | 1 | 단일 스레드 Stylo 순회(`pool: None`) 중 atomic store만 수행 (`unsafe {}` 0줄). |
| `krui-stylo-bridge` | `crates/stylo-bridge/src/dom_element.rs:301` | `unsafe fn set_dirty_descendants` (`TElement`) | 1 | 단일 스레드 Stylo 순회(`pool: None`) 중 atomic store만 수행 (`unsafe {}` 0줄). |
| `krui-stylo-bridge` | `crates/stylo-bridge/src/dom_element.rs:308` | `unsafe fn unset_dirty_descendants` (`TElement`) | 1 | 단일 스레드 Stylo 순회(`pool: None`) 중 atomic store만 수행 (`unsafe {}` 0줄). |
| `krui-stylo-bridge` | `crates/stylo-bridge/src/dom_element.rs:326` | `unsafe fn ensure_data` (`TElement`) | 1 | 활성 순회 노드에 한해 `ElementDataWrapper` atomic-flag + `RefCell` 가드 위임. |
| `krui-stylo-bridge` | `crates/stylo-bridge/src/dom_element.rs:333` | `unsafe fn clear_data` (`TElement`) | 1 | 단일 스레드 서브트리 해제(`display: none`) 시 pending hide 기록 후 안전 초기화. |
