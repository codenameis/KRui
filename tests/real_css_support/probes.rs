//! W3C Conformance Probes evaluated against Chromium `oracle.json` (`probes` map).

use super::gaps_toml::CheckOutcome;
use super::json_oracle::NodeOracle;
use super::tailwind_eval::{check_color_eq, check_scalar_eq};
use krui::*;
use krui_compiler::compile_krui_source;
use krui_preview::mount_compiled_document;
use std::collections::BTreeMap;

fn mount_and_render_probe(krui_src: &str) -> Result<(ViewTree, PaintScene, Vec<u8>), String> {
    let doc = compile_krui_source(krui_src).map_err(|e| e.message)?;
    let mut tree = ViewTree::new();
    tree.set_color_scheme_preference(ColorSchemePreference::Light);
    let _ = mount_compiled_document(&mut tree, &doc);
    let (scene, _) = tree.build_paint_scene(400, 320);
    let mut cpu = TinySkiaRenderer::new(400, 320, GpuResourceLimits::default());
    cpu.render_scene(&scene);
    let fb = cpu.framebuffer_rgba().expect("cpu fb").to_vec();
    Ok((tree, scene, fb))
}

pub fn run_all_probe_checks(probes: &BTreeMap<String, NodeOracle>, out: &mut Vec<CheckOutcome>) {
    eval_flex_inline_probe(&probes["probe-flex-inline"], out);
    eval_undef_var_probe(&probes["probe-undef-var"], out);
    eval_nested_var_probe(&probes["probe-nested-var"], out);
    eval_calc_zero_probe(&probes["probe-calc-zero"], out);
    eval_fallback_probe(&probes["probe-fallback"], out);
    eval_border_nostyle_probe(&probes["probe-border-nostyle"], out);
    eval_placeholder_probe(&probes["probe-placeholder"], out);
    eval_container_probe(&probes["probe-container"], out);
    eval_filter_probe(&probes["probe-filter"], out);
}

fn eval_flex_inline_probe(ora: &NodeOracle, out: &mut Vec<CheckOutcome>) {
    let src = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            .flex-box { display: flex; align-items: center; justify-content: center; width: 100px; height: 60px; }
            #probe-flex-inline { display: inline-block; height: 20px; padding: 0px 8px; background-color: #3b82f6; }
        </style>
        <div class="flex-box"><span id="probe-flex-inline">x</span></div>
    "#;
    let (tree, _, _) = mount_and_render_probe(src).expect("flex-inline probe must compile");
    let id = tree.find_by_id("probe-flex-inline").expect("node");
    let cstyle = tree.node_computed_style(id).expect("computed style");
    let rect = tree.node_layout(id).expect("rect");
    let paint = tree.node_effective_paint(id).expect("paint");
    out.push(CheckOutcome {
        key: "probes.probe-flex-inline.display".into(),
        passed: cstyle.layout.display == DisplayMode::Block && ora.display == "block",
        detail: format!("got {:?}, oracle {}", cstyle.layout.display, ora.display),
    });
    out.push(check_scalar_eq(
        "probes.probe-flex-inline.rect_y",
        rect.y,
        ora.rect[1],
        0.5,
    ));
    out.push(check_color_eq(
        "probes.probe-flex-inline.background_color",
        paint.background_color,
        ora.background_color,
    ));
}

fn eval_undef_var_probe(ora: &NodeOracle, out: &mut Vec<CheckOutcome>) {
    let src = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            :root { --unused-dark-ref: var(--only-in-dark-mode); }
            #probe-undef-var { width: 80px; height: 24px; background-color: #16a34a; color: var(--definitely-undefined-var); }
        </style>
        <div id="probe-undef-var"></div>
    "#;
    match mount_and_render_probe(src) {
        Ok((tree, _, _)) => {
            let id = tree.find_by_id("probe-undef-var").expect("node");
            let rect = tree.node_layout(id).expect("rect");
            let paint = tree.node_effective_paint(id).expect("paint");
            out.push(CheckOutcome {
                key: "probes.probe-undef-var.compile".into(),
                passed: true,
                detail: "compiled cleanly".into(),
            });
            out.push(check_scalar_eq(
                "probes.probe-undef-var.rect_width",
                rect.width,
                ora.rect[2],
                0.5,
            ));
            out.push(check_color_eq(
                "probes.probe-undef-var.background_color",
                paint.background_color,
                ora.background_color,
            ));
        }
        Err(err) => {
            for suffix in ["compile", "rect_width", "background_color"] {
                out.push(CheckOutcome {
                    key: format!("probes.probe-undef-var.{suffix}"),
                    passed: false,
                    detail: format!("CompileError: {err}"),
                });
            }
        }
    }
}

