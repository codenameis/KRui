//! Stage C2 verification suite:
//! - `disabled_nodes_are_skipped_by_tab`: disabled nodes and dynamically disabled focused nodes are excluded from Tab order
//! - `focus_blur_order_on_click_and_tab`: `onfocus`/`onblur`/`onchange` order on click, Tab, and IME preedit transition
//! - `press_outside_release_does_not_click`: pointer down/move/up lifecycle and release-outside click cancellation
//! - `held_space_activates_once`: repeat `Space`/`Enter` suppression on buttons while allowing `Space` repeat in inputs
//! - `onkeydown_can_prevent_default`: `onkeydown` returning `true` or calling `prevent_default_key()` suppresses default routing
//! - `window_focus_loss_clears_ime_and_mods`: `dispatch_window_focus_lost()` commits IME preedit and clears active pointer state

#![forbid(unsafe_code)]

use krui::*;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn disabled_nodes_are_skipped_by_tab() {
    let is_mid_disabled = Signal::new(true);
    let view = krui!(
        r#"
        <div class="col">
            <button id="first">First</button>
            <input id="mid" disabled="{is_mid_disabled}" value="mid" />
            <button id="last">Last</button>
        </div>
        <style>
            div.col { display: flex; flex-direction: column; width: 300px; height: 200px; gap: 8px; }
            button, input { width: 200px; height: 36px; }
        </style>
        "#
    );
    let mut tree = ViewTree::new();
    let _roots = view.mount(&mut tree, None);
    let _ = tree.build_paint_scene(300, 200);

    let first_id = tree.find_by_id("first").unwrap();
    let mid_id = tree.find_by_id("mid").unwrap();
    let last_id = tree.find_by_id("last").unwrap();

    // Tab skips disabled `mid` and cycles between `first` and `last` in document order.
    assert_eq!(tree.dispatch_key_tab(false), Some(first_id));
    assert_eq!(tree.dispatch_key_tab(false), Some(last_id));
    assert_eq!(tree.dispatch_key_tab(false), Some(first_id));

    // Enabling `mid` includes it in document Tab order; disabling while focused blurs it immediately.
    is_mid_disabled.set(false);
    assert_eq!(tree.dispatch_key_tab(false), Some(mid_id));
    assert_eq!(tree.focused_node_id(), Some(mid_id));

    is_mid_disabled.set(true);
    assert_eq!(tree.focused_node_id(), None);
}

fn push_cmd(events: &Rc<RefCell<Vec<String>>>, label: &'static str) -> Command {
    let ev = events.clone();
    Command::new(move || ev.borrow_mut().push(label.to_string()))
}

#[test]
fn focus_blur_order_on_click_and_tab() {
    let events: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let log_in_focus = push_cmd(&events, "input:focus");
    let log_in_blur = push_cmd(&events, "input:blur");
    let log_in_change = {
        let ev = events.clone();
        InputCommand::new(move |v: String| ev.borrow_mut().push(format!("input:change:{v}")))
    };
    let log_btn_focus = push_cmd(&events, "btn:focus");
    let log_btn_blur = push_cmd(&events, "btn:blur");

    let view = krui!(
        r#"
        <div class="col">
            <input id="inp" onfocus="{log_in_focus}" onblur="{log_in_blur}" onchange="{log_in_change}" />
            <button id="btn" onfocus="{log_btn_focus}" onblur="{log_btn_blur}">Action</button>
        </div>
        <style>
            div.col { display: flex; flex-direction: column; width: 300px; height: 200px; padding: 10px; gap: 10px; }
            input, button { width: 200px; height: 40px; }
        </style>
        "#
    );
    let mut tree = ViewTree::new();
    let _roots = view.mount(&mut tree, None);
    let _ = tree.build_paint_scene(300, 200);

    let _ = tree.dispatch_key_tab(false);
    let _ = tree.dispatch_text_input("abc");
    let _ = tree.dispatch_ime_event(ImeEvent::Preedit {
        text: "한".to_string(),
        cursor_offset: Some((1, 1)),
    });
    // Tabbing from `inp` to `btn` commits preedit, fires `onchange`, then `inp:blur`, then `btn:focus`.
    let _ = tree.dispatch_key_tab(false);
    // Clicking background blurs `btn`.
    let _ = tree.dispatch_pointer_down(280.0, 180.0);
    tree.end_pointer_drag();

    assert_eq!(
        *events.borrow(),
        vec![
            "input:focus".to_string(),
            "input:change:abc한".to_string(),
            "input:blur".to_string(),
            "btn:focus".to_string(),
            "btn:blur".to_string(),
        ]
    );
}

#[test]
fn press_outside_release_does_not_click() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let on_down = {
        let l = log.clone();
        Command::new(move || l.borrow_mut().push("down"))
    };
    let on_move = {
        let l = log.clone();
        Command::new(move || l.borrow_mut().push("move"))
    };
    let on_up = {
        let l = log.clone();
        Command::new(move || l.borrow_mut().push("up"))
    };
    let on_click = {
        let l = log.clone();
        Command::new(move || l.borrow_mut().push("click"))
    };

    let view = krui!(
        r#"
        <div class="wrap">
            <button id="btn"
                onpointerdown="{on_down}"
                onpointermove="{on_move}"
                onpointerup="{on_up}"
                onclick="{on_click}">Press</button>
        </div>
        <style>
            div.wrap { display: flex; width: 300px; height: 200px; padding: 10px; }
            button { width: 120px; height: 40px; }
        </style>
        "#
    );
    let mut tree = ViewTree::new();
    let _roots = view.mount(&mut tree, None);
    let _ = tree.build_paint_scene(300, 200);

    // 1) Press inside (30, 25), move inside (40, 25), move outside (250, 150), release outside ->
    //    pointer capture delivers `move` and `up` to `#btn` while suppressing `click`.
    assert!(tree.dispatch_pointer_down(30.0, 25.0));
    let _ = tree.dispatch_pointer_move(40.0, 25.0);
    let _ = tree.dispatch_pointer_move(250.0, 150.0);
    tree.end_pointer_drag();
    assert_eq!(*log.borrow(), vec!["down", "move", "move", "up"]);

    // 2) Press inside and release inside -> `down`, `up`, `click` all fire.
    log.borrow_mut().clear();
    assert!(tree.dispatch_pointer_down(30.0, 25.0));
    tree.end_pointer_drag();
    assert_eq!(*log.borrow(), vec!["down", "up", "click"]);
}

