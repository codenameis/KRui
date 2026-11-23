//! Compile-time SVG leaf shape parsing (`<path>`, `<rect>`, `<circle>`, `<ellipse>`, `<line>`, `<polyline>`, `<polygon>`).

use crate::html::RawAttr;
use crate::svg_compile::{
    apply_paint_attrs, parse_svg_len, CompiledLineJoin, CompiledSvgColor, CompiledSvgOp,
    PaintInherit,
};
use crate::svg_path::{parse_svg_path_data, CompiledPathSegment};
use crate::CompileError;
use std::f32::consts::PI;

pub(crate) fn emit_svg_leaf_shape(
    src: &str,
    tag: &str,
    tag_off: usize,
    attrs: &[RawAttr],
    inherit: PaintInherit,
    ops: &mut Vec<CompiledSvgOp>,
) -> Result<(), CompileError> {
    let paint = apply_paint_attrs(src, attrs, inherit)?;
    match tag {
        "path" => {
            validate_shape_attrs(src, tag, attrs, &["d"])?;
            let (d, d_off) = req_str(src, tag_off, attrs, "d")?;
            let segs = parse_svg_path_data(src, d_off, &d)?;
            push_fill_stroke_path(ops, segs, paint);
        }
        "rect" => emit_svg_rect(src, tag_off, attrs, paint, ops)?,
        "circle" => emit_svg_circle(src, tag_off, attrs, paint, ops)?,
        "ellipse" => emit_svg_ellipse(src, tag_off, attrs, paint, ops)?,
        "line" => emit_svg_line(src, attrs, paint, ops)?,
        "polyline" | "polygon" => {
            validate_shape_attrs(src, tag, attrs, &["points"])?;
            let (pts, off) = req_str(src, tag_off, attrs, "points")?;
            let segs = parse_points_attr(src, off, &pts, tag == "polygon")?;
            push_fill_stroke_path(ops, segs, paint);
        }
        other => {
            return Err(CompileError::at_offset(
                src,
                tag_off,
                format!("Unsupported SVG element `<{other}>`"),
            ));
        }
    }
    Ok(())
}

fn emit_svg_rect(
    src: &str,
    tag_off: usize,
    attrs: &[RawAttr],
    paint: PaintInherit,
    ops: &mut Vec<CompiledSvgOp>,
) -> Result<(), CompileError> {
    validate_shape_attrs(
        src,
        "rect",
        attrs,
        &["x", "y", "width", "height", "rx", "ry"],
    )?;
    let (x, y) = (
        opt_f32(src, attrs, "x", 0.0)?,
        opt_f32(src, attrs, "y", 0.0)?,
    );
    let (w, h) = (
        req_nonneg_f32(src, tag_off, attrs, "width")?,
        req_nonneg_f32(src, tag_off, attrs, "height")?,
    );
    let rx_opt = opt_nonneg_f32(src, attrs, "rx")?;
    let ry_opt = opt_nonneg_f32(src, attrs, "ry")?;
    let rad = rx_opt.or(ry_opt).unwrap_or(0.0);
    if let Some(c) = paint.fill {
        ops.push(CompiledSvgOp::FillRect {
            x,
            y,
            w,
            h,
            radius: rad,
            color: c,
        });
    }
    if let Some(c) = paint.stroke {
        push_svg_rect_stroke((x, y, w, h, rad), paint, c, ops);
    }
    Ok(())
}

fn push_svg_rect_stroke(
    (x, y, w, h, rad): (f32, f32, f32, f32, f32),
    paint: PaintInherit,
    c: CompiledSvgColor,
    ops: &mut Vec<CompiledSvgOp>,
) {
    if paint.line_join != CompiledLineJoin::Miter && rad <= 0.05 {
        ops.push(CompiledSvgOp::StrokePath {
            segments: vec![
                CompiledPathSegment::MoveTo(x, y),
                CompiledPathSegment::LineTo(x + w, y),
                CompiledPathSegment::LineTo(x + w, y + h),
                CompiledPathSegment::LineTo(x, y + h),
                CompiledPathSegment::Close,
            ],
            line_width: paint.stroke_width,
            line_cap: paint.line_cap,
            line_join: paint.line_join,
            color: c,
        });
    } else {
        ops.push(CompiledSvgOp::StrokeRect {
            x,
            y,
            w,
            h,
            line_width: paint.stroke_width,
            color: c,
        });
    }
}

