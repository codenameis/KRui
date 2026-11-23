//! Unit tests for `krui-layout`.

#![forbid(unsafe_code)]

use krui_layout::LayoutEngine;
use krui_style::{
    Dimension, DisplayMode, Edges, FlexDirection, GridTrack, GridTrackList, InsetEdges,
    LayoutStyle, OverflowMode, PositionMode,
};

#[test]
fn taffy_layout_solves_flexbox_and_skips_redundant_passes() {
    let mut engine = LayoutEngine::new();

    let root_style = LayoutStyle {
        width: Dimension::Px(400.0),
        height: Dimension::Px(300.0),
        padding: Edges::all(20.0),
        gap: 10.0,
        ..LayoutStyle::default()
    };
    let child_style = LayoutStyle {
        height: Dimension::Px(50.0),
        ..LayoutStyle::default()
    };

    let root = engine.create_node(&root_style);
    engine.append_root(root);
    let c1 = engine.create_node(&child_style);
    let c2 = engine.create_node(&child_style);
    engine.append_child(root, c1);
    engine.append_child(root, c2);

    assert!(engine.compute_if_dirty(root, 400.0, 300.0));
    assert_eq!(engine.relayout_passes(), 1);

    let r1 = engine.local_rect(c1);
    let r2 = engine.local_rect(c2);
    assert_eq!(r1.x, 20.0);
    assert_eq!(r1.y, 20.0);
    assert_eq!(r1.width, 360.0);
    assert_eq!(r2.y, 80.0); // 20 padding + 50 height + 10 gap

    // Subsequent frames without layout changes must NOT run Taffy again!
    assert!(!engine.compute_if_dirty(root, 400.0, 300.0));
    assert_eq!(engine.relayout_passes(), 1);
}

#[test]
fn synthetic_root_lays_out_multiple_roots_and_absolute_inset_respects_right_bottom() {
    let mut engine = LayoutEngine::new();

    let r1 = engine.create_node(&LayoutStyle {
        height: Dimension::Px(80.0),
        ..LayoutStyle::default()
    });
    let r2 = engine.create_node(&LayoutStyle {
        position: PositionMode::Relative,
        height: Dimension::Px(120.0),
        ..LayoutStyle::default()
    });
    engine.append_root(r1);
    engine.append_root(r2);

    let abs_child = engine.create_node(&LayoutStyle {
        position: PositionMode::Absolute,
        width: Dimension::Px(50.0),
        height: Dimension::Px(30.0),
        inset: InsetEdges {
            top: Dimension::Auto,
            right: Dimension::Px(10.0),
            bottom: Dimension::Px(15.0),
            left: Dimension::Auto,
        },
        ..LayoutStyle::default()
    });
    engine.append_child(r2, abs_child);

    assert!(engine.compute_if_dirty_with_measure(400.0, 300.0, |_, _| (0.0, 0.0)));
    assert_eq!(engine.relayout_passes(), 1);

    let rect1 = engine.local_rect(r1);
    let rect2 = engine.local_rect(r2);
    assert_eq!(rect1.y, 0.0);
    assert_eq!(rect1.height, 80.0);
    assert_eq!(rect2.y, 80.0);
    assert_eq!(rect2.height, 120.0);

    let abs_rect = engine.local_rect(abs_child);
    assert_eq!(abs_rect.x, 400.0 - 10.0 - 50.0);
    assert_eq!(abs_rect.y, 120.0 - 15.0 - 30.0);
}

#[test]
fn overflow_scroll_flex_child_constrains_to_viewport_and_reports_content_size() {
    let mut engine = LayoutEngine::new();

    let viewport = engine.create_node(&LayoutStyle {
        display: DisplayMode::Flex,
        flex_direction: FlexDirection::Column,
        width: Dimension::Px(320.0),
        height: Dimension::Px(200.0),
        ..LayoutStyle::default()
    });
    engine.append_root(viewport);

    let header = engine.create_node(&LayoutStyle {
        height: Dimension::Px(40.0),
        flex_shrink: 0.0,
        ..LayoutStyle::default()
    });
    let scroll_pane = engine.create_node(&LayoutStyle {
        display: DisplayMode::Flex,
        flex_direction: FlexDirection::Column,
        flex_grow: 1.0,
        overflow: OverflowMode::Scroll,
        ..LayoutStyle::default()
    });
    engine.append_child(viewport, header);
    engine.append_child(viewport, scroll_pane);

    for _ in 0..10 {
        let item = engine.create_node(&LayoutStyle {
            height: Dimension::Px(50.0),
            flex_shrink: 0.0,
            ..LayoutStyle::default()
        });
        engine.append_child(scroll_pane, item);
    }

    assert!(engine.compute_if_dirty(viewport, 320.0, 200.0));
    let pane_rect = engine.local_rect(scroll_pane);
    assert!(
        (pane_rect.height - 160.0).abs() < 0.01,
        "Flex child with overflow: scroll and flex-grow: 1 must constrain to remaining 160px viewport height, got {}",
        pane_rect.height
    );

    let (content_w, content_h) = engine.content_size(scroll_pane);
    assert!((content_w - 320.0).abs() < 0.01);
    assert!(
        (content_h - 500.0).abs() < 0.01,
        "Scrollable content_size height must reflect 10 * 50px = 500px, got {content_h}"
    );
}

