//! Preview IR binding helpers for `<img>`, `<a href>`, and compiled `<svg>` elements.

use crate::eval::eval_interpolated_parts;
use crate::slots::bind_interpolated;
use krui_compiler::{
    CompiledFillRule, CompiledLineCap, CompiledLineJoin, CompiledNode, CompiledPathSegment,
    CompiledSvgColor, CompiledSvgData, CompiledSvgOp, CompiledTextPart,
};
use krui_core::{
    CanvasFillRule, CanvasLineCap, CanvasLineJoin, NodeId, PathSegment, RegistryRef, SvgColor,
    SvgVectorOp, SvgVectorTemplate, ViewTree,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

thread_local! {
    static PREVIEW_DOC_BASE_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

struct BaseDirGuard(Option<PathBuf>);

impl Drop for BaseDirGuard {
    fn drop(&mut self) {
        let prev = self.0.take();
        PREVIEW_DOC_BASE_DIR.with(|slot| *slot.borrow_mut() = prev);
    }
}

/// Returns the active `.krui` document base directory during preview mounting, if any.
pub(crate) fn current_preview_doc_base_dir() -> Option<PathBuf> {
    PREVIEW_DOC_BASE_DIR.with(|slot| slot.borrow().clone())
}

/// Runs `f` with the current compiled `.krui` document's parent directory active for asset paths.
pub(crate) fn with_preview_doc_base_dir<R>(dir: Option<&Path>, f: impl FnOnce() -> R) -> R {
    let next = dir.map(Path::to_path_buf);
    let prev = PREVIEW_DOC_BASE_DIR.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), next));
    let _guard = BaseDirGuard(prev);
    f()
}

/// Binds `<img>`, `<a href>`, and `<svg>` media slots onto `node_id` in preview mode.
pub(crate) fn bind_media_slots(
    tree: &mut ViewTree,
    node_id: NodeId,
    node: &CompiledNode,
    registry: &RegistryRef,
) {
    bind_img_src_slot(tree, node_id, node, registry);
    bind_interpolated(
        tree,
        node_id,
        &node.link_href,
        registry,
        |t, id, href| t.set_link_href(id, href),
        |t, id, f| t.bind_dynamic_link_href(id, f),
    );
    if let Some(svg) = &node.svg_data {
        tree.set_node_svg_template(node_id, convert_svg_template(svg));
    }
}

fn bind_img_src_slot(
    tree: &mut ViewTree,
    node_id: NodeId,
    node: &CompiledNode,
    registry: &RegistryRef,
) {
    if node.img_src.is_empty() {
        return;
    }
    let base_dir = PREVIEW_DOC_BASE_DIR.with(|slot| slot.borrow().clone());
    let has_dyn = node
        .img_src
        .iter()
        .any(|p| matches!(p, CompiledTextPart::Dynamic { .. }));
    if !has_dyn {
        let mut combined = String::new();
        for part in &node.img_src {
            if let CompiledTextPart::Static(s) = part {
                combined.push_str(s);
            }
        }
        let trimmed = combined.trim();
        let path = resolve_preview_asset_path(trimmed, base_dir.as_deref());
        let _ = tree.set_node_image_file(node_id, path);
        return;
    }
    let parts = node.img_src.clone();
    let reg = registry.clone();
    match base_dir {
        Some(dir) => {
            tree.bind_dynamic_image_src_in_dir(node_id, dir, move || {
                eval_interpolated_parts(&parts, reg.as_ref(), false)
            });
        }
        None => {
            tree.bind_dynamic_image_src(node_id, move || {
                eval_interpolated_parts(&parts, reg.as_ref(), false)
            });
        }
    }
}

pub(crate) fn resolve_preview_asset_path(raw: &str, base_dir: Option<&Path>) -> PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    if let Some(dir) = base_dir {
        let candidate = dir.join(p);
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR") {
        let candidate = PathBuf::from(manifest).join(p);
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(dir) = base_dir {
        return dir.join(p);
    }
    p.to_path_buf()
}

fn convert_svg_template(svg: &CompiledSvgData) -> SvgVectorTemplate {
    let ops: Vec<SvgVectorOp> = svg.ops.iter().map(convert_svg_op).collect();
    SvgVectorTemplate {
        view_box: svg.view_box,
        intrinsic_size: svg.intrinsic_size,
        ops: Arc::from(ops),
    }
}

