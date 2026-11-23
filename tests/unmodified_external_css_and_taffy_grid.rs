#![forbid(unsafe_code)]

use krui::*;

fn sample_rgba_at(fb: &[u8], stride_px: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * stride_px + x) * 4) as usize;
    [fb[idx], fb[idx + 1], fb[idx + 2], fb[idx + 3]]
}

fn find_box_for_size(scene: &PaintScene, w: f32, h: f32) -> Option<(LayoutRect, Color, Affine2)> {
    scene.primitives.iter().find_map(|p| match p {
        DrawPrimitive::StyledBox {
            rect,
            background_color,
            transform,
            ..
        } if (rect.width - w).abs() < 1.5 && (rect.height - h).abs() < 1.5 => {
            Some((*rect, *background_color, *transform))
        }
        _ => None,
    })
}

fn assert_grad_stops_on_fb(fb: &[u8], stride: u32, r: LayoutRect) {
    let cy = (r.y + r.height * 0.5).round() as u32;
    let lx = (r.x + 12.0).round() as u32;
    let mx = (r.x + r.width * 0.5).round() as u32;
    let rx = (r.x + r.width - 12.0).round() as u32;
    let left_px = sample_rgba_at(fb, stride, lx, cy);
    let mid_px = sample_rgba_at(fb, stride, mx, cy);
    let right_px = sample_rgba_at(fb, stride, rx, cy);
    assert!(
        left_px[0] > left_px[1] && left_px[0] > left_px[2],
        "left stop must be red-dominant, got {left_px:?}"
    );
    assert!(
        mid_px[1] > mid_px[0] && mid_px[1] > mid_px[2],
        "middle stop (50%) must be green-dominant, got {mid_px:?}"
    );
    assert!(
        right_px[2] > right_px[0] && right_px[2] > right_px[1],
        "right stop must be blue-dominant, got {right_px:?}"
    );
}

fn mount_unmodified_css_fixture() -> ViewTree {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                @layer reset, components;
                @layer reset {
                    #grid { width: 400px; }
                }
                @layer components {
                    @supports (float: left) {
                        #grid { width: 10px; }
                    }
                    @supports (display: grid) {
                        #grid {
                            display: grid;
                            width: 600px;
                            margin: 0 auto;
                            padding: 5%;
                            box-sizing: border-box;
                            place-items: center;
                            gap: 10px;
                            grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
                        }
                    }
                    .card {
                        --card-bg: #1e293b;
                        background: var(--card-bg);
                        width: 120px;
                        height: 44px;
                        &:hover {
                            --card-bg: #3b82f6;
                        }
                    }
                    .span-two {
                        grid-column: span 2;
                        justify-self: stretch;
                        flex: 1 1 120px;
                        height: 44px;
                        background-color: #64748b;
                    }
                    #grad-box {
                        width: 240px;
                        height: 40px;
                        background-image: linear-gradient(90deg, #ef4444, #22c55e 50%, #3b82f6);
                        transform: skewX(-8deg);
                    }
                }
            </style>
            <div style="width: 100%; display: flex; flex-direction: column;">
                <div id="grid">
                    <div id="card" class="card">Hover Var</div>
                    <div id="span2" class="span-two">Span 2</div>
                    <div id="c3" class="card">C3</div>
                </div>
                <div id="grad-box"></div>
            </div>
        "#
        )
    });
    tree
}

