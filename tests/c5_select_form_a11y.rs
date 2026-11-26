//! Stage C5 Regression Suite:
//! - `<select>` + `<option value="..." selected>` keyboard (`ArrowDown`/`ArrowUp`/`Home`/`End`/`Space`) & pointer selection
//! - `<form onsubmit="..." onreset="...">`, `required`, `type="email|number"` validation, `<button type="submit|reset|button">`, and `Enter` submit
//! - AccessKit `ActionRequest` handling (`Action::Focus`, `Action::Click`, `Action::SetValue`)
//! - AccessKit node `bounds` including ancestor `scroll_y` and `transform` (`Affine2`)
//! - Mobile/IME `InputKind` mapping per `<input type="text|password|number|email|search">`

#![forbid(unsafe_code)]

use krui::*;

fn find_id(tree: &ViewTree, element_id: &str) -> NodeId {
    tree.find_by_id(element_id)
        .unwrap_or_else(|| panic!("expected element `#{element_id}`"))
}

fn center_of(tree: &ViewTree, id: NodeId) -> (f32, f32) {
    let rect = tree.node_layout(id).expect("expected layout rect");
    (rect.x + rect.width * 0.5, rect.y + rect.height * 0.5)
}

#[test]
fn select_keyboard_and_option_value() {
    reset_reactive_runtime();
    let role_val = Signal::new(String::from("eng"));
    let committed_role = Signal::new(String::new());
    let on_role_change = move |val: String| committed_role.set(val);

    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                div { display: flex; width: 360px; height: 220px; padding: 16px; }
                select { width: 220px; height: 34px; }
            </style>
            <div>
                <select id="role-select" bind:value="{role_val}" onchange="{on_role_change}">
                    <option value="design">Product Designer</option>
                    <option value="eng" selected>Systems Engineer</option>
                    <option value="pm">Technical PM</option>
                </select>
            </div>
        "#
        )
    });

    let sel_id = find_id(&tree, "role-select");
    let _ = tree.build_paint_scene(360, 220);
    assert_eq!(tree.node_text(sel_id).as_deref(), Some("eng"));
    assert_eq!(
        tree.node_selected_label(sel_id).as_deref(),
        Some("Systems Engineer")
    );

    assert_eq!(tree.dispatch_key_tab(false), Some(sel_id));
    assert!(tree.dispatch_cursor_down());
    assert_eq!(role_val.get(), "pm");
    assert_eq!(committed_role.get(), "pm");
    assert_eq!(
        tree.node_selected_label(sel_id).as_deref(),
        Some("Technical PM")
    );

    assert!(tree.dispatch_cursor_up() && role_val.get() == "eng");
    assert!(tree.dispatch_cursor_home() && role_val.get() == "design");
    assert!(tree.dispatch_cursor_end() && role_val.get() == "pm");

    let (sx, sy) = center_of(&tree, sel_id);
    assert!(tree.dispatch_click(sx, sy) && tree.is_select_popup_open(sel_id));
    let _ = tree.build_paint_scene(360, 220);
    let popup_rect = tree.active_select_popup_rect().expect("open popup rect");
    assert!(tree.dispatch_click(popup_rect.x + 24.0, popup_rect.y + 18.0));
    assert!(!tree.is_select_popup_open(sel_id));
    assert_eq!(role_val.get(), "design");
    assert_eq!(committed_role.get(), "design");

    assert!(tree.dispatch_click(sx, sy) && tree.is_select_popup_open(sel_id));
    assert!(tree.dispatch_click(348.0, 10.0) && !tree.is_select_popup_open(sel_id));
    assert_eq!(role_val.get(), "design");
}

