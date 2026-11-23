//! Stage C4 Regression Suite:
//! - `<label for="...">` and nested `<label>` click focus forwarding & control activation
//! - `<input type="checkbox">` (`checked`, `bind:checked`, `Space` / click toggle, `:checked` style & a11y)
//! - `<input type="radio" name="...">` single-selection radio group & arrow-key cycling
//! - `<input type="range" min max step>` stepping, clamping, `Home`/`End`, pointer drag, and Slider primitives
//! - `<textarea rows="...">` multiline editing (`Enter` newline, `ArrowUp`/`ArrowDown`/`Home`/`End`, IME)
//! - `:checked` and `:invalid` compile-time state style table overrides

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
fn label_click_focuses_control() {
    reset_reactive_runtime();
    let email = Signal::new(String::new());
    let nested = Signal::new(String::new());
    let agreed = Signal::new(false);

    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                div { display: flex; flex-direction: column; width: 480px; height: 320px; padding: 12px; gap: 8px; }
                label { width: 220px; height: 28px; }
                input { width: 220px; height: 32px; }
            </style>
            <div>
                <label id="email-label" for="email-input">이메일 주소</label>
                <input id="email-input" type="email" bind:value="{email}" />
                <label id="nested-label">
                    <span>중첩 라벨</span>
                    <input id="nested-input" bind:value="{nested}" />
                </label>
                <label id="chk-label" for="agree-chk">약관 동의</label>
                <input id="agree-chk" type="checkbox" bind:checked="{agreed}" />
            </div>
        "#
        )
    });

    let _ = tree.build_paint_scene(480, 320);
    let (email_lbl, email_inp) = (find_id(&tree, "email-label"), find_id(&tree, "email-input"));
    let (nested_lbl, nested_inp) = (
        find_id(&tree, "nested-label"),
        find_id(&tree, "nested-input"),
    );
    let (chk_lbl, chk_id) = (find_id(&tree, "chk-label"), find_id(&tree, "agree-chk"));

    let (elx, ely) = center_of(&tree, email_lbl);
    assert!(tree.dispatch_click(elx, ely));
    assert_eq!(tree.focused_node_id(), Some(email_inp));

    let nl_rect = tree.node_layout(nested_lbl).unwrap();
    assert!(tree.dispatch_click(nl_rect.x + 4.0, nl_rect.y + 4.0));
    assert_eq!(tree.focused_node_id(), Some(nested_inp));

    assert!(!tree.is_checked(chk_id) && !agreed.get());
    let (clx, cly) = center_of(&tree, chk_lbl);
    assert!(tree.dispatch_click(clx, cly));
    assert_eq!(tree.focused_node_id(), Some(chk_id));
    assert!(tree.is_checked(chk_id) && agreed.get());
}

#[test]
fn checkbox_space_toggles() {
    reset_reactive_runtime();
    let agreed = Signal::new(false);
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                div { display: flex; width: 320px; height: 160px; padding: 16px; }
                input:checked { background-color: #107c41; }
            </style>
            <div>
                <input id="terms-chk" type="checkbox" bind:checked="{agreed}" />
            </div>
        "#
        )
    });

    let chk_id = find_id(&tree, "terms-chk");
    let (scene_unchecked, _) = tree.build_paint_scene(320, 160);
    assert!(!tree.is_checked(chk_id));

    assert_eq!(tree.dispatch_key_tab(false), Some(chk_id));
    let space_action = tree.dispatch_routed_key(&KeyInfo::Space, Mods::default(), false);
    assert_eq!(space_action, KeyAction::Space);
    assert!(tree.dispatch_key_space());
    assert!(tree.is_checked(chk_id) && agreed.get());

    let (scene_checked, _) = tree.build_paint_scene(320, 160);
    assert!(scene_checked.primitives.len() > scene_unchecked.primitives.len());
    assert!(has_styled_box_bg(
        &scene_checked,
        Color::rgb(0x10, 0x7c, 0x41)
    ));
    assert_checkbox_a11y_toggled(&mut tree, chk_id);

    assert!(tree.dispatch_key_space());
    assert!(!tree.is_checked(chk_id) && !agreed.get());
}

fn has_styled_box_bg(scene: &PaintScene, expected: Color) -> bool {
    scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox {
                background_color,
                ..
            } if *background_color == expected
        )
    })
}

fn has_styled_box_border(scene: &PaintScene, expected: Color) -> bool {
    scene.primitives.iter().any(|p| match p {
        DrawPrimitive::StyledBox { border_color, .. } => *border_color == expected,
        DrawPrimitive::CanvasBatch { commands, .. } => commands.iter().any(
            |cmd| matches!(cmd, CanvasCommand::StrokePath { color, .. } if *color == expected),
        ),
        _ => false,
    })
}

fn assert_checkbox_a11y_toggled(tree: &mut ViewTree, chk_id: NodeId) {
    let a11y_update = tree.build_accessibility_tree(320.0, 160.0);
    let ak_id = accesskit::NodeId(A11yId::from(chk_id).0);
    let ak_node = a11y_update
        .nodes
        .iter()
        .find_map(|(id, n)| (*id == ak_id).then_some(n))
        .expect("checkbox node in accesskit tree");
    assert_eq!(ak_node.role(), accesskit::Role::CheckBox);
    assert_eq!(ak_node.toggled(), Some(accesskit::Toggled::True));
}

