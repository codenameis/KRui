//! Stage `C0` desktop input reproduction and instrumentation tests (`key_info_named_key_mapping`,
//! `route_key_arrow_modifier_matrix`, `example_07_control_acceptance`).
//!
//! Records and verifies the baseline (`f9bf919`) desktop keyboard mapping, modifier matrix,
//! and short-text / long-text / IME / disabled / selectable control behavior with both
//! `cursor_char_idx` and `PaintScene` caret rectangle coordinates.

#![forbid(unsafe_code)]

use krui::*;
use winit::keyboard::{Key, NamedKey, SmolStr};

const LONG_TEXT: &str =
    "This is a very long text string designed to overflow the 200px single-line input box cleanly";
const DISABLED_TEXT: &str = "Disabled input (immutable)";

#[view_model]
#[derive(Clone)]
struct ControlsLabViewModel {
    #[state]
    short_text: String,
    #[state]
    long_text: String,
    #[state]
    disabled_text: String,
    #[state]
    is_locked: bool,
}

impl ControlsLabViewModel {
    fn new() -> Self {
        Self {
            short_text: Signal::new("Hello".to_string()),
            long_text: Signal::new(LONG_TEXT.to_string()),
            disabled_text: Signal::new(DISABLED_TEXT.to_string()),
            is_locked: Signal::new(true),
        }
    }
}

const VIEWPORT_W: u32 = 1024;
const VIEWPORT_H: u32 = 720;

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
    assert!(
        (ra.width - rb.width).abs() < 1e-3,
        "rect.width mismatch: {ra:?} vs {rb:?}"
    );
    assert!(
        (ra.height - rb.height).abs() < 1e-3,
        "rect.height mismatch: {ra:?} vs {rb:?}"
    );
}

fn step_and_measure_caret(
    tree: &mut ViewTree,
    node_id: NodeId,
) -> (usize, Option<usize>, Option<LayoutRect>, Option<LayoutRect>) {
    let (scene, _) = tree.build_paint_scene(VIEWPORT_W, VIEWPORT_H);
    let caret_idx = tree.node_cursor_char_idx(node_id).unwrap_or(0);
    let sel_anchor = tree.node_selection_anchor_char_idx(node_id);
    let painted_caret = extract_painted_caret_rect(&scene, tree.node_layout(node_id));
    let ime_caret = tree.focused_ime_cursor_area();
    (caret_idx, sel_anchor, painted_caret, ime_caret)
}

#[test]
fn key_info_named_key_mapping() {
    let table: &[(Key, Option<&str>, KeyInfo)] = &[
        (Key::Named(NamedKey::Escape), None, KeyInfo::Escape),
        (Key::Named(NamedKey::Tab), None, KeyInfo::Tab),
        (Key::Named(NamedKey::Backspace), None, KeyInfo::Backspace),
        (Key::Named(NamedKey::Delete), None, KeyInfo::Delete),
        (Key::Named(NamedKey::ArrowLeft), None, KeyInfo::ArrowLeft),
        (Key::Named(NamedKey::ArrowRight), None, KeyInfo::ArrowRight),
        (Key::Named(NamedKey::ArrowUp), None, KeyInfo::ArrowUp),
        (Key::Named(NamedKey::ArrowDown), None, KeyInfo::ArrowDown),
        (Key::Named(NamedKey::Home), None, KeyInfo::Home),
        (Key::Named(NamedKey::End), None, KeyInfo::End),
        (Key::Named(NamedKey::Enter), None, KeyInfo::Enter),
        (Key::Named(NamedKey::Space), Some(" "), KeyInfo::Space),
        (Key::Named(NamedKey::PageUp), None, KeyInfo::PageUp),
        (Key::Named(NamedKey::PageDown), None, KeyInfo::PageDown),
        (
            Key::Character(SmolStr::new("a")),
            Some("a"),
            KeyInfo::Character {
                logical: "a".to_string(),
                text: Some("a".to_string()),
            },
        ),
    ];

    for (winit_key, text, expected) in table {
        let actual = key_info_from_logical_key(winit_key, *text);
        assert_eq!(
            &actual, expected,
            "NamedKey mapping mismatch for {winit_key:?}"
        );
    }
}

