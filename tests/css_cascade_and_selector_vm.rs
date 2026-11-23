#![forbid(unsafe_code)]

use krui::*;
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn cascade_priority_ancestor_hover_important_inline_and_checked_disabled() {
    reset_reactive_runtime();
    let dark = Signal::new(false);
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                .btn { width: 100px; height: 32px; background-color: #111111; }
                .btn:hover { background-color: #2563eb; }
                .dark-true .btn:hover { background-color: #9333ea; }
                #imp-btn { background-color: #ef4444 !important; }
                #imp-btn:hover { background-color: #eab308; }
                input[type="checkbox"]:checked:disabled { background-color: #16a34a; border-color: #14532d; }
                input[type="checkbox"]:invalid { border-color: #dc2626; }
            </style>
            <div class="dark-{dark}" style="width: 320px; height: 180px;">
                <button id="btn" class="btn">Hover</button>
                <button id="imp-btn" class="btn" style="background-color: #06b6d4">Imp</button>
                <input id="cd" type="checkbox" checked disabled />
            </div>
            "#
        )
    });
    let _ = tree.build_paint_scene(320, 180);

    let imp = tree.find_by_id("imp-btn").unwrap();
    let imp_r = tree.node_layout(imp).unwrap();
    let _ = tree.dispatch_pointer_move(imp_r.x + 5.0, imp_r.y + 5.0);
    let _ = tree.build_paint_scene(320, 180);
    assert_eq!(
        tree.node_effective_paint(imp).unwrap().background_color,
        Color::parse_css("#ef4444").unwrap(),
        "!important rule must beat normal inline style and :hover"
    );

    let btn = tree.find_by_id("btn").unwrap();
    let btn_r = tree.node_layout(btn).unwrap();
    dark.set(true);
    let _ = tree.dispatch_pointer_move(btn_r.x + 5.0, btn_r.y + 5.0);
    let _ = tree.build_paint_scene(320, 180);
    assert_eq!(
        tree.node_effective_paint(btn).unwrap().background_color,
        Color::parse_css("#9333ea").unwrap(),
        ".dark-true .btn:hover must win over .btn:hover"
    );

    let cd = tree.find_by_id("cd").unwrap();
    let cd_p = tree.node_effective_paint(cd).unwrap();
    assert_eq!(cd_p.background_color, Color::parse_css("#16a34a").unwrap());
}

fn li_row_bgs(scene: &PaintScene) -> Vec<Color> {
    let mut rows: Vec<(f32, Color)> = scene
        .primitives
        .iter()
        .filter_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if (rect.width - 103.0).abs() < 0.5 => Some((rect.y, *background_color)),
            _ => None,
        })
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.into_iter().map(|(_, c)| c).collect()
}

