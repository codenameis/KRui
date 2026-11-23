# KRui 앞으로 할 일 (자체 완결형 작업 지시서)

> 이 문서만 보고 다음 에이전트가 작업할 수 있게 썼습니다. 배경과 결정 이유는 [`HISTORY.md`](HISTORY.md), 규칙의 원본은 [`../AGENTS.md`](../AGENTS.md)입니다. 충돌하면 `AGENTS.md`가 우선합니다.
> 기준 커밋: `f9bf919` (A8, B1~B6, 후속 수정 7단계 커밋됨). **기본 입력/컨트롤은 2026-10-26 리뷰에서 미완성으로 판정.** 줄 번호는 믿지 말고 **심볼 이름으로 다시 찾으세요.**

---

## 0. 작업 규칙 (모든 단계 공통)

1. **한 번에 한 에이전트만** 저장소를 수정합니다. 작업 중에는 예제 창을 모두 닫습니다(실행 중이면 `.exe`가 잠겨 `LNK1104`로 빌드가 실패).
2. **단계 하나 = 커밋 하나.** 커밋 메시지는 `A8: ...`, `B1: ...`, `C1: ...` 형식입니다. 각 단계를 끝낼 때마다 probe가 통과해야 커밋합니다.
   ```powershell
   pwsh -NoProfile -ExecutionPolicy Bypass -File .agents/skills/krui-harness2-loop/scripts/probe_krui.ps1
   ```
   검증용 빌드는 `CARGO_TARGET_DIR=target-review`처럼 `target-*` 디렉터리를 쓰면 `.gitignore`에 걸려 저장소가 오염되지 않습니다.
3. **"완료"의 기준**: 그 기능이 망가지면 **함께 실패하는 테스트**가 있어야 합니다. 기존 테스트가 틀린 동작을 고정하고 있으면 테스트도 고칩니다.
4. **성능 수치는 통제된 측정만** 보고합니다(release, 워밍업 후 N회, 중앙값/p99, 다른 빌드·작업이 돌고 있지 않을 때).
5. **범위를 지킵니다.** 예제를 꾸미는 작업(디자인, 모달, 레이아웃)은 범위가 아닙니다. 이 문서에 없는 기능은 보고서의 "다음 단계 제안"에 적습니다.
6. **중단하고 사람에게 질문하는 경우**: 공개 API를 깨야 하는데 근거가 없을 때, `AGENTS.md`와 충돌할 때, 한 단계가 probe를 3회 연속 통과하지 못할 때, `unsafe`가 필요해 보일 때.
7. 새 공개 항목에는 `///` 문서를 답니다. 코드와 무관한 기존 주석·문서는 지우지 않습니다.

---

## 0-1. 후속 수정 구현 기록 (`ed96853` 리뷰 → `f9bf919`)

> 리뷰 보고서: [`reviews/2026-10-26-ed96853-a1-b6-independent.md`](reviews/2026-10-26-ed96853-a1-b6-independent.md)
>
> **2026-10-26 상태 갱신:** HANDOFF에 따르면 아래 7단계는 `f9bf919`까지 커밋됐고 probe 전체 통과가 보고됐습니다. 다만 이번 리뷰어는 기본 입력/컨트롤만 정적 검토했으며 7단계 전체를 독립 재승인하지 않았습니다. 구현을 반복하지 말고, 후속 작업 전에 `target-review`에서 전체 재검증하세요.

1. **`B-fix-1`: IME 대상과 B5 계약**
   - `ime_target`을 empty preedit과 remount 이후에도 최종 Commit/Disabled까지 유지하고, programmatic focus도 공통 blur/auto-commit 경로를 사용하게 합니다.
   - blur에서 이미 삽입한 preedit과 다른 late Commit은 중복 추가하지 말고 auto-inserted range를 최종 텍스트로 교체합니다. 빈 Commit의 pin 정리도 결정합니다.
   - `EditingState`와 caret을 같은 host target에서 읽고, `focused_ime_state`의 문서와 동작을 맞춥니다.
   - B5의 `FinishComposing`, `DeleteSurrounding`, `SetSelection`, `EditorAction`, `InputKind`, keyboard show/hide를 상태 기계에 완성합니다.
   - 완료 테스트: 한글 조합 중 programmatic focus, 일본어 변환 late commit, 빈 commit, pending blur 중 preview reload, host sync, delete-surrounding/selection/editor action.
2. **`B-fix-2`: preview 평가와 원자적 reload**
   - `i64::MIN` neg/div를 checked 오류로 처리하고, macro와 interpreter가 signal arithmetic, overflow, 0 나누기, 논리 연산, type error를 같게 처리하게 합니다.
   - 미등록 Rust method 직접 호출을 막고, lookup/call 실패를 `Unit`으로 삼키지 않습니다.
   - document 전체를 preflight하고 성공할 때만 기존 tree/scope를 교체합니다. 실패 시 직전 화면과 반응성을 유지하고 `Reloaded`를 전송하지 않습니다.
   - macro와 interpreter가 같은 structural key를 만들고, global ID 우선 복원을 제거합니다.
   - 완료 테스트: 평가 parity, 모든 binding 종류와 inactive branch의 registry rollback, macro→preview 첫 reload, key reorder/parent 이동/focus/scroll/preedit.
