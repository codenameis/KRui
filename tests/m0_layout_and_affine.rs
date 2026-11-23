//! P0-6 & P0-7 Verification: Intrinsic Multiline Text Layout, Multi-Root Synthetic Root,
//! and Unified `Affine2` Center-Origin Transform Matrix + Inverse Hit-Testing.

#![forbid(unsafe_code)]

use krui::*;

fn find_box_by_bg(scene: &PaintScene, color: Color) -> (LayoutRect, Affine2) {
    scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                transform,
                background_color,
                ..
            } if *background_color == color => Some((*rect, *transform)),
            _ => None,
        })
        .expect("matching StyledBox")
}

#[test]
fn multiline_text_growth_increases_node_height_and_two_roots_layout() {
    reset_reactive_runtime();

    let mut session = EngineSession::new(600, 500, RenderBackendPreference::ForceTinySkia);
    let paragraph = Signal::new("한 줄 텍스트".to_string());

    let fragment = krui!(
        r#"
        <div id="root-one" style="display: flex; flex-direction: column; width: 180px; padding: 8px; background-color: #1e293b">
            <p id="grow-para">{paragraph}</p>
        </div>
        <div id="root-two" style="display: flex; flex-direction: column; width: 180px; height: 60px; background-color: #334155">
            <span>두 번째 루트</span>
        </div>
        "#
    );
    let roots = session.tree.mount(fragment);
    assert_eq!(roots.len(), 2);

    let f1 = session.step_frame(1.0 / 120.0);
    assert!(f1.layout_ran);
    assert_eq!(f1.total_relayout_passes, 1);

    let (scene1, _) = session.tree.build_paint_scene(600, 500);
    let (root1_box_before, _) = find_box_by_bg(&scene1, Color::rgb(30, 41, 59));
    let (root2_box_before, _) = find_box_by_bg(&scene1, Color::rgb(51, 65, 85));

    assert!(root1_box_before.height > 16.0);
    assert!(root2_box_before.y >= root1_box_before.height - 0.5);

    paragraph.set(
        "첫 번째 줄을 넘어가는 아주 긴 한국어 문장이며 두 번째 줄과 세 번째 줄을 넘어 네 번째 줄까지 줄바꿈되어 높이가 크게 늘어납니다"
            .to_string(),
    );

    let f2 = session.step_frame(1.0 / 120.0);
    assert!(f2.layout_ran);
    assert_eq!(f2.total_relayout_passes, 2);

    let (scene2, _) = session.tree.build_paint_scene(600, 500);
    let (root1_box_after, _) = find_box_by_bg(&scene2, Color::rgb(30, 41, 59));
    let (root2_box_after, _) = find_box_by_bg(&scene2, Color::rgb(51, 65, 85));

    assert!(root1_box_after.height >= root1_box_before.height * 2.0);
    assert!(root2_box_after.y > root2_box_before.y);
}

