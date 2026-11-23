//! Stage `C1` single-line `<input>` completion regression tests (`long_input_does_not_wrap_and_caret_stays_visible`,
//! `shift_arrow_extends_selection`, `ctrl_word_navigation_and_deletion`, `undo_redo_text_transactions`,
//! `single_line_rejects_newline_tabs`, `input_type_roundtrip_macro_preview`,
//! `external_bind_preserves_caret_selection`).

#![forbid(unsafe_code)]

use krui::*;

const VIEWPORT_W: u32 = 800;
const VIEWPORT_H: u32 = 600;

fn extract_painted_caret_rect(
    scene: &PaintScene,
    node_rect: Option<LayoutRect>,
) -> Option<LayoutRect> {
    scene.primitives.iter().rev().find_map(|prim| match prim {
        DrawPrimitive::StyledBox {
            rect, border_width, ..
        } if *border_width == 0.0
            && (rect.width - 2.0).abs() < 0.01
            && node_rect.is_some_and(|nr| {
                rect.x >= nr.x
                    && rect.x <= nr.x + nr.width
                    && rect.y >= nr.y
                    && rect.y <= nr.y + nr.height
            }) =>
        {
            Some(*rect)
        }
        _ => None,
    })
}

fn assert_rects_approx_eq(a: Option<LayoutRect>, b: Option<LayoutRect>) {
    let (Some(ra), Some(rb)) = (a, b) else {
        assert_eq!(a, b);
        return;
    };
    assert!(
        (ra.x - rb.x).abs() < 1e-3,
        "rect.x mismatch: {ra:?} vs {rb:?}"
    );
    assert!(
        (ra.y - rb.y).abs() < 1e-3,
        "rect.y mismatch: {ra:?} vs {rb:?}"
    );
}

#[test]
fn long_input_does_not_wrap_and_caret_stays_visible() {
    let text_sig = Signal::new("초기".to_string());
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <div direction="column" style="padding: 16px; gap: 8px; width: 240px;">
                <input id="inp" style="width: 180px;" bind:value="{text_sig}" />
            </div>
        "#
        )
    });
    let inp_id = tree.find_by_id("inp").expect("inp");
    assert!(tree.set_focus_to_node(inp_id));
    let _ = tree.build_paint_scene(VIEWPORT_W, VIEWPORT_H);
    let short_box = tree.node_layout(inp_id).expect("short layout");

    let long_str =
        "한글과영문이섞인매우긴단일행텍스트0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ가나다라마바사";
    text_sig.set(long_str.to_string());
    let (scene_end, _) = tree.build_paint_scene(VIEWPORT_W, VIEWPORT_H);
    let long_box = tree.node_layout(inp_id).expect("long layout");
    assert!(
        (long_box.height - short_box.height).abs() < 0.5,
        "Single-line <input> height must not grow with long text: short={}, long={}",
        short_box.height,
        long_box.height
    );
    assert!(
        tree.node_scroll_x(inp_id).unwrap_or(0.0) > 10.0,
        "Long single-line <input> must scroll horizontally to keep end caret visible"
    );

    let caret_end = extract_painted_caret_rect(&scene_end, Some(long_box)).expect("caret at end");
    assert_rects_approx_eq(Some(caret_end), tree.focused_ime_cursor_area());
    assert!(
        caret_end.x >= long_box.x && caret_end.x <= long_box.x + long_box.width,
        "Caret at end must stay within input bounds: caret={caret_end:?}, box={long_box:?}"
    );

    assert!(tree.dispatch_cursor_left());
    let (scene_left1, _) = tree.build_paint_scene(VIEWPORT_W, VIEWPORT_H);
    let caret_left1 =
        extract_painted_caret_rect(&scene_left1, Some(long_box)).expect("caret left 1");
    assert_rects_approx_eq(Some(caret_left1), tree.focused_ime_cursor_area());
    assert!(caret_left1.x < caret_end.x);

    assert!(tree.dispatch_cursor_home());
    let (scene_home, _) = tree.build_paint_scene(VIEWPORT_W, VIEWPORT_H);
    let caret_home = extract_painted_caret_rect(&scene_home, Some(long_box)).expect("caret home");
    assert_eq!(tree.node_scroll_x(inp_id), Some(0.0));
    assert!(caret_home.x < caret_left1.x && caret_home.x >= long_box.x);
}

