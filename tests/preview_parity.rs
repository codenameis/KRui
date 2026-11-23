//! Stage B4 verification tests (`preview` feature):
//! 1. Macro (`krui!`) vs `krui-preview` interpreter parity on `<if>`/`<else-if>`/`<else>`,
//!    `<for each="{item in vm.items}" key="{item.id}">`/`<empty>`, state styles, tokens, and `onclick`.
//! 2. Structural path key state preservation (`(parent_path, tag, element_id, for_key, sibling_index)`)
//!    when a new `<input>` is added in a different container above an existing focused `<input>` without `id`.
//! 3. 100 consecutive live IR reloads with zero leak in `debug_stats()`, `retained_node_count()`,
//!    and `taffy_node_count()`.

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
        let increment_cmd = Command::new(move || {
            count.update(|v| *v += 1);
        })
        .with_can_execute(can_save);

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

const PARITY_KRUI_SRC: &str = r#"
<style>
    :root {
        --badge-bg: #1e293b;
    }
    .root-card {
        display: flex;
        flex-direction: column;
        width: 560px;
        height: 400px;
        padding: 16px;
        gap: 10px;
        background-color: var(--krui-surface);
    }
    .badge {
        background-color: var(--badge-bg);
        color: var(--krui-text);
        padding: 6px;
        border-radius: 6px;
    }
    button.action-btn {
        width: 180px;
        height: 40px;
        background-color: var(--krui-accent);
        color: #ffffff;
        border-radius: 6px;
    }
    button.action-btn:hover {
        background-color: #3b82f6;
    }
    button.action-btn:disabled {
        background-color: #64748b;
        opacity: 0.5;
    }
    .row {
        padding: 4px;
        color: var(--krui-text);
    }
</style>
<div class="root-card">
    <h2 id="header">카운트: {vm.count}</h2>
    <if test="{vm.is_logged_in}">
        <p id="status-msg" class="badge">환영합니다, {vm.user_name}님</p>
    <else-if test="{vm.is_loading}">
        <p id="status-msg" class="badge">로그인 중...</p>
    <else>
        <p id="status-msg" class="badge">게스트 모드</p>
    </if>
    <button id="inc-btn" class="action-btn" onclick="{vm.increment_cmd}">증가 (+1)</button>
    <ul id="item-list">
        <for each="{item in vm.items}" key="{item.id}">
            <li class="row">{item.id}: {item.title}</li>
        </for>
        <empty>
            <li class="row">항목이 없습니다</li>
        </empty>
    </ul>
</div>
"#;

