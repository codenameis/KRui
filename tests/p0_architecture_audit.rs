//! P0 architecture invariants audit (`PaintScene: Send + Sync`, reduced-motion CSS media rewrite).

#![forbid(unsafe_code)]

use krui::*;

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn p0_paint_scene_and_primitives_are_send_and_sync() {
    assert_send_sync::<PaintScene>();
    assert_send_sync::<DrawPrimitive>();
    assert_send_sync::<ImageSource>();
    assert_send_sync::<DecodedImage>();
    assert_send_sync::<GpuResourceLimits>();
    assert_send_sync::<FrameRenderStats>();
}

#[test]
fn p0_prefers_reduced_motion_media_query_updates_styles_with_preference() {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                .anim-box { width: 100px; height: 40px; background-color: #ef4444; }
                @media (prefers-reduced-motion: reduce) {
                    .anim-box { background-color: #22c55e; }
                }
            </style>
            <div id="motion-target" class="anim-box"></div>
            "#
        )
    });
    let _ = tree.build_paint_scene(320, 200);
    let node_id = tree.find_by_id("motion-target").expect("motion-target");
    let before = tree.node_effective_paint(node_id).unwrap().background_color;
    assert_eq!(before, Color::parse_css("#ef4444").unwrap());

    tree.set_reduced_motion_preference(ReducedMotionPreference::Reduce);
    let _ = tree.build_paint_scene(320, 200);
    let after = tree.node_effective_paint(node_id).unwrap().background_color;
    assert_eq!(after, Color::parse_css("#22c55e").unwrap());

    tree.set_reduced_motion_preference(ReducedMotionPreference::NoPreference);
    let _ = tree.build_paint_scene(320, 200);
    let restored = tree.node_effective_paint(node_id).unwrap().background_color;
    assert_eq!(restored, Color::parse_css("#ef4444").unwrap());
}
