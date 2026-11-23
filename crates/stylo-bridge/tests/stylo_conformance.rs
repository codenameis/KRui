//! End-to-end Wave 1 CSS conformance tests for `krui-stylo-bridge` covering real
//! Tailwind v4 CSS, CSS Color 4/5 (`oklch`, `color-mix`, `light-dark`), `@scope`, `@layer`,
//! `@supports`, `:has()`, `@media`, pseudo-elements, `@keyframes`, `@font-face`, damage, and leaks.

#![forbid(unsafe_code)]

use krui_style::{ColorScheme, DisplayMode, FontStyleSlope, PositionMode};
use krui_stylo_bridge::{
    DeviceConfig, PseudoElementKind, StyleSheetOrigin, StyloDamage, StyloDomMutation, StyloNodeId,
    StyloStyleEngine,
};

fn build_tree_pair() -> (StyloStyleEngine, StyloNodeId, StyloNodeId) {
    let mut eng = StyloStyleEngine::new(DeviceConfig::default());
    let root = eng.alloc_element("div");
    let child = eng.alloc_element("div");
    eng.set_root_element(root);
    eng.apply_mutation(StyloDomMutation::AppendChild {
        parent: root,
        child,
    });
    (eng, root, child)
}

#[test]
fn real_tailwind_v4_sheet_loads_and_cascades_utilities() {
    let tw_css = include_str!("../../../tests/fixtures/real_css/tailwind_v4/tailwind_v4.css");
    let (mut eng, root, card) = build_tree_pair();
    eng.add_stylesheet(StyleSheetOrigin::Author, tw_css);
    eng.apply_mutation(StyloDomMutation::SetClasses {
        node: card,
        classes: vec![
            "flex".into(),
            "p-4".into(),
            "rounded-xl".into(),
            "bg-white".into(),
            "shadow-lg".into(),
        ],
    });
    let report = eng.restyle();
    assert!(!report.nodes.is_empty());

    let meta = eng.box_meta(card).expect("card meta");
    assert_eq!(meta.display, DisplayMode::Flex);
    assert!(
        meta.padding.top >= 15.0,
        "expected p-4 padding, got {:?}",
        meta.padding
    );

    let paint = eng.paint_style(card).expect("card paint");
    assert!(
        paint.corner_radii.top_left > 0.0,
        "expected rounded-xl radius"
    );
    assert_eq!(
        (
            paint.background_color.r,
            paint.background_color.g,
            paint.background_color.b
        ),
        (255, 255, 255)
    );
    assert!(paint.box_shadow.is_some(), "expected shadow-lg box-shadow");
    let _ = eng.canvas_background(root);
}

#[test]
fn modern_colors_math_layer_supports_and_has_work() {
    let css = r#"
        @layer base, theme;
        :root { color-scheme: light dark; --brand: oklch(0.62 0.21 255); }
        @layer theme {
            .btn {
                background-color: color-mix(in srgb, red 50%, blue 50%);
                color: light-dark(rgb(10, 20, 30), rgb(240, 242, 245));
                border: 2px solid var(--brand);
                border-radius: clamp(4px, 2vw, 12px);
            }
        }
        @layer base {
            .btn { background-color: rgb(1, 2, 3); }
        }
        @supports (display: grid) {
            .box { display: grid; }
        }
        @supports (container-type: inline-size) {
            .box { opacity: 0.25; }
        }
        @container (min-width: 100px) {
            .box { opacity: 0.1; }
        }
        .card:has(> .active) { border-color: rgb(255, 0, 128); border-style: solid; border-width: 3px; }
        .grid-item { position: sticky; z-index: 7; overflow-y: scroll; scrollbar-width: thin; }
    "#;
    let (mut eng, root, card) = build_tree_pair();
    let child = eng.alloc_element("span");
    let sticky = eng.alloc_element("div");
    eng.apply_mutation(StyloDomMutation::AppendChild {
        parent: card,
        child,
    });
    eng.apply_mutation(StyloDomMutation::AppendChild {
        parent: root,
        child: sticky,
    });
    eng.add_stylesheet(StyleSheetOrigin::Author, css);
    eng.apply_mutation(StyloDomMutation::SetClasses {
        node: card,
        classes: vec!["card".into(), "btn".into(), "box".into()],
    });
    eng.apply_mutation(StyloDomMutation::SetClasses {
        node: sticky,
        classes: vec!["grid-item".into()],
    });
    eng.restyle();

    let card_paint = eng.paint_style(card).unwrap();
    assert_eq!(card_paint.text_color.r, 10);
    assert!(card_paint.background_color.r > 100 && card_paint.background_color.b > 100);
    assert_eq!(eng.box_meta(card).unwrap().display, DisplayMode::Grid);
    assert!(
        (card_paint.opacity - 1.0).abs() < 1e-4,
        "@container must be dropped cleanly"
    );

    let sticky_meta = eng.box_meta(sticky).unwrap();
    assert_eq!(sticky_meta.position, PositionMode::Sticky);
    assert_eq!(sticky_meta.z_index, Some(7));

    eng.set_color_scheme(ColorScheme::Dark);
    eng.apply_mutation(StyloDomMutation::SetClasses {
        node: child,
        classes: vec!["active".into()],
    });
    eng.restyle();
    let updated = eng.paint_style(card).unwrap();
    assert_eq!(updated.text_color.r, 240);
    assert_eq!(
        (
            updated.border_color.r,
            updated.border_color.g,
            updated.border_color.b
        ),
        (255, 0, 128)
    );
}

