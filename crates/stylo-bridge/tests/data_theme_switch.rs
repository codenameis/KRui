#![forbid(unsafe_code)]

use krui_style::ColorScheme;
use krui_stylo_bridge::{
    DeviceConfig, DomElementState, ElementAttr, PointerKind, PseudoElementKind, StyleSheetOrigin,
    StyloDamage, StyloDomMutation, StyloNodeId, StyloStyleEngine,
};

fn new_engine(unstyled: bool) -> StyloStyleEngine {
    StyloStyleEngine::new(DeviceConfig {
        viewport_width: 1000.0,
        viewport_height: 800.0,
        dpr: 1.0,
        color_scheme: ColorScheme::Light,
        pointer: PointerKind::FineHover,
        unstyled,
    })
}

fn setup_theme_tree(engine: &mut StyloStyleEngine) -> (StyloNodeId, StyloNodeId) {
    let html = engine.alloc_element("html");
    let body = engine.alloc_element("body");
    let root = engine.alloc_element("div");
    let child = engine.alloc_element("button");
    engine.set_root_element(html);
    engine.apply_mutation(StyloDomMutation::AppendChild {
        parent: html,
        child: body,
    });
    engine.apply_mutation(StyloDomMutation::AppendChild {
        parent: body,
        child: root,
    });
    engine.apply_mutation(StyloDomMutation::AppendChild {
        parent: root,
        child,
    });
    engine.apply_mutation(StyloDomMutation::SetId {
        node: root,
        id: Some("component-gallery-root".into()),
    });
    engine.apply_mutation(StyloDomMutation::SetClasses {
        node: root,
        classes: vec!["gallery-shell".into()],
    });
    engine.apply_mutation(StyloDomMutation::SetAttributes {
        node: root,
        attrs: vec![ElementAttr {
            name: "data-theme".into(),
            value: "material".into(),
        }],
    });
    (root, child)
}

