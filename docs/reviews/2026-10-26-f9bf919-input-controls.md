# KRui 입력·컨트롤 기본 동작 리뷰 (2026-10-26, HEAD `f9bf919`)

> 대상: 현재 HEAD `f9bf919` (`A-fix-4`). 작업 트리는 clean이다.
>
> 사용자 재현: 실제 데스크탑 앱의 텍스트 박스에서 방향키가 동작하지 않는 것처럼 느껴지고, 전체 컨트롤이 반쪽짜리다. 이번 리뷰는 새 컨트롤 구현이 아니라 **기본 입력이 실제로 성립하는지**와 **컨트롤 확장 전에 무엇을 먼저 끝내야 하는지**를 확인했다.
>
> 방법: `winit KeyboardInput → KeyInfo → route_key → apply_key_action → 커서/선택 → PaintScene/IME caret` 경로와 DSL 컨트롤 표면을 정적 추적했다. 리뷰어 쓰기 범위를 지키기 위해 cargo build/test/probe와 GUI 실행은 하지 않았다. 기존 `probe` 통과 보고는 입력 동작의 증거로 사용하지 않았다.

## 총평

좌우 이동·Home/End·grapheme 삭제·한/일 IME 조합의 core 경로는 실제로 구현돼 있고 일부 테스트도 있다. 그러나 **Up/Down은 키 라우팅에 아예 없고, Shift/Ctrl 조합은 기본 이동을 지우거나 삼키며, 단일행 input이 폭에 맞춰 줄바꿈되어 긴 텍스트에서는 캐럿이 오른쪽 끝에 고정된다.** 데스크탑 이벤트에서 이 동작을 끝까지 검증하는 테스트도 없다. 이 조합이 “방향키가 죽은 것처럼 보이는” 가장 유력한 정적 원인이다.

현재 DSL의 실제 컨트롤은 `button`, `input`, `canvas`와 selectable text/scroll container뿐이다. `label`, `textarea`, `select`, `checkbox`, `radio`, `range`, `form`은 없으며, 이들을 올리기 전에 필요한 포커스·이벤트·텍스트 기하·검증 기반도 아직 부족하다. **새 컨트롤을 먼저 추가하지 말고 C0~C3의 기본 입력을 먼저 완성해야 한다.**

이번 입력/컨트롤 범위에서 즉시 프로세스를 종료시키는 Critical은 확인되지 않았다. 아래 High는 모두 사용자 조작이 직접 실패하거나 컨트롤 계약을 조용히 위반하는 문제다.

## 사용자 증상과 실제 키 경로

| 키 | 현재 코드 결과 | 판정 |
|---|---|---|
| Left / Right | `KeyInfo::ArrowLeft/Right` → `CursorLeft/Right` → grapheme 이동 | core 구현 있음. 단, 데스크탑 E2E 테스트 없음 |
| Up / Down | `KeyInfo::Other("ArrowUp"/"ArrowDown")` → `KeyAction::None` | **동작 없음** |
| Home / End | `CursorHome/End` → 문자 0/끝 | 구현 있음 |
| Shift+Left/Right/Home/End | `mods.shift`를 사용하지 않고 이동 함수가 selection anchor를 지움 | **선택이 확장되지 않고 사라짐** |
| Ctrl+Left/Right | shortcut 분기에서 `KeyAction::None` | **동작 없음** |
| Ctrl+Backspace/Delete | shortcut 분기에서 `KeyAction::None` | **일반 Backspace보다 나쁜 no-op** |
| Ctrl+A/C/X/V | `SelectAll/Copy/Cut/Paste` | 동작. Copy 예외는 아래 Medium |
| Ctrl+Z/Y | shortcut 분기에서 `None`, undo stack 없음 | **undo/redo 없음** |
| Tab / Shift+Tab | focus 순회 | 동작. disabled 노드 문제는 아래 High |
| Enter / Space | `onchange`/공백 입력/button 활성화 | 동작. key repeat 문제는 아래 High |

코드상 짧은 텍스트의 Left/Right는 core에서 이동한다. 실제 앱에서 짧은 텍스트에서도 좌우가 전혀 움직이지 않는다면 아래 조건 중 하나를 먼저 계측해야 한다.

