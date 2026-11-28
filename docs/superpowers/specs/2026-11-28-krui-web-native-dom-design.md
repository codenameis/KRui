# KRui Web Native DOM (`krui-web` + 로컬 `<canvas>` 2D/3D WebGPU) 설계 명세

- **작성일**: 2026-11-28
- **대상 버전 / 상태**: 설계 초안 (구현 전 승인 대기)
- **관련 ADR**: [ADR-0001: KRui v3.1 Tier Coexistence Architecture](../../decisions/0001-v3-1-tier-coexistence.md)

---

## 1. 목적과 배경 (Intent & Why)

### 현재 문제
현재 `krui-showcase-wasm` (`src/bin/krui-showcase-wasm.rs`)은 웹 브라우저 안의 `<canvas id="krui-canvas">` 하나 위에 데스크탑용 엔진 전체(`krui-core` + `krui-stylo-bridge`/`stylo` + `taffy` + `krui-text`/`cosmic-text` + `krui-renderer`/`TinySkiaRenderer`)를 올려 실행한다. 이로 인해 두 가지 치명적 문제가 발생했다.

1. **전 세계 다국어 글자 깨짐 (`.notdef` 두부 박스 `□`)**:
   - WASM 샌드박스는 OS 시스템 폰트에 접근할 수 없어 `FontSystem::new()`가 시스템 폰트 0개로 초기화된다.
   - `examples/component_gallery/views/themes/material.css`가 임베드하는 유일한 폰트(`Roboto-Variable.ttf`)에는 라틴/그리스/키릴 글리프만 있고 한글·CJK·아랍어·이모지가 없어, 모든 한글 텍스트가 `□`로 출력된다.
2. **심각한 애니메이션 버벅거림 & 과도한 번들 크기**:
   - `web/pkg/krui_showcase_wasm_bg.wasm` 크기가 **10.71 MB**에 달한다.
   - 매 애니메이션 프레임마다 단일 스레드 CPU(`TinySkiaRenderer::render_scene`)로 전체 프레임버퍼(예: `1716×1911` @ DPR 1.5, ~13 MB)를 처음부터 다시 래스터라이즈한 뒤 전체 픽셀 `unpremultiply_rgba` + `ImageData` 동기 `put_image_data` 업로드를 반복한다.

### 왜 이 설계를 선택하는가
KRui의 제품 원칙은 **"각 기기 티어에서 호스트 플랫폼이 낼 수 있는 최대 성능을 최소 부담으로 뽑아내는 것"**이다([ADR-0001](../../decisions/0001-v3-1-tier-coexistence.md)).
웹 브라우저는 이미 C++로 컴파일된 **네이티브 레이아웃·CSS 캐스케이드·OS 폰트·OS IME·스크린리더·GPU 컴포지터**를 내장하고 있다. 브라우저 위에서 또 하나의 미니 브라우저 엔진(`Stylo + Taffy + cosmic-text + TinySkia`)을 WASM으로 돌리는 대신, **일반 UI/문서/입력/레이아웃은 브라우저 네이티브 DOM + CSS에 위임하고, 진짜 그래픽 렌더링(2D 벡터 `<canvas>` 및 3D WebGPU/WebGL2)만 로컬 `<canvas>` 슬롯에서 직접 처리**한다.

---

## 2. 목표, 비목표, 금지 사항 (Goals, Non-Goals, Anti-Goals)

### Goals (목표)
1. **동일 소스 단일 빌드 모델**: `examples/component_gallery`의 `.krui`/`.css` 파일과 Rust 컨트롤러(`GalleryController` 등)를 웹 전용으로 포크하지 않고, 동일한 `include_krui!` / `krui!` 매크로 출력으로 `native-engine`과 `web-dom` 양쪽 모두에서 컴파일·실행한다.
2. **무거운 네이티브 엔진 의존성 0개**: `--no-default-features --features web-dom` 빌드 의존성 트리에서 `krui-core`, `krui-stylo-bridge`, `stylo`, `taffy`, `krui-text`, `cosmic-text`, `krui-layout`, `krui-a11y`, `accesskit`, `krui-renderer`, `tiny-skia`를 완전히 제거한다(`cargo tree` 게이트 검증).
3. **웹 네이티브 최대 성능 & 글로벌 폰트/IME**:
   - CSS `transform`/`opacity`/트랜지션/`@keyframes`/스크롤을 브라우저 GPU 컴포지터가 60~120fps로 직접 처리한다.
   - 추가 폰트 번들 0 KB로 브라우저/OS에 설치된 전 세계 모든 언어(한글, 일본어, 중국어, 아랍어, 이모지 등)를 네이티브 셰이핑으로 선명하게 렌더링한다.
   - 한국어/일본어/모바일 소프트 키보드 등 OS 네이티브 IME 조합·후보창을 100% 그대로 사용한다.
