//! Integration tests for Wave 2 `CanvasContext2D` features:
//! transforms (`save`/`restore`, `translate`, `rotate`, `scale`, `set_transform`),
//! `global_alpha`, `set_line_dash`/`offset`, `CanvasGradient`, `clip_path`/`clip_rect`,
//! and interleaved `draw_text`/`draw_image` across `TinySkiaRenderer` and `WgpuRenderer`.

#![forbid(unsafe_code)]

use krui::{
    krui, AdaptiveRenderer, CanvasContext2D, CanvasDraw, CanvasGradient, CanvasTextAlign, Color,
    DecodedImage, GpuResourceLimits, RenderBackendPreference, Size, ViewTree,
};

fn render_tree_pixels(
    tree: &mut ViewTree,
    w: u32,
    h: u32,
    pref: RenderBackendPreference,
) -> Option<Vec<u8>> {
    let mut renderer = AdaptiveRenderer::try_new(w, h, pref, GpuResourceLimits::default()).ok()?;
    let (scene, _) = tree.build_paint_scene(w, h);
    let _ = renderer.render_scene(&scene);
    renderer.finish_gpu_work();
    renderer.framebuffer_rgba().map(|buf| buf.to_vec())
}

fn pixel_at(rgba: &[u8], stride_w: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * stride_w + x) * 4) as usize;
    [rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3]]
}

#[test]
fn canvas_transforms_save_restore_and_global_alpha() {
    let draw = CanvasDraw::new(|ctx: &mut CanvasContext2D, _sz: Size| {
        ctx.save();
        ctx.translate(40.0, 40.0);
        ctx.scale(2.0, 2.0);
        ctx.set_global_alpha(0.5);
        ctx.fill_rect(0.0, 0.0, 20.0, 20.0, Color::rgb(255, 0, 0));
        ctx.restore();

        // After restore, transform and global_alpha revert to identity / 1.0
        ctx.fill_rect(10.0, 10.0, 20.0, 20.0, Color::rgb(0, 0, 255));

        ctx.save();
        ctx.translate(140.0, 40.0);
        ctx.rotate(std::f32::consts::FRAC_PI_2);
        ctx.fill_rect(0.0, 0.0, 20.0, 10.0, Color::rgb(0, 255, 0));
        ctx.restore();
    });
    let mut tree = ViewTree::new();
    let frag = krui!(
        r#"<canvas style="width: 200px; height: 120px; background: #000000;" draw="{draw}" />"#
    );
    frag.mount(&mut tree, None);
    let px = render_tree_pixels(&mut tree, 200, 120, RenderBackendPreference::ForceTinySkia)
        .expect("TinySkia pixels");

    let transformed = pixel_at(&px, 200, 60, 60);
    assert!(
        transformed[0] > 100 && transformed[0] < 160 && transformed[2] < 5,
        "{transformed:?}"
    );
    let restored = pixel_at(&px, 200, 15, 15);
    assert!(restored[2] > 240 && restored[0] < 5, "{restored:?}");
    let rotated = pixel_at(&px, 200, 135, 50);
    assert!(rotated[1] > 240 && rotated[0] < 5, "{rotated:?}");
}

