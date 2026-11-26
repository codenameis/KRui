//! Regression tests proving CSS `border-radius` + `overflow: hidden` clips `::before`
//! radial ripple animations strictly inside the rounded pill silhouette on CPU and GPU,
//! even inside rounded `overflow-y: scroll` containers (both Fast-Path and corner-overlap).

#![forbid(unsafe_code)]

use krui::{
    krui, GpuResourceLimits, PaintScene, RendererBackend, TinySkiaRenderer, ViewTree, WgpuRenderer,
};

fn render_both_backends(scene: &PaintScene, w: u32, h: u32, check: impl Fn(&str, &[u8])) {
    let mut cpu = TinySkiaRenderer::new(w, h, GpuResourceLimits::default());
    let _ = cpu.render_scene(scene);
    check("TinySkia", cpu.framebuffer_rgba().expect("cpu fb"));

    if let Some(mut gpu) = WgpuRenderer::try_new(w, h, GpuResourceLimits::default()) {
        let _ = gpu.render_scene(scene);
        check("Wgpu", gpu.readback_framebuffer().expect("gpu fb"));
    }
}

fn px(fb: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [fb[i], fb[i + 1], fb[i + 2], fb[i + 3]]
}

fn assert_green(tag: &str, label: &str, px_rgba: [u8; 4]) {
    assert!(
        px_rgba[1] > 235 && px_rgba[0] < 20 && px_rgba[2] < 20,
        "[{tag}] {label} leaked past round border-radius: {px_rgba:?}"
    );
}

fn build_ripple_test_scene() -> (PaintScene, krui::LayoutRect, krui::LayoutRect) {
    let mut tree = ViewTree::new();
    krui!(
        r#"
        <style>
            .scroller {
                position: relative;
                width: 240px;
                height: 200px;
                border-radius: 28px;
                overflow-y: scroll;
                background-color: #00ff00;
            }
            .pill-btn {
                position: relative;
                overflow: hidden;
                width: 140px;
                height: 44px;
                border-radius: 999px;
                background-color: #102030;
                color: #ff00ff;
            }
            .pill-btn::before {
                content: "";
                position: absolute;
                inset: 0px;
                opacity: 0.6;
                background-image: radial-gradient(circle at 50% 50%, currentColor 60%, transparent 100%);
            }
            .center-pos { left: 40px; top: 80px; }
            .corner-pos { position: absolute; left: 0px; top: 0px; }
        </style>
        <div class="scroller">
            <button id="btn-center" class="pill-btn center-pos">Center</button>
            <button id="btn-corner" class="pill-btn corner-pos">Corner</button>
        </div>
        "#
    )
    .mount(&mut tree, None);

    let (scene, _) = tree.build_paint_scene(240, 200);
    let c_rect = tree
        .node_layout(tree.find_by_id("btn-center").unwrap())
        .unwrap();
    let k_rect = tree
        .node_layout(tree.find_by_id("btn-corner").unwrap())
        .unwrap();
    (scene, c_rect, k_rect)
}

#[test]
fn button_ripple_before_clips_to_pill_border_radius_inside_rounded_scroller() {
    let (scene, c_rect, k_rect) = build_ripple_test_scene();

    render_both_backends(&scene, 240, 200, |tag, fb| {
        assert_green(
            tag,
            "centered pill corner",
            px(fb, 240, c_rect.x as u32 + 2, c_rect.y as u32 + 2),
        );
        let c_mid = px(
            fb,
            240,
            (c_rect.x + c_rect.width * 0.5) as u32,
            (c_rect.y + c_rect.height * 0.5) as u32,
        );
        assert!(
            c_mid[0] > 100 && c_mid[2] > 100,
            "[{tag}] centered ripple missing at center: {c_mid:?}"
        );

        let k_top_left = px(fb, 240, k_rect.x as u32 + 3, k_rect.y as u32 + 3);
        assert!(
            k_top_left[0] > 240 && k_top_left[1] > 240 && k_top_left[2] > 240,
            "[{tag}] corner button leaked past outer scroller rounded clip: {k_top_left:?}"
        );
        assert_green(
            tag,
            "corner button bottom-left",
            px(fb, 240, k_rect.x as u32 + 2, k_rect.y as u32 + 42),
        );
        assert_green(
            tag,
            "corner button top-right",
            px(fb, 240, k_rect.x as u32 + 138, k_rect.y as u32 + 2),
        );
        let k_mid = px(
            fb,
            240,
            (k_rect.x + k_rect.width * 0.5) as u32,
            (k_rect.y + k_rect.height * 0.5) as u32,
        );
        assert!(
            k_mid[0] > 100 && k_mid[2] > 100,
            "[{tag}] corner ripple missing at center: {k_mid:?}"
        );
    });
}