#[test]
fn grid_track_list_solves_mixed_px_and_fr_columns_accurately() {
    let mut engine = LayoutEngine::new();
    let grid = engine.create_node(&LayoutStyle {
        display: DisplayMode::Grid,
        width: Dimension::Px(800.0),
        height: Dimension::Px(120.0),
        grid_template_columns: GridTrackList::from_slice(&[
            GridTrack::Px(200.0),
            GridTrack::Fr(1.0),
            GridTrack::Fr(2.0),
        ]),
        ..LayoutStyle::default()
    });
    engine.append_root(grid);

    let c0 = engine.create_node(&LayoutStyle::default());
    let c1 = engine.create_node(&LayoutStyle::default());
    let c2 = engine.create_node(&LayoutStyle::default());
    engine.append_child(grid, c0);
    engine.append_child(grid, c1);
    engine.append_child(grid, c2);

    assert!(engine.compute_if_dirty(grid, 800.0, 120.0));
    let r0 = engine.local_rect(c0);
    let r1 = engine.local_rect(c1);
    let r2 = engine.local_rect(c2);
    assert!((r0.width - 200.0).abs() < 0.01);
    assert!((r1.x - 200.0).abs() < 0.01 && (r1.width - 200.0).abs() < 0.01);
    assert!((r2.x - 400.0).abs() < 0.01 && (r2.width - 400.0).abs() < 0.01);
}

#[test]
fn flex_wrap_wraps_overflowing_row_items_onto_multiple_lines() {
    let mut engine = LayoutEngine::new();
    let container = engine.create_node(&LayoutStyle {
        display: DisplayMode::Flex,
        flex_direction: FlexDirection::Row,
        flex_wrap: true,
        width: Dimension::Px(250.0),
        gap: 10.0,
        ..LayoutStyle::default()
    });
    engine.append_root(container);

    let mut items = Vec::new();
    for _ in 0..3 {
        let chip = engine.create_node(&LayoutStyle {
            width: Dimension::Px(100.0),
            height: Dimension::Px(32.0),
            ..LayoutStyle::default()
        });
        engine.append_child(container, chip);
        items.push(chip);
    }

    assert!(engine.compute_if_dirty(container, 250.0, 200.0));
    let r0 = engine.local_rect(items[0]);
    let r1 = engine.local_rect(items[1]);
    let r2 = engine.local_rect(items[2]);
    assert_eq!(r0.y, 0.0);
    assert_eq!(r1.y, 0.0);
    assert_eq!(r1.x, 110.0);
    // Third 100px item (110 + 100 + 10 + 100 = 320 > 250) wraps onto row 2 at y = 32 + 10 = 42!
    assert_eq!(r2.x, 0.0);
    assert!((r2.y - 42.0).abs() < 0.01);
}

#[test]
fn hybrid_stylo_layout_and_explicit_override_and_subtree_removal_work() {
    use krui_stylo_bridge::{StyleSheetOrigin, StyloDomMutation, StyloStyleEngine};

    let mut stylo = StyloStyleEngine::new(Default::default());
    stylo.add_stylesheet(
        StyleSheetOrigin::Author,
        "html, body { margin: 0; } .box { width: calc(100px + 20px); height: 40px; }",
    );
    let html_id = stylo.alloc_element("html");
    let box_id = stylo.alloc_element("div");
    stylo.set_root_element(html_id);
    stylo.apply_mutation(StyloDomMutation::AppendChild {
        parent: html_id,
        child: box_id,
    });
    stylo.apply_mutation(StyloDomMutation::SetClasses {
        node: box_id,
        classes: vec!["box".to_string()],
    });
    let _ = stylo.restyle();
    assert_eq!(
        stylo.layout_style(box_id).map(|s| s.height),
        Some(Dimension::Px(40.0))
    );

    let mut engine = LayoutEngine::new();
    let taffy_box = engine.create_stylo_node(box_id);
    engine.append_root(taffy_box);
    assert!(engine.compute_if_dirty_with_stylo(&stylo, 300.0, 200.0, |_, _| (0.0, 0.0)));
    assert_eq!(engine.local_rect(taffy_box).width, 120.0);
    assert_eq!(engine.local_rect(taffy_box).height, 40.0);

    // Override wins during Tier-2 layout transitions, then clear restores Stylo `calc()`.
    engine.set_layout_override(
        taffy_box,
        LayoutStyle {
            width: Dimension::Px(180.0),
            height: Dimension::Px(60.0),
            ..LayoutStyle::default()
        },
    );
    assert!(engine.compute_if_dirty_with_stylo(&stylo, 300.0, 200.0, |_, _| (0.0, 0.0)));
    assert_eq!(engine.local_rect(taffy_box).width, 180.0);
    assert_eq!(engine.local_rect(taffy_box).height, 60.0);

    engine.clear_layout_override(taffy_box);
    assert!(engine.compute_if_dirty_with_stylo(&stylo, 300.0, 200.0, |_, _| (0.0, 0.0)));
    assert_eq!(engine.local_rect(taffy_box).width, 120.0);
    assert_eq!(engine.local_rect(taffy_box).height, 40.0);

    engine.remove_subtree(taffy_box);
    assert_eq!(engine.total_node_count(), 1);
}