1. 긴 텍스트가 줄바꿈되어 캐럿 x가 오른쪽 끝에 clamp되는가.
2. 포커스된 input이 reactive `disabled`로 바뀌어 `focused_input_rect()`는 남아 있지만 모든 편집이 no-op인가.
3. 조합 중인 `ime_target`의 preedit가 늦게 정리되어 `has_active_ime_preedit()`가 모든 키를 막고 있는가.
4. 포커스가 실제 input이 아니라 `selectable="true"` span/p인가. 이 경우 방향키 이동 자체가 없다.

## 필수 수정

### High

- **[High] `crates/winit/src/window_helpers.rs:63-82`, `crates/core/src/view_tree/key_routing.rs:157-185`, `crates/core/src/view_tree/events.rs:431-495` — Up/Down이 없고 데스크탑 방향키 경로가 검증되지 않는다.**
  - 영향: input에 포커스한 채 Up/Down을 누르면 `KeyInfo::Other`로 떨어져 아무 일도 없다. 사용자가 방향키 클러스터를 “반쪽”으로 느끼는 직접 원인이다. Left/Right도 `key_info_from_winit`과 `route_key`를 통과하는 테스트가 없어 플랫폼 매핑 회귀를 잡지 못한다.
  - 제안: `KeyInfo::ArrowUp/Down`과 명시적 `KeyAction::CursorUp/Down`을 추가한다. 단일행 input의 계약(no-op 또는 Home/End)을 문서화하고, multiline이 생기면 같은 x의 이전/다음 줄로 이동한다.
  - 테스트: `key_info_from_winit` NamedKey 표, `route_key`의 Left/Right/Up/Down/Home/End × none/Shift/Ctrl 표, `apply_key_action` 후 caret x와 paint를 함께 검사한다.

- **[High] `crates/core/src/view_tree/key_routing.rs:139-166`, `crates/core/src/view_tree/events.rs:433-495` — Shift+방향키 선택이 없다.**
  - 영향: `mods.shift`는 Tab에서만 사용된다. `Ctrl+A`로 전체 선택 후 Shift+Left를 누르면 선택이 유지·확장되지 않고 anchor가 지워지며 캐럿만 한 글자 이동한다. 키보드만으로 텍스트를 선택할 수 없다.
  - 제안: `CursorLeft { extend }`처럼 이동 action에 선택 확장 여부를 넣고, extend일 때 anchor를 보존한다. Home/End와 Shift 조합도 같은 계약을 사용한다.
  - 테스트: 선택 없음/기존 선택/문서 끝에서 Shift+Left/Right/Home/End의 anchor와 focus를 검사한다.

- **[High] `crates/core/src/view_tree/key_routing.rs:144-155` — Ctrl+방향키와 Ctrl+Backspace/Delete가 shortcut 분기에서 사라진다.**
  - 영향: `"hello world"` 끝에서 Ctrl+Backspace를 누르면 단어가 지워지기는커녕 아무것도 지워지지 않는다. Ctrl+Left/Right도 단어 이동이 아니라 no-op다.
  - 제안: shortcut 분기보다 편집 키를 먼저 분류하고 `WordLeft/WordRight/DeleteWordBackward/DeleteWordForward`를 Unicode word boundary로 구현한다.
  - 테스트: 공백, 구두점, 한글, emoji 주변의 단어 이동·삭제를 검사한다.

- **[High] `crates/core/src/view_tree/paint_text.rs:210-226`, `crates/text/src/font_stacks.rs:217-268`, `crates/text/src/rasterize.rs:107-118`, `crates/text/src/lib.rs:331-335`, `crates/core/src/view_tree/key_routing.rs:22-42` — 단일행 input이 줄바꿈되고 긴 텍스트의 캐럿이 고정된다.**
  - 영향: input도 다른 text role과 똑같이 `max_w`를 넘겨 cosmic-text 기본 `Wrap::WordOrGlyph`로 shaping된다. 둘째 줄의 glyph x는 0부터 다시 시작하지만 `char_x_offsets`는 단조 증가하도록 덮어써지고, `caret_rect`는 input 오른쪽에서 clamp된다. 200px input에 긴 문자열을 넣으면 둘째 줄 이후 문자를 선택해도 캐럿이 첫 줄 오른쪽 끝에 머문다. 텍스트는 input 박스 아래로 그려질 수 있고, 클릭 hit-test도 줄을 구분하지 못한다.
  - 제안: `TextInput|PasswordInput`은 no-wrap으로 shaping하고 `scroll_x`를 도입해 `origin_x`, glyph x, hit-test, selection box, caret_rect에 동일하게 적용한다. 캐럿이 바뀔 때 최소 이동량만 스크롤해 항상 보이게 한다.
  - 테스트: 200px input + 60자/20한글 음절에서 `measured_height`가 한 줄인지, End/Left/Right 후 캐럿이 보이는지, 박스 중앙 클릭이 올바른 문자로 매핑되는지 검사한다.

