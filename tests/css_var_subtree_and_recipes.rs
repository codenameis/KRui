#![forbid(unsafe_code)]

use krui::*;

#[test]
fn subtree_scoped_custom_props_and_nested_data_theme_update_children_independently() {
    reset_reactive_runtime();
    let panel_theme = Signal::new("light".to_string());
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                :root { --card-bg: #f1f5f9; --fg: #0f172a; }
                .panel[data-theme="dark"] { --card-bg: #1e293b; --fg: #f8fafc; }
                .card { width: 120px; height: 40px; background-color: var(--card-bg); color: var(--fg); }
            </style>
            <div style="width: 300px; height: 200px;">
                <section id="sec" class="panel" data-theme="{panel_theme}">
                    <div id="inner-card" class="card">Inside</div>
                </section>
                <div id="outer-card" class="card">Outside</div>
            </div>
            "#
        )
    });
    let _ = tree.build_paint_scene(300, 200);

    let inner = tree.find_by_id("inner-card").unwrap();
    let outer = tree.find_by_id("outer-card").unwrap();
    assert_eq!(
        tree.node_effective_paint(inner).unwrap().background_color,
        Color::parse_css("#f1f5f9").unwrap()
    );
    assert_eq!(
        tree.node_effective_paint(outer).unwrap().background_color,
        Color::parse_css("#f1f5f9").unwrap()
    );

    panel_theme.set("dark".to_string());
    let _ = tree.build_paint_scene(300, 200);
    let inner_dark = tree.node_effective_paint(inner).unwrap();
    let outer_still = tree.node_effective_paint(outer).unwrap();
    assert_eq!(
        inner_dark.background_color,
        Color::parse_css("#1e293b").unwrap()
    );
    assert_eq!(inner_dark.text_color, Color::parse_css("#f8fafc").unwrap());
    assert_eq!(
        outer_still.background_color,
        Color::parse_css("#f1f5f9").unwrap()
    );
}

#[test]
fn var_recipes_evaluate_calc_color_mix_and_container_percentage_width_at_runtime() {
    reset_reactive_runtime();
    let wide = Signal::new(false);
    let active = Signal::new(false);
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                .host { --w: 50%; --sp: 8px; --accent: #ef4444; width: 400px; height: 200px; }
                .host.w25-true { --w: 25%; --sp: 12px; }
                .host.hot-true { --accent: #22c55e; }
                .pct-child {
                    width: var(--w);
                    height: 36px;
                    padding: calc(var(--sp) * 2);
                    background-color: color-mix(in oklch, var(--accent) 50%, #0000ff);
                }
            </style>
            <div class="host w25-{wide} hot-{active}">
                <div id="pct" class="pct-child"></div>
            </div>
            "#
        )
    });
    let _ = tree.build_paint_scene(400, 200);
    let id = tree.find_by_id("pct").unwrap();
    let r0 = tree.node_layout(id).unwrap();
    assert!(
        (r0.width - 200.0).abs() < 1.0,
        "50% of 400px must be 200px (not font_size*0.5), got {}",
        r0.width
    );
    let c0 = tree.node_effective_paint(id).unwrap().background_color;

    wide.set(true);
    active.set(true);
    let _ = tree.build_paint_scene(400, 200);
    let r1 = tree.node_layout(id).unwrap();
    let c1 = tree.node_effective_paint(id).unwrap().background_color;
    assert!((r1.width - 100.0).abs() < 1.0, "got {}", r1.width);
    assert_ne!(c1, c0, "color-mix recipe must update when --accent changes");
}

#[test]
fn at_property_angle_time_and_inherits_false_enforce_syntax_and_boundaries() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                @property --ang { syntax: "<angle>"; inherits: false; initial-value: 30deg; }
                @property --dur { syntax: "<time>"; inherits: false; initial-value: 200ms; }
                .bad-props {
                    --ang: 20px;
                    --dur: red;
                    rotate: var(--ang);
                    transition: opacity var(--dur) linear;
                    width: 60px; height: 30px;
                }
                .good-parent {
                    --ang: 90deg;
                    rotate: var(--ang);
                    width: 60px; height: 30px;
                }
                .inh-child {
                    rotate: var(--ang);
                    width: 40px; height: 20px;
                }
            </style>
            <div>
                <div id="bad" class="bad-props"></div>
                <div id="parent" class="good-parent">
                    <div id="child" class="inh-child"></div>
                </div>
            </div>
            "#
        )
    });
    let _ = tree.build_paint_scene(300, 200);

    let bad_p = tree
        .node_effective_paint(tree.find_by_id("bad").unwrap())
        .unwrap();
    let par_p = tree
        .node_effective_paint(tree.find_by_id("parent").unwrap())
        .unwrap();
    let chi_p = tree
        .node_effective_paint(tree.find_by_id("child").unwrap())
        .unwrap();
    assert!(
        (bad_p.standalone_transform.rotate_deg - 30.0).abs() < 0.1,
        "got {}",
        bad_p.standalone_transform.rotate_deg
    );
    assert!(
        (par_p.standalone_transform.rotate_deg - 90.0).abs() < 0.1,
        "got {}",
        par_p.standalone_transform.rotate_deg
    );
    assert!(
        (chi_p.standalone_transform.rotate_deg - 30.0).abs() < 0.1,
        "inherits:false <angle> must reset on child to 30deg, got {}",
        chi_p.standalone_transform.rotate_deg
    );
}

#[test]
fn inline_style_custom_property_signal_updates_dependent_layout_property() {
    reset_reactive_runtime();
    let w_val = Signal::new("80px".to_string());
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <div style="width: 300px; height: 100px;">
                <div id="dyn-inline" style="--box-w: {w_val}; width: var(--box-w); height: 24px;"></div>
            </div>
            "#
        )
    });
    let _ = tree.build_paint_scene(300, 100);
    let id = tree.find_by_id("dyn-inline").unwrap();
    assert!((tree.node_layout(id).unwrap().width - 80.0).abs() < 0.5);

    w_val.set("165px".to_string());
    let _ = tree.build_paint_scene(300, 100);
    assert!((tree.node_layout(id).unwrap().width - 165.0).abs() < 0.5);
}