4. **로컬 `<canvas>` 2D + 3D WebGPU/WebGL2 통합**:
   - `<canvas draw="{...}">` (예: `GalleryCanvasStudio.krui`의 `#studio-2d-canvas`)는 `ResizeObserver` + DPR 스케일링 + 포인터 캡처 + `CanvasRenderingContext2d` 하드웨어 가속 경로로 `CanvasCommand`를 직접 그린다.
   - `#dev-3d-viewport-slot` (`GalleryViewport3d.krui`)은 3D 탭이 열릴 때만 지연(lazy) 초기화되는 로컬 `<canvas>`를 마운트해 `wgpu` (**WebGPU → WebGL2 자동 폴백**)로 `open_teapot.obj`를 렌더링하고, 탭을 닫으면 GPU/rAF 자원을 즉시 해제한다.
5. **완전한 메모리/수명 안전성 & 헤드리스 테스트 가능성**:
   - `#![forbid(unsafe_code)]`를 유지한다.
   - 네이티브 테스트 호스트에서도 `MockDomBackend`를 통해 마운트, `<if>`/`<for>` 키 기반 재조정, 이벤트, 2D `<canvas>` 명령 기록, `Scope` 해제 후 리스너/Effect 누수 0건을 검증한다.

### Non-Goals (비목표)
- 기존 데스크탑/키오스크/게임엔진 호스트(`krui-winit`, `krui-kms`, `krui-engine-host`, `krui-android-ffi`)의 동작이나 기본 피처(`default = ["native-engine", "desktop", "tray"]`) 변경.
- 브라우저 간(Chrome/Firefox/Safari) 및 데스크탑 `TinySkia`/`wgpu`와의 서브픽셀·스크롤바 100% 픽셀 동일성(브라우저 네이티브 폰트 메트릭·UA 스크롤바를 의도적으로 사용).
- 초기 `krui-web` 3D 슬롯 내부에서 Bevy 0.15 ECS(`krui-bevy`) 전체를 WASM으로 구동하는 것 (이번 단계에서는 공유 `open_teapot.obj`를 `wgpu` WebGPU/WebGL2 파이프라인으로 직접 렌더링하며, Bevy-on-Web은 후속 선택 과제로 남김).
- `web-dom` 타깃에서의 `preview` 라이브 핫리로드 인터프리터 지원.

### Anti-Goals (절대 하지 않을 것)
- `web-dom` WASM 바이너리에 `stylo`, `taffy`, `cosmic-text`, `tiny-skia`가 잔류하는 상태.
- 다국어 해결을 위해 수십 MB짜리 CJK/글로벌 폰트를 WASM에 정적 임베드하는 것.
- `component_gallery` 뷰(`.krui`)나 컨트롤러를 웹용으로 복제·분기하는 것.
- DOM 이벤트 리스너나 rAF 콜백에 `Closure::forget()`을 남발해 `<if>`/`<for>` 전환 시 메모리와 클로저가 누수되는 구조.

---

## 3. 제약 사항 (`AGENTS.md` Guardrails)

1. **`#![forbid(unsafe_code)]`**: 신규 크레이트 `krui-ui-types`, 기존 `krui-web`, `krui-patterns`, `krui`, `krui-showcase-wasm` 모두 `#![forbid(unsafe_code)]`를 유지한다.
2. **릴리즈 마크업/CSS 파서 금지**: `.krui`는 컴파일 타임에 `StaticDocumentDesc` 정적 `.rodata` 슬라이스로 내리고, CSS는 컴파일러가 수집/rewritten한 정적 문자열을 브라우저 `<style>`로 주입한다.
3. **코드 크기 규율**: 파일 ≤ 500줄(테스트 하드 상한 560줄), 함수 ≤ 60줄(하드 상한 80줄), 모든 공개 API에 `///` 문서 주석, `cargo fmt` 및 `cargo clippy --workspace --all-targets -- -D warnings` 통과.
4. **피처 격리**: `krui` 기본 피처는 `["native-engine", "desktop", "tray"]`로 유지하고, 웹 DOM 빌드는 `--no-default-features --features web-dom` (3D 포함 시 `web-gpu-3d` 추가)로 opt-in한다.

---

## 4. 검토한 대안과 선택된 아키텍처 (Approaches)