- **[High] `crates/text/src/lib.rs:117-126,331-335`, `crates/core/src/view_tree/hit_test.rs:421-455`, `crates/core/src/view_tree/events.rs:133-168`, `crates/core/src/view_tree/paint_text.rs:302-320` — selectable text의 선택 기하가 x만 사용한다.**
  - 영향: 줄바꿈된 `<p selectable="true">`에서 1줄에서 2줄로 드래그하면 `ly`가 버려져 같은 x의 1줄 문자로 선택이 되돌아간다. 선택 하이라이트도 줄별 사각형이 아니라 하나의 긴 가로 막대다. RTL glyph의 감소하는 x는 단조 clamp 때문에 시각 순서와 어긋난다.
  - 제안: `ShapedTextLayout`에 line index/top과 per-line x offsets를 보존하고 `hit_test(x, y)`를 제공한다. 선택은 줄별 rect로 그리고 RTL visual order를 보존한다.
  - 테스트: 줄바꿈 문단의 2줄 클릭·드래그, 줄별 highlight 수, 아랍어/히브리어 caret 위치를 검사한다.

- **[High] `crates/compiler/src/element_attrs.rs:187-214`, `crates/compiler/src/ir.rs:194-206`, `crates/core/src/view_tree/ime_host.rs:432-444` — `input type`이 조용히 사라진다.**
  - 영향: `number`, `email`, `search`는 compile을 통과하지만 IR에는 `is_secret_input: bool`만 있어 모두 `TextInput`/`InputKind::Text`가 된다. 숫자 검증, 모바일 숫자 키보드, 이메일 자동완성, 검색 semantics가 전부 사라지고 오류 메시지는 “text 또는 password만 지원”이라고 잘못 말한다. AGENTS의 “조용히 무시하지 않음”과 “가짜 기능 금지” 위반이다.
  - 제안: `InputType`을 IR과 runtime에 보존해 `InputKind`로 연결한다. 지금 구현할 수 없는 type은 명시적 `CompileError`로 거부한다.
  - 테스트: text/password/number/email/search의 macro/preview IR round-trip과 `focused_input_kind()`, 미지원 type의 compile error를 검사한다.

- **[High] `crates/core/src/view_tree/text_edit.rs:327-348`, `crates/style/src/lib.rs:420-433` — 단일행 input에 개행·탭이 들어간다.**
  - 영향: `sanitize_untrusted_text`는 `\n`과 `\t`를 보존한다. `dispatch_key_text`는 control 문자를 거부하지만 Paste와 IME Commit이 쓰는 `dispatch_text_input`은 빈 문자열만 검사한다. `"a\nb\tc"`를 붙여넣으면 단일행 input 내부에 개행/탭이 생겨 wrap·layout·hit-test가 깨진다.
  - 제안: input의 multiline capability를 타입으로 표현하고, 단일행에서는 `\n`/`\t`를 공백으로 정규화하거나 거부한다. Paste와 IME가 같은 정책을 사용한다.
  - 테스트: `"a\nb\tc"` 붙여넣기와 Commit 후 buffer에 control 문자가 없는지 검사한다.

- **[High] `crates/core/src/view_tree/key_routing.rs:144-155`, `crates/core/src/view_tree/text_edit.rs` — undo/redo가 없다.**
  - 영향: Ctrl+Z/Y는 shortcut 분기에서 `None`이 되고 노드에 편집 history가 없다. 한 글자 잘못 입력해도 되돌릴 수 없어 기본 텍스트 박스로 사용하기 어렵다.
  - 제안: text edit transaction 단위의 bounded undo/redo stack을 input 상태에 두고, insert/delete/IME replacement/paste/cut을 같은 transaction 모델로 기록한다. composition 중 snapshot 정책을 명시한다.
  - 테스트: 연속 입력, selection replace, paste, IME commit, undo 20회/redo, history bound를 검사한다.

