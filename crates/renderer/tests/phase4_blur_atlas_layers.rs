#![forbid(unsafe_code)]

use krui_layout::LayoutRect;
use krui_renderer::{
    AdaptiveRenderer, CanvasContext2D, DrawPrimitive, GpuContext, GpuResourceLimits, PaintScene,
    RenderBackendPreference, RendererBackend, TinySkiaRenderer, WgpuRenderer,
};
use krui_style::{Affine2, Color};
use krui_text::RasterizedGlyph;
use std::sync::{Arc, Mutex};

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

fn build_checkerboard_blur_scene(blur_px: f32) -> PaintScene {
    let mut scene = PaintScene::new(128, 96, Color::BLACK);
    for gy in 0..12 {
        for gx in 0..16 {
            if (gx + gy) % 2 == 0 {
                scene.primitives.push(DrawPrimitive::StyledBox {
                    rect: LayoutRect {
                        x: (gx * 8) as f32,
                        y: (gy * 8) as f32,
                        width: 8.0,
                        height: 8.0,
                    },
                    clip: None,
                    clip_radius: 0.0,
                    clip_radii: [0.0; 4],
                    transform: Affine2::IDENTITY,
                    opacity: 1.0,
                    background_color: Color::WHITE,
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
        }
    }
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: LayoutRect {
            x: 24.0,
            y: 16.0,
            width: 80.0,
            height: 64.0,
        },
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        background_color: Color {
            r: 255,
            g: 255,
            b: 255,
            a: 32,
        },
        background_gradient: None,
        radial_gradient: None,
        border_color: Color::TRANSPARENT,
        border_width: 0.0,
        border_radius: 14.0,
        corner_radii: [0.0; 4],
        box_shadow: None,
        backdrop_blur_px: blur_px,
    });
    scene
}

fn region_variance(fb: &[u8], w: u32, x0: u32, y0: u32, x1: u32, y1: u32) -> f64 {
    let mut vals = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            vals.push(px_at(fb, w, x, y)[0] as f64);
        }
    }
    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
    vals.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / vals.len() as f64
}

#[test]
fn dual_kawase_backdrop_blur_dramatically_reduces_checkerboard_variance() {
    let flat_scene = build_checkerboard_blur_scene(0.0);
    let blur_scene = build_checkerboard_blur_scene(20.0);

    let mut cpu = TinySkiaRenderer::new(128, 96, GpuResourceLimits::default());
    cpu.render_scene(&flat_scene);
    let cpu_var_flat = region_variance(cpu.framebuffer_rgba().unwrap(), 128, 36, 28, 92, 68);
    cpu.render_scene(&blur_scene);
    let cpu_fb_blur = cpu.framebuffer_rgba().unwrap();
    let cpu_var_blur = region_variance(cpu_fb_blur, 128, 36, 28, 92, 68);
    assert!(cpu_var_flat > 8_000.0 && cpu_var_blur < cpu_var_flat * 0.08);
    assert_eq!(px_at(cpu_fb_blur, 128, 4, 4)[0], 255);
    assert_eq!(px_at(cpu_fb_blur, 128, 12, 4)[0], 0);

    let _guard = GPU_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut gpu) = WgpuRenderer::try_new(128, 96, GpuResourceLimits::default()) {
        gpu.render_scene(&flat_scene);
        let gpu_var_flat = region_variance(gpu.framebuffer_rgba().unwrap(), 128, 36, 28, 92, 68);
        gpu.render_scene(&blur_scene);
        let gpu_fb_blur = gpu.framebuffer_rgba().unwrap();
        let gpu_var_blur = region_variance(gpu_fb_blur, 128, 36, 28, 92, 68);
        assert!(
            gpu_var_flat > 8_000.0 && gpu_var_blur < gpu_var_flat * 0.08,
            "WGPU Dual-Kawase blur variance {gpu_var_blur} must be << flat {gpu_var_flat}"
        );
        assert_eq!(px_at(gpu_fb_blur, 128, 4, 4)[0], 255);
        assert_eq!(px_at(gpu_fb_blur, 128, 12, 4)[0], 0);
    }
}

fn make_color_emoji(key: u64, x: f32, rgba: [u8; 4]) -> RasterizedGlyph {
    RasterizedGlyph {
        cache_key: key,
        x,
        y: 0.0,
        width: 4,
        height: 4,
        alpha_mask: Arc::from(vec![255_u8; 16]),
        color_rgba: Some(Arc::from(rgba.repeat(16))),
        is_notdef: false,
        is_color: true,
        span_index: 0,
        char_start: 0,
        char_end: 1,
        glyph_color: None,
    }
}

