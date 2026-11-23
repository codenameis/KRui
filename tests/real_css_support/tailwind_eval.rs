//! Evaluates `tests/fixtures/real_css/tailwind_v4/app.krui` compiled against unmodified
//! `@tailwindcss/cli` outputs (`tailwind_v4.css` and `tailwind_v4.optimize.css`).

use super::gaps_toml::CheckOutcome;
use super::json_oracle::NodeOracle;
use krui::*;
use krui_compiler::{compile_krui_source_with_virtual_buffers, VirtualBufferMap};
use krui_core::{DynCommand, DynSignal, RegistryRef, ScopeRegistry, Value};
use krui_preview::mount_compiled_document_with_registry;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::rc::Rc;

/// Color channel tolerance (`±3` out of 255) for 8-bit `oklch -> sRGB` rounding differences.
pub const COLOR_TOLERANCE_U8: i16 = 3;
/// Explicit container/layout scalar tolerance (`±2.0px`).
pub const LAYOUT_TOLERANCE_PX: f32 = 2.0;

pub fn check_color_eq(key: &str, got: Color, exp: [u8; 4]) -> CheckOutcome {
    let ok = (got.r as i16 - exp[0] as i16).abs() <= COLOR_TOLERANCE_U8
        && (got.g as i16 - exp[1] as i16).abs() <= COLOR_TOLERANCE_U8
        && (got.b as i16 - exp[2] as i16).abs() <= COLOR_TOLERANCE_U8
        && (got.a as i16 - exp[3] as i16).abs() <= COLOR_TOLERANCE_U8;
    CheckOutcome {
        key: key.to_string(),
        passed: ok,
        detail: format!(
            "got rgba({}, {}, {}, {}), oracle {:?}",
            got.r, got.g, got.b, got.a, exp
        ),
    }
}

pub fn check_scalar_eq(key: &str, got: f32, exp: f32, tol: f32) -> CheckOutcome {
    let ok = (got - exp).abs() <= tol;
    CheckOutcome {
        key: key.to_string(),
        passed: ok,
        detail: format!("got {got:.2}, oracle {exp:.2} (±{tol:.2})"),
    }
}

fn build_app_registry() -> RegistryRef {
    let mut reg = ScopeRegistry::new();
    let sq = Signal::new("Tailwind v4".to_string());
    let d_open = Signal::new(false);
    reg.insert_signal("search_query", DynSignal::from_signal(sq));
    reg.insert_signal("dialog_open", DynSignal::from_signal(d_open));
    let d1 = d_open;
    reg.insert_value(
        "open_dialog_cmd",
        Value::Command(DynCommand::new(Rc::new(move || d1.set(true)), None)),
    );
    let d2 = d_open;
    reg.insert_value(
        "close_dialog_cmd",
        Value::Command(DynCommand::new(Rc::new(move || d2.set(false)), None)),
    );
    Rc::new(reg)
}

pub fn evaluate_tailwind_suite(
    suite_prefix: &str,
    css_bytes: &str,
    viewport: [u32; 2],
    oracle_nodes: &BTreeMap<String, NodeOracle>,
    out: &mut Vec<CheckOutcome>,
) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let krui_path = root.join("tests/fixtures/real_css/tailwind_v4/app.krui");
    let css_target =
        std::fs::canonicalize(root.join("tests/fixtures/real_css/tailwind_v4/tailwind_v4.css"))
            .unwrap_or_else(|_| root.join("tests/fixtures/real_css/tailwind_v4/tailwind_v4.css"));
    let mut vbufs: VirtualBufferMap = HashMap::new();
    vbufs.insert(css_target, css_bytes.to_string());

    let doc = match compile_krui_source_with_virtual_buffers(&krui_path, None, &vbufs) {
        Ok(d) => {
            out.push(CheckOutcome {
                key: format!("{suite_prefix}.compile"),
                passed: true,
                detail: "compiled without error".into(),
            });
            d
        }
        Err(err) => {
            out.push(CheckOutcome {
                key: format!("{suite_prefix}.compile"),
                passed: false,
                detail: format!("CompileError: {}", err.message),
            });
            record_compile_blocked_checks(suite_prefix, oracle_nodes, &err.message, out);
            return;
        }
    };

    let mut tree = ViewTree::new();
    tree.set_color_scheme_preference(ColorSchemePreference::Light);
    let _ = mount_compiled_document_with_registry(&mut tree, &doc, build_app_registry());
    let (scene, _) = tree.build_paint_scene(viewport[0], viewport[1]);

    for (id, ora) in oracle_nodes {
        eval_single_tailwind_node(suite_prefix, id, ora, &tree, &scene, out);
    }
}

fn record_compile_blocked_checks(
    suite_prefix: &str,
    oracle_nodes: &BTreeMap<String, NodeOracle>,
    err_msg: &str,
    out: &mut Vec<CheckOutcome>,
) {
    for (id, ora) in oracle_nodes {
        let mut props = vec![
            "background_color",
            "color",
            "border_top_width",
            "border_radius",
            "font_size",
            "font_weight",
            "opacity",
        ];
        if ora.border_top_width > 0.0 {
            props.push("border_top_color");
        }
        if ora.has_gradient {
            props.push("has_gradient");
        }
        if ora.shadows.iter().any(|s| s.spread > 0.5 && s.blur < 0.5) {
            props.push("ring_shadow");
        }
        if matches!(
            id.as_str(),
            "tw-root"
                | "tw-banner"
                | "tw-search"
                | "peer-toggle"
                | "peer-track"
                | "peer-knob"
                | "group-card"
        ) {
            props.push("rect_width");
        }
        if matches!(
            id.as_str(),
            "tw-search" | "peer-toggle" | "peer-track" | "peer-knob"
        ) {
            props.push("rect_height");
        }
        for prop in props {
            out.push(CheckOutcome {
                key: format!("{suite_prefix}.{id}.{prop}"),
                passed: false,
                detail: format!("blocked by CompileError: {err_msg}"),
            });
        }
    }
}

