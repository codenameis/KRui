//! Wave 4 renderer parity tests covering multi-stop gradients, `conic-gradient()`,
//! arbitrary `clip-path`, and non-affine CSS 3D transforms (`perspective`, `preserve-3d`,
//! `backface-visibility`) across CPU (`TinySkiaRenderer`) and GPU (`WgpuRenderer`).

#![forbid(unsafe_code)]

use krui::{
    controller, krui, GpuResourceLimits, PaintScene, RendererBackend, Signal, TinySkiaRenderer,
    ViewTree, WgpuRenderer,
};

#[controller]
#[derive(Clone)]
struct HitCounter {
    hits_a: Signal<i32>,
    hits_b: Signal<i32>,
}

impl HitCounter {
    fn new() -> Self {
        Self {
            hits_a: Signal::new(0),
            hits_b: Signal::new(0),
        }
    }

    pub fn hit_a(&self) {
        self.hits_a.update(|v| *v += 1);
    }

    pub fn hit_b(&self) {
        self.hits_b.update(|v| *v += 1);
    }
}

fn render_both(scene: &PaintScene, w: u32, h: u32, verify: impl Fn(&str, &[u8])) {
    let mut cpu = TinySkiaRenderer::new(w, h, GpuResourceLimits::default());
    let _ = cpu.render_scene(scene);
    verify("TinySkia", cpu.framebuffer_rgba().expect("cpu framebuffer"));

    if let Some(mut gpu) = WgpuRenderer::try_new(w, h, GpuResourceLimits::default()) {
        let _ = gpu.render_scene(scene);
        verify("Wgpu", gpu.readback_framebuffer().expect("gpu framebuffer"));
    }
}

fn px(fb: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [fb[i], fb[i + 1], fb[i + 2], fb[i + 3]]
}

fn assert_rgb_near(tag: &str, label: &str, got: [u8; 4], want: [u8; 3], tol: i32) {
    let dr = (got[0] as i32 - want[0] as i32).abs();
    let dg = (got[1] as i32 - want[1] as i32).abs();
    let db = (got[2] as i32 - want[2] as i32).abs();
    assert!(
        dr <= tol && dg <= tol && db <= tol && got[3] > 230,
        "[{tag}] {label}: expected ~{want:?}, got {got:?}"
    );
}