#[test]
fn data_theme_attribute_switch_updates_descendant_custom_properties_with_zero_relayout() {
    let mut engine = new_engine(true);
    let (root, child) = setup_theme_tree(&mut engine);
    engine.add_stylesheet(
        StyleSheetOrigin::Author,
        r#"
        [data-theme="material"] {
            color-scheme: light dark;
            --md-sys-color-primary: light-dark(#6750a4, #d0bcff);
            --krui-accent: var(--md-sys-color-primary);
            --krui-btn-radius: 999px;
        }
        [data-theme="tailwind"] {
            color-scheme: light dark;
            --krui-accent: light-dark(#4f46e5, #818cf8);
            --krui-btn-radius: 6px;
        }
        .btn {
            background-color: var(--krui-accent);
            border-radius: var(--krui-btn-radius);
        }
        "#,
    );
    engine.apply_mutation(StyloDomMutation::SetClasses {
        node: child,
        classes: vec!["btn".into()],
    });

    let init_report = engine.restyle();
    let _ = engine.take_diagnostics();
    assert!(!init_report.nodes.is_empty());
    let init_paint = engine.paint_style(child).expect("initial child paint");
    assert_eq!(
        (
            init_paint.background_color.r,
            init_paint.background_color.g,
            init_paint.background_color.b
        ),
        (0x67, 0x50, 0xa4)
    );
    assert!((init_paint.border_radius - 999.0).abs() < 0.01);
    assert_tailwind_light_and_dark_switch(&mut engine, root, child);
}

fn assert_tailwind_light_and_dark_switch(
    engine: &mut StyloStyleEngine,
    root: StyloNodeId,
    child: StyloNodeId,
) {
    engine.apply_mutation(StyloDomMutation::SetAttributes {
        node: root,
        attrs: vec![
            ElementAttr {
                name: "id".into(),
                value: "component-gallery-root".into(),
            },
            ElementAttr {
                name: "data-theme".into(),
                value: "tailwind".into(),
            },
        ],
    });
    let switch_report = engine.restyle();
    let diags = engine.take_diagnostics();
    assert!(diags.is_empty(), "unexpected CSS diagnostics: {diags:?}");
    let tw_paint = engine.paint_style(child).expect("tailwind child paint");
    assert_eq!(
        (
            tw_paint.background_color.r,
            tw_paint.background_color.g,
            tw_paint.background_color.b
        ),
        (0x4f, 0x46, 0xe5),
        "Tailwind light accent must update via [data-theme], report={:?}",
        switch_report.nodes
    );
    assert!(
        (tw_paint.border_radius - 6.0).abs() < 0.01,
        "Tailwind border_radius must be 6px, got {}",
        tw_paint.border_radius
    );
    assert!(
        !switch_report
            .nodes
            .values()
            .any(|d| *d == StyloDamage::Relayout),
        "attribute switch must produce 0 Relayout damages, got {:?}",
        switch_report.nodes
    );

    engine.set_color_scheme(ColorScheme::Dark);
    let dark_report = engine.restyle();
    let dark_paint = engine.paint_style(child).expect("tailwind dark paint");
    assert_eq!(
        (
            dark_paint.background_color.r,
            dark_paint.background_color.g,
            dark_paint.background_color.b
        ),
        (0x81, 0x8c, 0xf8),
        "Dark mode must resolve Tailwind dark token, report={:?}",
        dark_report.nodes
    );
}

#[test]
fn gallery_stylesheets_switch_btn_filled_background_via_data_theme() {
    let mut engine = new_engine(true);
    let (root, child) = setup_theme_tree(&mut engine);

    let actions_raw = include_str!("../../../crates/compiler/builtins/actions.krui");
    let s0 = actions_raw.find("<style>").unwrap() + 7;
    let s1 = actions_raw.find("</style>").unwrap();
    engine.add_stylesheet(
        StyleSheetOrigin::Author,
        &format!("@layer component {{\n{}\n}}", &actions_raw[s0..s1]),
    );
    engine.add_stylesheet(
        StyleSheetOrigin::Author,
        include_str!("../../../examples/component_gallery/views/themes/material.css"),
    );
    engine.add_stylesheet(
        StyleSheetOrigin::Author,
        include_str!("../../../examples/component_gallery/views/themes/tailwind.css"),
    );
    engine.add_stylesheet(
        StyleSheetOrigin::Author,
        include_str!("../../../examples/component_gallery/views/component_gallery.css"),
    );

    engine.apply_mutation(StyloDomMutation::SetId {
        node: child,
        id: Some("btn-demo-filled".into()),
    });
    engine.apply_mutation(StyloDomMutation::SetClasses {
        node: child,
        classes: vec!["krui-ui-btn".into(), "krui-ui-btn-filled".into()],
    });
    engine.restyle();
    let _ = engine.take_diagnostics();
    assert_gallery_hover_and_tailwind_switch(&mut engine, root, child);
}

fn assert_gallery_hover_and_tailwind_switch(
    engine: &mut StyloStyleEngine,
    root: StyloNodeId,
    child: StyloNodeId,
) {
    // Hover first so `::before` transition (`opacity: 0 -> 0.08`) returns post-cascade `0.08`.
    engine.apply_mutation(StyloDomMutation::SetState {
        node: child,
        state: DomElementState::HOVER,
    });
    engine.restyle();
    let hover_before = engine
        .pseudo_paint_style(child, PseudoElementKind::Before)
        .expect("hovered ::before style");
    assert!(
        (hover_before.opacity - 0.08).abs() < 0.01,
        "::before with transition must return post-cascade 0.08, got {}",
        hover_before.opacity
    );
    engine.apply_mutation(StyloDomMutation::SetState {
        node: child,
        state: DomElementState::EMPTY,
    });
    engine.restyle();

    engine.apply_mutation(StyloDomMutation::SetAttributes {
        node: root,
        attrs: vec![ElementAttr {
            name: "data-theme".into(),
            value: "tailwind".into(),
        }],
    });
    let report = engine.restyle();
    let p = engine.paint_style(child).expect("tw btn paint");

    let fresh_child = engine.alloc_element("button");
    engine.apply_mutation(StyloDomMutation::AppendChild {
        parent: root,
        child: fresh_child,
    });
    engine.apply_mutation(StyloDomMutation::SetClasses {
        node: fresh_child,
        classes: vec!["krui-ui-btn".into(), "krui-ui-btn-filled".into()],
    });
    engine.restyle();
    let pf = engine.paint_style(fresh_child).expect("fresh btn paint");
    assert_eq!(
        (
            pf.background_color.r,
            pf.background_color.g,
            pf.background_color.b
        ),
        (0x4f, 0x46, 0xe5),
        "fresh sibling button under [data-theme=\"tailwind\"] must get #4f46e5"
    );
    assert_eq!(
        (
            p.background_color.r,
            p.background_color.g,
            p.background_color.b
        ),
        (0x4f, 0x46, 0xe5),
        "gallery sheets must switch btn bg to Tailwind (#4f46e5), got {:?}, report={:?}",
        p.background_color,
        report.nodes
    );
}
