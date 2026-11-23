//! End-to-end Phase 2 integration tests verifying `krui!` macro codegen and `krui::preview` parity:
//! 1. `{a < b}` & `{count < 10 && ratio > 0.5}` expression lexing + `title="Next >"`
//! 2. Non-destructive pseudo-state cascade (`button:hover` preserved across `button.primary`)
//! 3. Parent -> child CSS inheritance & element-scoped `--var` resolution
//! 4. Inline horizontal flow, `<Card>` component composition, nested `<for>`, `<select><for>`,
//!    linear dynamic class patching, and safe `/` & `%` by zero parity.

#![forbid(unsafe_code)]

use krui::*;

#[view_model]
#[derive(Clone, PartialEq)]
struct LeafTask {
    id: i32,
    label: String,
}

#[cfg(feature = "preview")]
impl TryFromRegistryValue for LeafTask {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        let Value::Record(rec) = value else {
            return None;
        };
        Some(Self {
            id: rec.get("id")?.as_i64()? as i32,
            label: rec.get("label")?.as_str()?.to_string(),
        })
    }
}

#[view_model]
#[derive(Clone, PartialEq)]
struct TaskGroup {
    id: i32,
    name: String,
    items: Vec<LeafTask>,
}

#[cfg(feature = "preview")]
impl TryFromRegistryValue for TaskGroup {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        let Value::Record(rec) = value else {
            return None;
        };
        let Value::List(raw_items) = rec.get("items")? else {
            return None;
        };
        let mut items = Vec::with_capacity(raw_items.len());
        for v in &raw_items {
            items.push(LeafTask::try_from_registry_value(v)?);
        }
        Some(Self {
            id: rec.get("id")?.as_i64()? as i32,
            name: rec.get("name")?.as_str()?.to_string(),
            items,
        })
    }
}

#[view_model]
#[derive(Clone, PartialEq)]
struct SelectOptItem {
    value: String,
    label: String,
}

#[cfg(feature = "preview")]
impl TryFromRegistryValue for SelectOptItem {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        let Value::Record(rec) = value else {
            return None;
        };
        Some(Self {
            value: rec.get("value")?.as_str()?.to_string(),
            label: rec.get("label")?.as_str()?.to_string(),
        })
    }
}

#[view_model]
#[derive(Clone)]
struct Phase2Vm {
    #[state]
    a: i32,
    #[state]
    b: i32,
    #[state]
    denom: i32,
    #[state]
    fdenom: f64,
    #[state]
    ratio: f64,
    #[state]
    name: String,
    #[state]
    selected_role: String,
    #[state]
    groups: Vec<TaskGroup>,
    #[state]
    options: Vec<SelectOptItem>,
}

impl Phase2Vm {
    fn sample() -> Self {
        Self {
            a: Signal::new(3),
            b: Signal::new(8),
            denom: Signal::new(0),
            fdenom: Signal::new(0.0),
            ratio: Signal::new(0.75),
            name: Signal::new("보현".to_string()),
            selected_role: Signal::new("editor".to_string()),
            groups: Signal::new(vec![
                TaskGroup {
                    id: 1,
                    name: "백엔드".to_string(),
                    items: vec![
                        LeafTask {
                            id: 101,
                            label: "파서".to_string(),
                        },
                        LeafTask {
                            id: 102,
                            label: "캐스케이드".to_string(),
                        },
                    ],
                },
                TaskGroup {
                    id: 2,
                    name: "런타임".to_string(),
                    items: vec![LeafTask {
                        id: 201,
                        label: "시그널".to_string(),
                    }],
                },
            ]),
            options: Signal::new(vec![
                SelectOptItem {
                    value: "viewer".to_string(),
                    label: "열람자".to_string(),
                },
                SelectOptItem {
                    value: "editor".to_string(),
                    label: "편집자".to_string(),
                },
            ]),
        }
    }
}

#[cfg(feature = "preview")]
const PARITY_PHASE2_SRC: &str = r#"
<template name="Card">
    <style>
        .card-box { padding: 12px; border-radius: 10px; }
    </style>
    <section class="card-box">
        <h3 id="card-title">{title}</h3>
        <slot/>
    </section>
