# KRui W3C CSS 엔진 정합화 및 `m3.material.io` 갤러리 클론 원샷 보고서

- **작성 및 실측 일자**: `2026-11-17`
- **기준 체크포인트**: `efae2f5` (`chore: checkpoint before CSS engine + M3 gallery one-shot`)
- **최종 구현 커밋**: `deba504` (`chore(H1): replace fake Tailwind CSS, enforce syn AST gate across examples, and align docs with measured conformance`)
- **원격 푸시 / 이력 재작성 / `apps/**` 수정**: 없음 (`0건`, 로컬 브랜치 `master`에만 단계별 분리 커밋 유지)

---

## 1. 단계별 커밋 및 Tailwind v4 실제 출력 일치율 요약

`tests/real_css_conformance.rs`에서 `@tailwindcss/cli@4.3.5` 무수정 출력(일반 및 `--optimize`)과 헤드리스 Chromium(`Playwright`) 실측 오라클(`tests/fixtures/real_css/tailwind_v4/oracle.json`)을 비교한 단계별 속성·기하 일치율 변화는 다음과 같다.

| 단계 | 로컬 커밋 | 핵심 변경 요약 | 일치율 (`N/M`) | 알려진 갭 (`KNOWN_GAPS.toml`) | 회귀 |
|---|---|---|---|---:|---:|
| Baseline | `efae2f5` | 작업 전 체크포인트 및 `target/gallery_before`, `target/examples_before` 저장 | 측정 전 | - | - |
| `S0` | `5d09d5f` | `@tailwindcss/cli@4.3.5` 실제 출력, Chromium 오라클, 양방향 래칫(`real_css_conformance.rs`) 도입 | **7 / 229 (3.1%)** | 222건 (`CompileError`로 전체 차단) | 0 |
| `E1` | `16a19a5` | W3C Flexbox §4 / Grid §6.1 blockification + `content` / `::before`·`::after` 정합화 | **9 / 229 (3.9%)** | 220건 | 0 |
| `E2` | `29e6f87` | `var()` 캐스케이드 후 일괄 치환, IACVT 폴백, `guaranteed-invalid`, `@property` syntax 검증 | **223 / 229 (97.4%)** | 6건 | 0 |
| `E3` | `c560be6` | `minify` 제거(폴백·`0px` 보존), 인라인 `style=""` 파서 단일화, `border-style: none` 초기값, 컴파일러 기반 `@supports` | **225 / 229 (98.3%)** | 4건 | 0 |
| `E4` | `3a66436` | UA CSS 분리(`unstyled.krui-sys`/`ua_shared.krui-sys`), `<ul>` 강제 스크롤 제거, `::placeholder`, 모달 전용 `::backdrop`, `core`/`renderer` AST 게이트 | **226 / 229 (98.7%)** | 3건 | 0 |
| `E5` | `e20f202` | `padding_set_mask`, 체크박스·라디오 UA CSS + `accent-color`, `scrollbar-width`/`scrollbar-color`, 미지원 의사요소 경고+drop | **226 / 229 (98.7%)** | 3건 | 0 |
| `E5b` | `8fb9ae2` | `icons/` 하위 컴포넌트 탐색 + `<template name>` + `<slot>` 기본 자식 폴백 지원 | **226 / 229 (98.7%)** | 3건 | 0 |
| `G1-icons` | `1e1b90e` | `m3.material.io` 48페이지 실측 + `@material-symbols/svg-400@0.49.0` 77개 아이콘 컴포넌트 + 내장 위젯 `<slot name="icon"/>` (엔진 수정 0건) | **226 / 229 (98.7%)** | 3건 | 0 |
| `G1-tokens` | `facdf59` | 실측 기반 `themes/{material,tailwind}.css` 토큰 및 `styles/{rail,drawer,doc_page,toc,home,overview}.css` 분리 (엔진 수정 0건) | **226 / 229 (98.7%)** | 3건 | 0 |
| `G1-pages` | `0d2f1df` | `D1–D13` 시각 결함 교정 및 `geometry_gates.rs` 헤드리스 기하·무잘림·레퍼런스 치수 게이트 5종 추가 (엔진 수정 0건) | **226 / 229 (98.7%)** | 3건 | 0 |
| `H1` | `deba504` | `09_tailwind_v4` 실제 CLI 출력 교체 + 호버 전환 단정, Fluent 2 실제 중립 토큰, `examples/**` 전체 `syn` AST 게이트, 문서 측정치 정합화 | **226 / 229 (98.7%)** | 3건 | 0 |

---

## 2. 단계별 상세 기록 (원인, Diff 요약, 실패 → 통과 로그)

### 2.1 `S0` — 실제 CSS 적합성 측정 장치 (`5d09d5f`)
- **원인**: 실제 `@tailwindcss/cli@4` 출력을 `examples/09_tailwind_v4/views/app.krui`에 연결하면 `Undefined CSS variable '--tw-gradient-position'`으로 `CompileError`가 발생했으나, 실제 외부 CLI 출력을 검증하는 테스트가 0개였다.
- **변경 요약**:
  - `tests/fixtures/real_css/tailwind_v4/{tailwind_v4.css, tailwind_v4.optimize.css, SOURCE.md, oracle.json}` 및 재생성 스크립트 `tools/regen_real_css.ps1`, 오라클 추출기 `tools/extract_real_css_oracle.mjs` 추가.
  - `tests/real_css_conformance.rs` 및 `tests/fixtures/real_css/KNOWN_GAPS.toml` 구현: 미등록 실패(회귀)와 이미 해결된 갭 잔류 모두 테스트 실패로 잡는 양방향 래칫 적용.
- **검증 로그**:
  - `속성 일치율 7/229 (3.1%) | 알려진 갭 222건 | 회귀 0건 | 해소된 갭 0건` (`2 passed; 0 failed`).

