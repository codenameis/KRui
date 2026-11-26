//! Minimal runnable headless `krui-engine-host` 120 FPS ping-pong HUD demo binary.

#![forbid(unsafe_code)]

use krui_core::{DynamicSlotOverride, ViewTree};
use krui_engine_host::{EngineHostConfig, EngineUiHost};
use krui_style::{
    Affine2, Color, CompiledStyle, Dimension, DynamicSlotId, LayoutStyle, PaintStyle,
};

fn hud_panel(w: f32, h: f32, color: Color) -> CompiledStyle {
    CompiledStyle {
        layout: LayoutStyle {
            width: Dimension::Px(w),
            height: Dimension::Px(h),
            ..LayoutStyle::default()
        },
        paint: PaintStyle {
            background_color: color,
            ..PaintStyle::default()
        },
        ..CompiledStyle::default()
    }
}

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio current_thread runtime");

    let mut tree = ViewTree::new();
    let root = tree.create_compiled_node(
        None,
        "div",
        None,
        None,
        hud_panel(1280.0, 720.0, Color::BLACK),
        false,
        false,
        false,
        false,
    );
    tree.create_compiled_node(
        Some(root),
        "div",
        None,
        None,
        hud_panel(160.0, 32.0, Color::WHITE),
        false,
        false,
        false,
        false,
    );

    let mut host =
        EngineUiHost::new_with_tokio_handle(tree, rt.handle().clone(), EngineHostConfig::default());
    host.bind_dynamic_slot(DynamicSlotId(0), 1);
    for _ in 0..2 {
        let _ = host.tick(1.0 / 120.0);
    }
    let relayouts_before = host.relayout_passes();
    let caps_before = host.scene_pool_capacities();
    for frame in 0..120 {
        host.set_dynamic_slot_override(
            DynamicSlotId(0),
            DynamicSlotOverride {
                transform: Some(Affine2::translation(frame as f32, 0.0)),
                opacity: Some(0.95),
                background_color: None,
            },
        );
        let _ = host.tick(1.0 / 120.0);
    }
    let relayouts_after = host.relayout_passes();
    let caps_after = host.scene_pool_capacities();
    println!(
        "krui-engine-host 120f HUD demo: relayouts={}->{}, capacities={:?}->{:?}",
        relayouts_before, relayouts_after, caps_before, caps_after
    );
}
