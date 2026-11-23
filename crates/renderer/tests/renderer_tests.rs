#![forbid(unsafe_code)]

use krui_layout::LayoutRect;
use krui_renderer::{
    tessellate_scene_for_surface_with_scale_batched, tessellate_scene_to_mesh, ActiveBackendKind,
    AdaptiveRenderer, CanvasContext2D, DrawPrimitive, GlyphAtlasPacker, GpuResourceLimits,
    PaintScene, RenderBackendPreference, RendererBackend, TinySkiaRenderer, WgpuRenderer,
    ATLAS_SIZE,
};
use krui_style::{Affine2, Color};

#[test]
fn tiny_skia_fallback_initializes_without_theme_coupling() {
    let mut renderer = AdaptiveRenderer::new(
        320,
        240,
        RenderBackendPreference::ForceTinySkia,
        GpuResourceLimits::default(),
    );
    assert_eq!(
        renderer.backend_kind(),
        ActiveBackendKind::TinySkiaCpuFallback
    );
    let scene = PaintScene::new(320, 240, Color::WHITE);
    let _ = renderer.render_scene(&scene);
    assert_eq!(renderer.framebuffer_dimensions(), Some((320, 240)));
}

#[test]
fn canvas_2d_tessellates_and_rasterizes_identically_within_resource_bounds() {
    let mut ctx = CanvasContext2D::new();
    let blue = Color::parse_css("#3b82f6").expect("valid color");
    let yellow = Color::parse_css("#facc15").expect("valid color");
    let dark_bg = Color::parse_css("#0a0c10").expect("valid color");
    ctx.fill_round_rect(10.0, 10.0, 100.0, 60.0, 8.0, blue);
    ctx.fill_circle(60.0, 40.0, 20.0, yellow);
    ctx.begin_path();
    ctx.move_to(15.0, 15.0);
    ctx.line_to(95.0, 55.0);
    ctx.stroke_path(3.0, Color::WHITE);

    let mut scene = PaintScene::new(200, 120, dark_bg);
    scene.primitives.push(DrawPrimitive::CanvasBatch {
        viewport: LayoutRect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 120.0,
        },
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        commands: std::sync::Arc::from(ctx.commands().to_vec()),
    });

    let mut cpu = TinySkiaRenderer::new(200, 120, GpuResourceLimits::default());
    let stats = cpu.render_scene(&scene);
    assert_eq!(stats.draw_primitives_submitted, 1);
    assert_eq!(stats.draw_primitives_clamped, 0);
    assert!(stats.vertices_generated > 0);
    assert!(stats.indices_generated > 0);
    assert_eq!(cpu.framebuffer_rgba().unwrap().len(), 200 * 120 * 4);
}

