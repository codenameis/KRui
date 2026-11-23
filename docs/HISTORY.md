# KRui 프로젝트 히스토리와 컨셉

> 개발 환경을 옮길 때 맥락을 잃지 않기 위한 문서입니다. **앞으로 할 일은 [`TODO.md`](TODO.md)** 에 따로 있습니다.
> 작성일: 2026-10-26 / 기준 커밋: `a76d8f8` (A7). 규칙의 원본은 저장소 루트의 [`AGENTS.md`](../AGENTS.md)입니다. 이 문서와 충돌하면 `AGENTS.md`가 우선합니다.

---

## 1. 컨셉 (왜 만드는가)

KRui는 **HTML/CSS 문법을 닮은 선언형 DSL(`.krui`)로 UI를 작성하는 순수 Rust 네이티브 GUI 프레임워크**입니다.

- **웹뷰가 아닙니다.** 모든 픽셀을 자체 렌더러(`wgpu` GPU, `tiny-skia` CPU)로 그립니다.
- **벤치마크는 Slint**입니다. 다만 웹 개발자와 Rust 입문자가 바로 읽고 고칠 수 있는 것을 더 우선합니다.
- **"HTML을 파싱하면 느리지 않나?"에 대한 답:** 릴리즈 빌드에는 파서가 없습니다. `krui!`/`include_krui!` 매크로가 컴파일 타임에 Rust 코드로 변환합니다. 파싱은 개발 중 미리보기(`preview` feature)에서 파일을 저장할 때 한 번만 일어납니다. 웹이 무거운 이유는 HTML 파싱이 아니라 실행 중의 CSS 매칭, DOM diff, 연쇄 reflow인데, KRui는 이를 컴파일 타임 계산과 시그널 직접 갱신으로 대체합니다.

### 제품 목표 (우선순위 순)

| 순위 | 목표 |
|---|---|
| 1 | **IME 최우선**: 한국어, 일본어, 영어 입력이 OS 네이티브 수준(조합, 변환, 후보창 위치) |
| 2 | **낮은 진입 장벽**: `.krui`는 HTML/CSS를 아는 사람이 5분 안에 읽고, Rust는 문법을 막 뗀 사람도 수정 가능 |
| 3 | **라이브 미리보기**: 저장하면 앱 상태를 유지한 채 즉시 반영. Dioxus/Flutter처럼 `krui serve`로 데스크탑, 디바이스 프레임, Android 에뮬레이터/iOS 시뮬레이터에 핫리로드 |
| 4 | **어디서나 동작**: Raspberry Pi(저사양) → 모바일 → 데스크탑. 게임 엔진·VR에 임베드 |
| 5 | **고성능**: 정적 UI는 유휴 0% CPU, 애니메이션은 transform/opacity만으로 레이아웃 없이 동작 |
| 6 | **디자인 시스템 4종**: Fluent, Material, Liquid Glass, Cosmic(저사양: 블러·그림자 없음) |
| 7 | **패턴 4종**: MVVM, MVC, MVI, Flux/Elm. 코어는 패턴 중립, 패턴은 얇은 라이브러리 |

### 핵심 설계 결정 (이유 포함)