#[test]
fn form_required_validation_and_submit_once() {
    reset_reactive_runtime();
    let user_name = Signal::new(String::new());
    let user_email = Signal::new(String::new());
    let agreed = Signal::new(false);
    let submit_count = Signal::new(0_i32);
    let reset_count = Signal::new(0_i32);
    let on_form_submit = move || submit_count.update(|c| *c += 1);
    let on_form_reset = move || reset_count.update(|c| *c += 1);

    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                form { display: flex; flex-direction: column; width: 420px; height: 320px; padding: 12px; gap: 8px; }
                input, button { width: 200px; height: 32px; }
            </style>
            <form id="signup-form" onsubmit="{on_form_submit}" onreset="{on_form_reset}">
                <input id="name-inp" required bind:value="{user_name}" />
                <input id="email-inp" type="email" required bind:value="{user_email}" />
                <input id="terms-chk" type="checkbox" required bind:checked="{agreed}" />
                <button id="submit-btn" type="submit">제출</button>
                <button id="reset-btn" type="reset">초기화</button>
            </form>
        "#
        )
    });

    let _ = tree.build_paint_scene(420, 320);
    let (name_id, email_id, chk_id) = (
        find_id(&tree, "name-inp"),
        find_id(&tree, "email-inp"),
        find_id(&tree, "terms-chk"),
    );
    let (sub_x, sub_y) = center_of(&tree, find_id(&tree, "submit-btn"));
    assert!(tree.dispatch_click(sub_x, sub_y));
    assert_eq!(submit_count.get(), 0);
    assert!(tree.is_invalid(name_id) && tree.is_invalid(email_id) && tree.is_invalid(chk_id));
    assert_eq!(tree.focused_node_id(), Some(name_id));

    assert!(tree.dispatch_text_input("김보현"));
    assert!(!tree.is_invalid(name_id));
    assert_eq!(tree.dispatch_key_tab(false), Some(email_id));
    assert!(tree.dispatch_text_input("not-an-email") && tree.dispatch_key_enter());
    assert!(submit_count.get() == 0 && tree.is_invalid(email_id));

    user_email.set("bohyun@krui.dev".to_string());
    agreed.set(true);
    let _ = tree.build_paint_scene(420, 320);
    assert!(tree.dispatch_click(sub_x, sub_y) && submit_count.get() == 1);
    let (res_x, res_y) = center_of(&tree, find_id(&tree, "reset-btn"));
    assert!(tree.dispatch_click(res_x, res_y) && reset_count.get() == 1);
    assert!(user_name.get().is_empty() && user_email.get().is_empty() && !agreed.get());
}

#[test]
fn accesskit_adapter_handles_action_request() {
    reset_reactive_runtime();
    let title = Signal::new(String::from("초기값"));
    let slider_val = Signal::new(10.0_f32);
    let clicks = Signal::new(0_i32);
    let on_btn_click = move || clicks.update(|c| *c += 1);

    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                div { display: flex; flex-direction: column; width: 360px; height: 220px; padding: 12px; gap: 8px; }
                input, button { width: 200px; height: 32px; }
            </style>
            <div>
                <input id="ak-input" bind:value="{title}" />
                <input id="ak-slider" type="range" min="0" max="100" step="5" bind:value="{slider_val}" />
                <button id="ak-btn" onclick="{on_btn_click}">실행</button>
            </div>
        "#
        )
    });

    let _ = tree.build_paint_scene(360, 220);
    let (inp_id, sld_id, btn_id) = (
        find_id(&tree, "ak-input"),
        find_id(&tree, "ak-slider"),
        find_id(&tree, "ak-btn"),
    );

    assert!(tree.dispatch_accesskit_action(&accesskit::ActionRequest {
        action: accesskit::Action::Focus,
        target: accesskit::NodeId(A11yId::from(inp_id).0),
        data: None,
    }));
    assert_eq!(tree.focused_node_id(), Some(inp_id));

    assert!(tree.dispatch_accesskit_action(&accesskit::ActionRequest {
        action: accesskit::Action::SetValue,
        target: accesskit::NodeId(A11yId::from(inp_id).0),
        data: Some(accesskit::ActionData::Value("접근성 입력".into())),
    }));
    assert_eq!(title.get(), "접근성 입력");

    assert!(tree.dispatch_accesskit_action(&accesskit::ActionRequest {
        action: accesskit::Action::SetValue,
        target: accesskit::NodeId(A11yId::from(sld_id).0),
        data: Some(accesskit::ActionData::NumericValue(45.0)),
    }));
    assert_eq!(slider_val.get(), 45.0);

    assert!(tree.dispatch_accesskit_action(&accesskit::ActionRequest {
        action: accesskit::Action::Click,
        target: accesskit::NodeId(A11yId::from(btn_id).0),
        data: None,
    }));
    assert_eq!(clicks.get(), 1);
}

