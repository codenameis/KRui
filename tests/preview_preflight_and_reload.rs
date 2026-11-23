//! Extended `B-fix-2` verification tests (`preview` feature):
//! - Evaluation parity on signal arithmetic, checked `i64::MIN` negation/division, division by zero, and strict types
//! - Atomic preflight rollback across all binding kinds and inactive `<else>` / `<for>` branches
//! - `krui!` macro -> `krui_preview` first reload, `<for>` key reorder, and strict structural path restoration

#![cfg(feature = "preview")]
#![forbid(unsafe_code)]

use krui::preview::compile_krui_source;
use krui::*;
use std::rc::Rc;

#[view_model]
#[derive(Clone, PartialEq)]
struct ParityItem {
    id: i32,
    title: String,
}

impl TryFromRegistryValue for ParityItem {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        let Value::Record(rec) = value else {
            return None;
        };
        Some(Self {
            id: rec.get("id")?.as_i64()? as i32,
            title: rec.get("title")?.as_str()?.to_string(),
        })
    }
}

#[view_model]
#[derive(Clone)]
struct ParityViewModel {
    #[state]
    count: i32,
    #[state]
    is_logged_in: bool,
    #[state]
    is_loading: bool,
    #[state]
    user_name: String,
    #[state]
    items: Vec<ParityItem>,
    #[computed]
    can_save: bool,
    #[command]
    increment_cmd: Command,
}

impl ParityViewModel {
    fn new() -> Self {
        let count = Signal::new(2_i32);
        let is_logged_in = Signal::new(true);
        let is_loading = Signal::new(false);
        let user_name = Signal::new("김보현".to_string());
        let items = Signal::new(vec![
            ParityItem {
                id: 10,
                title: "첫 번째 항목".to_string(),
            },
            ParityItem {
                id: 20,
                title: "두 번째 항목".to_string(),
            },
        ]);
        let can_save = Computed::new(move || count.get() < 4);
        let increment_cmd =
            Command::new(move || count.update(|v| *v += 1)).with_can_execute(can_save);
        Self {
            count,
            is_logged_in,
            is_loading,
            user_name,
            items,
            can_save,
            increment_cmd,
        }
    }
}

#[controller]
#[derive(Clone)]
struct BFix2Controller {
    buy_calls: Signal<i32>,
}

#[controller]
impl BFix2Controller {
    fn buy(&self, qty: i32) {
        self.buy_calls.update(|v| *v += qty);
    }
}

