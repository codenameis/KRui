# KRui Agent Guide

KRui는 **HTML/CSS 문법을 닮은 선언형 DSL(`.krui`)** 로 UI를 작성하는 순수 Rust GUI 프레임워크입니다.
Slint를 벤치마크 삼되, 웹 개발자와 Rust 입문자가 **바로 읽고 고칠 수 있는 것**을 최우선으로 합니다.

---

## 0. 제품 목표 (모든 판단의 기준)

| 우선순위 | 목표 |
|---|---|
| 1 | **IME 최우선**: 한국어, 일본어, 영어 입력이 OS 네이티브 수준으로 동작합니다(조합, 변환, 후보창 위치). |
| 2 | **낮은 진입 장벽**: `.krui`는 HTML/CSS를 아는 사람이 5분 안에 읽을 수 있어야 하고, Rust 코드는 문법을 막 뗀 사람도 수정할 수 있어야 합니다. |
| 3 | **라이브 미리보기**: 앱 개발 중 `.krui`/`.css`를 저장하면 앱 상태를 유지한 채 즉시 반영됩니다. Dioxus/Flutter처럼 `krui serve`로 데스크탑, 디바이스 프레임, Android 에뮬레이터/iOS 시뮬레이터에서 실행 중인 앱에 핫리로드합니다. |
| 4 | **어디서나 동작**: 데스크탑(`Win/Mac/Linux` + 멀티윈도우·터치) → 모바일(`Android` `krui-android-ffi` / `iOS` 예정). 게임 엔진·영상 통합은 `ExternalTexture` 슬롯으로 연결합니다. |
| 5 | **고성능**: 정적 UI와 비활성 창은 유휴 시 0% CPU, 컴포지터 애니메이션(transform/opacity/색상 등)은 레이아웃 없이 동작하며 레이아웃 속성 애니메이션만 변경 노드를 국소 dirty 처리합니다. |
| 6 | **순수 CSS 호환(목표 100%) & 헤드리스 위젯**: `krui-stylo-bridge`(`stylo 0.22.0` + `stylo_taffy` + `taffy 0.15.1`) 단일 CSS 소유 계층으로 외부 순수 `.css` 프레임워크 무수정 적용을 목표로 합니다(현재 `@tailwindcss/cli@4.3.5` 실제 출력 속성 일치율 **229/229 [100.0%]**, 측정일 `2026-11-25`, `tests/real_css_conformance.rs`). 헤드리스 동작 코어(`unstyled` 옵션 포함) 위에 순수 CSS 테마/디자인 시스템을 결합합니다. |
| 7 | **통합 반응형 + 비동기**: 표준 반응형 상태(`Signal`/`Computed`/`Effect` + 일반 메서드 호출 + 내장 `tokio` 비동기)를 기본으로 하고, `Store<State, Msg>` 등 아키텍처 패턴은 `krui-patterns`로 제공합니다. |

---

## 1. 절대 규칙 (Guardrails)

1. **`unsafe` 최소화**
   - 모든 크레이트는 기본적으로 `#![forbid(unsafe_code)]`를 둡니다.
   - **예외는 플랫폼 연결 크레이트(`krui-android-ffi`)와 Stylo DOM 트레이트 브리지(`krui-stylo-bridge`)뿐입니다.** (두 크레이트 모두 `#![deny(unsafe_code)]` 아래 직접 작성한 `unsafe {}` 블록 0줄 유지, 트레이트/FFI 계약상 필수인 `unsafe fn`/`#[no_mangle]`만 허용)
   - 예외 크레이트는 다음을 지킵니다.
     - 직접 작성한 `unsafe` 블록 0줄 및 트레이트/FFI `unsafe` 선언은 크레이트당 **10개 이내**로 둡니다. 각 항목에 `// SAFETY:` 주석을 답니다.
     - 루트 `README.md`의 "unsafe 목록" 표에 모두 기록합니다.
     - 안전한 대안(안정적인 의존성의 safe API, 코드 생성 도구)을 먼저 검토하고, 검토 결과를 PR에 적습니다.
   - `krui-core`, `krui-signals`, `krui-compiler`, `krui-macros`, `krui-patterns`, 렌더러에는 예외를 두지 않습니다.