#[test]
fn enforces_bounded_gpu_resource_clamping() {
    let strict_limits = GpuResourceLimits {
        max_viewport_dimension: 1024,
        max_draw_commands_per_frame: 2,
        max_vertices_per_frame: 1024,
        max_indices_per_frame: 3072,
    };
    let mut scene = PaintScene::new(256, 256, Color::BLACK);
    for _ in 0..5 {
        scene.primitives.push(DrawPrimitive::StyledBox {
            rect: LayoutRect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 40.0,
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

    let (_, _, clamped) = tessellate_scene_to_mesh(&scene, strict_limits);
    assert_eq!(clamped, 3);
}

#[test]
fn glyph_atlas_persists_cached_glyphs_and_defers_overflow_to_between_frames() {
    let mut packer = GlyphAtlasPacker::new();
    let mask = [255_u8; 64];
    let uv1 = packer
        .pack_glyph_by_key(101, 8, 8, &mask)
        .expect("pack first");
    let rows_after_first = packer.dirty_rows;
    assert_eq!(packer.cached_glyph_count(), 1);
    assert_eq!(packer.pixels.len(), (ATLAS_SIZE * ATLAS_SIZE) as usize);

    let uv2 = packer
        .pack_glyph_by_key(101, 8, 8, &mask)
        .expect("pack cached");
    assert_eq!(uv1, uv2);
    assert_eq!(packer.dirty_rows, rows_after_first);
    assert_eq!(packer.cached_glyph_count(), 1);

    let big_mask = vec![200_u8; (512 * 512) as usize];
    let mut key_id = 1000_u64;
    while !packer.overflow_pending {
        key_id += 1;
        let _ = packer.pack_glyph_by_key(key_id, 512, 512, &big_mask);
    }
    assert!(
        packer.cached_glyph_count() > 1,
        "Atlas must not wipe cached glyphs mid-frame when overflow occurs"
    );
    let epoch_before = packer.epoch;
    packer.begin_frame();
    assert_eq!(packer.epoch, epoch_before + 1);
    assert_eq!(packer.cached_glyph_count(), 0);
    assert!(!packer.overflow_pending);
}

fn sample_glyph(cache_key: u64, x: f32) -> krui_text::RasterizedGlyph {
    krui_text::RasterizedGlyph {
        cache_key,
        x,
        y: 0.0,
        width: 8,
        height: 8,
        alpha_mask: std::sync::Arc::from(vec![255_u8; 64]),
        color_rgba: None,
        is_notdef: false,
        is_color: false,
        span_index: 0,
        char_start: 0,
        char_end: 1,
        glyph_color: None,
    }
}

fn build_resource_test_scene() -> PaintScene {
    let mut scene = PaintScene::new(200, 200, Color::BLACK);
    let mut ctx = CanvasContext2D::new();
    ctx.fill_circle(50.0, 50.0, 30.0, Color::WHITE);
    ctx.begin_path();
    ctx.move_to(10.0, 10.0);
    ctx.line_to(80.0, 80.0);
    ctx.stroke_path(4.0, Color::WHITE);

    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: LayoutRect {
            x: 10.0,
            y: 10.0,
            width: 40.0,
            height: 40.0,
        },
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        background_color: Color::WHITE,
        background_gradient: None,
        radial_gradient: None,
        border_color: Color::parse_css("#ff0000").expect("valid color"),
        border_width: 2.0,
        border_radius: 6.0,
        corner_radii: [0.0; 4],
        box_shadow: None,
        backdrop_blur_px: 0.0,
    });
    scene.primitives.push(DrawPrimitive::CanvasBatch {
        viewport: LayoutRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        commands: std::sync::Arc::from(ctx.commands().to_vec()),
    });
    scene.primitives.push(DrawPrimitive::TextGlyphs {
        origin_x: 10.0,
        origin_y: 10.0,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        color: Color::WHITE,
        glyphs: std::sync::Arc::from(vec![sample_glyph(1, 0.0), sample_glyph(2, 10.0)]),
    });
    scene
}

#[test]
fn zero_and_small_gpu_resource_limits_never_panic_or_exceed_bounds() {
    let zero_limits = GpuResourceLimits {
        max_viewport_dimension: 0,
        max_draw_commands_per_frame: 0,
        max_vertices_per_frame: 0,
        max_indices_per_frame: 0,
    };
    assert_eq!(zero_limits.normalized().max_viewport_dimension, 1);

    let mut cpu = TinySkiaRenderer::new(200, 200, zero_limits);
    assert!(cpu.framebuffer_rgba().is_none());
    assert_eq!(cpu.physical_width(), 1);
    assert_eq!(cpu.physical_height(), 1);

    let empty_scene = PaintScene::new(200, 200, Color::BLACK);
    let stats0 = cpu.render_scene(&empty_scene);
    assert_eq!(stats0.vertices_generated, 0);
    assert_eq!(stats0.indices_generated, 0);
    assert_eq!(cpu.framebuffer_rgba().expect("rendered").len(), 4);

    let scene = build_resource_test_scene();
    for &(max_v, max_i) in &[(0, 0), (4, 6), (6, 4), (10, 12), (24, 36)] {
        let small_limits = GpuResourceLimits {
            max_viewport_dimension: 0,
            max_draw_commands_per_frame: 16,
            max_vertices_per_frame: max_v,
            max_indices_per_frame: max_i,
        };
        let (verts, indices, clamped) = tessellate_scene_to_mesh(&scene, small_limits);
        assert!(verts.len() <= max_v);
        assert!(indices.len() <= max_i);
        assert!(clamped >= 1);
    }
}

#[test]
fn hidpi_2x_physical_framebuffer_and_wgpu_staging_buffer_reuse() {
    let mut scene = PaintScene::new(200, 200, Color::BLACK);
    scene.scale_factor = 2.0;
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: LayoutRect {
            x: 100.0,
            y: 100.0,
            width: 80.0,
            height: 80.0,
        },
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        background_color: Color::parse_css("#ff0000").expect("valid color"),
        background_gradient: None,
        radial_gradient: None,
        border_color: Color::TRANSPARENT,
        border_width: 0.0,
        border_radius: 0.0,
        corner_radii: [0.0; 4],
        box_shadow: None,
        backdrop_blur_px: 0.0,
    });

    let mut cpu = TinySkiaRenderer::new(200, 200, GpuResourceLimits::default());
    cpu.render_scene(&scene);
    assert_eq!(cpu.physical_width(), 400);
    assert_eq!(cpu.physical_height(), 400);
    let fb = cpu.framebuffer_rgba().expect("rendered 2x framebuffer");
    assert_eq!(fb.len(), 400 * 400 * 4);
    let phys_idx = (300 * 400 + 300) * 4;
    assert_eq!(&fb[phys_idx..phys_idx + 3], &[255, 0, 0]);

    check_external_view_glyph_scale_invariance();
    check_wgpu_readback_staging_reuse(&scene);
}

