# 인수인계 (HANDOFF)

> 구현자가 **교대를 끝낼 때마다 이 파일을 덮어씁니다.** 상세 로드맵은 [`TODO.md`](TODO.md), 규칙의 원본은 [`../AGENTS.md`](../AGENTS.md)를 참조하세요.

## 현재 상태 (마지막 갱신: 2026-11-25)

- **기준 상태**: 19개 워크스페이스 크레이트 (`17`개 `#![forbid(unsafe_code)]`, `2`개 `#![deny(unsafe_code)]` — 직접 작성 `unsafe {}` 0줄)
- **CSS 적합성**: `krui-stylo-bridge` (`stylo 0.22.0` + `stylo_taffy` + `taffy 0.15.1`), `@tailwindcss/cli@4.3.5` 일치율 **229/229 (100.0%)**
- **진행 중(lock)**: 없음
- **다음 작업 방향**: [`TODO.md`](TODO.md) 기준 Wave 2 (2D `<canvas>` 확장 + 포인터/휠/우클릭 페이로드) → Wave 3 (Raw `wgpu` + `krui-bevy` + 3D 에디터 위젯) → Wave 4 (`ENGINE_LIMITS.toml` 백로그)

## 최근 완료 요약 (`2026-11-25`)

| 영역 | 주요 구현 및 검증 내용 |
|---|---|
| 순수 CSS / `krui-stylo-bridge` | `ThemePreset`·`TokenId`·`TokenTable`·`lightningcss` 완전 제거 후 Stylo 0.22 단일 계층으로 통합, `@layer preset, component;` 캐스케이드 순서 정규화, `@property`/`light-dark()`/`@container`/CSS `filter`/`:has()` 229/229 100.0% 달성. |
| Material 3 갤러리 & 내장 위젯 | `examples/component_gallery` 47종 기본 컴포넌트(77개 SVG 아이콘 포함) 동작 복구(Switch 썸, Slider, Radio, Modal NavigationDrawer, `<progress>` 비율 바, `arboard` 실 클립보드 JSON 복사), `[data-open]` + `.krui-ui-open-true` 0-relayout 오버레이 모션 및 Windows 릴리즈 GUI 서브시스템(`windows_subsystem = "windows"`) 적용. |
| v3.1 티어 공존 & 입력 하드닝 | `krui-core::static_ir` 정적 IR, `Hangul2BulsikAutomaton` + OSK, `krui-kms`(DRM/evdev 플래너), `krui-engine-host`(공유 `wgpu::Device` 120FPS 인패스), `krui-web`(`DomBackend`) 추가. 닫힌/퇴장 중 `pointer-events: none` 오버레이의 스크롤/클릭 통과 및 `Tab`/AccessKit 포커스 격리. |
| VS Code 라이브 미리보기 | `editors/vscode-krui` · `krui-cli` · `krui-winit`의 `0x0B` 옵코드를 순수 CSS `OP_CMD_SET_COLOR_SCHEME` (`system`/`light`/`dark`)로 통일, `views/icons/*.krui` 루트 추론, 비밀번호 붙여넣기, JS↔Rust 프로토콜 패리티 테스트 추가. |

## 사용법 요약 (빌드 옵션은 [`BUILD_OPTIONS.md`](BUILD_OPTIONS.md) 참고)

- 컴포넌트 갤러리 실행/검증: `cargo run --release --example component_gallery -- --headless`
- 데스크탑 라이브 미리보기: `cargo run -p krui-cli -- serve --example component_gallery`
- IDE 패널 브리지 단독 실행: `cargo run -p krui-cli -- serve --panel --watch examples/component_gallery/views/component_gallery.krui`
- VS Code 확장 개발 실행: `code --extensionDevelopmentPath=editors/vscode-krui` 후 `.krui`/`.css` 편집기에서 `Ctrl+Shift+V` (`KRui: Open Live Preview`)

## 체크리스트

- [x] `#![forbid(unsafe_code)]` 17개 크레이트 + `krui-android-ffi`/`krui-stylo-bridge` `0` `unsafe {}` 블록 유지
- [x] `cargo tree` 릴리즈 파서(`krui-compiler`, `krui-preview`, `lightningcss`) 미포함
- [x] `probe_krui.ps1` 5단계 전 게이트 통과 확인
