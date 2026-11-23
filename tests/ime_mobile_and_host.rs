//! Extended mobile/host IME (`B5` & `B-fix-1`) verification tests:
//! - `TextInputHost` `EditingState` synchronization and UTF-8 <-> UTF-16 offset/range round-trips
//! - Programmatic focus (`set_focus_to_node`), converted late commits (`にほんご` -> `日本語`), and empty commits
//! - Mobile `FinishComposing`, `DeleteSurrounding`, `SetSelection`, and `EditorAction` (`Done`/`Next`)

#![forbid(unsafe_code)]

use krui::*;

fn mount_single_input_session(text_sig: Signal<String>) -> EngineSession {
    let mut session = EngineSession::new(800, 300, RenderBackendPreference::ForceTinySkia);
    let fragment = krui!(
        r#"
        <div style="padding: 20px; background-color: #0f172a">
            <input id="ime-input" style="width: 480px; height: 44px; padding: 8px 12px" bind:value="{text_sig}" />
        </div>
        "#
    );
    session.tree.mount(fragment);
    session.step_frame(1.0 / 120.0);
    let focused = session.tree.dispatch_key_tab(false);
    assert!(focused.is_some());
    session
}

#[derive(Default)]
struct MockImeHost {
    allowed: bool,
    input_kind: Option<InputKind>,
    cursor_area: Option<LayoutRect>,
    editing_state: Option<EditingState>,
}

impl TextInputHost for MockImeHost {
    fn set_ime_allowed(&mut self, allowed: bool) {
        self.allowed = allowed;
    }

    fn set_ime_cursor_area(&mut self, rect: LayoutRect) {
        self.cursor_area = Some(rect);
    }

    fn update_editing_state(&mut self, state: &EditingState) {
        self.editing_state = Some(state.clone());
    }

    fn show_keyboard(&mut self, kind: InputKind) {
        self.allowed = true;
        self.input_kind = Some(kind);
    }

    fn hide_keyboard(&mut self) {
        self.allowed = false;
        self.input_kind = None;
    }
}

fn assert_utf8_utf16_korean_and_emoji_roundtrips() {
    let sample = "한글🚀A👨\u{200D}👩\u{200D}👧끝";
    assert_eq!(utf8_to_utf16_offset(sample, 0), 0);
    assert_eq!(utf8_to_utf16_offset(sample, 3), 1);
    assert_eq!(utf8_to_utf16_offset(sample, 6), 2);
    assert_eq!(utf8_to_utf16_offset(sample, 10), 4);
    assert_eq!(utf8_to_utf16_offset(sample, 11), 5);
    assert_eq!(utf8_to_utf16_offset(sample, 29), 13);
    assert_eq!(utf8_to_utf16_offset(sample, 32), 14);

    assert_eq!(utf16_to_utf8_offset(sample, 0), 0);
    assert_eq!(utf16_to_utf8_offset(sample, 1), 3);
    assert_eq!(utf16_to_utf8_offset(sample, 2), 6);
    assert_eq!(utf16_to_utf8_offset(sample, 3), 6);
    assert_eq!(utf16_to_utf8_offset(sample, 4), 10);
    assert_eq!(utf16_to_utf8_offset(sample, 5), 11);
    assert_eq!(utf16_to_utf8_offset(sample, 13), 29);
    assert_eq!(utf16_to_utf8_offset(sample, 14), 32);

    let u16_range = utf8_to_utf16_range(sample, 6..29);
    assert_eq!(u16_range, 2..13);
    let u8_roundtrip = utf16_to_utf8_range(sample, u16_range);
    assert_eq!(u8_roundtrip, 6..29);
    assert_eq!(&sample[u8_roundtrip], "🚀A👨\u{200D}👩\u{200D}👧");
}

