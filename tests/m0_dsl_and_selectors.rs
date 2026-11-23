//! Integration tests for M0 `P0-9`:
//! - `<if>` / `<else-if>` / `<else>` reactive control flow without layout wrapper boxes
//! - `<for each="{item in items}" key="{item.id}">` + `<empty>` keyed reconciliation & focus preservation
//! - Compound CSS selectors (`button.primary`, `.a.b`, `#id.cls`) and `(id, class, tag)` specificity
//! - Compile-time `:hover`, `:focus`, `:active`, `:disabled` state style tables
//! - `:root` CSS variables (`var(--*)`), extended CSS colors (`hsl`, `rgba`, `#RGB`, named)
//! - Separate `placeholder` vs `value`, `onchange`, `onkeydown`, boolean `disabled="{!can_save}"`
//! - Single-pass HTML entity decoding and `{{` / `}}` brace escaping

#![forbid(unsafe_code)]

use krui::{krui, Color, DrawPrimitive, InputCommand, Signal, ViewTree};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
struct TodoItem {
    id: u32,
    title: String,
}

#[test]
fn if_else_if_else_switches_branches_reactively_without_wrapper_boxes() {
    let is_logged_in = Signal::new(false);
    let is_loading = Signal::new(false);
    let user_name = Signal::new("보현".to_string());

    let fragment = krui!(
        r#"
        <div id="container" style="width: 400px; height: 200px; gap: 8px">
            <span>헤더</span>
            <if test="{is_logged_in}">
                <p id="welcome">환영합니다, {user_name}님</p>
            <else-if test="{is_loading}">
                <p id="loading">로그인 중...</p>
            <else>
                <button id="login-btn">로그인</button>
            </if>
            <span>푸터</span>
        </div>
    "#
    );

    let mut tree = ViewTree::new();
    tree.mount(fragment);

    // 1. Initial state: `<else>` branch (`#login-btn`) is mounted directly inside `#container`
    let _ = tree.build_paint_scene(400, 200);
    assert!(tree.find_by_id("login-btn").is_some());
    assert!(tree.find_by_id("welcome").is_none());
    assert!(tree.find_by_id("loading").is_none());
    // `#container` + `헤더` + `#login-btn` + `푸터` = 4 element nodes (no wrapper box!)
    assert_eq!(tree.node_count(), 4);

    // 2. Switch to `<else-if>` branch
    is_loading.set(true);
    let _ = tree.build_paint_scene(400, 200);
    assert!(tree.find_by_id("loading").is_some());
    assert!(tree.find_by_id("login-btn").is_none());
    assert_eq!(tree.node_count(), 4);

    // 3. Switch to `<if>` branch
    is_logged_in.set(true);
    let _ = tree.build_paint_scene(400, 200);
    assert!(tree.find_by_id("welcome").is_some());
    assert!(tree.find_by_id("loading").is_none());
    assert_eq!(tree.node_count(), 4);
}

fn todo_item(id: u32, title: &str) -> TodoItem {
    TodoItem {
        id,
        title: title.to_string(),
    }
}

#[test]
fn for_loop_with_empty_reconciles_by_key_and_preserves_input_focus() {
    let items: Signal<Vec<TodoItem>> = Signal::new(Vec::new());
    let fragment = krui!(
        r#"
        <ul id="list" style="width: 400px; height: 300px">
            <for each="{item in items}" key="{item.id}">
                <li class="row">
                    <span>{item.title}</span>
                    <input value="{item.title}" placeholder="할 일 입력" />
                </li>
            </for>
            <empty>
                <li id="empty-placeholder">항목이 없습니다</li>
            </empty>
        </ul>
    "#
    );

    let mut tree = ViewTree::new();
    tree.mount(fragment);
    let _ = tree.build_paint_scene(400, 300);
    assert!(tree.find_by_id("empty-placeholder").is_some());

    items.set(vec![todo_item(10, "첫 번째"), todo_item(20, "두 번째")]);
    let _ = tree.build_paint_scene(400, 300);
    assert!(tree.find_by_id("empty-placeholder").is_none());

    let first_input = tree.dispatch_key_tab(false).expect("first input");
    let second_input = tree.dispatch_key_tab(false).expect("second input");
    assert_ne!(first_input, second_input);
    assert_eq!(tree.focused_node_id(), Some(second_input));

    items.set(vec![
        todo_item(5, "새 항목"),
        todo_item(20, "두 번째"),
        todo_item(10, "첫 번째"),
    ]);
    let _ = tree.build_paint_scene(400, 300);
    assert_eq!(tree.focused_node_id(), Some(second_input));

    items.set(Vec::new());
    let _ = tree.build_paint_scene(400, 300);
    assert!(tree.find_by_id("empty-placeholder").is_some());
}