#[test]
fn pseudo_elements_selection_placeholder_and_backdrop_work() {
    let css = r#"
        .badge::before { content: "NEW"; color: rgb(255, 255, 255); background-color: rgb(30, 120, 240); }
        .badge::after { content: "!"; }
        input::placeholder { color: rgb(120, 130, 140); opacity: 0.75; }
        .editor::selection { color: rgb(255, 255, 255); background-color: rgb(0, 100, 255); }
        dialog::backdrop { background-color: rgba(10, 20, 30, 0.6); backdrop-filter: blur(8px); }
    "#;
    let (mut eng, root, badge) = build_tree_pair();
    let input = eng.alloc_element("input");
    let dialog = eng.alloc_element("dialog");
    eng.apply_mutation(StyloDomMutation::AppendChild {
        parent: root,
        child: input,
    });
    eng.apply_mutation(StyloDomMutation::AppendChild {
        parent: root,
        child: dialog,
    });
    eng.add_stylesheet(StyleSheetOrigin::Author, css);
    eng.apply_mutation(StyloDomMutation::SetClasses {
        node: badge,
        classes: vec!["badge".into(), "editor".into()],
    });
    eng.restyle();

    assert_eq!(
        eng.pseudo_content(badge, PseudoElementKind::Before)
            .as_deref(),
        Some("NEW")
    );
    assert_eq!(
        eng.pseudo_content(badge, PseudoElementKind::After)
            .as_deref(),
        Some("!")
    );
    assert_eq!(
        eng.pseudo_paint_style(badge, PseudoElementKind::Before)
            .unwrap()
            .background_color
            .b,
        240
    );

    let badge_paint = eng.paint_style(badge).unwrap();
    assert_eq!(badge_paint.selection_color.map(|c| c.r), Some(255));
    assert_eq!(badge_paint.selection_background.map(|c| c.b), Some(255));

    let input_paint = eng.paint_style(input).unwrap();
    assert_eq!(
        input_paint.placeholder_color.map(|c| (c.r, c.g, c.b)),
        Some((120, 130, 140))
    );
    assert!((input_paint.placeholder_opacity - 0.75).abs() < 1e-3);

    let bd = eng
        .pseudo_paint_style(dialog, PseudoElementKind::Backdrop)
        .unwrap();
    assert!((bd.backdrop_blur_px - 8.0).abs() < 1e-2);
    assert_eq!(bd.background_color.b, 30);
}

#[test]
fn keyframes_transitions_transforms_and_font_face_extract() {
    let css = r#"
        @font-face {
            font-family: "CustomSans";
            src: local("Segoe UI"), local("Malgun Gothic");
            font-weight: 300 700;
            font-style: italic;
            unicode-range: U+0020-007F;
        }
        @keyframes pulse {
            from { opacity: 0.25; transform: scale(1); }
            to { opacity: 1.0; transform: scale(1.2); }
        }
        @keyframes expand {
            from { width: 40px; }
            to { width: 120px; }
        }
        :root { font-family: "CustomSans", sans-serif; }
        .anim {
            animation: pulse 400ms ease-in-out infinite alternate;
            transition: transform 200ms cubic-bezier(0.2, 0.8, 0.2, 1), width 150ms linear;
            translate: 12px 50%;
            rotate: 45deg;
            transform: rotateX(15deg);
            background-image: linear-gradient(90deg, rgb(255, 0, 0), rgb(0, 0, 255));
            box-shadow: 0 2px 6px rgba(0, 0, 0, 0.3), inset 0 1px 2px rgba(255, 255, 255, 0.2);
        }
    "#;
    let (mut eng, _root, node) = build_tree_pair();
    eng.add_stylesheet(StyleSheetOrigin::Author, css);
    eng.apply_mutation(StyloDomMutation::SetClasses {
        node,
        classes: vec!["anim".into()],
    });
    eng.restyle();

    let faces = eng.font_face_rules();
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].family, "CustomSans");
    assert_eq!((faces[0].weight_min, faces[0].weight_max), (300, 700));
    assert_eq!(faces[0].style, FontStyleSlope::Italic);

    let kfs = eng.keyframes_defs();
    let pulse = kfs
        .iter()
        .find(|k| k.name == "pulse")
        .expect("pulse keyframes");
    let expand = kfs
        .iter()
        .find(|k| k.name == "expand")
        .expect("expand keyframes");
    assert!(!pulse.touches_layout);
    assert!(expand.touches_layout);
    assert_eq!(pulse.stops.len(), 2);

    let paint = eng.paint_style(node).unwrap();
    assert!(!paint.animations.is_empty());
    assert!(paint.transition.is_active());
    assert!(paint.transition.has_layout_transition());
    assert!(paint.background_gradient.is_some());
    assert!(paint.box_shadow.is_some() && paint.secondary_shadow.is_some());
    assert!(paint.has_non_identity_transform());
}