#[test]
fn held_space_activates_once() {
    let clicks = Signal::new(0_i32);
    let on_click = {
        let c = clicks;
        Command::new(move || c.update(|v| *v += 1))
    };
    let view = krui!(
        r#"
        <div class="row">
            <button id="btn" onclick="{on_click}">Activate</button>
            <input id="inp" value="" />
        </div>
        <style>
            div.row { display: flex; width: 320px; height: 100px; gap: 8px; }
            button, input { width: 120px; height: 36px; }
        </style>
        "#
    );
    let mut tree = ViewTree::new();
    let _roots = view.mount(&mut tree, None);
    let _ = tree.build_paint_scene(320, 100);

    // Focus button: initial Space activates once; repeated Space/Enter while held does not re-trigger!
    let _ = tree.dispatch_key_tab(false);
    let a1 = tree.dispatch_routed_key(&KeyInfo::Space, Mods::default(), false);
    apply_key_action(&mut tree, a1);
    let a2 = tree.dispatch_routed_key(&KeyInfo::Space, Mods::default(), true);
    apply_key_action(&mut tree, a2);
    let a3 = tree.dispatch_routed_key(&KeyInfo::Enter, Mods::default(), true);
    apply_key_action(&mut tree, a3);
    assert_eq!(clicks.get(), 1);

    // Focus input: repeated Space inserts spaces into the text field.
    let inp_id = tree.dispatch_key_tab(false).unwrap();
    for repeat in [false, true, true] {
        let act = tree.dispatch_routed_key(&KeyInfo::Space, Mods::default(), repeat);
        apply_key_action(&mut tree, act);
    }
    assert_eq!(tree.node_text(inp_id).as_deref(), Some("   "));
}

#[test]
fn onkeydown_can_prevent_default() {
    let intercepted = Signal::new(String::new());
    let on_kd = {
        let ic = intercepted;
        InputCommand::with_prevent_default(move |key: String| {
            if key == "Tab" || key == "a" {
                ic.set(key);
                true
            } else {
                false
            }
        })
    };
    let view = krui!(
        r#"
        <div class="row">
            <input id="first" onkeydown="{on_kd}" value="" />
            <button id="second">Next</button>
        </div>
        <style>
            div.row { display: flex; width: 300px; height: 100px; gap: 8px; }
            input, button { width: 120px; height: 36px; }
        </style>
        "#
    );
    let mut tree = ViewTree::new();
    let _roots = view.mount(&mut tree, None);
    let _ = tree.build_paint_scene(300, 100);

    let first_id = tree.dispatch_key_tab(false).unwrap();
    // `Tab` is intercepted by `onkeydown` returning `true`, so focus stays on `first`.
    let tab_act = tree.dispatch_routed_key(&KeyInfo::Tab, Mods::default(), false);
    assert_eq!(tab_act, KeyAction::None);
    assert_eq!(tree.focused_node_id(), Some(first_id));
    assert_eq!(intercepted.get(), "Tab");

    // Direct closure calling `prevent_default_key()` also suppresses character insertion.
    tree.set_on_keydown(first_id, |key| {
        if key == "x" {
            prevent_default_key();
        }
    });
    let x_act = tree.dispatch_routed_key(
        &KeyInfo::Character {
            logical: "x".to_string(),
            text: Some("x".to_string()),
        },
        Mods::default(),
        false,
    );
    assert_eq!(x_act, KeyAction::None);
}

#[test]
fn window_focus_loss_clears_ime_and_mods() {
    let clicks = Signal::new(0_i32);
    let on_click = {
        let c = clicks;
        Command::new(move || c.update(|v| *v += 1))
    };
    let view = krui!(
        r#"
        <div class="row">
            <input id="inp" value="초기" />
            <button id="btn" onclick="{on_click}">Btn</button>
        </div>
        <style>
            div.row { display: flex; width: 300px; height: 100px; padding: 10px; gap: 10px; }
            input, button { width: 120px; height: 36px; }
        </style>
        "#
    );
    let mut tree = ViewTree::new();
    let _roots = view.mount(&mut tree, None);
    let _ = tree.build_paint_scene(300, 100);

    let inp_id = tree.dispatch_key_tab(false).unwrap();
    let _ = tree.dispatch_ime_event(ImeEvent::Enabled);
    let _ = tree.dispatch_ime_event(ImeEvent::Preedit {
        text: "값".to_string(),
        cursor_offset: Some((1, 1)),
    });
    // Start a press on the button, then lose window focus before release.
    let _ = tree.dispatch_pointer_down(180.0, 25.0);
    assert!(tree.dispatch_window_focus_lost());

    assert_eq!(tree.node_text(inp_id).as_deref(), Some("초기값"));
    assert!(!tree.has_active_ime_preedit());
    tree.end_pointer_drag();
    assert_eq!(clicks.get(), 0);
}
