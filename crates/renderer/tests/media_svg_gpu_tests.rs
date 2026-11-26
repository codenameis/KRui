#![forbid(unsafe_code)]

use krui_layout::LayoutRect;
use krui_renderer::{
    CanvasCommand, CanvasContext2D, CanvasMeshCache, DecodedImage, DrawPrimitive,
    ExternalTextureId, GpuContext, GpuResourceLimits, ImageSource, PaintScene, RendererBackend,
    TinySkiaRenderer, WgpuRenderer,
};
use krui_style::{Affine2, Color};
use krui_text::RasterizedGlyph;
use std::sync::{Arc, Mutex};

static GPU_MEDIA_TEST_LOCK: Mutex<()> = Mutex::new(());

fn px_at(fb: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * width + x) * 4) as usize;
    [fb[idx], fb[idx + 1], fb[idx + 2], fb[idx + 3]]
}

fn render_on_both(scene: &PaintScene, mut verify: impl FnMut(&str, &[u8], u32, u32)) {
    let mut cpu = TinySkiaRenderer::new(scene.width, scene.height, GpuResourceLimits::default());
    cpu.render_scene(scene);
    let cpu_fb = cpu.framebuffer_rgba().expect("cpu fb").to_vec();
    verify(
        "TinySkia",
        &cpu_fb,
        cpu.physical_width(),
        cpu.physical_height(),
    );

    let _guard = GPU_MEDIA_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(mut gpu) =
        WgpuRenderer::try_new(scene.width, scene.height, GpuResourceLimits::default())
    {
        gpu.render_scene(scene);
        let (gw, gh) = gpu.target_size();
        let gpu_fb = gpu.framebuffer_rgba().expect("wgpu fb").to_vec();
        verify("Wgpu", &gpu_fb, gw, gh);
    }
}

