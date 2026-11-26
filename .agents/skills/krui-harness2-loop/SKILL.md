---
name: krui-harness2-loop
description: >-
  Executes the Harness2 Propose-Probe-Compose verification and refinement loop for KRui.
  Use this skill whenever refactoring KRui crates, fixing layout/transition bugs,
  modifying the HTML/CSS compiler, or verifying 120fps frame-budget and architectural invariants.
---

# KRui Harness² (Propose–Probe–Compose) Verification Skill

이 스킬은 `AGENTS.md`(2026-11-25 현행판)의 **Propose–Probe–Compose** 작업 루프를 실행합니다.

## 언제 사용하는가?
- KRui 워크스페이스 19개 크레이트(`signals`, `style`, `stylo-bridge`, `compiler`, `macros`, `layout`, `text`, `a11y`, `core`, `patterns`, `renderer`, `winit`, `kms`, `engine-host`, `web-dom`, `preview`, `cli`, `android-ffi`, `krui`)를 수정·리팩토링할 때
- IME(한국어 조합, 조합 중 포커스 이동, 일본어 변환, 영어, AltGr), 화면 전환(`Scope` 및 `Effect` 해제), 레이아웃(Taffy 텍스트 측정), `Affine2` 행렬 히트테스트를 검증할 때
- 릴리즈 프레임 예산(`krui-bench`) 및 `component_gallery` 헤드리스 다중 프레임 실행을 점검할 때

## 실행 단계 (Propose -> Probe -> Compose)

### Step 1: Propose (변경 후보와 데이터 흐름 수립)
1. 바꿀 크레이트, **수정 예정 파일 목록(영향 반경 확인: 비-아키텍처 수정 `1~3`개 파일 원칙, `>= 5`개 파일 또는 크레이트 간 IR/API 변경은 사전 합의)**, 그리고 데이터 흐름(`DSL -> IR / static_ir -> codegen/interpreter -> ViewTree -> Stylo Bridge -> Layout -> PaintScene -> Renderer`)을 먼저 적습니다.
2. `AGENTS.md`의 절대 규칙을 확인합니다:
   - `#![forbid(unsafe_code)]` 준수 (`android-ffi`, `stylo-bridge`만 `#![deny(unsafe_code)]` + 직접 `unsafe {}` 0줄)
   - 릴리즈 빌드에 파서(`krui-compiler`) 런타임 의존성 배제 (`cargo tree -p krui -e normal,no-proc-macro -i krui-compiler`)
   - 조용한 무시 금지 (HTML/바인딩 오류는 `CompileError`, 미지원 CSS 선언은 W3C 선언 drop + `Diagnostic`)
   - 매크로 파일 쓰기 금지 및 `include_bytes!` 추적
   - Fast-Path 불변식 (`transform`/`opacity` 변경 시 relayout 0회, 텍스트/크기 변경 시 relayout 정확히 1회)
   - 수명 관리 (100회 화면 전환 후 모든 런타임/Taffy/노드 카운트 `+0` 일치)
   - 순수 CSS가 정답 (`krui-stylo-bridge` 단일 CSS 계층; `core`/`renderer`에 `Color::rgb(` 하드코딩 및 RGB 휴리스틱 금지, `[data-theme]` 순수 CSS로만 테마 전환)
   - IME `ime_target` 고정 및 채널 기반 중복 제거
   - 작성 목표는 파일 500줄·함수 60줄 이하, 게이트 하드 완충선은 파일 560줄·함수 80줄 이하 (국소 버그 수정 중 억지 파일/함수 분할 금지)

### Step 2: Probe (자동화 검증 스크립트 실행)
코드 수정 후 아래 Probe 스크립트를 실행합니다:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .agents/skills/krui-harness2-loop/scripts/probe_krui.ps1
```

`probe_krui.ps1`은 다음을 순서대로 검증합니다:
1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings` (기본 feature 및 `--features preview` 모두)
3. `cargo test --workspace` (기본 feature 및 `--features preview` 모두)
4. `crates/core`, `crates/renderer` 내 `Color::rgb(` 리터럴 0건 검사
5. `cargo tree -p krui -e normal,no-proc-macro -i krui-compiler`로 릴리즈 의존성 그래프에 `krui-compiler`가 없는지 검사
6. 통제된 release 벤치마크(`cargo run --release --bin krui-bench`: 30 워밍업 + 300 측정 프레임, TinySkia·Wgpu 각각 웜/콜드 첫 프레임 및 p50 <= 8,333us, p99 <= 8,333us 검증)
7. 쇼케이스 및 전 예제 `--headless` 120프레임 + 입력 시뮬레이션 실행

### Step 3: Compose (합성 및 회귀 방지)
1. 통과한 변경만 커밋하고, 발견한 엣지 케이스는 회귀 테스트로 고정합니다.