| 결정 | 이유 |
|---|---|
| **DSL 제어 흐름은 HTML 태그 형태**: `<if test="{..}">/<else-if>/<else>`, `<for each="{item in list}" key="{..}">` + `<empty>` | HTML을 아는 사람에게 익숙해야 함. `{#if}` 같은 템플릿 기호는 쓰지 않음 |
| **속성값은 항상 따옴표**, 이벤트는 HTML 이름(`onclick`, `oninput`), `on:click`은 별칭 | HTML 호환성 |
| **`<for>`의 `key`는 필수** | 목록 갱신 성능과 입력 포커스·IME 상태 보존 |
| **`<if>`/`<for>`는 레이아웃 박스를 만들지 않음** | 자식이 부모에 직접 배치되어야 CSS가 예측 가능 |
| **템플릿 식 `{..}`은 작은 식 언어로 제한** (경로, 리터럴, 연산자, 등록된 메서드 호출, enum 경로) | 매크로(릴리즈)와 미리보기 인터프리터가 **같게** 해석해야 하고, 인터프리터는 Rust 코드를 실행할 수 없기 때문. 복잡한 로직은 ViewModel의 `Computed`/메서드로 |
| **`#[view_model]`/Store/Controller가 "이름 → Signal/Command" 레지스트리를 생성** (preview feature에서만) | 인터프리터가 이름으로 실제 상태에 바인딩 → 앱 상태를 유지한 채 핫리로드 |
| **IR(`CompiledDocument`)은 직렬화 가능(serde)** | `krui serve`가 네트워크로 모바일에 IR을 보내기 위해 |
| **테마는 CSS 변수(토큰) 교체로만 전환**. core/렌더러는 색상 값을 보고 용도를 추측하지 않음 | "CSS가 정답". 한 번 RGB 휴리스틱이 들어왔다가 규칙 퇴행으로 판정되어 삭제함 |
| **조합(composing)은 OS가 소유**, IME 이벤트는 조합을 시작한 노드(`ime_target`)로 끝까지 전달 | Windows 한국어 IME는 포커스 변경 시 Commit을 보냄. 현재 포커스 노드로 보내면 글자가 사라짐 |
| **core는 플랫폼 중립 `EditingState { text, selection, composing }` + `TextInputHost` 트레이트** | Flutter의 `TextEditingValue` 구조를 참고. 데스크탑(winit)/Android/iOS/임베드 호스트가 같은 상태 기계 사용 |
| **`unsafe` 최소화**: 기본 금지, 예외는 `krui-*-sys`/`krui-*-ffi` 크레이트만(직접 작성 10줄 이내, `// SAFETY:`, README 표에 기록) | 모바일 IME는 JNI/ObjC 연결 때문에 불가피할 수 있으나 격리 |
| **모바일 IME**: Android는 Kotlin `InputConnection` shim(Flutter·Slint 방식), iOS는 Swift `UITextInput` 뷰. Rust 연결은 UniFFI(직접 unsafe 0줄) 또는 `jni` pull 방식(약 2줄) | winit은 Android에서 IME 조합 이벤트를 안 보내고 iOS는 marked text 미지원(0.30 기준, 소스로 재확인 필요) |
| **Flux/Elm/MVI는 `Store<State, Msg>` 하나로 통합**: `update(&mut State, Msg) -> Effects<Msg>`. MVI는 예제로 보여줌 | 코어는 패턴 중립, 중복 구현 방지 |
| **성능 수치는 통제된 측정만** (release, 워밍업 후 N회, 중앙값/p99, 다른 작업 동시 실행 금지) | 동시 작업 중 측정한 값이 5배 차이 난 경험 |
| **"완료" 보고에는 그 기능이 망가지면 함께 실패하는 테스트 필수** | 지난 M0에서 "완료"로 표시되었지만 동작하지 않는 기능이 여럿 있었음 |

### 참고한 외부 설계

- **Flutter**: 핫리로드(VM Service + 증분 컴파일, Key+위치로 상태 보존, reload/restart 구분), IME(OS 네이티브 텍스트 입력 시스템에 연결만 하고 `TextEditingValue`를 양방향 동기화, 캐럿 사각형과 변환 행렬을 OS에 전달).
- **Dioxus**: `dx serve --platform android`. 앱을 실제로 빌드해 에뮬레이터에 설치하고, 템플릿 diff를 WebSocket으로 보내 상태 유지한 채 반영. Rust 로직 변경은 재빌드(또는 subsecond 바이너리 패치).
- **Slint**: 템플릿 식을 자체 언어로 제한, `.slint`를 빌드 시점에 변환.

---

## 2. 현재 아키텍처

```
.krui / .css
   │  (compile time: krui!/include_krui! 매크로 — 같은 IR을 preview 인터프리터도 사용)
   ▼
krui-compiler (IR: CompiledDocument)  ──►  krui-macros (Rust 코드 생성)
   │
   ▼
krui-core: ViewTree(노드 slab, generational NodeId) ─ 바인딩(Signal/Effect, Scope 소유)
           ├─ 입력/IME 상태 기계, 텍스트 편집(grapheme), 히트테스트
           ├─ layout (Taffy 래퍼, TextContext measure)  ─ text (cosmic-text)
           └─ PaintScene (Affine2 행렬, clip)
   ▼
krui-renderer: wgpu / tiny-skia   ◄── krui-winit (창, 이벤트 루프, IME, 클립보드)
```

