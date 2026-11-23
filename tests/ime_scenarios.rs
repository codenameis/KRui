//! Headless IME accuracy, target-pinning (`ime_target`), key routing (`route_key`),
//! and unified caret rectangle (`caret_rect`) scenarios (A2):
//! 1. Korean Hangul syllable & multi-syllable composition (`ㅎ` -> `하` -> `한` -> `한ㄱ` -> `한글`)
//!    with `Backspace` ignored while preedit is active.
//! 2. Korean preedit `한` -> button click (focus moves) -> `Preedit("")` + `Commit("한")` arrives
//!    -> original input has `"한"`, button is unaffected, no leftover preedit underline.
//! 3. Korean preedit `한` -> `Tab` to second `<input>` -> committed into first input, second input remains empty.
//! 4. Japanese Kana-to-Kanji conversion (`にほんご` -> `Space` -> `日本語` with target clause
//!    `Some((0, 9))` -> `Enter` -> Commit `日本語`) preserving focus and emitting clause highlights.
//! 5. English channel-based deduplication: IME disabled (`l`, `l` -> `ll`) vs IME enabled (`KeyEvent.text` + `Commit` -> once).
//! 6. `route_key` unit verification: `AltGr+Q` (`@`) inserts text, `Escape` without focus clears focus without exiting,
//!    `Ctrl+C`/`V`/`X`/`A`, and number keys `1..=4` do not switch themes in the framework.
//! 7. Unified `caret_rect`: painted caret `StyledBox` in `PaintScene` matches `focused_ime_cursor_area` exactly.

#![forbid(unsafe_code)]

use krui::*;

fn mount_single_input_session(text_sig: Signal<String>) -> EngineSession {
    let mut session = EngineSession::new(800, 300, RenderBackendPreference::ForceTinySkia);
    let fragment = krui!(
        r#"
        <style>
            div {
                padding: 20px;
                background-color: #0f172a;
            }
            input {
                width: 480px;
                height: 44px;
                padding: 8px 12px;
                font-size: 16px;
                background-color: #1e293b;
                color: #f8fafc;
            }
        </style>
        <div>
            <input id="ime-input" bind:value="{text_sig}" />
        </div>
        "#
    );
    session.tree.mount(fragment);
    session.step_frame(1.0 / 120.0);
    let focused = session.tree.dispatch_key_tab(false);
    assert!(focused.is_some(), "Input should receive focus via Tab");
    session
}

#[test]
fn korean_ime_composition_and_backspace_guard() {
    let text_sig = Signal::new("초기:".to_string());
    let mut session = mount_single_input_session(text_sig);

    assert!(session.tree.ime_enabled());
    for step in ["ㅎ", "하", "한", "한ㄱ", "한글"] {
        assert!(session.tree.ime_preedit(step, None));
        assert!(session.tree.has_active_ime_preedit());
        assert_eq!(text_sig.get(), "초기:");
    }

    assert!(!session.tree.dispatch_backspace());
    assert_eq!(text_sig.get(), "초기:");
    assert_eq!(
        session.tree.focused_ime_state().unwrap().preedit,
        "한글",
        "Preedit is owned by platform IME and must not be popped by KRui Backspace"
    );

    assert!(session.tree.ime_commit("한글"));
    assert!(!session.tree.has_active_ime_preedit());
    assert_eq!(text_sig.get(), "초기:한글");

    assert!(session.tree.dispatch_backspace());
    assert_eq!(text_sig.get(), "초기:한");
}

#[test]
fn korean_ime_focus_move_via_button_click_and_tab_commits_to_original_input() {
    let input1_sig = Signal::new(String::new());
    let input2_sig = Signal::new(String::new());
    let btn_clicks = Signal::new(0_i32);
    let on_btn = Command::new(move || btn_clicks.update(|n| *n += 1));

    let mut session = EngineSession::new(800, 300, RenderBackendPreference::ForceTinySkia);
    let fragment = krui!(
        r#"
        <div style="display: flex; flex-direction: column; width: 800px; height: 300px; padding: 20px; gap: 12px">
            <input id="input-1" style="width: 300px; height: 40px" bind:value="{input1_sig}" />
            <input id="input-2" style="width: 300px; height: 40px" bind:value="{input2_sig}" />
            <button id="submit-btn" style="width: 140px; height: 40px" onclick="{on_btn}">확인</button>
        </div>
        "#
    );
    session.tree.mount(fragment);
    session.step_frame(1.0 / 120.0);

    let inp1_id = session.tree.find_by_id("input-1").expect("input-1");
    let inp2_id = session.tree.find_by_id("input-2").expect("input-2");
    let btn_id = session.tree.find_by_id("submit-btn").expect("submit-btn");

    assert_eq!(session.tree.dispatch_key_tab(false), Some(inp1_id));
    assert!(session.tree.ime_enabled());
    assert!(session.tree.ime_preedit("한", None));

    assert!(session.tree.dispatch_click(60.0, 144.0));
    assert_eq!(session.tree.focused_node_id(), Some(btn_id));
    assert_eq!(btn_clicks.get(), 1);
    assert_eq!(input1_sig.get(), "한");

    session.tree.ime_preedit("", None);
    session.tree.ime_commit("한");
    assert_eq!(input1_sig.get(), "한");
    assert_eq!(input2_sig.get(), "");
    assert_no_leftover_preedit_underline(&mut session);

    session.tree.dispatch_click(60.0, 35.0);
    assert_eq!(session.tree.focused_node_id(), Some(inp1_id));
    assert!(session.tree.ime_preedit("글", None));

    assert_eq!(session.tree.dispatch_key_tab(false), Some(inp2_id));
    session.tree.ime_preedit("", None);
    session.tree.ime_commit("글");
    assert_eq!(input1_sig.get(), "한글");
    assert_eq!(input2_sig.get(), "");
}