#[test]
fn canvas_dashed_stroke_leaves_alternating_gaps() {
    let draw = CanvasDraw::new(|ctx: &mut CanvasContext2D, _sz: Size| {
        ctx.set_line_dash(&[10.0, 10.0]);
        ctx.set_line_dash_offset(0.0);
        ctx.stroke_line(10.0, 30.0, 110.0, 30.0, 4.0, Color::rgb(255, 255, 255));
        ctx.set_line_dash(&[]);
    });
    let mut tree = ViewTree::new();
    krui!(r#"<canvas style="width: 140px; height: 60px; background: #000000;" draw="{draw}" />"#)
        .mount(&mut tree, None);
    let px = render_tree_pixels(&mut tree, 140, 60, RenderBackendPreference::ForceTinySkia)
        .expect("TinySkia pixels");

    let on_dash = pixel_at(&px, 140, 15, 30);
    let in_gap = pixel_at(&px, 140, 25, 30);
    let second_dash = pixel_at(&px, 140, 35, 30);
    assert!(on_dash[0] > 200, "first dash should be lit: {on_dash:?}");
    assert!(in_gap[0] < 20, "gap should remain dark: {in_gap:?}");
    assert!(
        second_dash[0] > 200,
        "second dash should be lit: {second_dash:?}"
    );
}

#[test]
fn canvas_linear_and_radial_gradients_on_both_backends() {
    let draw = CanvasDraw::new(|ctx: &mut CanvasContext2D, _sz: Size| {
        let mut lin = CanvasGradient::linear(10.0, 10.0, 110.0, 10.0);
        lin.add_stop(0.0, Color::rgb(255, 0, 0))
            .add_stop(0.5, Color::rgb(0, 255, 0))
            .add_stop(1.0, Color::rgb(0, 0, 255));
        ctx.fill_rect_gradient(10.0, 10.0, 100.0, 30.0, 0.0, lin);

        let mut rad = CanvasGradient::radial(60.0, 80.0, 2.0, 60.0, 80.0, 28.0);
        rad.add_stop(0.0, Color::rgb(255, 255, 0))
            .add_stop(1.0, Color::rgb(0, 0, 255));
        ctx.fill_circle_gradient(60.0, 80.0, 28.0, rad);
    });
    let mut tree = ViewTree::new();
    krui!(r#"<canvas style="width: 140px; height: 120px; background: #000000;" draw="{draw}" />"#)
        .mount(&mut tree, None);
    for pref in [
        RenderBackendPreference::ForceTinySkia,
        RenderBackendPreference::ForceWgpu,
    ] {
        let Some(px) = render_tree_pixels(&mut tree, 140, 120, pref) else {
            continue;
        };
        let left = pixel_at(&px, 140, 14, 25);
        let mid = pixel_at(&px, 140, 60, 25);
        let right = pixel_at(&px, 140, 106, 25);
        assert!(left[0] > 200 && left[2] < 30, "{pref:?} left={left:?}");
        assert!(
            mid[1] > 200 && mid[0] < 40 && mid[2] < 40,
            "{pref:?} mid={mid:?}"
        );
        assert!(right[2] > 200 && right[0] < 30, "{pref:?} right={right:?}");

        let center = pixel_at(&px, 140, 60, 80);
        let rim = pixel_at(&px, 140, 85, 80);
        assert!(
            center[0] > 200 && center[1] > 200,
            "{pref:?} center={center:?}"
        );
        assert!(rim[2] > 180, "{pref:?} rim={rim:?}");
    }
}

#[test]
fn canvas_clip_path_and_restore_parity_across_backends() {
    let draw = CanvasDraw::new(|ctx: &mut CanvasContext2D, _sz: Size| {
        ctx.save();
        ctx.begin_path();
        ctx.arc(60.0, 60.0, 30.0, 0.0, std::f32::consts::TAU);
        ctx.close_path();
        ctx.clip_path();
        ctx.fill_rect(10.0, 10.0, 100.0, 100.0, Color::rgb(255, 0, 0));
        ctx.restore();

        // Drawn after restore(): outside the clip region, must be visible
        ctx.fill_rect(105.0, 10.0, 20.0, 20.0, Color::rgb(0, 255, 0));
    });
    let mut tree = ViewTree::new();
    krui!(r#"<canvas style="width: 140px; height: 120px; background: #000000;" draw="{draw}" />"#)
        .mount(&mut tree, None);
    for pref in [
        RenderBackendPreference::ForceTinySkia,
        RenderBackendPreference::ForceWgpu,
    ] {
        let Some(px) = render_tree_pixels(&mut tree, 140, 120, pref) else {
            continue;
        };
        let inside = pixel_at(&px, 140, 60, 60);
        let clipped_corner = pixel_at(&px, 140, 20, 20);
        let after_restore = pixel_at(&px, 140, 115, 20);
        assert!(inside[0] > 240, "{pref:?} inside={inside:?}");
        assert!(
            clipped_corner[0] < 10 && clipped_corner[1] < 10,
            "{pref:?} clipped={clipped_corner:?}"
        );
        assert!(
            after_restore[1] > 240,
            "{pref:?} after_restore={after_restore:?}"
        );
    }
}

#[test]
fn canvas_interleaved_vector_text_and_image_rendering() {
    let mut rgba = Vec::with_capacity(4 * 4 * 4);
    for _ in 0..16 {
        rgba.extend_from_slice(&[0, 255, 0, 255]);
    }
    let img = DecodedImage::from_rgba8(4, 4, rgba).expect("4x4 green image");
    let draw = CanvasDraw::new(move |ctx: &mut CanvasContext2D, _sz: Size| {
        ctx.fill_rect(5.0, 5.0, 150.0, 80.0, Color::rgb(16, 24, 40));
        ctx.draw_image(img.clone(), 120.0, 20.0, 30.0, 30.0);
        ctx.draw_text_styled(
            "OK",
            60.0,
            24.0,
            18.0,
            700,
            Color::rgb(255, 255, 255),
            CanvasTextAlign::Center,
        );
        ctx.fill_rect(10.0, 65.0, 20.0, 15.0, Color::rgb(255, 0, 0));
    });
    let mut tree = ViewTree::new();
    krui!(r#"<canvas style="width: 180px; height: 100px; background: #000000;" draw="{draw}" />"#)
        .mount(&mut tree, None);
    let px = render_tree_pixels(&mut tree, 180, 100, RenderBackendPreference::ForceTinySkia)
        .expect("TinySkia pixels");

    let img_px = pixel_at(&px, 180, 135, 35);
    let post_vec = pixel_at(&px, 180, 20, 72);
    assert!(img_px[1] > 240 && img_px[0] < 20, "image pixel={img_px:?}");
    assert!(
        post_vec[0] > 240 && post_vec[1] < 20,
        "post-image rect={post_vec:?}"
    );
    let scene = tree.build_paint_scene(180, 100).0;
    assert!(
        scene
            .primitives
            .iter()
            .any(|p| matches!(p, krui::DrawPrimitive::TextGlyphs { .. })),
        "interleaved draw_text must emit TextGlyphs primitive"
    );
}