#[test]
fn compound_selectors_css_variables_and_state_pseudo_classes_work_end_to_end() {
    let can_save = Signal::new(false);

    let fragment = krui!(
        r#"
        <style>
            :root {
                --brand-bg: rgb(30, 64, 175);
                --brand-hover: hsl(217, 91%, 60%);
                --brand-active: #1d4ed8;
                --brand-disabled: rgba(100, 116, 139, 0.5);
            }
            button {
                background-color: #334155;
                padding: 8px 16px 10px;
            }
            button.primary {
                background-color: var(--brand-bg);
                border: 2px solid var(--undefined-token, #38bdf8);
            }
            button.primary:hover {
                background-color: var(--brand-hover);
            }
            button.primary:active {
                background-color: var(--brand-active);
            }
            button.primary:disabled {
                background-color: var(--brand-disabled);
                opacity: 0.4;
            }
        </style>
        <div style="width: 300px; height: 120px">
            <button id="save-btn" class="primary" style="width: 160px; height: 44px" disabled="{!can_save}">
                저장 ({{Ctrl+S}}) &amp;lt;OK&amp;gt;
            </button>
        </div>
    "#
    );

    let mut tree = ViewTree::new();
    tree.mount(fragment);
    assert!(tree.find_by_id("save-btn").is_some());
    assert_primary_button_states(&mut tree, can_save);
}

fn assert_primary_button_states(tree: &mut ViewTree, can_save: Signal<bool>) {
    let (scene_disabled, _) = tree.build_paint_scene(300, 120);
    let mut layer_stack = vec![1.0_f32];
    let mut btn_box_disabled = None;
    for p in &scene_disabled.primitives {
        match p {
            DrawPrimitive::PushLayer { opacity, .. } => {
                let cur = layer_stack.last().copied().unwrap_or(1.0);
                layer_stack.push(cur * *opacity);
            }
            DrawPrimitive::PopLayer => {
                let _ = layer_stack.pop();
            }
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                border_width,
                opacity,
                ..
            } if (rect.width - 160.0).abs() < 1.0 => {
                let layer_op = layer_stack.last().copied().unwrap_or(1.0);
                btn_box_disabled = Some((*background_color, *border_width, layer_op * *opacity));
            }
            _ => {}
        }
    }
    let btn_box_disabled = btn_box_disabled.expect("button box");
    assert_eq!(btn_box_disabled.0, Color::rgba(100, 116, 139, 128));
    assert_eq!(btn_box_disabled.1, 2.0);
    assert!((btn_box_disabled.2 - 0.4).abs() < 1e-4);

    can_save.set(true);
    let (scene_enabled, _) = tree.build_paint_scene(300, 120);
    let bg_enabled = find_button_bg(&scene_enabled, 160.0);
    assert_eq!(bg_enabled, Color::rgb(30, 64, 175));

    tree.dispatch_pointer_move(40.0, 20.0);
    let (scene_hover, _) = tree.build_paint_scene(300, 120);
    let bg_hover = find_button_bg(&scene_hover, 160.0);
    assert_ne!(bg_hover, bg_enabled);

    tree.dispatch_pointer_down(40.0, 20.0);
    let (scene_active, _) = tree.build_paint_scene(300, 120);
    assert_eq!(
        find_button_bg(&scene_active, 160.0),
        Color::rgb(0x1d, 0x4e, 0xd8)
    );
    tree.dispatch_pointer_up();
}

fn find_button_bg(scene: &krui::PaintScene, width: f32) -> Color {
    scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if (rect.width - width).abs() < 1.0 => Some(*background_color),
            _ => None,
        })
        .unwrap()
}

#[test]
fn input_placeholder_onchange_and_onkeydown_work_distinctly_from_value_and_oninput() {
    let live_log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let committed_log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let keydown_log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    let live_clone = live_log.clone();
    let on_input = InputCommand::new(move |val: String| live_clone.borrow_mut().push(val));
    let commit_clone = committed_log.clone();
    let on_change = InputCommand::new(move |val: String| commit_clone.borrow_mut().push(val));
    let kd_clone = keydown_log.clone();
    let on_keydown = InputCommand::new(move |key: String| kd_clone.borrow_mut().push(key));

    let fragment = krui!(
        r#"
        <div style="width: 320px; height: 120px">
            <input
                id="search-box"
                style="width: 240px; height: 36px"
                placeholder="검색어를 입력하세요"
                oninput="{on_input}"
                onchange="{on_change}"
                onkeydown="{on_keydown}" />
        </div>
    "#
    );

    let mut tree = ViewTree::new();
    tree.mount(fragment);
    let _ = tree.build_paint_scene(320, 120);

    tree.dispatch_click(40.0, 18.0);
    tree.dispatch_key_down("K");
    tree.dispatch_key_text("K");
    tree.dispatch_key_down("R");
    tree.dispatch_key_text("R");

    assert_eq!(*live_log.borrow(), vec!["K".to_string(), "KR".to_string()]);
    assert!(committed_log.borrow().is_empty());
    assert_eq!(
        *keydown_log.borrow(),
        vec!["K".to_string(), "R".to_string()]
    );

    assert!(tree.dispatch_key_enter());
    assert_eq!(*committed_log.borrow(), vec!["KR".to_string()]);

    tree.dispatch_key_text("ui");
    tree.clear_focus();
    assert_eq!(
        *committed_log.borrow(),
        vec!["KR".to_string(), "KRui".to_string()]
    );
}

