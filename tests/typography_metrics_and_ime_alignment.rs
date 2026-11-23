#![forbid(unsafe_code)]

use krui::*;
use krui_style::{
    FontContainerFormat, FontFaceMetricsOverride, FontFaceRule, FontFaceSource, FontStyleSlope,
    FontVariationList, LineHeight, UnicodeRange,
};
use std::path::Path;

const ROBOTO_TTF_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/component_gallery/assets/fonts/Roboto-Variable.ttf"
);

fn roboto_font_face_rule(family: &str, overrides: FontFaceMetricsOverride) -> FontFaceRule {
    let bytes = std::fs::read(ROBOTO_TTF_PATH).expect("Roboto-Variable.ttf must exist");
    FontFaceRule {
        family: family.to_string(),
        sources: vec![FontFaceSource::Embedded(std::sync::Arc::from(bytes))],
        container_format: FontContainerFormat::Sfnt,
        weight_min: 100,
        weight_max: 900,
        style: FontStyleSlope::Normal,
        unicode_ranges: Vec::new(),
        metrics_override: overrides,
    }
}

fn measure_ink_vertical_bounds(
    rgba: &[u8],
    fb_w: u32,
    box_phys: (u32, u32, u32, u32),
    bg_rgb: [u8; 3],
) -> (u32, u32) {
    let (bx, by, bw, bh) = box_phys;
    let x_pad = if bw > bh + 8 { bh / 2 } else { bw / 3 };
    let y_pad = (bh / 6).max(3);
    let (mut min_y, mut max_y) = (u32::MAX, 0_u32);
    for py in (by + y_pad)..(by + bh).saturating_sub(y_pad) {
        for px in (bx + x_pad)..(bx + bw).saturating_sub(x_pad) {
            let idx = ((py * fb_w + px) * 4) as usize;
            let diff = rgba[idx].abs_diff(bg_rgb[0]) as u16
                + rgba[idx + 1].abs_diff(bg_rgb[1]) as u16
                + rgba[idx + 2].abs_diff(bg_rgb[2]) as u16;
            if diff > 45 {
                min_y = min_y.min(py);
                max_y = max_y.max(py);
            }
        }
    }
    assert!(
        min_y <= max_y,
        "expected visible glyph ink inside {box_phys:?}"
    );
    (min_y, max_y)
}

#[test]
fn a1_use_typo_metrics_negative_half_leading_and_font_face_overrides() {
    let mut engine = TextEngine::new_isolated();
    assert!(engine.register_font_faces(&[roboto_font_face_rule(
        "RobotoTest",
        FontFaceMetricsOverride::default(),
    )]));
    let base_style = TextStyle {
        font_size: 20.0,
        font_weight: 400,
        line_height: LineHeight::Normal,
        font_family: FontFamilyStack::parse_css("\"RobotoTest\""),
        ..TextStyle::default()
    };
    let normal_h = engine.measure_empty_line_height(&base_style);
    assert!(
        (normal_h - 25.0).abs() < 0.25,
        "Roboto-Variable 20px normal height must follow OS/2 sTypo metrics (25.0px, not usWin 26.38px), got {normal_h}"
    );

    let tight_style = TextStyle {
        line_height: LineHeight::Px(8.0),
        ..base_style
    };
    let tight_shaped = engine.shape_styled_text("H", &tight_style, None, 1.0);
    assert!(
        !tight_shaped.glyphs.is_empty(),
        "negative half-leading must never drop line 0"
    );
    assert!((tight_shaped.measured_height - 8.0).abs() < 1e-3);

    let ov = FontFaceMetricsOverride {
        ascent_pct: Some(0.80),
        descent_pct: Some(0.20),
        line_gap_pct: Some(0.0),
    };
    let roboto_bytes = std::sync::Arc::<[u8]>::from(std::fs::read(ROBOTO_TTF_PATH).unwrap());
    let local_or_embed = FontFaceRule {
        family: "CalibratedUI".to_string(),
        sources: vec![
            FontFaceSource::Local("Segoe UI".to_string()),
            FontFaceSource::Embedded(roboto_bytes),
        ],
        container_format: FontContainerFormat::Sfnt,
        weight_min: 100,
        weight_max: 900,
        style: FontStyleSlope::Normal,
        unicode_ranges: Vec::new(),
        metrics_override: ov,
    };
    assert!(engine.register_font_faces(&[local_or_embed]));
    let cal_style = TextStyle {
        font_family: FontFamilyStack::parse_css("\"CalibratedUI\""),
        ..base_style
    };
    let cal_h = engine.measure_empty_line_height(&cal_style);
    assert!((cal_h - 20.0).abs() < 1e-3, "expected 20.0px, got {cal_h}");
}