- **[High] `crates/core/src/view_tree/mod.rs:325-334`, `crates/a11y/src/lib.rs:101-116`, `crates/core/src/view_tree/bindings.rs:133-169`, `crates/core/src/view_tree/a11y.rs:72-86` — disabled 컨트롤이 Tab 순서와 포커스를 유지한다.**
  - 영향: 생성 시 role만 보고 focusable로 등록하며 `focus_next/previous`는 disabled를 거르지 않는다. Tab이 disabled input/button에 도달하고 `:focus` 스타일과 캐럿이 표시되지만 편집은 모두 no-op이다. reactive disabled 전환은 포커스를 지우지 않는다.
  - 제안: Tab 순회에서 disabled를 제외하고, disabled로 바뀌는 순간 IME blur/onchange를 처리한 뒤 focus를 해제한다.
  - 테스트: focusable 3개 중 가운데 disabled, reactive disable, disabled button activation 0회를 검사한다.

- **[High] `crates/winit/src/window_loop.rs:205-238,312-322,448-473`, `crates/winit/src/window_helpers.rs:63-83` — button이 mouse-down에서 실행되고 key repeat를 막지 않는다.**
  - 영향: Left press에서 `dispatch_click`이 즉시 실행되고 release는 drag 종료만 한다. 버튼을 누른 뒤 밖으로 드래그해 놓아도 명령이 이미 실행됐다. `KeyEvent.repeat`은 `KeyInfo`로 전달되지 않아 Space/Enter를 누르고 있으면 OS repeat rate로 command가 반복된다. press 실행이 의도된 디자인이라도 key repeat는 별도 계약이 필요하다.
  - 제안: pointer press/release를 같은 node에 pair로 전달하고 release-inside에서 click을 발생시킨다. repeat flag를 전달해 Enter/Space activation에서는 repeat를 무시한다.
  - 테스트: press-outside/release, press-inside/release, held Space/Enter 1회 활성화를 검사한다.

- **[High] `crates/compiler/src/ir.rs:194-206`, `crates/compiler/src/element_attrs.rs:154-185` — DSL 이벤트가 click/input/change/keydown/draw로 제한된다.**
  - 영향: `onfocus`, `onblur`, `onpointerdown/up/move`가 없다. validation-on-blur, popup open/close, drag handle, form dirty 상태를 컨트롤 수준에서 표현할 수 없다. `onchange`는 값이 바뀐 blur에서만 발생해 focus event의 대체가 아니다.
  - 제안: focus/blur와 pointer event slot을 IR에 추가하고 click, Tab, programmatic focus가 하나의 dispatch 순서(old blur → new focus)를 사용하게 한다.
  - 테스트: click/Tab/programmatic focus의 blur/focus 순서와 pointer event 순서를 검사한다.

- **[High] `crates/core/src/view_tree/paint.rs:331-371`, `crates/core/src/view_tree/events.rs:263-275`, `crates/core/src/view_tree/node.rs:223` — 스크롤바는 그림이고 입력 장치가 없다.**
  - 영향: thumb는 `StyledBox`로만 그려지고 hit region/drag state가 없다. 스크롤은 wheel만 가능하며 PageUp/PageDown, 방향키 스크롤, scroll event도 없다. 수직 `scroll_y`만 있고 수평 스크롤 모델이 없다.
  - 제안: thumb geometry를 hit-test와 공유하고 drag state를 추가한다. keyboard scroll action과 scroll event를 정의하고, 수평 overflow는 구현 전까지 명시적으로 거부한다.
  - 테스트: thumb drag, track click, PageDown, focus를 유지한 keyboard scroll을 검사한다.

- **[High] `crates/core/src/view_tree/bindings.rs:39-70` — `bind:value` 외부 갱신이 입력 중 캐럿/선택을 덮어쓴다.**
  - 영향: 포커스 중 signal이 바뀌면 effect가 node text를 교체하고 caret을 clamp하며 selection anchor를 지운다. 외부 상태 동기화가 타이핑 중간에 일어나면 커서가 튀고 선택이 사라진다.
  - 제안: 포커스/composition 중에는 즉시 덮어쓰지 말고, 필요하면 이전/새 selection range를 매핑해 복원한다.
  - 테스트: focus 중 외부 bind 갱신 후 caret/selection/preedit 보존을 검사한다.