#[test]
fn unmodified_css_nesting_layers_grid_auto_fit_and_hover_custom_props_zero_relayout() {
    let mut tree = mount_unmodified_css_fixture();
    let (scene, did_layout) = tree.build_paint_scene(800, 600);
    assert!(did_layout);
    let baseline_passes = tree.relayout_passes();

    let grid_id = tree.find_by_id("grid").expect("grid");
    let grid_r = tree.node_layout(grid_id).expect("grid rect");
    assert!(
        (grid_r.width - 600.0).abs() < 0.5 && (grid_r.x - 100.0).abs() < 0.5,
        "margin: 0 auto and @supports(display: grid) must center 600px grid at x=100, got {grid_r:?}"
    );

    let span2_id = tree.find_by_id("span2").expect("span2");
    let span2_r = tree.node_layout(span2_id).expect("span2 rect");
    assert!(
        span2_r.width > 200.0,
        "grid-column: span 2 must span multiple auto-fit tracks at 600px, got {span2_r:?}"
    );

    let card_id = tree.find_by_id("card").expect("card");
    let card_r = tree.node_layout(card_id).expect("card rect");
    let (_, base_bg, _) = find_box_for_size(&scene, 120.0, 44.0).expect("card base box");
    assert_eq!(base_bg, Color::rgb(0x1e, 0x29, 0x3b));

    assert!(tree.dispatch_pointer_move(card_r.x + 12.0, card_r.y + 12.0));
    let (hover_scene, hover_layout) = tree.build_paint_scene(800, 600);
    assert!(
        !hover_layout && tree.relayout_passes() == baseline_passes,
        ":hover --card-bg override must run in 0 Taffy relayout passes"
    );
    let (_, hover_bg, _) = find_box_for_size(&hover_scene, 120.0, 44.0).expect("card hover box");
    assert_eq!(hover_bg, Color::rgb(0x3b, 0x82, 0xf6));

    let grad_id = tree.find_by_id("grad-box").expect("grad-box");
    let (grad_r, _, grad_tf) =
        find_box_for_size(&hover_scene, 240.0, 40.0).expect("grad StyledBox");
    assert!(
        grad_tf.b.abs() > 0.05 || grad_tf.c.abs() > 0.05,
        "skewX(-8deg) must populate shear term, got {grad_tf:?}"
    );
    let mut cpu = TinySkiaRenderer::new(800, 600, GpuResourceLimits::default());
    cpu.render_scene(&hover_scene);
    let fb = cpu.framebuffer_rgba().expect("cpu fb");
    assert_grad_stops_on_fb(fb, 800, grad_r);
    if let Some(mut gpu) = WgpuRenderer::try_new(800, 600, GpuResourceLimits::default()) {
        gpu.render_scene(&hover_scene);
        let gpw = gpu.target_size().0;
        if let Some(gfb) = gpu.framebuffer_rgba() {
            assert_grad_stops_on_fb(gfb, gpw, grad_r);
        }
    }
    let _ = tree.node_layout(grad_id);
}

#[test]
fn rich_inline_paragraph_collapses_and_matches_macro_and_preview() {
    let mut macro_tree = ViewTree::new();
    macro_tree.set_scale_factor(1.5);
    macro_tree.mount_with(|| {
        krui!(
            r#"
            <style>
                .wrap { width: 220px; padding: 8px; }
                #rich { color: #111827; font-size: 16px; line-height: 1.4; }
            </style>
            <div class="wrap">
                <p id="rich">한글 문단 안에서 <b>굵은 강조</b>와 <span style="color: #dc2626">빨강</span>과 <code>inline_code</code>가 줄바꿈됩니다.</p>
            </div>
        "#
        )
    });
    let (s_mac, _) = macro_tree.build_paint_scene(260, 180);
    let rich_id = macro_tree.find_by_id("rich").expect("rich");
    let rich_rect = macro_tree.node_layout(rich_id).expect("rich rect");
    assert!(
        rich_rect.height > 30.0,
        "wrapped rich paragraph must span multiple lines, got {rich_rect:?}"
    );
    let glyphs = s_mac
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::TextGlyphs { glyphs, .. } if glyphs.len() > 10 => Some(glyphs.clone()),
            _ => None,
        })
        .expect("collapsed rich TextGlyphs primitive");
    let distinct_spans: std::collections::HashSet<u16> =
        glyphs.iter().map(|g| g.span_index).collect();
    assert!(
        distinct_spans.len() >= 4,
        "rich paragraph must retain distinct span indices across text/b/span/code, got {distinct_spans:?}"
    );
    assert!(
        glyphs
            .iter()
            .any(|g| g.glyph_color == Some(Color::rgb(0xdc, 0x26, 0x26))),
        "per-span inline color must propagate to RasterizedGlyph::glyph_color"
    );

    #[cfg(feature = "preview")]
    {
        let src = r#"
            <style>
                .wrap { width: 220px; padding: 8px; }
                #rich { color: #111827; font-size: 16px; line-height: 1.4; }
            </style>
            <div class="wrap">
                <p id="rich">한글 문단 안에서 <b>굵은 강조</b>와 <span style="color: #dc2626">빨강</span>과 <code>inline_code</code>가 줄바꿈됩니다.</p>
            </div>
        "#;
        let doc = krui::preview::compile_krui_source(src).expect("preview compile");
        let mut prev_tree = ViewTree::new();
        prev_tree.set_scale_factor(1.5);
        let _ = krui::preview::mount_compiled_document(&mut prev_tree, &doc);
        let (s_prv, _) = prev_tree.build_paint_scene(260, 180);
        let mut r_mac = TinySkiaRenderer::new(260, 180, GpuResourceLimits::default());
        let mut r_prv = TinySkiaRenderer::new(260, 180, GpuResourceLimits::default());
        r_mac.render_scene(&s_mac);
        r_prv.render_scene(&s_prv);
        assert_eq!(
            r_mac.framebuffer_rgba().unwrap(),
            r_prv.framebuffer_rgba().unwrap(),
            "krui! macro and krui-preview must produce byte-identical pixels for rich paragraphs"
        );
    }
}