#[test]
fn rotated_and_scaled_parent_button_hit_testing_and_center_origin_scale() {
    reset_reactive_runtime();

    let mut session = EngineSession::new(800, 600, RenderBackendPreference::ForceTinySkia);
    let clicked = Signal::new(0_i32);
    let on_btn = Command::new(move || clicked.update(|c| *c += 1));

    let fragment = krui!(
        r#"
        <div style="display: flex; flex-direction: column; width: 800px; height: 600px">
            <div id="rot-parent" style="display: flex; flex-direction: column; margin-left: 100px; margin-top: 100px; width: 300px; height: 200px; transform: scale(1.5) rotate(30deg); background-color: #1e293b">
                <button id="rot-btn" style="width: 120px; height: 44px; background-color: #ef4444" onclick="{on_btn}">회전버튼</button>
            </div>
            <button id="scaled-btn" style="width: 160px; height: 50px; transform: scale(1.1); background-color: #22c55e">스케일버튼</button>
        </div>
        "#
    );
    session.tree.mount(fragment);
    let _ = session.step_frame(1.0 / 120.0);

    let (scene, _) = session.tree.build_paint_scene(800, 600);

    let (rot_btn_rect, rot_btn_affine) = find_box_by_bg(&scene, Color::rgb(239, 68, 68));
    let local_center_x = rot_btn_rect.x + rot_btn_rect.width * 0.5;
    let local_center_y = rot_btn_rect.y + rot_btn_rect.height * 0.5;
    let (screen_cx, screen_cy) = rot_btn_affine.transform_point(local_center_x, local_center_y);

    assert!(session.tree.dispatch_click(screen_cx, screen_cy));
    assert_eq!(clicked.get(), 1);

    session.tree.dispatch_click(local_center_x, local_center_y);
    assert_eq!(clicked.get(), 1);

    let (scaled_rect, scaled_affine) = find_box_by_bg(&scene, Color::rgb(34, 197, 94));
    let sc_cx = scaled_rect.x + scaled_rect.width * 0.5;
    let sc_cy = scaled_rect.y + scaled_rect.height * 0.5;
    let (tx_cx, tx_cy) = scaled_affine.transform_point(sc_cx, sc_cy);
    assert!((tx_cx - sc_cx).abs() < 1e-3 && (tx_cy - sc_cy).abs() < 1e-3);
}