| 크레이트 | 역할 |
|---|---|
| `signals` | Signal/Computed/Effect/batch, `Scope` 소유·해제, generational ID, `FrameClock`(spring/shake/nudge) |
| `style` | `CompiledStyle`(layout/paint/text 분리), 토큰 테이블, `Affine2` |
| `compiler` | `.krui` HTML/CSS 파서, 캐스케이드(특이도), 상태 가상 클래스, `<if>/<for>`, 진단(줄·열) |
| `macros` | `krui!`, `include_krui!`, `#[view_model]` |
| `layout` | Taffy 래퍼 (`TaffyTree<TextContext>`, 합성 루트) |
| `text` | cosmic-text 셰이핑, LRU 캐시, 한글/CJK 폴백 |
| `renderer` | wgpu + tiny-skia, R8 글리프 아틀라스, 클리핑 |
| `a11y` | accesskit 트리(OS 어댑터는 미연결) |
| `core` | ViewTree, 입력, IME, 페인트, 히트테스트 (**winit/wgpu 의존 없음**) |
| `winit` | 데스크탑 호스트, `preview` 모듈(feature) |
| `patterns` | `Command`(+async, `is_running`), `Store`, `Effects` (MVVM/MVC/Store) |
| 루트 `krui` | 56줄 퍼사드 + 쇼케이스 바이너리 |

예제: `01_business_studio`, `02_mvvm_pattern`, `03_mvc_pattern`, `04_elm_flux_pattern`(단방향), `05_mvi_pattern`.
테스트: `ime_scenarios`, `m0_dsl_and_selectors`, `m0_integration_and_budget`, `m0_layout_and_affine`, `transition_stress` + 각 크레이트 단위 테스트.
검증 스크립트: `.agents/skills/krui-harness2-loop/scripts/probe_krui.ps1`.

---

## 3. 작업 타임라인

저장소 커밋은 총 21개입니다. 코드 작업은 **지시서를 내가 쓰고 다른 에이전트가 실행**하는 방식으로 진행했습니다(리뷰·지시서 작성 에이전트는 코드를 직접 수정하지 않음).

### 3-1. 1차 리뷰 (M0 시작 전)
`baseline: pre-M0 review state`(`06c7e23`) 시점의 코드를 4갈래로 리뷰했습니다. 주요 발견:

- **가드레일 위반**: 매크로가 `target/release/*.enc`에 파일을 씀, `include_str!` 미생성(수정해도 재컴파일 안 됨), powershell 클립보드(UI 블로킹), `_ => {}`로 모르는 속성 무시, 프록 매크로 패닉(`{frame:x}`, 비ASCII).
- **릴리즈에 파서 포함**: 핫리로드 모듈이 feature로 막혀 있지 않음.
- **Effect 해제 없음**: 화면 전환 시 이전 페이지 바인딩이 새 노드를 덮어쓰거나 패닉, `FrameClock` 콜백 누적.
- **텍스트 변경 시 레이아웃 미갱신**(측정값을 스타일에 영구 기록), 루트가 여러 개면 첫 루트만 레이아웃, Transform이 창 원점 기준.
- **"Compositor Fast-Path"는 존재하지 않음**: 매 프레임 씬 전체를 재생성. 노드 45개에 프레임당 3.7ms.
- HTML 기능 부족(`if`/`for` 없음, `:hover` 등 상태 미지원, `button.primary` 셀렉터가 조용히 무시, `placeholder`를 실제 값으로 처리).
- 가짜 "암호화"(하드코딩 키 XOR).

### 3-2. M0 (지시서 `m0_fix_prompt.md`, 커밋 `5d73e93`~`5b8bc54`)

| 단계 | 내용 |
|---|---|
| P0-1 | 가드레일 위반 제거, 엄격한 속성 검증, 줄·열 오프셋 |
| P0-2 | `preview` feature 분리(컴파일러를 optional로) |
| P0-3 | `src/lib.rs` 분할 → `krui-core`/`krui-winit`/`krui-patterns` |
| P0-4 | `Scope` 해제, generational ID, `transition_stress` |
| P0-5 | 한/일/영 IME, grapheme 커서, DPI |
| P0-6 | `TaffyTree<TextContext>` 측정, 합성 루트, `TextStyle` 분리 |
| P0-7 | `Affine2` 중심 원점 행렬 통합, 역행렬 히트테스트 |
| P0-8 | 패턴 레이어 통합(`Effects<Msg>`, async `Command`, `05_mvi_pattern`) |
| P0-9 | `<if>/<for>`, 복합 셀렉터, 상태 가상 클래스, CSS 변수/색상 |
| P1 | wgpu 버퍼 재사용, 아틀라스, 텍스트 캐시, 스크롤 |