fn make_sampleable_target(
    ctx: &GpuContext,
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = ctx.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("media_svg_gpu_test_target"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}

fn readback_gpu_texture(ctx: &GpuContext, tex: &wgpu::Texture, w: u32, h: u32) -> Vec<u8> {
    assert_eq!((w * 4) % 256, 0, "aligned row stride expected for tests");
    let bytes_per_row = w * 4;
    let total = (bytes_per_row * h) as u64;
    let staging = ctx.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("media_svg_readback"),
        size: total,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = ctx
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    enc.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &staging,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    ctx.queue().submit(std::iter::once(enc.finish()));
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    ctx.device().poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range().to_vec();
    staging.unmap();
    data
}

fn tess_probe(
    cache: &mut CanvasMeshCache,
    cmds: &Arc<[CanvasCommand]>,
    vp: LayoutRect,
    clip_radius: f32,
    scale_factor: f32,
) {
    let _ = cache.get_or_tessellate(
        cmds,
        vp,
        Some(vp),
        clip_radius,
        scale_factor,
        Affine2::IDENTITY,
        1.0,
        (80.0, 80.0),
        GpuResourceLimits::default(),
    );
}

#[test]
fn canvas_mesh_cache_aba_resistance_and_clip_radius_keys() {
    let mut cache = CanvasMeshCache::new();
    let vp = LayoutRect {
        x: 0.0,
        y: 0.0,
        width: 80.0,
        height: 80.0,
    };
    let mut c1 = CanvasContext2D::new();
    c1.fill_rect(4.0, 4.0, 30.0, 30.0, Color::WHITE);
    let cmds1: Arc<[_]> = Arc::from(c1.commands().to_vec());
    tess_probe(&mut cache, &cmds1, vp, 0.0, 1.0);
    assert_eq!(cache.stats(), (0, 1));
    tess_probe(&mut cache, &cmds1, vp, 0.0, 1.0);
    assert_eq!(cache.stats(), (1, 1));

    drop(cmds1);
    let mut c2 = CanvasContext2D::new();
    c2.fill_rect(4.0, 4.0, 30.0, 30.0, Color::BLACK);
    let cmds2: Arc<[_]> = Arc::from(c2.commands().to_vec());
    tess_probe(&mut cache, &cmds2, vp, 0.0, 1.0);
    assert_eq!(cache.stats(), (1, 2));
    tess_probe(&mut cache, &cmds2, vp, 8.0, 1.0);
    tess_probe(&mut cache, &cmds2, vp, 8.0, 2.0);
    assert_eq!(cache.stats(), (1, 4));
}

fn push_rounded_curve_canvas_and_glyph(scene: &mut PaintScene) {
    let mut ctx = CanvasContext2D::new();
    ctx.begin_path();
    ctx.move_to(0.0, 0.0);
    ctx.bezier_curve_to(16.0, 0.0, 48.0, 0.0, 64.0, 0.0);
    ctx.arc(48.0, 32.0, 16.0, 0.0, std::f32::consts::PI * 0.5);
    ctx.bezier_curve_to(48.0, 64.0, 16.0, 64.0, 0.0, 64.0);
    ctx.close_path();
    ctx.fill_path(Color {
        r: 40,
        g: 220,
        b: 90,
        a: 255,
    });
    let box_rect = LayoutRect {
        x: 8.0,
        y: 8.0,
        width: 64.0,
        height: 64.0,
    };
    scene.primitives.push(DrawPrimitive::CanvasBatch {
        viewport: box_rect,
        clip: Some(box_rect),
        clip_radius: 14.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        commands: Arc::from(ctx.commands().to_vec()),
    });
    scene.primitives.push(DrawPrimitive::TextGlyphs {
        origin_x: 80.0,
        origin_y: 8.0,
        clip: Some(LayoutRect {
            x: 80.0,
            y: 8.0,
            width: 32.0,
            height: 32.0,
        }),
        clip_radius: 12.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        color: Color::WHITE,
        glyphs: Arc::from(vec![RasterizedGlyph {
            cache_key: 8801,
            x: 0.0,
            y: 0.0,
            width: 32,
            height: 32,
            alpha_mask: Arc::from(vec![255_u8; 32 * 32]),
            color_rgba: None,
            is_notdef: false,
            is_color: false,
            span_index: 0,
            char_start: 0,
            char_end: 1,
            glyph_color: None,
        }]),
    });
}

#[test]
fn cubic_bezier_arc_and_rounded_clip_render_on_cpu_and_gpu() {
    let mut scene = PaintScene::new(128, 96, Color::BLACK);
    push_rounded_curve_canvas_and_glyph(&mut scene);
    render_on_both(&scene, |label, fb, w, _| {
        assert_eq!(px_at(fb, w, 9, 9), [0, 0, 0, 255], "[{label}] canvas clip");
        let mid = px_at(fb, w, 36, 36);
        assert!(
            mid[1] > 180 && mid[0] < 60,
            "[{label}] curve interior {mid:?}"
        );
        assert_eq!(
            px_at(fb, w, 81, 9),
            [0, 0, 0, 255],
            "[{label}] glyph corner"
        );
        assert!(px_at(fb, w, 96, 24)[0] > 200, "[{label}] glyph center");
    });
}

#[test]
fn decoded_image_uv_crop_border_radius_and_external_texture_on_gpu() {
    let mut pixels = Vec::with_capacity(4 * 4 * 4);
    for _y in 0..4 {
        for x in 0..4 {
            if x < 2 {
                pixels.extend_from_slice(&[240, 20, 20, 255]);
            } else {
                pixels.extend_from_slice(&[20, 40, 240, 255]);
            }
        }
    }
    let img = DecodedImage::from_rgba8(4, 4, pixels).expect("valid rgba8");
    let mut scene = PaintScene::new(64, 64, Color::BLACK);
    scene.primitives.push(DrawPrimitive::Image {
        rect: LayoutRect {
            x: 4.0,
            y: 4.0,
            width: 56.0,
            height: 56.0,
        },
        uv_rect: [0.5, 0.0, 1.0, 1.0],
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        border_radius: 14.0,
        source: ImageSource::Decoded(img),
    });

    render_on_both(&scene, |label, fb, w, _| {
        assert_eq!(
            px_at(fb, w, 5, 5),
            [0, 0, 0, 255],
            "[{label}] rounded corner"
        );
        let center = px_at(fb, w, 32, 32);
        assert!(
            center[2] > 200 && center[0] < 40,
            "[{label}] blue crop {center:?}"
        );
    });

    verify_external_texture_and_cpu_noop();
}

fn make_external_texture_scene(ext_id: ExternalTextureId) -> PaintScene {
    let mut scene = PaintScene::new(64, 64, Color::BLACK);
    scene.primitives.push(DrawPrimitive::Image {
        rect: LayoutRect {
            x: 8.0,
            y: 8.0,
            width: 48.0,
            height: 48.0,
        },
        uv_rect: [0.0, 0.0, 1.0, 1.0],
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        border_radius: 0.0,
        source: ImageSource::External {
            id: ext_id,
            width: 32,
            height: 32,
        },
    });
    scene
}

fn verify_external_texture_and_cpu_noop() {
    let ext_id = ExternalTextureId(7);
    let scene = make_external_texture_scene(ext_id);
    let mut cpu = TinySkiaRenderer::new(64, 64, GpuResourceLimits::default());
    cpu.render_scene(&scene);

    let _guard = GPU_MEDIA_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let Some(ctx) = GpuContext::try_new() else {
        return;
    };
    let mut gpu = WgpuRenderer::from_context(
        Arc::clone(&ctx),
        wgpu::TextureFormat::Rgba8Unorm,
        64,
        64,
        GpuResourceLimits::default(),
    );
    let (ext_tex, ext_view) = make_sampleable_target(&ctx, 64, 64, wgpu::TextureFormat::Rgba8Unorm);
    gpu.register_external_texture(ext_id, &ext_view);
    let fill_scene = PaintScene::new(
        64,
        64,
        Color {
            r: 20,
            g: 230,
            b: 70,
            a: 255,
        },
    );
    gpu.render_to_view(&fill_scene, &ext_view, (64, 64), 1.0);
    gpu.render_scene(&scene);
    let fb = gpu.framebuffer_rgba().expect("ext fb");
    let center = px_at(fb, 64, 32, 32);
    assert!(
        center[1] > 200 && center[0] < 40,
        "ext green expected, got {center:?}"
    );
    gpu.unregister_external_texture(ext_id);
    drop((ext_view, ext_tex));
}

#[test]
fn wgpu_lazy_offscreen_srgb_and_sampleable_blur_lifecycle() {
    let _guard = GPU_MEDIA_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let Some(ctx) = GpuContext::try_new() else {
        return;
    };
    let mut gpu = WgpuRenderer::from_context(
        Arc::clone(&ctx),
        wgpu::TextureFormat::Bgra8UnormSrgb,
        64,
        64,
        GpuResourceLimits::default(),
    );
    assert!(!gpu.has_offscreen_target());
    let mut mid_scene = PaintScene::new(64, 64, Color::BLACK);
    mid_scene.primitives.push(DrawPrimitive::StyledBox {
        rect: LayoutRect {
            x: 8.0,
            y: 8.0,
            width: 48.0,
            height: 48.0,
        },
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        background_color: Color {
            r: 64,
            g: 128,
            b: 200,
            a: 255,
        },
        background_gradient: None,
        radial_gradient: None,
        conic_gradient: None,
        border_color: Color::TRANSPARENT,
        border_width: 0.0,
        border_radius: 0.0,
        corner_radii: [0.0; 4],
        box_shadow: None,
        backdrop_blur_px: 0.0,
    });
    let (srgb_tex, srgb_view) =
        make_sampleable_target(&ctx, 64, 64, wgpu::TextureFormat::Bgra8UnormSrgb);
    gpu.render_to_view(&mid_scene, &srgb_view, (64, 64), 1.0);
    assert!(!gpu.has_offscreen_target());
    let raw = readback_gpu_texture(&ctx, &srgb_tex, 64, 64);
    let px = px_at(&raw, 64, 32, 32);
    assert!(
        (px[0] as i32 - 200).abs() <= 4
            && (px[1] as i32 - 128).abs() <= 4
            && (px[2] as i32 - 64).abs() <= 4,
        "Bgra8UnormSrgb parity failed, got BGRA {px:?}"
    );
    drop((srgb_view, srgb_tex));
    sampleable_swapchain_blur_survives_prior_surface_destruction(&ctx, &mut gpu);
}

fn make_striped_blur_box(
    rect: LayoutRect,
    background_color: Color,
    border_radius: f32,
    backdrop_blur_px: f32,
) -> DrawPrimitive {
    DrawPrimitive::StyledBox {
        rect,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        background_color,
        background_gradient: None,
        radial_gradient: None,
        conic_gradient: None,
        border_color: Color::TRANSPARENT,
        border_width: 0.0,
        border_radius,
        corner_radii: [0.0; 4],
        box_shadow: None,
        backdrop_blur_px,
    }
}

fn build_striped_blur_scene() -> PaintScene {
    let mut scene = PaintScene::new(64, 64, Color::BLACK);
    for gx in (0..8).filter(|gx| gx % 2 == 0) {
        let rect = LayoutRect {
            x: (gx * 8) as f32,
            y: 0.0,
            width: 8.0,
            height: 64.0,
        };
        scene
            .primitives
            .push(make_striped_blur_box(rect, Color::WHITE, 0.0, 0.0));
    }
    let glass = LayoutRect {
        x: 8.0,
        y: 8.0,
        width: 48.0,
        height: 48.0,
    };
    let glass_bg = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 24,
    };
    scene
        .primitives
        .push(make_striped_blur_box(glass, glass_bg, 8.0, 10.0));
    scene
}

fn sampleable_swapchain_blur_survives_prior_surface_destruction(
    ctx: &GpuContext,
    gpu: &mut WgpuRenderer,
) {
    let blur_scene = build_striped_blur_scene();
    let (tex1, view1) = make_sampleable_target(ctx, 64, 64, wgpu::TextureFormat::Bgra8UnormSrgb);
    gpu.render_to_view(&blur_scene, &view1, (64, 64), 1.0);
    drop(view1);
    tex1.destroy();

    let (tex2, view2) = make_sampleable_target(ctx, 64, 64, wgpu::TextureFormat::Bgra8UnormSrgb);
    gpu.render_to_view(&blur_scene, &view2, (64, 64), 1.0);
    let fb = readback_gpu_texture(ctx, &tex2, 64, 64);
    let v_left = px_at(&fb, 64, 18, 32)[0] as i32;
    let v_right = px_at(&fb, 64, 24, 32)[0] as i32;
    assert!(
        (v_left - v_right).abs() < 200,
        "expected blur softening, got {v_left} vs {v_right}"
    );
    drop((view2, tex2));
}

#[path = "media_svg_gpu/aa_cases.rs"]
mod aa_cases;