#[test]
fn a1_text_box_trim_centers_buttons_and_badges_at_1x_and_15x_dpi() {
    for scale in [1.0_f32, 1.5_f32] {
        verify_button_and_badge_optical_centering(scale);
    }
}

fn mount_optical_centering_tree(scale: f32) -> ViewTree {
    let mut tree = ViewTree::new();
    tree.set_scale_factor(scale);
    tree.mount_with(|| {
        krui!(
            r##"
            <style>
            @font-face {
                font-family: "Roboto";
                src: url("examples/component_gallery/assets/fonts/Roboto-Variable.ttf") format("truetype");
                font-weight: 100 900;
            }
            .wrap {
                display: flex;
                flex-direction: row;
                align-items: center;
                gap: 16px;
                padding: 24px;
                background-color: #f3edf7;
                font-family: "Roboto", "Malgun Gothic", sans-serif;
            }
            .pill-filled {
                height: 40px;
                padding: 0px 24px;
                border-radius: 999px;
                background-color: #6750a4;
                color: #ffffff;
                font-size: 14px;
                font-weight: 500;
            }
            .pill-tonal {
                height: 40px;
                padding: 0px 24px;
                border-radius: 999px;
                background-color: #e8def8;
                color: #1d192b;
                font-size: 14px;
                font-weight: 500;
            }
            .step-badge {
                display: flex;
                align-items: center;
                justify-content: center;
                width: 28px;
                height: 28px;
                border-radius: 999px;
                background-color: #322f35;
                color: #f5eff7;
                font-size: 13px;
                font-weight: 600;
                line-height: 1;
                text-box: trim-both cap alphabetic;
            }
            </style>
            <div class="wrap">
                <Button id="cap-btn" class="pill-filled">DIALOG</Button>
                <Button id="open-btn" class="pill-filled">Open basic dialog</Button>
                <Button id="micl-btn" class="pill-tonal">Toggle MICL &lt;dialog&gt;</Button>
                <Button id="kr-btn" class="pill-filled">한국어 대화상자</Button>
                <span id="badge-1" class="step-badge">1</span>
                <span id="badge-2" class="step-badge">2</span>
                <span id="badge-3" class="step-badge">3</span>
            </div>
        "##
        )
    });
    tree
}

fn verify_button_and_badge_optical_centering(scale: f32) {
    let mut tree = mount_optical_centering_tree(scale);
    let (scene, _) = tree.build_paint_scene(820, 100);
    let mut cpu = TinySkiaRenderer::new(820, 100, GpuResourceLimits::default());
    cpu.render_scene(&scene);
    let (pw, ph) = (cpu.physical_width(), cpu.physical_height());
    let fb = cpu.framebuffer_rgba().expect("cpu fb").to_vec();
    assert_all_buttons_and_badges_on_fb(&tree, &fb, pw, scale);

    if let Some(mut gpu) = WgpuRenderer::try_new(820, 100, GpuResourceLimits::default()) {
        gpu.render_scene(&scene);
        let gpw = gpu.target_size().0;
        if let Some(gfb) = gpu.framebuffer_rgba() {
            assert_all_buttons_and_badges_on_fb(&tree, gfb, gpw, scale);
        }
    }
    maybe_dump_alignment_png(scale, &fb, pw, ph);
}

fn assert_all_buttons_and_badges_on_fb(tree: &ViewTree, fb: &[u8], pw: u32, scale: f32) {
    assert_cap_centered_in_node(tree, fb, pw, scale, "cap-btn", [0x67, 0x50, 0xa4], 2.0);
    assert_cap_centered_in_node(tree, fb, pw, scale, "kr-btn", [0x67, 0x50, 0xa4], 4.0);
    for bid in ["badge-1", "badge-2", "badge-3"] {
        assert_cap_centered_in_node(tree, fb, pw, scale, bid, [0x32, 0x2f, 0x35], 2.5);
    }
    assert_top_gap_matches_cap_ref(
        tree,
        fb,
        pw,
        scale,
        "cap-btn",
        "open-btn",
        [0x67, 0x50, 0xa4],
    );
    assert_top_gap_matches_cap_ref(
        tree,
        fb,
        pw,
        scale,
        "cap-btn",
        "micl-btn",
        [0xe8, 0xde, 0xf8],
    );
}