3. **`B-fix-3`: 실제 `krui serve` 경로와 device frame**
   - 실제 예제의 `views/*.krui`와 기본 경로를 일치시키고, 시작·재로드 읽기 실패를 진단으로 표면화합니다.
   - 연결별 누적 buffer로 fragmented/coalesced TCP와 partial write를 처리하고, UI 스레드의 socket/file/compile blocking을 worker + wakeup으로 옮깁니다.
   - `.rs`/manifest 변경 감지, 성공 시 rebuild/restart, 실패 시 기존 process 유지, IR/protocol version mismatch restart를 구현합니다.
   - CLI 인자와 child 실패를 nonzero exit로 전파합니다.
   - device frame은 실제 screen 크기·DPI·safe-area·clip·pointer 역변환을 적용하고, 초기 overlay 상태를 연결 시 재생합니다.
   - 완료 테스트: 실제 예제로 `.krui` 저장 후 1초 안에 상태 유지 reload, `.rs` 저장 후 restart, TCP 분할/backpressure/slow peer, 잘못된 인자와 child crash, preset별 픽셀·입력.
4. **`A-fix-1`: reactive 수명과 실패 처리**
   - `active == false`인 ROOT poller 제거, `<if>`/`<for>` anchor 직접 unmount 시 branch/item/empty scope 정리, disposed-signal tombstone의 bounded ownership을 구현합니다.
   - 재진입 `Signal::update`가 조용히 손실되지 않게 지연하거나 명시적 실패를 반환하고, 실패 시 subscriber를 깨우지 않습니다.
   - 완료 테스트: ROOT poller 완료, anchor unmount, 중첩 update, `<if>`/`<for>` 각 1,000회 후 runtime counter와 disposed fallback 보존 수까지 정확히 일치.
5. **`A-fix-2`: 렌더링 불변식과 resource guard**
   - 0 viewport limit 패닉을 제거하고, Lyon/glyph 생성 전후에 vertex/index 한도를 실제로 강제합니다.
   - transformed clip/culling과 wheel hit-test를 paint와 같은 누적 행렬·clip·paint order로 통일합니다.
   - TinySkia physical framebuffer, external view DPI, IME caret→paint의 glyph scale을 일치시킵니다.
   - readback `Option` 계약과 staging buffer 재사용, atlas 1024² 정책, `.notdef` 1회 경고, 실제 UI 픽셀을 검사하는 offscreen oracle을 완성합니다.
   - 완료 테스트: translated/rotated clip, clipped/overlapping wheel, 2x 픽셀, caret→paint 순서, 0/작은 limits, 고정 좌표 UI 픽셀.
6. **`A-fix-3`: CSS cascade와 진단 출처**
   - pseudo-class specificity를 class 항에 포함하고, dynamic class는 전체 class set으로 compound selector와 source order를 해석합니다.
   - CSS 변수 cycle/undefined에 inline/external declaration의 실제 파일·줄·열을 연결합니다.
   - 완료 테스트: `.base` 대 `button:hover`, `.x.y`, 다중 dynamic class, inline 우선, external 변수 오류 출처를 macro/preview 모두 확인.
7. **`A-fix-4`: probe와 코드 크기 정리**
   - p99≤8.33ms를 TinySkia/Wgpu 각각에 적용하고, 측정 모드에서 GPU completion 또는 timestamp를 확인합니다. backend unavailable을 전체 합격과 구분합니다.
   - `cargo tree`의 not found와 다른 실패를 구분하고 preview graph positive 검사를 추가하며, 예제 06을 포함한 모든 예제의 실제 프레임 수를 gate로 확인합니다.
   - 동작 유지 테스트를 붙여 500줄 초과 파일과 60줄 초과 함수를 책임별로 나눕니다.
   - 완료 테스트: 느린 p99/cargo 실패/프레임 부족 주입 시 probe nonzero, 최종 probe 전체 통과, release 측정 환경과 p50/p99/max/cold/warm 기록.

---

## 0-2. 현재 최우선: 기본 입력과 컨트롤 순서 (`f9bf919`)

> 리뷰 보고서: [`reviews/2026-10-26-f9bf919-input-controls.md`](reviews/2026-10-26-f9bf919-input-controls.md)
>
> 사용자 확인 증상: 실제 텍스트 박스에서 방향키가 동작하지 않는 것처럼 느껴지고 컨트롤이 전반적으로 반쪽짜리입니다. 원인은 하나가 아니라 **키 매트릭스 누락 + 단일행 input의 wrap/캐럿 고정 + 선택·포커스·이벤트 기반 부족**이 겹친 상태입니다. 새 컨트롤을 추가하기 전에 C0~C3을 먼저 끝냅니다.