fn mount_parity_macro_tree(vm_macro: &ParityViewModel) -> ViewTree {
    let mut tree_macro = ViewTree::new();
    let vm = vm_macro.clone();
    tree_macro.mount_with(move || {
        krui!(
            r#"
            <style>
                :root { --badge-bg: #1e293b; }
                .root-card { display: flex; flex-direction: column; width: 560px; height: 400px; padding: 16px; gap: 10px; background-color: var(--krui-surface); }
                .badge { background-color: var(--badge-bg); color: var(--krui-text); padding: 6px; border-radius: 6px; }
                button.action-btn { width: 180px; height: 40px; background-color: var(--krui-accent); color: #ffffff; border-radius: 6px; }
                button.action-btn:hover { background-color: #3b82f6; }
                button.action-btn:disabled { background-color: #64748b; opacity: 0.5; }
                .row { padding: 4px; color: var(--krui-text); }
            </style>
            <div class="root-card">
                <h2 id="header">카운트: {vm.count}</h2>
                <if test="{vm.is_logged_in}">
                    <p id="status-msg" class="badge">환영합니다, {vm.user_name}님</p>
                <else-if test="{vm.is_loading}">
                    <p id="status-msg" class="badge">로그인 중...</p>
                <else>
                    <p id="status-msg" class="badge">게스트 모드</p>
                </if>
                <button id="inc-btn" class="action-btn" onclick="{vm.increment_cmd}">증가 (+1)</button>
                <ul id="item-list">
                    <for each="{item in vm.items}" key="{item.id}">
                        <li class="row">{item.id}: {item.title}</li>
                    </for>
                    <empty>
                        <li class="row">항목이 없습니다</li>
                    </empty>
                </ul>
            </div>
            "#
        )
    });
    tree_macro
}

#[test]
fn b4_macro_and_preview_interpreter_produce_identical_paint_scene_and_state_transitions() {
    reset_reactive_runtime();

    let vm_macro = ParityViewModel::new();
    let vm_interp = ParityViewModel::new();
    let mut tree_macro = mount_parity_macro_tree(&vm_macro);

    let compiled_doc = compile_krui_source(PARITY_KRUI_SRC).expect("compile parity krui");
    let mut scope_reg = ScopeRegistry::new();
    scope_reg.insert_registry("vm", vm_interp.clone());
    let reg_ref: RegistryRef = Rc::new(scope_reg);

    let mut tree_interp = ViewTree::new();
    preview::mount_compiled_document_with_registry(&mut tree_interp, &compiled_doc, reg_ref);

    let (scene_macro_1, _) = tree_macro.build_paint_scene(800, 600);
    let (scene_interp_1, _) = tree_interp.build_paint_scene(800, 600);
    assert_eq!(scene_macro_1.primitives, scene_interp_1.primitives);

    let btn_macro_rect = tree_macro
        .node_layout(tree_macro.find_by_id("inc-btn").unwrap())
        .unwrap();
    let btn_interp_rect = tree_interp
        .node_layout(tree_interp.find_by_id("inc-btn").unwrap())
        .unwrap();
    assert_eq!(btn_macro_rect, btn_interp_rect);

    for _ in 0..2 {
        tree_macro.dispatch_click(btn_macro_rect.x + 10.0, btn_macro_rect.y + 10.0);
        tree_interp.dispatch_click(btn_interp_rect.x + 10.0, btn_interp_rect.y + 10.0);
    }
    assert_eq!(vm_macro.count.get(), 4);
    assert_eq!(vm_interp.count.get(), 4);

    vm_macro.is_logged_in.set(false);
    vm_macro.is_loading.set(true);
    vm_macro.items.set(Vec::new());

    vm_interp.is_logged_in.set(false);
    vm_interp.is_loading.set(true);
    vm_interp.items.set(Vec::new());

    let (scene_macro_2, _) = tree_macro.build_paint_scene(800, 600);
    let (scene_interp_2, _) = tree_interp.build_paint_scene(800, 600);
    assert_eq!(scene_macro_2.primitives, scene_interp_2.primitives);
}

#[test]
fn b4_structural_path_key_preserves_unidentified_input_state_when_new_input_is_inserted_above() {
    reset_reactive_runtime();

    let doc_v1 = compile_krui_source(
        r#"
        <div class="page">
            <div class="header-bar"><span>상단 안내 영역</span></div>
            <div class="form-panel"><input placeholder="이름 입력" /></div>
        </div>
        "#,
    )
    .expect("compile doc_v1");
    let doc_v2 = compile_krui_source(
        r#"
        <div class="page">
            <div class="header-bar">
                <input placeholder="새로 추가된 상단 검색창" />
                <span>상단 안내 영역</span>
            </div>
            <div class="form-panel"><input placeholder="이름 입력" /></div>
        </div>
        "#,
    )
    .expect("compile doc_v2");

    let reg: RegistryRef = Rc::new(ScopeRegistry::new());
    let mut tree = ViewTree::new();
    preview::mount_compiled_document_with_registry(&mut tree, &doc_v1, reg.clone());

    let focused_v1 = tree.dispatch_key_tab(false).expect("focus form input");
    assert_eq!(
        tree.node_preview_path_key(focused_v1).as_deref(),
        Some("root/0:div/1:div/0:input")
    );
    assert!(tree.dispatch_text_input("홍길동"));
    assert!(tree.dispatch_cursor_left());
    assert_eq!(tree.node_text(focused_v1).as_deref(), Some("홍길동"));
    assert_eq!(tree.node_cursor_char_idx(focused_v1), Some(2));

    let count = preview::reload_compiled_document_with_registry(&mut tree, &doc_v2, reg);
    assert_eq!(count, 6);

    let focused_v2 = tree.focused_node_id().expect("focus preserved");
    assert_eq!(
        tree.node_preview_path_key(focused_v2).as_deref(),
        Some("root/0:div/1:div/0:input")
    );
    assert_eq!(tree.node_text(focused_v2).as_deref(), Some("홍길동"));
    assert_eq!(tree.node_cursor_char_idx(focused_v2), Some(2));
}

#[test]
fn b4_100_consecutive_preview_reloads_have_zero_signal_subscriber_scope_or_node_leaks() {
    reset_reactive_runtime();

    let vm = ParityViewModel::new();
    let doc = compile_krui_source(PARITY_KRUI_SRC).expect("compile krui");
    let mut scope_reg = ScopeRegistry::new();
    scope_reg.insert_registry("vm", vm);
    let reg: RegistryRef = Rc::new(scope_reg);

    let mut tree = ViewTree::new();
    preview::mount_compiled_document_with_registry(&mut tree, &doc, reg.clone());

    // Perform reload #1 to establish baseline counts
    preview::reload_compiled_document_with_registry(&mut tree, &doc, reg.clone());
    let baseline_stats = debug_stats();
    let baseline_retained = tree.retained_node_count();
    let baseline_taffy = tree.taffy_node_count();

    // Perform 99 more reloads (total 100 reloads)
    for _ in 0..99 {
        preview::reload_compiled_document_with_registry(&mut tree, &doc, reg.clone());
    }

    let after_100_stats = debug_stats();
    assert_eq!(
        after_100_stats, baseline_stats,
        "100 preview reloads must not leak signals, subscribers, scopes, pollers, or frame callbacks"
    );
    assert_eq!(
        tree.retained_node_count(),
        baseline_retained,
        "100 preview reloads must not leak retained node slots"
    );
    assert_eq!(
        tree.taffy_node_count(),
        baseline_taffy,
        "100 preview reloads must not leak Taffy layout nodes"
    );
}

#[test]
fn b_fix_1_pending_blur_ime_target_survives_preview_reload_until_late_commit() {
    let doc = compile_krui_source(
        r#"
        <div style="display: flex; flex-direction: column; width: 400px; height: 240px; padding: 12px; gap: 8px">
            <input id="inp-a" width="240" height="36" />
            <input id="inp-b" width="240" height="36" />
        </div>
        "#,
    )
    .expect("compile two inputs");
    let reg: RegistryRef = Rc::new(ScopeRegistry::new());
    let mut tree = ViewTree::new();
    preview::mount_compiled_document_with_registry(&mut tree, &doc, reg.clone());

    let id_a = tree.find_by_id("inp-a").expect("inp-a");
    let id_b = tree.find_by_id("inp-b").expect("inp-b");

    // 1. Focus `#inp-a`, type `"시작"`, start IME preedit `"にほんご"`, then move focus to `#inp-b`
    //    (auto-committing `"にほんご"` on `#inp-a` with `pending_blur_commit` awaiting late platform commit).
    assert!(tree.set_focus_to_node(id_a));
    assert!(tree.dispatch_text_input("시작"));
    assert!(tree.ime_enabled());
    assert!(tree.ime_preedit("にほんご", Some((0, 12))));
    assert!(tree.set_focus_to_node(id_b));
    assert_eq!(tree.node_text(id_a).as_deref(), Some("시작にほんご"));

    // 2. Trigger a live preview IR reload while `#inp-b` is focused and `#inp-a` is still `ime_target`!
    preview::reload_compiled_document_with_registry(&mut tree, &doc, reg);
    let id_a_after = tree.find_by_id("inp-a").expect("inp-a after reload");
    let id_b_after = tree.find_by_id("inp-b").expect("inp-b after reload");
    assert_eq!(tree.focused_node_id(), Some(id_b_after));

    // 3. Platform IME now sends `Preedit("")` + converted `Commit("日本語")` ->
    //    `#inp-a` must replace `"にほんご"` with `"日本語"`, and `#inp-b` must remain empty!
    assert!(tree.ime_preedit("", None));
    assert!(tree.ime_commit("日本語"));
    assert_eq!(
        tree.node_text(id_a_after).as_deref(),
        Some("시작日本語"),
        "Pending blur ime_target must survive preview reload and replace auto-committed preedit with converted commit"
    );
    assert_eq!(
        tree.node_text(id_b_after).as_deref(),
        Some(""),
        "Focused #inp-b must not receive late commit meant for #inp-a after preview reload"
    );
}
