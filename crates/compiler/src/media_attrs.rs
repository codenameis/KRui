//! Attribute extraction and validation for `<img>`, `<svg>`, and `<a href>` elements.

use crate::element_attrs::ParsedElementAttrs;
use crate::html::RawAttr;
use crate::interpolation::parse_interpolated_text_strict;
use crate::svg_compile::{
    parse_svg_fill_rule, parse_svg_len, parse_svg_line_cap, parse_svg_line_join, parse_svg_paint,
    CompiledFillRule, CompiledLineCap, CompiledLineJoin, CompiledSvgColor,
};
use crate::{CompileError, CompiledTextPart, HtmlTag};

/// Aggregated media (`<img>`, `<a href>`, `<svg>`) fields stored inside `ParsedElementAttrs`.
#[derive(Clone, Debug, Default)]
pub(crate) struct ParsedMediaAttrs {
    pub(crate) img_src: Vec<CompiledTextPart>,
    pub(crate) img_alt: Vec<CompiledTextPart>,
    pub(crate) link_href: Vec<CompiledTextPart>,
    pub(crate) svg_root: SvgRootAttrs,
}

/// Parsed root presentation/viewBox attributes collected on an `<svg>` opening tag.
#[derive(Clone, Debug, Default)]
pub(crate) struct SvgRootAttrs {
    pub(crate) view_box_raw: Option<(String, usize)>,
    pub(crate) width_px: Option<f32>,
    pub(crate) height_px: Option<f32>,
    pub(crate) fill: Option<Option<CompiledSvgColor>>,
    pub(crate) stroke: Option<Option<CompiledSvgColor>>,
    pub(crate) stroke_width: Option<f32>,
    pub(crate) line_cap: Option<CompiledLineCap>,
    pub(crate) line_join: Option<CompiledLineJoin>,
    pub(crate) fill_rule: Option<CompiledFillRule>,
}

fn parse_nonneg_svg_len(
    source: &str,
    attr_offset: usize,
    k: &str,
    v: &str,
) -> Result<f32, CompileError> {
    let parsed = parse_svg_len(source, attr_offset, v)?;
    if parsed < 0.0 {
        return Err(CompileError::at_offset(
            source,
            attr_offset,
            format!("SVG `{k}` cannot be negative: `{v}`"),
        ));
    }
    Ok(parsed)
}

fn apply_svg_root_attr(
    source: &str,
    k: &str,
    v: &str,
    attr_offset: usize,
    svg: &mut SvgRootAttrs,
) -> Result<bool, CompileError> {
    match k {
        "width" | "height" => {
            let parsed = parse_nonneg_svg_len(source, attr_offset, k, v)?;
            if k == "width" {
                svg.width_px = Some(parsed);
            } else {
                svg.height_px = Some(parsed);
            }
            Ok(false)
        }
        "viewbox" => {
            svg.view_box_raw = Some((v.to_string(), attr_offset));
            Ok(true)
        }
        "fill" => {
            svg.fill = Some(parse_svg_paint(source, attr_offset, v)?);
            Ok(true)
        }
        "stroke" => {
            svg.stroke = Some(parse_svg_paint(source, attr_offset, v)?);
            Ok(true)
        }
        "stroke-width" => {
            svg.stroke_width = Some(parse_nonneg_svg_len(source, attr_offset, k, v)?);
            Ok(true)
        }
        "stroke-linecap" => {
            svg.line_cap = Some(parse_svg_line_cap(source, attr_offset, v)?);
            Ok(true)
        }
        "stroke-linejoin" => {
            svg.line_join = Some(parse_svg_line_join(source, attr_offset, v)?);
            Ok(true)
        }
        "fill-rule" => {
            svg.fill_rule = Some(parse_svg_fill_rule(source, attr_offset, v)?);
            Ok(true)
        }
        "xmlns" | "focusable" => Ok(true),
        _ => Ok(false),
    }
}

/// Applies media (`<img>`), hyperlink (`<a>`), or `<svg>` attributes if `k` matches.
pub(crate) fn apply_media_attr(
    source: &str,
    tag: HtmlTag,
    k: &str,
    v: &str,
    attr_offset: usize,
    out: &mut ParsedElementAttrs,
) -> Result<bool, CompileError> {
    match k {
        "src" if tag == HtmlTag::Img => {
            out.media.img_src = parse_interpolated_text_strict(source, attr_offset, v)?;
            Ok(true)
        }
        "alt" if tag == HtmlTag::Img => {
            out.media.img_alt = parse_interpolated_text_strict(source, attr_offset, v)?;
            Ok(true)
        }
        "src" | "alt" => Err(CompileError::at_offset(
            source,
            attr_offset,
            format!(
                "Attribute `{k}` is only valid on `<img>`, not `<{}>`",
                tag.as_str()
            ),
        )),
        _ if tag == HtmlTag::Svg => {
            apply_svg_root_attr(source, k, v, attr_offset, &mut out.media.svg_root)
        }
        _ => Ok(false),
    }
}

/// Finalizes and validates media constraints after all element attributes have been scanned.
pub(crate) fn finalize_media_attrs(
    source: &str,
    tag: HtmlTag,
    attributes: &[RawAttr],
    out: &mut ParsedElementAttrs,
) -> Result<(), CompileError> {
    if tag == HtmlTag::Img {
        if out.media.img_src.is_empty() {
            let fallback_offset = attributes.first().map_or(0, |a| a.3);
            return Err(CompileError::at_offset(
                source,
                fallback_offset,
                "`<img>` element requires a `src` attribute",
            ));
        }
        if out.aria_label_parts.is_empty() && !out.media.img_alt.is_empty() {
            if out
                .media
                .img_alt
                .iter()
                .all(|p| matches!(p, CompiledTextPart::Static(_)))
            {
                let mut combined = String::new();
                for p in &out.media.img_alt {
                    if let CompiledTextPart::Static(s) = p {
                        combined.push_str(s);
                    }
                }
                out.aria_label = Some(combined);
            }
            out.aria_label_parts = out.media.img_alt.clone();
        }
    }
    Ok(())
}