### 2.2 `E1` — Flex/Grid 아이템 Blockification + `content` / `::before`·`::after` (`16a19a5`)
- **원인 (`파일:줄`)**:
  1. `crates/layout/src/style_conv.rs:9-19` 및 `crates/core/src/view_tree/rich_inline.rs:46`: Flex/Grid 컨테이너 직계 자식의 `Display::Inline` / `Display::InlineBlock`을 W3C CSS Flexbox §4 / Grid §6.1 규칙에 따라 `Block`으로 승격(blockify)하지 않아 교차축 `align-items: center` 정렬이 무시되고 상단·좌측에 붙었다.
  2. `crates/compiler/src/declarations_extra.rs:148` 및 `crates/compiler/src/selector_match.rs:92`: `::before`/`::after` 박스는 생성하면서도 `content: ""` 선언을 `"Unsupported CSS property 'content'"` 경고와 함께 drop했고, 동적 클래스 바인딩(`class="base {sig}"`) 호스트에서 정적 클래스의 `::before` 매칭이 누락되었다.
- **변경 요약**:
  - `crates/layout/src/style_conv.rs`에 부모 `Display` 및 자식 `Position` 기반 W3C blockification(`Inline`/`InlineBlock` → `Block`, `InlineFlex` → `Flex`, `InlineGrid` → `Grid`, `absolute`/`fixed` CSS 2.1 §9.7) 구현 — 매크로 코드젠, `class_patch`, `preview` 인터프리터가 동일한 레이아웃 변환 경로를 공유.
  - `declarations_extra.rs`, `pseudo_boxes.rs`, `selector_match.rs`, `rich_inline.rs`에서 `content`(`""`, 문자열, `none`/`normal`) 정식 처리 및 `::before`/`::after` 인접 텍스트 래핑 보정.
- **회귀 테스트 (`tests/flex_item_blockification.rs`)**:
  - **수정 전 실패 로그**: `r-inl: actual=11.50, expected=30.00 (diff=18.50)` 및 `Unsupported CSS property 'content'` 경고 발생.
  - **수정 후 통과 로그**: `test result: ok. 4 passed; 0 failed` (매크로·`preview` 동치 `±0.5px` 및 `content: ""` 페인트 / `content: none` 미페인트 단정 통과).

### 2.3 `E2` — `var()` 및 커스텀 속성 W3C 의미론 (`29e6f87`)
- **원인 (`파일:줄`)**:
  - `crates/compiler/src/cascade_rules.rs:259-274`, `stylesheet_tokens.rs:112-158`, `inheritance.rs:144-158`, `scoped_tokens.rs:57`: 미정의 `--*` 변수, 순환 참조, 중첩 체인이 `CompileError`로 전체 컴파일을 중단시켰으며, `cascade_rules.rs:261`은 실제 에러 문자열과 다른 `"Circular CSS variable"` 문자열 비교에 의존했다.
  - `crates/compiler/src/cascade.rs:375-383`: 요소별 캐스케이드 종료 전에 규칙마다 즉시 `var()`를 해석해 후순위 클래스의 `--*` 오버라이드가 선행 규칙에 반영되지 않았다.
- **변경 요약**:
  - `crates/compiler/src/{css_custom_props.rs, values_var.rs}` 신설: 구조적 `CssVarError` enum 도입, 요소별 캐스케이드(`@layer`, `!important`, 명시도, 선언 순서) 완료 후 커스텀 속성 일괄 해석, 순환·미정의 `guaranteed-invalid` 처리, `@property` `syntax`(`<length>`, `<color>`, `<number>`, `<percentage>`, `<length-percentage>`) 검증 및 불일치 시 `initial-value` 폴백, 일반 속성의 IACVT 시 상속/초기값 리셋 + 비차단 `CompileWarning`(`strict_css`에서만 `CompileError`).
  - `lib_tests.rs:40-76`의 미정의 변수 에러 단정은 `strict` 모드 전용으로 전환.
- **회귀 테스트 (`tests/css_custom_properties_w3c.rs`)**:
  - **수정 전 실패 로그**: `CompileError: Undefined CSS variable '--only-dark'` 및 `@property <length>` 오염(`400` 그대로 통과).
  - **수정 후 통과 로그**: `test result: ok. 3 passed; 0 failed`, Tailwind v4 실제 출력 일치율 `9/229 → 223/229 (97.4%)` 상승.

### 2.4 `E3` — 파서 충실도 및 컴파일러 기반 `@supports` (`c560be6`)
- **원인 (`파일:줄`)**:
  - `crates/compiler/src/lcss_frontend.rs:47-62`: `stylesheet.minify(MinifyOptions::default())`가 선행 유효 폴백(`width: 100%; width: fit-content`)을 덮어쓰고 `0px`를 `0`으로 축약해 `calc(2px + 0)`을 깨뜨렸다.
  - `crates/compiler/src/css_scan.rs:76-123`: 인라인 `style=""`만 수제 스캐너를 사용했다.
  - `crates/compiler/src/borders_css.rs:70-91`: `border-style` 초기값을 `solid`처럼 취급해 `border-width`만 지정해도 테두리가 그려졌다.
  - `crates/compiler/src/lcss_shorthands.rs:291-360` & `css_supports.rs:120-122`: 손실 처리(`dashed`/`dotted`/`double`, 다중 `background`) 무경고, `@supports` 수기 목록 의존 및 `not (unknown())` 오판정.
- **변경 요약**:
  - `Targets` 지정 안(`223/229`)과 `minify` 제거 안(`225/229`)을 `S0` 일치율로 실측 비교해 **`minify` 제거 안 채택**.
  - 인라인 `style=""`을 `lightningcss` `DeclarationBlock::parse_string`으로 단일화.
  - W3C `border-style: none` 초기값 강제(`border-style` 미지정 시 `border-width` 실효 `0px`), 손실 속성 `CompileWarning` 추가, `@supports`를 실제 선언 컴파일러 + W3C `<general-enclosed>`(`not (unknown()) == true`) 판정으로 교체.
- **회귀 테스트 (`tests/css_parser_fidelity_e3.rs`)**:
  - **수정 전 실패 로그**: `0 passed; 5 failed` (`width: 100%; width: fit-content` → `Auto`, `border-width: 4px` 단독 테두리 노출, `not (unknown())` → `false`).
  - **수정 후 통과 로그**: `test result: ok. 5 passed; 0 failed`, 일치율 `223/229 → 225/229 (98.3%)`.