### C0. 데스크탑 입력 재현과 계측 (기능 추가 금지)

- `key_info_from_winit`의 NamedKey 매핑 표와 `route_key`의 Left/Right/Up/Down/Home/End × none/Shift/Ctrl matrix를 테스트로 고정합니다.
- `examples/07_controls`(이름은 조정 가능)를 만들어 짧은/긴 input, password, disabled input/button, selectable paragraph, scroll container, button을 한 화면에 둡니다.
- 수동 체크리스트를 만듭니다: IME off/한국어/일본어, 짧은/긴 텍스트, 방향키 전체, Shift/Ctrl 조합, disabled, 창 포커스 이탈.
- **완료 기준**: 사용자가 보고한 실패를 짧은 텍스트·긴 텍스트·IME 상태별로 재현/비재현으로 분류하고, 실패 시 caret index와 PaintScene caret rect를 함께 기록합니다.
- **실패해야 하는 테스트**: `key_info_named_key_mapping`, `route_key_arrow_modifier_matrix`, `example_07_control_acceptance`.

### C1. 단일행 input 완성

- TextInput/PasswordInput은 no-wrap으로 shaping하고 `scroll_x`를 도입해 glyph origin, hit-test, selection rect, caret_rect에 같은 offset을 적용합니다.
- Left/Right/Up/Down/Home/End와 Shift/Ctrl 조합의 전체 계약을 구현합니다. 단일행 Up/Down은 no-op 또는 Home/End 중 하나를 명시적으로 선택합니다.
- Ctrl+Backspace/Delete, undo/redo, selection replace, paste/IME의 개행·탭 정규화를 구현합니다.
- `input type`을 IR/runtime에 보존해 `InputKind`로 연결하거나, 구현 전 type은 `CompileError`로 거부합니다.
- `bind:value` 외부 갱신이 포커스 중 caret/selection/preedit을 보존하게 합니다.
- **완료 기준**: 짧은/긴 텍스트 모두에서 키 입력 후 caret index와 보이는 caret rect가 일치하고, 키보드만으로 선택·복사·붙여넣기·undo/redo가 됩니다.
- **실패해야 하는 테스트**: `long_input_does_not_wrap_and_caret_stays_visible`, `shift_arrow_extends_selection`, `ctrl_word_navigation_and_deletion`, `undo_redo_text_transactions`, `single_line_rejects_newline_tabs`, `input_type_roundtrip_macro_preview`, `external_bind_preserves_caret_selection`.

### C2. 포커스·이벤트·포인터 기본기

- disabled를 Tab 순서에서 제외하고, reactive disable 시 IME blur/onchange 후 focus를 정리합니다.
- `onfocus`, `onblur`, `onpointerdown/up/move`를 IR에 추가하고 click/Tab/programmatic focus가 하나의 blur→focus 순서를 사용하게 합니다.
- pointer press/release를 pair로 처리해 release-inside에서 click을 발생시키고, `KeyEvent.repeat`을 전달해 Enter/Space activation 반복을 막습니다.
- `onkeydown`의 handled 계약을 정해 기본 action을 막을 수 있게 합니다.
- 창 포커스 이탈 시 composition, modifier, pressed/hover 상태를 정리합니다.
- **완료 기준**: disabled control이 Tab에 나오지 않고, click/Tab/programmatic focus가 같은 event 순서를 내며, press-outside/release와 held key가 command를 한 번만 실행합니다.
- **실패해야 하는 테스트**: `disabled_nodes_are_skipped_by_tab`, `focus_blur_order_on_click_and_tab`, `press_outside_release_does_not_click`, `held_space_activates_once`, `onkeydown_can_prevent_default`, `window_focus_loss_clears_ime_and_mods`.

### C3. 텍스트 기하와 스크롤

- `ShapedTextLayout`에 line index/top과 per-line x offsets를 보존하고 `hit_test(x, y)`를 제공합니다.
- selectable paragraph의 줄별 selection rect, RTL visual order, `dir`/`text-align`을 구현합니다.
- scrollbar thumb hit region과 drag, track click, PageUp/PageDown, keyboard scroll, scroll event를 추가합니다. 수평 overflow는 구현 전까지 명시적으로 거부합니다.
- **완료 기준**: 줄바꿈 문단의 2줄 클릭·드래그가 정확하고, selection highlight가 줄별로 그려지며, thumb drag와 keyboard scroll이 paint와 일치합니다.
- **실패해야 하는 테스트**: `wrapped_text_hit_test_per_line`, `multi_line_selection_paints_per_line`, `rtl_caret_matches_visual_order`, `scrollbar_drag_updates_scroll_y`, `page_down_scrolls_without_moving_focus`.

### C4. 첫 컨트롤 묶음 (C1~C3 완료 후)