이후 에이전트가 예제 정리 커밋 3개(`6533594`, `847fe6d`, `57f49ea`)를 추가했는데, 마지막 커밋에서 **core에 RGB 값으로 색 용도를 추측하는 휴리스틱이 다시 들어왔습니다**.

### 3-3. 2차 리뷰 (M0 결과, 문서 `m0_result_review.md`)

구조 작업(분할, generational ID, Taffy 측정, Affine2, `<if>/<for>`, 패턴 레이어)은 실제로 잘 됐고 `unsafe`는 0건이었으나, **"완료"로 표시된 것 중 가짜인 항목**이 여럿이었습니다.

| 심각도 | 문제 |
|---|---|
| 🔴 | 테마 전환 가짜 + RGB 휴리스틱 퇴행 (`node.rs resolve_effective_paint`) |
| 🔴 | 미리보기가 `<if>/<for>`를 무시(페이지가 빈 화면), 바인딩을 "태그별 전역 순번"으로 재연결 |
| 🔴 | 한글 조합 중 포커스 이동 시 글자 소실(테스트가 이 동작을 고정) |
| 🔴 | 숨은 전역 `pending_mount_scope`가 Store를 조용히 해제, 해제된 `Signal::get`이 같은 타입의 **첫 시그널 값**을 반환 |
| 🔴 | 비동기 poller 실행 중 `borrow` 유지 → 재진입 패닉 |
| 🔴 | 클리핑 가짜(scissor/mask 없음), 텍스트 캐시는 이름만 LRU(FIFO), 아틀라스는 RGBA 2048²·픽셀 해시 키·프레임 중간 초기화 |
| 🔴 | probe가 증명 못함(exe에서 문자열 검색, 프레임 시간 미검사, 헤드리스 1프레임, 하드코딩 경로) |
| 🟠 | AltGr 입력 소실, Esc가 앱 종료, 숫자 키가 테마 전환, KeyEvent.text/Commit 중복 제거가 `ll`을 삼킴, 캐럿 위치 중복 계산 |
| 🟠 | Taffy 노드 누수, 히트테스트가 slab 역순(페인트 순서와 불일치), 스크롤 최대값 매 프레임 전체 순회 |
| 🟠 | 프록 매크로 패닉(`each="{(i, x) in v}"`), `<style>` 안의 `{x}`가 CSS 파서를 깨뜨림, CSS 에러 위치가 연결된 CSS 기준, `id="row-{item.id}"`가 문자열 그대로 |
| 🟡 | wgpu `Backends::PRIMARY`(GL 제외, Pi 불가), 임베드 API(외부 wgpu 텍스처) 없음, 유휴 CPU 0% 아님, `core → patterns` 의존 방향 역전, 사용자 코드의 `Rc<dyn Fn>`, 상태 스타일 캐스케이드 순서 오류, IR 직렬화·레지스트리 없음 |

**성능 실측(통제 안 됨)**: 리뷰어 release 측정은 애니메이션 120프레임 평균 10.9ms/최대 41.6ms(예산 8.33ms 초과), 별도 debug 측정은 평균 2.0ms. 동시 작업 때문에 5배 차이가 났으므로 **신뢰할 수 없는 수치**입니다. 공통 사실은 첫 프레임이 45~95ms로 예산을 크게 넘는다는 것입니다.

### 3-4. 모바일 IME 조사 요약
(1차 출처를 직접 열지 못한 항목은 **구현 전에 소스로 재확인** 필요)

| 방법 | 우리 코드의 unsafe | 비고 |
|---|---|---|
| Android `android-activity` GameActivity + GameTextInput | 0줄 | 삼성 키보드 한글 조합 문제 보고, 후보창 위치 API 없음 → 기본값으로 쓰지 않음 |
| **Android Kotlin `InputConnection` shim + `jni` pull 방식** | 약 2줄(raw VM/Activity 포인터 변환, `*-sys`에 격리) | 추천. Flutter/Slint와 같은 방식 |
| Android Kotlin shim + UniFFI | 직접 0줄(생성 코드는 `*-ffi`에 격리) | 추천 대안 |
| **iOS Swift `UITextInput` 뷰 + UniFFI/swift-bridge** | 직접 0줄 | 추천 |
| iOS objc2 `define_class!` 직접 구현 | `unsafe(method)` 약 30개 | 비추천 |