- **[High] `crates/winit/Cargo.toml`, `crates/core/src/view_tree/a11y.rs:248-282,358-390` — 접근성 트리가 OS에 연결되지 않았다.**
  - 영향: `accesskit_winit` adapter가 없고 `build_accessibility_tree`는 테스트에서만 호출된다. bounds도 scroll/transform을 반영하지 않으며 disabled/checked/selected 상태가 없다. 화면 낭독기에서는 컨트롤이 존재하지 않는다.
  - 제안: adapter를 연결하고 ActionRequest(Focus/Click/SetValue)를 처리한다. transform/scroll 반영 bounds와 컨트롤 상태를 export한다.
  - 테스트: adapter tree/action request, scroll·transform 후 bounds, disabled/value 상태를 검사한다.

- **[High] `crates/winit/src/clipboard.rs:7-38` — OS 클립보드 동기화 실패가 이후 외부 붙여넣기를 영구 차단한다.**
  - 영향: 한 번 `set_clipboard_text`의 arboard 쓰기가 실패하면 `LAST_SET_SYNCED_TO_OS=false`가 되고, 이후 `get_clipboard_text`는 OS clipboard를 읽지 않는다. 다른 앱에서 복사한 텍스트를 다시는 붙여넣을 수 없고 stale internal text만 나온다.
  - 제안: get은 항상 OS clipboard를 먼저 시도하고 internal buffer는 fallback으로만 사용한다. 실패 상태를 영구 poison flag로 쓰지 않는다.
  - 테스트: OS read 성공/실패, internal fallback, 실패 후 외부 복구를 검사한다.

### Medium

- **[Medium] `crates/winit/src/window_loop.rs:216-236` — `onkeydown`이 기본 동작을 막을 수 없다.** `dispatch_key_down`의 bool 반환을 버리고 항상 `route_key`를 실행한다. 커스텀 컨트롤이 ArrowLeft를 처리해도 캐럿이 함께 움직인다. handled 계약을 정의하고 기본 action을 건너뛰어야 한다.
- **[Medium] `crates/a11y/src/lib.rs:143-151`, `crates/core/src/view_tree/a11y.rs:276-281` — Tab 순서가 문서/페인트 순서가 아니라 slab 순서다.** `<if>` remount나 slot 재사용 후 Tab이 예측 불가능해질 수 있다. focusable order를 document-order traversal에서 만든다.
- **[Medium] `crates/core/src/view_tree/text_edit.rs:288-302` — 선택 없이 Ctrl+C를 누르면 selectable 노드 전체가 복사된다.** input은 아니지만 `selectable` paragraph에서 예상치 못한 전체 복사를 한다. 선택 range가 없으면 `None`을 반환한다.
- **[Medium] `crates/core/src/view_tree/events.rs:133-168,421-455` — double-click/triple-click/Shift+click 선택이 없다.** 단어/줄 선택과 선택 확장이 없어 데스크탑 텍스트 컨트롤의 기본 gesture가 빠져 있다.
- **[Medium] `crates/winit/src/window_loop.rs:78-93`, `examples/*` — 헤드리스 검증이 컨트롤 동작을 assert하지 않는다.** Tab 2회, activate 1회, `"A4"` 입력, 120프레임을 실행하지만 focus/선택/스크롤/disabled 결과를 검사하지 않는다. probe는 “panic 없음”만 증명한다.
- **[Medium] `crates/core/src/view_tree/paint.rs:356-369` — scrollbar 상수 24.0/12.0/7.0/6.0/4.0/18.0/0.75가 매직 넘버다.** AGENTS의 매직 넘버 금지에 해당한다. theme token 또는 named const로 옮긴다.

## 컨트롤 표면 현황

| 표면 | 현재 지원 | 기본 동작 판정 | 컨트롤 확장 전 필요한 것 |
|---|---|---|---|
| 정적 텍스트/박스 | div/section/header/footer/main/span/p/h1~h3/ul/li | 레이아웃·텍스트·selectable | multiline selection geometry, RTL |
| button | disabled, aria-label, click, Enter/Space, `:hover/:active/:focus/:disabled` | 부분 | press/release, repeat, focus/blur, form submit |
| input | text/password, value, bind:value, placeholder, disabled, IME, mouse selection | 부분 | full key matrix, horizontal scroll, undo/redo, type, validation |
| canvas | ondraw, CSS size | 그리기 전용 | pointer event, focus/a11y, fallback |
| selectable text | click/drag/Ctrl+A/C/X | 부분 | per-line hit/selection, Shift+Arrow, double-click |
| scroll container | wheel 수직 스크롤, thumb 표시 | 부분 | thumb drag, keyboard scroll, scroll event |
| 누락 | label, textarea, select/option, checkbox, radio, range, form | 없음 | 공통 value/focus/event/validation 기반 |