fn assert_no_leftover_preedit_underline(session: &mut EngineSession) {
    let (scene, _) = session
        .tree
        .build_paint_scene(session.width, session.height);
    let has_preedit_underline = scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if *background_color == krui_style::UA_PRIMARY_TEXT_COLOR && rect.height <= 3.0
        )
    });
    assert!(
        !has_preedit_underline,
        "No leftover preedit underline should remain after focus loss commit"
    );
}

#[test]
fn japanese_ime_conversion_preserves_focus_and_highlights_clause() {
    let text_sig = Signal::new(String::new());
    let mut session = mount_single_input_session(text_sig);

    assert!(session.tree.ime_enabled());
    assert!(session.tree.ime_preedit("にほんご", Some((0, 12))));
    assert!(session.tree.has_active_ime_preedit());

    assert!(!session.tree.dispatch_key_space());
    assert_eq!(text_sig.get(), "");

    assert!(session.tree.ime_preedit("日本語", Some((0, 9))));
    let (scene, _) = session
        .tree
        .build_paint_scene(session.width, session.height);
    let has_target_highlight = scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox {
                background_color,
                ..
            } if *background_color == krui_style::UA_IME_TARGET_BG_COLOR
        )
    });
    let has_preedit_underline = scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if *background_color == krui_style::UA_PRIMARY_TEXT_COLOR && rect.height <= 3.0
        )
    });
    assert!(
        has_target_highlight && has_preedit_underline,
        "PaintScene must emit both target clause highlight and preedit underline from theme tokens"
    );

    assert!(!session.tree.dispatch_key_enter());
    assert!(
        session.tree.focused_input_rect().is_some(),
        "Enter during Japanese IME preedit must never clear focus!"
    );

    assert!(session.tree.ime_commit("日本語"));
    assert_eq!(text_sig.get(), "日本語");
    assert!(
        session.tree.focused_input_rect().is_some(),
        "Focus must remain on the input after IME commit"
    );
}

#[test]
fn english_channel_deduplication_and_double_l_typing() {
    let text_sig = Signal::new(String::new());
    let mut session = mount_single_input_session(text_sig);

    // 1. IME disabled (`enabled == false` by default): typing 'l', 'l' via `dispatch_key_text` -> `"ll"`!
    assert!(session.tree.dispatch_key_text("l"));
    assert!(session.tree.dispatch_key_text("l"));
    assert_eq!(
        text_sig.get(),
        "ll",
        "Repeated 'l' when IME is disabled must produce 'll'"
    );

    // 2. IME enabled (`ime_enabled()`): both `KeyEvent.text` and `Ime::Commit` arrive for the same key press ->
    // `dispatch_key_text` is ignored while `ime_state.enabled` is true, and `ime_commit` inserts once.
    // Even if the user types the same character twice (`"l"`, `"l"`), both commits succeed!
    assert!(session.tree.ime_enabled());
    assert!(!session.tree.dispatch_key_text("a"));
    assert!(session.tree.ime_commit("a"));
    assert_eq!(text_sig.get(), "lla");

    assert!(!session.tree.dispatch_key_text("l"));
    assert!(session.tree.ime_commit("l"));
    assert!(!session.tree.dispatch_key_text("l"));
    assert!(session.tree.ime_commit("l"));
    assert_eq!(text_sig.get(), "llall");

    // 3. After `Preedit("")` followed by `Commit("x")`, commit still works cleanly
    assert!(session.tree.ime_preedit("x", None));
    assert!(session.tree.ime_preedit("", None));
    assert!(session.tree.ime_commit("x"));
    assert_eq!(text_sig.get(), "llallx");
}

