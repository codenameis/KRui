//! Compile-time SVG body parser and vector IR generator (`<svg>` -> [`CompiledSvgData`]).
//!
//! Unknown child elements or attributes fail fast with [`CompileError`].

use crate::html::{consume_closing_tag, parse_tag_open, skip_whitespace_and_comments, RawAttr};
use crate::media_attrs::SvgRootAttrs;
use crate::svg_path::CompiledPathSegment;
use crate::svg_shapes::emit_svg_leaf_shape;
use crate::CompileError;
use krui_style::Color;

/// Serializable SVG fill/stroke color (explicit CSS color or `currentColor`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CompiledSvgColor {
    /// Explicit RGBA color.
    Explicit(Color),
    /// Resolves to the node's CSS `color` at paint time.
    CurrentColor,
}

/// Serializable SVG `stroke-linecap` mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CompiledLineCap {
    #[default]
    Butt,
    Round,
    Square,
}

/// Serializable SVG `stroke-linejoin` mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CompiledLineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

/// Serializable SVG `fill-rule` parity mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CompiledFillRule {
    #[default]
    NonZero,
    EvenOdd,
}

/// Serializable vector drawing primitive in a [`CompiledSvgData`].
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CompiledSvgOp {
    /// Filled rounded rectangle.
    FillRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        color: CompiledSvgColor,
    },
    /// Stroked rectangle.
    StrokeRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        line_width: f32,
        color: CompiledSvgColor,
    },
    /// Filled circle.
    FillCircle {
        cx: f32,
        cy: f32,
        radius: f32,
        color: CompiledSvgColor,
    },
    /// Stroked circle.
    StrokeCircle {
        cx: f32,
        cy: f32,
        radius: f32,
        line_width: f32,
        color: CompiledSvgColor,
    },
    /// Filled vector path.
    FillPath {
        segments: Vec<CompiledPathSegment>,
        fill_rule: CompiledFillRule,
        color: CompiledSvgColor,
    },
    /// Stroked vector path.
    StrokePath {
        segments: Vec<CompiledPathSegment>,
        line_width: f32,
        line_cap: CompiledLineCap,
        line_join: CompiledLineJoin,
        color: CompiledSvgColor,
    },
}

/// Compiled resolution-independent vector `<svg>` template stored on [`crate::CompiledNode`].
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CompiledSvgData {
    /// Normalized `[min_x, min_y, width, height]`.
    pub view_box: [f32; 4],
    /// Intrinsic CSS `(width, height)` fallback in logical px.
    pub intrinsic_size: (f32, f32),
    /// Flattened drawing operations in `view_box` coordinates.
    pub ops: Vec<CompiledSvgOp>,
}

#[derive(Clone, Copy)]
pub(crate) struct PaintInherit {
    pub(crate) fill: Option<CompiledSvgColor>,
    pub(crate) stroke: Option<CompiledSvgColor>,
    pub(crate) stroke_width: f32,
    pub(crate) line_cap: CompiledLineCap,
    pub(crate) line_join: CompiledLineJoin,
    pub(crate) fill_rule: CompiledFillRule,
}

/// Consumes everything inside `<svg ...>...</svg>`, advancing `cursor` past `</svg>`.
pub(crate) fn consume_svg_inner_body(
    input: &str,
    cursor: &mut usize,
    open_offset: usize,
) -> Result<(String, usize), CompileError> {
    let body_start = *cursor;
    let mut depth = 1usize;
    let mut quote: Option<char> = None;
    while *cursor < input.len() {
        let rest = &input[*cursor..];
        if let Some(q) = quote {
            let ch = rest.chars().next().expect("non-empty rest");
            *cursor += ch.len_utf8();
            if ch == q {
                quote = None;
            }
            continue;
        }
        if let Some(after_comment) = rest.strip_prefix("<!--") {
            let Some(end) = after_comment.find("-->") else {
                return Err(CompileError::at_offset(
                    input,
                    *cursor,
                    "Unclosed HTML comment inside `<svg>`",
                ));
            };
            *cursor += 4 + end + 3;
            continue;
        }
        let ch = rest.chars().next().expect("non-empty rest");
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            *cursor += 1;
        } else if starts_with_ci_tag(rest, "<svg") {
            let mut open_cur = *cursor;
            let (_, _, self_closing, _) = parse_tag_open(input, &mut open_cur)?;
            *cursor = open_cur;
            if !self_closing {
                depth += 1;
            }
        } else if starts_with_ci_tag(rest, "</svg") {
            let close_pos = *cursor;
            depth -= 1;
            consume_closing_tag(input, cursor, "svg", false)?;
            if depth == 0 {
                return Ok((input[body_start..close_pos].to_string(), body_start));
            }
        } else {
            *cursor += ch.len_utf8();
        }
    }
    Err(CompileError::at_offset(
        input,
        open_offset,
        "Unclosed `<svg>` element",
    ))
}