#[test]
fn b5_text_input_host_editing_state_and_utf8_utf16_korean_emoji_conversions() {
    assert_utf8_utf16_korean_and_emoji_roundtrips();

    let text_sig = Signal::new("안녕🚀".to_string());
    let mut session = mount_single_input_session(text_sig);
    let mut host = MockImeHost::default();

    session.tree.dispatch_cursor_end();
    session.tree.sync_text_input_host(&mut host);
    assert!(host.allowed);
    assert_eq!(host.cursor_area, session.tree.focused_ime_cursor_area());
    let initial_state = host.editing_state.clone().expect("initial editing state");
    assert_eq!(initial_state.text, "안녕🚀");
    assert_eq!(initial_state.selection, 10..10);
    assert_eq!(initial_state.composing, None);
    assert_eq!(
        utf8_to_utf16_range(&initial_state.text, initial_state.selection),
        4..4
    );

    assert!(session.tree.dispatch_ime_event(ImeEvent::Enabled));
    assert!(session.tree.dispatch_ime_event(ImeEvent::Preedit {
        text: "한글".to_string(),
        cursor_offset: Some((3, 6)),
    }));
    session.tree.sync_text_input_host(&mut host);
    let preedit_state = host.editing_state.clone().expect("preedit editing state");
    assert_eq!(preedit_state.text, "안녕🚀한글");
    assert_eq!(preedit_state.composing, Some(10..16));
    assert_eq!(preedit_state.selection, 13..16);
    assert_eq!(
        utf8_to_utf16_range(&preedit_state.text, preedit_state.composing.unwrap()),
        4..6
    );
    assert_eq!(
        utf8_to_utf16_range(&preedit_state.text, preedit_state.selection),
        5..6
    );

    assert!(session
        .tree
        .dispatch_ime_event(ImeEvent::Commit("한글".to_string())));
    session.tree.sync_text_input_host(&mut host);
    let committed_state = host.editing_state.clone().expect("committed editing state");
    assert_eq!(committed_state.text, "안녕🚀한글");
    assert_eq!(committed_state.composing, None);
    assert_eq!(committed_state.selection, 16..16);

    session.tree.clear_focus();
    session.tree.sync_text_input_host(&mut host);
    assert!(!host.allowed);
    assert_eq!(host.input_kind, None);
}

#[test]
fn b_fix_1_programmatic_focus_converted_late_commit_empty_commit_and_host_sync() {
    let inp1_sig = Signal::new("초기".to_string());
    let inp2_sig = Signal::new("둘째".to_string());
    let mut session = EngineSession::new(800, 300, RenderBackendPreference::ForceTinySkia);
    let fragment = krui!(
        r#"
        <div style="width: 800px; height: 300px; padding: 20px; gap: 12px">
            <input id="inp-1" style="width: 300px; height: 40px" bind:value="{inp1_sig}" />
            <input id="inp-2" type="password" style="width: 300px; height: 40px" bind:value="{inp2_sig}" />
        </div>
        "#
    );
    session.tree.mount(fragment);
    session.step_frame(1.0 / 120.0);

    let id1 = session.tree.find_by_id("inp-1").expect("inp-1");
    let id2 = session.tree.find_by_id("inp-2").expect("inp-2");

    assert!(session.tree.set_focus_to_node(id1));
    assert!(session.tree.dispatch_cursor_end());
    assert!(session.tree.ime_enabled());
    assert!(session.tree.ime_preedit("한", None));
    assert_eq!(session.tree.focused_ime_state().unwrap().preedit, "한");

    assert!(session.tree.set_focus_to_node(id2));
    assert_eq!(inp1_sig.get(), "초기한");
    assert_eq!(inp2_sig.get(), "둘째");

    let mut host = MockImeHost::default();
    session.tree.sync_text_input_host(&mut host);
    assert!(host.allowed);
    assert_eq!(host.input_kind, Some(InputKind::Password));
    assert_eq!(host.editing_state.as_ref().unwrap().text, "둘째");
    let inp2_layout = session.tree.node_layout(id2).unwrap();
    let host_caret = host.cursor_area.expect("host caret");
    assert!(host_caret.y >= inp2_layout.y && host_caret.y <= inp2_layout.y + inp2_layout.height);

    assert!(session.tree.ime_preedit("", None));
    assert!(session.tree.ime_commit("한"));
    assert_eq!(inp1_sig.get(), "초기한");
    assert_eq!(inp2_sig.get(), "둘째");

    verify_japanese_late_commit_and_empty_commit(&mut session, id1, id2, inp1_sig, inp2_sig);
}