`input type`의 실제 상태는 다음과 같다.

| type | compile | IR | runtime | 결과 |
|---|---|---|---|---|
| text | 통과 | `is_secret_input=false` | TextInput / InputKind::Text | 정상 |
| password | 통과 | `is_secret_input=true` | PasswordInput / InputKind::Password | 마스킹 정상 |
| number/email/search | 통과 | `is_secret_input=false` | TextInput / InputKind::Text | type semantics 손실, 모바일 키보드 없음 |
| tel/date/checkbox/radio/range 등 | 오류 | — | — | 명시적 미지원 |

## 앞으로 할 일: 기본 동작 우선 로드맵

### C0. 데스크탑 입력 재현과 계측 (새 기능 금지)

- `key_info_from_winit`의 NamedKey 매핑 표와 `route_key`의 방향키/Shift/Ctrl matrix를 테스트로 고정한다.
- `examples/07_controls` 같은 headless·수동 공용 예제를 만들어 짧은/긴 input, password, disabled, selectable paragraph, scroll container, button을 한 화면에 둔다.
- 수동 체크리스트: IME off/한국어/일본어, 짧은/긴 텍스트, Left/Right/Up/Down/Home/End, Shift/Ctrl 조합, disabled, 창 포커스 이탈.
- 완료 기준: 사용자가 보고한 “방향키가 안 된다”를 짧은 텍스트·긴 텍스트·IME별로 재현 또는 비재현으로 분류하고, 실패 지점을 caret index와 PaintScene caret rect로 기록한다.

### C1. 단일행 input 완성

- no-wrap shaping, `scroll_x`, 캐럿 가시성, click/hit-test를 하나의 text model로 통합한다.
- Left/Right/Up/Down/Home/End와 Shift/Ctrl 조합의 전체 계약을 구현한다.
- Ctrl+Backspace/Delete, undo/redo, paste/IME control 문자 정규화, type 보존 또는 명시적 거부를 포함한다.
- `bind:value` 외부 갱신이 포커스 중 캐럿/선택을 보존하게 한다.
- 완료 기준: 짧은/긴 텍스트에서 모든 키가 같은 caret index와 보이는 caret rect를 내고, 키보드만으로 선택·복사·붙여넣기·undo/redo가 된다.

### C2. 포커스·이벤트·포인터 기본기

- disabled를 Tab 순서에서 제외하고 reactive disable 시 focus/IME를 정리한다.
- `onfocus/onblur/onpointerdown/up/move`와 press/release click, key repeat, `onkeydown` handled 계약을 추가한다.
- window focus loss에서 composition과 modifier/pressed 상태를 정리한다.
- 완료 기준: click/Tab/programmatic focus가 같은 blur→focus 순서를 따르고, press-outside/release·held key가 한 번만 활성화된다.

### C3. 텍스트 기하와 스크롤

- line-aware shaping, y hit-test, per-line selection, RTL visual order, `dir`/`text-align`을 구현한다.
- scrollbar drag, track click, PageUp/PageDown, keyboard scroll, scroll event를 추가한다.
- 완료 기준: 줄바꿈 문단의 2줄 클릭·드래그가 정확하고, 선택 highlight가 줄별로 그려지며, thumb drag와 키보드 스크롤이 paint와 일치한다.

### C4. 첫 컨트롤 묶음

- `label` → control focus, `checkbox`, `radio`, `textarea`, `range`를 추가한다.
- 공통 value/checked/selected 상태와 `:checked/:invalid` 상태, focus/a11y role을 먼저 설계한다.
- `textarea`는 C1/C3의 multiline text model을 재사용한다. 별도 text editor를 만들지 않는다.
- 완료 기준: label click focus, Space toggle, range arrow step, textarea multiline selection/newline, form value 전달이 동작한다.

### C5. 선택형 컨트롤·form·접근성

- `select/option`, `form`, required/validation/submit/reset을 추가한다.
- `accesskit_winit` adapter와 ActionRequest, transform/scroll bounds, disabled/checked/selected/value 상태를 연결한다.
- 완료 기준: form이 invalid submit을 막고 한 번만 submit하며, 화면 낭독기와 모바일 키보드가 type/state를 올바르게 받는다.

