//! Phase 1 KRui 2.0 end-to-end vertical slice verification tests.

#![forbid(unsafe_code)]

use krui::{
    debug_stats, krui, poll_async_tasks, reset_reactive_runtime, spawn_tokio_task, DecodedImage,
    DrawPrimitive, Effect, ExternalTextureId, GpuResourceLimits, ImageSource, PaintScene,
    ReactiveSession, RendererBackend, Scope, Signal, TinySkiaRenderer, ViewTree, WgpuRenderer,
};
use std::time::Duration;

fn mount_vertical_slice_tree() -> ViewTree {
    let img_path = Signal::new(String::new());
    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        krui!(
            r##"
            <style>
                .clip-card { display: flex; flex-direction: column; width: 200px; height: 180px; padding: 12px; gap: 8px; border-radius: 14px; overflow: hidden; background-color: #1e293b; color: #38bdf8; }
                img.media-thumb { width: 48px; height: 36px; object-fit: cover; border-radius: 6px; }
                .ext-slot { width: 48px; height: 36px; }
            </style>
            <div id="clip-root" class="clip-card">
                <svg id="hero-svg" width="28" height="28" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" fill="none">
                    <circle cx="12" cy="12" r="9" />
                    <path d="M5 12 L10 17 L19 7" />
                </svg>
                <img id="raster-img" class="media-thumb" src="{img_path}" alt="Thumb" />
                <div id="ext-slot" class="ext-slot"></div>
                <a id="doc-link" href="https://krui.dev/docs">Read Docs</a>
            </div>
            "##
        )
    });

    let img_id = tree.find_by_id("raster-img").expect("raster-img node");
    let decoded = DecodedImage::from_rgba8(4, 4, [240_u8, 80, 40, 255].repeat(16)).expect("rgba8");
    tree.set_node_decoded_image(img_id, decoded);

    let ext_id = tree.find_by_id("ext-slot").expect("ext-slot node");
    tree.mount_external_texture(ext_id, ExternalTextureId(7), 32, 32);

    let link_id = tree.find_by_id("doc-link").expect("doc-link node");
    assert_eq!(
        tree.link_href(link_id).as_deref(),
        Some("https://krui.dev/docs")
    );
    tree
}

fn assert_media_scene_primitives(scene: &PaintScene) {
    let has_svg = scene.primitives.iter().any(
        |p| matches!(p, DrawPrimitive::CanvasBatch { clip_radius, .. } if *clip_radius >= 13.5),
    );
    let has_img = scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::Image { clip_radius, source: ImageSource::Decoded(_), .. }
                if *clip_radius >= 13.5
        )
    });
    let has_ext = scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::Image {
                clip_radius,
                source: ImageSource::External { id: ExternalTextureId(7), .. },
                ..
            } if *clip_radius >= 13.5
        )
    });
    let has_text = scene.primitives.iter().any(
        |p| matches!(p, DrawPrimitive::TextGlyphs { clip_radius, .. } if *clip_radius >= 13.5),
    );
    assert!(has_svg, "missing rounded-clipped SVG CanvasBatch");
    assert!(has_img, "missing rounded-clipped Decoded Image");
    assert!(has_ext, "missing rounded-clipped ExternalTexture Image");
    assert!(has_text, "missing rounded-clipped link TextGlyphs");
}

#[test]
fn compiled_media_svg_external_texture_and_rounded_clip_cpu_gpu_parity() {
    reset_reactive_runtime();
    let mut tree = mount_vertical_slice_tree();
    let (scene, _) = tree.build_paint_scene(240, 220);
    assert_media_scene_primitives(&scene);

    let mut cpu = TinySkiaRenderer::new(240, 220, GpuResourceLimits::default());
    cpu.render_scene(&scene);
    assert!(
        cpu.framebuffer_rgba()
            .is_some_and(|px| px.iter().any(|&b| b != 0)),
        "TinySkia framebuffer must be non-empty"
    );

    if let Some(mut gpu) = WgpuRenderer::try_new(240, 220, GpuResourceLimits::default()) {
        gpu.render_scene(&scene);
        assert!(
            gpu.framebuffer_rgba()
                .is_some_and(|px| px.iter().any(|&b| b != 0)),
            "Wgpu framebuffer must be non-empty"
        );
    }
}

