//! Wave 3 adversarial integration tests: real 4-pack design systems (Fluent 2, Material 3,
//! Liquid Glass, Cosmic zero-shadow/zero-blur), bottom indicators, state-layer preservation,
//! `:focus-visible` non-layout outline rings, container typography inheritance, `inherit`, and `currentColor`.

#![forbid(unsafe_code)]

use krui::{
    krui, Color, DrawPrimitive, GpuResourceLimits, LayoutRect, PaintScene, RendererBackend,
    TinySkiaRenderer, ViewTree,
};

fn render_rgba(scene: &PaintScene) -> (u32, Vec<u8>) {
    let mut r = TinySkiaRenderer::new(scene.width, scene.height, GpuResourceLimits::default());
    let _ = r.render_scene(scene);
    let rgba = r.framebuffer_rgba().expect("framebuffer").to_vec();
    (scene.width, rgba)
}

fn px_at(w: u32, rgba: &[u8], x: f32, y: f32) -> (u8, u8, u8, u8) {
    let ix = (x.max(0.0) as u32).min(w.saturating_sub(1)) as usize;
    let iy = y.max(0.0) as usize;
    let idx = (iy * (w as usize) + ix) * 4;
    (rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3])
}

fn base_box_for_rect(scene: &PaintScene, target: LayoutRect) -> Option<(f32, Color)> {
    scene.primitives.iter().find_map(|p| match p {
        DrawPrimitive::StyledBox {
            rect,
            border_radius,
            border_color,
            border_width,
            ..
        } if (rect.x - target.x).abs() < 1.5
            && (rect.y - target.y).abs() < 1.5
            && (rect.width - target.width).abs() < 1.5
            && (rect.height - target.height).abs() < 1.5
            && *border_width > 0.0 =>
        {
            Some((*border_radius, *border_color))
        }
        _ => None,
    })
}

fn first_glyph_color_in(scene: &PaintScene, rect: LayoutRect) -> Option<Color> {
    scene.primitives.iter().find_map(|p| match p {
        DrawPrimitive::TextGlyphs {
            origin_x,
            origin_y,
            color,
            glyphs,
            ..
        } if !glyphs.is_empty()
            && *origin_x >= rect.x - 2.0
            && *origin_x <= rect.x + rect.width + 2.0
            && *origin_y >= rect.y - 2.0
            && *origin_y <= rect.y + rect.height + 2.0 =>
        {
            Some(*color)
        }
        _ => None,
    })
}