fn eval_single_tailwind_node(
    suite_prefix: &str,
    id: &str,
    ora: &NodeOracle,
    tree: &ViewTree,
    scene: &PaintScene,
    out: &mut Vec<CheckOutcome>,
) {
    let pfx = format!("{suite_prefix}.{id}");
    let Some(nid) = tree.find_by_id(id) else {
        out.push(CheckOutcome {
            key: format!("{pfx}.mounted"),
            passed: false,
            detail: "node missing in ViewTree".into(),
        });
        return;
    };
    let rect = tree.node_layout(nid).expect("node layout");
    let paint = tree.node_effective_paint(nid).expect("node paint");
    let cstyle = tree.node_computed_style(nid).expect("computed style");
    eval_node_paint_and_text(&pfx, ora, rect, &paint, &cstyle, out);
    eval_node_geometry_and_effects(&pfx, id, ora, rect, &paint, scene, out);
}

fn eval_node_paint_and_text(
    pfx: &str,
    ora: &NodeOracle,
    rect: LayoutRect,
    paint: &PaintStyle,
    cstyle: &CompiledStyle,
    out: &mut Vec<CheckOutcome>,
) {
    out.push(check_color_eq(
        &format!("{pfx}.background_color"),
        paint.background_color,
        ora.background_color,
    ));
    out.push(check_color_eq(
        &format!("{pfx}.color"),
        paint.text_color,
        ora.color,
    ));
    out.push(check_scalar_eq(
        &format!("{pfx}.border_top_width"),
        paint.border_width,
        ora.border_top_width,
        0.25,
    ));
    if ora.border_top_width > 0.0 {
        out.push(check_color_eq(
            &format!("{pfx}.border_top_color"),
            paint.border_color,
            ora.border_top_color,
        ));
    }
    let radius_ok = if ora.border_radius > 1000.0 {
        paint.border_radius >= rect.width.min(rect.height) * 0.5 - 0.5
    } else {
        (paint.border_radius - ora.border_radius).abs() <= 0.5
    };
    out.push(CheckOutcome {
        key: format!("{pfx}.border_radius"),
        passed: radius_ok,
        detail: format!(
            "got {:.2}, oracle {:.2}",
            paint.border_radius, ora.border_radius
        ),
    });
    out.push(check_scalar_eq(
        &format!("{pfx}.font_size"),
        cstyle.text.font_size,
        ora.font_size,
        0.5,
    ));
    out.push(check_scalar_eq(
        &format!("{pfx}.font_weight"),
        cstyle.text.font_weight as f32,
        ora.font_weight as f32,
        1.0,
    ));
    out.push(check_scalar_eq(
        &format!("{pfx}.opacity"),
        paint.opacity,
        ora.opacity,
        0.02,
    ));
}

fn eval_node_geometry_and_effects(
    pfx: &str,
    id: &str,
    ora: &NodeOracle,
    rect: LayoutRect,
    paint: &PaintStyle,
    scene: &PaintScene,
    out: &mut Vec<CheckOutcome>,
) {
    if ora.has_gradient {
        out.push(CheckOutcome {
            key: format!("{pfx}.has_gradient"),
            passed: paint.background_gradient.is_some(),
            detail: format!("gradient={:?}", paint.background_gradient),
        });
    }
    if let Some(ring_ora) = ora.shadows.iter().find(|s| {
        s.spread > 0.5 && s.blur < 0.5 && s.offset_x.abs() < 0.5 && s.offset_y.abs() < 0.5
    }) {
        let got_ring = find_ring_shadow_for_rect(scene, rect);
        let ring_col = got_ring.map(|s| s.color).unwrap_or(Color::TRANSPARENT);
        out.push(check_color_eq(
            &format!("{pfx}.ring_shadow"),
            ring_col,
            ring_ora.color,
        ));
    }
    if matches!(
        id,
        "tw-root"
            | "tw-banner"
            | "tw-search"
            | "peer-toggle"
            | "peer-track"
            | "peer-knob"
            | "group-card"
    ) {
        out.push(check_scalar_eq(
            &format!("{pfx}.rect_width"),
            rect.width,
            ora.rect[2],
            LAYOUT_TOLERANCE_PX,
        ));
    }
    if matches!(id, "tw-search" | "peer-toggle" | "peer-track" | "peer-knob") {
        out.push(check_scalar_eq(
            &format!("{pfx}.rect_height"),
            rect.height,
            ora.rect[3],
            LAYOUT_TOLERANCE_PX,
        ));
    }
}

fn find_ring_shadow_for_rect(scene: &PaintScene, target: LayoutRect) -> Option<BoxShadow> {
    scene.primitives.iter().find_map(|p| match p {
        DrawPrimitive::StyledBox {
            rect,
            box_shadow: Some(sh),
            ..
        } if (rect.x - target.x).abs() < 2.0
            && (rect.y - target.y).abs() < 2.0
            && (rect.width - target.width).abs() < 2.0
            && sh.spread_radius > 0.5
            && sh.blur_radius < 0.5
            && sh.color.a > 0 =>
        {
            Some(*sh)
        }
        _ => None,
    })
}