fn check_external_view_glyph_scale_invariance() {
    let mut text_scene = PaintScene::new(200, 200, Color::BLACK);
    text_scene.scale_factor = 1.0;
    text_scene.primitives.push(DrawPrimitive::TextGlyphs {
        origin_x: 20.0,
        origin_y: 20.0,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        color: Color::WHITE,
        glyphs: std::sync::Arc::from(vec![krui_text::RasterizedGlyph {
            cache_key: 77,
            x: 0.0,
            y: 0.0,
            width: 8,
            height: 8,
            alpha_mask: std::sync::Arc::from(vec![255_u8; 64]),
            color_rgba: None,
            is_notdef: false,
            is_color: false,
            span_index: 0,
            char_start: 0,
            char_end: 1,
            glyph_color: None,
        }]),
    });
    let mut atlas = GlyphAtlasPacker::new();
    let (verts_1x, _, _, _) = tessellate_scene_for_surface_with_scale_batched(
        &text_scene,
        GpuResourceLimits::default(),
        &mut atlas,
        200,
        200,
        1.0,
    );
    let (verts_2x, _, _, _) = tessellate_scene_for_surface_with_scale_batched(
        &text_scene,
        GpuResourceLimits::default(),
        &mut atlas,
        400,
        400,
        2.0,
    );
    let ndc_w_1x = (verts_1x[1].position[0] - verts_1x[0].position[0]).abs();
    let ndc_w_2x = (verts_2x[1].position[0] - verts_2x[0].position[0]).abs();
    assert!((ndc_w_1x - ndc_w_2x).abs() < 1e-4);
}

fn check_wgpu_readback_staging_reuse(scene: &PaintScene) {
    if let Some(mut gpu) = WgpuRenderer::try_new(
        64,
        64,
        GpuResourceLimits {
            max_viewport_dimension: 0,
            ..GpuResourceLimits::default()
        },
    ) {
        assert!(gpu.framebuffer_rgba().is_none());
        assert_eq!(gpu.readback_staging_allocations(), 0);
        gpu.render_scene(scene);
        assert_eq!(gpu.target_size(), (1, 1));
        let _ = gpu.framebuffer_rgba().expect("first readback");
        assert_eq!(gpu.readback_staging_allocations(), 1);
        gpu.render_scene(scene);
        let _ = gpu.framebuffer_rgba().expect("second readback");
        assert_eq!(gpu.readback_staging_allocations(), 1);
    }
}

#[test]
fn tiny_skia_blits_color_emoji_rgba_and_span_glyph_color() {
    let mut scene = PaintScene::new(64, 64, Color::BLACK);
    let emoji_rgba = vec![20_u8, 180, 240, 255];
    let color_glyph = krui_text::RasterizedGlyph {
        cache_key: 501,
        x: 0.0,
        y: 0.0,
        width: 1,
        height: 1,
        alpha_mask: std::sync::Arc::from(vec![255_u8]),
        color_rgba: Some(std::sync::Arc::from(emoji_rgba)),
        is_notdef: false,
        is_color: true,
        span_index: 0,
        char_start: 0,
        char_end: 1,
        glyph_color: None,
    };
    let span_glyph = krui_text::RasterizedGlyph {
        cache_key: 502,
        x: 10.0,
        y: 0.0,
        width: 1,
        height: 1,
        alpha_mask: std::sync::Arc::from(vec![255_u8]),
        color_rgba: None,
        is_notdef: false,
        is_color: false,
        span_index: 1,
        char_start: 1,
        char_end: 2,
        glyph_color: Some(Color {
            r: 250,
            g: 90,
            b: 40,
            a: 255,
        }),
    };
    scene.primitives.push(DrawPrimitive::TextGlyphs {
        origin_x: 5.0,
        origin_y: 5.0,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        color: Color::WHITE,
        glyphs: std::sync::Arc::from(vec![color_glyph, span_glyph]),
    });

    let mut cpu = TinySkiaRenderer::new(64, 64, GpuResourceLimits::default());
    cpu.render_scene(&scene);
    let fb = cpu.framebuffer_rgba().expect("framebuffer");
    let emoji_px = (5 * 64 + 5) * 4;
    assert_eq!(&fb[emoji_px..emoji_px + 4], &[20, 180, 240, 255]);
    let span_px = (5 * 64 + 15) * 4;
    assert_eq!(&fb[span_px..span_px + 4], &[250, 90, 40, 255]);
}