### 2.5 `E4` — `core`/`renderer` 하드코딩 제거 및 UA CSS 이전 (`3a66436`)
- **원인 (`파일:줄`)**:
  - `crates/core/src/view_tree/{mod.rs:445, paint.rs:179,358, stacking.rs:75, scroll.rs:274,303,330, hit_test.rs:63,206,262, hit_test_geom.rs:69, insets.rs:90}`: `SemanticRole::List`(`<ul>`)에 `overflow` 계산값과 무관하게 클리핑·스크롤·스크롤바를 강제했다.
  - `paint.rs:40, 467-471`, `paint_types.rs:16, 366-371`, `borders_css.rs:142-147`, `paint_box_layers.rs:250-263`, `stacking_toplayer.rs:55, 192-219`, `renderer/src/lib.rs:233-237`, `winit/src/session.rs:67,88,118`, `android-ffi/src/session.rs:64`, `css_unstyled.rs`, `control_attrs.rs:336-371`: 플레이스홀더 색상 고정, `caret-color: auto` 테마 토큰 참조, 백엔드별 테마 선택, `border-bottom` 인디케이터 왜곡, 비모달 `<dialog>` 백드롭 강제, Rust 상수 UA 기본값 이중 관리.
- **변경 요약**:
  - 모든 UA 기본 규칙을 `crates/style/presets/{unstyled.krui-sys, ua_shared.krui-sys}`로 이전하고 `crates/style/build.rs`가 시스템 토큰 배열을 빌드 타임 생성하도록 단일화.
  - `<ul>` 스크롤·클리핑을 오직 `overflow` 계산값으로만 판정, `::placeholder` 지원, `caret-color: auto → currentColor`, 렌더러·세션의 테마 선택 제거, 표준 4변 `border-bottom` 렌더링, 모달 `<dialog>` 전용 `::backdrop`(`transparent` 존중), 루트/`body` 배경 캔버스 전파.
  - `tests/core_ua_css_ast_gate_e4.rs`로 `core`/`renderer` 시각 경로 태그·역할 분기 `syn` AST 게이트 구축.
- **회귀 테스트 (`tests/core_ua_css_decoupling_e4.rs`, `tests/core_ua_css_ast_gate_e4.rs`)**:
  - **수정 전 실패 로그**: `<ul>` 기본 `overflow: visible`에서 강제 클립 발생, `::placeholder` 미적용, 비모달 `<dialog open>` 백드롭 생성.
  - **수정 후 통과 로그**: `9 passed; 0 failed` + AST 게이트 `1 passed; 0 failed`, 일치율 `225/229 → 226/229 (98.7%)`.

### 2.6 `E5` & `E5b` — 폼 컨트롤·스크롤바 CSS화 및 아이콘/슬롯 확장 (`e20f202`, `8fb9ae2`)
- **원인 (`파일:줄`)**:
  - `crates/core/src/view_tree/{forms.rs:113-152, paint_canvas.rs:134-259, scroll.rs:17-33}`: 명시적 `padding: 0`과 미지정을 구분하지 못했고, 체크박스·라디오 `20px`/반경·체크마크가 Rust 상수에 묶여 있었으며, 슬라이더가 `caret_color`를 재사용하고 스크롤바 폭·색상이 고정되어 있었다.
  - `crates/compiler/src/components.rs`: `views/icons/` 하위 디렉터리 컴포넌트 탐색 및 `<slot>` 기본 자식 폴백 미지원.
- **변경 요약**:
  - `padding_set_mask` 도입, UA CSS 기반 체크박스·라디오 `20px`/`border-radius: 999px` 및 상속형 `accent-color` 도입, `scrollbar-width`(`auto`/`thin`/`none`)·`scrollbar-color` 지원, `::-webkit-*` 의사요소 경고 후 drop + `@supports selector(::-webkit-slider-thumb) == false`.
  - `E5b`(`8fb9ae2`)에서 `icons/` 디렉터리 탐색 및 `<slot>기본 아이콘</slot>` 폴백 지원 추가.
- **회귀 테스트 (`tests/css_form_controls_scrollbars_e5.rs`, `tests/component_icon_slot_e5b.rs`)**:
  - **수정 전 실패 로그**: `0 passed; 4 failed` (`padding: 0` 무시, `accent-color` 미반영, `scrollbar-width: none` 미적용, `@supports selector(::-webkit-slider-thumb)` 오판) 및 `0 passed; 2 failed` (`icons/` 미탐색).
  - **수정 후 통과 로그**: `4 passed; 0 failed` 및 `2 passed; 0 failed`.

### 2.7 `G1-icons`, `G1-tokens`, `G1-pages` — `m3.material.io` 실측 클론 (`1e1b90e`, `facdf59`, `0d2f1df`)
- **엔진 수정 여부**: **0건** (`G1` 3개 커밋 전체가 `examples/08_component_gallery/**`, `crates/compiler/builtins/**`, `tools/**`, `tests/**`, `.gitignore`만 수정).
- **변경 요약**:
  - **`G1-icons` (`1e1b90e`)**: `tools/m3_reference/measure_m3.mjs`로 `1280×840` 라이트 모드에서 `m3.material.io` 7개 상위 섹션 + 41개 `doc_page`(`0..40`) 전수 실측(`reference/m3_spec/*.json`, `reference/README.md`). `@material-symbols/svg-400@0.49.0` 기반 `tools/gen_m3_icons.mjs`로 `views/icons/*.krui` 77종 생성, 내장 위젯 10종에 `<slot name="icon"/>` 추가, 갤러리 내 이모지·유니코드·수제 `<svg>` 153건 전량 교체.
  - **`G1-tokens` (`facdf59`)**: 실측 토큰(`--md-sys-typescale-*`, `--md-sys-shape-corner-*`, `--md-icon-size*`, `--md-site-*`)을 `views/themes/{material,tailwind}.css`에 정의(테마 전환 0-relayout 유지), `1,266`줄이던 `component_gallery.css`를 `views/styles/{rail,drawer,doc_page,toc,home,overview}.css` + 베이스 시트로 분리, `@layer preset`의 `span` 색상 강제 제거 및 `border-style: solid` 명시.
  - **`G1-pages` (`0d2f1df`)**: 문서 히어로 배너·탭 바를 `.m3-doc-viewport` 전폭 구조로 정합화, 목차 `22px` 중복 제목 제거, 체크박스/라디오 `@layer preset` `padding: 0px` 리셋, `cat4` 기본 문서 페이지를 `Checkbox(6)`로 교정, `examples/08_component_gallery/geometry_gates.rs` 5대 기하·무잘림·치수 게이트 연결.