2. **릴리즈 빌드에는 마크업/미리보기 파서가 없습니다.**
   - `.krui` 마크업/템플릿은 `krui!`/`include_krui!` 매크로가 컴파일 타임에 정적 UI 디스크립터 슬라이스(`StaticDocumentDesc`/`StaticNodeDesc` in `krui-core::static_ir`)와 단일 바인딩 디스패치로 변환합니다. (외부 테마 교체용 `.kruitheme` 고정 헤더·슬라이스 경계 검증 테이블 로더만 예외로 허용하며, 마크업 파서가 아님을 명시합니다.)
   - 미리보기용 인터프리터는 `preview` feature(기본 off, dev 전용)에서만 컴파일됩니다.
   - 매크로 코드 생성과 미리보기 인터프리터는 **같은 IR(`krui-compiler`의 `CompiledDocument`)** 을 사용합니다. 미리보기 결과와 릴리즈 결과가 달라지면 버그입니다.
3. **단일 CSS 소유 계층(`krui-stylo-bridge`) & W3C 전방 호환 CSS 파싱**
   - HTML 마크업·제어흐름 태그와 `{...}` 템플릿 바인딩 오류는 `CompileError`(파일·줄·열 포함, 프로시저 매크로는 패닉 없이 `syn::Error`)로 처리합니다.
   - 모든 CSS 파싱·캐스케이드·커스텀 프로퍼티(`@property`, `light-dark()`, `@layer`, `@media`)·애니메이션 계산은 단일 크레이트 [`crates/stylo-bridge`](crates/stylo-bridge/src/lib.rs)(`stylo 0.22.0` + `stylo_taffy` + `taffy 0.15.1`)가 전담합니다. 현재 `@tailwindcss/cli@4.3.5` 실제 출력 속성 일치율은 **229/229 [100.0%]**(측정일 `2026-11-25`)입니다.
   - 미지원 선언·값·`@container` 등은 Stylo W3C 선언 무시(drop) 정책과 `Diagnostic`으로 비차단 처리해 같은 블록 내 선행 유효 폴백 선언을 보존합니다.
   - `ThemePreset`·`TokenId`·`TokenTable` 및 `lightningcss`는 완전히 제거되었으며 `examples/`에는 `component_gallery`만 유지합니다.
4. **매크로 위생**
   - 매크로는 파일을 쓰지 않습니다(`std::fs::write` 금지).
   - 읽은 모든 외부 파일(`.krui`, `.css`)은 `include_bytes!`로 Cargo가 변경을 추적하게 합니다.
5. **2계층 Fast-Path 불변식**
   - **1계층(컴포지터 Fast-Path)**: 명시적 Fast-Path 속성은 컴파일 타임에 확정합니다. `transform`(2D 및 4×4 행렬의 `z=0` 아핀 투영; 원근 3D 투영은 `docs/ENGINE_LIMITS.toml` 백로그로 추적), `opacity`, 색상, 4모서리 `border-radius`, 4변 `border`, 그라데이션(선형 3-stop / 방사형 2-stop; 초과 정지점 및 `conic-gradient`는 경고+백로그), 다중/내부(`inset`) `box-shadow`는 레이아웃(Taffy) 없이 WGSL SDF 셰이더 + 인스턴스 버퍼 단일 배칭으로 처리하며, `backdrop-filter`·CSS `filter`·그룹 `opacity` 서브트리(복합 `clip-path`는 추후 스텐실 백로그)만 오프스크린 렌더 타깃으로 분리합니다(기본 테마·벤치마크는 Fast-Path만 사용).
   - **2계층(레이아웃 애니메이션 트랙)**: `@keyframes`와 `transition`에서 `width`, `height`, `padding` 등 레이아웃 속성도 허용하되, `transition: all`(속성 생략 단축 표기 포함)과 `var()` 변수는 **런타임에 실제 변경된 속성이 레이아웃 속성일 때만** 해당 노드를 Taffy dirty 처리하고 비차단 `CompileWarning`(스타일시트별 경고 억제 지원)을 남깁니다. 핫패치로 CSS가 재컴파일될 때 진행 중인 레이아웃 애니메이션 타깃을 즉시 재평가해 Taffy dirty 상태를 동기화합니다.
   - 텍스트, 폰트, 정적 크기 변경은 **반드시** 해당 노드를 레이아웃 dirty로 표시합니다. `transform`은 노드별 행렬(기본 원점 = 박스 중심)로 부모→자식 순서로 누적하고, 렌더링과 히트테스트가 같은 행렬을 씁니다.