fn eval_nested_var_probe(ora: &NodeOracle, out: &mut Vec<CheckOutcome>) {
    let src = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            @property --c { syntax: "*"; inherits: false; initial-value: #6366f1; }
            .ring-probe { width: 80px; height: 28px; --bg: var(--c); background-color: var(--bg); --s: 0 0 0 2px var(--c); box-shadow: var(--s); }
            .red-probe { --c: #ef4444; }
        </style>
        <div id="probe-nested-var" class="ring-probe red-probe"></div>
    "#;
    let (tree, _, _) = mount_and_render_probe(src).expect("nested-var probe must compile");
    let id = tree.find_by_id("probe-nested-var").expect("node");
    let rect = tree.node_layout(id).expect("rect");
    let paint = tree.node_effective_paint(id).expect("paint");
    out.push(check_scalar_eq(
        "probes.probe-nested-var.rect_width",
        rect.width,
        ora.rect[2],
        0.5,
    ));
    out.push(check_color_eq(
        "probes.probe-nested-var.background_color",
        paint.background_color,
        ora.background_color,
    ));
    let ring_col = paint
        .box_shadow
        .map(|s| s.color)
        .unwrap_or(Color::TRANSPARENT);
    out.push(check_color_eq(
        "probes.probe-nested-var.ring_shadow",
        ring_col,
        ora.shadows[0].color,
    ));
}

fn eval_calc_zero_probe(ora: &NodeOracle, out: &mut Vec<CheckOutcome>) {
    let src = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            @property --zero-len { syntax: "<length>"; inherits: false; initial-value: 0px; }
            #probe-calc-zero { width: calc(96px + var(--zero-len)); height: 24px; background-color: #0ea5e9; }
        </style>
        <div id="probe-calc-zero"></div>
    "#;
    match mount_and_render_probe(src) {
        Ok((tree, _, _)) => {
            let id = tree.find_by_id("probe-calc-zero").expect("node");
            let rect = tree.node_layout(id).expect("rect");
            let paint = tree.node_effective_paint(id).expect("paint");
            out.push(check_scalar_eq(
                "probes.probe-calc-zero.rect_width",
                rect.width,
                ora.rect[2],
                0.5,
            ));
            out.push(check_color_eq(
                "probes.probe-calc-zero.background_color",
                paint.background_color,
                ora.background_color,
            ));
        }
        Err(err) => {
            out.push(CheckOutcome {
                key: "probes.probe-calc-zero.rect_width".into(),
                passed: false,
                detail: format!("CompileError: {err}"),
            });
        }
    }
}

fn eval_fallback_probe(ora: &NodeOracle, out: &mut Vec<CheckOutcome>) {
    let src = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            #probe-fallback-host { width: 180px; height: 24px; }
            #probe-fallback { display: inline-block; height: 24px; width: 100%; width: stretch; background-color: #8b5cf6; }
            #probe-fit-content { width: 100%; width: -krui-unsupported-size; height: 10px; }
        </style>
        <div id="probe-fallback-host">
            <div id="probe-fallback"></div>
            <div id="probe-fit-content"></div>
        </div>
    "#;
    let (tree, _, _) = mount_and_render_probe(src).expect("fallback probe must compile");
    let id = tree.find_by_id("probe-fallback").expect("node");
    let rect = tree.node_layout(id).expect("rect");
    let paint = tree.node_effective_paint(id).expect("paint");
    let fit_id = tree.find_by_id("probe-fit-content").expect("fit node");
    let fit_style = tree.node_computed_style(fit_id).expect("fit style");
    let fit_preserved = fit_style.layout.width == Dimension::Percent(100.0);
    out.push(CheckOutcome {
        key: "probes.probe-fallback.rect_width".into(),
        passed: (rect.width - ora.rect[2]).abs() <= 0.5 && fit_preserved,
        detail: format!(
            "stretch rect.width={:.2} (oracle {:.2}), fallback width={:?}",
            rect.width, ora.rect[2], fit_style.layout.width
        ),
    });
    out.push(check_color_eq(
        "probes.probe-fallback.background_color",
        paint.background_color,
        ora.background_color,
    ));
}

fn eval_border_nostyle_probe(ora: &NodeOracle, out: &mut Vec<CheckOutcome>) {
    let src = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            #probe-border-nostyle { width: 100px; height: 32px; border-width: 4px; border-color: #ef4444; background-color: #1e293b; }
        </style>
        <div id="probe-border-nostyle"></div>
    "#;
    let (tree, _, _) = mount_and_render_probe(src).expect("border-nostyle probe must compile");
    let id = tree.find_by_id("probe-border-nostyle").expect("node");
    let paint = tree.node_effective_paint(id).expect("paint");
    out.push(check_scalar_eq(
        "probes.probe-border-nostyle.border_top_width",
        paint.border_width,
        ora.border_top_width,
        0.1,
    ));
    out.push(check_color_eq(
        "probes.probe-border-nostyle.background_color",
        paint.background_color,
        ora.background_color,
    ));
}

fn eval_placeholder_probe(ora: &NodeOracle, out: &mut Vec<CheckOutcome>) {
    let src = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            #probe-placeholder { width: 180px; height: 32px; color: #0f172a; background-color: #f1f5f9; border: 1px solid #cbd5e1; }
            #probe-placeholder::placeholder { color: #f43f5e; opacity: 1; }
        </style>
        <input id="probe-placeholder" type="text" placeholder="Hint text" />
    "#;
    let (tree, scene, _) = mount_and_render_probe(src).expect("placeholder probe must compile");
    let id = tree.find_by_id("probe-placeholder").expect("node");
    let paint = tree.node_effective_paint(id).expect("paint");
    out.push(check_color_eq(
        "probes.probe-placeholder.background_color",
        paint.background_color,
        ora.background_color,
    ));
    let glyph_col = scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::TextGlyphs { color, .. } => Some(*color),
            _ => None,
        })
        .unwrap_or(Color::TRANSPARENT);
    out.push(check_color_eq(
        "probes.probe-placeholder.placeholder_color",
        glyph_col,
        ora.placeholder_color,
    ));
}