- **회귀 및 기하 게이트 로그**:
  - `tests/m3_icons_svg_parity.rs`: 수정 전 `Found 153 banned icon/SVG occurrences` 실패 → 수정 후 `2 passed; 0 failed`.
  - `tests/m3_gallery_tokens_parity.rs`: 수정 전 `height: 58px` 레거시 치수 실패 → 수정 후 `2 passed; 0 failed`.
  - `geometry_gates.rs`: 수정 전 `Category 4 (Selection) must open Checkbox(6), left: 20, right: 6` 실패 → 수정 후 5개 게이트 전수 통과.

### 2.8 `H1` — 정직성 정리 (`deba504`)
- **원인 (`파일:줄`)**:
  - `examples/09_tailwind_v4/views/tailwind_v4.css`: 실제 CLI 출력이 아닌 수제 축약본(`@property` `initial-value: #6366f1/#4f46e5/#06b6d4`, `min-height: 760px`, `.tw-popover`)을 사용하고 있었고, `examples/09_tailwind_v4/main.rs`에 호버 전후 스타일 변화 단정이 없었다.
  - `crates/style/presets/fluent.krui-sys`: 헤더가 프리셋 성격을 과장했고 색상이 Tailwind Slate(`#0f172a`, `#1e293b`, `#334155`, `#94a3b8`)였다.
  - `tests/probe_and_bench_gates.rs:205-224`: `08` 두 파일에만 문자열 `.contains` 검사를 수행했고 `examples/02..07/main.rs`에 불필요한 `switch_theme_preset`/`set_theme`/`ThemePreset` 호출이 남아 있었다.
  - `probe_krui.ps1:121-141`의 의미 없는 SHA256 해시 상이성 검사 및 `tests/live_preview_component_gallery_e2e.rs:247`의 `.gallery-shell { background-color }` 직접 덮어쓰기 존재.
- **변경 요약**:
  - `examples/09_tailwind_v4/views/tailwind_v4.css`를 `S0`의 실제 `@tailwindcss/cli@4.3.5` 출력으로 무수정 교체하고 `examples/09_tailwind_v4/main.rs`에 `#open-popover-btn` 호버 후 `background_color` 및 `standalone_transform.translate_y` 변화 + 120프레임 `0` relayout 단정 추가.
  - `themes/{material,tailwind}.css`, `micl.css`, `fluent.krui-sys` 헤더를 `"KRui 토큰 팩(○○ 팔레트 참고)"`로 통일하고 `fluent.krui-sys`를 Microsoft Fluent 2 Web Dark 실제 중립·브랜드 토큰(`#141414`, `#1f1f1f`, `#292929`, `#3d3d3d`, `#616161`, `#999999`, `#d6d6d6`, `#ffffff`, `#0078d4`, `#479ef5`)으로 교체(`tests/w3_design_systems_and_inheritance.rs` 기대 픽셀 동기화).
  - `tests/probe_and_bench_gates.rs`를 `examples/**/*.rs` 전체 대상 `syn` AST 게이트로 확장(`apps/**`는 읽기 전용 보고: `28건`), `probe_krui.ps1` SHA256 검사 삭제, `live_preview_component_gallery_e2e.rs`에서 토큰 경로만으로 프레임버퍼 변경 단정, `README.md`·`AGENTS.md`를 실측치 `226/229 (98.7%)`에 맞게 갱신.
- **회귀 테스트 (`tests/honesty_cleanup_h1.rs`)**:
  - **수정 전 실패 로그**: `0 passed; 3 failed` (CLI 출력 불일치, `micl.css`/`fluent.krui-sys` 헤더·팔레트 불일치, `examples/02..07/main.rs` AST 위반 16건).
  - **수정 후 통과 로그**: `test result: ok. 3 passed; 0 failed` 및 `live_preview_component_gallery_e2e` `1 passed; 0 failed`.

### 2.9 사전 승인 기본 범위 외 수정 파일 및 사유

| 수정 파일 | 수정 사유 ("왜 필요했는지") |
|---|---|
| `examples/02_mvvm_pattern/main.rs`, `examples/03_mvc_pattern/main.rs`, `examples/04_elm_flux_pattern/main.rs`, `examples/05_mvi_pattern/main.rs` | 각 예제의 `.krui` 파일이 이미 `<link rel="krui-preset" href="krui://preset/..." />`를 선언하고 있음에도 `main.rs`에 중복 `session.switch_theme_preset(ThemePreset::*)` 호출이 남아 있어 `H1`의 `examples/**` 전체 `syn` AST 게이트를 통과시키기 위해 중복 Rust 호출 1줄씩 제거(`.krui`/`.css` 무수정). |
| `examples/06_embed_offscreen/main.rs` | `ThemeTokens::from_preset(ThemePreset::Fluent)` 대신 트리의 선언형 토큰 테이블(`ThemeTokens::from_table(&rt.tree.token_table())`)을 읽도록 1줄 교체해 `ThemePreset` 의존 제거. |
| `examples/07_controls/main.rs` | `controls_lab.krui` 무수정 원칙을 지키면서 `switch_theme_preset` 및 `set_theme(ThemePreset::*)` 호출을 가드레일 10 허용 API인 `set_color_scheme_preference(ColorSchemePreference::*)`로 교체. |
| `examples/09_tailwind_v4/views/tailwind_v4.css`, `examples/09_tailwind_v4/main.rs` | `H1` 지시에 따라 가짜 데모 스타일시트를 `S0` 실제 `@tailwindcss/cli` 출력으로 교체하고 호버 후 `translate`/`background_color` 실변화 단정을 추가(`app.krui` 무수정). |
| `.agents/skills/krui-harness2-loop/scripts/probe_krui.ps1` | `H1` 지시에 따라 121–141줄의 의미 없는 5개 CSS 파일 SHA256 상이성 검사를 삭제. |
| `README.md`, `AGENTS.md`, `.gitignore` | `H1` 지시에 따라 `"100% 무수정"`, `@container`, `cqw`/`cqi`, `filter`, `clip-path`, `::placeholder` 관련 문구를 실측 일치율(`226/229 [98.7%]`)에 맞추고, `G1(1)` 저작권 보호를 위해 `reference/screens/`를 `.gitignore`에 추가. |

