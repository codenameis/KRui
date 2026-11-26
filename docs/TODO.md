# KRui 앞으로 할 일 (현행 로드맵)

> 규칙의 원본은 [`../AGENTS.md`](../AGENTS.md), 아키텍처 개요는 [`../ARCHITECTURE.md`](../ARCHITECTURE.md), 빌드·피처 옵션은 [`BUILD_OPTIONS.md`](BUILD_OPTIONS.md)를 따릅니다.
> 기준 상태(`2026-11-25`): 19개 워크스페이스 크레이트, `krui-stylo-bridge`(`stylo 0.22.0` + `stylo_taffy` + `taffy 0.15.1`), `@tailwindcss/cli@4.3.5` 일치율 **229/229 (100.0%)**, M3 47종 기본 컴포넌트 및 `examples/component_gallery` 헤드리스 게이트 통과.

---

## Wave 1. 즉시 수정 & 문서/환경 정리 *(진행 중)*

- [x] VS Code Live Preview `0x0B` 프로토콜 크래시(`OP_CMD_SET_COLOR_SCHEME`), `views/icons/*.krui` 루트 추론, 비밀번호 붙여넣기, prebuilt `krui` 실행, JS↔Rust opcode 일치 테스트 추가
- [x] `README.md`, `ARCHITECTURE.md`, `CHANGELOG.md`, `docs/TODO.md`, `docs/HANDOFF.md`, `docs/BUILD_OPTIONS.md`, `.agents/skills/krui-harness2-loop/SKILL.md` 정합화
- [ ] 로컬 미커밋 작업물을 논리 단위 체크포인트 커밋으로 분리 정리
- [ ] 외부 `<link rel="stylesheet" href="...">` 시트 내부 중첩 `@import` 상대 경로 해석 ([`css-engine-and-m3-gallery-oneshot.md`](css-engine-and-m3-gallery-oneshot.md#72-g1-진행-중-발견한-엔진-백로그-우회-없이-기록))

---

## Wave 2. 2D `<canvas>` 확장 & 뷰포트 인터랙션 *(Fabric.js급 드로잉 툴 공통 기반)*

- [x] **`CanvasContext2D` 확장** ([`crates/core/src/canvas_2d.rs`](../crates/core/src/canvas_2d.rs))
  - 변환 스택 & 클리핑: `save()`, `restore()`, `translate()`, `rotate()`, `scale()`, `set_transform()`, `clip_path()`
  - 선/채우기 스타일: `set_line_dash()`, `global_alpha`, 캔버스 내부 선형·방사형 그라데이션
  - 복합 드로잉: `draw_text(...)`(캔버스 텍스트·3D 기즈모 축 라벨), `draw_image(...)`(래스터/스프라이트)
- [x] **`.krui` 포인터 · 휠 · 우클릭 이벤트 페이로드 & 커서**
  - `onpointerdown` / `onpointermove` / `onpointerup`에 로컬 `(x, y)` 좌표, 마우스 버튼(좌/중/우), 수정키(`Shift`/`Ctrl`/`Alt`) 전달
  - `onwheel`(캔버스·3D 줌), `oncontextmenu`(우클릭 메뉴), **포인터 락 & 상대 델타**(3D Fly 카메라)
  - 동적 CSS `cursor` (`crosshair`, `grab`/`grabbing`, `move`, `nwse-resize` 등)
- [ ] **Fabric.js 스타일 2D 캔버스 씬 객체 레이어 *(선택형 라이브러리/예제)***
  - 도형 선택/이동/회전/스케일 핸들, 마키(marquee) 다중 선택, 스냅 가이드, `Undo/Redo`

---

## Wave 3. Raw `wgpu` 오리지널 모드 · Bevy 엔진 통합 · 3D 게임 엔진 에디터 UI

- [x] **Raw `wgpu` 오리지널 모드**: 별도 게임 엔진 래퍼 없이 KRui 앱 내부에서 `wgpu::Device` / `Queue` / 커스텀 `RenderPass`를 직접 실행하고 `ExternalTextureId` 뷰포트 슬롯에 제로카피 출력 (리사이즈/MSAA/깊이 버퍼 포함)
- [x] **Bevy Engine 통합 (`krui-bevy`, 선택형 어댑터 크레이트)**
  - `wgpu` 메이저 버전 정렬을 전제로 `krui-core` 오염 없이 독립 크레이트로 제공
  - 모드 A(에디터 호스트): Bevy `App`/`World` 3D 씬 오프스크린 `RenderTarget` → KRui `ExternalTextureId` 표시 + 양방향 ECS 인스펙터 동기화
  - 모드 B(인패스 HUD): Bevy의 `wgpu::Device`/`Queue`를 `krui-engine-host`로 공유해 3D 패스 위에 KRui UI 합성
- [x] **3D 게임 엔진 에디터 위젯 팩**
  - `SplitPane` / 도킹 레이아웃 (Hierarchy · Viewport · Inspector · Asset Browser)
  - 가상화 `TreeView`(드래그앤드롭 재부모화), 수치 스크러버(`DragFloat` / `Vec3`), `ColorPicker`, `ContextMenu`, 공용 `Undo/Redo`
  - `ExternalTexture` 위 투명 `<canvas>` 오버레이 기반 2D/3D 기즈모(Translate/Rotate/Scale) 예제

---

## Wave 4. 렌더러 표현 한계([`ENGINE_LIMITS.toml`](ENGINE_LIMITS.toml)) & 갤러리 보강

- [x] `conic-gradient()` `PaintScene`/WGSL/TinySkia 프리미티브
- [x] 다중 정지점 그라데이션 (`linear-gradient` > 3 stops, `radial-gradient` > 2 stops)
- [x] 원근 3D 변환 (`perspective` / `preserve-3d` 비아핀 투영 및 Z-정렬; 교차 BSP 분할은 비범위)
- [x] 임의 다각형/경로 `clip-path: polygon()` / `path()` 스텐실 패스
- [x] `component_gallery` M3 `/specs` 세부 토큰 매트릭스 및 `/styles`/`/foundations` 하위 문서 라우트 보강

---

## Wave 5. 실기 플랫폼 백엔드 완성 & 배포 *(후속)*

- [ ] **`krui-kms` 실기 구현**: Linux `/dev/dri/card*` DRM DumbBuffer ioctl + `evdev` 실장치 연결 및 Raspberry Pi SPI 패널 실측(`<= 200ms` warm)
- [ ] **`krui-web` (`crates/web-dom`)**: `wasm32` `web-sys` `DomBackend` 어댑터 및 `StaticDocumentDesc` 마운터
- [ ] **`krui-ios-ffi`**: Swift `UITextInput` + `CAMetalLayer` 호스트 브리지
- [ ] **`krui-android-ffi` 실기 검증**: Android / Wear OS 실기기 IME 및 `krui serve --android` 확인
- [ ] **데스크탑/배포**: 멀티윈도우 드래그앤드롭, 네이티브 파일 다이얼로그·트레이, 원격 `http(s)://` `AssetLoader` 구현 예제, `vscode-krui` `.vsix` 패키징
