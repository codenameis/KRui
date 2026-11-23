use super::{px_at, GPU_MEDIA_TEST_LOCK};
use krui_core::{PathSegment, SvgColor, SvgVectorOp, SvgVectorTemplate};
use krui_layout::LayoutRect;
use krui_renderer::{
    CanvasCommand, CanvasContext2D, CanvasFillRule, CanvasLineCap, CanvasLineJoin, DecodedImage,
    DrawPrimitive, GpuResourceLimits, ImageSource, PaintScene, RendererBackend, WgpuRenderer,
};
use krui_style::{Affine2, Color};
use std::sync::Arc;

fn render_wgpu(scene: &PaintScene) -> Option<(Vec<u8>, u32, u32)> {
    let _guard = GPU_MEDIA_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut gpu = WgpuRenderer::try_new(scene.width, scene.height, GpuResourceLimits::default())?;
    gpu.render_scene(scene);
    let (w, h) = gpu.target_size();
    let fb = gpu.framebuffer_rgba()?.to_vec();
    Some((fb, w, h))
}

fn push_canvas_scene(
    scene: &mut PaintScene,
    viewport: LayoutRect,
    transform: Affine2,
    commands: Vec<CanvasCommand>,
) {
    scene.primitives.push(DrawPrimitive::CanvasBatch {
        viewport,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform,
        opacity: 1.0,
        commands: Arc::from(commands),
    });
}