#[test]
fn w3_runtime_data_theme_updates_radii_and_cosmic_stays_zero_shadow_zero_blur() {
    let view = krui!(
        r#"
        <style>
            [data-theme="fluent"] { --t-radius: 4px; --t-shadow: 0px 2px 6px rgba(0, 0, 0, 0.2); --t-blur: 16px; }
            [data-theme="material"] { --t-radius: 999px; --t-shadow: 0px 4px 10px rgba(0, 0, 0, 0.25); --t-blur: 0px; }
            [data-theme="glass"] { --t-radius: 14px; --t-shadow: 0px 4px 10px rgba(0, 0, 0, 0.2); --t-blur: 24px; }
            [data-theme="cosmic"] { --t-radius: 8px; --t-shadow: none; --t-blur: 0px; }
            #btn { border: 1px solid #445566; border-radius: var(--t-radius); box-shadow: var(--t-shadow); backdrop-filter: blur(var(--t-blur)); }
        </style>
        <div id="host" class="card" data-theme="fluent" style="width: 340px; padding: 16px; gap: 12px;">
            <button id="btn" style="width: 140px; height: 40px;">테마</button>
            <input id="inp" style="width: 200px; height: 36px;" value="가나다" />
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let host = tree.find_by_id("host").unwrap();
    let btn = tree.find_by_id("btn").unwrap();
    let expected = [
        ("fluent", 4.0_f32),
        ("material", 999.0),
        ("glass", 14.0),
        ("cosmic", 8.0),
    ];
    for (preset, want_radius) in expected {
        tree.set_node_attr(host, "data-theme", preset);
        let (scene, _) = tree.build_paint_scene(360, 180);
        let rb = tree.node_layout(btn).unwrap();
        let (got_radius, _) = base_box_for_rect(&scene, rb).expect("button box");
        assert!(
            (got_radius - want_radius).abs() < 0.1,
            "{preset}: expected btn radius {want_radius}, got {got_radius}"
        );
        if preset == "cosmic" {
            for prim in &scene.primitives {
                if let DrawPrimitive::StyledBox {
                    box_shadow,
                    backdrop_blur_px,
                    ..
                } = prim
                {
                    assert!(box_shadow.is_none(), "Cosmic must emit 0 box shadows");
                    assert_eq!(*backdrop_blur_px, 0.0, "Cosmic must emit 0 backdrop blur");
                }
            }
        }
    }
}

#[test]
fn w3_fluent_and_material_input_bottom_indicator_renders_1px_to_2px_with_zero_relayout() {
    for (preset, want_rest, want_focus) in [
        ("fluent", (0x99, 0x99, 0x99, 0xff), (0x47, 0x9e, 0xf5, 0xff)),
        (
            "material",
            (0xb8, 0xa9, 0xd1, 0xff),
            (0xc4, 0xb5, 0xfd, 0xff),
        ),
    ] {
        let view = krui!(
            r#"
            <style>
                [data-theme="fluent"] input { border-style: none none solid none; border-bottom: 1px solid #999999; }
                [data-theme="fluent"] input:focus { border-bottom: 2px solid #479ef5; }
                [data-theme="material"] input { border-style: none none solid none; border-bottom: 1px solid #b8a9d1; }
                [data-theme="material"] input:focus { border-bottom: 2px solid #c4b5fd; }
            </style>
            <div id="host" data-theme="fluent" style="padding: 20px; width: 320px; height: 100px;">
                <input id="inp" style="width: 220px; height: 38px;" value="테스트" />
            </div>
            "#
        );
        let mut tree = ViewTree::new();
        tree.mount(view);
        let host = tree.find_by_id("host").unwrap();
        tree.set_node_attr(host, "data-theme", preset);
        let (s_rest, _) = tree.build_paint_scene(320, 100);
        let inp = tree.find_by_id("inp").unwrap();
        let ri = tree.node_layout(inp).unwrap();
        let (rw, rest_px) = render_rgba(&s_rest);
        assert_eq!(
            px_at(rw, &rest_px, ri.x + 110.0, ri.y + ri.height - 0.5),
            want_rest
        );

        let base_relayouts = tree.relayout_passes();
        assert!(tree.dispatch_pointer_down(ri.x + 40.0, ri.y + 18.0));
        let _ = tree.dispatch_pointer_up_at(ri.x + 40.0, ri.y + 18.0);
        let (s_focus, did_relayout) = tree.build_paint_scene(320, 100);
        assert!(!did_relayout);
        assert_eq!(tree.relayout_passes(), base_relayouts);
        let (fw, focus_px) = render_rgba(&s_focus);
        assert_eq!(
            px_at(fw, &focus_px, ri.x + 110.0, ri.y + ri.height - 1.5),
            want_focus
        );
    }
}

#[test]
fn w3_state_overlay_preserves_custom_button_hue_and_focus_visible_draws_outline() {
    let view = krui!(
        r#"
        <style>
            .danger { background-color: #d83b01; width: 130px; height: 42px; }
            .danger:focus-visible { outline: 2px solid #2563eb; outline-offset: 2px; }
            .explicit { background-color: #334155; width: 130px; height: 42px; }
            .explicit:hover { background-color: #22aa44; }
        </style>
        <div style="display: flex; flex-direction: row; padding: 20px; gap: 16px; width: 360px; height: 100px;">
            <button id="danger" class="danger">삭제</button>
            <button id="explicit" class="explicit">저장</button>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let _ = tree.build_paint_scene(360, 100);
    let danger = tree.find_by_id("danger").unwrap();
    let explicit = tree.find_by_id("explicit").unwrap();
    let rd = tree.node_layout(danger).unwrap();
    let re = tree.node_layout(explicit).unwrap();

    assert!(tree.dispatch_pointer_move(rd.x + 12.0, rd.y + 12.0));
    let (s_hd, did_relayout) = tree.build_paint_scene(360, 100);
    assert!(!did_relayout);
    let (hw, hpx) = render_rgba(&s_hd);
    let (r, _, b, _) = px_at(hw, &hpx, rd.x + 10.0, rd.y + 10.0);
    assert!(
        r > 180 && r > b * 2,
        "hover overlay must preserve warm danger hue: got rgb=({r}, _, {b})"
    );

    assert!(tree.dispatch_pointer_move(re.x + 12.0, re.y + 12.0));
    let (s_he, _) = tree.build_paint_scene(360, 100);
    let (ew, epx) = render_rgba(&s_he);
    assert_eq!(
        px_at(ew, &epx, re.x + 10.0, re.y + 10.0),
        (0x22, 0xaa, 0x44, 0xff)
    );

    assert_eq!(tree.dispatch_key_tab(false), Some(danger));
    let (s_tab, tab_relayout) = tree.build_paint_scene(360, 100);
    assert!(!tab_relayout);
    assert!(s_tab.primitives.iter().any(|p| matches!(
        p,
        DrawPrimitive::StyledBox { rect, background_color, border_width, .. }
            if *background_color == Color::TRANSPARENT
                && *border_width >= 1.5
                && rect.x < rd.x
                && rect.width > rd.width + 2.0
    )));
}

#[test]
fn w3_button_child_span_inherits_typography_and_current_color_tracks_text_color() {
    let view = krui!(
        r#"
        <style>
            [data-theme="fluent"] #btn { color: #ffffff; background-color: #0078d4; }
            [data-theme="material"] #btn { color: #381e72; background-color: #d0bcff; }
            .cc-box { width: 120px; height: 38px; border: 2px solid currentColor; padding: 4px; }
            .cc-box:hover { color: #22c55e; }
        </style>
        <div id="host" data-theme="fluent" style="display: flex; flex-direction: column; gap: 10px; padding: 16px; width: 320px; height: 220px;">
            <button id="btn" style="width: 140px; height: 40px;"><span id="lbl">확인</span></button>
            <div style="color: #ef4444; gap: 8px;">
                <input id="inh" style="width: 140px; height: 34px; color: inherit;" value="상속" />
                <div id="cc" class="cc-box"><span>테두리</span></div>
            </div>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let host = tree.find_by_id("host").unwrap();
    let lbl = tree.find_by_id("lbl").unwrap();
    let inh = tree.find_by_id("inh").unwrap();
    let cc = tree.find_by_id("cc").unwrap();

    let (s_fl, _) = tree.build_paint_scene(320, 220);
    let rl = tree.node_layout(lbl).unwrap();
    let ri = tree.node_layout(inh).unwrap();
    let rc = tree.node_layout(cc).unwrap();
    assert_eq!(first_glyph_color_in(&s_fl, rl), Some(Color::WHITE));
    assert_eq!(
        first_glyph_color_in(&s_fl, ri),
        Some(Color::rgb(0xef, 0x44, 0x44))
    );
    assert_eq!(
        base_box_for_rect(&s_fl, rc).map(|b| b.1),
        Some(Color::rgb(0xef, 0x44, 0x44))
    );

    assert!(tree.dispatch_pointer_move(rc.x + 12.0, rc.y + 12.0));
    let (s_hover, did_relayout) = tree.build_paint_scene(320, 220);
    assert!(!did_relayout);
    assert_eq!(
        base_box_for_rect(&s_hover, rc).map(|b| b.1),
        Some(Color::rgb(0x22, 0xc5, 0x5e))
    );

    tree.set_node_attr(host, "data-theme", "material");
    let (s_mat, _) = tree.build_paint_scene(320, 220);
    let rl_m = tree.node_layout(lbl).unwrap();
    assert_eq!(
        first_glyph_color_in(&s_mat, rl_m),
        Some(Color::rgb(0x38, 0x1e, 0x72))
    );
}

#[test]
fn wave1_root_typography_inheritance_and_media_canvas_bg() {
    let mut tree = ViewTree::new();
    tree.set_color_scheme_preference(krui::ColorSchemePreference::Light);
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                :root {
                    color: #224466;
                    font-size: 20px;
                    font-weight: 600;
                    line-height: 1.6;
                    background-color: #fafafa;
                }
                @media (prefers-color-scheme: dark) {
                    :root { background-color: #102030; }
                }
            </style>
            <div><p id="inh">Inherited Root Typography</p></div>
            "#
        )
    });
    let (light_scene, _) = tree.build_paint_scene(240, 120);
    let p_id = tree.find_by_id("inh").expect("inh");
    let p_rect = tree.node_layout(p_id).expect("p rect");
    assert_eq!(
        first_glyph_color_in(&light_scene, p_rect),
        Some(Color::rgb(0x22, 0x44, 0x66))
    );
    let light_px = {
        let (w, fb) = render_rgba(&light_scene);
        px_at(w, &fb, 200.0, 100.0)
    };
    assert_eq!(light_px, (0xfa, 0xfa, 0xfa, 0xff));

    tree.set_color_scheme_preference(krui::ColorSchemePreference::Dark);
    let (dark_scene, _) = tree.build_paint_scene(240, 120);
    let dark_px = {
        let (w, fb) = render_rgba(&dark_scene);
        px_at(w, &fb, 200.0, 100.0)
    };
    assert_eq!(dark_px, (0x10, 0x20, 0x30, 0xff));
}
