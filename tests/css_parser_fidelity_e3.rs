//! Stage E3 regression tests: parser fidelity, fallback & `0px` unit preservation,
//! stylesheet vs inline `style=""` IR equivalence, W3C `border-style: none` initial value,
//! lossy shorthand `CompileWarning` emission, and compiler-backed `@supports` evaluation.

#![forbid(unsafe_code)]

use krui::*;

#[test]
fn fallback_declarations_and_zero_px_units_survive_css_frontend() {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                :root { --z: 0px; }
                #host { width: 200px; height: 80px; }
                #fit-fb { width: 100%; width: -krui-unsupported-size; height: 20px; }
                #stretch-fb { display: inline-block; width: 100%; width: stretch; height: 20px; }
                #calc-z { width: calc(2px + var(--z)); height: 16px; }
            </style>
            <div id="host">
                <div id="fit-fb"></div>
                <div id="stretch-fb"></div>
                <div id="calc-z"></div>
            </div>
        "#
        )
    });
    let _ = tree.build_paint_scene(300, 200);

    let fit = tree
        .node_computed_style(tree.find_by_id("fit-fb").unwrap())
        .unwrap();
    assert_eq!(
        fit.layout.width,
        Dimension::Percent(100.0),
        "width: 100% fallback must not be clobbered to Auto by width: fit-content"
    );

    let stretch_rect = tree
        .node_layout(tree.find_by_id("stretch-fb").unwrap())
        .unwrap();
    assert!(
        (stretch_rect.width - 200.0).abs() <= 0.5,
        "width: 100% fallback before width: stretch must resolve to 200px, got {}",
        stretch_rect.width
    );
    let calc_rect = tree
        .node_layout(tree.find_by_id("calc-z").unwrap())
        .unwrap();
    assert!(
        (calc_rect.width - 2.0).abs() <= 0.5,
        "calc(2px + var(--z)) where --z: 0px must keep px unit and resolve to 2px, got {}",
        calc_rect.width
    );
}

#[test]
fn stylesheet_rule_and_inline_style_produce_identical_compiled_style_ir() {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                #from-sheet { display: flex; flex: 1 1 0%; gap: 8px 12px; padding: 6px 12px; margin-inline: 8px; border-style: solid; border-width: 1px 2px 3px 4px; border-color: #ef4444 #3b82f6 #22c55e #eab308; border-radius: 4px 8px 12px 16px; background: linear-gradient(90deg, #ef4444, #3b82f6); color: #f8fafc !important; }
            </style>
            <div id="from-sheet"></div>
            <div id="from-inline" style="display: flex; flex: 1 1 0%; gap: 8px 12px; padding: 6px 12px; margin-inline: 8px; border-style: solid; border-width: 1px 2px 3px 4px; border-color: #ef4444 #3b82f6 #22c55e #eab308; border-radius: 4px 8px 12px 16px; background: linear-gradient(90deg, #ef4444, #3b82f6); color: #f8fafc !important"></div>
            "#
        )
    });
    let _ = tree.build_paint_scene(300, 200);
    let a = tree
        .node_computed_style(tree.find_by_id("from-sheet").unwrap())
        .unwrap();
    let b = tree
        .node_computed_style(tree.find_by_id("from-inline").unwrap())
        .unwrap();
    assert_eq!(
        a, b,
        "stylesheet rule and inline style=\"\" must produce identical ComputedStyle"
    );
}

#[test]
fn border_style_initial_none_suppresses_border_until_line_style_is_specified() {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                .w-only { box-sizing: content-box; width: 100px; height: 30px; border-width: 4px; border-color: #ef4444; background-color: #1e293b; }
                .with-solid { border-style: solid; }
                .top-only { box-sizing: content-box; width: 100px; height: 30px; border-width: 4px; border-color: #22c55e; border-style: solid none none none; }
            </style>
            <div id="no-style" class="w-only"></div>
            <div id="solid-after" class="w-only with-solid"></div>
            <div id="top-solid" class="top-only"></div>
        "#
        )
    });
    let _ = tree.build_paint_scene(300, 240);

    let id_none = tree.find_by_id("no-style").unwrap();
    let r_none = tree.node_layout(id_none).unwrap();
    let p_none = tree.node_effective_paint(id_none).unwrap();
    assert_eq!(
        p_none.border_width, 0.0,
        "paint border_width must be 0 without border-style"
    );
    assert!(
        (r_none.width - 100.0).abs() <= 0.1,
        "content-box width without border-style must stay 100px, got {}",
        r_none.width
    );

    let id_solid = tree.find_by_id("solid-after").unwrap();
    let r_solid = tree.node_layout(id_solid).unwrap();
    let p_solid = tree.node_effective_paint(id_solid).unwrap();
    assert_eq!(
        p_solid.border_width, 4.0,
        "paint border_width must be 4px once border-style: solid applies"
    );
    assert!(
        (r_solid.width - 108.0).abs() <= 0.1,
        "content-box width with 4px solid border must be 108px, got {}",
        r_solid.width
    );

    let id_top = tree.find_by_id("top-solid").unwrap();
    let r_top = tree.node_layout(id_top).unwrap();
    assert!((r_top.width - 100.0).abs() <= 0.1 && (r_top.height - 34.0).abs() <= 0.1);
}

#[test]
fn stylo_supports_and_lossy_features_render_without_panics() {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                #sup-probe { width: 40px; height: 20px; background-color: #64748b; }
                #b-dash { border: 2px dashed #ef4444; width: 40px; height: 20px; }
                #o-dot { outline: 2px dotted #3b82f6; width: 40px; height: 20px; }
                #bg-multi { background: linear-gradient(90deg, #ef4444, #3b82f6), linear-gradient(0deg, #22c55e, #eab308); width: 40px; height: 20px; }
                @supports (width: var(--w)) {
                    #sup-probe { width: 120px; }
                }
                @supports (width: invalid-keyword) {
                    #sup-probe { height: 999px; }
                }
                @supports not (unknown()) {
                    #sup-probe { opacity: 0.5; }
                }
                @supports not (unknown_future_fn(1px)) {
                    #sup-probe { background-color: #10b981; }
                }
            </style>
            <div id="sup-probe"></div>
            <div id="b-dash"></div>
            <div id="o-dot"></div>
            <div id="bg-multi"></div>
        "#
        )
    });
    let _ = tree.build_paint_scene(300, 240);
    let n = tree
        .node_computed_style(tree.find_by_id("sup-probe").unwrap())
        .unwrap();
    assert_eq!(n.layout.width, Dimension::Px(120.0));
    assert_eq!(n.layout.height, Dimension::Px(20.0));
    assert_eq!(n.paint.background_color, Color::rgb(16, 185, 129));
    assert!((n.paint.opacity - 0.5).abs() < 1e-4);
}