#[test]
fn two_page_navigation_preserves_state_and_toggles_message_box_modal() {
    use krui::{Command, Computed};

    let (is_page_1, is_msgbox_open) = (Signal::new(true), Signal::new(false));
    let (page1_title, page1_qty) = (Signal::new("초기 상품명".to_string()), Signal::new(2_i32));
    let page2_memo = Signal::new("초기 배송 메모".to_string());
    let go_page_1 = Command::new(move || is_page_1.set(true));
    let go_page_2 = Command::new(move || is_page_1.set(false));
    let inc_qty = Command::new(move || page1_qty.update(|q| *q += 1));
    let open_msgbox = Command::new(move || is_msgbox_open.set(true));
    let close_msgbox = Command::new(move || is_msgbox_open.set(false));
    let msgbox_summary = Computed::new(move || {
        let (t, q, m) = (page1_title.get(), page1_qty.get(), page2_memo.get());
        format!("{t} ({q}개) / {m}")
    });

    let fragment = krui!(
        r#"
        <div style="width: 600px; height: 400px; gap: 10px">
            <if test="{is_page_1}">
                <section id="page-1">
                    <input id="p1-input" style="width: 240px; height: 36px" bind:value="{page1_title}" />
                    <button id="p1-inc" style="width: 100px; height: 36px" onclick="{inc_qty}">+1</button>
                    <button id="to-p2" style="width: 120px; height: 36px" onclick="{go_page_2}">2페이지로</button>
                    <button id="open-msgbox" style="width: 140px; height: 36px" onclick="{open_msgbox}">메시지 박스 열기</button>
                </section>
            <else>
                <section id="page-2">
                    <input id="p2-input" style="width: 240px; height: 36px" bind:value="{page2_memo}" />
                    <span id="p2-preserved">{page1_title} ({page1_qty}개)</span>
                    <button id="to-p1" style="width: 120px; height: 36px" onclick="{go_page_1}">1페이지로</button>
                </section>
            </if>
            <if test="{is_msgbox_open}">
                <div id="msgbox-modal">
                    <span id="msgbox-content">{msgbox_summary}</span>
                    <button id="close-msgbox" style="width: 120px; height: 36px" onclick="{close_msgbox}">닫기</button>
                </div>
            </if>
        </div>
    "#
    );

    let mut tree = ViewTree::new();
    tree.set_scale_factor(1.5);
    tree.mount(fragment);
    let (scene, _) = tree.build_paint_scene(600, 400);
    assert!((scene.scale_factor - 1.5).abs() < 1e-4);
    verify_two_page_navigation_and_modal(
        &mut tree,
        page1_title,
        page1_qty,
        page2_memo,
        msgbox_summary,
    );
}

fn verify_two_page_navigation_and_modal(
    tree: &mut ViewTree,
    page1_title: Signal<String>,
    page1_qty: Signal<i32>,
    page2_memo: Signal<String>,
    msgbox_summary: krui::Computed<String>,
) {
    assert!(tree.find_by_id("page-1").is_some());
    assert!(tree.find_by_id("page-2").is_none());
    let _ = tree.dispatch_key_tab(false);
    assert!(tree.dispatch_text_input("-수정됨"));
    let _ = tree.dispatch_key_tab(false);
    assert!(tree.dispatch_key_activate());
    assert_eq!(page1_title.get(), "초기 상품명-수정됨");
    assert_eq!(page1_qty.get(), 3);

    let _ = tree.dispatch_key_tab(false);
    assert!(tree.dispatch_key_activate());
    let _ = tree.build_paint_scene(600, 400);
    assert!(tree.find_by_id("page-1").is_none());
    assert!(tree.find_by_id("page-2").is_some());

    let _ = tree.dispatch_key_tab(false);
    assert!(tree.dispatch_text_input("-당일출고"));
    assert_eq!(page2_memo.get(), "초기 배송 메모-당일출고");

    let _ = tree.dispatch_key_tab(false);
    assert!(tree.dispatch_key_activate());
    let _ = tree.build_paint_scene(600, 400);
    assert!(tree.find_by_id("page-1").is_some());
    assert_eq!(page1_title.get(), "초기 상품명-수정됨");
    assert_eq!(page1_qty.get(), 3);
    assert_eq!(page2_memo.get(), "초기 배송 메모-당일출고");

    assert!(tree.find_by_id("msgbox-modal").is_none());
    for _ in 0..4 {
        let _ = tree.dispatch_key_tab(false);
    }
    assert!(tree.dispatch_key_activate());
    let _ = tree.build_paint_scene(600, 400);
    assert!(tree.find_by_id("msgbox-modal").is_some());
    assert_eq!(
        msgbox_summary.get(),
        "초기 상품명-수정됨 (3개) / 초기 배송 메모-당일출고"
    );
    let _ = tree.dispatch_key_tab(false);
    assert!(tree.dispatch_key_activate());
    let _ = tree.build_paint_scene(600, 400);
    assert!(tree.find_by_id("msgbox-modal").is_none());
}