#[test]
fn ancestor_sibling_combinator_and_control_flow_unmount_invalidate_selectors() {
    reset_reactive_runtime();
    let items = Signal::new(vec![1_i32, 2, 3]);
    let show_flag = Signal::new(true);
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                #root { width: 400px; height: 240px; }
                .chk { width: 20px; height: 20px; }
                .chk:checked + .lbl .badge { background-color: #22c55e; }
                .badge { width: 30px; height: 20px; background-color: #ef4444; }
                li { width: 103px; height: 20px; background-color: #334155; }
                li:not(:last-child) { background-color: #0ea5e9; }
                #has-host { width: 80px; height: 24px; background-color: #64748b; }
                #has-host:has(> .flag) { background-color: #f59e0b; }
                #has-host:empty { background-color: #ec4899; }
            </style>
            <div id="root">
                <div style="display: flex; flex-direction: row;">
                    <input id="cb" type="checkbox" class="chk" />
                    <label class="lbl"><span id="badge" class="badge">ok</span></label>
                </div>
                <ul id="list">
                    <for each="{it in items}" key="{it}">
                        <li>item</li>
                    </for>
                </ul>
                <div id="has-host"><if test="{show_flag}"><span class="flag">F</span></if></div>
            </div>
            "#
        )
    });
    let (s0, _) = tree.build_paint_scene(400, 240);

    let cb = tree.find_by_id("cb").unwrap();
    let badge = tree.find_by_id("badge").unwrap();
    let cbr = tree.node_layout(cb).unwrap();
    assert_eq!(
        tree.node_effective_paint(badge).unwrap().background_color,
        Color::parse_css("#ef4444").unwrap()
    );
    assert!(tree.dispatch_click(cbr.x + 4.0, cbr.y + 4.0));
    let non_last = Color::parse_css("#0ea5e9").unwrap();
    let last_col = Color::parse_css("#334155").unwrap();
    assert_eq!(li_row_bgs(&s0), vec![non_last, non_last, last_col]);

    items.set(vec![1, 2, 3, 4]);
    let (s1, _) = tree.build_paint_scene(400, 240);
    assert_eq!(
        tree.node_effective_paint(badge).unwrap().background_color,
        Color::parse_css("#22c55e").unwrap(),
        "A + B C combinator must invalidate child .badge when input toggles"
    );
    assert_eq!(
        li_row_bgs(&s1),
        vec![non_last, non_last, non_last, last_col],
        "<for> append must update previous last-child matching :not(:last-child)"
    );

    let host = tree.find_by_id("has-host").unwrap();
    assert_eq!(
        tree.node_effective_paint(host).unwrap().background_color,
        Color::parse_css("#f59e0b").unwrap()
    );
    show_flag.set(false);
    let _ = tree.build_paint_scene(400, 240);
    assert_eq!(
        tree.node_effective_paint(host).unwrap().background_color,
        Color::parse_css("#ec4899").unwrap(),
        "<if> branch unmount must re-evaluate :has and :empty on parent"
    );
}

#[test]
fn nth_of_mixed_text_runs_has_chain_media_dynamic_class_and_keywords() {
    reset_reactive_runtime();
    let vis = Signal::new(true);
    let md_on = Signal::new(false);
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                @layer base, ov;
                b:nth-child(2) { color: #16a34a; }
                .row > .item { width: 40px; height: 20px; background-color: #1e293b; }
                .row > .item:nth-child(2n+1 of .vis-true) { background-color: #2563eb; }
                .outer { width: 60px; height: 24px; background-color: #ef4444; }
                .outer:has(> .mid > input:checked) { background-color: #22c55e; }
                .mbox { width: 40px; height: 20px; background-color: #64748b; }
                @media (min-width: 600px) { .mbox.active-true { background-color: #a855f7; } }
                @layer base { .rev { background-color: #0ea5e9; color: #ffffff; } }
                @layer ov { .rev { background-color: #f97316; background-color: revert-layer; } }
                .unset-box { color: #dc2626; background-color: #111111; }
                .unset-child { color: #2563eb; background-color: #999999; all: unset; }
            </style>
            <div style="width: 640px; height: 260px;">
                <div id="mix">hello <span>world</span><b id="b2">!</b></div>
                <div class="row">
                    <div id="i1" class="item vis-{vis}"></div>
                    <div id="i2" class="item vis-true"></div>
                </div>
                <div id="outer" class="outer"><div class="mid"><input id="mid-cb" type="checkbox" checked /></div></div>
                <div id="mbox" class="mbox active-{md_on}"></div>
                <div id="rev" class="rev" style="width: 30px; height: 20px;">r</div>
                <div class="unset-box"><span id="u-child" class="unset-child">u</span></div>
            </div>
            "#
        )
    });
    let _ = tree.build_paint_scene(500, 260);

    let hex = |s: &str| Color::parse_css(s).unwrap();
    assert_eq!(text_of(&tree, "b2"), hex("#16a34a"));
    assert_eq!(bg_of(&tree, "i1"), hex("#2563eb"));
    vis.set(false);
    let _ = tree.build_paint_scene(500, 260);
    assert_eq!(
        bg_of(&tree, "i2"),
        hex("#2563eb"),
        ":nth-child(2n+1 of .vis-true) must re-index when sibling loses .vis-true"
    );
    assert_eq!(bg_of(&tree, "outer"), hex("#22c55e"));
    assert_eq!(bg_of(&tree, "rev"), hex("#0ea5e9"));
    assert_eq!(bg_of(&tree, "u-child"), Color::TRANSPARENT);
    assert_eq!(text_of(&tree, "u-child"), hex("#dc2626"));

    md_on.set(true);
    let _ = tree.build_paint_scene(700, 260);
    assert_eq!(bg_of(&tree, "mbox"), hex("#a855f7"));
}

fn bg_of(tree: &ViewTree, id: &str) -> Color {
    let node = tree.find_by_id(id).unwrap();
    tree.node_effective_paint(node).unwrap().background_color
}

fn text_of(tree: &ViewTree, id: &str) -> Color {
    let node = tree.find_by_id(id).unwrap();
    tree.node_effective_paint(node).unwrap().text_color
}

#[test]
fn runtime_parent_inheritance_dynamic_pseudo_focus_placeholder_nesting_and_import() {
    reset_reactive_runtime();
    let big = Signal::new(false);
    let badge_on = Signal::new(false);
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                .parent { color: #1d4ed8; font-size: 14px; }
                .parent.big-true { color: #16a34a; font-size: 24px; }
                .tab { width: 80px; height: 24px; }
                .tab.on-true::after { content: attr(data-badge); color: #ef4444; }
                #ph-in { width: 120px; height: 28px; }
                #ph-in::placeholder { opacity: 1.0; color: #334155; }
                #ph-in:focus::placeholder { opacity: 0.25; color: #ef4444; }
                .a, .b { & > .nest-kid { background-color: #0ea5e9; width: 30px; height: 20px; } }
            </style>
            <div class="a" style="width: 300px; height: 200px;">
                <div id="par" class="parent big-{big}">
                    <span id="kid">inherit</span>
                </div>
                <div id="tab" class="tab on-{badge_on}" data-badge="NEW"></div>
                <input id="ph-in" placeholder="type" />
                <div id="nk" class="nest-kid"></div>
            </div>
            "#
        )
    });
    let _ = tree.build_paint_scene(300, 200);

    let ph = tree.find_by_id("ph-in").unwrap();
    assert!((tree.node_effective_paint(ph).unwrap().placeholder_opacity - 1.0).abs() < 0.01);
    assert!(tree.set_focus_to_node(ph));
    let _ = tree.build_paint_scene(300, 200);
    assert!((tree.node_effective_paint(ph).unwrap().placeholder_opacity - 0.25).abs() < 0.05);

    let kid_id = tree.find_by_id("kid").unwrap();
    let kid_h0 = tree.node_layout(kid_id).unwrap().height;
    big.set(true);
    badge_on.set(true);
    let (scene, _) = tree.build_paint_scene(300, 200);
    assert_eq!(
        tree.node_effective_paint(kid_id).unwrap().text_color,
        Color::parse_css("#16a34a").unwrap()
    );
    assert!(
        tree.node_layout(kid_id).unwrap().height > kid_h0 + 8.0,
        "inherited font-size 14->24 must grow child text height"
    );
    assert_eq!(
        tree.node_effective_paint(tree.find_by_id("nk").unwrap())
            .unwrap()
            .background_color,
        Color::parse_css("#0ea5e9").unwrap()
    );
    let has_after_glyphs = scene.primitives.iter().any(|p| {
        matches!(p, DrawPrimitive::TextGlyphs { color, .. } if *color == Color::parse_css("#ef4444").unwrap())
    });
    assert!(
        has_after_glyphs,
        "dynamic ::after attr(data-badge) must appear"
    );

    verify_component_scoping_and_virtual_nested_import();
}

fn verify_component_scoping_and_virtual_nested_import() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let app_path = root.join("virtual_scope/App.krui");
    let comp_path = root.join("virtual_scope/BoxTag.krui");
    let link_css = root.join("virtual_scope/styles/main.css");
    let imp_css = root.join("virtual_scope/styles/sub/tokens.css");
    let mut vbufs = HashMap::new();
    vbufs.insert(
        comp_path,
        "<style>.box { background-color: #2563eb; }</style><div id=\"in-comp\" class=\"box\" style=\"width:40px;height:20px;\"></div>".into(),
    );
    vbufs.insert(
        link_css,
        "@import \"./sub/tokens.css\"; #imp-box { background-color: var(--imp-c); width: 40px; height: 20px; }".into(),
    );
    vbufs.insert(imp_css, ":root { --imp-c: #16a34a; }".into());
    vbufs.insert(
        app_path.clone(),
        "<link rel=\"stylesheet\" href=\"./styles/main.css\" /><BoxTag /><div id=\"out-box\" class=\"box\" style=\"width:40px;height:20px;\"></div><div id=\"imp-box\"></div>".into(),
    );
    let doc = krui_compiler::compile_krui_source_with_virtual_buffers(&app_path, None, &vbufs)
        .expect("compile virtual component + nested import");
    let mut tree = ViewTree::new();
    let _ = krui_preview::mount_compiled_document(&mut tree, &doc);
    let _ = tree.build_paint_scene(200, 100);
    assert_eq!(
        tree.node_effective_paint(tree.find_by_id("in-comp").unwrap())
            .unwrap()
            .background_color,
        Color::parse_css("#2563eb").unwrap()
    );
    assert_eq!(
        tree.node_effective_paint(tree.find_by_id("out-box").unwrap())
            .unwrap()
            .background_color,
        Color::TRANSPARENT,
        "component <style> must not leak outside component boundary"
    );
    assert_eq!(
        tree.node_effective_paint(tree.find_by_id("imp-box").unwrap())
            .unwrap()
            .background_color,
        Color::parse_css("#16a34a").unwrap()
    );
}