fn emit_svg_circle(
    src: &str,
    tag_off: usize,
    attrs: &[RawAttr],
    paint: PaintInherit,
    ops: &mut Vec<CompiledSvgOp>,
) -> Result<(), CompileError> {
    validate_shape_attrs(src, "circle", attrs, &["cx", "cy", "r"])?;
    let (cx, cy) = (
        opt_f32(src, attrs, "cx", 0.0)?,
        opt_f32(src, attrs, "cy", 0.0)?,
    );
    let r = req_nonneg_f32(src, tag_off, attrs, "r")?;
    if let Some(c) = paint.fill {
        ops.push(CompiledSvgOp::FillCircle {
            cx,
            cy,
            radius: r,
            color: c,
        });
    }
    if let Some(c) = paint.stroke {
        ops.push(CompiledSvgOp::StrokeCircle {
            cx,
            cy,
            radius: r,
            line_width: paint.stroke_width,
            color: c,
        });
    }
    Ok(())
}

fn emit_svg_line(
    src: &str,
    attrs: &[RawAttr],
    paint: PaintInherit,
    ops: &mut Vec<CompiledSvgOp>,
) -> Result<(), CompileError> {
    validate_shape_attrs(src, "line", attrs, &["x1", "y1", "x2", "y2"])?;
    let (x1, y1) = (
        opt_f32(src, attrs, "x1", 0.0)?,
        opt_f32(src, attrs, "y1", 0.0)?,
    );
    let (x2, y2) = (
        opt_f32(src, attrs, "x2", 0.0)?,
        opt_f32(src, attrs, "y2", 0.0)?,
    );
    if let Some(c) = paint.stroke {
        ops.push(CompiledSvgOp::StrokePath {
            segments: vec![
                CompiledPathSegment::MoveTo(x1, y1),
                CompiledPathSegment::LineTo(x2, y2),
            ],
            line_width: paint.stroke_width,
            line_cap: paint.line_cap,
            line_join: paint.line_join,
            color: c,
        });
    }
    Ok(())
}

fn emit_svg_ellipse(
    src: &str,
    tag_off: usize,
    attrs: &[RawAttr],
    paint: PaintInherit,
    ops: &mut Vec<CompiledSvgOp>,
) -> Result<(), CompileError> {
    validate_shape_attrs(src, "ellipse", attrs, &["cx", "cy", "rx", "ry"])?;
    let (cx, cy) = (
        opt_f32(src, attrs, "cx", 0.0)?,
        opt_f32(src, attrs, "cy", 0.0)?,
    );
    let (rx, ry) = (
        req_nonneg_f32(src, tag_off, attrs, "rx")?,
        req_nonneg_f32(src, tag_off, attrs, "ry")?,
    );
    let segs = vec![
        CompiledPathSegment::MoveTo(cx + rx, cy),
        CompiledPathSegment::Arc {
            cx,
            cy,
            rx,
            ry,
            start_rad: 0.0,
            sweep_rad: PI,
            x_rotation_rad: 0.0,
        },
        CompiledPathSegment::Arc {
            cx,
            cy,
            rx,
            ry,
            start_rad: PI,
            sweep_rad: PI,
            x_rotation_rad: 0.0,
        },
        CompiledPathSegment::Close,
    ];
    push_fill_stroke_path(ops, segs, paint);
    Ok(())
}

fn push_fill_stroke_path(
    ops: &mut Vec<CompiledSvgOp>,
    segments: Vec<CompiledPathSegment>,
    paint: PaintInherit,
) {
    if let Some(c) = paint.fill {
        ops.push(CompiledSvgOp::FillPath {
            segments: segments.clone(),
            fill_rule: paint.fill_rule,
            color: c,
        });
    }
    if let Some(c) = paint.stroke {
        ops.push(CompiledSvgOp::StrokePath {
            segments,
            line_width: paint.stroke_width,
            line_cap: paint.line_cap,
            line_join: paint.line_join,
            color: c,
        });
    }
}