#[test]
fn radio_group_single_selection() {
    reset_reactive_runtime();
    let free_sel = Signal::new(true);
    let pro_sel = Signal::new(false);
    let ent_sel = Signal::new(false);

    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                div { display: flex; flex-direction: row; width: 360px; height: 120px; padding: 12px; gap: 12px; }
            </style>
            <div>
                <input id="r-free" type="radio" name="tier" value="free" bind:checked="{free_sel}" />
                <input id="r-pro" type="radio" name="tier" value="pro" bind:checked="{pro_sel}" />
                <input id="r-ent" type="radio" name="tier" value="ent" bind:checked="{ent_sel}" />
            </div>
        "#
        )
    });

    let _ = tree.build_paint_scene(360, 120);
    let (r_free, r_pro, r_ent) = (
        find_id(&tree, "r-free"),
        find_id(&tree, "r-pro"),
        find_id(&tree, "r-ent"),
    );

    assert!(tree.is_checked(r_free) && !tree.is_checked(r_pro) && !tree.is_checked(r_ent));

    let (px, py) = center_of(&tree, r_pro);
    assert!(tree.dispatch_click(px, py));
    assert!(!free_sel.get() && pro_sel.get() && !ent_sel.get());
    assert_eq!(tree.focused_node_id(), Some(r_pro));

    let right_act = tree.dispatch_routed_key(&KeyInfo::ArrowRight, Mods::default(), false);
    assert_eq!(right_act, KeyAction::CursorRight);
    assert!(tree.dispatch_cursor_right());
    assert_eq!(tree.focused_node_id(), Some(r_ent));
    assert!(!free_sel.get() && !pro_sel.get() && ent_sel.get());

    assert!(tree.dispatch_cursor_right());
    assert_eq!(tree.focused_node_id(), Some(r_free));
    assert!(free_sel.get() && !pro_sel.get() && !ent_sel.get());
}

#[test]
fn range_arrow_step() {
    reset_reactive_runtime();
    let volume = Signal::new(20.0_f32);
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                div { display: flex; width: 360px; height: 120px; padding: 16px; }
                input { width: 200px; height: 24px; }
            </style>
            <div>
                <input id="vol-slider" type="range" min="10" max="50" step="5" bind:value="{volume}" />
            </div>
        "#
        )
    });

    let slider_id = find_id(&tree, "vol-slider");
    let (scene, _) = tree.build_paint_scene(360, 120);
    assert!(scene.primitives.len() >= 3);
    assert_eq!(tree.node_range_value(slider_id), Some(20.0));

    assert_eq!(tree.dispatch_key_tab(false), Some(slider_id));
    assert!(tree.dispatch_cursor_right());
    assert_eq!(
        (tree.node_range_value(slider_id), volume.get()),
        (Some(25.0), 25.0)
    );

    assert!(tree.dispatch_cursor_up());
    assert_eq!(
        (tree.node_range_value(slider_id), volume.get()),
        (Some(30.0), 30.0)
    );

    assert!(tree.dispatch_cursor_left());
    assert_eq!(
        (tree.node_range_value(slider_id), volume.get()),
        (Some(25.0), 25.0)
    );

    assert!(tree.dispatch_cursor_end());
    assert_eq!(
        (tree.node_range_value(slider_id), volume.get()),
        (Some(50.0), 50.0)
    );
    assert!(tree.dispatch_cursor_right());
    assert_eq!(tree.node_range_value(slider_id), Some(50.0));

    assert!(tree.dispatch_cursor_home());
    assert_eq!(
        (tree.node_range_value(slider_id), volume.get()),
        (Some(10.0), 10.0)
    );
    assert!(tree.dispatch_cursor_left());
    assert_eq!(tree.node_range_value(slider_id), Some(10.0));
}

#[test]
fn textarea_multiline_editing() {
    reset_reactive_runtime();
    let notes = Signal::new(String::new());
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                div { display: flex; width: 420px; height: 260px; padding: 12px; }
                textarea { width: 320px; }
                textarea:invalid { border-color: #d83b01; }
            </style>
            <div>
                <textarea id="memo-area" rows="4" bind:value="{notes}" placeholder="메모를 입력하세요" />
            </div>
        "#
        )
    });

    let memo_id = find_id(&tree, "memo-area");
    let _ = tree.build_paint_scene(420, 260);
    let rect = tree.node_layout(memo_id).expect("textarea layout");
    assert!(rect.height >= 80.0);

    assert_eq!(tree.dispatch_key_tab(false), Some(memo_id));
    assert!(tree.dispatch_text_input("첫째줄"));
    assert!(tree.dispatch_key_enter());
    assert!(tree.dispatch_ime_preedit("둘째"));
    assert!(tree.ime_commit("둘째줄입니다"));
    assert_eq!(notes.get(), "첫째줄\n둘째줄입니다");

    assert!(tree.dispatch_cursor_home());
    assert_eq!(tree.node_cursor_char_idx(memo_id), Some(4));
    assert!(tree.dispatch_cursor_end());
    assert_eq!(tree.node_cursor_char_idx(memo_id), Some(10));
    assert!(tree.dispatch_cursor_up());
    assert_eq!(tree.node_cursor_char_idx(memo_id), Some(3));
    assert!(tree.dispatch_cursor_left());
    assert_eq!(tree.node_cursor_char_idx(memo_id), Some(2));
    assert!(tree.dispatch_cursor_down());
    assert_eq!(tree.node_cursor_char_idx(memo_id), Some(6));

    tree.set_invalid(memo_id, true);
    assert!(tree.is_invalid(memo_id));
    let (scene_invalid, _) = tree.build_paint_scene(420, 260);
    assert!(has_styled_box_border(
        &scene_invalid,
        Color::rgb(0xd8, 0x3b, 0x01)
    ));
}
