#![forbid(unsafe_code)]

use krui_layout::LayoutRect;
use krui_renderer::{
    DrawPrimitive, GpuResourceLimits, PaintScene, RendererBackend, TinySkiaRenderer, WgpuRenderer,
};
use krui_style::{Affine2, BoxShadow, Color, LinearGradient};
use std::sync::Mutex;

static GPU_TEST_LOCK: Mutex<()> = Mutex::new(());

fn px_at(fb: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * width + x) * 4) as usize;
    [fb[idx], fb[idx + 1], fb[idx + 2], fb[idx + 3]]
}

fn render_on_both_backends(scene: &PaintScene, mut verify: impl FnMut(&str, &[u8], u32, u32)) {
    let mut cpu = TinySkiaRenderer::new(scene.width, scene.height, GpuResourceLimits::default());
    cpu.render_scene(scene);
    let (cw, ch) = (cpu.physical_width(), cpu.physical_height());
    let cpu_fb = cpu.framebuffer_rgba().expect("cpu framebuffer").to_vec();
    verify("TinySkia", &cpu_fb, cw, ch);

    let _guard = GPU_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut gpu) =
        WgpuRenderer::try_new(scene.width, scene.height, GpuResourceLimits::default())
    {
        gpu.render_scene(scene);
        let (gw, gh) = gpu.target_size();
        let gpu_fb = gpu.framebuffer_rgba().expect("wgpu framebuffer").to_vec();
        verify("Wgpu", &gpu_fb, gw, gh);
    }
}

fn assert_rounded_box_pixels(label: &str, fb: &[u8], w: u32, h: u32) {
    for y in 0..h {
        for x in 0..w {
            if !(20..120).contains(&x) || !(20..100).contains(&y) {
                assert_eq!(
                    px_at(fb, w, x, y),
                    [0, 0, 0, 255],
                    "[{label}] outward border bleed at ({x}, {y})"
                );
            }
        }
    }
    assert_eq!(px_at(fb, w, 20, 20), [0, 0, 0, 255], "[{label}] corner");
    let border_px = px_at(fb, w, 22, 60);
    assert!(
        border_px[0] > 220 && border_px[1] < 60,
        "[{label}] inset border {border_px:?}"
    );
    assert_eq!(px_at(fb, w, 70, 60), [255, 255, 255, 255], "[{label}] fill");
    let mut fractional_aa_count = 0_usize;
    for y in 20..38 {
        for x in 20..38 {
            if (12..=228).contains(&px_at(fb, w, x, y)[0]) {
                fractional_aa_count += 1;
            }
        }
    }
    assert!(
        fractional_aa_count >= 4,
        "[{label}] expected smooth AA fractional pixels on rounded arc, got {fractional_aa_count}"
    );
}

#[test]
fn rounded_corners_are_antialiased_and_borders_stay_strictly_inset() {
    let mut scene = PaintScene::new(160, 120, Color::BLACK);
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: LayoutRect {
            x: 20.0,
            y: 20.0,
            width: 100.0,
            height: 80.0,
        },
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        background_color: Color::WHITE,
        background_gradient: None,
        radial_gradient: None,
        border_color: Color {
            r: 240,
            g: 40,
            b: 40,
            a: 255,
        },
        border_width: 6.0,
        border_radius: 18.0,
        corner_radii: [0.0; 4],
        box_shadow: None,
        backdrop_blur_px: 0.0,
    });
    render_on_both_backends(&scene, assert_rounded_box_pixels);
}