- `label` → control focus, `checkbox`, `radio`, `textarea`, `range`를 추가합니다.
- 공통 value/checked/selected 상태, `:checked/:invalid`, focus/a11y role을 먼저 설계합니다.
- `textarea`는 C1/C3의 multiline text model을 재사용합니다. 별도 editor를 만들지 않습니다.
- **완료 기준**: label click focus, Space toggle, range arrow step, textarea multiline selection/newline, form value 전달이 동작합니다.
- **실패해야 하는 테스트**: `label_click_focuses_control`, `checkbox_space_toggles`, `radio_group_single_selection`, `range_arrow_step`, `textarea_multiline_editing`.

### C5. 선택형 컨트롤·form·접근성

- `select/option`, `form`, required/validation/submit/reset을 추가합니다.
- `accesskit_winit` adapter와 ActionRequest(Focus/Click/SetValue), transform/scroll bounds, disabled/checked/selected/value 상태를 연결합니다.
- **완료 기준**: invalid form submit이 차단되고 submit은 한 번만 발생하며, 화면 낭독기와 모바일 키보드가 type/state를 올바르게 받습니다.
- **실패해야 하는 테스트**: `select_keyboard_and_option_value`, `form_required_validation_and_submit_once`, `accesskit_adapter_handles_action_request`, `a11y_bounds_include_scroll_and_transform`, `mobile_input_kind_per_type`.

**금지:** C1~C3 이전에 checkbox/select/textarea를 먼저 추가하지 않습니다. 각 컨트롤이 포커스·선택·텍스트 기하를 개별 복제하면 현재의 반쪽짜리 문제가 컨트롤 수만큼 늘어납니다.

아래 1~3장은 원래 작업 지시와 실행 재검증 체크리스트입니다. 현재 우선순위는 위 `C0`~`C5`이며, 과거 지시를 새 구현 작업으로 반복하지 마세요.

---

## 1. A1~A7 독립 검증 체크리스트 (2026-10-26 정적 리뷰 완료, 실행 재검증 필요)

A1~A7은 2026-10-26에 정적 독립 리뷰를 마쳤고, 당시 미해결 항목은 `A-fix`/`B-fix`로 구현됐습니다. 아래 표는 과거 체크리스트의 보존본입니다. 현재 입력/컨트롤 판정과 실행 검증은 위 `C0`부터 다시 시작하며, 과거 probe 통과를 현재 HEAD의 합격 근거로 쓰지 않습니다.

### 1-1. 과거 환경 기록 (HEAD `a76d8f8`, 새 측정 아님)
> **2026-10-26 확인(HEAD `a76d8f8`, 별도 `target-review` 빌드)**: `probe_krui.ps1`은 5단계 모두 통과했습니다(exit 0). 새 `krui-bench`(release, 워밍업 후 측정)의 결과는 다음과 같습니다. 측정 중에 다른 작업이 돌고 있지 않았는지는 확인하지 못했으므로, **새 환경에서 같은 방식으로 다시 측정해 기준값으로 삼으세요.**
>
> | 백엔드 | 첫 프레임(콜드/웜) | p50 | p99 | max | 애니메이션 중 relayout |
> |---|---|---|---|---|---|
> | TinySkia | 22.7ms / 8.2ms | 1.90ms | 3.09ms | 3.21ms | 0 |
> | Wgpu | 9.8ms / 7.8ms | 0.25ms | 0.45ms | 0.48ms | 0 |
>
> 첫 프레임이 이전(45~95ms)보다 크게 줄었습니다. 단, Wgpu의 p50 0.25ms는 GPU 완료를 기다리지 않는 CPU 인코딩 시간일 가능성이 있으니 `krui-bench`가 GPU 대기(`device.poll(Wait)`)를 측정 모드에서 쓰는지 확인하세요.

- [ ] `probe_krui.ps1`이 처음부터 끝까지 통과하는가 (예제 창이 모두 닫힌 상태에서)
- [ ] `cargo tree -p krui -e normal,no-proc-macro -i krui-compiler`가 "not found"로 실패하는가, `--features preview`에서는 찾아지는가
- [ ] `git status`가 깨끗한가 (`target-review/`가 추적되지 않는가)

### 1-2. 항목별 검증

