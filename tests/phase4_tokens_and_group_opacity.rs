#![forbid(unsafe_code)]

use krui::{
    krui, Color, DrawPrimitive, GpuResourceLimits, NodeId, RendererBackend, TinySkiaRenderer,
    ViewTree, WgpuRenderer,
};

fn assert_preset_blur_and_shadow_primitives(
    scene: &krui::PaintScene,
    theme_name: &str,
    expected_blur: f32,
    expect_shadow: bool,
) {
    let push_positions: Vec<usize> = scene
        .primitives
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            matches!(p, DrawPrimitive::PushLayer { opacity, .. } if (*opacity - 0.5).abs() < 1e-3)
                .then_some(i)
        })
        .collect();
    let pop_positions: Vec<usize> = scene
        .primitives
        .iter()
        .enumerate()
        .filter_map(|(i, p)| matches!(p, DrawPrimitive::PopLayer).then_some(i))
        .collect();
    assert_eq!(
        push_positions.len(),
        1,
        "opacity<1 container must emit PushLayer"
    );
    assert_eq!(
        pop_positions.len(),
        1,
        "opacity<1 container must emit PopLayer"
    );
    let (push_idx, pop_idx) = (push_positions[0], pop_positions[0]);
    if expected_blur > 0.0 {
        assert!(
            scene.primitives[..push_idx].iter().any(|p| matches!(
                p,
                DrawPrimitive::StyledBox { backdrop_blur_px, opacity, .. }
                    if (*backdrop_blur_px - expected_blur).abs() < 1e-3 && (*opacity - 0.5).abs() < 1e-3
            )),
            "Expected pre-layer backdrop_blur_px={expected_blur} for {theme_name}"
        );
    } else {
        assert!(
            !scene.primitives.iter().any(|p| matches!(
                p,
                DrawPrimitive::StyledBox { backdrop_blur_px, .. } if *backdrop_blur_px > 0.0
            )),
            "Expected 0 backdrop blur for {theme_name}"
        );
    }
    if expect_shadow {
        assert!(
            scene.primitives[push_idx + 1..pop_idx]
                .iter()
                .any(|p| matches!(
                    p,
                    DrawPrimitive::StyledBox { box_shadow, opacity, .. }
                        if box_shadow.is_some() && (*opacity - 1.0).abs() < 1e-3
                )),
            "Expected in-layer card StyledBox with box_shadow for {theme_name}"
        );
    } else {
        assert!(
            !scene.primitives.iter().any(|p| matches!(
                p,
                DrawPrimitive::StyledBox { box_shadow, .. } if box_shadow.is_some()
            )),
            "Expected 0 box_shadow for {theme_name}"
        );
    }
}

fn verify_preset_scene(
    tree: &mut ViewTree,
    root_id: NodeId,
    theme_name: &str,
    expected_blur: f32,
    expect_shadow: bool,
) {
    tree.set_node_attr(root_id, "data-theme", theme_name);
    let (scene, _) = tree.build_paint_scene(200, 160);
    assert_preset_blur_and_shadow_primitives(&scene, theme_name, expected_blur, expect_shadow);

    let mut cpu = TinySkiaRenderer::new(200, 160, GpuResourceLimits::default());
    cpu.render_scene(&scene);
    assert_eq!(cpu.framebuffer_rgba().unwrap().len(), 200 * 160 * 4);
    if let Some(mut gpu) = WgpuRenderer::try_new(200, 160, GpuResourceLimits::default()) {
        let mut dark_scene = scene;
        dark_scene.clear_color = Color::BLACK;
        gpu.render_scene(&dark_scene);
        assert_eq!(gpu.framebuffer_rgba().unwrap().len(), 200 * 160 * 4);
    }
}

#[test]
fn css_tokens_toggle_backdrop_blur_and_box_shadow_and_containers_emit_push_pop_layer() {
    let mut tree = ViewTree::new();
    let frag = krui!(
        r#"
        <style>
        .root {
            width: 200px;
            height: 160px;
            padding: 16px;
        }
        [data-theme="fluent"] {
            --demo-blur: 16px;
            --demo-shadow: 0px 4px 12px 0px rgba(0, 0, 0, 0.25);
        }
        [data-theme="material"] {
            --demo-blur: 0px;
            --demo-shadow: 0px 2px 6px 0px rgba(0, 0, 0, 0.20);
        }
        [data-theme="glass"] {
            --demo-blur: 24px;
            --demo-shadow: 0px 6px 16px 0px rgba(0, 0, 0, 0.30);
        }
        [data-theme="cosmic"] {
            --demo-blur: 0px;
            --demo-shadow: none;
        }
        .glass-card {
            width: 140px;
            height: 90px;
            backdrop-filter: blur(var(--demo-blur));
            box-shadow: var(--demo-shadow);
            border-radius: 12px;
            opacity: 0.5;
        }
        .inner-box {
            width: 80px;
            height: 50px;
            background: #ffffff;
        }
        </style>
        <div id="root" class="root" data-theme="fluent">
            <div class="glass-card">
                <div class="inner-box"></div>
            </div>
        </div>
        "#
    );
    let _ = tree.mount(frag);
    let root_id = tree.find_by_id("root").expect("root");

    for (theme_name, expected_blur, expect_shadow) in [
        ("fluent", 16.0_f32, true),
        ("material", 0.0_f32, true),
        ("glass", 24.0_f32, true),
        ("cosmic", 0.0_f32, false),
    ] {
        verify_preset_scene(&mut tree, root_id, theme_name, expected_blur, expect_shadow);
    }
}