#[test]
fn display_contents_promotes_grid_items_and_position_fixed_ignores_parent_scroll() {
    reset_reactive_runtime();
    let hud_clicks = Signal::new(0_u32);
    let on_hud = move || hud_clicks.update(|v| *v += 1);

    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                #scroller {
                    width: 800px;
                    height: 200px;
                    overflow-y: scroll;
                    display: flex;
                    flex-direction: column;
                }
                #grid-host {
                    width: 400px;
                    flex-shrink: 0;
                    display: grid;
                    grid-template-columns: 1fr 1fr;
                }
                #tall-spacer { width: 100px; height: 800px; flex-shrink: 0; }
                #hud {
                    position: fixed;
                    top: 12px;
                    right: 16px;
                    width: 120px;
                    height: 36px;
                    background-color: #2563eb;
                }
            </style>
            <div id="scroller">
                <div id="grid-host">
                    <div style="display: contents;">
                        <div id="col-a" style="height: 40px;">A</div>
                        <div id="col-b" style="height: 40px;">B</div>
                    </div>
                </div>
                <div id="tall-spacer">spacer</div>
                <button id="hud" onclick="{on_hud}">HUD</button>
            </div>
        "#
        )
    });

    let _ = tree.build_paint_scene(800, 600);

    let col_a = tree
        .node_layout(tree.find_by_id("col-a").expect("col-a"))
        .expect("rect a");
    let col_b = tree
        .node_layout(tree.find_by_id("col-b").expect("col-b"))
        .expect("rect b");
    assert!(
        (col_a.x - 0.0).abs() < 0.5
            && (col_a.width - 200.0).abs() < 0.5
            && (col_b.x - 200.0).abs() < 0.5
            && (col_b.width - 200.0).abs() < 0.5,
        "display: contents must promote children directly into 2-col 400px Grid: a={col_a:?}, b={col_b:?}"
    );

    let scroller_id = tree.find_by_id("scroller").expect("scroller");
    assert!(tree.set_scroll_offsets(scroller_id, 0.0, 250.0));
    let (scrolled_scene, _) = tree.build_paint_scene(800, 600);
    let (hud_rect, _, _) =
        find_box_for_size(&scrolled_scene, 120.0, 36.0).expect("fixed hud StyledBox");
    assert!(
        (hud_rect.x - 664.0).abs() < 0.5 && (hud_rect.y - 12.0).abs() < 0.5,
        "position: fixed must stay pinned to viewport (664, 12) after parent scroll_y=250: {hud_rect:?}"
    );

    assert!(tree.dispatch_click(664.0 + 60.0, 12.0 + 18.0));
    assert_eq!(hud_clicks.get(), 1);
}
