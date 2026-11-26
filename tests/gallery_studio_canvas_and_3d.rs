//! Integration tests for `component_gallery`'s shared `.obj` 3D model rendered via both
//! direct Raw `wgpu` (`RawWgpuViewport`) and Bevy 0.15 ECS (`BevyEditorViewportHost`).

#![forbid(unsafe_code)]
#![cfg(feature = "desktop")]

#[path = "../examples/component_gallery/viewport3d/mod.rs"]
mod viewport3d;

use krui::{krui, GpuContext, GpuResourceLimits, NodeId, RendererBackend, ViewTree, WgpuRenderer};
use viewport3d::{mesh_obj::load_shared_open_model, Gallery3dHost, Viewport3dFrameInput};

fn count_non_clear_pixels(fb: &[u8]) -> usize {
    if fb.len() < 4 {
        return 0;
    }
    let (bg_r, bg_g, bg_b) = (fb[0] as i32, fb[1] as i32, fb[2] as i32);
    fb.as_chunks::<4>()
        .0
        .iter()
        .filter(|px| {
            (px[0] as i32 - bg_r).abs() > 12
                || (px[1] as i32 - bg_g).abs() > 12
                || (px[2] as i32 - bg_b).abs() > 12
        })
        .count()
}

fn render_3d_mode_frame(
    host: &mut Gallery3dHost,
    tree: &mut ViewTree,
    renderer: &mut WgpuRenderer,
    vp_node: NodeId,
    mode: i32,
    yaw_deg: f32,
) -> (Vec<u8>, String) {
    let _ = host.ensure_bound(tree, vp_node, mode);
    let label = host.sync_and_render(
        tree,
        vp_node,
        1.0,
        Viewport3dFrameInput {
            engine_mode: mode,
            yaw_deg,
            pitch_deg: 18.0,
            distance: 3.0,
            model_scale: 1.0,
        },
    );
    let (scene, _) = tree.build_paint_scene(320, 240);
    let _ = renderer.render_scene(&scene);
    let fb = renderer.readback_framebuffer().expect("3D frame readback");
    (fb.to_vec(), label)
}

#[test]
fn shared_open_teapot_obj_parses_with_valid_normals_and_indices() {
    let mesh = load_shared_open_model();
    assert!(mesh.vertices.len() >= 1000, "verts={}", mesh.vertices.len());
    assert!(
        mesh.indices.len() / 3 >= 1500,
        "tris={}",
        mesh.indices.len() / 3
    );
    for &idx in &mesh.indices {
        assert!((idx as usize) < mesh.vertices.len());
    }
    for v in &mesh.vertices {
        let len =
            (v.normal[0] * v.normal[0] + v.normal[1] * v.normal[1] + v.normal[2] * v.normal[2])
                .sqrt();
        assert!((len - 1.0).abs() < 1e-3, "non-unit normal length={len}");
    }
}

#[test]
fn raw_wgpu_and_bevy_ecs_render_same_open_model_and_respond_to_orbit_drag() {
    let Some(gpu) = GpuContext::try_new() else {
        return;
    };
    let mut tree = ViewTree::new();
    krui!(r#"<style>#slot { width: 320px; height: 240px; }</style><div id="slot" />"#)
        .mount(&mut tree, None);
    let vp_node = tree.find_by_id("slot").expect("slot node");
    let _ = tree.build_paint_scene(320, 240);

    let mut host = Gallery3dHost::new(gpu.clone());
    assert_eq!(host.active_physical_size(0), (640, 400));
    assert_eq!(host.active_physical_size(1), (640, 400));
    assert!(host.mode_summary(0).contains("open_teapot.obj"));
    assert!(host.mode_summary(1).contains("Bevy 0.15 ECS"));

    let mut renderer = WgpuRenderer::from_context(
        gpu,
        wgpu::TextureFormat::Rgba8Unorm,
        320,
        240,
        GpuResourceLimits::default(),
    );

    let (wgpu_a, label_a) =
        render_3d_mode_frame(&mut host, &mut tree, &mut renderer, vp_node, 0, 35.0);
    let (wgpu_b, _) = render_3d_mode_frame(&mut host, &mut tree, &mut renderer, vp_node, 0, 100.0);
    assert!(label_a.contains("Raw wgpu Direct") && count_non_clear_pixels(&wgpu_a) > 1500);
    assert_ne!(
        wgpu_a, wgpu_b,
        "orbiting in Raw wgpu must change rendered pixels"
    );

    let (bevy_a, label_b) =
        render_3d_mode_frame(&mut host, &mut tree, &mut renderer, vp_node, 1, 35.0);
    let (bevy_b, _) = render_3d_mode_frame(&mut host, &mut tree, &mut renderer, vp_node, 1, 100.0);
    assert!(label_b.contains("Bevy 0.15 ECS") && count_non_clear_pixels(&bevy_a) > 1500);
    assert_ne!(
        bevy_a, bevy_b,
        "orbiting in Bevy ECS must change rendered pixels"
    );
    assert_ne!(
        wgpu_b, bevy_b,
        "Raw wgpu and Bevy ECS backends must render distinct frames"
    );

    let snap = host.bevy_entity_snapshot().expect("Bevy entity snapshot");
    assert!((snap.transform.rotation_deg[0] - 18.0).abs() < 1e-3);
    assert!((snap.transform.rotation_deg[1] - 100.0).abs() < 1e-3);
}