| 접근 방식 | 설명 | 트레이드오프 | 결정 |
|---|---|---|---|
| **방안 1: 브라우저 네이티브 DOM(`krui-web`) + 로컬 `<canvas>` 2D/3D WebGPU** | UI 셸·텍스트·CSS·스크롤·IME는 브라우저 DOM/CSS에 위임하고, `<canvas draw>` 2D와 3D WebGPU/WebGL2 뷰포트만 로컬 `<canvas>`에서 실행 | `krui-ui-types` 분리 및 `krui-web` 전체 구현이 필요하지만, WASM 대폭 경량화·0KB 글로벌 폰트·네이티브 IME·브라우저 120fps·로컬 3D WebGPU를 동시에 달성 | **채택 (사용자 선택)** |
| **방안 2: 풀뷰포트 `<canvas>` + WebGPU/WebGL2/SIMD CPU + Canvas2D 폰트 폴백** | 기존 `ViewTree`/`Stylo`/`Taffy`/`wgpu` 전체를 웹 `<canvas>`에 띄우고 누락 글자만 브라우저 `fillText`로 추출 | 10.71+ MB 엔진 유지, 아랍/인도계 셰이핑 한계, `measure`/`rasterize` 불일치 위험, `getImageData` 콜드 프레임 스파이크, OS 폰트 굵기 이질감, 자체 IME 에뮬레이션 필요 | 기각 |
| **방안 3: CPU `TinySkia` DamageRect + `simd128` 최적화만 적용** | 기존 CPU 경로에 부분 렌더링과 SIMD만 추가 | 전체 화면 스크롤·테마 전환 시 CPU 한계 여전, 폰트/IME 문제 미해결 | 기각 |

---

## 5. 상세 아키텍처 설계

### 5.1 크레이트 분리 및 의존성 그래프 (`krui-ui-types` 추출)

현재 `krui-patterns`와 `krui-macros` 코드젠 출력이 `krui-core`에 직접 의존하고 있어, `krui-web`이 `krui` 루트를 통하면 `stylo`/`taffy`/`cosmic-text`가 딸려 들어오는 문제가 있다. 이를 깔끔하게 끊기 위해 **초경량 공유 타입 크레이트 `crates/ui-types` (`krui-ui-types`)** 를 도입한다.

```
krui-signals ─┐
              ├─▶ krui-ui-types (static_ir, events, canvas_2d, svg)
krui-style  ──┘         │
          ┌─────────────┴─────────────┐
          ▼                           ▼
   krui-core (native-engine)     krui-web (web-dom)
   (Stylo/Taffy/cosmic-text)     (DOM + web-sys, zero heavy deps)
          │                           │
          └──────────┬────────────────┘
                     ▼
               krui (root crate)
     [feature: native-engine | web-dom]
```

#### `crates/ui-types` (`krui-ui-types`, `#![forbid(unsafe_code)]`) 이동 대상
- **의존성**: `krui-signals`, `krui-style`만 의존 (무거운 엔진·네이티브 OS 의존성 0개).
- **모듈 구성**:
  1. `static_ir`: `StaticNodeKind`, `StaticButtonKind`, `StaticInputKind`, `StaticAttrDesc`, `StaticSelectOptDesc`, `StaticNodeDesc`, `StaticDocumentDesc`.
     - *(주의: 현재 `crates/core/src/view_tree/static_ir.rs`에 함께 있는 `mount_static_document` / `mount_static_nodes` 함수는 `ViewTree`를 받으므로 `krui-core`에 남기고, `krui-web`은 동일한 `StaticDocumentDesc`/`StaticNodeDesc` 데이터를 소비하는 자체 `mount_static_document`/`mount_static_nodes`를 제공한다.)*
  2. `events`: `Mods`, `PointerButton`, `PointerEvent`, `PointerHandler`, `WheelEvent`, `WheelHandler`, `ClickTarget`, `ClickBinding`, `InputTarget`, `PointerTarget`, `WheelTarget`, `ButtonKind`, `InputKind`, `prevent_default_key`, `prevent_default_wheel`.
  3. `canvas_2d`: `Size`, `PathSegment`, `CanvasLineCap`, `CanvasLineJoin`, `CanvasFillRule`, `CanvasGradient`, `CanvasGradientStop`, `CanvasTextAlign`, `CanvasCommand`, `CanvasContext2D`, `CanvasDraw`, `CanvasRedrawMode` (및 `canvas_2d_extras`의 `CanvasState`).
  4. `svg`: `SvgColor`, `SvgVectorOp`, `SvgVectorTemplate`.