const PARITY_KRUI_SRC: &str = r#"
<style>
    :root { --badge-bg: #1e293b; }
    .root-card { display: flex; flex-direction: column; width: 560px; height: 400px; padding: 16px; gap: 10px; background-color: var(--krui-surface); }
    .badge { background-color: var(--badge-bg); color: var(--krui-text); padding: 6px; border-radius: 6px; }
    button.action-btn { width: 180px; height: 40px; background-color: var(--krui-accent); color: #ffffff; border-radius: 6px; }
</style>
<div class="root-card">
    <h2 id="header">카운트: {vm.count}</h2>
    <if test="{vm.is_logged_in}">
        <p id="status-msg" class="badge">환영합니다, {vm.user_name}님</p>
    <else>
        <p id="status-msg" class="badge">게스트 모드</p>
    </if>
    <button id="inc-btn" class="action-btn" onclick="{vm.increment_cmd}">증가 (+1)</button>
</div>
"#;

#[test]
fn b_fix_2_eval_parity_signal_arithmetic_checked_overflow_div_zero_and_strict_types() {
    reset_reactive_runtime();

    let vm_macro = ParityViewModel::new();
    let vm_interp = ParityViewModel::new();
    let ctrl_macro = BFix2Controller {
        buy_calls: Signal::new(0),
    };
    let ctrl_interp = BFix2Controller {
        buy_calls: Signal::new(0),
    };

    let mut tree_macro = ViewTree::new();
    {
        let vm = vm_macro.clone();
        let ctrl = ctrl_macro.clone();
        tree_macro.mount_with(move || {
            krui!(
                r#"
                <div style="display: flex; flex-direction: column; width: 400px; height: 220px">
                    <span id="arith">{vm.count + 3} / {vm.count * 2}</span>
                    <if test="{vm.is_logged_in && !vm.is_loading}">
                        <button id="buy-btn" disabled="{vm.count >= 10 || vm.is_loading}" onclick="{ctrl.buy(5)}">구매</button>
                    </if>
                </div>
                "#
            )
        });
    }

    let doc = compile_krui_source(
        r#"
        <div style="display: flex; flex-direction: column; width: 400px; height: 220px">
            <span id="arith">{vm.count + 3} / {vm.count * 2}</span>
            <if test="{vm.is_logged_in && !vm.is_loading}">
                <button id="buy-btn" disabled="{vm.count >= 10 || vm.is_loading}" onclick="{ctrl.buy(5)}">구매</button>
            </if>
        </div>
        "#,
    )
    .expect("compile signal arithmetic krui");
    let mut scope_reg = ScopeRegistry::new();
    scope_reg.insert_registry("vm", vm_interp.clone());
    scope_reg.insert_registry("ctrl", ctrl_interp.clone());
    let reg: RegistryRef = Rc::new(scope_reg);

    let mut tree_interp = ViewTree::new();
    preview::mount_compiled_document_with_registry(&mut tree_interp, &doc, reg);

    assert_signal_arithmetic_and_controller_parity(
        &mut tree_macro,
        &mut tree_interp,
        &vm_macro,
        &vm_interp,
        &ctrl_macro,
        &ctrl_interp,
    );
    assert_checked_overflow_div_zero_and_strict_types();
}

fn assert_signal_arithmetic_and_controller_parity(
    tree_macro: &mut ViewTree,
    tree_interp: &mut ViewTree,
    vm_macro: &ParityViewModel,
    vm_interp: &ParityViewModel,
    ctrl_macro: &BFix2Controller,
    ctrl_interp: &BFix2Controller,
) {
    let arith_m = tree_macro.find_by_id("arith").unwrap();
    let arith_i = tree_interp.find_by_id("arith").unwrap();
    assert_eq!(tree_macro.node_text(arith_m).as_deref(), Some("5 / 4"));
    assert_eq!(tree_interp.node_text(arith_i).as_deref(), Some("5 / 4"));

    vm_macro.count.set(7);
    vm_interp.count.set(7);
    assert_eq!(tree_macro.node_text(arith_m).as_deref(), Some("10 / 14"));
    assert_eq!(tree_interp.node_text(arith_i).as_deref(), Some("10 / 14"));

    let _ = tree_macro.build_paint_scene(400, 220);
    let _ = tree_interp.build_paint_scene(400, 220);
    let rect_m = tree_macro
        .node_layout(tree_macro.find_by_id("buy-btn").unwrap())
        .unwrap();
    let rect_i = tree_interp
        .node_layout(tree_interp.find_by_id("buy-btn").unwrap())
        .unwrap();
    tree_macro.dispatch_click(rect_m.x + 4.0, rect_m.y + 4.0);
    tree_interp.dispatch_click(rect_i.x + 4.0, rect_i.y + 4.0);
    assert_eq!(ctrl_macro.buy_calls.get(), 5);
    assert_eq!(ctrl_interp.buy_calls.get(), 5);
}

fn assert_checked_overflow_div_zero_and_strict_types() {
    let mut min_reg = ScopeRegistry::new();
    min_reg.insert_value("min_i64", Value::Int(i64::MIN));
    min_reg.insert_value("max_i64", Value::Int(i64::MAX));
    min_reg.insert_value("num", Value::Int(42));

    for bad_expr in [
        "<span>{-min_i64}</span>",
        "<span>{min_i64 - 1}</span>",
        "<span>{max_i64 + 1}</span>",
        r#"<if test="{num}"><span>bad</span></if>"#,
        "<span>{!num}</span>",
    ] {
        let doc = compile_krui_source(bad_expr).unwrap();
        assert!(
            preview::validate_compiled_document_against_registry(&doc, &min_reg).is_err(),
            "Expected validation error on {bad_expr}"
        );
    }

    for safe_zero_expr in ["<span>{num / 0}</span>", "<span>{num % 0}</span>"] {
        let doc = compile_krui_source(safe_zero_expr).unwrap();
        assert!(
            preview::validate_compiled_document_against_registry(&doc, &min_reg).is_ok(),
            "Safe zero div/rem should succeed on {safe_zero_expr}"
        );
    }
}

#[test]
fn b_fix_2_atomic_preflight_rollback_preserves_tree_and_reactivity_on_any_invalid_slot_or_branch() {
    reset_reactive_runtime();

    let vm = ParityViewModel::new();
    let ctrl = BFix2Controller {
        buy_calls: Signal::new(0),
    };
    let mut scope_reg = ScopeRegistry::new();
    scope_reg.insert_registry("vm", vm.clone());
    scope_reg.insert_registry("ctrl", ctrl.clone());
    let reg: RegistryRef = Rc::new(scope_reg);

    let valid_doc = compile_krui_source(PARITY_KRUI_SRC).expect("valid doc");
    let mut tree = ViewTree::new();
    preview::mount_compiled_document_with_registry(&mut tree, &valid_doc, reg.clone());

    let header_id = tree.find_by_id("header").expect("header before bad reload");
    assert_eq!(tree.node_text(header_id).as_deref(), Some("카운트: 2"));

    let bad_sources = [
        r#"<div><span>{vm.non_existent_field}</span></div>"#,
        r#"<div><if test="{vm.is_logged_in}"><span>OK</span><else><span>{vm.missing_in_inactive_else}</span></if></div>"#,
        r#"<div><span style="opacity: {vm.user_name}">bad paint</span></div>"#,
        r#"<div><span width="{vm.user_name}">bad layout</span></div>"#,
        r#"<div><button disabled="{vm.count}">bad disabled</button></div>"#,
        r#"<div><button onclick="{ctrl.unregistered_method(1)}">bad click</button></div>"#,
        r#"<div><input bind:value="{vm.can_save}" /></div>"#,
        r#"<div><input oninput="{vm.can_save}" /></div>"#,
        r#"<div><input onchange="{vm.missing_handler}" /></div>"#,
        r#"<div><input onkeydown="{vm.missing_keydown}" /></div>"#,
        r#"<div><canvas ondraw="{vm.user_name}" /></div>"#,
        r#"<ul><for each="{item in vm.items}" key="{item.id}"><li>{vm.missing_in_for}</li></for></ul>"#,
    ];

    for src in bad_sources {
        let bad_doc = compile_krui_source(src).expect("parse bad_doc");
        assert!(preview::try_reload_compiled_document_with_registry(
            &mut tree,
            &bad_doc,
            reg.clone()
        )
        .is_err());
    }

    assert_eq!(ctrl.buy_calls.get(), 0);
    let header_after = tree.find_by_id("header").expect("header still alive");
    assert_eq!(header_after, header_id);
    vm.count.set(9);
    assert_eq!(tree.node_text(header_after).as_deref(), Some("카운트: 9"));

    verify_tcp_client_preflight_failure_response(&mut tree);
}

fn verify_tcp_client_preflight_failure_response(tree: &mut ViewTree) {
    let mut server = preview::PreviewTcpServer::bind("127.0.0.1:0").expect("bind test tcp server");
    let addr = server.local_addr().to_string();
    let mut client = preview::PreviewTcpClient::connect(&addr).expect("connect tcp client");
    std::thread::sleep(std::time::Duration::from_millis(15));
    server.accept_pending();

    let bad_ir = preview::encode_compiled_document(
        &compile_krui_source(r#"<div><span>{vm.does_not_exist}</span></div>"#).unwrap(),
    )
    .expect("encode bad_ir");
    server.broadcast(&preview::PreviewCommand::Reload(bad_ir));
    std::thread::sleep(std::time::Duration::from_millis(15));

    let mut compile_err = None;
    let mut reload_cnt = 0;
    assert!(client.poll(tree, &mut compile_err, &mut reload_cnt));
    assert!(compile_err.is_some());
    assert_eq!(reload_cnt, 0);

    std::thread::sleep(std::time::Duration::from_millis(15));
    let responses = server.poll_responses();
    assert_eq!(responses.len(), 1);
    assert!(matches!(&responses[0], preview::PreviewResponse::Error(_)));
}

#[test]
fn b_fix_2_macro_to_preview_first_reload_key_reorder_and_strict_structural_path_restoration() {
    reset_reactive_runtime();

    let vm = ParityViewModel::new();
    let mut tree = mount_macro_for_list_tree(&vm);

    let captured_reg = tree
        .preview_registry()
        .expect("krui! macro must populate tree.preview_registry() under preview feature");

    let inspected_macro = tree.inspect_nodes();
    let row_inputs_macro: Vec<&InspectedNodeInfo> = inspected_macro
        .iter()
        .filter(|n| n.tag == "input" && n.path_key.contains("for["))
        .collect();
    assert_eq!(row_inputs_macro.len(), 2);
    assert_eq!(
        row_inputs_macro[0].path_key,
        "root/0:div/1:ul#list/0:#for/for[10]/0:li/0:input"
    );
    assert_eq!(
        row_inputs_macro[1].path_key,
        "root/0:div/1:ul#list/0:#for/for[20]/0:li/0:input"
    );

    let shared_in = tree.find_by_id("shared-input").unwrap();
    let box_a = tree.find_by_id("box-a").unwrap();
    tree.set_node_scroll_y(box_a, 48.0);
    assert!(tree.set_focus_to_node(shared_in));
    assert!(tree.dispatch_text_input("박스A의 텍스트"));

    let input_10 = row_inputs_macro[0].id;
    let input_20 = row_inputs_macro[1].id;
    assert!(tree.set_focus_to_node(input_10));
    assert!(tree.dispatch_text_input("항목10 메모"));
    assert!(tree.set_focus_to_node(input_20));
    assert!(tree.dispatch_text_input("항목20 "));
    assert!(tree.ime_enabled());
    assert!(tree.ime_preedit("조합중", Some((0, 9))));

    vm.items.set(vec![
        ParityItem {
            id: 20,
            title: "두 번째 항목".to_string(),
        },
        ParityItem {
            id: 10,
            title: "첫 번째 항목".to_string(),
        },
    ]);

    verify_macro_to_preview_reloaded_state(&mut tree, captured_reg);
}

fn mount_macro_for_list_tree(vm: &ParityViewModel) -> ViewTree {
    let mut tree = ViewTree::new();
    let vm_c = vm.clone();
    tree.mount_with(move || {
        let vm = vm_c.clone();
        krui!(
            r#"
            <div class="root-card">
                <div id="box-a">
                    <input id="shared-input" />
                </div>
                <ul id="list">
                    <for each="{item in vm.items}" key="{item.id}">
                        <li class="row">
                            <input />
                            <span>{item.title}</span>
                        </li>
                    </for>
                </ul>
            </div>
            "#
        )
    });
    tree
}

fn verify_macro_to_preview_reloaded_state(tree: &mut ViewTree, captured_reg: RegistryRef) {
    let reloaded_doc = compile_krui_source(
        r#"
        <div class="root-card">
            <div id="box-a">
                <span>이전 입력칸이 있던 자리</span>
            </div>
            <ul id="list">
                <for each="{item in vm.items}" key="{item.id}">
                    <li class="row">
                        <input />
                        <span>{item.title}</span>
                    </li>
                </for>
            </ul>
            <div id="box-b">
                <input id="shared-input" />
            </div>
        </div>
        "#,
    )
    .expect("compile reloaded_doc");

    let count =
        preview::try_reload_compiled_document_with_registry(tree, &reloaded_doc, captured_reg)
            .expect("macro -> preview first reload must succeed");
    assert!(count > 0);

    let box_a_after = tree.find_by_id("box-a").unwrap();
    assert_eq!(tree.node_scroll_y(box_a_after), Some(48.0));

    let moved_shared_in = tree.find_by_id("shared-input").unwrap();
    assert_eq!(tree.node_text(moved_shared_in).as_deref(), Some(""));

    let inspected_after = tree.inspect_nodes();
    let row_inputs_after: Vec<&InspectedNodeInfo> = inspected_after
        .iter()
        .filter(|n| n.tag == "input" && n.path_key.contains("for["))
        .collect();
    assert_eq!(row_inputs_after.len(), 2);
    assert_eq!(
        row_inputs_after[0].path_key,
        "root/0:div/1:ul#list/0:#for/for[20]/0:li/0:input"
    );
    assert_eq!(
        row_inputs_after[1].path_key,
        "root/0:div/1:ul#list/0:#for/for[10]/0:li/0:input"
    );
    assert_eq!(
        tree.node_text(row_inputs_after[0].id).as_deref(),
        Some("항목20 ")
    );
    assert_eq!(
        tree.node_text(row_inputs_after[1].id).as_deref(),
        Some("항목10 메모")
    );
    assert_eq!(tree.focused_node_id(), Some(row_inputs_after[0].id));
    let ime_st = tree.focused_ime_state().expect("ime state on key 20");
    assert_eq!(ime_st.preedit, "조합중");
}