</template>
<div style="width: 640px; height: 480px">
    <p id="cmp-p">{vm.a < vm.b}</p>
    <if test="{vm.a < 10 && vm.ratio > 0.5}">
        <button id="next-btn" title="Next >">진행</button>
    </if>
    <p id="safe-math">{vm.b / vm.denom} | {vm.b % vm.denom} | {vm.ratio / vm.fdenom}</p>
    <p id="inline-p">안녕 <span id="hl-span">{vm.name}</span>님</p>
    <Card title="팀: {vm.name}">
        <ul id="nested-list">
            <for each="{grp in vm.groups}" key="{grp.id}">
                <for each="{item in grp.items}" key="{item.id}">
                    <li>{grp.name}:{item.label}</li>
                </for>
            </for>
        </ul>
    </Card>
    <select id="role-select" bind:value="{vm.selected_role}">
        <for each="{opt in vm.options}" key="{opt.value}">
            <option value="{opt.value}">{opt.label}</option>
        </for>
    </select>
</div>
"#;

fn mount_phase2_macro_tree(vm: Phase2Vm) -> ViewTree {
    let fragment = krui!(
        r#"
        <template name="Card">
            <style>
                .card-box { padding: 12px; border-radius: 10px; }
            </style>
            <section class="card-box">
                <h3 id="card-title">{title}</h3>
                <slot/>
            </section>
        </template>
        <div style="width: 640px; height: 480px">
            <p id="cmp-p">{vm.a < vm.b}</p>
            <if test="{vm.a < 10 && vm.ratio > 0.5}">
                <button id="next-btn" title="Next >">진행</button>
            </if>
            <p id="safe-math">{vm.b / vm.denom} | {vm.b % vm.denom} | {vm.ratio / vm.fdenom}</p>
            <p id="inline-p">안녕 <span id="hl-span">{vm.name}</span>님</p>
            <Card title="팀: {vm.name}">
                <ul id="nested-list">
                    <for each="{grp in vm.groups}" key="{grp.id}">
                        <for each="{item in grp.items}" key="{item.id}">
                            <li>{grp.name}:{item.label}</li>
                        </for>
                    </for>
                </ul>
            </Card>
            <select id="role-select" bind:value="{vm.selected_role}">
                <for each="{opt in vm.options}" key="{opt.value}">
                    <option value="{opt.value}">{opt.label}</option>
                </for>
            </select>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(fragment);
    tree
}

fn verify_phase2_tree_behavior(tree: &mut ViewTree, vm: &Phase2Vm) {
    let _ = tree.build_paint_scene(640, 480);

    let cmp_id = tree.find_by_id("cmp-p").expect("#cmp-p");
    assert_eq!(tree.node_text(cmp_id).as_deref(), Some("true"));

    let btn_id = tree.find_by_id("next-btn").expect("#next-btn");
    assert_eq!(tree.node_title(btn_id).as_deref(), Some("Next >"));

    let math_id = tree.find_by_id("safe-math").expect("#safe-math");
    assert_eq!(tree.node_text(math_id).as_deref(), Some("0 | 0 | 0"));
    vm.denom.set(3);
    vm.fdenom.set(0.25);
    assert_eq!(tree.node_text(math_id).as_deref(), Some("2 | 2 | 3"));

    let hl_id = tree.find_by_id("hl-span").expect("#hl-span");
    let inline_p_id = tree.find_by_id("inline-p").expect("#inline-p");
    let p_rect = tree.node_layout(inline_p_id).expect("inline-p layout");
    let hl_rect = tree.node_layout(hl_id).expect("hl-span layout");
    assert!(hl_rect.x > p_rect.x, "inline span must flow horizontally");

    let card_title = tree.find_by_id("card-title").expect("#card-title");
    assert_eq!(tree.node_text(card_title).as_deref(), Some("팀: 보현"));

    verify_nested_for_and_dynamic_select(tree, vm);
}

fn verify_nested_for_and_dynamic_select(tree: &mut ViewTree, vm: &Phase2Vm) {
    let sel_id = tree.find_by_id("role-select").expect("#role-select");
    assert_eq!(
        tree.select_options(sel_id),
        vec![
            ("viewer".to_string(), "열람자".to_string()),
            ("editor".to_string(), "편집자".to_string()),
        ]
    );
    assert_eq!(tree.node_selected_label(sel_id).as_deref(), Some("편집자"));

    vm.options.update(|opts| {
        opts.push(SelectOptItem {
            value: "admin".to_string(),
            label: "관리자".to_string(),
        });
    });
    vm.selected_role.set("admin".to_string());
    let _ = tree.build_paint_scene(640, 480);
    assert_eq!(tree.select_options(sel_id).len(), 3);
    assert_eq!(tree.node_selected_label(sel_id).as_deref(), Some("관리자"));

    let inspected = tree.inspect_nodes();
    let texts: Vec<&str> = inspected.iter().map(|n| n.text.as_str()).collect();
    assert!(texts.contains(&"백엔드:파서"));
    assert!(texts.contains(&"백엔드:캐스케이드"));
    assert!(texts.contains(&"런타임:시그널"));
}

#[test]
fn macro_codegen_handles_expressions_components_nested_for_select_and_safe_math() {
    let vm = Phase2Vm::sample();
    let mut tree = mount_phase2_macro_tree(vm.clone());
    verify_phase2_tree_behavior(&mut tree, &vm);
}

#[cfg(feature = "preview")]
#[test]
fn preview_interpreter_matches_macro_on_components_nested_for_select_and_safe_math() {
    let vm = Phase2Vm::sample();
    let mut scope_reg = ScopeRegistry::new();
    scope_reg.insert_registry("vm", vm.clone());
    let reg: RegistryRef = std::rc::Rc::new(scope_reg);

    let doc = preview::compile_krui_source(PARITY_PHASE2_SRC).expect("compile parity doc");
    preview::validate_compiled_document_against_registry(&doc, reg.as_ref())
        .expect("preflight must succeed including / 0 and <select><for>");
    let mut tree = ViewTree::new();
    preview::mount_compiled_document_with_registry(&mut tree, &doc, reg);
    verify_phase2_tree_behavior(&mut tree, &vm);
}

#[test]
fn runtime_hover_cascade_inheritance_scoped_vars_and_linear_dynamic_classes_work() {
    let dyn_cls = Signal::new("c1 c5 c11".to_string());
    let fragment = krui!(
        r#"
        <style>
            :root { --badge-fg: #224466; }
            button.primary:hover { background-color: #ff0000; }
            button.primary { background-color: #0000ff; }
            #inherited-box { color: #12ab34; font-size: 20px; }
            .danger-scope { --badge-fg: #ef4444; }
            .scoped-text { color: var(--badge-fg); }
            .c0 { border-radius: 1px; } .c1 { border-radius: 2px; } .c2 { border-radius: 3px; }
            .c3 { border-radius: 4px; } .c4 { border-radius: 5px; } .c5 { border-radius: 6px; }
            .c6 { border-radius: 7px; } .c7 { border-radius: 8px; } .c8 { border-radius: 9px; }
            .c9 { border-radius: 10px; } .c10 { border-radius: 11px; } .c11 { border-radius: 15px; }
        </style>
        <div id="inherited-box" style="width: 500px; height: 320px">
            <button id="pri-btn" class="primary" style="width: 140px; height: 40px">주 버튼</button>
            <p id="inh-p">상속 텍스트</p>
            <div class="danger-scope"><span id="danger-span" class="scoped-text">경고</span></div>
            <div id="patch-box" class="{dyn_cls}" style="width: 180px; height: 40px; background-color: #333333"></div>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(fragment);
    assert_hover_inheritance_and_patch_scene(&mut tree, dyn_cls);
}

fn assert_hover_inheritance_and_patch_scene(tree: &mut ViewTree, dyn_cls: Signal<String>) {
    let _ = tree.build_paint_scene(500, 320);
    let btn_rect = tree
        .node_layout(tree.find_by_id("pri-btn").expect("#pri-btn"))
        .expect("btn rect");
    let _ = tree.dispatch_pointer_move(btn_rect.x + 8.0, btn_rect.y + 8.0);
    let (scene, _) = tree.build_paint_scene(500, 320);

    let pri_hover_bg = scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if (rect.width - 140.0).abs() < 1.0 => Some(*background_color),
            _ => None,
        })
        .expect("pri-btn box");
    assert_eq!(pri_hover_bg, Color::rgb(255, 0, 0));
    assert_inherited_and_scoped_text_colors(&scene);

    dyn_cls.set("c5".to_string());
    let (scene2, _) = tree.build_paint_scene(500, 320);
    let radius = scene2
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                border_radius,
                ..
            } if (rect.width - 180.0).abs() < 1.0 => Some(*border_radius),
            _ => None,
        })
        .expect("patch-box");
    assert!((radius - 6.0).abs() < 1e-4);
}

fn assert_inherited_and_scoped_text_colors(scene: &PaintScene) {
    let glyph_colors: Vec<Color> = scene
        .primitives
        .iter()
        .filter_map(|p| match p {
            DrawPrimitive::TextGlyphs { color, .. } => Some(*color),
            _ => None,
        })
        .collect();
    assert!(
        glyph_colors.contains(&Color::rgb(0x12, 0xab, 0x34)),
        "expected inherited #12ab34 glyph color in {glyph_colors:?}"
    );
    assert!(
        glyph_colors.contains(&Color::rgb(0xef, 0x44, 0x44)),
        "expected scoped --badge-fg #ef4444 glyph color in {glyph_colors:?}"
    );
}