#[test]
fn shift_arrow_extends_selection() {
    let text_sig = Signal::new("가나다라마".to_string());
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <input id="inp" bind:value="{text_sig}" />
        "#
        )
    });
    let inp_id = tree.find_by_id("inp").expect("inp");
    assert!(tree.set_focus_to_node(inp_id));
    let _ = tree.build_paint_scene(VIEWPORT_W, VIEWPORT_H);

    let focus_state = FocusState {
        is_input_focused: true,
        has_ime_preedit: false,
        is_ime_enabled: false,
    };
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };

    // Start at end (5), press Shift+ArrowLeft twice -> selection anchor=5, cursor=3 ("라마")
    apply_key_action(
        &mut tree,
        route_key(&KeyInfo::ArrowLeft, shift, &focus_state),
    );
    apply_key_action(
        &mut tree,
        route_key(&KeyInfo::ArrowLeft, shift, &focus_state),
    );
    assert_eq!(tree.node_selection_anchor_char_idx(inp_id), Some(5));
    assert_eq!(tree.node_cursor_char_idx(inp_id), Some(3));
    assert_eq!(tree.selected_text().as_deref(), Some("라마"));

    // Press Shift+Home (or Shift+ArrowUp) -> anchor=5, cursor=0 ("가나다라마")
    apply_key_action(&mut tree, route_key(&KeyInfo::ArrowUp, shift, &focus_state));
    assert_eq!(tree.node_selection_anchor_char_idx(inp_id), Some(5));
    assert_eq!(tree.node_cursor_char_idx(inp_id), Some(0));
    assert_eq!(tree.selected_text().as_deref(), Some("가나다라마"));

    // Press ArrowLeft without Shift -> collapses selection to start (0)
    apply_key_action(
        &mut tree,
        route_key(&KeyInfo::ArrowLeft, Mods::default(), &focus_state),
    );
    assert_eq!(tree.node_selection_anchor_char_idx(inp_id), None);
    assert_eq!(tree.node_cursor_char_idx(inp_id), Some(0));

    // Press Shift+End (or Shift+ArrowDown) -> anchor=0, cursor=5
    apply_key_action(
        &mut tree,
        route_key(&KeyInfo::ArrowDown, shift, &focus_state),
    );
    assert_eq!(tree.node_selection_anchor_char_idx(inp_id), Some(0));
    assert_eq!(tree.node_cursor_char_idx(inp_id), Some(5));
}