6. **수명 관리**: 노드, Effect, 애니메이션 드라이버는 소유 스코프가 해제되면 함께 해제됩니다. 해제된 `NodeId`(generational)에 접근하면 무시되거나 `None`을 반환하고, 패닉하지 않습니다.
7. **플랫폼 분리**
   - `krui-core`(ViewTree, 레이아웃, 입력 처리, PaintScene)는 winit/wgpu에 의존하지 않습니다.
   - 윈도우와 이벤트 루프는 `krui-winit`, 리눅스 키오스크 DRM/evdev 호스트는 `krui-kms`, 자체 3D 엔진 인패스 호스트는 `krui-engine-host`, 브라우저 DOM 호스트는 `krui-web`, Android/Wear OS 호스트 브리지는 `krui-android-ffi`, GPU/SW 렌더러는 `krui-renderer`(`gpu` feature 선택)가 담당합니다. 게임 엔진이나 외부 호스트는 core에 이벤트를 주입하고 `PaintScene`/`DynamicSlotBuffer`/`ExternalTexture` 또는 오프스크린 wgpu 텍스처로 결과를 받습니다.
   - 비동기 런타임(`tokio`)은 `krui-signals`에 내장하되 `AsyncRuntimeMode`(`MultiThread` / `SingleWorkerKiosk` / `ExternalHandle` 주입)로 제어하며, 모든 백그라운드 태스크·스트림은 소유 스코프 해제 또는 패닉/취소 시 자동 정리되는 `Drop` 가드를 둡니다.
8. **UI 스레드 블로킹 금지**: 서브프로세스(powershell 등) 실행이나 동기 I/O를 하지 않습니다. 클립보드는 `arboard` 같은 네이티브 API를 씁니다.
9. **가짜 기능 금지**: 동작하지 않는 스텁, "암호화"라고 부르는 난독화, 측정하지 않은 성능 수치를 문서나 테스트에 쓰지 않습니다. "완료"라고 보고하려면 **그 기능이 실패하면 함께 실패하는 테스트**가 있어야 합니다.
10. **순수 CSS 전담 & 헤드리스 위젯 코어 원칙**
    - core와 렌더러는 색상 값·노드 인덱스·태그 이름에 따른 특수 처리를 금지합니다.
    - 다크/라이트 모드와 테마 토큰은 전적으로 순수 CSS(`@media (prefers-color-scheme)`, `light-dark()`, `@layer`, `@property`, 런타임 DOM 상속 `--*` 변수)가 담당하며, Rust API는 윈도우/앱 단위의 `@media (prefers-color-scheme)` 평가 모드(`System`/`Light`/`Dark`) 오버라이드 인터페이스만 제공합니다.
    - `@font-face`와 `background`/`mask`의 `url(...)`은 로컬 상대 경로·임베디드 번들·인라인 `data:` URI(Base64/UTF-8 SVG)·SVG 래스터라이즈·`ttf/otf/woff2` 디코딩·`local()` 시스템 폰트를 기본 지원합니다. 코어에 HTTP/TLS 의존성을 강제하지 않도록 원격 `http(s)://` 에셋은 비차단 경고 후 다음 폴백으로 넘기되 호스트 주입용 비동기 `AssetLoader` 트레이트 인터페이스만 열어둡니다.
11. **IME 구조**
    - `krui-core`가 플랫폼 중립 `EditingState { text, selection, composing }`와 IME 상태 기계, 리눅스 키오스크용 내장 두벌식 한글 오토마타(`Hangul2BulsikAutomaton`)를 가집니다. 플랫폼 어댑터(winit, `krui-kms`, `krui-android-ffi`, iOS, 임베드 호스트)는 `TextInputHost` 트레이트로 연결합니다.
    - 조합(composing)은 OS가 소유합니다(단, OS IME가 없는 `kiosk-spi` 키오스크는 KRui 내장 오토마타+OSK, 후순위 `watch-apple` 타깃은 Swift 시스템 받아쓰기/손글씨 위임 예외). 조합 중인 입력은 **조합이 끝날 때까지 시작한 노드(ime_target)로** 이벤트를 받습니다. 포커스가 바뀌어도 조합 텍스트를 잃지 않습니다. 데스크탑 비밀번호 필드는 OS IME 조합을 비활성화하고, 모바일은 `InputKind::Password` 소프트 키보드를 씁니다.
