#![forbid(unsafe_code)]

use krui::*;

fn new_cpu_session(w: u32, h: u32) -> EngineSession {
    reset_reactive_runtime();
    EngineSession::new(w, h, RenderBackendPreference::CpuFallback)
}

fn px_at(fb: &[u8], stride_w: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * stride_w + x) * 4) as usize;
    [fb[idx], fb[idx + 1], fb[idx + 2], fb[idx + 3]]
}

fn weighted_ink_centroid(fb: &[u8], stride_w: u32, rect: LayoutRect) -> (f64, f64, f64) {
    let x0 = (rect.x.floor() as u32).saturating_add(2);
    let y0 = (rect.y.floor() as u32).saturating_add(2);
    let x1 = ((rect.x + rect.width).ceil() as u32).saturating_sub(2);
    let y1 = ((rect.y + rect.height).ceil() as u32).saturating_sub(2);
    let (mut sum_w, mut sum_x, mut sum_y) = (0.0_f64, 0.0_f64, 0.0_f64);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = px_at(fb, stride_w, x, y);
            let lum = (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0;
            if lum > 90.0 {
                let w = lum - 60.0;
                sum_w += w;
                sum_x += (x as f64 + 0.5) * w;
                sum_y += (y as f64 + 0.5) * w;
            }
        }
    }
    (sum_x / sum_w, sum_y / sum_w, sum_w)
}

#[test]
fn circular_step_badge_centers_digits_geometrically_and_optically() {
    let mut sess = new_cpu_session(200, 120);
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                .wrap {
                    width: 200px; height: 120px; background-color: #FEF7FF;
                    display: flex; flex-direction: row; gap: 16px; padding: 20px;
                }
                .badge {
                    width: 28px; height: 28px; border-radius: 999px;
                    display: flex; align-items: center; justify-content: center;
                    background-color: #322F35; color: #F5EFF7;
                    font-size: 13px; font-weight: 600; line-height: 1; text-align: center;
                }
            </style>
            <div class="wrap">
                <div id="b1" class="badge"><span>1</span></div>
                <div id="b2" class="badge">2</div>
            </div>
        "#
        )
    });
    let _ = sess.step_frame(1.0 / 120.0);
    let r1 = sess
        .tree
        .node_layout(sess.tree.find_by_id("b1").unwrap())
        .unwrap();
    let r2 = sess
        .tree
        .node_layout(sess.tree.find_by_id("b2").unwrap())
        .unwrap();
    let fb = sess.renderer.framebuffer_rgba().expect("cpu rgba").to_vec();
    for (label, rect) in [("span_1", r1), ("direct_2", r2)] {
        let (cx, cy, w) = weighted_ink_centroid(&fb, 200, rect);
        assert!(w > 1000.0, "{label} must render visible ink");
        let (tx, ty) = (rect.x as f64 + 14.0, rect.y as f64 + 14.0);
        let (dx, dy) = ((cx - tx).abs(), (cy - ty).abs());
        assert!(
            dx <= 1.25 && dy <= 1.25,
            "{label} badge center offset too large: cx={cx:.2} cy={cy:.2} target=({tx:.1},{ty:.1}) dx={dx:.2} dy={dy:.2}"
        );
    }
}