#[test]
fn ctrl_word_navigation_and_deletion() {
    let text_sig = Signal::new("hello world 한국어 테스트".to_string());
    let mut tree = ViewTree::new();
    tree.mount_with(|| krui!(r#"<input id="inp" bind:value="{text_sig}" />"#));
    let inp_id = tree.find_by_id("inp").expect("inp");
    assert!(tree.set_focus_to_node(inp_id));

    let focus_state = FocusState {
        is_input_focused: true,
        has_ime_preedit: false,
        is_ime_enabled: false,
    };
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    let ctrl_shift = Mods {
        ctrl: true,
        shift: true,
        ..Mods::default()
    };

    apply_key_action(
        &mut tree,
        route_key(&KeyInfo::ArrowLeft, ctrl, &focus_state),
    );
    assert_eq!(tree.node_cursor_char_idx(inp_id), Some(16));
    apply_key_action(
        &mut tree,
        route_key(&KeyInfo::ArrowLeft, ctrl, &focus_state),
    );
    assert_eq!(tree.node_cursor_char_idx(inp_id), Some(12));

    apply_key_action(
        &mut tree,
        route_key(&KeyInfo::ArrowLeft, ctrl_shift, &focus_state),
    );
    assert_eq!(tree.node_selection_anchor_char_idx(inp_id), Some(12));
    assert_eq!(tree.node_cursor_char_idx(inp_id), Some(6));
    assert_eq!(tree.selected_text().as_deref(), Some("world "));

    assert!(tree.dispatch_cursor_end());
    apply_key_action(
        &mut tree,
        route_key(&KeyInfo::Backspace, ctrl, &focus_state),
    );
    assert_eq!(text_sig.get(), "hello world 한국어 ");

    assert!(tree.dispatch_cursor_home());
    apply_key_action(&mut tree, route_key(&KeyInfo::Delete, ctrl, &focus_state));
    assert_eq!(text_sig.get(), " world 한국어 ");
}

#[test]
fn undo_redo_text_transactions() {
    let text_sig = Signal::new(String::new());
    let mut tree = ViewTree::new();
    tree.mount_with(|| krui!(r#"<input id="inp" bind:value="{text_sig}" />"#));
    let inp_id = tree.find_by_id("inp").expect("inp");
    assert!(tree.set_focus_to_node(inp_id));

    assert!(tree.dispatch_text_input("첫번째"));
    assert!(tree.dispatch_text_input(" 두번째"));
    assert_eq!(text_sig.get(), "첫번째 두번째");

    let focus_state = FocusState {
        is_input_focused: true,
        has_ime_preedit: false,
        is_ime_enabled: false,
    };
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    let ctrl_shift = Mods {
        ctrl: true,
        shift: true,
        ..Mods::default()
    };
    let z_key = KeyInfo::Character {
        logical: "z".to_string(),
        text: None,
    };
    let y_key = KeyInfo::Character {
        logical: "y".to_string(),
        text: None,
    };

    assert!(apply_key_action(
        &mut tree,
        route_key(&z_key, ctrl, &focus_state)
    ));
    assert_eq!(text_sig.get(), "첫번째");
    assert!(apply_key_action(
        &mut tree,
        route_key(&z_key, ctrl, &focus_state)
    ));
    assert_eq!(text_sig.get(), "");

    assert!(apply_key_action(
        &mut tree,
        route_key(&y_key, ctrl, &focus_state)
    ));
    assert_eq!(text_sig.get(), "첫번째");
    assert!(apply_key_action(
        &mut tree,
        route_key(&z_key, ctrl_shift, &focus_state)
    ));
    assert_eq!(text_sig.get(), "첫번째 두번째");
}

#[test]
fn single_line_rejects_newline_tabs() {
    let text_sig = Signal::new("초기\n값\t테스트".to_string());
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <input id="inp" bind:value="{text_sig}" />
        "#
        )
    });
    let inp_id = tree.find_by_id("inp").expect("inp");
    assert!(tree.set_focus_to_node(inp_id));
    assert_eq!(tree.node_text(inp_id).as_deref(), Some("초기 값 테스트"));

    assert!(tree.select_all());
    assert!(tree.dispatch_text_input("첫줄\r\n둘째줄\n셋째줄\t끝"));
    assert_eq!(
        tree.node_text(inp_id).as_deref(),
        Some("첫줄 둘째줄 셋째줄 끝")
    );
    assert_eq!(text_sig.get(), "첫줄 둘째줄 셋째줄 끝");

    let _ = tree.dispatch_ime_event(ImeEvent::Commit(" / 한글\n조합\t완료".to_string()));
    assert_eq!(
        tree.node_text(inp_id).as_deref(),
        Some("첫줄 둘째줄 셋째줄 끝 / 한글 조합 완료")
    );
}

#[test]
fn input_type_roundtrip_macro_preview() {
    let mut macro_tree = ViewTree::new();
    macro_tree.mount_with(|| {
        krui!(
            r#"
            <div>
                <input id="t-text" type="text" />
                <input id="t-pass" type="password" />
                <input id="t-num" type="number" />
                <input id="t-mail" type="email" />
                <input id="t-search" type="search" />
            </div>
        "#
        )
    });

    let expected = [
        ("t-text", InputKind::Text),
        ("t-pass", InputKind::Password),
        ("t-num", InputKind::Number),
        ("t-mail", InputKind::Email),
        ("t-search", InputKind::Search),
    ];
    for (id, kind) in expected {
        let nid = macro_tree.find_by_id(id).expect(id);
        assert_eq!(macro_tree.node_input_kind(nid), Some(kind));
        assert!(macro_tree.set_focus_to_node(nid));
        assert_eq!(macro_tree.focused_input_kind(), Some(kind));
    }

    #[cfg(feature = "preview")]
    {
        let src = r#"
            <div>
                <input id="t-text" type="text" />
                <input id="t-pass" type="password" />
                <input id="t-num" type="number" />
                <input id="t-mail" type="email" />
                <input id="t-search" type="search" />
            </div>
        "#;
        let doc = krui_preview::compile_krui_source(src).expect("compile preview doc");
        let bytes = krui_preview::encode_compiled_document(&doc).expect("encode doc");
        let decoded = krui_preview::decode_compiled_document(&bytes).expect("decode doc");
        let mut preview_tree = ViewTree::new();
        krui_preview::mount_compiled_document(&mut preview_tree, &decoded);
        for (id, kind) in expected {
            let nid = preview_tree.find_by_id(id).expect(id);
            assert_eq!(preview_tree.node_input_kind(nid), Some(kind));
            assert!(preview_tree.set_focus_to_node(nid));
            assert_eq!(preview_tree.focused_input_kind(), Some(kind));
        }
    }
}

#[test]
fn external_bind_preserves_caret_selection() {
    let text_sig = Signal::new("hello world".to_string());
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <input id="inp" bind:value="{text_sig}" />
        "#
        )
    });
    let inp_id = tree.find_by_id("inp").expect("inp");
    assert!(tree.set_focus_to_node(inp_id));

    // Select "hello" (anchor=0, cursor=5) and start an IME preedit
    assert!(tree.dispatch_cursor_home());
    for _ in 0..5 {
        assert!(tree.dispatch_select_right());
    }
    assert_eq!(tree.node_selection_anchor_char_idx(inp_id), Some(0));
    assert_eq!(tree.node_cursor_char_idx(inp_id), Some(5));
    assert_eq!(tree.selected_text().as_deref(), Some("hello"));

    // External signal update appends "!" to the suffix while input is focused
    text_sig.set("hello world!".to_string());
    assert_eq!(tree.node_text(inp_id).as_deref(), Some("hello world!"));
    assert_eq!(tree.node_selection_anchor_char_idx(inp_id), Some(0));
    assert_eq!(tree.node_cursor_char_idx(inp_id), Some(5));
    assert_eq!(tree.selected_text().as_deref(), Some("hello"));

    // Collapse to index 5 and start an IME preedit; external suffix update must not wipe preedit
    assert!(tree.dispatch_cursor_right());
    let _ = tree.dispatch_ime_event(ImeEvent::Preedit {
        text: "한".to_string(),
        cursor_offset: Some((0, "한".len())),
    });
    text_sig.set("hello world!!".to_string());
    assert!(tree.has_active_ime_preedit());
    assert_eq!(
        tree.focused_ime_state().map(|s| s.preedit),
        Some("한".to_string())
    );
}