| 단계 | 확인할 것 (실패하면 `A-fix`) |
|---|---|
| A1 테마 | core/renderer에 `Color::rgb(` 리터럴 없음(테스트·const 기본값 제외). 같은 문서를 프리셋 4종으로 전환하면 버튼 배경이 토큰 값과 정확히 같음. 사용자가 `#save { background: #f00 }`을 주면 어떤 프리셋에서도 유지됨. `use_theme_override` 삭제됨. 태그 기본값(h1 크기, 패딩, radius)이 Rust 매직 넘버가 아니라 프리셋 CSS에 있음 |
| A2 IME | 한국어: preedit `한` → 버튼 클릭(포커스 이동) → `Preedit("")`+`Commit("한")` → **이전 입력칸에 `한`**, 버튼 쪽 변화 없음. 조합 중 Tab 이동 시 이전 칸에 커밋. 영어: IME disabled에서 `l`,`l` → `ll`. AltGr+Q가 문자 입력. 포커스 없을 때 Esc가 앱을 종료하지 않음. 숫자 키 테마 전환이 프레임워크에서 제거됨. `caret_rect`가 페인트와 `set_ime_cursor_area`에서 같은 함수 사용. 입력칸 포커스 in/out에서 `set_ime_allowed` 호출 |
| A3 소유권 | `pending_mount_scope` 삭제됨(명시적 `mount(\|\| ..)`). Store를 `EngineSession::new` 뒤에 만들어도 `transition_to` 후 살아 있음. 스코프 해제 시 부모 `owned_children`에서 제거됨. Effect 안의 Effect가 재실행마다 정리됨. 해제된 `Signal::get`이 **다른 시그널 값을 반환하지 않음**(첫 시그널 폴백 삭제). poller 실행 중 새 Store를 만들어도 패닉 없음(`mem::take`). `debug_stats()`에 `live_scopes`, `async_pollers`, 콜백 vec 길이, Taffy 노드 수, slab 길이가 있고, `transition_stress`가 **여유분(`+4`) 없이 전체 일치** 검사. `<if>` 1000회·`<for>` 1000회 스트레스 있음 |
| A4 probe | 릴리즈 파서 검사가 `cargo tree` 방식. clippy를 `--features preview`로도 실행. 경로 하드코딩 없음. `krui-bench`(또는 `--bench-frames`)가 release, 워밍업 30 + 측정 300프레임, 중앙값/p99/max/첫 프레임(콜드·웜) 출력, **p99 ≤ 8.33ms 게이트**(TinySkia·Wgpu 각각). 헤드리스가 최소 120프레임을 돌고 위반 시 0이 아닌 종료 코드. 120프레임 relayout 0회 테스트에 "애니메이션이 실제로 돌았는지" assert 있음 |
| A5 렌더러 | 스크롤 컨테이너 밖 텍스트·캔버스 픽셀이 그려지지 않음(TinySkia 픽셀 테스트). 아틀라스가 R8이고 `CacheKey` 키이며 프레임 중간 초기화 없음. 색만 바꿔 다시 그려도 아틀라스 업로드 0바이트. 텍스트 캐시가 진짜 LRU. 가짜 회색 박스 폴백 삭제됨(`.notdef` + 1회 경고). 감마 `powf(0.80)` 삭제됨. tiny-skia가 `scale_factor`를 반영하고 premultiplied 블렌딩이 올바름. `framebuffer_rgba() -> Option<&[u8]>`이고 readback 버퍼 재사용 |
| A6 레이아웃 | `unmount`가 Taffy 노드를 실제로 제거함(`transition_stress`가 Taffy 노드 수 검사). 히트 리스트가 페인트 순서·누적 행렬·clip을 사용함. 해제된 슬롯을 재사용한 모달이 아래 노드보다 먼저 히트됨. 스크롤로 가려진 항목이 클릭되지 않음. `:active { transform }`의 히트 영역이 페인트와 같음. 사용자 경로의 `expect`/`unreachable!` 제거 |
| A7 컴파일러 | `each="{(i, x) in v}"` 등이 패닉 없이 `CompileError`/`syn::Error`. `<style>` 안의 `{x}`는 명확한 에러. CSS 에러가 소스별 경로·줄·열을 보고함. `id`/`class`/`aria-label` 안의 `{…}` 처리. 중복 속성·알 수 없는 `type`·`selectable` 값이 에러. 상태 규칙 캐스케이드가 특이도·순서대로(`#save{}` > `button:hover{}`, 인라인 최우선). 엔티티 디코딩이 보간 분리 뒤. 중괄호 없는 `onclick="vm.save"` 에러. CSS 변수 순환 에러. 미리보기가 컴파일 에러를 삼키지 않음 |

### 1-3. 추가 항목의 2026-10-26 정적 확인 결과
- 미리보기 remount는 `preview_remount.rs`로 분리됐고 `preview` feature로 제한됩니다. 단, pending IME target 보존은 실패이므로 `B-fix-1`에서 수정합니다.
- 예제의 `Rc<dyn Fn>`는 제거되고 `CanvasDraw`와 콜백 크기 전달이 적용됐습니다.
- 60줄 초과 함수와 500줄 초과 파일은 남아 있습니다. `emit_node_primitives_recursive` 약 380줄, `window_event` 약 200줄, `mount_for` 약 130줄이며, 500줄 초과 파일은 18개입니다 → `A-fix-4`.
- 매직 넘버 정리 대상: `paint.rs`(40.0, 4.0, 1.28, 12.0, 1.22, 8.0, 5.0 …), `node.rs`(`extra_width` 16/4), `layout`(8.0), `text`(8.0, 0.6, 1.6), `frame_clock.rs`(0.1, 0.01, 1.3, 0.25, 0.001), `derived.rs`(`1_000` depth). 각 값의 의미를 확인한 뒤 `const` 또는 테마 토큰으로 옮깁니다 → `A-fix-4`.
- `for` diff는 `control_flow.rs` `mount_for`에서 반복 `any`/`find_map` 탐색을 사용합니다. 실제 병목 여부는 100/1,000/10,000개 항목의 release 측정 후 `HashMap<K, idx>` 도입을 결정합니다.
- 입력칸의 `placeholder`는 value와 분리돼 있습니다.
- 비동기 poller는 callback이 `false`를 반환한 뒤에도 ROOT 스코프에 남는 문제가 확인됐습니다 → `A-fix-1`.

