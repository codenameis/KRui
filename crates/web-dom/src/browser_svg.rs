//! Native browser `<svg>` child element generation for compiled `SvgVectorTemplate` icons.

use krui_style::Color;
use krui_ui_types::{
    CanvasFillRule, CanvasLineCap, CanvasLineJoin, PathSegment, SvgColor, SvgVectorOp,
    SvgVectorTemplate,
};
use std::fmt::Write;

pub(super) const SVG_NS: &str = "http://www.w3.org/2000/svg";

pub(super) fn populate_svg_children(
    doc: &web_sys::Document,
    svg: &web_sys::Element,
    template: &SvgVectorTemplate,
) {
    let [vx, vy, vw, vh] = template.view_box;
    let _ = svg.set_attribute("viewBox", &format!("{vx} {vy} {vw} {vh}"));
    let _ = svg.set_attribute("fill", "none");
    svg.set_inner_html("");
    for op in template.ops.iter() {
        if let Some(child) = build_svg_op_element(doc, op) {
            let _ = svg.append_child(&child);
        }
    }
}

fn css_hex_or_rgba(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("rgba({}, {}, {}, {:.4})", c.r, c.g, c.b, c.a as f32 / 255.0)
    }
}

fn svg_color_str(c: SvgColor) -> String {
    match c {
        SvgColor::CurrentColor => "currentColor".to_string(),
        SvgColor::Explicit(rgba) => css_hex_or_rgba(rgba),
    }
}

fn path_segments_to_d(segments: &[PathSegment]) -> String {
    let mut d = String::with_capacity(segments.len() * 16);
    for &seg in segments {
        match seg {
            PathSegment::MoveTo(x, y) => {
                let _ = write!(d, "M{x:.3} {y:.3} ");
            }
            PathSegment::LineTo(x, y) => {
                let _ = write!(d, "L{x:.3} {y:.3} ");
            }
            PathSegment::QuadTo { cx, cy, x, y } => {
                let _ = write!(d, "Q{cx:.3} {cy:.3} {x:.3} {y:.3} ");
            }
            PathSegment::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                let _ = write!(d, "C{c1x:.3} {c1y:.3} {c2x:.3} {c2y:.3} {x:.3} {y:.3} ");
            }
            PathSegment::Arc { .. } => {}
            PathSegment::Close => d.push_str("Z "),
        }
    }
    d
}

fn build_svg_op_element(doc: &web_sys::Document, op: &SvgVectorOp) -> Option<web_sys::Element> {
    match op {
        SvgVectorOp::FillRect {
            x,
            y,
            w,
            h,
            radius,
            color,
        } => build_svg_rect(doc, *x, *y, *w, *h, Some(*radius), None, *color),
        SvgVectorOp::StrokeRect {
            x,
            y,
            w,
            h,
            line_width,
            color,
        } => build_svg_rect(doc, *x, *y, *w, *h, None, Some(*line_width), *color),
        SvgVectorOp::FillCircle {
            cx,
            cy,
            radius,
            color,
        } => build_svg_circle(doc, *cx, *cy, *radius, None, *color),
        SvgVectorOp::StrokeCircle {
            cx,
            cy,
            radius,
            line_width,
            color,
        } => build_svg_circle(doc, *cx, *cy, *radius, Some(*line_width), *color),
        SvgVectorOp::FillPath {
            segments,
            fill_rule,
            color,
        } => {
            let el = doc.create_element_ns(Some(SVG_NS), "path").ok()?;
            let _ = el.set_attribute("d", &path_segments_to_d(segments));
            let _ = el.set_attribute("fill", &svg_color_str(*color));
            if *fill_rule == CanvasFillRule::EvenOdd {
                let _ = el.set_attribute("fill-rule", "evenodd");
            }
            Some(el)
        }
        SvgVectorOp::StrokePath {
            segments,
            line_width,
            line_cap,
            line_join,
            color,
        } => build_svg_stroke_path(doc, segments, *line_width, *line_cap, *line_join, *color),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_svg_rect(
    doc: &web_sys::Document,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fill_radius: Option<f32>,
    stroke_width: Option<f32>,
    color: SvgColor,
) -> Option<web_sys::Element> {
    let el = doc.create_element_ns(Some(SVG_NS), "rect").ok()?;
    let _ = el.set_attribute("x", &x.to_string());
    let _ = el.set_attribute("y", &y.to_string());
    let _ = el.set_attribute("width", &w.to_string());
    let _ = el.set_attribute("height", &h.to_string());
    if let Some(radius) = fill_radius {
        if radius > 0.0 {
            let _ = el.set_attribute("rx", &radius.to_string());
        }
        let _ = el.set_attribute("fill", &svg_color_str(color));
    }
    if let Some(lw) = stroke_width {
        let _ = el.set_attribute("stroke", &svg_color_str(color));
        let _ = el.set_attribute("stroke-width", &lw.to_string());
    }
    Some(el)
}

fn build_svg_circle(
    doc: &web_sys::Document,
    cx: f32,
    cy: f32,
    radius: f32,
    stroke_width: Option<f32>,
    color: SvgColor,
) -> Option<web_sys::Element> {
    let el = doc.create_element_ns(Some(SVG_NS), "circle").ok()?;
    let _ = el.set_attribute("cx", &cx.to_string());
    let _ = el.set_attribute("cy", &cy.to_string());
    let _ = el.set_attribute("r", &radius.to_string());
    match stroke_width {
        None => {
            let _ = el.set_attribute("fill", &svg_color_str(color));
        }
        Some(lw) => {
            let _ = el.set_attribute("stroke", &svg_color_str(color));
            let _ = el.set_attribute("stroke-width", &lw.to_string());
        }
    }
    Some(el)
}

fn build_svg_stroke_path(
    doc: &web_sys::Document,
    segments: &[PathSegment],
    line_width: f32,
    line_cap: CanvasLineCap,
    line_join: CanvasLineJoin,
    color: SvgColor,
) -> Option<web_sys::Element> {
    let el = doc.create_element_ns(Some(SVG_NS), "path").ok()?;
    let _ = el.set_attribute("d", &path_segments_to_d(segments));
    let _ = el.set_attribute("stroke", &svg_color_str(color));
    let _ = el.set_attribute("stroke-width", &line_width.to_string());
    let cap = match line_cap {
        CanvasLineCap::Butt => "butt",
        CanvasLineCap::Round => "round",
        CanvasLineCap::Square => "square",
    };
    let join = match line_join {
        CanvasLineJoin::Miter => "miter",
        CanvasLineJoin::Round => "round",
        CanvasLineJoin::Bevel => "bevel",
    };
    let _ = el.set_attribute("stroke-linecap", cap);
    let _ = el.set_attribute("stroke-linejoin", join);
    Some(el)
}