- `#![forbid(unsafe_code)]`는 `#[no_mangle]`과 `unsafe impl`도 막으므로, Java→Rust 네이티브 콜백이 필요하면 별도 `*-ffi` 크레이트가 필요합니다.
- 데스크탑: winit 0.31의 `request_ime_update`/`DeleteSurrounding`을 검토. macOS 한글 첫 자모 `insertText` 문제 등 OS별 버그가 있으므로 **core의 IME 상태 기계에 OS별 시나리오 테스트**를 붙입니다.

### 3-5. M0.5 정정 (지시서 `m1_work_order.md` Phase A, 커밋 `7a9634f`~`a76d8f8`)

2차 리뷰 결과를 바로잡는 단계입니다.

| 단계 | 커밋 | 커밋 메시지 요약 |
|---|---|---|
| A1 | `7a9634f` | 테마 휴리스틱 삭제, 런타임 토큰 테이블 해석 |
| A2 | `7574097` | 조합 중 `ime_target` 고정, `caret_rect` 통일, 순수 `route_key` 추출 |
| A3 | `3a15f81` | Scope 소유 결함, Effect 재실행 정리, 전환 누수 0 |
| A4 | `380ec61` | 정직한 probe, release 벤치, wgpu readback lazy, DSL 엣지 테스트 |
| A5 | `935e014` | scissor/mask 클리핑, R8 글리프 아틀라스, LRU 텍스트 캐시 |
| A6 | `805f490` | 페인트 순서 기반 클리핑 히트테스트, 스크롤 캐시, Taffy 서브트리 정리 |
| A7 | `a76d8f8` | 엄격한 컴파일러 진단, CSS 출처 추적, 동적 class/aria-label 바인딩 |

### 3-6. A8 및 Phase B(`B1`~`B6`) + 독립 리뷰 후속 정정 (`B-fix-1`~`A-fix-4`)

`ed96853`까지 `A8` 및 `B1`~`B6`가 구현되었고, 2026-10-26 독립 리뷰(`docs/reviews/2026-10-26-ed96853-a1-b6-independent.md`)에서 발견된 Critical/High 결함 7개 단계를 순서대로 완결했습니다.

| 단계 | 커밋 | 핵심 내용 |
|---|---|---|
| A8 | `dc369a2` | `krui-core`/`krui-patterns` 의존 분리, `wgpu::Backends::all()`, 외부 `TextureView` 임베드 API(`06_embed_offscreen`), `TextInputHost`, 유휴 `ControlFlow::Wait` |
| B1~B6 | `db37795`~`ed96853` | 템플릿 식 AST, `#[view_model]`/`Store`/`Controller` 레지스트리, `krui-preview` 인터프리터, 구조적 경로 상태 보존, 모바일 IME FFI 브리지(`krui-ime-ffi`), `krui serve` 라이브 서버 및 디바이스 프레임 |
| B-fix-1 | `d975583` | `ime_target` empty preedit/remount 유지, programmatic focus 공통 auto-commit/교체, `EditingState`+caret 단일 대상 동기화, B5(`FinishComposing`/`DeleteSurrounding`/`SetSelection`/`EditorAction`/`InputKind`/키보드 표시) 상태 기계 완성 |
| B-fix-2 | `3f4ccf4` | `i64::MIN` checked 산술·논리 단락·타입 에러 매크로/인터프리터 동치, 미등록 Rust 메서드 호출 차단, 전체 문서 preflight 후 원자적 reload(실패 시 기존 화면·반응성 유지), 매크로↔프리뷰 동일 `StructuralPathKey` |
| B-fix-3 | `dea3f0a` | 실제 예제 `views/*.krui` 기본 경로 일치, 연결별 누적 버퍼로 TCP 분할/병합/백프레셔 처리, 워커 스레드 파일 감시+컴파일(UI 스레드 논블로킹), `.rs`/`Cargo.toml` 변경 시 자동 재빌드·재시작, 디바이스 프레임 실제 해상도·DPI·safe-area·포인터 역변환 |
| A-fix-1 | `ae9b9a2` | 완료된 ROOT poller 즉시 제거, `<if>`/`<for>` 앵커 직접 unmount 시 하위 스코프 정리, 재진입 `Signal::update` 큐잉/`try_update`, `<if>`/`<for>` 1,000회 스트레스 후 모든 런타임 카운터(tombstone 포함) 정확히 일치 |
| A-fix-2 | `5a4819c` | 0 viewport limit 방어, Lyon/glyph 정점·인덱스 한도 실제 강제, 회전/이동 클리핑 및 휠 히트테스트 페인트 순서 통일, TinySkia 물리 픽셀 프레임버퍼, IME 캐럿→페인트 스케일 일치, readback `Option` 및 스테이징 버퍼 재사용, R8 아틀라스 1024² 정책, `.notdef` 1회 경고, 고정 좌표 오프스크린 픽셀 오라클 |
| A-fix-3 | `68a9332` | 상태 가상 클래스 특이도 `(0, 1, 0)` 합산, 다중 동적 클래스 부분집합 복합 셀렉터·소스 순서 사전 계산, 인라인/외부 CSS 변수 순환·미정의 에러에 실제 파일·줄·열 연결 |
| A-fix-4 | (현재 HEAD) | `krui-bench`에서 Wgpu `device.poll(Maintain::Wait)` 대기 및 TinySkia/Wgpu 모두 `p99 <= 8,333us` 강제, `cargo tree` 양/음성 검증, 6개 예제 120프레임 게이트, 모든 `.rs` 파일 `<= 500`줄 및 모든 함수 `<= 60`줄 분할 완료 |