fn assert_cap_centered_in_node(
    tree: &ViewTree,
    fb: &[u8],
    pw: u32,
    scale: f32,
    id: &str,
    bg: [u8; 3],
    tol_phys: f32,
) {
    let r = tree
        .node_layout(tree.find_by_id(id).expect(id))
        .expect("rect");
    let (bx, by) = ((r.x * scale).round() as u32, (r.y * scale).round() as u32);
    let (bw, bh) = (
        (r.width * scale).round() as u32,
        (r.height * scale).round() as u32,
    );
    let (ink_top, ink_bot) = measure_ink_vertical_bounds(fb, pw, (bx, by, bw, bh), bg);
    let top_gap = (ink_top - by) as f32;
    let bot_gap = ((by + bh - 1) - ink_bot) as f32;
    assert!(
        (top_gap - bot_gap).abs() <= tol_phys,
        "[{id} @{scale}x] optical gap asymmetry: top_gap={top_gap}, bot_gap={bot_gap}"
    );
}

fn assert_top_gap_matches_cap_ref(
    tree: &ViewTree,
    fb: &[u8],
    pw: u32,
    scale: f32,
    ref_id: &str,
    target_id: &str,
    target_bg: [u8; 3],
) {
    let rr = tree.node_layout(tree.find_by_id(ref_id).unwrap()).unwrap();
    let tr = tree
        .node_layout(tree.find_by_id(target_id).unwrap())
        .unwrap();
    let r_box = (
        (rr.x * scale) as u32,
        (rr.y * scale) as u32,
        (rr.width * scale) as u32,
        (rr.height * scale) as u32,
    );
    let t_box = (
        (tr.x * scale) as u32,
        (tr.y * scale) as u32,
        (tr.width * scale) as u32,
        (tr.height * scale) as u32,
    );
    let (r_top, _) = measure_ink_vertical_bounds(fb, pw, r_box, [0x67, 0x50, 0xa4]);
    let (t_top, _) = measure_ink_vertical_bounds(fb, pw, t_box, target_bg);
    let ref_gap = r_top as f32 - (rr.y * scale).round();
    let tgt_gap = t_top as f32 - (tr.y * scale).round();
    assert!(
        (ref_gap - tgt_gap).abs() <= 2.0,
        "[{target_id} @{scale}x] {tgt_gap} vs {ref_gap}"
    );
}

fn maybe_dump_alignment_png(scale: f32, fb: &[u8], pw: u32, ph: u32) {
    if let Ok(dir) = std::env::var("KRUI_DUMP_ALIGNMENT_PNG_DIR") {
        let p = Path::new(&dir);
        let _ = std::fs::create_dir_all(p);
        let tag = (scale * 100.0).round() as u32;
        let out = p.join(format!("buttons_and_badges_{tag}pct.png"));
        image::save_buffer(out, fb, pw, ph, image::ColorType::Rgba8).expect("save png");
    }
}