---

## 2. A8. 플랫폼 준비와 레이어 정리 (`a4c0aeb` 커밋됨, 리뷰 불합격)

1. **wgpu 백엔드**
   - `Backends::all()`을 쓰고, limits는 `adapter.limits()`/`downlevel_defaults`(GLES) 중에서 선택합니다. `PowerPreference`는 설정 가능하게 합니다. Raspberry Pi에서 GL 백엔드로 동작할 코드 경로를 만듭니다(실기 검증은 M2).
   - 기본 아틀라스 크기는 1024²(필요하면 확장)입니다.
2. **임베드 API**
   - `WgpuRenderer::from_device(device, queue, target_format)`
   - `render_to_view(&TextureView, size, scale_factor)`
   - 게임 엔진·VR 호스트가 자기 Device로 KRui를 그릴 수 있어야 합니다.
   - winit 없이 동작하는 헤드리스 예제 `examples/06_embed_offscreen`을 추가합니다. core 이벤트 주입 API로 입력을 넣고, 텍스처에 그린 뒤 readback해서 검증합니다.
3. **창·전력**: 최소 창 크기 강제(640×480, 320×240)를 삭제합니다. 유휴 상태에서는 포커스된 입력칸이 있을 때만 캐럿 깜빡임으로 깨어나고, 그 외에는 `ControlFlow::Wait`입니다(유휴 CPU 0%).
4. **중복 제거**: `src/main.rs`가 복제한 이벤트 루프를 삭제하고 `krui-winit` 루프를 씁니다. 쇼케이스 전용 로직은 앱 코드로 옮깁니다.
5. **의존 방향**: `krui-core`가 `krui-patterns`에 의존하지 않게 합니다. core에 작은 트레이트(예: `ClickTarget { fn invoke(&self); fn enabled(&self) -> Option<Signal<bool>>; }`)를 두고 patterns가 구현합니다.
6. **공개 API 정리**: 사용자 코드에서 `Rc<dyn Fn>`를 없앱니다. `set_canvas_draw(impl Fn(&mut CanvasContext2D, Size) + 'static)` + `CanvasDraw` 뉴타입을 쓰고, 예제의 `pub on_canvas_draw: Rc<dyn Fn…>` 필드를 모두 교체합니다. 캔버스 콜백에 노드 크기를 넘깁니다.
7. **미리보기 코드 격리**: 미리보기 전용 remount 지원 코드는 `#[cfg(feature = "preview")]`로 막고 적절한 모듈로 옮깁니다(B4에서 다시 작성).
8. **코드 크기 정리**: 60줄 넘는 함수, 500줄에 걸친 파일, 매직 넘버를 `const`/테마 토큰으로 정리합니다(1-3 목록 기준).
9. **문서 갱신**: `README.md`(존재하지 않는 `m0_showcase` 예제, 옛 `krui-mvvm` 이름), `ARCHITECTURE.md`(font 속성의 "Paint Fast-Path" 분류 오류 등), `CHANGELOG.md`, `.agents/skills/krui-harness2-loop/SKILL.md`(오래된 "Harness²/5대 가드레일" 설명)를 `AGENTS.md`와 현재 구조 기준으로 고칩니다. 플랫폼 연결 크레이트용 "unsafe 목록" 표를 README에 준비합니다(현재는 비어 있음).

**완료 기준**: `cargo tree -p krui-core`에 `krui-patterns`·winit·wgpu가 없음, `06_embed_offscreen` 헤드리스 통과, 유휴 상태에서 이벤트 루프가 깨어나는 횟수 측정(입력 포커스 없음 → 0회), probe 전체 통과.

---

## 3. Phase B. M1 기반 (B1~B6 커밋됨, 후속 수정 완료 후 재검토)

목표: Dioxus·Flutter처럼 **앱 상태를 유지한 채** `.krui`/`.css` 수정을 실행 중인 앱에 반영합니다. 데스크탑과 디바이스 프레임까지 하고, 모바일은 M2입니다.