#[test]
fn a11y_bounds_include_scroll_and_transform() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                #scroll-box {
                    display: flex;
                    flex-direction: column;
                    width: 300px;
                    height: 120px;
                    overflow-y: scroll;
                }
                .spacer { width: 260px; height: 140px; flex-shrink: 0; }
                #shifted-btn {
                    width: 120px;
                    height: 36px;
                    flex-shrink: 0;
                    transform: translate(24px, 10px);
                }
            </style>
            <div id="scroll-box">
                <div class="spacer">Spacer</div>
                <button id="shifted-btn">이동 버튼</button>
            </div>
        "#
        )
    });

    let _ = tree.build_paint_scene(300, 120);
    let btn_id = find_id(&tree, "shifted-btn");
    let ak_btn_id = accesskit::NodeId(A11yId::from(btn_id).0);

    let before_update = tree.build_accessibility_tree(300.0, 120.0);
    let before_bounds = before_update
        .nodes
        .iter()
        .find_map(|(id, n)| (*id == ak_btn_id).then(|| n.bounds()))
        .flatten()
        .expect("before bounds");
    assert!((before_bounds.x0 - 24.0).abs() < 1.0);
    assert!((before_bounds.y0 - 150.0).abs() < 1.0);

    assert!(tree.dispatch_scroll(50.0, 50.0, -40.0));
    let _ = tree.build_paint_scene(300, 120);
    let after_update = tree.build_accessibility_tree(300.0, 120.0);
    let after_bounds = after_update
        .nodes
        .iter()
        .find_map(|(id, n)| (*id == ak_btn_id).then(|| n.bounds()))
        .flatten()
        .expect("after bounds");
    assert!(
        ((before_bounds.y0 - after_bounds.y0) - 40.0).abs() < 1.0,
        "AccessKit bounds must reflect scroll_y and transform: before={before_bounds:?}, after={after_bounds:?}"
    );
}

#[derive(Default)]
struct RecordingHost {
    shown_kinds: Vec<InputKind>,
    allowed: bool,
}

impl TextInputHost for RecordingHost {
    fn set_ime_allowed(&mut self, allowed: bool) {
        self.allowed = allowed;
    }
    fn set_ime_cursor_area(&mut self, _rect: LayoutRect) {}
    fn update_editing_state(&mut self, _state: &EditingState) {}
    fn show_keyboard(&mut self, kind: InputKind) {
        self.allowed = true;
        self.shown_kinds.push(kind);
    }
}

#[test]
fn mobile_input_kind_per_type() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                div { display: flex; flex-direction: column; width: 320px; height: 240px; gap: 6px; }
                input { width: 200px; height: 28px; }
            </style>
            <div>
                <input id="i-text" type="text" />
                <input id="i-pw" type="password" />
                <input id="i-num" type="number" />
                <input id="i-email" type="email" />
                <input id="i-search" type="search" />
            </div>
        "#
        )
    });

    let _ = tree.build_paint_scene(320, 240);
    let expected = [
        ("i-text", InputKind::Text),
        ("i-pw", InputKind::Password),
        ("i-num", InputKind::Number),
        ("i-email", InputKind::Email),
        ("i-search", InputKind::Search),
    ];
    let mut host = RecordingHost::default();
    for (elem_id, exp_kind) in expected {
        let nid = find_id(&tree, elem_id);
        assert_eq!(tree.node_input_kind(nid), Some(exp_kind));
        assert!(tree.set_focus_to_node(nid));
        tree.sync_text_input_host(&mut host);
    }
    assert_eq!(
        host.shown_kinds,
        vec![
            InputKind::Text,
            InputKind::Password,
            InputKind::Number,
            InputKind::Email,
            InputKind::Search,
        ]
    );
}

#[test]
fn password_drag_selection_delete_and_clipboard_block() {
    reset_reactive_runtime();
    let pw = Signal::new(String::from("hunter2!"));
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                div { display: flex; width: 320px; height: 100px; padding: 12px; }
                input { width: 220px; height: 34px; }
            </style>
            <div>
                <input id="pw-inp" type="password" bind:value="{pw}" />
            </div>
        "#
        )
    });

    let _ = tree.build_paint_scene(320, 100);
    let pw_id = find_id(&tree, "pw-inp");
    let rect = tree.node_layout(pw_id).expect("pw layout");

    // Drag across the password input from right to left to select characters
    tree.dispatch_pointer_down(rect.x + rect.width - 8.0, rect.y + rect.height * 0.5);
    assert!(tree.dispatch_pointer_drag(rect.x + 2.0, rect.y + rect.height * 0.5));
    tree.end_pointer_drag();

    assert!(tree.node_selection_anchor_char_idx(pw_id).is_some());
    assert_ne!(
        tree.node_selection_anchor_char_idx(pw_id),
        tree.node_cursor_char_idx(pw_id)
    );
    // Secret password must never expose plaintext to `selected_text` or `cut_selected_text`
    assert_eq!(tree.selected_text(), None);
    assert_eq!(tree.cut_selected_text(), None);

    // Paint scene must still render a selection highlight box for the selected bullets
    let (scene, _) = tree.build_paint_scene(320, 100);
    let has_sel_box = scene.primitives.iter().any(|p| match p {
        DrawPrimitive::StyledBox {
            rect: box_r,
            background_color,
            ..
        } => {
            *background_color == krui_style::UA_SELECTION_BG_COLOR
                && box_r.width > 3.0
                && box_r.x >= rect.x
                && box_r.x < rect.x + rect.width
        }
        _ => false,
    });
    assert!(has_sel_box, "password selection highlight box must render");

    // Pressing Backspace deletes the dragged selection range
    assert!(tree.dispatch_backspace());
    assert!(pw.get().len() < "hunter2!".len());

    // Shift+ArrowLeft selection + Delete also works on password input
    pw.set("secret99".to_string());
    let _ = tree.build_paint_scene(320, 100);
    assert!(tree.dispatch_cursor_end());
    assert!(tree.dispatch_select_left());
    assert!(tree.dispatch_select_left());
    assert!(tree.dispatch_delete());
    assert_eq!(pw.get(), "secret");
}