#[test]
fn a2_line_height_units_inheritance_content_box_and_zero_padding() {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
            :root { --custom-lh: 26px; --pct-lh: 150%; }
            .unitless-parent { font-size: 16px; line-height: 1.5; }
            .pct-parent { font-size: 16px; line-height: 150%; }
            .var-pct-parent { font-size: 16px; line-height: var(--pct-lh); }
            .em-parent { font-size: 16px; line-height: 1.5em; }
            .big-child { font-size: 24px; }
            .calc-lh { font-size: 16px; line-height: calc(1em + 6px); }
            .rem-lh { line-height: 2rem; }
            .var-lh { line-height: var(--custom-lh); }
            .zero-btn { padding: 0px; border: 0px solid transparent; line-height: 20px; text-box: normal; }
            .asym-btn {
                height: 50px;
                padding: 20px 10px 4px 10px;
                border-top: 0px solid transparent;
                border-bottom: 6px solid #ff0000;
                line-height: 20px;
                text-box: normal;
            }
            .krui-ui-btn { padding: 0px; min-height: 0px; line-height: 18px; text-box: normal; }
            </style>
            <div direction="column" style="align-items: flex-start;">
                <div class="unitless-parent"><span id="inh-factor" class="big-child">A</span></div>
                <div class="pct-parent"><span id="inh-pct" class="big-child">A</span></div>
                <div class="var-pct-parent"><span id="inh-var-pct" class="big-child">A</span></div>
                <div class="em-parent"><span id="inh-em" class="big-child">A</span></div>
                <span id="lh-calc" class="calc-lh">A</span>
                <span id="lh-rem" class="rem-lh">A</span>
                <span id="lh-var" class="var-lh">A</span>
                <button id="zero-pad" class="zero-btn">A</button>
                <button id="asym-pad" class="asym-btn">A</button>
                <Button id="layer-override">A</Button>
            </div>
        "#
        )
    });
    let (scene, _) = tree.build_paint_scene(600, 500);
    let h = |id: &str| {
        tree.node_layout(tree.find_by_id(id).expect(id))
            .unwrap()
            .height
    };
    assert!((h("inh-factor") - 36.0).abs() < 0.5);
    assert!((h("inh-pct") - 24.0).abs() < 0.5);
    assert!((h("inh-var-pct") - 24.0).abs() < 0.5);
    assert!((h("inh-em") - 24.0).abs() < 0.5);
    assert!((h("lh-calc") - 22.0).abs() < 0.5);
    assert!((h("lh-rem") - 32.0).abs() < 0.5);
    assert!((h("lh-var") - 26.0).abs() < 0.5);
    assert!((h("zero-pad") - 20.0).abs() < 0.5);
    assert!((h("layer-override") - 18.0).abs() < 0.5);

    let asym_rect = tree
        .node_layout(tree.find_by_id("asym-pad").unwrap())
        .unwrap();
    let asym_text_y = scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::TextGlyphs { origin_y, .. }
                if *origin_y >= asym_rect.y && *origin_y < asym_rect.y + asym_rect.height =>
            {
                Some(*origin_y)
            }
            _ => None,
        })
        .expect("asym-pad TextGlyphs");
    assert!((asym_text_y - (asym_rect.y + 20.0)).abs() < 0.5);
}

fn register_dual_script_engine() -> TextEngine {
    let mut engine = TextEngine::new_isolated();
    let bytes = std::sync::Arc::<[u8]>::from(std::fs::read(ROBOTO_TTF_PATH).unwrap());
    let latin_face = FontFaceRule {
        family: "DualScript".to_string(),
        sources: vec![FontFaceSource::Embedded(bytes.clone())],
        container_format: FontContainerFormat::Sfnt,
        weight_min: 100,
        weight_max: 900,
        style: FontStyleSlope::Normal,
        unicode_ranges: UnicodeRange::parse_css_list("U+0000-00FF").unwrap(),
        metrics_override: FontFaceMetricsOverride::default(),
    };
    let cjk_face = FontFaceRule {
        family: "DualScript".to_string(),
        sources: vec![
            FontFaceSource::Local("Malgun Gothic".to_string()),
            FontFaceSource::Embedded(bytes),
        ],
        container_format: FontContainerFormat::Sfnt,
        weight_min: 100,
        weight_max: 900,
        style: FontStyleSlope::Normal,
        unicode_ranges: UnicodeRange::parse_css_list("U+AC00-D7AF").unwrap(),
        metrics_override: FontFaceMetricsOverride::default(),
    };
    assert!(engine.register_font_faces(&[latin_face, cjk_face]));
    engine
}