#[test]
fn dynamic_class_partial_border_and_corner_preserve_other_sides() {
    let mut sess = new_cpu_session(400, 180);
    let active = Signal::new(true);
    let edge_open = Signal::new(true);
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                .tab { height: 36px; padding: 0px 14px; border-bottom: 2px solid transparent; }
                .tab.sel-true { border-bottom: 2px solid #6750A4; }
                .btn {
                    width: 100px; height: 36px; border: 2px solid #79747E;
                    border-radius: 6px 20px 20px 6px;
                }
                .btn.open-true { border-left-width: 0px; border-top-left-radius: 20px; }
                .btn-in { width: 10px; height: 10px; }
            </style>
            <div>
                <div id="tab" class="tab sel-{active}">개요</div>
                <div id="btn" class="btn open-{edge_open}"><div id="btn_in" class="btn-in"></div></div>
            </div>
        "#
        )
    });
    let _ = sess.step_frame(1.0 / 120.0);
    let tab_id = sess.tree.find_by_id("tab").unwrap();
    let btn_id = sess.tree.find_by_id("btn").unwrap();
    let in_id = sess.tree.find_by_id("btn_in").unwrap();
    let tab_rect = sess.tree.node_layout(tab_id).unwrap();
    let tab_paint = sess.tree.node_effective_paint(tab_id).unwrap();
    assert!(tab_rect.width > 36.0, "tab must not collapse: {tab_rect:?}");
    assert_eq!(tab_paint.raw_border_width, 2.0);
    assert_eq!(
        tab_paint.border_side_colors.map(|c| c[2]),
        Some(Color::rgb(0x67, 0x50, 0xA4))
    );

    let (rb, ri) = (
        sess.tree.node_layout(btn_id).unwrap(),
        sess.tree.node_layout(in_id).unwrap(),
    );
    assert!((ri.x - rb.x).abs() < 0.2 && (ri.y - rb.y - 2.0).abs() < 0.2);
    assert_eq!(
        sess.tree.node_effective_paint(btn_id).unwrap().corner_radii,
        CornerRadii {
            top_left: 20.0,
            top_right: 20.0,
            bottom_right: 20.0,
            bottom_left: 6.0
        }
    );
    active.set(false);
    edge_open.set(false);
    let _ = sess.step_frame(1.0 / 120.0);
    let (rb2, ri2) = (
        sess.tree.node_layout(btn_id).unwrap(),
        sess.tree.node_layout(in_id).unwrap(),
    );
    assert!((ri2.x - rb2.x - 2.0).abs() < 0.2);
    assert_eq!(
        sess.tree
            .node_effective_paint(tab_id)
            .unwrap()
            .border_side_colors
            .map(|c| c[2]),
        Some(Color::TRANSPARENT)
    );
    assert_eq!(
        sess.tree
            .node_effective_paint(btn_id)
            .unwrap()
            .corner_radii
            .top_left,
        6.0
    );
}

#[cfg(feature = "preview")]
#[test]
fn preview_interpreter_matches_macro_on_dynamic_partial_border_patches() {
    use krui::dev_hot_reload::{compile_krui_source, mount_compiled_document};
    let doc = compile_krui_source(
        r#"
        <style>
            .t { height: 36px; border-bottom: 2px solid transparent; }
            .t.on-true { border-bottom: 2px solid #6750A4; }
        </style>
        <div id="tab" class="t on-true">Tab</div>
    "#,
    )
    .expect("compile partial border doc");
    let mut sess = new_cpu_session(320, 120);
    mount_compiled_document(&mut sess.tree, &doc);
    let _ = sess.step_frame(1.0 / 120.0);
    let tid = sess.tree.find_by_id("tab").unwrap();
    assert!(sess.tree.node_layout(tid).unwrap().width > 24.0);
    assert_eq!(
        sess.tree
            .node_effective_paint(tid)
            .unwrap()
            .raw_border_width,
        2.0
    );
}

#[test]
fn button_default_center_and_css_text_align_left_override() {
    let mut sess = new_cpu_session(400, 140);
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                .col { display: flex; flex-direction: column; gap: 8px; padding: 10px; }
                .bc { width: 180px; height: 36px; padding: 8px 16px; }
                .bl { width: 180px; height: 36px; padding: 8px 16px; text-align: left; }
            </style>
            <div class="col">
                <button id="btn_c" class="bc">확인</button>
                <button id="btn_l" class="bl">개요</button>
            </div>
        "#
        )
    });
    let _ = sess.step_frame(1.0 / 120.0);
    let rc = sess
        .tree
        .node_layout(sess.tree.find_by_id("btn_c").unwrap())
        .unwrap();
    let rl = sess
        .tree
        .node_layout(sess.tree.find_by_id("btn_l").unwrap())
        .unwrap();
    let (scene, _) = sess.tree.build_paint_scene(400, 140);
    let mut text_xs: Vec<f32> = scene
        .primitives
        .iter()
        .filter_map(|p| match p {
            DrawPrimitive::TextGlyphs { origin_x, .. } => Some(*origin_x),
            _ => None,
        })
        .collect();
    assert_eq!(text_xs.len(), 2);
    let (xc, xl) = (text_xs.remove(0), text_xs.remove(0));
    assert!(
        xc > rc.x + 55.0,
        "default button must center text: xc={xc}, rc={rc:?}"
    );
    assert!(
        xl >= rl.x + 15.0 && xl <= rl.x + 18.0,
        "text-align: left button must start at padding-left: xl={xl}, rl={rl:?}"
    );
}