fn eval_container_probe(ora: &NodeOracle, out: &mut Vec<CheckOutcome>) {
    let src = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            #probe-cq-host { container-type: inline-size; width: 240px; height: 28px; }
            #probe-container { width: 60px; height: 24px; background-color: #64748b; }
            @container (max-width: 300px) {
                #probe-container { width: 180px; background-color: #10b981; }
            }
        </style>
        <div id="probe-cq-host"><div id="probe-container"></div></div>
    "#;
    let (tree, _, _) = mount_and_render_probe(src).expect("container probe must compile");
    let id = tree.find_by_id("probe-container").expect("node");
    let rect = tree.node_layout(id).expect("rect");
    let paint = tree.node_effective_paint(id).expect("paint");
    out.push(check_scalar_eq(
        "probes.probe-container.rect_width",
        rect.width,
        ora.rect[2],
        0.5,
    ));
    out.push(check_color_eq(
        "probes.probe-container.background_color",
        paint.background_color,
        ora.background_color,
    ));
}

fn eval_filter_probe(ora: &NodeOracle, out: &mut Vec<CheckOutcome>) {
    let src = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            #probe-filter { width: 80px; height: 24px; background-color: #ef4444; filter: invert(1); }
        </style>
        <div id="probe-filter"></div>
    "#;
    let (tree, _, fb) = mount_and_render_probe(src).expect("filter probe must compile");
    let id = tree.find_by_id("probe-filter").expect("node");
    let rect = tree.node_layout(id).expect("rect");
    out.push(check_scalar_eq(
        "probes.probe-filter.rect_width",
        rect.width,
        ora.rect[2],
        0.5,
    ));
    let cx = (rect.x + rect.width * 0.5) as u32;
    let cy = (rect.y + rect.height * 0.5) as u32;
    let off = ((cy * 400 + cx) * 4) as usize;
    let px = Color::rgba(fb[off], fb[off + 1], fb[off + 2], fb[off + 3]);
    assert_eq!(ora.filter, "invert(1)");
    let inv = [
        255 - ora.background_color[0],
        255 - ora.background_color[1],
        255 - ora.background_color[2],
        255,
    ];
    out.push(check_color_eq("probes.probe-filter.filter_effect", px, inv));
}