fn expected_arrow_modifier_rows() -> [(KeyInfo, KeyAction, KeyAction, KeyAction, KeyAction); 6] {
    let up_key = key_info_from_logical_key(&Key::Named(NamedKey::ArrowUp), None);
    let down_key = key_info_from_logical_key(&Key::Named(NamedKey::ArrowDown), None);
    [
        (
            KeyInfo::ArrowLeft,
            KeyAction::CursorLeft,
            KeyAction::SelectLeft,
            KeyAction::WordLeft,
            KeyAction::SelectWordLeft,
        ),
        (
            KeyInfo::ArrowRight,
            KeyAction::CursorRight,
            KeyAction::SelectRight,
            KeyAction::WordRight,
            KeyAction::SelectWordRight,
        ),
        (
            up_key,
            KeyAction::CursorUp,
            KeyAction::SelectUp,
            KeyAction::CursorHome,
            KeyAction::SelectHome,
        ),
        (
            down_key,
            KeyAction::CursorDown,
            KeyAction::SelectDown,
            KeyAction::CursorEnd,
            KeyAction::SelectEnd,
        ),
        (
            KeyInfo::Home,
            KeyAction::CursorHome,
            KeyAction::SelectHome,
            KeyAction::CursorHome,
            KeyAction::SelectHome,
        ),
        (
            KeyInfo::End,
            KeyAction::CursorEnd,
            KeyAction::SelectEnd,
            KeyAction::CursorEnd,
            KeyAction::SelectEnd,
        ),
    ]
}

#[test]
fn route_key_arrow_modifier_matrix() {
    let focused = FocusState {
        is_input_focused: true,
        has_ime_preedit: false,
        is_ime_enabled: false,
    };
    let composing = FocusState {
        is_input_focused: true,
        has_ime_preedit: true,
        is_ime_enabled: true,
    };
    let none = Mods::default();
    let shift = Mods {
        shift: true,
        ..Mods::default()
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

    for (key, exp_none, exp_shift, exp_ctrl, exp_ctrl_shift) in &expected_arrow_modifier_rows() {
        assert_eq!(route_key(key, none, &focused), *exp_none);
        assert_eq!(route_key(key, shift, &focused), *exp_shift);
        assert_eq!(route_key(key, ctrl, &focused), *exp_ctrl);
        assert_eq!(route_key(key, ctrl_shift, &focused), *exp_ctrl_shift);
        for m in [none, shift, ctrl, ctrl_shift] {
            assert_eq!(route_key(key, m, &composing), KeyAction::None);
        }
    }
}

#[test]
fn example_07_control_acceptance() {
    let vm = ControlsLabViewModel::new();
    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        krui!(
            r#"
            <style>
                .root-shell { display: flex; flex-direction: column; width: 960px; padding: 16px; gap: 10px; }
                .short-box { width: 220px; height: 34px; padding: 6px 10px; }
                .long-box { width: 200px; height: 34px; padding: 6px 10px; }
                .disabled-box { width: 200px; height: 34px; padding: 6px 10px; }
                .selectable-box { width: 320px; height: 72px; padding: 6px 10px; }
            </style>
            <div class="root-shell">
                <input id="short-input" class="short-box" required bind:value="{vm.short_text}" />
                <input id="long-input" class="long-box" bind:value="{vm.long_text}" />
                <input id="disabled-input" class="disabled-box" bind:value="{vm.disabled_text}" disabled="{vm.is_locked}" />
                <p id="selectable-paragraph" class="selectable-box" selectable="true">Selectable paragraph text for focus separation verification.</p>
            </div>
            "#
        )
    });
    let _ = tree.build_paint_scene(VIEWPORT_W, VIEWPORT_H);

    let short_id = tree.find_by_id("short-input").expect("short-input");
    let long_id = tree.find_by_id("long-input").expect("long-input");
    let disabled_id = tree.find_by_id("disabled-input").expect("disabled-input");
    let para_id = tree
        .find_by_id("selectable-paragraph")
        .expect("selectable-paragraph");

    verify_short_input_arrow_and_modifiers(&mut tree, short_id);
    verify_long_input_wrap_and_caret_clamp(&mut tree, long_id);
    verify_ime_preedit_and_non_editable_focus(&mut tree, short_id, disabled_id, para_id);
}

fn verify_short_input_arrow_and_modifiers(tree: &mut ViewTree, short_id: NodeId) {
    assert!(tree.set_focus_to_node(short_id));
    let focus_state = FocusState {
        is_input_focused: true,
        has_ime_preedit: false,
        is_ime_enabled: false,
    };
    let (idx_end, _, painted_end, ime_end) = step_and_measure_caret(tree, short_id);
    assert_eq!(idx_end, 5);
    assert_rects_approx_eq(painted_end, ime_end);

    // Non-reproduced on short text: ArrowLeft / ArrowRight / Home / End move caret & PaintScene rect
    apply_key_action(
        tree,
        route_key(&KeyInfo::ArrowLeft, Mods::default(), &focus_state),
    );
    let (idx_left, _, painted_left, ime_left) = step_and_measure_caret(tree, short_id);
    assert_eq!(idx_left, 4);
    assert_rects_approx_eq(painted_left, ime_left);
    assert!(painted_left.unwrap().x < painted_end.unwrap().x);

    apply_key_action(
        tree,
        route_key(&KeyInfo::Home, Mods::default(), &focus_state),
    );
    let (idx_home, _, painted_home, _) = step_and_measure_caret(tree, short_id);
    assert_eq!(idx_home, 0);
    assert!(painted_home.unwrap().x < painted_left.unwrap().x);

    apply_key_action(
        tree,
        route_key(&KeyInfo::End, Mods::default(), &focus_state),
    );
    let (idx_back, _, painted_back, _) = step_and_measure_caret(tree, short_id);
    assert_eq!(idx_back, 5);
    assert_rects_approx_eq(painted_back, painted_end);

    // Shift+ArrowLeft preserves selection anchor and shrinks/extends selection
    let _ = tree.select_all();
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    apply_key_action(tree, route_key(&KeyInfo::ArrowLeft, shift, &focus_state));
    let (idx_shift, sel_after_shift, _, _) = step_and_measure_caret(tree, short_id);
    assert_eq!(idx_shift, 4);
    assert_eq!(sel_after_shift, Some(0));
}