#[test]
fn rounded_per_side_borders_render_without_square_corner_bleed() {
    let mut sess = new_cpu_session(240, 120);
    sess.tree.mount_with(|| {
        krui!(
            r#"
            <style>
                .bg {
                    width: 240px; height: 120px; background-color: #FEF7FF;
                    display: flex; flex-direction: row; align-items: center; gap: 24px; padding: 20px;
                }
                .spin {
                    width: 40px; height: 40px; border-radius: 999px;
                    border-top: 4px solid #6750A4; border-right: 4px solid #625B71;
                    border-bottom: 4px solid transparent; border-left: 4px solid #6750A4;
                }
                .grp-end {
                    width: 60px; height: 36px; background-color: #E8DEF8;
                    border: 2px solid #6750A4; border-left-width: 0px;
                    border-radius: 0px 18px 18px 0px;
                }
            </style>
            <div class="bg">
                <div id="sp" class="spin"></div>
                <div id="ge" class="grp-end"></div>
            </div>
        "#
        )
    });
    let _ = sess.step_frame(1.0 / 120.0);
    let rsp = sess
        .tree
        .node_layout(sess.tree.find_by_id("sp").unwrap())
        .unwrap();
    let rge = sess
        .tree
        .node_layout(sess.tree.find_by_id("ge").unwrap())
        .unwrap();
    let fb = sess.renderer.framebuffer_rgba().expect("cpu rgba").to_vec();
    let corner_sp = px_at(&fb, 240, rsp.x as u32 + 1, rsp.y as u32 + 1);
    assert_eq!(
        corner_sp,
        [0xFE, 0xF7, 0xFF, 255],
        "spinner top-left outside radius must stay clear"
    );
    let top_sp = px_at(&fb, 240, rsp.x as u32 + 20, rsp.y as u32 + 2);
    assert_eq!(
        top_sp,
        [0x67, 0x50, 0xA4, 255],
        "spinner top arc must paint accent border"
    );
    let corner_ge = px_at(&fb, 240, (rge.x + rge.width) as u32 - 2, rge.y as u32 + 1);
    assert_eq!(
        corner_ge,
        [0xFE, 0xF7, 0xFF, 255],
        "rounded right corner must stay clear"
    );
    let left_mid_ge = px_at(&fb, 240, rge.x as u32 + 1, rge.y as u32 + 18);
    assert_eq!(
        left_mid_ge,
        [0xE8, 0xDE, 0xF8, 255],
        "border-left-width: 0px must not paint border on left"
    );
}