#[test]
fn multi_stop_linear_and_radial_gradients_cpu_and_gpu_parity() {
    let mut tree = ViewTree::new();
    krui!(
        r#"
        <style>
            .root { position: relative; width: 240px; height: 180px; background-color: #ffffff; }
            .lin5 {
                position: absolute; left: 0px; top: 0px; width: 200px; height: 40px;
                background-image: linear-gradient(90deg, #ff0000 0%, #00ff00 25%, #0000ff 50%, #ffff00 75%, #ff00ff 100%);
            }
            .rad4 {
                position: absolute; left: 0px; top: 50px; width: 120px; height: 120px;
                background-image: radial-gradient(circle 60px at 60px 60px, #ff0000 0%, #00ff00 35%, #0000ff 70%, #ffff00 100%);
            }
        </style>
        <div class="root"><div class="lin5"></div><div class="rad4"></div></div>
        "#
    )
    .mount(&mut tree, None);

    let (scene, _) = tree.build_paint_scene(240, 180);
    render_both(&scene, 240, 180, |tag, fb| {
        assert_rgb_near(
            tag,
            "lin stop 2 (green)",
            px(fb, 240, 50, 20),
            [0, 255, 0],
            35,
        );
        assert_rgb_near(
            tag,
            "lin stop 3 (blue)",
            px(fb, 240, 100, 20),
            [0, 0, 255],
            35,
        );
        assert_rgb_near(
            tag,
            "lin stop 4 (yellow)",
            px(fb, 240, 150, 20),
            [255, 255, 0],
            35,
        );
        assert_rgb_near(
            tag,
            "lin stop 5 (magenta)",
            px(fb, 240, 198, 20),
            [255, 0, 255],
            35,
        );
        assert_rgb_near(
            tag,
            "rad center (red)",
            px(fb, 240, 60, 110),
            [255, 0, 0],
            35,
        );
        assert_rgb_near(
            tag,
            "rad stop 2 (green)",
            px(fb, 240, 81, 131),
            [0, 255, 0],
            45,
        );
        assert_rgb_near(
            tag,
            "rad stop 3 (blue)",
            px(fb, 240, 102, 152),
            [0, 0, 255],
            45,
        );
        assert_rgb_near(
            tag,
            "rad stop 4 (yellow)",
            px(fb, 240, 118, 168),
            [255, 255, 0],
            45,
        );
    });
}

#[test]
fn conic_gradient_four_quadrants_cpu_and_gpu_parity() {
    let mut tree = ViewTree::new();
    krui!(
        r#"
        <style>
            .conic-box {
                width: 120px; height: 120px;
                background-image: conic-gradient(from 0deg at 50% 50%,
                    #ff0000 0deg 90deg, #00ff00 90deg 180deg, #0000ff 180deg 270deg, #ffff00 270deg 360deg);
            }
        </style>
        <div class="conic-box"></div>
        "#
    )
    .mount(&mut tree, None);

    let (scene, _) = tree.build_paint_scene(120, 120);
    render_both(&scene, 120, 120, |tag, fb| {
        assert_rgb_near(
            tag,
            "top-right (0..90deg)",
            px(fb, 120, 90, 30),
            [255, 0, 0],
            25,
        );
        assert_rgb_near(
            tag,
            "bottom-right (90..180deg)",
            px(fb, 120, 90, 90),
            [0, 255, 0],
            25,
        );
        assert_rgb_near(
            tag,
            "bottom-left (180..270deg)",
            px(fb, 120, 30, 90),
            [0, 0, 255],
            25,
        );
        assert_rgb_near(
            tag,
            "top-left (270..360deg)",
            px(fb, 120, 30, 30),
            [255, 255, 0],
            25,
        );
    });
}

#[test]
fn clip_path_polygon_and_path_clips_pixels_and_hit_testing() {
    let mut tree = ViewTree::new();
    let controller = HitCounter::new();
    let hits_a = controller.hits_a;
    krui!(
        r#"
        <style>
            .root { position: relative; width: 240px; height: 120px; background-color: #ffffff; }
            .tri {
                position: absolute; left: 10px; top: 10px; width: 100px; height: 100px;
                background-color: #ff0000;
                clip-path: polygon(50% 0%, 100% 100%, 0% 100%);
            }
            .path-half {
                position: absolute; left: 120px; top: 10px; width: 100px; height: 100px;
                background-color: #0000ff;
                clip-path: path("M 0 0 L 100 0 L 0 100 Z");
            }
        </style>
        <div class="root">
            <button id="tri-btn" class="tri" onclick="{controller.hit_a()}"></button>
            <div class="path-half"></div>
        </div>
        "#
    )
    .mount(&mut tree, None);

    let (scene, _) = tree.build_paint_scene(240, 120);
    let _ = tree.dispatch_pointer_down(15.0, 15.0);
    let _ = tree.dispatch_pointer_up_at(15.0, 15.0);
    assert_eq!(hits_a.get_untracked(), 0, "clipped corner must miss");
    assert!(tree.dispatch_pointer_down(60.0, 85.0));
    assert!(tree.dispatch_pointer_up_at(60.0, 85.0));
    assert_eq!(hits_a.get_untracked(), 1, "inside polygon must hit");

    render_both(&scene, 240, 120, |tag, fb| {
        assert_rgb_near(
            tag,
            "tri outside corner",
            px(fb, 240, 15, 15),
            [255, 255, 255],
            15,
        );
        assert_rgb_near(tag, "tri inside body", px(fb, 240, 60, 85), [255, 0, 0], 20);
        assert_rgb_near(
            tag,
            "path inside upper-left",
            px(fb, 240, 140, 25),
            [0, 0, 255],
            20,
        );
        assert_rgb_near(
            tag,
            "path clipped lower-right",
            px(fb, 240, 205, 95),
            [255, 255, 255],
            15,
        );
    });
}

fn vertical_red_span(fb: &[u8], w: u32, h: u32, col_x: u32) -> u32 {
    let (mut top, mut bot) = (None, 0);
    for y in 0..h {
        let p = px(fb, w, col_x, y);
        if p[0] > 200 && p[1] < 80 && p[2] < 80 {
            top.get_or_insert(y);
            bot = y;
        }
    }
    top.map_or(0, |t| bot + 1 - t)
}

#[test]
fn perspective_trapezoid_and_backface_visibility_hidden() {
    let mut tree = ViewTree::new();
    let controller = HitCounter::new();
    let (hits_a, hits_b) = (controller.hits_a, controller.hits_b);
    krui!(
        r#"
        <style>
            .root { position: relative; width: 240px; height: 160px; background-color: #ffffff; }
            .trapezoid {
                position: absolute; left: 40px; top: 30px; width: 100px; height: 100px;
                background-color: #ff0000;
                transform: perspective(300px) rotateY(45deg);
                backface-visibility: hidden;
            }
            .backside {
                position: absolute; left: 150px; top: 30px; width: 70px; height: 70px;
                background-color: #00ff00;
                transform: perspective(300px) rotateY(180deg);
                backface-visibility: hidden;
            }
        </style>
        <div class="root">
            <button id="persp-card" class="trapezoid" onclick="{controller.hit_a()}"></button>
            <button id="backface-card" class="backside" onclick="{controller.hit_b()}"></button>
        </div>
        "#
    )
    .mount(&mut tree, None);

    let (scene, _) = tree.build_paint_scene(240, 160);
    assert!(tree.dispatch_pointer_down(90.0, 80.0));
    assert!(tree.dispatch_pointer_up_at(90.0, 80.0));
    assert_eq!(hits_a.get_untracked(), 1, "front 3D card should hit");
    let _ = tree.dispatch_pointer_down(185.0, 65.0);
    let _ = tree.dispatch_pointer_up_at(185.0, 65.0);
    assert_eq!(hits_b.get_untracked(), 0, "culled backface must miss");

    render_both(&scene, 240, 160, |tag, fb| {
        let left_h = vertical_red_span(fb, 240, 160, 62);
        let right_h = vertical_red_span(fb, 240, 160, 118);
        assert!(
            left_h > right_h + 6,
            "[{tag}] trapezoid left_h={left_h} vs right_h={right_h}"
        );
        assert_rgb_near(
            tag,
            "culled backface center",
            px(fb, 240, 185, 65),
            [255, 255, 255],
            10,
        );
    });
}

#[test]
fn preserve_3d_sorts_closer_translate_z_in_front_of_later_dom_child() {
    let mut tree = ViewTree::new();
    let controller = HitCounter::new();
    let (hits_a, hits_b) = (controller.hits_a, controller.hits_b);
    krui!(
        r#"
        <style>
            .root { position: relative; width: 160px; height: 160px; background-color: #ffffff; }
            .stage {
                position: relative; width: 160px; height: 160px;
                perspective: 400px; transform-style: preserve-3d;
            }
            .front-first {
                position: absolute; left: 30px; top: 30px; width: 100px; height: 100px;
                background-color: #ff0000; transform: translateZ(40px);
            }
            .back-second {
                position: absolute; left: 30px; top: 30px; width: 100px; height: 100px;
                background-color: #0000ff; transform: translateZ(-40px);
            }
        </style>
        <div class="root">
            <div class="stage">
                <button id="front-z" class="front-first" onclick="{controller.hit_a()}"></button>
                <button id="back-z" class="back-second" onclick="{controller.hit_b()}"></button>
            </div>
        </div>
        "#
    )
    .mount(&mut tree, None);

    let (scene, _) = tree.build_paint_scene(160, 160);
    assert!(tree.dispatch_pointer_down(80.0, 80.0));
    assert!(tree.dispatch_pointer_up_at(80.0, 80.0));
    assert_eq!(hits_a.get_untracked(), 1, "closer +Z node must hit first");
    assert_eq!(hits_b.get_untracked(), 0);

    render_both(&scene, 160, 160, |tag, fb| {
        assert_rgb_near(
            tag,
            "preserve-3d +Z covers -Z",
            px(fb, 160, 80, 80),
            [255, 0, 0],
            15,
        );
    });
}