fn convert_svg_color(color: CompiledSvgColor) -> SvgColor {
    match color {
        CompiledSvgColor::Explicit(c) => SvgColor::Explicit(c),
        CompiledSvgColor::CurrentColor => SvgColor::CurrentColor,
    }
}

fn convert_path_segment(seg: &CompiledPathSegment) -> PathSegment {
    match *seg {
        CompiledPathSegment::MoveTo(x, y) => PathSegment::MoveTo(x, y),
        CompiledPathSegment::LineTo(x, y) => PathSegment::LineTo(x, y),
        CompiledPathSegment::QuadTo { cx, cy, x, y } => PathSegment::QuadTo { cx, cy, x, y },
        CompiledPathSegment::CubicTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
        } => PathSegment::CubicTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
        },
        CompiledPathSegment::Arc {
            cx,
            cy,
            rx,
            ry,
            start_rad,
            sweep_rad,
            x_rotation_rad,
        } => PathSegment::Arc {
            cx,
            cy,
            rx,
            ry,
            start_rad,
            sweep_rad,
            x_rotation_rad,
        },
        CompiledPathSegment::Close => PathSegment::Close,
    }
}

fn convert_svg_op(op: &CompiledSvgOp) -> SvgVectorOp {
    match op {
        CompiledSvgOp::FillRect {
            x,
            y,
            w,
            h,
            radius,
            color,
        } => SvgVectorOp::FillRect {
            x: *x,
            y: *y,
            w: *w,
            h: *h,
            radius: *radius,
            color: convert_svg_color(*color),
        },
        CompiledSvgOp::StrokeRect {
            x,
            y,
            w,
            h,
            line_width,
            color,
        } => SvgVectorOp::StrokeRect {
            x: *x,
            y: *y,
            w: *w,
            h: *h,
            line_width: *line_width,
            color: convert_svg_color(*color),
        },
        other => convert_svg_circle_or_path(other),
    }
}

fn convert_fill_rule(rule: CompiledFillRule) -> CanvasFillRule {
    match rule {
        CompiledFillRule::NonZero => CanvasFillRule::NonZero,
        CompiledFillRule::EvenOdd => CanvasFillRule::EvenOdd,
    }
}

fn convert_line_cap(cap: CompiledLineCap) -> CanvasLineCap {
    match cap {
        CompiledLineCap::Butt => CanvasLineCap::Butt,
        CompiledLineCap::Round => CanvasLineCap::Round,
        CompiledLineCap::Square => CanvasLineCap::Square,
    }
}

fn convert_line_join(join: CompiledLineJoin) -> CanvasLineJoin {
    match join {
        CompiledLineJoin::Miter => CanvasLineJoin::Miter,
        CompiledLineJoin::Round => CanvasLineJoin::Round,
        CompiledLineJoin::Bevel => CanvasLineJoin::Bevel,
    }
}

fn convert_svg_circle_or_path(op: &CompiledSvgOp) -> SvgVectorOp {
    match op {
        CompiledSvgOp::FillCircle {
            cx,
            cy,
            radius,
            color,
        } => SvgVectorOp::FillCircle {
            cx: *cx,
            cy: *cy,
            radius: *radius,
            color: convert_svg_color(*color),
        },
        CompiledSvgOp::StrokeCircle {
            cx,
            cy,
            radius,
            line_width,
            color,
        } => SvgVectorOp::StrokeCircle {
            cx: *cx,
            cy: *cy,
            radius: *radius,
            line_width: *line_width,
            color: convert_svg_color(*color),
        },
        CompiledSvgOp::FillPath {
            segments,
            fill_rule,
            color,
        } => SvgVectorOp::FillPath {
            segments: segments.iter().map(convert_path_segment).collect(),
            fill_rule: convert_fill_rule(*fill_rule),
            color: convert_svg_color(*color),
        },
        CompiledSvgOp::StrokePath {
            segments,
            line_width,
            line_cap,
            line_join,
            color,
        } => SvgVectorOp::StrokePath {
            segments: segments.iter().map(convert_path_segment).collect(),
            line_width: *line_width,
            line_cap: convert_line_cap(*line_cap),
            line_join: convert_line_join(*line_join),
            color: convert_svg_color(*color),
        },
        _ => unreachable!(),
    }
}
