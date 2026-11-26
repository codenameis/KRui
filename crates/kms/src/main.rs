//! Minimal runnable headless `krui-kms` PiTFT 3.5" (`480x320` RGB565) demo binary.

#![forbid(unsafe_code)]

use krui_core::ViewTree;
use krui_kms::{KioskAppHost, KioskHostConfig, MockDrmSink};
use krui_signals::Signal;
use krui_style::{
    Color, CompiledStyle, Dimension, InsetEdges, LayoutStyle, PaintStyle, PositionMode,
};

fn panel_style(x: f32, y: f32, w: f32, h: f32, color: Color) -> CompiledStyle {
    CompiledStyle {
        layout: LayoutStyle {
            position: PositionMode::Absolute,
            inset: InsetEdges {
                left: Dimension::Px(x),
                top: Dimension::Px(y),
                ..InsetEdges::AUTO
            },
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
    let sink = MockDrmSink::new();
    let commits = sink.log_handle();
    let mut tree = ViewTree::new();
    let root = tree.create_compiled_node(
        None,
        "div",
        None,
        None,
        panel_style(0.0, 0.0, 480.0, 320.0, Color::BLACK),
        false,
        false,
        false,
        false,
    );
    let badge = tree.create_compiled_node(
        Some(root),
        "div",
        None,
        None,
        panel_style(24.0, 24.0, 96.0, 36.0, Color::WHITE),
        false,
        false,
        false,
        false,
    );
    let badge_opacity = Signal::new(1.0_f32);
    tree.bind_paint_opacity(badge, badge_opacity);

    let mut host = KioskAppHost::new(tree, sink, KioskHostConfig::default());
    let f1 = host.render_and_submit();
    let _ = host.wait_flushed(f1.flush_seq);
    badge_opacity.set(0.6);
    let f2 = host.render_and_submit();
    let stats = host.wait_flushed(f2.flush_seq);
    let timings = host.boot_timings();
    let count = commits.lock().map(|g| g.len()).unwrap_or(0);
    println!(
        "krui-kms 480x320 RGB565 demo: commits={count}, last_flush_us={}, boot_init_us={}",
        stats.last_flush_us, timings.host_init_us
    );
}