#[test]
fn textarea_multiline_ime_cursor_area_tracks_line_top() {
    reset_reactive_runtime();
    let memo = Signal::new(String::from("첫째 줄 텍스트\n둘째 줄 한글"));
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                div { display: flex; width: 360px; height: 180px; padding: 12px; }
                textarea { width: 260px; height: 96px; }
            </style>
            <div>
                <textarea id="memo-area" bind:value="{memo}"></textarea>
            </div>
        "#
        )
    });

    let _ = tree.build_paint_scene(360, 180);
    let area_id = find_id(&tree, "memo-area");
    assert!(tree.set_focus_to_node(area_id));

    assert!(tree.dispatch_cursor_up());
    assert!(tree.dispatch_cursor_home());
    let line1_ime_rect = tree.focused_ime_cursor_area().expect("line 1 ime rect");

    assert!(tree.dispatch_cursor_down());
    let line2_ime_rect = tree.focused_ime_cursor_area().expect("line 2 ime rect");
    assert!(
        line2_ime_rect.y > line1_ime_rect.y + 10.0,
        "multiline <textarea> IME cursor area Y must advance on line 2: line1={line1_ime_rect:?}, line2={line2_ime_rect:?}"
    );
}

#[test]
fn link_krui_preset_propagates_to_tree_and_all_four_data_themes_differ() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                [data-theme="fluent"] #theme-btn { background-color: #0078d4; border-radius: 4px; }
                [data-theme="material"] #theme-btn { background-color: #6750a4; border-radius: 999px; }
                [data-theme="liquid-glass"] #theme-btn { background-color: #0ea5e9; border-radius: 14px; }
                [data-theme="cosmic"] #theme-btn { background-color: #8b5cf6; border-radius: 8px; }
            </style>
            <div id="host" data-theme="liquid-glass" style="width: 320px; height: 120px;">
                <button id="theme-btn" style="width: 140px; height: 38px">유리 버튼</button>
            </div>
        "#
        )
    });
    let _ = tree.build_paint_scene(320, 120);
    let host = find_id(&tree, "host");
    let btn = find_id(&tree, "theme-btn");
    let glass = tree.node_computed_style(btn).unwrap().paint;
    assert_eq!(glass.background_color, Color::rgb(0x0e, 0xa5, 0xe9));
    assert!((glass.border_radius - 14.0).abs() < 1e-3);

    tree.set_node_attr(host, "data-theme", "fluent");
    let _ = tree.build_paint_scene(320, 120);
    let fluent = tree.node_computed_style(btn).unwrap().paint;
    assert_ne!(fluent.background_color, glass.background_color);
    assert_ne!(fluent.border_radius, glass.border_radius);
}

#[test]
fn a11y_tree_update_never_references_missing_children() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                .root { width: 320px; height: 240px; }
                .hidden-pe { pointer-events: none; opacity: 0; }
                .vis-hidden { visibility: hidden; }
                .vis-visible { visibility: visible; }
                .disp-none { display: none; }
            </style>
            <div class="root">
                <button id="ok-btn">확인</button>
                <div class="hidden-pe">
                    <button id="ghost-btn">숨김 버튼</button>
                </div>
                <div class="vis-hidden">
                    <button id="revealed-btn" class="vis-visible">보이는 자식</button>
                </div>
                <div class="disp-none">
                    <button id="none-btn">없음</button>
                </div>
            </div>
        "#
        )
    });
    let _ = tree.build_paint_scene(320, 240);
    let update = tree.build_accessibility_tree(320.0, 240.0);
    let present: std::collections::HashSet<_> = update.nodes.iter().map(|(id, _)| *id).collect();
    for (id, node) in &update.nodes {
        for child_id in node.children() {
            assert!(
                present.contains(child_id),
                "Node {id:?} references missing child {child_id:?} in TreeUpdate"
            );
        }
    }
    assert!(present.contains(&update.focus));
    let revealed = accesskit::NodeId(A11yId::from(find_id(&tree, "revealed-btn")).0);
    assert!(present.contains(&revealed));
}
