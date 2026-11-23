#![forbid(unsafe_code)]

use krui::*;

const REPRO_CASES_SRC: &str = r#"
<link rel="krui-preset" href="krui://preset/unstyled" />
<style>
    @property --c {
        syntax: "*";
        inherits: false;
        initial-value: #6366f1;
    }
    :root {
        --a: var(--only-dark);
        --cyc1: var(--cyc2);
        --cyc2: var(--cyc1);
        --d0: 10px;
        --d1: var(--d0);
        --d2: var(--d1);
        --d3: var(--d2);
        --d4: var(--d3);
        --d5: var(--d4);
        --d6: var(--d5);
        --d7: var(--d6);
        --d8: var(--d7);
        --d9: var(--d8);
        --d10: var(--d9);
    }
    .dark { --only-dark: #000000; }

    .a { --x: var(--y); background-color: var(--x); width: 40px; height: 20px; }
    .b { --y: #ff0000; }

    .ring { --s: 0px 0px 0px 2px var(--c); box-shadow: var(--s); width: 40px; height: 20px; }
    .r { --c: #ff0000; }

    .inh-parent { color: #16a34a; }
    .inh-child { color: #ef4444; color: var(--undefined-text-color); }
    .noninh-box {
        width: 140px;
        width: var(--undefined-width);
        height: 28px;
        background-color: var(--cyc1, #0ea5e9);
    }
    .depth-box {
        width: var(--d10, 66px);
        height: 20px;
    }
    .cyc-fb-box {
        width: var(--cyc1, 66px);
        height: 20px;
    }
</style>
<div>
    <div id="repro1" class="a b"></div>
    <div id="repro2" class="ring r"></div>
    <div class="r"><div id="repro2-no-inherit" class="ring"></div></div>
    <div class="inh-parent"><span id="iacvt-inh" class="inh-child">txt</span></div>
    <div style="display:flex; width:300px">
        <div id="iacvt-noninh" class="noninh-box"><div style="width:36px;height:10px"></div></div>
    </div>
    <div id="depth-fb" class="depth-box"></div>
    <div id="cyc-fb" class="cyc-fb-box"></div>
</div>
"#;

const AT_PROPERTY_SYNTAX_SRC: &str = r#"
<link rel="krui-preset" href="krui://preset/unstyled" />
<style>
    @property --p-len {
        syntax: "<length>";
        inherits: false;
        initial-value: 24px;
    }
    @property --p-col {
        syntax: "<color>";
        inherits: false;
        initial-value: #22c55e;
    }
    @property --p-num {
        syntax: "<number>";
        inherits: false;
        initial-value: 0.5;
    }
    @property --p-pct {
        syntax: "<percentage>";
        inherits: false;
        initial-value: 50%;
    }
    .bad-types {
        --p-len: not-a-length;
        --p-col: 123px;
        --p-num: red;
        --p-pct: 12px;
        width: var(--p-len);
        height: 20px;
        background-color: var(--p-col);
        opacity: var(--p-num);
    }
    .pct-host { width: 200px; height: 20px; }
    .bad-pct {
        --p-pct: 12px;
        width: var(--p-pct);
        height: 20px;
    }
    .good-types {
        --p-len: 48px;
        --p-col: #ef4444;
        --p-num: 0.75;
        width: var(--p-len);
        height: 20px;
        background-color: var(--p-col);
        opacity: var(--p-num);
    }
</style>
<div>
    <div id="bad" class="bad-types"></div>
    <div class="pct-host"><div id="bad-pct" class="bad-pct"></div></div>
    <div id="good" class="good-types"></div>
</div>
"#;

const DYNAMIC_BOUNDARY_SRC: &str = r#"
<link rel="krui-preset" href="krui://preset/unstyled" />
<style>
    :root { --surface-tok: #1e293b; }
    [data-theme="warm"] { --surface-tok: #f59e0b; }
    .dyn-card {
        --btn-bg: #334155;
        width: 100px; height: 40px;
        background-color: var(--btn-bg);
    }
    .dyn-card:hover {
        --btn-bg: #ef4444;
    }
    .themed-box {
        width: 80px; height: 30px;
        background-color: var(--surface-tok);
    }
</style>
<div id="theme-root" data-theme="cool">
    <button id="hover-btn" class="dyn-card">Hover</button>
    <div id="themed" class="themed-box"></div>
</div>
"#;

#[test]
fn w3c_custom_properties_post_cascade_iacvt_and_guaranteed_invalid() {
    let doc = krui_compiler::compile_krui_source(REPRO_CASES_SRC)
        .expect("W3C mode must not fail on undefined/cyclic/deep custom properties");

    let mut tree = ViewTree::new();
    let _ = krui_preview::mount_compiled_document(&mut tree, &doc);
    let _ = tree.build_paint_scene(400, 300);

    let repro1_p = tree
        .node_effective_paint(tree.find_by_id("repro1").unwrap())
        .unwrap();
    assert_eq!(repro1_p.background_color, Color::rgb(255, 0, 0));

    let repro2_p = tree
        .node_effective_paint(tree.find_by_id("repro2").unwrap())
        .unwrap();
    assert_eq!(
        repro2_p.box_shadow.expect("ring shadow").color,
        Color::rgb(255, 0, 0),
        "Repro 2: .ring + .r on same element must resolve --c to red, not initial indigo"
    );

    let no_inh_p = tree
        .node_effective_paint(tree.find_by_id("repro2-no-inherit").unwrap())
        .unwrap();
    assert_eq!(
        no_inh_p.box_shadow.expect("child ring shadow").color,
        Color::parse_css("#6366f1").unwrap(),
        "@property inherits:false must reset child --c to initial-value #6366f1"
    );

    let inh_p = tree
        .node_effective_paint(tree.find_by_id("iacvt-inh").unwrap())
        .unwrap();
    assert_eq!(
        inh_p.text_color,
        Color::parse_css("#16a34a").unwrap(),
        "IACVT on inherited `color` must compute to `inherit`, overriding earlier rule"
    );

    let noninh_id = tree.find_by_id("iacvt-noninh").unwrap();
    let noninh_r = tree.node_layout(noninh_id).unwrap();
    let noninh_p = tree.node_effective_paint(noninh_id).unwrap();
    assert!(
        (noninh_r.width - 36.0).abs() <= 0.5,
        "IACVT on non-inherited `width` must reset to `auto` (36px content), got {}",
        noninh_r.width
    );
    assert_eq!(
        noninh_p.background_color,
        Color::parse_css("#0ea5e9").unwrap(),
        "guaranteed-invalid cyclic var must fall back to #0ea5e9"
    );

    let depth_r = tree
        .node_layout(tree.find_by_id("depth-fb").unwrap())
        .unwrap();
    assert!(
        (depth_r.width - 10.0).abs() <= 0.5,
        "valid 11-level non-cyclic var() chain must resolve to 10px, got {}",
        depth_r.width
    );
    let cyc_r = tree
        .node_layout(tree.find_by_id("cyc-fb").unwrap())
        .unwrap();
    assert!((cyc_r.width - 66.0).abs() <= 0.5);
}

#[test]
fn at_property_syntax_validation_falls_back_to_initial_value() {
    let doc =
        krui_compiler::compile_krui_source(AT_PROPERTY_SYNTAX_SRC).expect("compile @property");
    let mut tree = ViewTree::new();
    let _ = krui_preview::mount_compiled_document(&mut tree, &doc);
    let _ = tree.build_paint_scene(400, 200);

    let bad_id = tree.find_by_id("bad").unwrap();
    let bad_r = tree.node_layout(bad_id).unwrap();
    let bad_p = tree.node_effective_paint(bad_id).unwrap();
    assert!(
        (bad_r.width - 24.0).abs() <= 0.5,
        "bad <length> -> 24px, got {}",
        bad_r.width
    );
    assert_eq!(bad_p.background_color, Color::parse_css("#22c55e").unwrap());
    assert!((bad_p.opacity - 0.5).abs() <= 1e-4);

    let bad_pct_r = tree
        .node_layout(tree.find_by_id("bad-pct").unwrap())
        .unwrap();
    assert!(
        (bad_pct_r.width - 100.0).abs() <= 0.5,
        "bad <percentage> -> 50% of 200px = 100px"
    );

    let good_id = tree.find_by_id("good").unwrap();
    let good_r = tree.node_layout(good_id).unwrap();
    let good_p = tree.node_effective_paint(good_id).unwrap();
    assert!((good_r.width - 48.0).abs() <= 0.5);
    assert_eq!(
        good_p.background_color,
        Color::parse_css("#ef4444").unwrap()
    );
    assert!((good_p.opacity - 0.75).abs() <= 1e-4);
}

#[test]
fn dynamic_hover_and_scoped_theme_custom_property_boundaries_preserved() {
    let doc = krui_compiler::compile_krui_source(DYNAMIC_BOUNDARY_SRC).expect("compile dynamic");
    let mut tree = ViewTree::new();
    let _ = krui_preview::mount_compiled_document(&mut tree, &doc);
    let _ = tree.build_paint_scene(300, 200);

    let btn_id = tree.find_by_id("hover-btn").unwrap();
    let btn_r = tree.node_layout(btn_id).unwrap();
    assert_eq!(
        tree.node_effective_paint(btn_id).unwrap().background_color,
        Color::parse_css("#334155").unwrap()
    );
    let _ = tree.dispatch_pointer_move(btn_r.x + 10.0, btn_r.y + 10.0);
    let _ = tree.build_paint_scene(300, 200);
    assert_eq!(
        tree.node_effective_paint(btn_id).unwrap().background_color,
        Color::parse_css("#ef4444").unwrap(),
        ":hover custom property override must update background_color"
    );

    let themed_id = tree.find_by_id("themed").unwrap();
    let root_id = tree.find_by_id("theme-root").unwrap();
    assert_eq!(
        tree.node_effective_paint(themed_id)
            .unwrap()
            .background_color,
        Color::parse_css("#1e293b").unwrap()
    );
    tree.set_node_attr(root_id, "data-theme", "warm");
    let _ = tree.build_paint_scene(300, 200);
    assert_eq!(
        tree.node_effective_paint(themed_id)
            .unwrap()
            .background_color,
        Color::parse_css("#f59e0b").unwrap(),
        "[data-theme] scoped custom property must update at runtime"
    );
}

#[test]
fn wave1_root_layer_order_unmatched_scope_non_leak_and_strict_custom_props() {
    let layer_src = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            @layer lo, hi;
            @layer hi { :root { --c: #22aa33; } }
            @layer lo { :root { --c: #aa2233; } }
            @layer sub_parent { :root { --nested: #111111; } }
            @layer sub_later { :root { --nested: #223344; } }
            @layer sub_parent.inner { :root { --inner-only: 1px; } }
            #a { background-color: var(--c); color: var(--nested); }
        </style>
        <div id="a">ok</div>
    "#;
    let doc = krui_compiler::compile_krui_source(layer_src).expect("compile layers");
    let mut tree = ViewTree::new();
    let _ = krui_preview::mount_compiled_document(&mut tree, &doc);
    let _ = tree.build_paint_scene(200, 100);
    let p = tree
        .node_effective_paint(tree.find_by_id("a").unwrap())
        .unwrap();
    assert_eq!(p.background_color, Color::rgb(0x22, 0xaa, 0x33));
    assert_eq!(p.text_color, Color::rgb(0x22, 0x33, 0x44));

    let leak_src = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            [data-theme="neon"] { --secret: #123456; }
            .box { width: 40px; height: 40px; background-color: var(--secret); }
        </style>
        <div id="box" class="box"></div>
    "#;
    let leak_doc = krui_compiler::compile_krui_source(leak_src).expect("compile non-leak");
    let mut leak_tree = ViewTree::new();
    let _ = krui_preview::mount_compiled_document(&mut leak_tree, &leak_doc);
    let _ = leak_tree.build_paint_scene(200, 100);
    assert_eq!(
        leak_tree
            .node_computed_style(leak_tree.find_by_id("box").unwrap())
            .unwrap()
            .paint
            .background_color,
        Color::TRANSPARENT
    );

    let strict_ok = r#"
        <style>
            :root { --content: "hi"; --grid: 1fr 2fr; --badge: #112233; }
            .card { background-color: var(--badge); }
        </style>
        <div class="card">ok</div>
    "#;
    assert!(krui_compiler::compile_krui_source_strict(strict_ok).is_ok());
}