fn spawn_pending_session_work(session: &ReactiveSession) -> (Scope, krui::AsyncTaskHandle) {
    session.enter(|| {
        let base = debug_stats();
        let scope = Scope::new();
        let handle = scope.run(|| {
            let sig = Signal::new(0_i32);
            let _eff = Effect::new(move || {
                let _ = sig.get();
            });
            spawn_tokio_task(
                async {
                    krui::tokio::time::sleep(Duration::from_secs(5)).await;
                    99_i32
                },
                move |v| sig.set(v),
            )
        });
        assert!(handle.is_active());
        assert_eq!(
            debug_stats().live_async_pollers,
            base.live_async_pollers + 1
        );
        (scope, handle)
    })
}

#[test]
fn concurrent_reactive_sessions_and_tokio_cancellation_return_to_baseline() {
    let session_a = ReactiveSession::new();
    let session_b = ReactiveSession::new();
    assert_ne!(session_a.id(), session_b.id());

    let base_a = session_a.enter(debug_stats);
    let base_b = session_b.enter(debug_stats);

    let (scope_a, handle_a) = spawn_pending_session_work(&session_a);
    let (scope_b, handle_b) = spawn_pending_session_work(&session_b);
    assert_eq!(
        session_a.enter(debug_stats).live_async_pollers,
        base_a.live_async_pollers + 1
    );
    assert_eq!(
        session_b.enter(debug_stats).live_async_pollers,
        base_b.live_async_pollers + 1
    );

    session_a.enter(|| {
        scope_a.dispose();
        poll_async_tasks();
        assert!(!handle_a.is_active());
    });
    assert!(session_b.enter(|| handle_b.is_active()));
    assert_eq!(session_a.enter(debug_stats), base_a);
    assert_eq!(
        session_b.enter(debug_stats).live_async_pollers,
        base_b.live_async_pollers + 1
    );

    session_b.enter(|| {
        scope_b.dispose();
        poll_async_tasks();
        assert!(!handle_b.is_active());
    });
    assert_eq!(session_a.enter(debug_stats), base_a);
    assert_eq!(session_b.enter(debug_stats), base_b);
}

#[cfg(feature = "preview")]
#[test]
fn macro_and_preview_interpreter_emit_matching_paint_primitives() {
    const PARITY_SRC: &str = r##"
            <style>
                .card { display: flex; flex-direction: column; width: 220px; height: 140px; padding: 12px; gap: 8px; border-radius: 12px; overflow: hidden; background-color: #0f172a; color: #f8fafc; }
            </style>
            <div class="card">
                <svg width="24" height="24" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" fill="none">
                    <path d="M4 12 L10 18 L20 6" />
                </svg>
                <a href="https://krui.dev">Portal</a>
                <p>Parity Check</p>
            </div>
    "##;

    reset_reactive_runtime();
    let mut tree_macro = ViewTree::new();
    tree_macro.mount_with(|| {
        krui!(
            r##"
            <style>
                .card { display: flex; flex-direction: column; width: 220px; height: 140px; padding: 12px; gap: 8px; border-radius: 12px; overflow: hidden; background-color: #0f172a; color: #f8fafc; }
            </style>
            <div class="card">
                <svg width="24" height="24" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" fill="none">
                    <path d="M4 12 L10 18 L20 6" />
                </svg>
                <a href="https://krui.dev">Portal</a>
                <p>Parity Check</p>
            </div>
            "##
        )
    });

    let doc = krui::preview::compile_krui_source(PARITY_SRC).expect("compile parity markup");
    let mut tree_interp = ViewTree::new();
    krui::preview::mount_compiled_document(&mut tree_interp, &doc);

    let (scene_macro, _) = tree_macro.build_paint_scene(320, 240);
    let (scene_interp, _) = tree_interp.build_paint_scene(320, 240);
    assert_eq!(scene_macro.primitives, scene_interp.primitives);

    let has_box = scene_macro
        .primitives
        .iter()
        .any(|p| matches!(p, DrawPrimitive::StyledBox { .. }));
    let has_svg = scene_macro.primitives.iter().any(
        |p| matches!(p, DrawPrimitive::CanvasBatch { clip_radius, .. } if *clip_radius >= 11.5),
    );
    let has_text = scene_macro.primitives.iter().any(
        |p| matches!(p, DrawPrimitive::TextGlyphs { clip_radius, .. } if *clip_radius >= 11.5),
    );
    assert!(
        has_box && has_svg && has_text,
        "expected StyledBox, CanvasBatch, and TextGlyphs"
    );
}