12. **성능 수치는 통제된 측정만**: release 빌드, 워밍업 후 N회 반복, 중앙값과 p99를 보고합니다. 다른 빌드나 작업이 동시에 돌고 있을 때 잰 숫자는 쓰지 않습니다.
13. **실기기 핫리로드 & 그래픽 프리미티브**: 가짜 웹 샌드박스/더미 뷰포트를 두지 않으며, `krui serve`는 실행 중인 실제 데스크탑/Android 앱에 TCP로 핫리로드합니다. 래스터 이미지는 `<img>`(PNG/JPG/WebP), 아이콘은 컴파일 타임 `<svg>`, 외부 영상·엔진 통합은 `ExternalTexture` 슬롯을 사용합니다.

---

## 2. 클린 코드 & 가독성 규칙 (입문자 기준)

- **파일 하나는 500줄 이하**, 함수 하나는 화면 한 페이지(~60줄) 이하를 **작성 목표(권장선)** 로 둡니다. 단, 국소 버그 수정이나 `cargo fmt` 줄바꿈 때문에 억지로 파일·함수를 쪼개며 연쇄 수정을 일으키지 않도록 **테스트 하드 상한은 파일 560줄·함수 80줄(완충선)** 로 두며, 모듈 분할은 별도 리팩터링 요청 시에만 수행합니다.
- **변경 영향 반경(Blast Radius) 최소화**: 아키텍처 오류가 아닌 버그·보완은 원인 파일 내부(원칙적 1~3개 소스 파일 + 회귀 테스트)에서만 국소 수정합니다. 인접 파일의 부수적 리팩터링·이름 변경·모듈 재구성을 섞지 않으며, 크레이트 간 시그니처/IR 변경이나 **5개 이상 소스 파일**을 건드리는 작업은 먼저 대상 파일 목록과 사유를 제안해 합의한 뒤 진행합니다.
- 매크로 코드젠(`krui-macros`) 내부 전용 타입·헬퍼를 새로 연결할 때는 루트 공개 API 목록을 늘리지 않고 `::krui::__private::<crate>::*` 경로를 우선 사용합니다.
- 공개 API는 **구체 타입 + 짧은 이름**을 씁니다. 제네릭, 트레이트 객체, 라이프타임은 내부에서만 쓰고, 사용자 예제 코드에 `Rc<dyn Fn>`가 보이면 안 됩니다.
- 같은 로직이 2번 나오면 헬퍼로 뽑습니다(예: `with_focused_input(|input| ..)`).
- 매직 넘버를 쓰지 않습니다. 값은 `const` 또는 테마 토큰으로 둡니다.
- 렌더러는 CSS가 정한 값을 **임의로 바꾸지 않습니다**. 기본 모양은 테마 프리셋 CSS로 표현합니다.
- 모든 공개 항목에 `///` 문서를 답니다. 예제 주석은 한국어, 코드 식별자는 영어로 씁니다.
- `cargo fmt`, `cargo clippy -- -D warnings`를 통과해야 합니다.
- 에러는 `Result`로 반환합니다. 사용자 입력 경로에서 `unwrap`/`expect`/인덱스 패닉을 쓰지 않습니다.

---

## 3. DSL 원칙 (`.krui`)

- 문법: HTML 태그 + `<style>` CSS + `{expr}` 바인딩. **HTML을 아는 사람이 처음 봐도 읽을 수 있어야 합니다.**
  - 속성값은 HTML처럼 **항상 따옴표**로 감쌉니다. 바인딩은 따옴표 안에 `{}`를 씁니다: `class="card"`, `value="{vm.name}"`
  - 이벤트는 HTML과 같은 이름을 씁니다: `onclick`, `oninput`, `onchange`, `onkeydown`. `on:click`은 별칭으로만 허용합니다.
  - 양방향 바인딩: `<input bind:value="{vm.name}">`
  - 불리언 속성은 HTML과 같습니다: `<button disabled="{!vm.can_save}">`, `<input type="password">`
  - 텍스트 보간: `<p>안녕하세요, {vm.name}님</p>`. 중괄호 문자 자체를 쓰려면 `{{`, `}}`를 씁니다.
  - 주석: `<!-- ... -->`