fn starts_with_ci_tag(rest: &str, prefix: &str) -> bool {
    rest.len() >= prefix.len()
        && rest[..prefix.len()].eq_ignore_ascii_case(prefix)
        && rest[prefix.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace() || c == '>' || c == '/')
}

/// Parses root `<svg>` attributes plus inner vector markup into [`CompiledSvgData`].
pub(crate) fn compile_svg_element(
    source: &str,
    tag_offset: usize,
    root: &SvgRootAttrs,
    inner_markup: &str,
    inner_offset: usize,
) -> Result<CompiledSvgData, CompileError> {
    let view_box = match &root.view_box_raw {
        Some((raw, off)) => parse_view_box(source, *off, raw)?,
        None => [
            0.0,
            0.0,
            root.width_px.unwrap_or(24.0).max(1.0),
            root.height_px.unwrap_or(24.0).max(1.0),
        ],
    };
    let intrinsic_size = (
        root.width_px.unwrap_or(view_box[2]).max(1.0),
        root.height_px.unwrap_or(view_box[3]).max(1.0),
    );
    let base_paint = PaintInherit {
        fill: root.fill.unwrap_or(Some(CompiledSvgColor::CurrentColor)),
        stroke: if root.stroke_width == Some(0.0) {
            None
        } else {
            root.stroke.unwrap_or(None)
        },
        stroke_width: root.stroke_width.unwrap_or(1.0),
        line_cap: root.line_cap.unwrap_or_default(),
        line_join: root.line_join.unwrap_or_default(),
        fill_rule: root.fill_rule.unwrap_or_default(),
    };
    let mut ops = Vec::new();
    let mut cursor = 0usize;
    parse_svg_children(inner_markup, &mut cursor, None, base_paint, &mut ops).map_err(
        |mut e| {
            e = offset_local_error(source, inner_offset, inner_markup, &e, tag_offset);
            e
        },
    )?;
    Ok(CompiledSvgData {
        view_box,
        intrinsic_size,
        ops,
    })
}

fn offset_local_error(
    full_src: &str,
    base_off: usize,
    inner: &str,
    local: &CompileError,
    fallback_off: usize,
) -> CompileError {
    let mut cur = 0usize;
    let mut line = 1usize;
    while cur < inner.len() && line < local.line {
        if let Some(nl) = inner[cur..].find('\n') {
            cur += nl + 1;
            line += 1;
        } else {
            break;
        }
    }
    let mut col_bytes = 0usize;
    for (chars_seen, (_, ch)) in inner[cur..].char_indices().enumerate() {
        if chars_seen >= local.column.saturating_sub(1) || ch == '\n' {
            break;
        }
        col_bytes += ch.len_utf8();
    }
    let byte_off = (base_off + cur + col_bytes).min(full_src.len());
    let final_off = if base_off <= full_src.len() {
        byte_off
    } else {
        fallback_off
    };
    CompileError::at_offset(full_src, final_off, local.message.clone())
}

fn parse_svg_children(
    markup: &str,
    cursor: &mut usize,
    closing_parent: Option<(&str, usize)>,
    inherit: PaintInherit,
    ops: &mut Vec<CompiledSvgOp>,
) -> Result<(), CompileError> {
    loop {
        skip_whitespace_and_comments(markup, cursor)?;
        if *cursor >= markup.len() {
            if let Some((tag, off)) = closing_parent {
                return Err(CompileError::at_offset(
                    markup,
                    off,
                    format!("Unclosed `<{tag}>` in `<svg>`"),
                ));
            }
            break;
        }
        if markup[*cursor..].starts_with("</") {
            break;
        }
        if !markup[*cursor..].starts_with('<') {
            return Err(CompileError::at_offset(
                markup,
                *cursor,
                "Unexpected stray text inside `<svg>`",
            ));
        }
        let (raw_tag, attrs, sc, off) = parse_tag_open(markup, cursor)?;
        let tag = raw_tag.to_ascii_lowercase();
        if tag == "g" {
            crate::svg_shapes::validate_shape_attrs(markup, "g", &attrs, &[])?;
            let group_paint = apply_paint_attrs(markup, &attrs, inherit)?;
            if !sc {
                parse_svg_children(markup, cursor, Some(("g", off)), group_paint, ops)?;
                consume_closing_tag(markup, cursor, "g", false)?;
            }
            continue;
        }
        emit_svg_leaf_shape(markup, &tag, off, &attrs, inherit, ops)?;
        if !sc {
            skip_whitespace_and_comments(markup, cursor)?;
            consume_closing_tag(markup, cursor, &tag, false)?;
        }
    }
    Ok(())
}