#[test]
fn overflow_clip_masks_rotated_child_pixels_and_hover_color_reuses_shaped_text() {
    reset_reactive_runtime();

    let mut session = EngineSession::new(200, 200, RenderBackendPreference::ForceTinySkia);
    let fragment = krui!(
        r#"
        <style>
            .viewport { width: 200px; height: 200px; background: #000000; }
            .clip-box { width: 80px; height: 80px; overflow: hidden; background: #111111; }
            .overflow-child { width: 160px; height: 40px; background: #ff0000; color: #ffffff; transform: rotate(35deg); }
            .overflow-child:hover { color: #00ff00; }
        </style>
        <div class="viewport">
            <div class="clip-box">
                <button id="clip-btn" class="overflow-child">Clipped Text</button>
            </div>
        </div>
        "#
    );
    session.tree.mount(fragment);
    session.step_frame(1.0 / 120.0);

    let (mut scene, _) = session.tree.build_paint_scene(200, 200);
    scene.clear_color = Color::BLACK;
    let layouts_after_first = session.tree.cached_text_layout_count();
    assert!(layouts_after_first >= 1);

    assert_clip_box_masks_pixels_outside_80(&scene);
    verify_clip_button_hit_test_and_hover_reuse(&mut session, layouts_after_first);
}

fn assert_clip_box_masks_pixels_outside_80(scene: &PaintScene) {
    let mut cpu = TinySkiaRenderer::new(200, 200, GpuResourceLimits::default());
    cpu.render_scene(scene);
    let fb = cpu.framebuffer_rgba().expect("rendered framebuffer");
    for y in 0..200_usize {
        for x in 85..200_usize {
            let idx = (y * 200 + x) * 4;
            assert_eq!(&fb[idx..idx + 3], &[0, 0, 0]);
        }
    }
}

fn verify_clip_button_hit_test_and_hover_reuse(
    session: &mut EngineSession,
    layouts_after_first: usize,
) {
    let clicked_outside_clip = Signal::new(0_i32);
    if let Some(btn_id) = session.tree.find_by_id("clip-btn") {
        let sig = clicked_outside_clip;
        session
            .tree
            .set_on_click(btn_id, Command::new(move || sig.update(|v| *v += 1)));
    }
    session.tree.dispatch_click(100.0, 20.0);
    assert_eq!(clicked_outside_clip.get(), 0);

    assert!(session.tree.dispatch_pointer_move(60.0, 20.0));
    session.step_frame(1.0 / 120.0);
    let (hover_scene, _) = session.tree.build_paint_scene(200, 200);
    assert_eq!(session.tree.cached_text_layout_count(), layouts_after_first);
    let green_text_found = hover_scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::TextGlyphs { color, .. } if *color == Color::rgb(0, 255, 0)
        )
    });
    assert!(green_text_found);
}

#[test]
fn hover_transform_and_unmount_taffy_count_stay_consistent() {
    reset_reactive_runtime();

    let mut session = EngineSession::new(300, 200, RenderBackendPreference::ForceTinySkia);
    let show_extra = Signal::new(true);
    let hover_clicked = Signal::new(0_i32);
    let on_hover_btn = Command::new(move || hover_clicked.update(|v| *v += 1));

    let fragment = krui!(
        r#"
        <style>
            .stage { width: 300px; height: 200px; }
            .hover-move { width: 80px; height: 40px; background: #3b82f6; }
            .hover-move:hover { transform: translate(60px, 0px); }
            .extra-box { width: 100px; height: 50px; padding: 10px; border-width: 2px; }
        </style>
        <div class="stage">
            <button id="hover-btn" class="hover-move" onclick="{on_hover_btn}">Move</button>
            <if test="{show_extra}">
                <div class="extra-box">
                    <span>Child 1</span>
                    <span>Child 2</span>
                </div>
            </if>
        </div>
        "#
    );
    session.tree.mount(fragment);
    session.step_frame(1.0 / 120.0);

    let taffy_with_extra = session.tree.taffy_node_count();
    show_extra.set(false);
    session.step_frame(1.0 / 120.0);
    let taffy_without_extra = session.tree.taffy_node_count();
    assert!(taffy_without_extra < taffy_with_extra);

    for _ in 0..50 {
        show_extra.set(true);
        session.step_frame(1.0 / 120.0);
        show_extra.set(false);
        session.step_frame(1.0 / 120.0);
    }
    assert_eq!(session.tree.taffy_node_count(), taffy_without_extra);

    assert!(session.tree.dispatch_pointer_move(40.0, 20.0));
    assert!(session.tree.dispatch_click(110.0, 20.0));
    assert_eq!(hover_clicked.get(), 1);
}

#[test]
fn translated_clip_and_transform_into_clip_keep_paint_and_hit_test_in_sync() {
    reset_reactive_runtime();

    let mut session = EngineSession::new(300, 200, RenderBackendPreference::ForceTinySkia);
    let hit_inside = Signal::new(0_i32);
    let on_inside = Command::new(move || hit_inside.update(|v| *v += 1));

    let fragment = krui!(
        r#"
        <style>
            .stage { width: 300px; height: 200px; background: #000000; }
            .clip-owner { width: 80px; height: 80px; overflow: hidden; transform: translate(100px, 0px); background: #111111; }
            .brought-in { margin-left: 100px; width: 40px; height: 40px; transform: translate(-80px, 0px); background: #00ff00; }
        </style>
        <div class="stage">
            <div class="clip-owner">
                <button id="outside-brought-in" class="brought-in" onclick="{on_inside}">In</button>
            </div>
        </div>
        "#
    );
    session.tree.mount(fragment);
    session.step_frame(1.0 / 120.0);

    let (mut scene, _) = session.tree.build_paint_scene(300, 200);
    scene.clear_color = Color::BLACK;

    let mut cpu = TinySkiaRenderer::new(300, 200, GpuResourceLimits::default());
    cpu.render_scene(&scene);
    let fb = cpu.framebuffer_rgba().expect("rendered framebuffer");

    let px_in = (20 * 300 + 130) * 4;
    assert_eq!(&fb[px_in..px_in + 3], &[0, 255, 0]);
    assert!(session.tree.dispatch_click(130.0, 20.0));
    assert_eq!(hit_inside.get(), 1);

    let px_orig = (20 * 300 + 30) * 4;
    assert_eq!(&fb[px_orig..px_orig + 3], &[0, 0, 0]);
    session.tree.dispatch_click(30.0, 20.0);
    assert_eq!(hit_inside.get(), 1);
}

#[test]
fn wheel_scroll_respects_ancestor_clip_and_paint_order_after_slot_reuse() {
    reset_reactive_runtime();

    let mut tree = ViewTree::new();
    let show_first = Signal::new(true);

    let fragment = krui!(
        r#"
        <div style="display: flex; flex-direction: column; width: 300px; height: 300px">
            <div style="width: 80px; height: 80px; overflow: hidden">
                <div id="clipped-scroll" style="margin-left: 120px; width: 100px; height: 60px; overflow: scroll">
                    <div style="width: 100px; min-height: 240px">Tall Clipped</div>
                </div>
            </div>
            <if test="{show_first}">
                <div id="slot-holder" style="width: 10px; height: 10px">Slot</div>
            </if>
            <div id="back-scroll" style="width: 100px; height: 60px; overflow: scroll">
                <div style="width: 100px; min-height: 240px">Back Scroll</div>
            </div>
            <div id="front-scroll" style="margin-top: -60px; width: 100px; height: 60px; overflow: scroll">
                <div style="width: 100px; min-height: 240px">Front Scroll</div>
            </div>
        </div>
        "#
    );
    tree.mount(fragment);
    let _ = tree.build_paint_scene(300, 300);

    let clipped_id = tree.find_by_id("clipped-scroll").expect("clipped-scroll");
    let back_id = tree.find_by_id("back-scroll").expect("back-scroll");
    let front_id = tree.find_by_id("front-scroll").expect("front-scroll");

    assert!(!tree.dispatch_scroll(130.0, 20.0, -40.0));
    assert_eq!(tree.node_scroll_y(clipped_id), Some(0.0));

    show_first.set(false);
    let _ = tree.build_paint_scene(300, 300);
    assert!(tree.dispatch_scroll(40.0, 110.0, -30.0));
    assert_eq!(tree.node_scroll_y(front_id), Some(30.0));
    assert_eq!(tree.node_scroll_y(back_id), Some(0.0));
}

#[test]
fn dpi_change_followed_by_ime_caret_query_then_paint_rasterizes_glyphs_at_new_dpi() {
    reset_reactive_runtime();

    let mut tree = ViewTree::new();
    let val = Signal::new("가나다".to_string());
    let fragment = krui!(
        r#"
        <div style="width: 240px; height: 120px">
            <input id="dpi-input" width="180" height="36" bind:value="{val}" />
        </div>
        "#
    );
    tree.mount(fragment);

    // 1. Initial layout & paint at 1.0x scale
    let (scene_1x, _) = tree.build_paint_scene(240, 120);
    let max_glyph_h_1x = scene_1x
        .primitives
        .iter()
        .filter_map(|p| match p {
            DrawPrimitive::TextGlyphs { glyphs, .. } => glyphs.iter().map(|g| g.height).max(),
            _ => None,
        })
        .max()
        .expect("1x glyphs");

    // 2. Focus input, change DPI to 2.0x, and query `focused_ime_cursor_area()` BEFORE `build_paint_scene`!
    let input_id = tree.find_by_id("dpi-input").expect("dpi-input");
    assert!(tree.set_focus_to_node(input_id));
    tree.set_scale_factor(2.0);
    let caret = tree
        .focused_ime_cursor_area()
        .expect("caret after set_scale_factor(2.0)");
    assert!(caret.height > 0.0);

    // 3. Subsequent `build_paint_scene` at 2.0x must emit 2.0x physical glyph bitmaps (`max_glyph_h_2x > max_glyph_h_1x`)!
    let (scene_2x, _) = tree.build_paint_scene(240, 120);
    let max_glyph_h_2x = scene_2x
        .primitives
        .iter()
        .filter_map(|p| match p {
            DrawPrimitive::TextGlyphs { glyphs, .. } => glyphs.iter().map(|g| g.height).max(),
            _ => None,
        })
        .max()
        .expect("2x glyphs");
    assert!(
        max_glyph_h_2x > max_glyph_h_1x,
        "2.0x glyph height ({max_glyph_h_2x}) must be larger than 1.0x glyph height ({max_glyph_h_1x}) even when focused_ime_cursor_area() ran before build_paint_scene()"
    );
}