fn verify_japanese_late_commit_and_empty_commit(
    session: &mut EngineSession,
    id1: NodeId,
    id2: NodeId,
    inp1_sig: Signal<String>,
    inp2_sig: Signal<String>,
) {
    assert!(session.tree.set_focus_to_node(id1));
    assert!(session.tree.dispatch_cursor_end());
    assert!(session.tree.ime_preedit("にほんご", Some((0, 12))));
    assert!(session.tree.set_focus_to_node(id2));
    assert_eq!(inp1_sig.get(), "초기한にほんご");

    assert!(session.tree.ime_preedit("", None));
    assert!(session.tree.ime_commit("日本語"));
    assert_eq!(inp1_sig.get(), "초기한日本語");
    assert_eq!(inp2_sig.get(), "둘째");

    assert!(session.tree.set_focus_to_node(id1));
    assert!(session.tree.ime_preedit("ㄱ", None));
    assert!(session.tree.ime_preedit("", None));
    assert!(!session.tree.ime_commit(""));
    assert!(session.tree.set_focus_to_node(id2));
    assert!(session.tree.dispatch_cursor_end());
    assert!(session.tree.ime_commit("!"));
    assert_eq!(inp1_sig.get(), "초기한日本語");
    assert_eq!(inp2_sig.get(), "둘째!");
}

#[test]
fn b_fix_1_mobile_ime_events_finish_composing_delete_surrounding_selection_and_editor_action() {
    let inp1_sig = Signal::new("가나다라마".to_string());
    let inp2_sig = Signal::new(String::new());
    let changed_log = Signal::new(String::new());
    let on_chg = InputCommand::new(move |val: String| changed_log.set(val));

    let mut session = EngineSession::new(800, 300, RenderBackendPreference::ForceTinySkia);
    let fragment = krui!(
        r#"
        <div style="width: 800px; height: 300px; padding: 20px; gap: 12px">
            <input id="inp-1" style="width: 300px; height: 40px" bind:value="{inp1_sig}" onchange="{on_chg}" />
            <input id="inp-2" style="width: 300px; height: 40px" bind:value="{inp2_sig}" />
        </div>
        "#
    );
    session.tree.mount(fragment);
    session.step_frame(1.0 / 120.0);

    let id1 = session.tree.find_by_id("inp-1").expect("inp-1");
    let id2 = session.tree.find_by_id("inp-2").expect("inp-2");
    assert!(session.tree.set_focus_to_node(id1));

    assert!(session
        .tree
        .dispatch_ime_event(ImeEvent::SetSelection(3..9)));
    assert_eq!(session.tree.selected_text().as_deref(), Some("나다"));

    assert!(session
        .tree
        .dispatch_ime_event(ImeEvent::SetSelection(6..6)));
    assert!(session
        .tree
        .dispatch_ime_event(ImeEvent::DeleteSurrounding {
            before_bytes: 3,
            after_bytes: 3,
        }));
    assert_eq!(inp1_sig.get(), "가라마");

    assert!(session.tree.dispatch_ime_event(ImeEvent::SetComposing {
        text: "하".to_string(),
        cursor: Some((0, 3)),
    }));
    assert!(session.tree.has_active_ime_preedit());
    assert!(session.tree.dispatch_ime_event(ImeEvent::FinishComposing));
    assert!(!session.tree.has_active_ime_preedit());
    assert_eq!(inp1_sig.get(), "가하라마");

    assert!(session
        .tree
        .dispatch_ime_event(ImeEvent::EditorAction(EditorActionKind::Done)));
    assert_eq!(changed_log.get(), "가하라마");

    assert!(session
        .tree
        .dispatch_ime_event(ImeEvent::EditorAction(EditorActionKind::Next)));
    assert_eq!(session.tree.focused_node_id(), Some(id2));
}