---

## 3. 타 예제(`01–07`, `09`) 전후 스크린샷 변화 및 브라우저 정당성

`.krui`/`.css`를 수정하지 않은 예제들(`01–07` 전체 및 `09`의 `app.krui`)에 대해 작업 전(`target/examples_before/*.png`)과 작업 후(`target/examples_after/*.png`) `krui serve --panel` 실제 프레임버퍼를 비교한 결과는 다음과 같다.

| 예제 | 해상도 | 변경 픽셀 수 (비율) | 전후 스크린샷 경로 | 변화 원인 및 W3C 브라우저 정당성 |
|---|---|---:|---|---|
| `01_business_studio` | `1024×720` | `641,092 / 737,280` (`86.95%`) | `target/examples_before/01_business_studio.png` ↔ `target/examples_after/01_business_studio.png` | `H1`에서 `fluent.krui-sys`의 위장된 Tailwind Slate 배경(`#0f172a`/`#1e293b`)을 실제 Microsoft Fluent 2 Web Dark 중립 배경(`#141414`/`#1f1f1f`/`#292929`)으로 정정하고, `E1` flex 아이템 blockification으로 버튼·배지 내부 인라인 텍스트가 수직 중앙 정렬됨. |
| `02_mvvm_pattern` | `1080×720` | `126,485 / 777,600` (`16.27%`) | `target/examples_before/02_mvvm_pattern.png` ↔ `target/examples_after/02_mvvm_pattern.png` | `E1` flex/grid 아이템 blockification으로 배지·버튼 내부 `<span>` 교차축 정렬이 브라우저와 동일하게 중앙 정렬되고, `E5` UA CSS `20px` + `accent-color` 체크박스가 적용됨. |
| `03_mvc_pattern` | `1040×720` | `131,992 / 748,800` (`17.63%`) | `target/examples_before/03_mvc_pattern.png` ↔ `target/examples_after/03_mvc_pattern.png` | `E1` blockification 정렬 교정 및 `E3` W3C `border-style: none` 초기값 적용(`border-width`만 있고 `border-style`을 생략한 박스에서 브라우저와 동일하게 테두리 미표시). |
| `04_elm_flux_pattern` | `1060×720` | `131,678 / 763,200` (`17.25%`) | `target/examples_before/04_elm_flux_pattern.png` ↔ `target/examples_after/04_elm_flux_pattern.png` | `E1` 상단 상태 칩·KPI 카드 flex 정렬 정합화 및 `E4` `<ul>` 계산된 `overflow` 기반 스크롤 처리 반영. |
| `05_mvi_pattern` | `1080×720` | `165,453 / 777,600` (`21.28%`) | `target/examples_before/05_mvi_pattern.png` ↔ `target/examples_after/05_mvi_pattern.png` | `E1` flex 자식 정렬 정합화 및 `E3` `border-style: none` 초기값 적용으로 브라우저 CSS 렌더링과 일치. |
| `07_controls` | `1024×720` | `731,573 / 737,280` (`99.23%`) | `target/examples_before/07_controls.png` ↔ `target/examples_after/07_controls.png` | `H1` Fluent 2 실제 다크 중립 팔레트 반영 + `E4` `::placeholder` / 표준 `border-bottom` + `E5` UA CSS 체크박스·라디오·슬라이더 `accent-color` 및 `scrollbar-color` 반영. |
| `09_tailwind_v4` | `1120×760` | `196,180 / 851,200` (`23.05%`) | `target/examples_before/09_tailwind_v4.png` ↔ `target/examples_after/09_tailwind_v4.png` | `H1`에서 수제 축약 시트를 실제 `@tailwindcss/cli@4.3.5` 무수정 출력으로 교체. `oklch` 선형 그라데이션 배너, 5중 `ring-2 ring-indigo-500 shadow-lg`, `peer-checked` 스위치가 실제 CLI CSS만으로 Chromium 오라클과 동일하게 렌더링되며, Tailwind Preflight(`border: 0 solid transparent`)에 따라 `appearance` 미지정 기본 체크박스의 테두리가 리셋되는 동작까지 브라우저와 일치함. |

---

## 4. Material Symbols 아이콘 통일 매핑표, 생성기 사용법 및 라이선스

- **패키지 및 버전**: `@material-symbols/svg-400@0.49.0` (`outlined`, `wght=400`, 기본 `FILL=0` 및 활성 탭·레일용 `*Fill` `FILL=1`)
- **라이선스**: `Apache-2.0` (Google Material Symbols 공식 배포본, 상세 SHA-256 목록은 `examples/08_component_gallery/views/icons/SOURCE.md` 참조)
- **생성기 사용법**:
  ```powershell
  node tools/gen_m3_icons.mjs
  ```
  - `examples/08_component_gallery/views/icons/` 아래에 컴포넌트 1파일당 1아이콘(`IconHome.krui` 등 총 **77개**)을 결정론적으로 생성하며 재실행 시 `git diff`는 `0`줄이다.
  - `tests/m3_icons_svg_parity.rs`가 생성된 모든 `<path d="...">` 문자열이 `@material-symbols/svg-400` 원본 SVG와 **바이트 단위로 100% 일치**하고 갤러리 뷰 전체에 금지된 이모지·유니코드 아이콘·인라인 `<svg>`가 **0건**임을 검증한다.