- **호환성 유지**: `krui-core`는 `krui_ui_types::*`를 그대로 `pub use` 재노출하므로 기존 `krui-core` 소비자 코드는 깨지지 않는다.
- **`krui-patterns` 조정**: `crates/patterns/Cargo.toml`에서 기본 의존성을 `krui-core` 대신 `krui-ui-types`로 바꾸고, `krui-core`는 `optional = true`로 두어 `preview` feature에서만 활성화한다(`store_preview.rs` 등).
- **`src/widgets` 조정**: `src/widgets/editor/*`는 `krui_ui_types`를 사용하고, `Hangul2BulsikAutomaton`에 의존하는 `virtual_keyboard`는 `#[cfg(feature = "native-engine")]`로 게이트한다.
- **`krui` 루트 `Cargo.toml` 피처 구성**:
  - `default = ["native-engine", "desktop", "tray"]`
  - `native-engine = ["dep:krui-core", "dep:krui-layout", "dep:krui-text", "dep:krui-a11y", "dep:krui-renderer", "dep:accesskit", "dep:image"]`
  - `desktop = ["native-engine", "krui-renderer/gpu", "dep:krui-winit", "dep:krui-engine-host", "dep:krui-bevy", "dep:pollster", "dep:wgpu", "dep:winit"]`
  - `kiosk = ["native-engine"]`
  - `web-dom = ["dep:krui-web"]`
  - `web-gpu-3d = ["web-dom", "dep:wgpu"]` (3D `<canvas>` 호스트용 — `krui-renderer`나 `stylo`/`taffy` 없이 순수 `wgpu` WebGPU/WebGL2 파이프라인만 선택적으로 포함)
  - **상호 배제 가드**: `#[cfg(all(feature = "native-engine", feature = "web-dom"))]` 일 때 `compile_error!("Features `native-engine` and `web-dom` are mutually exclusive; pass `--no-default-features --features web-dom`")`를 발생시킨다.
  - **`web-dom` 모드 공개 별칭 (`src/lib.rs`)**:
    - `pub type ViewTree = krui_web::WebDomTree;`
    - `pub type NodeId = krui_web::DomNodeId;`
    - `pub type ViewFragment = krui_web::WebViewFragment;`
    - `pub type BranchMountFn = krui_web::WebBranchMountFn;`
    - `krui::__private::core::static_ir` 경로가 `krui_web::static_ir` + `krui_ui_types::static_ir`를 가리키도록 통일한다.
    - *(Architect's call)*: 루트 크레이트의 `#[cfg(feature = "web-dom")]` 모듈에서 `GpuResourceLimits`, `FrameRenderStats`, `ColorSchemePreference`, `ReducedMotionPreference`, `DecodedImage` 등 컨트롤러/앱 코드가 참조할 수 있는 경량 타입을 `krui_style` 또는 `krui_web` 스텁/공통 타입으로 재노출해 기존 `.krui` 및 `GalleryController`가 조건부 컴파일(`#[cfg]`) 없이 컴파일되게 한다.

---

### 5.2 `krui-macros` ↔ `WebDomTree` 호환 계약

`krui-macros`는 컴파일 타임에 어떤 렌더러인지 알 필요 없이 `__tree.<method>(...)`와 `::krui::__private::core::static_ir::*`를 방출한다. 따라서 `WebDomTree`가 `ViewTree`와 **동일한 메서드 시그니처 계약**을 구현하면 `.krui` 템플릿 전체가 수정 없이 동작한다.

`crates/macros/src/` 전수 조사로 도출된 필수 계약 목록:

1. **문서/배치 마운트 및 정적 IR**:
   - `begin_mount_batch(&mut self)`, `end_mount_batch(&mut self)`
   - `register_embedded_font(&mut self, key: &str, bytes: Arc<[u8]>)`
   - `static_ir::mount_static_document(tree, parent, doc, is_top_mount) -> Vec<DomNodeId>`
   - `static_ir::mount_static_nodes(tree, parent, nodes) -> Vec<DomNodeId>`
   - `ViewFragment::new(f)` / `mount_with`
2. **제어 흐름 (`codegen_control_flow.rs`)**:
   - `attach_if_to_anchor(&mut self, parent: Option<DomNodeId>, anchor: DomNodeId, branch_fn: impl Fn() -> Option<usize> + 'static, branches: Vec<WebBranchMountFn>)`
   - `attach_for_to_anchor<I, K, F>(&mut self, parent: Option<DomNodeId>, anchor: DomNodeId, items_fn, key_fn, body_fn, empty_fn)`
   - `mount_if`, `mount_for` (루트 레벨 제어 흐름용)
3. **텍스트·리치 텍스트·클래스·속성·스타일 (`static_codegen.rs`, `codegen_slots.rs`, `codegen_rich_text.rs`)**:
   - `set_static_text`, `bind_dynamic_text`
   - `set_rich_spans`, `bind_dynamic_rich_spans` (HTML 내부 `<span>` 인라인 요소들로 변환, `font-weight`, `font-style`, `font-family`, `color` 반영)
   - `set_element_classes`, `bind_dynamic_class`
   - `set_node_attr`, `bind_dynamic_attr`, `bind_dynamic_bool_attr`
   - `set_inline_style`, `bind_dynamic_inline_style`
   - `set_placeholder`, `bind_dynamic_placeholder`, `set_aria_label`, `bind_dynamic_aria_label`, `set_title`, `bind_dynamic_title`, `set_link_href`, `bind_dynamic_link_href`
4. **폼 컨트롤 & 양방향 바인딩 (`codegen_events.rs`, `select_bindings.rs`)**:
   - `set_disabled`/`bind_disabled`, `set_required`/`bind_required`, `set_checked`/`bind_checked`, `set_on_checked_change`
   - `add_select_option`, `bind_select_options` (`<option value="...">label</option>`)
   - `set_label_for`, `set_control_name`, `set_button_kind`, `set_checkbox_role`, `set_radio_role`, `set_range_bounds`, `set_input_kind`
5. **Fast-Path Paint/Layout 슬롯 (`codegen_slots.rs`)**:
   - `bind_paint_translate_x/y`, `bind_paint_scale_x/y/uniform`, `bind_paint_rotate_deg`, `bind_paint_opacity`, `bind_paint_fast_transform`, `bind_paint_eval` → DOM 요소의 `style.transform` (누적 `translate3d(...) scale(...) rotate(...)`) / `style.opacity` 직접 갱신.
   - `bind_layout_width_px`, `bind_layout_height_px`, `bind_layout_top_px`, `bind_layout_left_px`, `bind_layout_right_px`, `bind_layout_bottom_px`, `bind_layout_margin_px`, `bind_layout_padding_px`, `bind_layout_gap_px`, `bind_layout_eval` → DOM 요소의 해당 CSS 인라인 속성(`px` 단위) 직접 갱신.
6. **이벤트 바인딩 (`codegen_events.rs`)**:
   - `bind_click` (`ClickTarget`), `set_on_click`
   - `set_on_input` (`InputTarget` — `<input>`/`<textarea>`/`<select>` 변경 시 현재 `value` 전달)
   - `set_on_change`, `set_on_keydown`, `set_on_focus`, `set_on_blur`
   - `set_on_pointer_down`, `set_on_pointer_up`, `set_on_pointer_move`, `set_on_context_menu`, `set_on_wheel`, `set_on_scroll`, `set_on_submit`, `set_on_reset`
7. **미디어 & 벡터 (`media_codegen.rs`)**:
   - `set_node_svg_template(id, SvgVectorTemplate)`: 실제 `<svg xmlns="http://www.w3.org/2000/svg" viewBox="...">` DOM 요소와 내부 `<path>`, `<rect>`, `<circle>`을 생성한다. `SvgColor::CurrentColor`는 `currentColor`로 매핑해 CSS `color` 상속과 호환되게 한다.
   - `set_canvas_draw(id, CanvasDraw)`: `<canvas>`에 리액티브 2D 그리기 바인딩을 연결한다.
   - 정적 `<img>` (`media_codegen.rs`): 현재 `emit_static_img_src_slot`이 `krui::DecodedImage::from_encoded_bytes(include_bytes!(...))`를 방출하므로 `image` 크레이트에 의존한다. `web-dom`에서 `image` 크레이트를 제거하기 위해 `media_codegen.rs`가 `::krui::__private::core::static_ir::mount_static_image_bytes(__tree, __node_id, include_bytes!(#abs_str))` 헬퍼를 호출하도록 통일한다 (`native-engine`은 `DecodedImage::from_encoded_bytes`로 디코딩하고, `web-dom`은 매직 바이트로 MIME 타입(`image/png`, `image/jpeg`, `image/webp`)을 판별해 `Blob` URL → `<img src="blob:...">`로 브라우저 네이티브 디코더에 위임한다. 동적 `<img src="...">`는 `bind_dynamic_image_src`로 URL 직접 설정).

---

### 5.3 `krui-web` 런타임 설계 (`WebDomTree`, `DomBackend`, `BrowserDomBackend`)

#### (1) `ControlFlowAnchor`와 키 기반 재조정 (`<if>` / `<for>`)
- **DOM 앵커 표현**: `StaticNodeKind::ControlFlowAnchor`는 DOM의 주석 노드(`document.createComment("krui-anchor")`)로 생성한다.
- **`insertBefore(child, anchor)` 삽입**: 모든 `<if>` 브랜치와 `<for>` 항목의 루트 노드는 자신의 앵커 노드 바로 앞에 `insert_before`로 삽입되므로 형제 순서가 항상 정확하다.
- **서브스코프(`Scope`) 격리 및 누수 방지**:
  - `<if>`의 활성 브랜치마다 자식 `Scope`를 생성해 그 안에서만 `Effect`와 DOM 노드를 바인딩한다. 브랜치가 바뀌면 이전 브랜치의 `Scope`를 `dispose()`하고(내부 모든 `Effect`·이벤트 리스너·`<canvas>` 옵저버 자동 정리) DOM 노드를 제거한다.
  - `<for>`는 `key` 문자열 기준으로 기존 항목의 `(Scope, Vec<DomNodeId>)`를 재사용하고 순서만 `insert_before`로 재배치한다. 따라서 리스트 갱신 중에도 `<input>` 포커스·IME 조합 상태가 유지되며, 삭제된 항목의 `Scope`와 DOM만 정리된다.

#### (2) 이벤트 리스너 수명 관리 (Zero `.forget()`)
- 기존 `browser_host.rs`처럼 `closure.forget()`을 사용하면 `<if>`/`<for>`가 열리고 닫힐 때마다 JS 힙과 WASM 클로저 테이블이 영구 누수된다.
- `BrowserDomBackend`는 `DomNodeId`별로 등록된 `(EventTarget, event_name, Closure)` 핸들 목록을 보관한다. `remove_node(id)` 또는 소유 `Scope` 정리 시 `remove_event_listener_with_callback`을 호출한 뒤 `Closure`를 드롭한다.
- 버블링/캡처 및 `PointerEvent.local_x / local_y`: `getBoundingClientRect()` 기준으로 로컬 좌표를 계산하고, `WheelEvent`는 `deltaX / deltaY` 부호를 KRui 규약(위로 스크롤 시 양수 등 — `crates/core`와 일치)에 맞춘다.

#### (3) 순수 CSS 스타일시트, UA 리셋, 폰트, 테마/모션 동기화
- **UA 리셋 레이어**: `krui://preset/unstyled`가 지정된 문서가 마운트될 때 작성자 스타일시트보다 먼저 `@layer krui-ua-reset`을 주입한다:
  - `*, *::before, *::after { box-sizing: border-box; }`
  - `html, body, #krui-root { width: 100%; height: 100%; margin: 0; padding: 0; overflow: hidden; }`
  - `h1, h2, h3, h4, p, ul, ol, li, figure { margin: 0; padding: 0; }` / `ul, ol { list-style: none; }`
  - `button { font: inherit; color: inherit; cursor: pointer; display: inline-flex; align-items: center; justify-content: center; }`
  - `div, section, nav, aside, main, header, footer` 등 일반 블록 태그가 `component_gallery` CSS에서 `flex-direction` 등을 기대하는 경우(Stylo/Taffy와 CSS 기본값 차이) 점검 후 최소 리셋으로 보정한다.
- **스타일시트 & `@font-face` 주입**:
  - `doc.stylesheets`의 각 CSS 문자열을 `<style data-krui-sheet="idx">`로 `<head>`에 1회 주입한다.
  - 주입 전 CSS 내 `url("krui://inline/<key>")`를 `register_embedded_font`로 받은 바이트의 `Blob` Object URL(`blob:...`)로 치환한다.
  - **폰트 동작**: `@font-face { font-family: "Roboto"; src: url("blob:..."); }`로 영문은 `Roboto-Variable.ttf`가 적용되고, 한글/일본어/중국어/이모지는 브라우저의 CSS `font-family` 폴백(`"Noto Sans KR", "Malgun Gothic", sans-serif` → 시스템 폰트)에 의해 **0 KB 추가 다운로드로 자동 렌더링**된다.
  - *(주의)* `examples/component_gallery/views/themes/material.css`의 `@font-face`는 `Roboto-Variable.ttf` 하나뿐이므로, `"Roboto"` 폰트가 한글 글리프를 못 찾으면 CSS 명세에 따라 자동으로 다음 패밀리(`"Noto Sans KR"`, `"Malgun Gothic"`, `sans-serif`)로 글리프별 폴백(per-character font fallback)이 동작한다.
- **색채 스킴 & 모션 동기화**:
  - `set_color_scheme_preference(pref)`: `document.documentElement.style.colorScheme`를 `"light dark"` / `"light"` / `"dark"`로 설정한다.
  - `set_animations_paused(paused)` / `set_reduced_motion(pref)`: `:root`의 `data-krui-motion="reduce"` 속성을 토글하여 `krui-compiler`가 재작성한 `:root:where([data-krui-motion="reduce"])` 규칙이 그대로 작동하게 하고, 명시적 일시정지용 클래스를 지원한다.
- **Material 잉크 리플 (`--krui-pointer-x`, `--krui-pointer-y`)**:
  - 마운트 루트(`#krui-root`)에 단일 위임 `pointerdown` 리스너를 두어, 클릭된 대상(또는 가장 가까운 `[class*="krui-ui-"]` / `button` 조상)에 `style.setProperty("--krui-pointer-x", ...)`와 `--krui-pointer-y`를 기록한다.

#### (4) `<input>` / `<textarea>` 양방향 바인딩 & OS IME 보호
- 한글/일본어 IME 조합 중 `input` 이벤트가 발생해 Rust `Signal<String>`이 갱신되면, 같은 노드의 리액티브 `Effect`가 다시 `el.set_value(...)`를 호출할 수 있다.
- **조합 보호 규칙**:
  - 각 입력 요소에 `composing: bool` 플래그(`compositionstart` → `true`, `compositionend` → `false`)를 둔다.
  - `composing == true`인 동안에는 Rust → DOM 방향 `set_value` 덮어쓰기를 건너뛴다.
  - 조합 중이 아닐 때도 `el.value() == next_val`이면 `set_value`를 호출하지 않아 커서/선택 영역이 끝으로 튀는 현상을 막는다.
- `<input type="range">` (`Sliders`)는 `input` 이벤트에서 숫자 문자열을 전달하고 `min`/`max`/`step` 속성을 유지한다.

---

### 5.4 로컬 `<canvas>` 2D & 3D WebGPU/WebGL2 통합 설계

#### (1) 2D `<canvas draw="{...}">` (예: `GalleryCanvasStudio`)
- `WebDomTree::set_canvas_draw(node_id, draw)` 호출 시 `CanvasDraw` 핸들을 노드에 연결하고, `DomBackend`에 2D 캔버스 초기화를 요청한다.
- **크기 및 HiDPI 동기화**:
  - `BrowserDomBackend`는 해당 `<canvas>`에 `ResizeObserver`를 부착해 CSS 레이아웃 크기(`clientWidth`, `clientHeight`) × `window.devicePixelRatio()`(최소 1.0)로 `canvas.width`/`canvas.height`를 맞춘다.
- **리액티브 그리기 (`Effect` + `CanvasRenderingContext2d`)**:
  - 노드의 `Scope` 안에서 `Effect`를 등록해 `draw.draw(&mut ctx_2d, Size::new(lw, lh))`를 실행한다. `draw` 클로저가 읽는 `Signal`(예: `shapes`, `selected_id`)이 바뀔 때만 해당 `<canvas>`만 재실행된다(유휴 시 0 CPU).
  - 기록된 `Vec<CanvasCommand>`를 브라우저 `CanvasRenderingContext2d` API로 변환한다 (`save()`/`scale(dpr, dpr)` 후 `FillRect`, `StrokeRect`, `FillCircle`, `StrokeLine`, `FillPath`, `StrokePath`, `SetTransform`, `PushClip`, `PopClip`, 그라데이션, 텍스트, `restore()`).
- **포인터 드래그 (`setPointerCapture`)**:
  - `<canvas>`에 `on_pointer_down` 발생 시 `set_pointer_capture(pointer_id)`를 호출해, 사용자가 도형을 빠르게 드래그하다 커서가 캔버스 밖으로 나가도 `pointermove`/`pointerup`이 끊기지 않게 한다.

#### (2) 3D WebGPU → WebGL2 `<canvas>` (`#dev-3d-viewport-slot`)
- **범용 마운트 훅 (`krui-web`)**:
  - `krui-web`은 특정 3D 씬을 알지 않는다. 대신 임의의 요소 `id`가 DOM에 마운트·언마운트될 때 호스트가 외부 `<canvas>`(WebGPU, WebGL2, WebRTC 비디오 등)를 부착·해제할 수 있는 **범용 라이프사이클 훅**(`register_slot_host(element_id, on_mount, on_unmount)`)을 제공한다.
- **지연(Lazy) GPU 초기화 & 백엔드 자동 선택 (`krui-showcase-wasm`, `web-gpu-3d` 피처)**:
  - 사용자가 `Develop → 3D Viewport`로 진입해 `#dev-3d-viewport-slot`이 `<if>`로 DOM에 마운트될 때만:
    1. 슬롯 내부에 `<canvas>`(폭/높이 `100%`)와 `ResizeObserver`를 생성한다.
    2. `wasm_bindgen_futures::spawn_local`로 `wgpu::Instance::new(Backends::BROWSER_WEBGPU | Backends::GL)` → `request_adapter(compatible_surface: Some(&surface))` → `request_device(Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()))`를 비동기 초기화한다. (초기화 중 슬롯이 언마운트되면 세대 토큰(generation token) 검사로 안전하게 폐기한다.)
    3. `examples/component_gallery/assets/open_teapot.obj`와 `MODEL_3D_WGSL` 셰이더 + `Depth32Float` 뎁스 버퍼를 생성하고, `StudioController`의 `orbit_yaw`, `orbit_pitch`, `orbit_dist`, `model_scale`, `auto_spin` 시그널과 연동해 `requestAnimationFrame` 루프로 렌더링한다.
  - **언마운트 시 즉시 자원 반납**: 다른 탭/페이지로 이동해 `#dev-3d-viewport-slot`이 DOM에서 제거되면 `on_unmount`가 호출되어 `rAF` 루프·`ResizeObserver`·`wgpu` surface/device 핸들을 즉시 드롭한다. 따라서 일반 UI 페이지에서는 GPU 메모리와 rAF 비용이 **0**이다.
  - **포인터/휠 오빗 조작**: `GalleryViewport3d.krui`의 `#dev-3d-viewport-hit` 오버레이가 `WebDomTree`의 `onpointerdown`/`onpointermove`/`onpointerup`/`onwheel` 핸들러를 그대로 받아 `StudioController` 시그널을 갱신한다.
  - **폴백 안내**: 브라우저/환경이 WebGPU와 WebGL2를 모두 지원하지 않으면 패닉 없이 슬롯 내부에 안내 배지를 표시한다.

---

### 5.5 진입점 및 빌드 구성 (`web/index.html`, `krui-showcase-wasm`)

- **`web/index.html`**:
  - 기존 전체 화면 `<canvas id="krui-canvas">`를 `<div id="krui-root"></div>`로 교체하고 `overflow: hidden; width: 100%; height: 100%;` 기본 컨테이너만 둔다.
- **`src/bin/krui-showcase-wasm.rs`**:
  - `#[cfg(all(target_arch = "wasm32", feature = "web-dom"))]`: `BrowserDomBackend::mount_in("#krui-root")`를 초기화하고 `GalleryController` + `include_krui!("examples/component_gallery/views/component_gallery.krui")`를 마운트한 뒤 `#dev-3d-viewport-slot` 3D 캔버스 호스트를 등록한다.
  - `#[cfg(not(target_arch = "wasm32"))]`: 네이티브 테스트 호스트에서도 `MockDomBackend`로 동일 마운트·상호작용·해제 단위 테스트를 실행할 수 있다.
  - 기존 `native-engine` 호환성: `cargo test --workspace` 등 기본 피처 빌드에서는 `krui-showcase-wasm`이 `native-engine` 또는 `web-dom` 중 활성화된 경로에 맞춰 깔끔하게 컴파일되도록 `Cargo.toml`의 `required-features` 또는 모듈 게이트를 정리한다.

---

## 6. 검증 전략 (Verification Strategy)

구현 완료 선언 전 다음 5가지 검증을 반드시 통과시킨다.

1. **의존성 격리 검증 (`cargo tree`)**:
   - `cargo tree --target wasm32-unknown-unknown --no-default-features --features web-dom -p krui` 출력에 `stylo`, `taffy`, `cosmic-text`, `tiny-skia`, `krui-core`, `krui-renderer`, `wgpu`, `image`가 **0건**임을 확인한다.
2. **네이티브 `MockDomBackend` 단위 테스트**:
   - `cargo test -p krui-ui-types` 및 `cargo test -p krui-web`
   - 정적 IR 마운트, `<if>` 브랜치 전환 시 이전 `Scope`/DOM 정리, `<for>` 키 기반 재조정 시 노드/포커스 유지, `CanvasCommand` 기록, 리스너/Effect 누수 0건 검증.
3. **WASM 빌드 & 바이너리 크기 측정**:
   - `cargo build --target wasm32-unknown-unknown --no-default-features --features web-dom,web-gpu-3d --bin krui-showcase-wasm --release` + `wasm-bindgen --target web --out-dir web/pkg ...`
   - 결과 `.wasm` 크기(및 `web-dom` 단독 빌드 크기)를 기존 **10.71 MB**와 비교 보고한다.
4. **실제 브라우저 동작 확인 (`http://127.0.0.1:8788/`)**:
   - **한글/다국어**: `Blog`, `Buttons`, `Sliders`, `Loading & progress`, `2D Canvas Studio` 등에서 한글이 `.notdef` 박스(`□`) 없이 선명하게 렌더링되는지 확인.
   - **애니메이션/스크롤**: `Loading & progress` CSS `@keyframes` 스피너, 드로어 전환, 마우스 휠 스크롤, 다크 모드(`light-dark()`) 전환이 버벅거림 없이 60fps 수준으로 동작하는지 확인.
   - **2D `<canvas>` & 3D WebGPU/WebGL2**: `2D Canvas Studio`에서 도형 추가/드래그/색상 변경이 동작하고, `3D Viewport`에서 `open_teapot.obj`가 렌더링·회전·줌 되며 다른 탭 이동 시 멈추는지 확인.
   - **한글 IME 입력**: `Text fields`에서 실제 입력 및 양방향 바인딩이 깨지지 않는지 확인.
5. **기존 워크스페이스 회귀 검증**:
   - `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` 전체 통과.

---

## 7. 구현자(Builder)에게 위임되는 세부 사항

- `crates/ui-types` 및 `crates/web-dom` 내부의 모듈 파일 분할 구조(단, 각 파일 ≤ 500줄 / 함수 ≤ 60줄 규율 준수).
- `crates/web-dom/Cargo.toml`의 정확한 `web-sys` 피처 플래그 목록.
- 구현 계획(`writing-plans`)에서의 단계별 커밋/검증 단위 분할 순서.