fn count_aa_pixels(fb: &[u8], w: u32, rx: (u32, u32), ry: (u32, u32), lo: u8, hi: u8) -> usize {
    let mut count = 0;
    for y in ry.0..ry.1 {
        for x in rx.0..rx.1 {
            if (lo..=hi).contains(&px_at(fb, w, x, y)[0]) {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn diagonal_line_and_circle_emit_smooth_aa_without_24gon_faceting() {
    let mut c = CanvasContext2D::with_size(192.0, 192.0);
    c.stroke_line(8.0, 12.0, 88.0, 58.0, 2.0, Color::WHITE);
    c.fill_circle(100.5, 100.5, 80.0, Color::WHITE);
    let mut scene = PaintScene::new(192, 192, Color::BLACK);
    let full = LayoutRect {
        x: 0.0,
        y: 0.0,
        width: 192.0,
        height: 192.0,
    };
    push_canvas_scene(&mut scene, full, Affine2::IDENTITY, c.into_commands());
    let Some((fb, w, _)) = render_wgpu(&scene) else {
        return;
    };

    let line_aa = count_aa_pixels(&fb, w, (14, 84), (14, 56), 16, 239);
    assert!(
        line_aa >= 12,
        "expected diagonal line AA pixels, got {line_aa}"
    );
    let mid_facet = px_at(&fb, w, 179, 110)[0];
    assert!(
        mid_facet >= 200,
        "adaptive circle at 7.2deg mid-chord must stay filled (>200), got {mid_facet}"
    );
    let circle_aa = count_aa_pixels(&fb, w, (90, 185), (20, 182), 16, 239);
    assert!(
        circle_aa >= 40,
        "expected circle AA pixels, got {circle_aa}"
    );
}

#[test]
fn subpixel_hairline_preserves_continuity_without_dropouts() {
    let mut c = CanvasContext2D::with_size(300.0, 160.0);
    c.stroke_line(10.0, 24.0, 270.0, 120.0, 1.0, Color::WHITE);
    let mut scene = PaintScene::new(128, 96, Color::BLACK);
    let vp = LayoutRect {
        x: 0.0,
        y: 0.0,
        width: 300.0,
        height: 160.0,
    };
    push_canvas_scene(&mut scene, vp, Affine2::scale(0.4, 0.4), c.into_commands());
    let Some((fb, w, h)) = render_wgpu(&scene) else {
        return;
    };

    for x in 8..104u32 {
        let peak = (0..h).map(|y| px_at(&fb, w, x, y)[0]).max().unwrap_or(0);
        assert!(
            (25..=170).contains(&peak),
            "subpixel 0.4px hairline at x={x} expected energy-conserved peak 25..=170, got {peak}"
        );
    }
}

#[test]
fn translucent_fill_and_concave_polygon_have_no_double_alpha_seam() {
    let mut c = CanvasContext2D::with_size(128.0, 96.0);
    c.begin_path();
    c.move_to(12.0, 12.0);
    c.line_to(72.0, 12.0);
    c.line_to(72.0, 36.0);
    c.line_to(36.0, 36.0);
    c.line_to(36.0, 78.0);
    c.line_to(12.0, 78.0);
    c.close_path();
    let translucent_white = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 64,
    };
    c.fill_path(translucent_white);

    let mut ell = CanvasContext2D::with_size(40.0, 64.0);
    ell.fill_circle(16.0, 32.0, 12.0, translucent_white);

    let mut scene = PaintScene::new(128, 96, Color::BLACK);
    let vp = LayoutRect {
        x: 0.0,
        y: 0.0,
        width: 128.0,
        height: 96.0,
    };
    let rot = Affine2::translation(42.0, 45.0)
        * Affine2::rotation_deg(14.0)
        * Affine2::translation(-42.0, -45.0);
    push_canvas_scene(&mut scene, vp, rot, c.into_commands());
    let non_uniform = Affine2::translation(84.0, 24.0) * Affine2::scale(1.8, 0.6);
    push_canvas_scene(&mut scene, vp, non_uniform, ell.into_commands());
    let Some((fb, w, h)) = render_wgpu(&scene) else {
        return;
    };

    let interior = px_at(&fb, w, 26, 22)[0];
    assert!(
        (62..=66).contains(&interior),
        "expected ~64 alpha fill, got {interior}"
    );
    assert_eq!(
        px_at(&fb, w, 58, 58),
        [0, 0, 0, 255],
        "concave cutout must stay empty"
    );
    let max_val = fb
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| px[0])
        .max()
        .unwrap_or(0);
    assert!(
        max_val <= 66,
        "double-alpha seam overshoot detected: max={max_val}"
    );
    let aa_edges = count_aa_pixels(&fb, w, (0, w), (0, h), 2, 60);
    assert!(
        aa_edges >= 20,
        "expected translucent AA edge pixels, got {aa_edges}"
    );
}

fn make_checkerboard_image(size: u32) -> DecodedImage {
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let v = if (x + y) % 2 == 0 { 255 } else { 0 };
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
    }
    DecodedImage::from_rgba8(size, size, rgba).expect("checkerboard")
}

fn push_image(scene: &mut PaintScene, rect: LayoutRect, transform: Affine2, img: DecodedImage) {
    scene.primitives.push(DrawPrimitive::Image {
        rect,
        uv_rect: [0.0, 0.0, 1.0, 1.0],
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform,
        opacity: 1.0,
        border_radius: 0.0,
        source: ImageSource::Decoded(img),
    });
}

#[test]
fn checkerboard_image_downscale_mipmaps_rotated_edge_and_static_snap() {
    let checker = make_checkerboard_image(128);
    let sharp = DecodedImage::from_rgba8(8, 8, [0, 240, 60, 255].repeat(64)).expect("sharp");
    let mut scene = PaintScene::new(64, 64, Color::BLACK);
    let rot_center = Affine2::translation(20.0, 32.0)
        * Affine2::rotation_deg(25.0)
        * Affine2::translation(-20.0, -32.0);
    push_image(
        &mut scene,
        LayoutRect {
            x: 12.0,
            y: 24.0,
            width: 16.0,
            height: 16.0,
        },
        rot_center,
        checker,
    );
    push_image(
        &mut scene,
        LayoutRect {
            x: 42.4,
            y: 12.4,
            width: 8.0,
            height: 8.0,
        },
        Affine2::IDENTITY,
        sharp,
    );
    let Some((fb, w, _)) = render_wgpu(&scene) else {
        return;
    };

    let center_r = px_at(&fb, w, 20, 32)[0];
    assert!(
        (110..=200).contains(&center_r),
        "downscaled checkerboard must converge via mipmaps to mid-gray, got {center_r}"
    );
    let rotated_aa = count_aa_pixels(&fb, w, (8, 32), (20, 44), 8, 105);
    assert!(
        rotated_aa >= 6,
        "expected rotated quad edge AA pixels, got {rotated_aa}"
    );
    assert_eq!(
        px_at(&fb, w, 42, 12)[1],
        240,
        "static 42.4x12.4 snaps to (42,12)"
    );
    assert_eq!(px_at(&fb, w, 41, 12)[1], 0, "left neighbor stays empty");
}

fn build_svg_cap_join_fill_template() -> SvgVectorTemplate {
    SvgVectorTemplate {
        view_box: [0.0, 0.0, 128.0, 96.0],
        intrinsic_size: (128.0, 96.0),
        ops: Arc::from(vec![
            SvgVectorOp::StrokePath {
                segments: vec![
                    PathSegment::MoveTo(24.0, 24.0),
                    PathSegment::LineTo(48.0, 24.0),
                    PathSegment::LineTo(48.0, 48.0),
                ],
                line_width: 8.0,
                line_cap: CanvasLineCap::Round,
                line_join: CanvasLineJoin::Round,
                color: SvgColor::Explicit(Color::WHITE),
            },
            SvgVectorOp::StrokePath {
                segments: vec![
                    PathSegment::MoveTo(24.0, 64.0),
                    PathSegment::LineTo(48.0, 64.0),
                ],
                line_width: 8.0,
                line_cap: CanvasLineCap::Butt,
                line_join: CanvasLineJoin::Miter,
                color: SvgColor::Explicit(Color::WHITE),
            },
            SvgVectorOp::FillPath {
                segments: vec![
                    PathSegment::MoveTo(70.0, 20.0),
                    PathSegment::LineTo(118.0, 20.0),
                    PathSegment::LineTo(118.0, 68.0),
                    PathSegment::LineTo(70.0, 68.0),
                    PathSegment::Close,
                    PathSegment::MoveTo(84.0, 34.0),
                    PathSegment::LineTo(104.0, 34.0),
                    PathSegment::LineTo(104.0, 54.0),
                    PathSegment::LineTo(84.0, 54.0),
                    PathSegment::Close,
                ],
                fill_rule: CanvasFillRule::EvenOdd,
                color: SvgColor::Explicit(Color::WHITE),
            },
        ]),
    }
}

#[test]
fn svg_round_cap_round_join_and_evenodd_fill_rule_render_on_gpu() {
    let tpl = build_svg_cap_join_fill_template();
    let rect = LayoutRect {
        x: 0.0,
        y: 0.0,
        width: 128.0,
        height: 96.0,
    };
    let cmds = tpl.to_canvas_commands(rect, Color::WHITE);
    let mut scene = PaintScene::new(128, 96, Color::BLACK);
    push_canvas_scene(&mut scene, rect, Affine2::IDENTITY, cmds);
    let Some((fb, w, _)) = render_wgpu(&scene) else {
        return;
    };

    assert!(
        px_at(&fb, w, 21, 24)[0] >= 180,
        "round cap extends past x=24"
    );
    assert_eq!(
        px_at(&fb, w, 21, 64),
        [0, 0, 0, 255],
        "butt cap stops at x=24"
    );
    assert!(
        px_at(&fb, w, 75, 44)[0] >= 220,
        "evenodd outer ring is filled"
    );
    assert_eq!(
        px_at(&fb, w, 94, 44),
        [0, 0, 0, 255],
        "evenodd inner same-winding hole remains unfilled"
    );
}