| 기존 이모지 / 유니코드 / 수제 SVG | 교체된 공식 Material Symbol | 컴포넌트 이름 (`views/icons/`) | 사용 위치 |
|---|---|---|---|
| `🔍` / 수제 돋보기 `<svg>` | `search` | `<IconSearch />` | 레일 상단 검색 FAB(`56×56`), 홈 검색 버튼, `SearchBar`, `SearchView` |
| `🏠` / 수제 집 `<svg>` | `home` (`FILL 0/1`) | `<IconHome />`, `<IconHomeFill />` | 사이트 레일 `Home`, 모바일 하단 바, 브레드크럼 |
| 수제 그리드 `<svg>` | `apps` (`FILL 0/1`) | `<IconApps />`, `<IconAppsFill />` | 사이트 레일 `Get started`, 개요 배너 |
| 수제 `<>` `<svg>` | `code` (`FILL 0/1`) | `<IconCode />`, `<IconCodeFill />` | 사이트 레일 `Develop` |
| 수제 북마크 `<svg>` | `book` / `bookmark` (`FILL 0/1`) | `<IconBook />`, `<IconBookmark />` | 사이트 레일 `Foundations`, `IconButton`, `Cards` |
| 수제 팔레트 `<svg>` | `palette` (`FILL 0/1`) | `<IconPalette />`, `<IconPaletteFill />` | 사이트 레일 `Styles` |
| `+` 원형 / `+` 텍스트 | `add_circle` (`FILL 0/1`), `add` | `<IconAddCircle />`, `<IconAdd />` | 사이트 레일 `Components`, `Fab`(`40/56/96dp`), `ExtendedFab`, `SplitButton` |
| 수제 별 페이지 `<svg>` / `★` | `pages` / `star` (`FILL 0/1`) | `<IconPages />`, `<IconStar />` | 사이트 레일 `Blog`, `IconButton`, 리스트 보조 아이콘 |
| 문서 탭 무아이콘 / 수제 아이콘 | `info`, `style`, `design_services`, `accessibility_new` | `<IconInfoFill />`, `<IconStyle />` 등 | 문서 상단 4탭 (`Overview`, `Specs`, `Guidelines`, `Accessibility`) |
| `▾` / `▸` 유니코드 화살표 | `arrow_drop_down`, `arrow_drop_up`, `chevron_left`, `chevron_right` | `<IconArrowDropDown />` 등 | `SplitButton` 트레일링 메뉴 버튼, 드로어 카테고리 토글, 캐러셀·날짜 이동 |
| `⏸` / `▶` / `☀` / `🌙` | `pause`, `play_arrow`, `light_mode`, `dark_mode` | `<IconPause />`, `<IconDarkMode />` 등 | 레일 하단 모션 정지 토글(`48×48`), 라이트/다크 토글(`48×48`) |
| `🔔`, `📁`, `🧩`, `☰`, `☑`, `✎`, `•`, `✦` | `notifications`, `folder`, `widgets`, `menu`, `check_box`, `edit`, `check_circle`, `auto_awesome_mosaic` | `<IconNotifications />`, `<IconMenu />` 등 | `AppBar`, `Badge`, `NavigationDrawer`, `Checkbox`, `Chips`, `Dialog`, 카테고리 카드 |

---

## 5. 갤러리 결함 `D1–D10` 및 추가 결함(`D11–D13`) 교정 결과

전후 스크린샷은 `target/gallery_before/*.png`와 `target/gallery_after/*.png`에 저장되어 있으며, 잔여 오차는 `examples/08_component_gallery/geometry_gates.rs`가 실제 `node_layout`과 `PaintScene` 프리미티브로 매 실행마다 검증한다.

