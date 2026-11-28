//! Tests for platform-neutral mobile widget snapshot & hotspot contract (`krui_core::app_widget`).

#![forbid(unsafe_code)]

use krui_core::{
    collect_widget_hotspots, ViewTree, WidgetActionBinding, WidgetHotspot, WidgetRenderConfig,
    WidgetSizeClass, WidgetSnapshot, MAX_WIDGET_EDGE_PX, MAX_WIDGET_SCALE, MIN_WIDGET_SCALE,
};
use krui_style::{ColorScheme, CompiledStyle, Dimension, DisplayMode, FlexDirection};

fn rect_style(w: f32, h: f32) -> CompiledStyle {
    let mut s = CompiledStyle::default();
    s.layout.display = DisplayMode::Flex;
    s.layout.flex_direction = FlexDirection::Column;
    s.layout.width = Dimension::Px(w);
    s.layout.height = Dimension::Px(h);
    s
}

#[test]
fn widget_render_config_clamps_scale_and_computes_sizes() {
    assert_eq!(WidgetSizeClass::Small.default_dp(), (160, 160));
    assert_eq!(WidgetSizeClass::Medium.default_dp(), (320, 160));

    let clamped_low = WidgetRenderConfig::new(WidgetSizeClass::Medium, f32::NAN, ColorScheme::Dark);
    assert_eq!(clamped_low.sanitized_scale(), MIN_WIDGET_SCALE);
    assert_eq!(clamped_low.physical_size(), (320, 160));

    let clamped_high = WidgetRenderConfig::from_physical_px(4096, 4096, 8.0, ColorScheme::Light);
    assert_eq!(clamped_high.sanitized_scale(), MAX_WIDGET_SCALE);
    assert!(clamped_high.physical_size().0 <= MAX_WIDGET_EDGE_PX);
}

#[test]
fn collect_widget_hotspots_extracts_positive_view_tree_rects() {
    let mut tree = ViewTree::new();
    let root = tree.create_compiled_node(
        None,
        "div",
        None,
        None,
        rect_style(320.0, 160.0),
        false,
        false,
        false,
        false,
    );
    let btn = tree.create_compiled_node(
        Some(root),
        "button",
        None,
        None,
        rect_style(120.0, 40.0),
        true,
        false,
        false,
        false,
    );
    let hidden_zero = tree.create_compiled_node(
        Some(root),
        "div",
        None,
        None,
        rect_style(0.0, 0.0),
        false,
        false,
        false,
        false,
    );
    let _ = tree.build_paint_scene(320, 160);

    let spots = collect_widget_hotspots(
        &tree,
        &[
            WidgetActionBinding {
                node_id: btn,
                action_id: 7,
            },
            WidgetActionBinding {
                node_id: hidden_zero,
                action_id: 99,
            },
        ],
    );
    assert_eq!(spots.len(), 1);
    assert_eq!(spots[0].action_id, 7);
    assert_eq!((spots[0].width, spots[0].height), (120.0, 40.0));
    assert_eq!(spots[0].to_physical_i32_tuple(2.0), [7, 0, 0, 240, 80]);
}

#[test]
fn snapshot_argb_conversion_and_topmost_hotspot_hit_test() {
    let snap = WidgetSnapshot {
        width_px: 2,
        height_px: 1,
        logical_width: 2,
        logical_height: 1,
        scale_factor: 1.0,
        revision: 3,
        rgba8: vec![0x11, 0x22, 0x33, 0xFF, 0xAA, 0xBB, 0xCC, 0x80],
        hotspots: vec![
            WidgetHotspot {
                action_id: 1,
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            WidgetHotspot {
                action_id: 2,
                x: 20.0,
                y: 20.0,
                width: 30.0,
                height: 30.0,
            },
        ],
    };
    let mut argb = Vec::new();
    snap.to_argb8888(&mut argb);
    assert_eq!(argb, vec![0xFF11_2233_u32 as i32, 0x80AA_BBCC_u32 as i32]);

    assert_eq!(snap.hit_test_action(25.0, 25.0), Some(2));
    assert_eq!(snap.hit_test_action(5.0, 5.0), Some(1));
    assert_eq!(snap.hit_test_action(150.0, 5.0), None);
}