fn verify_long_input_wrap_and_caret_clamp(tree: &mut ViewTree, long_id: NodeId) {
    assert!(tree.set_focus_to_node(long_id));
    let long_chars = LONG_TEXT.chars().count();
    let box_rect = tree.node_layout(long_id).expect("long-input layout");
    let clamp_max_x =
        box_rect.x + box_rect.width - krui_core::view_tree::key_routing::CARET_RIGHT_MARGIN;

    let (idx_end, _, painted_end, ime_end) = step_and_measure_caret(tree, long_id);
    assert_eq!(idx_end, long_chars);
    assert_rects_approx_eq(painted_end, ime_end);
    let end_rect = painted_end.expect("painted caret at end of long input");
    assert!(end_rect.x <= clamp_max_x + 0.01);

    // Press ArrowLeft in long single-line input: caret x moves left smoothly without freezing
    let focus_state = FocusState {
        is_input_focused: true,
        has_ime_preedit: false,
        is_ime_enabled: false,
    };
    apply_key_action(
        tree,
        route_key(&KeyInfo::ArrowLeft, Mods::default(), &focus_state),
    );
    let (idx_prev1, _, painted_prev1, ime_prev1) = step_and_measure_caret(tree, long_id);
    assert_eq!(idx_prev1, long_chars - 1);
    assert_rects_approx_eq(painted_prev1, ime_prev1);

    apply_key_action(
        tree,
        route_key(&KeyInfo::ArrowLeft, Mods::default(), &focus_state),
    );
    let (idx_prev2, _, painted_prev2, ime_prev2) = step_and_measure_caret(tree, long_id);
    assert_eq!(idx_prev2, long_chars - 2);
    assert_rects_approx_eq(painted_prev2, ime_prev2);

    let prev1_rect = painted_prev1.expect("painted caret at long_chars - 1");
    let prev2_rect = painted_prev2.expect("painted caret at long_chars - 2");
    assert!(
        prev2_rect.x < prev1_rect.x && prev1_rect.x < end_rect.x,
        "Long-input caret must move strictly left on each ArrowLeft: prev2_x={}, prev1_x={}, end_x={}",
        prev2_rect.x,
        prev1_rect.x,
        end_rect.x,
    );
}

fn verify_ime_preedit_and_non_editable_focus(
    tree: &mut ViewTree,
    short_id: NodeId,
    disabled_id: NodeId,
    para_id: NodeId,
) {
    // 1. Active IME preedit blocks arrow key routing until composition finishes
    assert!(tree.set_focus_to_node(short_id));
    let _ = tree.dispatch_ime_event(ImeEvent::Enabled);
    let _ = tree.dispatch_ime_event(ImeEvent::Preedit {
        text: "한".to_string(),
        cursor_offset: Some((0, "한".len())),
    });
    assert!(tree.has_active_ime_preedit());
    let composing_state = FocusState {
        is_input_focused: true,
        has_ime_preedit: tree.has_active_ime_preedit(),
        is_ime_enabled: true,
    };
    assert_eq!(
        route_key(&KeyInfo::ArrowLeft, Mods::default(), &composing_state),
        KeyAction::None
    );
    let _ = tree.dispatch_ime_event(ImeEvent::Commit("한".to_string()));
    assert!(!tree.has_active_ime_preedit());

    // 2. Clicking selectable paragraph blurs input (selection_owner holds `<p>` without stealing keyboard focus)
    let para_rect = tree
        .node_layout(para_id)
        .expect("selectable paragraph rect");
    let _ = tree.dispatch_click(para_rect.x + 12.0, para_rect.y + 8.0);
    assert_eq!(tree.focused_node_id(), None);
    assert_eq!(tree.selection_owner(), Some(para_id));
    assert_eq!(tree.focused_input_rect(), None);

    // 3. Disabled input rejects text mutation even if reached by Tab in C0
    assert_eq!(tree.node_text(disabled_id).as_deref(), Some(DISABLED_TEXT));
    let _ = tree.build_paint_scene(VIEWPORT_W, VIEWPORT_H);
}