| ID | 결함 요약 | 원인 및 수정 위치 | 전후 비교 스크린샷 (`before` → `after`) | `m3.material.io` 대비 잔여 오차 |
|---|---|---|---|---|
| `D1` | 레일 라벨 좌측 쏠림 및 폭·타이포 불일치 | `E1` 이전 인라인 `<span>` 교차축 미정렬 + 임의 `88px`/`11px` 치수 → `E1` blockification + `styles/rail.css`(`width: 96px`, pill `56×32px`, 라벨 `12px/16px`) | `default_cards_overview_light.png` | `|label_cx - pill_cx| = 0.00px`, 레일 폭·pill·라벨 높이 오차 `0.00px` |
| `D2` | 문서 탭(`.md-pill-tab`) 형태·높이·아이콘 불일치 | 기존 `58px` 카드형 탭 + 아이콘 누락 → `component_gallery.krui` 전폭 헤더/탭 배치 + `styles/doc_page.css`(`height: 80px`, `border-radius: 48px`, `24px` Material Symbol + `16px` 라벨, 활성 라벤더 pill) | `doc_page_03.png`, `doc_page_04.png` | 탭 높이 오차 `0.00px` (`80.00px`), 아이콘·라벨 중심 오차 최대 `0.50px` |
| `D3` | FAB `+` 및 `.m3-step-badge` 번호 쏠림·암색 겹침 | 텍스트 `+` 폰트 메트릭 편향 + `@layer preset`의 `span { color: var(--krui-text) }`가 배지 색상을 덮어씀 → `<Fab><IconAdd slot="icon" /></Fab>`(`40/56/96dp`) + `component_gallery.css` `span` 색상 강제 제거 | `doc_page_27.png`, `doc_page_01.png` | FAB·IconButton 아이콘 중심 오차 최대 `0.00px`, Large FAB `96.0×96.0px` (`0.00px`) |
| `D4` | MICL Connected Button Group `"Day/We/Mo"` 잘림 | `micl.css` `.micl-btn-group > button`이 좁은 열에서 수축하며 줄바꿈·클리핑 발생 → `white-space: nowrap; flex-shrink: 0; padding: 10px 18px;` 및 스테이지 배치 조정 | `doc_page_03.png` | 글리프 박스 부모 content box 내부 100% 수용 (잘림 `0px`) |
| `D5` | `SplitButton` `▾` 기호 왜소 및 대칭 모서리 | 텍스트 `▾` 사용 + 균일 반경 → `crates/compiler/builtins/actions.krui`에 `<IconArrowDropDown />` 기본 슬롯 및 M3 비대칭 모서리(`20px 4px 4px 20px` / `4px 20px 20px 4px`) 적용 | `doc_page_31.png` | `24×24px` 아이콘 중앙 정렬 오차 `0.00px` |
| `D6` | 우측 목차 `22px` 제목 반복 및 테마 칩 왜소 | `component_gallery.krui`에서 `"On this page"` 아래 `22px` 섹션 제목 중복 렌더링 + 작은 칩 → 중복 제목 제거, `styles/toc.css` `180px` 폭·`12px` 헤더·`14px/32px` 보라색 앵커·`36px` 테마 전환 pill 적용 | `default_cards_overview_light.png`, `home_material.png` | 텍스트 무잘림 검증 통과 (`Gate 4`) |
| `D7` | 이모지·유니코드·수제 SVG 혼재 | 갤러리 전반 153건 수제 기호 사용 → 77개 공식 Material Symbols 컴포넌트로 전량 교체 (`tests/m3_icons_svg_parity.rs` 강제) | 전 스크린샷 (`home_material.png`, `doc_page_04.png` 등) | 금지 기호·인라인 SVG 잔존 `0건` |
| `D8` | 레일 하단 `Light`/`Auto` 칩 잘림 및 크기 이상 | 좁은 고정 폭 버튼 안에 긴 텍스트 배치 → `styles/rail.css`에서 `48×48px` 원형 모션·다크모드 아이콘 버튼 + `min-width: 56px; height: 28px` 스킴 모드 pill로 분리 | `default_cards_overview_light.png`, `cards_overview_dark.png` | 아이콘 중심 오차 `0.00px`, `Light`/`Dark`/`Auto` 무잘림 통과 |
| `D9` | `cat4_material.png`(Selection)가 `Switch` 표시 | `controller/sections.rs` `default_doc_page_for_category(4)`가 첫 항목 `Checkbox(6)` 대신 `Switch(20)` 반환 → `4 => 6`으로 수정 | `cat4_material.png` | `Checkbox(6)` 즉시 표시 + 체크마크 정상 렌더링 |
| `D10` | `::before` 상태 레이어 동작 및 무경고 검증 | `E1` `content: ""` 지원 및 동적 클래스 호스트 정적 셀렉터 매칭 반영 → `.krui-ui-btn::before`/`.micl-btn-group > button::before` 포인터 래치 리플이 `0` relayout으로 동작 | `doc_page_03.png` + `geometry_gates.rs` `Gate 4` | 경고 `0건`, 포인터 입력 시 `relayout = false` + 방사형 상태 레이어 생성 확인 |
| `D11` *(추가)* | 모달 `Dialog` 하단 `"취소"` / `"적용"` 버튼 텍스트 잘림 | `crates/compiler/builtins/containment.krui` `.krui-ui-dialog-actions button`에 `flex-shrink: 0; white-space: nowrap;` 누락 → 추가하여 한글·영문 액션 라벨 클리핑 해소 | `overlay_dialog_material.png` | `#sync-dialog` 내부 전 글리프 박스 무잘림 통과 (`Gate 4`) |
| `D12` *(추가)* | 문서 히어로 배너(`h1` `88px`) 패딩·타이포 레퍼런스 차이 | 기존 작은 카드형 제목(`28px`) → 실측 `m3.material.io` 히어로 컨테이너(`padding: 56px`, `border-radius: 24px`, `h1` `88px/96px`, 요약 `22px/28px`) 적용 (`styles/doc_page.css`) | `doc_page_04.png`, `default_cards_overview_light.png` | `header_padding` `56.00px`(`0.00px`), `title_lh` `97.00px` vs `96.00px`(오차 `1.00px <= 2.0px`) |
| `D13` *(추가)* | `E3` `border-style: none` 적용 후 테두리 소실 및 체크박스 패딩 압착 | `border-width`만 있던 규칙에 `border-style: solid` 명시, `@layer preset`의 `input { padding: 8px 14px }`가 `20×20` 체크박스 내부 체크마크 영역을 줄이던 문제를 `input[type="checkbox"], input[type="radio"] { padding: 0px; }`로 정정 | `cat4_material.png`, `home_tailwind.png` | `20×20px` 체크박스 체크마크 정상 비율 렌더링 |

---

## 6. 레퍼런스 실측 커버리지 및 `m3.material.io` 전체 인벤토리 구현 현황

### 6.1 실측 커버리지
- **측정 스크립트**: `tools/m3_reference/measure_m3.mjs` (`1280×840`, 라이트 모드, CAPTCHA/봇 차단 `0건`)
- **상위 섹션**: **7 / 7 측정 완료** (`section_home`, `section_get_started`, `section_develop`, `section_foundations`, `section_styles`, `section_components`, `section_blog`)
- **컴포넌트 문서 페이지 (`doc_page 0..40`)**: **41 / 41 측정 완료** (`examples/08_component_gallery/reference/m3_spec/doc_page_00.json` .. `doc_page_40.json`)
- **미측정 페이지 수**: **0개** (`geometry_gates.rs` 실행 시 `Reference Spec Coverage: sections=7/7, doc_pages=41/41, unmeasured=0` 출력)

### 6.2 `m3.material.io` 발견 링크 388개 대비 구현 현황 (다음 단계 백로그)

`reference/m3_spec/_site_inventory.json`에 수집된 사이트 내 고유 링크 **388개** 대비 현재 `08_component_gallery`의 구현 상태는 다음과 같다.

| 사이트 경로군 | 수집 링크 수 | 현재 상태 | 현재 `08_component_gallery` 구현 범위 | 다음 단계 백로그 (미구현·확장 대상) |
|---|---:|---|---|---|
| `/` (`Home` + 전역 검색) | 3 | **구현** | 히어로 배너, 디자인 시스템 스위처, 6대 카테고리 그리드, 모달·시트·`SearchView` 오버레이 | 공식 홈페이지 비디오 배너 에셋 연동 |
| `/components` (`Overview` + 40개 컴포넌트 문서군) | 147 | **구현(개요 41종) / 부분(세부 서브탭)** | `doc_page 0..40` 전 컴포넌트 라이브 인터랙티브 데모 + `Overview`/`Specs`/`Guidelines`/`Accessibility` 4탭 전환 및 핵심 토큰·접근성 요약 | 컴포넌트별 `/specs` 전체 토큰 매트릭스 표(100+ 행) 및 `/guidelines` 개별 Do/Don't 일러스트 아티클 분리 |
| `/styles` (`Color`, `Typography`, `Shape`, `Elevation`, `Icons`, `Motion`) | 28 | **부분** | 상위 `Styles` 섹션 내 M3 색상 역할 팔레트, 타입스케일, 코너 반경, 모션·이징 라이브 데모 구현 | 26개 하위 개별 문서(`/styles/color/system/*`, `/styles/motion/easing-and-duration/*` 등) 독립 문서 라우트 |
| `/foundations` (`Layout`, `Adaptive`, `Accessibility`, `Interaction`) | 34 | **부분 / 미구현** | 상위 `Foundations` 섹션 요약 및 실제 반응형 Window Size Class(`Compact`/`Medium`/`Expanded`) 동작 구현 | 32개 세부 아티클(`/foundations/layout/applying-layout/*`, `/foundations/accessible-design/*` 등) 장문 가이드 본문 |
| `/develop` (`Android`, `Flutter`, `Web`, `KRui`) | 6 | **부분** | 상위 `Develop` 섹션 요약 및 KRui 선언형 `.krui` + 순수 CSS 통합 안내 구현 | 플랫폼별 하위 설치·마이그레이션 상세 가이드 페이지 |
| `/get-started` 및 `/m3/*` 온보딩 문서 | 47 | **부분 / 미구현** | `Get started` 랜딩 워크스페이스 및 빠른 탐색 구현 | `/m3/pages/*` 45개 릴리즈 노트·온보딩 읽을거리 개별 문서 |
| `/blog` 및 개별 포스트 | 123 | **미구현 (랜딩만 부분)** | `Blog` 섹션 랜딩 카드 요약만 제공 | 120개 개별 블로그 아티클(`/blog/google-io-2025-m3-expressive` 등) 본문 |