**C4 이전에 C1~C3을 끝내야 한다.** 그렇지 않으면 각 컨트롤이 포커스·선택·텍스트 기하·이벤트를 개별적으로 복제하게 되어 지금의 “반쪽짜리” 문제가 컨트롤 수만큼 늘어난다.

## 성능 트레이드오프

이번 리뷰에서는 성능 수치를 측정하지 않았다. no-wrap/hscroll과 line-aware shaping은 text cache key에 폭·line 정보를 추가하므로 캐시 적중률이 달라질 수 있다. 구현 후 동일 문자열·폭·스크롤 위치에서 cache miss와 frame time을 비교하고, 측정 전 최적화를 단정하지 않는다.

## 확인 필요

- 실제 앱에서 **짧은 텍스트의 Left/Right도 전혀 움직이지 않는지** 확인해야 한다. 정적 코드에는 Left/Right 경로가 있으므로, 그 경우 포커스가 input이 아닌 selectable text인지, `ime_target` preedit가 남아 있는지, input이 reactive disabled인지 먼저 계측해야 한다.
- 한국어 조합 중 OS 후보창이 Arrow/Enter를 소비하는 경우 KRui에 이벤트가 오지 않을 수 있다. OS별 실제 이벤트 순서를 기록해야 한다.
- `sync_text_input_host`가 매 redraw마다 `show_keyboard`를 호출해 `set_ime_allowed(true)`/`set_ime_purpose`를 반복하는 것이 Windows 조합을 끊는지 확인해야 한다.
- `WindowEvent::Focused(false)`에서 OS가 항상 `Ime::Disabled`를 보내는지 확인해야 한다. 보내지 않으면 preedit가 남아 방향키 전체가 막힌다.
- 긴 input 텍스트가 실제 렌더러에서 input 박스 밖으로 그려지는지는 layout/clip 설정에 따라 달라질 수 있다. C0에서 픽셀과 caret rect를 함께 확인한다.

## 테스트 제안

1. **키 매트릭스:** `key_info_from_winit` NamedKey 매핑 + `route_key` Left/Right/Up/Down/Home/End × none/Shift/Ctrl의 정확한 action.
2. **단일행 긴 텍스트:** no wrap, caret 가시성, End/Left/Right 후 caret rect, click 위치→char index.
3. **선택:** Shift+방향키, Shift+Home/End, Ctrl+A 후 축소, 줄바꿈 문단의 줄별 selection rect.
4. **편집:** Ctrl+Backspace/Delete, undo/redo, paste 개행·탭, IME commit 개행, selection replace.
5. **포커스:** disabled Tab skip, reactive disable, click/Tab/programmatic focus의 blur/focus 순서, window focus loss.
6. **포인터:** press-outside/release, press-inside/release, held Space/Enter 1회, thumb drag.
7. **컨트롤:** input type round-trip, label→focus, checkbox/radio toggle, textarea multiline, range step, form submit/validation.
8. **접근성:** adapter tree/action request, scroll·transform bounds, disabled/value/checked 상태.
9. **E2E:** 실제 예제 창에서 짧은/긴 텍스트와 IME on/off로 방향키를 누르고 caret index·caret rect·buffer를 함께 기록하는 수동/자동 체크.

## 정적으로 확인된 정상 경로

- 짧은 input에서 Left/Right/Home/End core 이동, grapheme/ZWJ emoji 삭제, Backspace/Delete selection 처리는 구현돼 있다.
- 한글 조합 중 포커스 이동 시 원래 `ime_target`으로 commit하는 경로와 일본어 변환 late commit replacement 테스트가 있다.
- `caret_rect`는 paint와 `set_ime_cursor_area`에서 같은 함수를 사용한다.
- Ctrl+A/C/X/V, Tab/Shift+Tab, Enter `onchange`, Space 입력, placeholder와 password 마스킹은 기본 경로가 있다.
- button의 click, Enter/Space 활성화, hover/active/focus/disabled CSS 상태는 존재한다.
- mouse drag로 input/selectable text의 selection anchor와 caret을 갱신하는 기본 경로가 있다.
- 현재 예제들은 `button`, `input`, `canvas`, selectable span/p만 사용하며, 컨트롤 타입·검증·포커스 이벤트를 검증하지 않는다.