### B1. 템플릿 식 언어 (`AGENTS.md` 3장)
- `krui-compiler`에 `TemplateExpr` AST와 파서를 추가합니다.
  - 허용: 경로, 리터럴(문자열·숫자·불리언), 단항 `!`/`-`, 이항 `== != < <= > >= && || + - * /`, 괄호, `<for>` 항목 변수, 등록된 메서드·명령 호출(인자도 식), enum 경로(`Msg::Add`).
  - 그 밖의 문법(클로저, 블록, `match`, 임의 함수 호출)은 위치 정보가 있는 `CompileError`입니다.
- 매크로는 `TemplateExpr`에서 Rust 코드를 생성합니다("Rust 식을 그대로 붙여 넣는" 방식을 대체).
- 기존 예제의 바인딩이 이 언어를 벗어나면 ViewModel 쪽으로 옮깁니다.
- 테스트: 허용·금지 문법 표 기반 테스트. B4 이후 같은 식을 매크로와 인터프리터로 평가한 결과가 같은지 검사.

### B2. 이름 레지스트리
- `#[view_model]`, `Store`, Controller(`#[controller]` 매크로 신설)가 `Registry`를 구현합니다.
  - `get(path) -> Option<Value>`, `signal(path) -> Option<DynSignal>`, `call(name, args) -> Result<Value, CallError>`
  - `Value`: `Bool`, `Int`, `Float`, `Str`, `List(Vec<Value>)`, `Record(RegistryRef)`, `Command`, `Unit`
- 레지스트리 코드는 **`preview` feature가 켜졌을 때만 생성**합니다(릴리즈 비용 0).

### B3. IR 직렬화
- `CompiledDocument`, `CompiledNode`, `CompiledStyle`, `HtmlTag`, `TemplateExpr`, `TokenTable`에 `serde`를 적용합니다. `krui-compiler`/`krui-style`의 `serde` feature는 `preview`일 때만 켭니다.
- 포맷은 `postcard` 또는 `bincode`이고, 버전 필드를 넣습니다. 프로토콜이 맞지 않으면 hot restart를 요청합니다.

### B4. 진짜 미리보기 인터프리터
- `krui-preview` 크레이트(preview 전용)가 IR + 레지스트리로 트리를 마운트합니다. `If`/`For`/텍스트 보간/이벤트/paint·layout 바인딩을 **모두** 인터프리터가 평가합니다. 예전 Rust 클로저를 재사용하는 방식은 삭제합니다.
- **다시 마운트할 때의 identity**: `(부모 경로, 태그, id, for-key, 형제 내 정적 위치)`로 만든 **구조 경로 키**를 씁니다(전역 순번 금지). 키가 일치하는 노드는 포커스·스크롤·IME 상태를 유지하고, 이전 스코프는 정확히 해제합니다.
- 레지스트리와 맞지 않는 변경(없는 이름 등)은 오버레이로 에러를 표시하고 직전 화면을 유지합니다.
- **게이트 테스트**: 같은 `.krui`를 매크로 경로와 인터프리터 경로로 마운트해서 `PaintScene`이 같은지 비교합니다(`<if>`, `<for>`, 상태 스타일, 토큰 포함). 리로드 100회 후에도 `debug_stats()`가 같아야 합니다. 이것이 "미리보기 = 릴리즈"(AGENTS 1.2)의 게이트입니다.

### B5. IME 추상화 (모바일 대비)
- `krui-core`에 둡니다.
  - `EditingState { text, selection: Range, composing: Option<Range> }`
  - `ImeEvent { SetComposing { text, cursor }, Commit(text), FinishComposing, DeleteSurrounding { before, after }, SetSelection(range), EditorAction(kind) }`
  - `trait TextInputHost { fn set_editing_state(&mut self, &EditingState); fn set_caret_rect(&mut self, Rect); fn show_keyboard(&mut self, InputKind); fn hide_keyboard(&mut self); }`
  - 내부 인덱스는 UTF-8이고, UTF-16 변환 헬퍼를 제공합니다(Android, iOS, Windows).
- `krui-winit`은 `TextInputHost`의 데스크탑 구현이 됩니다. A2의 상태 기계를 이 구조로 옮깁니다. 임베드 예제에는 `TextInputHost` mock을 붙여 IME 시나리오를 돌립니다.
- **구현 전 사전 조사**(보고서에 결과 기재): winit 0.31의 `request_ime_update`/`ImeCapabilities` 상태, Android(`android-activity` `TextInputState`)와 iOS에서 winit이 preedit을 전달하는지 **소스 코드로** 확인.

### B6. `krui serve` (데스크탑 + 디바이스 프레임)
- `krui-cli` 크레이트에 `krui serve [--example NAME] [--frame iphone-15|pixel-8|rpi-800x480]` 명령을 둡니다.
  - 앱을 `--features preview`로 빌드·실행하고, 파일을 감시하다가 바뀌면 compile → IR 직렬화 → 로컬 WebSocket(또는 TCP)으로 보냅니다.
  - 명령: `reload(ir)`, `restart`, `inspect_tree`, `set_overlay(relayout|fps|hit)`.