---

## 7. 미해결 항목, 엔진 백로그 및 `apps/` 읽기 전용 감사 결과

### 7.1 `KNOWN_GAPS.toml` 추적 미해결 CSS 엔진 항목 (3건 / 229건 중 1.3%)
1. **`@container` 및 컨테이너 쿼리 단위(`cqw`, `cqi`)** (`tests/fixtures/real_css/KNOWN_GAPS.toml`: `container_query`, 2개 체크):
   - **사유**: Taffy 레이아웃 패스 이후 조상 `container-type: inline-size` 박스의 확정 폭을 기준으로 하위 서브트리 스타일을 재평가하는 2-패스 컨테이너 쿼리 엔진이 아직 구현되지 않아 비차단 경고 후 선언을 건너뛴다.
2. **요소 서브트리 CSS `filter`(`blur()`, `invert()`, `grayscale()` 등)** (`KNOWN_GAPS.toml`: `css_filter`, 1개 체크) 및 **복합 `clip-path: polygon(...)`**:
   - **사유**: `backdrop-filter`는 구현되어 있으나 일반 요소 서브트리 `filter` 체인과 임의 다각형 스텐실 `clip-path`는 오프스크린 렌더 타깃/스텐실 패스 확장이 필요하여 현행 W3C 선언 무시(drop) + `@supports (filter: invert(1)) == false`로 정직하게 처리한다.

### 7.2 `G1` 진행 중 발견한 엔진 백로그 (우회 없이 기록)
- **외부 `<link rel="stylesheet" href="...">` 시트 내부의 중첩 `@import` 경로 미해석 (`crates/compiler/src/lcss_ast_walk.rs:395`)**:
  - **현상 및 원인**: `.krui` 문서에서 `<link rel="stylesheet" href="component_gallery.css" />`로 로드한 외부 시트 안에서 다시 `@import "styles/rail.css";`를 선언하면, `CssSourceMap`의 `origin.file`이 `None`으로 전달되어 `lcss_ast_walk.rs:395`(`let Some(parent_dir) = origin.file.as_ref().and_then(|p| p.parent())`)가 비차단 경고를 남기고 중첩 시트를 건너뛴다.
  - **최소 재현 코드**:
    ```html
    <!-- app.krui -->
    <link rel="stylesheet" href="outer.css" />
    <div class="from-inner">텍스트</div>
    ```
    ```css
    /* outer.css */
    @import "inner.css";
    ```
  - **조치**: `G1` 단계 엔진 수정 금지 원칙에 따라 엔진을 건드리지 않고 `component_gallery.krui`에서 표준 `<link rel="stylesheet" href="styles/*.css" />`로 명시 로드했으며, `load_external_stylesheets`에서 외부 `.css` 파일 경로를 `CssSourceMap` `origin.file`에 주입하는 작업을 다음 엔진 웨이브 백로그로 기록한다.

### 7.3 `apps/**` 읽기 전용 `syn` AST 감사 보고
- `apps/**` 수정 금지 규칙을 준수하여 한 줄도 수정하지 않았으며, `tests/probe_and_bench_gates.rs`의 `PureCssAstGate`가 읽기 전용으로 스캔한 결과 `apps/teaching-tool-fluent2/src/studio_vm/theme_palette.rs`(`ThemePreset::Fluent`, `apply_theme_preset`, `set_app_tokens`, `Color::rgb` 등 28건 — `CanvasDraw::new` 내부 `scene_painter.rs` 제외)가 보고되었다. 추후 `apps/teaching-tool-fluent2` 작업 시 `[data-theme]` 순수 CSS 토큰 시트로 이전할 수 있다.

---

## 8. 최종 게이트 검증 요약

단독 실행한 `.agents/skills/krui-harness2-loop/scripts/probe_krui.ps1` 전체 결과:
- `cargo fmt --check`: **PASS**
- `cargo clippy --workspace --all-targets -- -D warnings` (기본 및 `--features preview`): **PASS**
- `cargo test --workspace` (기본 및 `--features preview`): **PASS**
- `cargo tree` 릴리즈 그래프 검사 (`krui-compiler`, `krui-preview`, `cssparser`, `lightningcss` 미포함): **PASS**
- 통제된 릴리즈 성능 벤치마크 (`krui-bench`, 워밍업 30 + 측정 300프레임, 예산 `<= 8,333us`):
  - `TinySkia`: `p50 = 3,129us`, `p99 = 6,672us`, `anim_relayouts = 0` (**PASS**)
  - `Wgpu`: `p50 = 839us`, `p99 = 1,918us`, `anim_relayouts = 0` (**PASS**)
- 쇼케이스 한국어 IME + 120프레임 애니메이션 `0` relayout + 라우터 전환 누수 `0`: **PASS**
- 전 예제(`00_hello_counter` ~ `09_tailwind_v4`) `--headless` 120프레임 실행: **10 / 10 PASS**
- `tests/real_css_conformance.rs`: **226 / 229 (98.7%)**, 회귀 `0건`, 미정리 갭 `0건` (**PASS**)