#[test]
fn rounded_clip_and_cached_tiny_skia_mask_work_without_per_primitive_alloc() {
    let clip_rect = Some(LayoutRect {
        x: 20.0,
        y: 20.0,
        width: 80.0,
        height: 80.0,
    });
    let mut scene = PaintScene::new(120, 120, Color::BLACK);
    for _ in 0..3 {
        scene.primitives.push(DrawPrimitive::StyledBox {
            rect: LayoutRect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 120.0,
            },
            clip: clip_rect,
            clip_radius: 24.0,
            clip_radii: [0.0; 4],
            transform: Affine2::IDENTITY,
            opacity: 1.0,
            background_color: Color {
                r: 40,
                g: 200,
                b: 120,
                a: 255,
            },
            background_gradient: None,
            radial_gradient: None,
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            border_radius: 0.0,
            corner_radii: [0.0; 4],
            box_shadow: None,
            backdrop_blur_px: 0.0,
        });
    }

    let mut cpu = TinySkiaRenderer::new(120, 120, GpuResourceLimits::default());
    cpu.render_scene(&scene);
    let (hits, allocs) = cpu.clip_mask_stats();
    assert_eq!(allocs, 1, "Clip mask must be allocated once and cached");
    assert_eq!(hits, 2, "Subsequent primitives sharing clip must hit cache");

    render_on_both_backends(&scene, |label, fb, w, _| {
        assert_eq!(
            px_at(fb, w, 21, 21),
            [0, 0, 0, 255],
            "[{label}] rounded clip corner"
        );
        let center = px_at(fb, w, 60, 60);
        assert!(center[1] > 190, "[{label}] center unclipped {center:?}");
    });
}

const fn clr(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color { r, g, b, a }
}

fn build_shadow_and_gradient_scene() -> PaintScene {
    let mut scene = PaintScene::new(180, 150, Color::BLACK);
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: LayoutRect {
            x: 20.0,
            y: 15.0,
            width: 60.0,
            height: 50.0,
        },
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        background_color: clr(30, 30, 30, 255),
        background_gradient: Some(LinearGradient::two_stop(
            90.0,
            clr(250, 20, 20, 255),
            clr(20, 40, 250, 255),
        )),
        radial_gradient: None,
        border_color: Color::TRANSPARENT,
        border_width: 0.0,
        border_radius: 8.0,
        corner_radii: [0.0; 4],
        box_shadow: Some(BoxShadow {
            offset_x: 0.0,
            offset_y: 8.0,
            blur_radius: 12.0,
            spread_radius: 0.0,
            color: clr(0, 220, 255, 220),
            inset: false,
        }),
        backdrop_blur_px: 0.0,
    });
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: LayoutRect {
            x: 100.0,
            y: 15.0,
            width: 60.0,
            height: 60.0,
        },
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        background_color: Color::WHITE,
        background_gradient: None,
        radial_gradient: None,
        border_color: clr(20, 200, 60, 255),
        border_width: 4.0,
        border_radius: 10.0,
        corner_radii: [0.0; 4],
        box_shadow: Some(BoxShadow {
            offset_x: 0.0,
            offset_y: 6.0,
            blur_radius: 10.0,
            spread_radius: 2.0,
            color: clr(0, 0, 0, 220),
            inset: true,
        }),
        backdrop_blur_px: 0.0,
    });
    scene
}

#[test]
fn box_shadow_outer_and_inset_plus_linear_gradient_render_accurately() {
    let scene = build_shadow_and_gradient_scene();
    render_on_both_backends(&scene, |label, fb, w, _| {
        let left_g = px_at(fb, w, 24, 40);
        let mid_g = px_at(fb, w, 50, 40);
        let right_g = px_at(fb, w, 76, 40);
        assert!(
            left_g[0] > 210 && left_g[2] < 50,
            "[{label}] grad start {left_g:?}"
        );
        assert!(
            mid_g[0] > 90 && mid_g[2] > 90,
            "[{label}] grad mid {mid_g:?}"
        );
        assert!(
            right_g[2] > 210 && right_g[0] < 50,
            "[{label}] grad end {right_g:?}"
        );

        let sh_near = px_at(fb, w, 50, 68)[2];
        let sh_mid = px_at(fb, w, 50, 75)[2];
        let sh_far = px_at(fb, w, 50, 84)[2];
        assert!(
            sh_near > sh_mid && sh_mid > sh_far && sh_far > 0,
            "[{label}] outer shadow smooth falloff: {sh_near} > {sh_mid} > {sh_far}"
        );
        assert_eq!(
            px_at(fb, w, 50, 115)[2],
            0,
            "[{label}] shadow fades to zero"
        );

        assert_eq!(
            px_at(fb, w, 98, 45),
            [0, 0, 0, 255],
            "[{label}] inset shadow never bleeds out"
        );
        let inset_top = px_at(fb, w, 130, 21)[0];
        let inset_center = px_at(fb, w, 130, 52)[0];
        assert!(
            inset_top + 40 < inset_center,
            "[{label}] inset shadow darkens top inner edge ({inset_top} vs {inset_center})"
        );
    });
}