- **템플릿 식(`{...}`)은 작은 식 언어로 제한합니다.** 매크로(릴리즈)와 미리보기 인터프리터가 **똑같이** 해석해야 하기 때문입니다.
  - 허용: 경로(`vm.cart.total`), 리터럴(문자열, 숫자, 불리언), `! && || == != < <= > >= + - * /`, 괄호, `<for>`의 항목 변수, 등록된 메서드·명령 호출(`ctrl.buy(5)`, `store.on(Msg::Add)`), enum 경로(`Msg::Add`).
  - 금지: 클로저, 블록, `match`, 임의의 Rust 함수 호출. 복잡한 로직은 ViewModel의 `Computed`나 메서드로 옮기고, 템플릿에는 이름만 씁니다.
  - `#[view_model]`과 Store·Controller 매크로는 "이름 → Signal/Command/메서드" 레지스트리를 생성합니다. 인터프리터는 이 레지스트리로 바인딩합니다.
- 제어 흐름은 **HTML 태그 형태**로 씁니다(템플릿 기호 `{#if}` 같은 것은 쓰지 않습니다).
  ```html
  <if test="{vm.is_logged_in}">
    <p>환영합니다, {vm.user_name}님</p>
  <else-if test="{vm.is_loading}">
    <p>로그인 중...</p>
  <else>
    <button onclick="{vm.login}">로그인</button>
  </if>

  <ul>
    <for each="{item in vm.items}" key="{item.id}">
      <li class="row">{item.title}</li>
    </for>
    <empty>
      <li>항목이 없습니다</li>
    </empty>
  </ul>
  ```
  - `<else-if>`/`<else>`는 `<if>` 안에서 구분자 역할을 하며, 닫는 태그가 없습니다.
  - `<for>`의 `key`는 필수입니다. 키가 없으면 컴파일 에러를 냅니다(목록 갱신 성능과 입력 포커스 보존을 위해).
  - `<for>` 바로 뒤에 오는 `<empty>`는 목록이 비었을 때 표시됩니다.
  - `<if>`/`<for>`는 레이아웃 박스를 만들지 않습니다(자식이 부모에 직접 배치됨).
- 컴포넌트: 파일 하나가 컴포넌트 하나입니다. `Card.krui` → `<Card title="{..}">…</Card>`. 대문자로 시작하는 태그는 컴포넌트입니다. 컴포넌트의 `<style>`은 클래스명 해싱(mangling) 없이 원본 태그·클래스·속성명을 보존한 채 `:where()` 또는 `@scope`와 스타일시트 최상단(외부 `.css` 로드 이전)에 선언되는 암묵적 하위 레이어(`@layer preset, component;`)로 스코핑하여 외부 `.css`가 항상 우선 덮어쓸 수 있게 합니다. 외부 프레임워크 전체 적용을 위한 내장 기본 스타일 비활성화(`unstyled`) 옵션과 `<slot/>`/`<slot name="..."/>`을 지원합니다.
- **표준 순수 CSS 호환(목표 100%, 현행 실측 229/229 [100.0%]) + 자동 Fast-Path 최적화 원칙**:
  - **셀렉터**: 태그·`.class`·`#id`·`*`, 후손(`A B`)·자식(`A > B`)·인접/일반 형제(`+`, `~`) 결합자, 속성 부분 일치(`[attr^=.../$=.../*=...]`), `:hover`, `:focus`, `:focus-within`, `:placeholder-shown`, `:active`, `:disabled`, `:checked`, `:nth-child`, `:not()`, `:is()`, `:where()`, `:has()` 의사 클래스, 그리고 독립 레이아웃·페인트·애니메이션 박스로 동작하는 `::before`/`::after` 및 `::placeholder`·`::selection`·모달 `::backdrop`을 지원합니다. 컴파일 타임에 **셀렉터 의존성 플래그(Invalidation Map)** 를 생성해 일반 노드는 Fast-Path를 유지하고 동적 상태가 걸린 부모·형제 노드만 런타임에 선택적으로 재평가합니다.
  - **규칙·색상·레이아웃·타이포 스펙**: `@media`(색상 스킴·모션 감소[`prefers-reduced-motion`는 웹 외 타깃에서 컴파일러가 `:root:where([data-krui-motion="reduce"])`로 재작성]·뷰포트·`hover`/`pointer`), `@layer`, `@import`(파일 기반 시트; 외부 링크 시트 내 중첩 `@import`는 엔진 백로그), `@supports`(선언 컴파일러 판정), `@property`, `@keyframes`, CSS Color 4/5(`oklch`/`oklab`/`hwb`, 공백·슬래시 알파, `color-mix`, 상대 색상 `from`, `light-dark()`, `currentColor`, wgpu 선형/sRGB 이중 감마 보정 방지), 단위·수식(`rem`/`em`/`vw`/`vh`/`vmin`/`vmax`/`dvh`/`svh`/`%`, `calc`/`min`/`max`/`clamp`), `block`/`inline-block`/`contents`(`display: table` 계열→Grid/Flex 매핑, `float`/`clear`→비차단 경고+폴백), `fixed`/`sticky`, `overflow-x`/`overflow-y`, `appearance: none`, `aspect-ratio`, 타이포그래피(`line-height`, `word-break`/`overflow-wrap`, `line-clamp`/`-webkit-line-clamp`, `font-variation-settings`, OpenType `liga`), `@container` 크기 쿼리, CSS `filter`를 구현하며, 복합 `clip-path`·`conic-gradient`·원근 3D 투영은 `docs/ENGINE_LIMITS.toml`에 추적합니다.
