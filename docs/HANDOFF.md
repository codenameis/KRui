# 인수인계 (HANDOFF)

> 구현자가 **교대를 끝낼 때마다 이 파일을 덮어씁니다.** 이력은 git log가 가집니다.

## 현재 상태 (마지막 갱신: 2026-11-15, 전 크레이트 적대적 리뷰 및 KRui 2.0 하드닝 완료)

- 기준 베이스 커밋: `b10bfcf` (+ KRui 2.0 수직 슬라이스 및 적대적 리뷰 전체 수정 반영)
- 진행 중(lock): 없음
- 다음 교대가 할 일: iOS `UITextInput` / `CAMetalLayer` 호스트 브리지(`krui-ios-ffi`) 확장 및 멀티윈도우 드래그앤드롭 시나리오 추가

## 마지막 교대 요약

| 영역 | 주요 결함 수정 및 하드닝 내용 |
|---|---|
| `krui-signals` & `krui-patterns` | 12비트 세션 태그(`ThreadSessionTable`) + 세션별 `SessionWakerState` 격리로 `Signal<T>` 8바이트 `Copy`를 유지하면서 다중 창 `ui_tasks` 간섭, `reset_in_place` 후 ABA 세대 충돌, `enter` 외부 `Scope::dispose`/`Command::execute`/`Store::dispatch` 오동작을 근절. `Drop for ReactiveRuntime` 시 `tokio` 태스크 즉시 취소 및 패닉 복원 RAII 가드 적용. |
| `krui-core`, `krui-style`, `krui-text`, `krui-a11y` | `<a href>` 클릭/`Enter`/AccessKit 활성화 콜백(`on_link_activate`) 연결, 동적 `<img src>` 백그라운드 `spawn_blocking` 디코딩(UI 스레드 블로킹 0) 및 `16384x16384`/`256MiB` 디컴프레션 폭탄 방어, `ObjectFit::None` 전 경로 지원, `<canvas>` 콜백 중 `ViewTree` 재진입 패닉 해소, 둥근 부모 내부 직각 자식 클리핑 복원, `set_app_tokens` 즉시 재해석. |
| `krui-compiler` & `krui-macros` | `:root { --krui-*: ... }` 사용자 테마 토큰 오버라이드 반영, 잘못된 토큰/단위/`NaN`/`inf` 조용한 무시 제거(`CompileError`), `style_tokens.rs` 118개 `TokenId` 전수 매핑, 중첩 self-closing `<svg/>` 깊이 추적 및 멀티바이트 UTF-8 컬럼 인덱스 보정. |
| `krui-renderer`, `krui-preview`, `krui-winit` | GPU 이미지 업로드 시 Premultiplied Alpha 변환·최대 텍스처 크기 자동 다운스케일·`128MiB` LRU 캐시 적용, 셰이더 half-texel UV 클램핑, TinySkia `64MiB` 이미지 캐시 및 서브픽셀 도형 정합성 확보, 오프스크린 레이어 풀 깊이 상한(`16`), `PreviewTcpClient` 백그라운드 블로킹 리스너 기반 유휴 `0%` CPU 달성, HiDPI `"KRFR"` 실제 프레임버퍼 크기 일치. |
| `krui-android-ffi`, `krui-cli`, `vscode-krui` | Android JNI 5심볼 전 경계 `catch_unwind` + null/길이 검증(`unsafe {}` 블록 0줄 유지), `ViewTree::set_scale_factor` 전파, `KruiView` `SurfaceHolder` 실제 `ARGB_8888` 비트맵 블리팅 및 멀티터치 `ACTION_MOVE`/입력 종류별 `restartInput` 지원. `krui serve --panel` 다중 프레임 버퍼링/EOF 유실 수정, 비동기 백그라운드 리빌드, `extension.js` 모듈 분리(500줄/60줄 한도 준수) 및 웹뷰 붙여넣기·조합 보존 수정. |

## 사용법 요약 (실기기 / VS Code 라이브 미리보기)

- 데스크탑 라이브 미리보기: `cargo run -p krui-cli -- serve --example 02_mvvm_pattern`
- IDE 패널 브리지 단독 실행: `cargo run -p krui-cli -- serve --panel --watch examples/02_mvvm_pattern/views/mvvm_store.krui`
- VS Code 확장 실행: `--extensionDevelopmentPath=editors/vscode-krui` 로 VS Code 실행 후 `.krui` 파일에서 `Ctrl+Shift+V` (`KRui: Open Live Preview`) 실행

## 다음 교대 시작 체크리스트

- [x] `#![forbid(unsafe_code)]` 14개 크레이트 + `krui-android-ffi` `0` `unsafe {}` 블록 / `5` `#[no_mangle]` 유지
- [x] `cargo fmt --all` 후 모든 소스 파일 500줄 이하·함수 60줄 이하 유지
- [x] `probe_krui.ps1` 전 게이트 통과 확인