#[test]
fn a3_font_family_order_variable_axes_generic_families_and_atlas_key() {
    let mut engine = register_dual_script_engine();

    let s450 = TextStyle {
        font_size: 28.0,
        font_weight: 450,
        font_family: FontFamilyStack::parse_css("\"DualScript\", system-ui, emoji"),
        ..TextStyle::default()
    };
    let g450 = engine.shape_styled_text("MW", &s450, None, 1.0);
    let g451 = engine.shape_styled_text(
        "MW",
        &TextStyle {
            font_weight: 451,
            ..s450
        },
        None,
        1.0,
    );
    let g850 = engine.shape_styled_text(
        "MW",
        &TextStyle {
            font_weight: 850,
            ..s450
        },
        None,
        1.0,
    );
    assert_eq!(g450.glyphs[0].cache_key, g451.glyphs[0].cache_key);
    assert_ne!(g450.glyphs[0].cache_key, g850.glyphs[0].cache_key);
    let ink = |layout: &ShapedTextLayout| -> u64 {
        layout
            .glyphs
            .iter()
            .flat_map(|g| g.alpha_mask.iter())
            .map(|&a| u64::from(a))
            .sum()
    };
    assert!(
        ink(&g850) > ink(&g450),
        "wght 850 ink ({}) must exceed wght 450 ({})",
        ink(&g850),
        ink(&g450)
    );

    let narrow = TextStyle {
        variations: FontVariationList::parse_css("\"wdth\" 75").expect("parse wdth"),
        ..s450
    };
    assert!(
        engine
            .shape_styled_text("MW", &narrow, None, 1.0)
            .measured_width
            < g450.measured_width
    );

    for generic in [
        "system-ui",
        "ui-sans-serif",
        "ui-serif",
        "ui-monospace",
        "sans-serif",
        "serif",
        "monospace",
        "emoji",
    ] {
        let gs = TextStyle {
            font_family: FontFamilyStack::parse_css(generic),
            ..s450
        };
        assert!(!engine
            .shape_styled_text("A", &gs, None, 1.0)
            .glyphs
            .is_empty());
    }
}

#[cfg(feature = "preview")]
#[test]
fn macro_and_preview_typography_render_identically() {
    let src = r##"
        <style>
        @font-face {
            font-family: "Roboto";
            src: url("examples/component_gallery/assets/fonts/Roboto-Variable.ttf") format("truetype");
            font-weight: 100 900;
        }
        .row { display: flex; align-items: center; gap: 12px; padding: 16px; background-color: #f3edf7; font-family: "Roboto", "Malgun Gothic", sans-serif; }
        .btn { height: 40px; padding: 0px 24px; border-radius: 999px; background-color: #6750a4; color: #ffffff; font-size: 14px; font-weight: 500; line-height: 1.4em; text-box: trim-both cap alphabetic; }
        </style>
        <div class="row"><button class="btn">Open basic dialog</button><button class="btn">한국어 버튼</button></div>
    "##;
    let mut macro_tree = ViewTree::new();
    macro_tree.set_scale_factor(1.5);
    macro_tree.mount_with(|| {
        krui!(
            r##"
            <style>
            @font-face {
                font-family: "Roboto";
                src: url("examples/component_gallery/assets/fonts/Roboto-Variable.ttf") format("truetype");
                font-weight: 100 900;
            }
            .row { display: flex; align-items: center; gap: 12px; padding: 16px; background-color: #f3edf7; font-family: "Roboto", "Malgun Gothic", sans-serif; }
            .btn { height: 40px; padding: 0px 24px; border-radius: 999px; background-color: #6750a4; color: #ffffff; font-size: 14px; font-weight: 500; line-height: 1.4em; text-box: trim-both cap alphabetic; }
            </style>
            <div class="row"><button class="btn">Open basic dialog</button><button class="btn">한국어 버튼</button></div>
        "##
        )
    });
    let doc = krui::preview::compile_krui_source(src).expect("preview compile");
    let mut prev_tree = ViewTree::new();
    prev_tree.set_scale_factor(1.5);
    let _ = krui::preview::mount_compiled_document(&mut prev_tree, &doc);

    let (s_mac, _) = macro_tree.build_paint_scene(420, 80);
    let (s_prv, _) = prev_tree.build_paint_scene(420, 80);
    let mut r_mac = TinySkiaRenderer::new(420, 80, GpuResourceLimits::default());
    let mut r_prv = TinySkiaRenderer::new(420, 80, GpuResourceLimits::default());
    r_mac.render_scene(&s_mac);
    r_prv.render_scene(&s_prv);
    assert_eq!(
        r_mac.framebuffer_rgba().unwrap(),
        r_prv.framebuffer_rgba().unwrap()
    );
}