- 테마 프리셋은 **토큰(CSS 변수)과 태그 기본값**만 정의합니다. `.row` 같은 앱 클래스를 넣지 않습니다.

---

## 4. 아키텍처 패턴

- 기본 표준은 **통합 반응형 모델(`Signal`/`Computed`/`Effect` + 일반 메서드 호출 + `bind:value` + `tokio` 비동기)** 입니다.
- 아래 확장 패턴은 선택형 라이브러리(`krui-patterns`)로 제공합니다.

| 패턴 | 제공 API | 예제 |
|---|---|---|
| MVVM | `#[view_model]`, `Command`(+`can_execute`, `tokio` async), `bind:value` | `02_mvvm` |
| MVC | Controller의 일반 메서드 + `onclick="{ctrl.buy(5)}"`. Controller에 그리기 코드 금지 | `03_mvc` |
| Flux/Elm | `Store<State, Msg>` + `update(&mut State, Msg) -> Effects<Msg>` + `select` | `04_elm` |
| MVI | 같은 `Store`를 Intent → Reducer → State 흐름으로 사용, Effects로 부수효과 처리 | `05_mvi` |

- 단방향 패턴 예제에서 `bind:value` + Effect 안의 dispatch는 금지합니다. `oninput="{store.on_input(Msg::X)}"`를 씁니다.

---

## 5. 작업 루프 (Propose → Probe → Compose)

1. **Propose**: 바꿀 크레이트, **수정 예정 파일 목록(영향 반경)**, 그리고 데이터 흐름(DSL → IR → codegen/interpreter → ViewTree → Layout → PaintScene → Renderer)을 먼저 적습니다(비-아키텍처 수정은 원칙적 1~3개 소스 파일 이내).
2. **Probe**: `.agents/skills/krui-harness2-loop/scripts/probe_krui.ps1`를 실행합니다.
   - `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`를 **기본 feature와 `--features preview` 모두**로, 그리고 `cargo test --workspace`
   - 릴리즈 의존성 그래프에 `krui-compiler`가 없는지 `cargo tree`로 검사(문자열 검색 금지)
   - IME 시나리오 테스트: 한국어 조합, **조합 중 포커스 이동**, 일본어 변환과 Enter/Space, 영어, AltGr
   - 프레임 예산: 120프레임 애니메이션 동안 relayout 0회(애니메이션이 실제로 돌았는지도 확인). 텍스트 변경 시 relayout 정확히 1회. 화면 전환 100회 스트레스 후 **모든** 런타임 카운트(signal, subscriber, scope, poller, frame 콜백, Taffy 노드, 노드 slab)가 여유분 없이 같을 것
   - 통제된 release 성능 측정(워밍업 후 중앙값/p99)이 예산 안에 들 것
   - 예제 전체 헤드리스 다중 프레임 실행
3. **Compose**: 통과한 변경만 반영하고, 발견한 엣지 케이스는 회귀 테스트로 고정합니다.
4. **리뷰 시 수정 범위 제한**: 코드 리뷰 요청 시 자동 코드 수정은 재현 시나리오가 명확한 `Critical`/`High` 실제 버그에 한해 원인 파일 내부에서만 최소 diff로 수행합니다. `Medium`/`Low` 및 구조·가독성 개선(모듈 분리·함수 추출·리팩터링)은 보고서에 제안으로만 남기고 별도 요청 시에만 수행합니다.