pub(crate) fn validate_shape_attrs(
    src: &str,
    tag: &str,
    attrs: &[RawAttr],
    allowed: &[&str],
) -> Result<(), CompileError> {
    for (k, _, _, off) in attrs {
        if matches!(
            k.as_str(),
            "fill"
                | "stroke"
                | "stroke-width"
                | "stroke-linecap"
                | "stroke-linejoin"
                | "fill-rule"
                | "id"
                | "class"
        ) || allowed.contains(&k.as_str())
        {
            continue;
        }
        return Err(CompileError::at_offset(
            src,
            *off,
            format!("Unsupported or unknown attribute `{k}` on SVG `<{tag}>`"),
        ));
    }
    Ok(())
}

fn parse_points_attr(
    src: &str,
    off: usize,
    raw: &str,
    close: bool,
) -> Result<Vec<CompiledPathSegment>, CompileError> {
    let nums: Result<Vec<f32>, _> = raw
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f32>())
        .collect();
    let vals = nums
        .map_err(|_| CompileError::at_offset(src, off, format!("Invalid SVG `points`: `{raw}`")))?;
    if vals.len() < 4 || vals.len() % 2 != 0 || vals.iter().any(|v| !v.is_finite()) {
        return Err(CompileError::at_offset(
            src,
            off,
            format!("SVG `points` must contain an even number of >= 4 finite coordinates: `{raw}`"),
        ));
    }
    let mut segs = Vec::with_capacity(vals.len() / 2 + 1);
    segs.push(CompiledPathSegment::MoveTo(vals[0], vals[1]));
    for &[x, y] in vals[2..].as_chunks::<2>().0 {
        segs.push(CompiledPathSegment::LineTo(x, y));
    }
    if close {
        segs.push(CompiledPathSegment::Close);
    }
    Ok(segs)
}

fn req_str(
    src: &str,
    tag_off: usize,
    attrs: &[RawAttr],
    key: &str,
) -> Result<(String, usize), CompileError> {
    attrs
        .iter()
        .find(|(k, _, _, _)| k == key)
        .map(|(_, v, _, off)| (v.clone(), *off))
        .ok_or_else(|| {
            CompileError::at_offset(
                src,
                tag_off,
                format!("Missing required SVG attribute `{key}`"),
            )
        })
}

fn opt_opt_f32(src: &str, attrs: &[RawAttr], key: &str) -> Result<Option<f32>, CompileError> {
    attrs
        .iter()
        .find(|(k, _, _, _)| k == key)
        .map(|(_, v, _, off)| parse_svg_len(src, *off, v))
        .transpose()
}

fn opt_nonneg_f32(src: &str, attrs: &[RawAttr], key: &str) -> Result<Option<f32>, CompileError> {
    let Some((_, v, _, off)) = attrs.iter().find(|(k, _, _, _)| k == key) else {
        return Ok(None);
    };
    let parsed = parse_svg_len(src, *off, v)?;
    if parsed < 0.0 {
        return Err(CompileError::at_offset(
            src,
            *off,
            format!("SVG `{key}` cannot be negative: `{v}`"),
        ));
    }
    Ok(Some(parsed))
}

fn opt_f32(src: &str, attrs: &[RawAttr], key: &str, default: f32) -> Result<f32, CompileError> {
    Ok(opt_opt_f32(src, attrs, key)?.unwrap_or(default))
}

fn req_nonneg_f32(
    src: &str,
    tag_off: usize,
    attrs: &[RawAttr],
    key: &str,
) -> Result<f32, CompileError> {
    let (v, off) = req_str(src, tag_off, attrs, key)?;
    let parsed = parse_svg_len(src, off, &v)?;
    if parsed < 0.0 {
        return Err(CompileError::at_offset(
            src,
            off,
            format!("SVG `{key}` cannot be negative: `{v}`"),
        ));
    }
    Ok(parsed)
}