#[test]
fn pure_css_themes_populate_tokens_under_unstyled_preset() {
    let mat_css = include_str!("../examples/component_gallery/views/themes/material.css");
    let tw_css = include_str!("../examples/component_gallery/views/themes/tailwind.css");
    assert!(
        mat_css.contains(r#"[data-theme="material"]"#)
            && tw_css.contains(r#"[data-theme="tailwind"]"#)
    );
    #[cfg(feature = "preview")]
    {
        use krui::dev_hot_reload::{compile_krui_source, mount_compiled_document};
        let src = format!(
            "<link rel=\"krui-preset\" href=\"krui://preset/unstyled\" />\n<style>\n{mat_css}\n{tw_css}\n.fld {{ background-color: var(--krui-input-bg); }}\n.acc {{ background-color: var(--krui-accent); }}\n</style><div id=\"root\" data-theme=\"material\"><div id=\"inp\" class=\"fld\"></div><div id=\"acc\" class=\"acc\"></div></div>"
        );
        let doc = compile_krui_source(&src).expect("material.css and tailwind.css compile");
        let mut sess = new_cpu_session(240, 120);
        sess.tree
            .set_color_scheme_preference(ColorSchemePreference::Light);
        mount_compiled_document(&mut sess.tree, &doc);
        let _ = sess.step_frame(1.0 / 120.0);
        let iid = sess.tree.find_by_id("inp").unwrap();
        let aid = sess.tree.find_by_id("acc").unwrap();
        assert_eq!(
            sess.tree
                .node_effective_paint(iid)
                .unwrap()
                .background_color,
            Color::rgb(0xE6, 0xE0, 0xE9)
        );
        let root_id = sess.tree.find_by_id("root").unwrap();
        sess.tree.set_node_attr(root_id, "data-theme", "tailwind");
        let _ = sess.step_frame(1.0 / 120.0);
        assert_eq!(
            sess.tree
                .node_effective_paint(iid)
                .unwrap()
                .background_color,
            Color::rgb(0xFF, 0xFF, 0xFF)
        );
        assert_eq!(
            sess.tree
                .node_effective_paint(aid)
                .unwrap()
                .background_color,
            Color::rgb(0x4F, 0x46, 0xE5)
        );
    }
}

#[cfg(feature = "preview")]
#[test]
fn scoped_theme_dark_light_cascade_is_order_invariant() {
    use krui::dev_hot_reload::{compile_krui_source, mount_compiled_document};
    let mat_css = include_str!("../examples/component_gallery/views/themes/material.css");
    let tw_css = include_str!("../examples/component_gallery/views/themes/tailwind.css");
    let src = format!(
        "<link rel=\"krui-preset\" href=\"krui://preset/unstyled\" />\n<style>\n{mat_css}\n{tw_css}\n.probe {{ width: 40px; height: 20px; background-color: var(--krui-accent); border: 1px solid var(--krui-bg); }}\n</style><div id=\"root\" data-theme=\"material\"><div id=\"probe\" class=\"probe\">ok</div></div>"
    );
    let doc = compile_krui_source(&src).expect("themes compile");
    let mut sess_a = new_cpu_session(240, 120);
    mount_compiled_document(&mut sess_a.tree, &doc);
    sess_a
        .tree
        .set_color_scheme_preference(ColorSchemePreference::Dark);
    let _ = sess_a.step_frame(1.0 / 120.0);
    let root_a = sess_a.tree.find_by_id("root").unwrap();
    sess_a.tree.set_node_attr(root_a, "data-theme", "tailwind");
    let _ = sess_a.step_frame(1.0 / 120.0);

    let mut sess_b = new_cpu_session(240, 120);
    mount_compiled_document(&mut sess_b.tree, &doc);
    let root_b = sess_b.tree.find_by_id("root").unwrap();
    sess_b.tree.set_node_attr(root_b, "data-theme", "tailwind");
    let _ = sess_b.step_frame(1.0 / 120.0);
    sess_b
        .tree
        .set_color_scheme_preference(ColorSchemePreference::Dark);
    let _ = sess_b.step_frame(1.0 / 120.0);

    let pa = sess_a
        .tree
        .node_effective_paint(sess_a.tree.find_by_id("probe").unwrap())
        .unwrap();
    let pb = sess_b
        .tree
        .node_effective_paint(sess_b.tree.find_by_id("probe").unwrap())
        .unwrap();
    assert_eq!(pa.background_color, Color::rgb(0x81, 0x8C, 0xF8));
    assert_eq!(pa.border_color, Color::rgb(0x09, 0x09, 0x0B));
    assert_eq!(pa.background_color, pb.background_color);
    assert_eq!(pa.border_color, pb.border_color);
}