---

## 6. 규칙 강제 매트릭스

반복 발생 실수는 문서 설명에 그치지 않고 구조적 API/컴파일 제약 또는 자동 게이트로 강제합니다.

| 반복 실수 유형 | 강제 수단 | 검증 대상 및 불변식 |
|---|---|---|
| 가짜 통과 / 프록시 검증 테스트 | 동작·E2E 테스트 (`tests/live_preview_component_gallery_e2e.rs`, `tests/css_compositor_transitions.rs`) | 주석·더미 변경 대신 실제 `:root` 모션/색상/트랜지션 CSS 교체 후 픽셀 diff·모션 보간·`<if>` 재마운트까지 검증 |
| Rust 코드에서 CSS 토큰 덮어쓰기 | 순수 CSS 전담 + 게이트 (`examples/component_gallery/main.rs`, `tests/probe_and_bench_gates.rs`, `probe_krui.ps1`) | `krui://preset/unstyled` + `[data-theme]` 순수 CSS 테마팩만 사용 (`set_theme`·`switch_theme_preset`·`set_app_tokens`·`Color::rgb` 금지) |
| 컴파일 타임 `:root` 및 CSS 전방 호환 정책 | 컴파일러 분류 검증 (`crates/compiler/src/stylesheet_tokens.rs`) + 회귀 테스트 (`tests/css_compositor_transitions.rs`) | 시스템·모션/커브/transform CSS 변수를 검증하고, Wave 1-A(W3C 선언 drop + 선택적 `strict` 린트 + `@supports`) 전환 전까지 현행 비테이블 `:root` 검증을 유지 |
| 하위 앱·브리지 안전·크기 규율 이탈 | 정적/위생 게이트 (`tests/probe_and_bench_gates.rs`, `probe_krui.ps1`) | 일반 `crates/`·`src/`·`examples/`·`tests/` `#![forbid(unsafe_code)]`, `android-ffi`·`stylo-bridge` `#![deny(unsafe_code)]`(0 `unsafe {}`, ≤ 10개), 파일 ≤ 500/560줄, 함수 ≤ 60/80줄 강제 |
| 핫패치 시 비활성 제어흐름/제거된 토큰 잔류 | 미리보기 패치 판별 (`crates/preview/src/style_patch.rs`, `crates/preview/src/lib.rs`) + E2E (`tests/css_compositor_transitions.rs`) | `<if>`/`<for>` 제어흐름 스타일 변경 시 상태 보존 재마운트, 스타일시트 패치 시 Stylo 캐스케이드·제거된 토큰 상태 재동기화 |
| 릴리즈 파서 유입 / Fast-Path 레이아웃 누수 | 의존성·런타임 게이트 (`probe_krui.ps1`, `cargo tree`) | 릴리즈 그래프에 `krui-compiler`·`krui-preview`·`lightningcss` 미포함, `StyloDamage` 기반 Fast-Path 120프레임 중 Taffy relayout 0회(레이아웃 트랙은 변경 노드만 dirty), 화면 전환 100회 누수 0 |
| 런타임 `if ==` / 문자열·색상·프리셋 하드코딩 | Clippy + `syn` AST 래칫 (`crates/{core,renderer,layout}/clippy.toml`, `tests/anti_hardcode_lint_gate.rs`, `probe_krui.ps1`) | `Color::rgb/rgba`·`ThemePreset` 차단, Tier A(`renderer`/`layout`/`a11y`/`signals`/`android-ffi`) 문자열/`ThemePreset` 분기 0건, Tier B(`core`/`winit`/`patterns`) 신규 하드코딩 차단 및 기존 목록 양방향 축소 강제 |
| `kiosk-spi` 최저 티어 공존·부팅·부분 손상 게이트 | CPU 전용 피처 게이트 + `krui-kms` 모의 게이트 (`cargo check -p krui-renderer --no-default-features`, `cargo test -p krui-kms`, `crates/engine-host`) | `kiosk-spi` 빌드에서 `wgpu`/`naga`/`winit` 완전 제외, `hx8357d` 외접 사각형 분할 커밋 및 플러시 스레드 비블로킹, 엔진 `DirtyFlagOnly` 0-alloc 불변식 강제 |