pub(crate) fn apply_paint_attrs(
    src: &str,
    attrs: &[RawAttr],
    base: PaintInherit,
) -> Result<PaintInherit, CompileError> {
    let mut out = base;
    let mut explicit_zero_sw = false;
    for (k, v, _, off) in attrs {
        match k.as_str() {
            "fill" => out.fill = parse_svg_paint(src, *off, v)?,
            "stroke" => out.stroke = parse_svg_paint(src, *off, v)?,
            "stroke-width" => {
                let sw = parse_svg_len(src, *off, v)?;
                if sw < 0.0 {
                    return Err(CompileError::at_offset(
                        src,
                        *off,
                        format!("SVG `stroke-width` cannot be negative: `{v}`"),
                    ));
                }
                explicit_zero_sw = sw == 0.0;
                out.stroke_width = sw;
            }
            "stroke-linecap" => out.line_cap = parse_svg_line_cap(src, *off, v)?,
            "stroke-linejoin" => out.line_join = parse_svg_line_join(src, *off, v)?,
            "fill-rule" => out.fill_rule = parse_svg_fill_rule(src, *off, v)?,
            _ => {}
        }
    }
    if explicit_zero_sw || out.stroke_width == 0.0 {
        out.stroke = None;
    }
    Ok(out)
}

pub(crate) fn parse_svg_line_cap(
    src: &str,
    off: usize,
    raw: &str,
) -> Result<CompiledLineCap, CompileError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "butt" => Ok(CompiledLineCap::Butt),
        "round" => Ok(CompiledLineCap::Round),
        "square" => Ok(CompiledLineCap::Square),
        _ => Err(CompileError::at_offset(
            src,
            off,
            format!("Invalid SVG `stroke-linecap` `{raw}` (expected `butt`, `round`, or `square`)"),
        )),
    }
}

pub(crate) fn parse_svg_line_join(
    src: &str,
    off: usize,
    raw: &str,
) -> Result<CompiledLineJoin, CompileError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "miter" => Ok(CompiledLineJoin::Miter),
        "round" => Ok(CompiledLineJoin::Round),
        "bevel" => Ok(CompiledLineJoin::Bevel),
        _ => Err(CompileError::at_offset(
            src,
            off,
            format!(
                "Invalid SVG `stroke-linejoin` `{raw}` (expected `miter`, `round`, or `bevel`)"
            ),
        )),
    }
}

pub(crate) fn parse_svg_fill_rule(
    src: &str,
    off: usize,
    raw: &str,
) -> Result<CompiledFillRule, CompileError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "nonzero" => Ok(CompiledFillRule::NonZero),
        "evenodd" => Ok(CompiledFillRule::EvenOdd),
        _ => Err(CompileError::at_offset(
            src,
            off,
            format!("Invalid SVG `fill-rule` `{raw}` (expected `nonzero` or `evenodd`)"),
        )),
    }
}

/// Parses `"none"`, `"transparent"`, `"currentColor"`, or any CSS color accepted by [`Color::parse_css`].
pub(crate) fn parse_svg_paint(
    src: &str,
    off: usize,
    raw: &str,
) -> Result<Option<CompiledSvgColor>, CompileError> {
    let t = raw.trim();
    if t.eq_ignore_ascii_case("none") || t.eq_ignore_ascii_case("transparent") {
        return Ok(None);
    }
    if t.eq_ignore_ascii_case("currentcolor") {
        return Ok(Some(CompiledSvgColor::CurrentColor));
    }
    Color::parse_css(t)
        .map(|c| Some(CompiledSvgColor::Explicit(c)))
        .ok_or_else(|| {
            CompileError::at_offset(src, off, format!("Invalid SVG paint color `{raw}`"))
        })
}

pub(crate) fn parse_svg_len(src: &str, off: usize, raw: &str) -> Result<f32, CompileError> {
    let t = raw.trim().strip_suffix("px").unwrap_or(raw.trim()).trim();
    let val = t.parse::<f32>().map_err(|_| {
        CompileError::at_offset(src, off, format!("Invalid numeric SVG length `{raw}`"))
    })?;
    if !val.is_finite() {
        return Err(CompileError::at_offset(
            src,
            off,
            format!("Non-finite SVG length `{raw}`"),
        ));
    }
    Ok(val)
}

fn parse_view_box(src: &str, off: usize, raw: &str) -> Result<[f32; 4], CompileError> {
    let nums: Result<Vec<f32>, _> = raw
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f32>())
        .collect();
    let vals = nums
        .map_err(|_| CompileError::at_offset(src, off, format!("Invalid SVG `viewBox` `{raw}`")))?;
    if vals.len() != 4 || vals.iter().any(|v| !v.is_finite()) || vals[2] <= 0.0 || vals[3] <= 0.0 {
        return Err(CompileError::at_offset(
            src,
            off,
            format!("SVG `viewBox` must be 4 finite numbers `min_x min_y width height` with positive width/height: `{raw}`"),
        ));
    }
    Ok([vals[0], vals[1], vals[2], vals[3]])
}