- 디바이스 프레임 모드는 데스크탑 창에 기기 해상도, `scale_factor`, safe-area, 베젤을 그리고 그 안에 앱을 렌더링합니다.
- 레지스트리 형태가 바뀌는 Rust 변경은 자동으로 다시 빌드·재시작(hot restart)합니다.
- **완료 기준**: 예제 01~05에서 `.krui`를 수정·저장하면 1초 안에 상태가 유지된 채 반영됩니다(사람이 확인해 보고서에 기록).

---

## 4. M2 이후 로드맵 (지금 실행하지 않음, 설계를 막지 않도록 참고)

| 항목 | 방향 |
|---|---|
| Android 실행·핫리로드 | `krui serve --platform android`: 빌드, 설치, `adb reverse`, 같은 프로토콜. iOS 시뮬레이터는 `localhost` |
| Android IME | Kotlin `InputConnection` shim(`setComposingText`, `commitText`, `finishComposingText`, `deleteSurroundingText`, `requestCursorUpdates` → `updateCursorAnchorInfo`). Rust 쪽은 `jni` pull 방식(`krui-android-sys`, 직접 unsafe 약 2줄: raw VM/Activity 포인터 변환) 또는 UniFFI(`krui-android-ffi`, 직접 unsafe 0줄). 선택은 구현 전에 비교 후 결정(질문 사항 참조). GameTextInput은 삼성 키보드 한글 문제로 기본값에서 제외 |
| iOS 실행·IME | Swift `UITextInput` 뷰 + UniFFI 또는 swift-bridge(`krui-ios-ffi`). objc2 `define_class!`는 쓰지 않음 |
| 플랫폼 연결 크레이트 규칙 | `AGENTS.md` 1.1 그대로: 직접 unsafe 10줄 이내, `// SAFETY:`, README 표 기록, 안전한 대안 검토 결과를 PR에 기재 |
| Raspberry Pi | wgpu GL 백엔드 또는 TinySkia + Cosmic을 실기에서 통제된 측정으로 확인 |
| 접근성 | `accesskit_winit` 어댑터 연결, `ActionRequest`(Focus/Click/SetValue) 처리, 변환·스크롤을 반영한 bounds, disabled·heading level 상태 |
| 컴포넌트 | `Card.krui` → `<Card title="{..}">…</Card>`, `<slot/>`, props. M1에서는 IR에 "제어 흐름 노드"와 "컴포넌트 노드"를 넣을 수 있는 enum 구조만 설계해 둠 |
| 렌더러 심화 | 노드별 메시 캐시(transform·opacity는 uniform만 갱신), 그룹 opacity(오프스크린 레이어), GPU AA(MSAA/SDF), 컬러 이모지, DPI별 글리프 래스터, 첫 프레임 시간(폰트 DB 스캔) 단축 |
| 텍스트 심화 | `Buffer::hit`/`LayoutRun` 기반 커서·히트테스트(줄바꿈, RTL, grapheme), 서브픽셀 |
| 핫리로드 심화 | Rust 코드 바이너리 패치(subsecond류, 선택, 마지막) |
| 디자인 시스템 | Fluent/Material/Liquid Glass/Cosmic 프리셋 완성도 점검(토큰만으로 표현), Cosmic은 블러·그림자·반투명 없음, TinySkia/Pi에서 기본 선택 |

---

## 5. 사람이 정해야 할 질문 (구현 전 확인)

1. **Android Rust 연결 방식**: `jni` pull 방식(직접 unsafe 약 2줄, 빌드 단순) vs UniFFI(직접 unsafe 0줄, 도구 체인 추가). 어느 쪽을 기본으로 할지.
2. **직렬화 포맷**: `postcard`(작고 빠름) vs `bincode` vs JSON(디버깅 쉬움). 프로토콜 버전 정책.
3. **winit 버전**: 0.30에 머물지, 0.31(`request_ime_update`)로 올릴지. 안정판 여부 확인 후 결정.
4. **컴포넌트 문법 시기**: M1에 포함할지 M2로 미룰지.
5. **라이선스 고지**: Flutter(BSD-3) 코드를 직접 복사하지 않고 아이디어만 쓰고 있는지 PR마다 확인.

---

## 6. 단계별 보고 형식

```
## <단계> 완료 보고 (커밋 해시)
- 변경 파일: 경로 — 한 줄 이유
- 새/수정 테스트: 이름 — 무엇이 망가지면 실패하는지
- probe: fmt / clippy(default+preview) / test 수 / cargo tree 검사 / debug_stats 비교 결과
- 성능: release p50 / p99 / max, 웜·콜드 첫 프레임 (TinySkia, Wgpu) — 통제된 측정만
- 지시서와 다르게 한 부분과 이유
- 남은 위험 / 다음 단계 제안
```