#[test]
fn color_emoji_and_multi_page_atlas_eviction_never_drop_a_single_glyph() {
    let mut emoji_scene = PaintScene::new(80, 32, Color::BLACK);
    let span_g = RasterizedGlyph {
        cache_key: 9004,
        x: 48.0,
        y: 0.0,
        width: 4,
        height: 4,
        alpha_mask: Arc::from(vec![255_u8; 16]),
        color_rgba: None,
        is_notdef: false,
        is_color: false,
        span_index: 1,
        char_start: 1,
        char_end: 2,
        glyph_color: Some(Color {
            r: 240,
            g: 200,
            b: 30,
            a: 255,
        }),
    };
    emoji_scene.primitives.push(DrawPrimitive::TextGlyphs {
        origin_x: 8.0,
        origin_y: 8.0,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        color: Color::WHITE,
        glyphs: Arc::from(vec![
            make_color_emoji(9001, 0.0, [240, 30, 40, 255]),
            make_color_emoji(9002, 16.0, [30, 230, 60, 255]),
            make_color_emoji(9003, 32.0, [40, 90, 250, 255]),
            span_g,
        ]),
    });
    render_on_both_backends(&emoji_scene, |label, fb, w, _| {
        assert_eq!(px_at(fb, w, 9, 9), [240, 30, 40, 255], "[{label}] red");
        assert_eq!(px_at(fb, w, 25, 9), [30, 230, 60, 255], "[{label}] green");
        assert_eq!(px_at(fb, w, 41, 9), [40, 90, 250, 255], "[{label}] blue");
        assert_eq!(px_at(fb, w, 57, 9), [240, 200, 30, 255], "[{label}] span");
    });

    verify_atlas_stress_zero_dropped_glyphs();
}

fn verify_atlas_stress_zero_dropped_glyphs() {
    let (glyph_count, cols) = (1_450_usize, 50_usize);
    let mut glyphs = Vec::with_capacity(glyph_count);
    for idx in 0..glyph_count {
        let mut mask = vec![0_u8; 64 * 64];
        mask[0] = 255;
        mask[1] = 255;
        mask[64] = 255;
        mask[65] = 255;
        mask[128 + (idx % 60)] = ((idx % 250) + 1) as u8;
        glyphs.push(RasterizedGlyph {
            cache_key: 50_000 + idx as u64,
            x: ((idx % cols) * 4) as f32,
            y: ((idx / cols) * 4) as f32,
            width: 64,
            height: 64,
            alpha_mask: Arc::from(mask),
            color_rgba: None,
            is_notdef: false,
            is_color: false,
            span_index: 0,
            char_start: 0,
            char_end: 1,
            glyph_color: None,
        });
    }
    let mut scene = PaintScene::new(280, 200, Color::BLACK);
    scene.primitives.push(DrawPrimitive::TextGlyphs {
        origin_x: 4.0,
        origin_y: 4.0,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        color: Color::WHITE,
        glyphs: Arc::from(glyphs),
    });

    render_on_both_backends(&scene, |label, fb, w, _| {
        for idx in 0..glyph_count {
            let px = 4 + ((idx % cols) * 4) as u32;
            let py = 4 + ((idx / cols) * 4) as u32;
            assert_eq!(
                px_at(fb, w, px, py),
                [255, 255, 255, 255],
                "[{label}] dropped glyph #{idx} at ({px}, {py})"
            );
        }
    });
}

#[test]
fn group_opacity_push_pop_layer_eliminates_double_alpha_bleed_and_shares_gpu_context() {
    let mut scene = PaintScene::new(160, 160, Color::BLACK);
    let bounds = LayoutRect {
        x: 20.0,
        y: 20.0,
        width: 120.0,
        height: 120.0,
    };
    scene.primitives.push(DrawPrimitive::PushLayer {
        bounds,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        local_clip: None,
        opacity: 0.5,
    });
    for (x, y) in [(20.0, 20.0), (50.0, 50.0)] {
        scene.primitives.push(DrawPrimitive::StyledBox {
            rect: LayoutRect {
                x,
                y,
                width: 80.0,
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
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            border_radius: 0.0,
            corner_radii: [0.0; 4],
            box_shadow: None,
            backdrop_blur_px: 0.0,
        });
    }
    scene.primitives.push(DrawPrimitive::PopLayer);

    render_on_both_backends(&scene, |label, fb, w, _| {
        let parent_only = px_at(fb, w, 35, 35)[0];
        let overlap = px_at(fb, w, 65, 65)[0];
        let child_only = px_at(fb, w, 115, 115)[0];
        for (region, v) in [
            ("parent", parent_only),
            ("overlap", overlap),
            ("child", child_only),
        ] {
            assert!(
                (125..=130).contains(&v),
                "[{label}] {region} expected ~128 uniform 0.5 alpha, got {v}"
            );
        }
        assert!((overlap as i32 - parent_only as i32).abs() <= 1);
    });

    verify_shared_gpu_context_and_canvas_cache();
}

fn verify_shared_gpu_context_and_canvas_cache() {
    let _guard = GPU_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(ctx) = GpuContext::try_new() else {
        return;
    };
    let mut r1 = WgpuRenderer::from_context(
        Arc::clone(&ctx),
        wgpu::TextureFormat::Rgba8Unorm,
        64,
        64,
        GpuResourceLimits::default(),
    );
    assert!(Arc::ptr_eq(&ctx, r1.shared_context()));
    let adaptive = AdaptiveRenderer::new(
        64,
        64,
        RenderBackendPreference::Auto,
        GpuResourceLimits::default(),
    );
    assert!(adaptive.gpu_context().is_some());

    let mut c2d = CanvasContext2D::new();
    c2d.fill_circle(32.0, 32.0, 16.0, Color::WHITE);
    let cmds: Arc<[_]> = Arc::from(c2d.commands().to_vec());
    let mut canvas_scene = PaintScene::new(64, 64, Color::BLACK);
    canvas_scene.primitives.push(DrawPrimitive::CanvasBatch {
        viewport: LayoutRect {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 64.0,
        },
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        commands: cmds,
    });
    r1.render_scene(&canvas_scene);
    r1.render_scene(&canvas_scene);
    r1.render_scene(&canvas_scene);
    assert_eq!(r1.canvas_cache_stats(), (2, 1));
}