#[test]
fn route_key_handles_altgr_escape_shortcuts_and_no_implicit_theme_switch() {
    let focused_ime_off = FocusState {
        is_input_focused: true,
        has_ime_preedit: false,
        is_ime_enabled: false,
    };
    let focused_ime_on = FocusState {
        is_input_focused: true,
        has_ime_preedit: false,
        is_ime_enabled: true,
    };
    let unfocused = FocusState {
        is_input_focused: false,
        has_ime_preedit: false,
        is_ime_enabled: false,
    };

    let altgr_mods = Mods {
        ctrl: true,
        alt: true,
        shift: false,
        super_key: false,
    };
    let q_at = KeyInfo::Character {
        logical: "@".to_string(),
        text: Some("@".to_string()),
    };
    assert_eq!(
        route_key(&q_at, altgr_mods, &focused_ime_off),
        KeyAction::InsertText("@".to_string())
    );
    assert_eq!(
        route_key(&q_at, altgr_mods, &focused_ime_on),
        KeyAction::InsertText("@".to_string())
    );
    assert_eq!(
        route_key(&KeyInfo::Escape, Mods::default(), &unfocused),
        KeyAction::ClearFocus
    );

    assert_ctrl_shortcuts_and_unfocused_numbers(&focused_ime_on, &unfocused);
}

fn assert_ctrl_shortcuts_and_unfocused_numbers(
    focused_ime_on: &FocusState,
    unfocused: &FocusState,
) {
    let ctrl_mods = Mods {
        ctrl: true,
        alt: false,
        shift: false,
        super_key: false,
    };
    for (ch, expected) in [
        ("a", KeyAction::SelectAll),
        ("c", KeyAction::Copy),
        ("x", KeyAction::Cut),
        ("v", KeyAction::Paste),
    ] {
        let ev = KeyInfo::Character {
            logical: ch.to_string(),
            text: None,
        };
        assert_eq!(route_key(&ev, ctrl_mods, focused_ime_on), expected);
    }
    for num in ["1", "2", "3", "4"] {
        let ev = KeyInfo::Character {
            logical: num.to_string(),
            text: Some(num.to_string()),
        };
        assert_eq!(route_key(&ev, Mods::default(), unfocused), KeyAction::None);
    }
}

#[test]
fn grapheme_cluster_backspace_delete_and_cursor_navigation() {
    let text_sig = Signal::new(String::new());
    let mut session = mount_single_input_session(text_sig);

    let family_emoji = "👨\u{200D}👩\u{200D}👧";
    assert_eq!(family_emoji.chars().count(), 5);

    assert!(session.tree.dispatch_text_input("A"));
    assert!(session.tree.dispatch_text_input(family_emoji));
    assert_eq!(text_sig.get(), format!("A{family_emoji}"));

    assert!(session.tree.dispatch_backspace());
    assert_eq!(text_sig.get(), "A");

    let combining_e = "e\u{0301}";
    assert!(session.tree.dispatch_text_input(combining_e));
    assert!(session.tree.dispatch_text_input(family_emoji));
    assert!(session.tree.dispatch_cursor_left());
    assert!(session.tree.dispatch_delete());
    assert_eq!(text_sig.get(), format!("A{combining_e}"));

    assert!(session.tree.dispatch_backspace());
    assert_eq!(text_sig.get(), "A");
}

#[test]
fn painted_caret_rect_matches_focused_ime_cursor_area_exactly() {
    let text_sig = Signal::new(String::new());
    let mut session = mount_single_input_session(text_sig);

    session.tree.ime_preedit("한글 입력", None);
    let (scene, _) = session
        .tree
        .build_paint_scene(session.width, session.height);
    let ime_area = session
        .tree
        .focused_ime_cursor_area()
        .expect("focused input must report IME cursor area");

    let inp = session.tree.find_by_id("ime-input").expect("ime-input");
    let eff = session
        .tree
        .node_effective_paint(inp)
        .expect("ime-input paint");
    let caret_color = eff.resolve_caret_color(eff.text_color);

    let painted_caret = scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if *background_color == caret_color && rect.height >= 12.0 && rect.width <= 4.0 => {
                Some(*rect)
            }
            _ => None,
        })
        .expect("PaintScene must contain the focused input caret StyledBox");

    assert!(
        (painted_caret.x - ime_area.x).abs() < 1e-4
            && (painted_caret.y - ime_area.y).abs() < 1e-4
            && (painted_caret.width - ime_area.width).abs() < 1e-4
            && (painted_caret.height - ime_area.height).abs() < 1e-4,
        "Painted caret rect ({painted_caret:?}) must match focused_ime_cursor_area ({ime_area:?}) exactly"
    );
}