**통제된 release 성능 실측 (`krui-bench`, 30 워밍업 + 300 측정 프레임, Wgpu `Maintain::Wait` 포함)**:
- **TinySkia**: `cold_1st = 24,622us (24.62ms)`, `warm_1st = 8,218us (8.22ms)`, `p50 = 1,845us (1.85ms)`, `p99 = 3,799us (3.80ms)`, `max = 4,351us (4.35ms)`, `anim_relayouts = 0`
- **Wgpu (GPU 완료 대기 포함)**: `cold_1st = 18,494us (18.49ms)`, `warm_1st = 14,502us (14.50ms)`, `p50 = 453us (0.45ms)`, `p99 = 877us (0.88ms)`, `max = 896us (0.90ms)`, `anim_relayouts = 0`

---

## 4. 작업 방식과 운영 노하우

- **역할 분리**: 리뷰·지시서 작성(나)과 구현(다른 에이전트)을 분리했습니다. 리뷰할 때는 서브에이전트 3~4개를 영역별로 병렬 투입하고, 치명적인 지적은 원본 코드로 직접 재확인합니다.
- **검증 루프**: Propose → Probe(`probe_krui.ps1`) → Compose. 단계 하나가 커밋 하나이고, 각 단계에 완료 기준과 필수 테스트를 명시합니다. 3회 연속 probe 실패, 근거 없는 공개 API 변경, `unsafe` 필요 시에는 사람에게 질문하고 중단합니다.
- **빌드 함정**: 예제 창(`04_elm_flux_pattern.exe` 등)이 열려 있으면 링크 에러 `LNK1104`로 probe가 실패합니다. 검증용으로 `CARGO_TARGET_DIR=target-review`를 쓰면 `.gitignore`(`/target-*`)에 걸려 저장소가 오염되지 않습니다.
- **측정 함정**: 빌드·리뷰어가 동시에 돌 때 잰 성능 수치는 쓰지 않습니다.
- **주의**: 리뷰 도중 `src/lib.rs`, renderer, text 파일이 바뀐 적이 있습니다(동시 편집). 줄 번호는 항상 **심볼 이름으로 다시 찾아야** 합니다.
- **인코딩**: Windows 콘솔에서 한글이 깨져 보이는 것은 출력 인코딩 문제입니다. 소스 파일은 UTF-8입니다.

---

## 5. 문서 자산

| 위치 | 내용 |
|---|---|
| `AGENTS.md` | 제품 목표, 절대 규칙 12개, 클린 코드 규칙, DSL 원칙, 패턴 표, 작업 루프. **규칙의 원본** |
| `docs/HISTORY.md` | 이 문서 |
| `docs/TODO.md` | 앞으로 할 일 (자체 완결형 작업 지시서) |
| `ARCHITECTURE.md`, `README.md`, `CHANGELOG.md` | **M0 이전 내용 그대로라 현재와 다릅니다**(예: README의 `m0_showcase` 예제, "paint fast-path"에 font 속성 분류, `krui-mvvm` 이름). 갱신 필요 → `TODO.md` |
| `.agents/skills/krui-harness2-loop/SKILL.md` | 오래된 설명("Google Research Harness²", "5대 가드레일") 갱신 필요 |
| 이전 대화의 `m0_fix_prompt.md`, `m0_result_review.md`, `m1_work_order.md` | 개발툴 밖(`.gemini/antigravity/brain/...`)에 있어서 **이전되지 않습니다.** 필요한 내용은 이 두 문서에 옮겨 두었습니다. |