#[test]
fn damage_distinguishes_repaint_vs_relayout_and_disposes_cleanly() {
    let css = r#"
        .item { background-color: rgb(10, 10, 10); width: 100px; height: 40px; opacity: 1; }
        .painted { background-color: rgb(250, 20, 20); opacity: 0.5; transform: translateX(8px); }
        .resized { width: 180px; }
    "#;
    let (mut eng, root, item) = build_tree_pair();
    eng.add_stylesheet(StyleSheetOrigin::Author, css);
    eng.apply_mutation(StyloDomMutation::SetClasses {
        node: item,
        classes: vec!["item".into()],
    });
    eng.restyle();

    eng.apply_mutation(StyloDomMutation::SetClasses {
        node: item,
        classes: vec!["item".into(), "painted".into()],
    });
    let r_paint = eng.restyle();
    assert_eq!(
        r_paint.nodes.get(&item).copied(),
        Some(StyloDamage::Repaint)
    );

    eng.apply_mutation(StyloDomMutation::SetClasses {
        node: item,
        classes: vec!["item".into(), "resized".into()],
    });
    let r_layout = eng.restyle();
    assert_eq!(
        r_layout.nodes.get(&item).copied(),
        Some(StyloDamage::Relayout)
    );

    for _ in 0..50 {
        let tmp = eng.alloc_element("div");
        eng.apply_mutation(StyloDomMutation::AppendChild {
            parent: root,
            child: tmp,
        });
        eng.restyle();
        eng.apply_mutation(StyloDomMutation::DisposeSubtree(tmp));
        eng.restyle();
    }
    assert_eq!(eng.active_slot_count(), 2);
}

#[test]
fn active_before_pseudo_paint_change_without_after_emits_only_repaint() {
    let css = r#"
        .btn { width: 120px; height: 40px; position: relative; background-color: rgb(32, 26, 48); }
        .btn::before { content: ""; position: absolute; inset: 0px; opacity: 0; background-color: rgb(255, 85, 34); }
        .btn:active::before { opacity: 0.85; }
    "#;
    let (mut eng, _root, btn) = build_tree_pair();
    eng.add_stylesheet(StyleSheetOrigin::Author, css);
    eng.apply_mutation(StyloDomMutation::SetClasses {
        node: btn,
        classes: vec!["btn".into()],
    });
    eng.restyle();

    eng.apply_mutation(StyloDomMutation::SetState {
        node: btn,
        state: krui_stylo_bridge::DomElementState::ACTIVE,
    });
    let press_dmg = eng.restyle();
    assert_eq!(press_dmg.nodes.get(&btn), Some(&StyloDamage::Repaint));

    eng.apply_mutation(StyloDomMutation::SetState {
        node: btn,
        state: krui_stylo_bridge::DomElementState::EMPTY,
    });
    let release_dmg = eng.restyle();
    assert_eq!(release_dmg.nodes.get(&btn), Some(&StyloDamage::Repaint));
}

#[test]
fn fixed_border_box_container_border_change_emits_relayout_when_it_has_element_children() {
    let css = r#"
        .box { width: 100px; height: 36px; border: 2px solid #79747e; }
        .box.open { border-left-width: 0px; }
    "#;
    let (mut eng, _root, container) = build_tree_pair();
    let inner = eng.alloc_element("div");
    eng.apply_mutation(StyloDomMutation::AppendChild {
        parent: container,
        child: inner,
    });
    eng.add_stylesheet(StyleSheetOrigin::Author, css);
    eng.apply_mutation(StyloDomMutation::SetClasses {
        node: container,
        classes: vec!["box".into(), "open".into()],
    });
    eng.restyle();

    eng.apply_mutation(StyloDomMutation::SetClasses {
        node: container,
        classes: vec!["box".into()],
    });
    let dmg = eng.restyle();
    assert_eq!(dmg.nodes.get(&container), Some(&StyloDamage::Relayout));
}
