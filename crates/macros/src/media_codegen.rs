//! Compile-time codegen for `<img>`, `<a href>`, and `<svg>` vector templates.

use crate::codegen_control_flow::emit_interpolated_slot;
use crate::expr_vars::{
    collect_free_idents_from_template_expr, emit_clone_lets, emit_template_expr,
};
use krui_compiler::{
    CompiledFillRule, CompiledLineCap, CompiledLineJoin, CompiledNode, CompiledPathSegment,
    CompiledSvgColor, CompiledSvgOp, CompiledTextPart,
};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use std::path::{Path, PathBuf};

/// Generates dynamic-only media slot setup tokens (`<img>`, dynamic `<a href>`, `<svg>`) when
/// static `link_href` was already emitted in `StaticNodeDesc`.
pub(crate) fn emit_dynamic_media_slots(
    node: &CompiledNode,
    source_base_dir: Option<&Path>,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let img_stmt = emit_img_src_slot(node, source_base_dir, span, loop_vars)?;
    let has_dyn_link = node
        .link_href
        .iter()
        .any(|p| matches!(p, CompiledTextPart::Dynamic { .. }));
    let link_stmt = if has_dyn_link {
        emit_interpolated_slot(
            &node.link_href,
            span,
            format_ident!("set_link_href"),
            format_ident!("bind_dynamic_link_href"),
            loop_vars,
        )?
    } else {
        quote! {}
    };
    let svg_stmt = emit_svg_template_slot(node);
    Ok(quote! { #img_stmt #link_stmt #svg_stmt })
}

/// Generates media slot setup tokens (`<img>`, `<a href>`, `<svg>`) for an element node.
#[allow(dead_code)]
pub(crate) fn emit_media_slots(
    node: &CompiledNode,
    source_base_dir: Option<&Path>,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    let img_stmt = emit_img_src_slot(node, source_base_dir, span, loop_vars)?;
    let link_stmt = emit_interpolated_slot(
        &node.link_href,
        span,
        format_ident!("set_link_href"),
        format_ident!("bind_dynamic_link_href"),
        loop_vars,
    )?;
    let svg_stmt = emit_svg_template_slot(node);
    Ok(quote! { #img_stmt #link_stmt #svg_stmt })
}

fn resolve_static_asset_path(raw: &str, base_dir: Option<&Path>) -> Option<PathBuf> {
    let p = Path::new(raw);
    if p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    if let Some(dir) = base_dir {
        let candidate = dir.join(p);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR") {
        let candidate = PathBuf::from(manifest).join(p);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn emit_img_src_slot(
    node: &CompiledNode,
    source_base_dir: Option<&Path>,
    span: Span,
    loop_vars: &[String],
) -> Result<TokenStream2, syn::Error> {
    if node.img_src.is_empty() {
        return Ok(quote! {});
    }
    let has_dyn = node
        .img_src
        .iter()
        .any(|p| matches!(p, CompiledTextPart::Dynamic { .. }));
    if !has_dyn {
        return emit_static_img_src_slot(node, source_base_dir, span);
    }
    let mut idents = Vec::new();
    let mut segment_pushers = Vec::with_capacity(node.img_src.len());
    for part in &node.img_src {
        match part {
            CompiledTextPart::Static(s) => segment_pushers.push(quote! { __buf.push_str(#s); }),
            CompiledTextPart::Dynamic { expr, .. } => {
                let expr_toks = emit_template_expr(expr, span, loop_vars)?;
                collect_free_idents_from_template_expr(expr, span, loop_vars, &mut idents)?;
                segment_pushers.push(quote! {
                    __buf.push_str(&::std::format!("{}", #expr_toks));
                });
            }
        }
    }
    let clones = emit_clone_lets(&idents);
    if let Some(dir) = source_base_dir {
        let dir_str = dir.to_string_lossy().into_owned();
        Ok(quote! {{
            #clones
            __tree.bind_dynamic_image_src_in_dir(
                __node_id,
                ::std::path::PathBuf::from(#dir_str),
                move || {
                    let mut __buf = ::std::string::String::new();
                    #(#segment_pushers)*
                    __buf
                },
            );
        }})
    } else {
        Ok(quote! {{
            #clones
            __tree.bind_dynamic_image_src(__node_id, move || {
                let mut __buf = ::std::string::String::new();
                #(#segment_pushers)*
                __buf
            });
        }})
    }
}

fn read_bounded_static_image_bytes(
    path: &Path,
    raw: &str,
    span: Span,
) -> Result<Vec<u8>, syn::Error> {
    const MAX_STATIC_IMAGE_FILE_BYTES: u64 = 64 * 1024 * 1024;
    let meta = std::fs::metadata(path).map_err(|e| {
        syn::Error::new(
            span,
            format!(
                "Failed to stat static `<img>` src `{raw}` (`{}`): {e}",
                path.display()
            ),
        )
    })?;
    if meta.len() > MAX_STATIC_IMAGE_FILE_BYTES {
        return Err(syn::Error::new(
            span,
            format!(
                "Static `<img>` src `{raw}` (`{}`) exceeds 64 MiB compile-time limit",
                path.display()
            ),
        ));
    }
    std::fs::read(path).map_err(|e| {
        syn::Error::new(
            span,
            format!(
                "Failed to read static `<img>` src `{raw}` (`{}`): {e}",
                path.display()
            ),
        )
    })
}

fn validate_static_image_file(path: &Path, raw: &str, span: Span) -> Result<(), syn::Error> {
    const MAX_IMAGE_DIM: u32 = 16_384;
    const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;

    let bytes = read_bounded_static_image_bytes(path, raw, span)?;
    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| {
            syn::Error::new(
                span,
                format!(
                    "Invalid static `<img>` asset `{raw}` (`{}`): {e}",
                    path.display()
                ),
            )
        })?;
    let (w, h) = reader.into_dimensions().map_err(|e| {
        syn::Error::new(
            span,
            format!(
                "Corrupt static `<img>` asset `{raw}` (`{}`): {e}",
                path.display()
            ),
        )
    })?;
    let decoded_bytes = (w as u64).saturating_mul(h as u64).saturating_mul(4);
    if w == 0
        || h == 0
        || w > MAX_IMAGE_DIM
        || h > MAX_IMAGE_DIM
        || decoded_bytes > MAX_DECODED_BYTES
    {
        return Err(syn::Error::new(
            span,
            format!(
                "Static `<img>` asset `{raw}` dimensions {w}x{h} exceed safe limit ({MAX_IMAGE_DIM}x{MAX_IMAGE_DIM}, 256 MiB)"
            ),
        ));
    }
    image::load_from_memory(&bytes).map_err(|e| {
        syn::Error::new(
            span,
            format!(
                "Failed to decode static `<img>` asset `{raw}` (`{}`): {e}",
                path.display()
            ),
        )
    })?;
    Ok(())
}

fn emit_static_img_src_slot(
    node: &CompiledNode,
    source_base_dir: Option<&Path>,
    span: Span,
) -> Result<TokenStream2, syn::Error> {
    let mut combined = String::new();
    for part in &node.img_src {
        if let CompiledTextPart::Static(s) = part {
            combined.push_str(s);
        }
    }
    let trimmed = combined.trim();
    let resolved = resolve_static_asset_path(trimmed, source_base_dir).ok_or_else(|| {
        let searched = source_base_dir
            .map(|p| p.display().to_string())
            .or_else(|| std::env::var("CARGO_MANIFEST_DIR").ok())
            .unwrap_or_else(|| ".".to_string());
        syn::Error::new(
            span,
            format!("Static `<img>` src `{trimmed}` not found relative to `{searched}`"),
        )
    })?;
    validate_static_image_file(&resolved, trimmed, span)?;
    let abs_str = resolved.to_string_lossy().into_owned();
    Ok(quote! {{
        ::krui::__private::core::static_ir::mount_static_image_bytes(
            __tree,
            __node_id,
            include_bytes!(#abs_str),
        );
    }})
}

fn emit_svg_color(color: CompiledSvgColor) -> TokenStream2 {
    match color {
        CompiledSvgColor::CurrentColor => quote! { ::krui::SvgColor::CurrentColor },
        CompiledSvgColor::Explicit(c) => {
            let (r, g, b, a) = (c.r, c.g, c.b, c.a);
            quote! { ::krui::SvgColor::Explicit(::krui::__private::style::Color { r: #r, g: #g, b: #b, a: #a }) }
        }
    }
}

fn emit_path_segment(seg: &CompiledPathSegment) -> TokenStream2 {
    match *seg {
        CompiledPathSegment::MoveTo(x, y) => quote! { ::krui::PathSegment::MoveTo(#x, #y) },
        CompiledPathSegment::LineTo(x, y) => quote! { ::krui::PathSegment::LineTo(#x, #y) },
        CompiledPathSegment::QuadTo { cx, cy, x, y } => {
            quote! { ::krui::PathSegment::QuadTo { cx: #cx, cy: #cy, x: #x, y: #y } }
        }
        CompiledPathSegment::CubicTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
        } => quote! {
            ::krui::PathSegment::CubicTo { c1x: #c1x, c1y: #c1y, c2x: #c2x, c2y: #c2y, x: #x, y: #y }
        },
        CompiledPathSegment::Arc {
            cx,
            cy,
            rx,
            ry,
            start_rad,
            sweep_rad,
            x_rotation_rad,
        } => quote! {
            ::krui::PathSegment::Arc {
                cx: #cx, cy: #cy, rx: #rx, ry: #ry,
                start_rad: #start_rad, sweep_rad: #sweep_rad, x_rotation_rad: #x_rotation_rad
            }
        },
        CompiledPathSegment::Close => quote! { ::krui::PathSegment::Close },
    }
}

fn emit_svg_fill_rule(rule: CompiledFillRule) -> TokenStream2 {
    match rule {
        CompiledFillRule::NonZero => quote! { ::krui::__private::core::CanvasFillRule::NonZero },
        CompiledFillRule::EvenOdd => quote! { ::krui::__private::core::CanvasFillRule::EvenOdd },
    }
}

fn emit_svg_line_cap(cap: CompiledLineCap) -> TokenStream2 {
    match cap {
        CompiledLineCap::Butt => quote! { ::krui::__private::core::CanvasLineCap::Butt },
        CompiledLineCap::Round => quote! { ::krui::__private::core::CanvasLineCap::Round },
        CompiledLineCap::Square => quote! { ::krui::__private::core::CanvasLineCap::Square },
    }
}

fn emit_svg_line_join(join: CompiledLineJoin) -> TokenStream2 {
    match join {
        CompiledLineJoin::Miter => quote! { ::krui::__private::core::CanvasLineJoin::Miter },
        CompiledLineJoin::Round => quote! { ::krui::__private::core::CanvasLineJoin::Round },
        CompiledLineJoin::Bevel => quote! { ::krui::__private::core::CanvasLineJoin::Bevel },
    }
}

fn emit_svg_op(op: &CompiledSvgOp) -> TokenStream2 {
    match op {
        CompiledSvgOp::FillRect {
            x,
            y,
            w,
            h,
            radius,
            color,
        } => {
            let col = emit_svg_color(*color);
            quote! { ::krui::SvgVectorOp::FillRect { x: #x, y: #y, w: #w, h: #h, radius: #radius, color: #col } }
        }
        CompiledSvgOp::StrokeRect {
            x,
            y,
            w,
            h,
            line_width,
            color,
        } => {
            let col = emit_svg_color(*color);
            quote! { ::krui::SvgVectorOp::StrokeRect { x: #x, y: #y, w: #w, h: #h, line_width: #line_width, color: #col } }
        }
        CompiledSvgOp::FillCircle {
            cx,
            cy,
            radius,
            color,
        } => {
            let col = emit_svg_color(*color);
            quote! { ::krui::SvgVectorOp::FillCircle { cx: #cx, cy: #cy, radius: #radius, color: #col } }
        }
        CompiledSvgOp::StrokeCircle {
            cx,
            cy,
            radius,
            line_width,
            color,
        } => {
            let col = emit_svg_color(*color);
            quote! { ::krui::SvgVectorOp::StrokeCircle { cx: #cx, cy: #cy, radius: #radius, line_width: #line_width, color: #col } }
        }
        _ => emit_svg_path_op(op),
    }
}

fn emit_svg_path_op(op: &CompiledSvgOp) -> TokenStream2 {
    match op {
        CompiledSvgOp::FillPath {
            segments,
            fill_rule,
            color,
        } => {
            let col = emit_svg_color(*color);
            let fr = emit_svg_fill_rule(*fill_rule);
            let seg_toks: Vec<_> = segments.iter().map(emit_path_segment).collect();
            quote! { ::krui::SvgVectorOp::FillPath { segments: ::std::vec![#(#seg_toks),*], fill_rule: #fr, color: #col } }
        }
        CompiledSvgOp::StrokePath {
            segments,
            line_width,
            line_cap,
            line_join,
            color,
        } => {
            let col = emit_svg_color(*color);
            let lc = emit_svg_line_cap(*line_cap);
            let lj = emit_svg_line_join(*line_join);
            let seg_toks: Vec<_> = segments.iter().map(emit_path_segment).collect();
            quote! { ::krui::SvgVectorOp::StrokePath { segments: ::std::vec![#(#seg_toks),*], line_width: #line_width, line_cap: #lc, line_join: #lj, color: #col } }
        }
        _ => quote! {},
    }
}

fn emit_svg_template_slot(node: &CompiledNode) -> TokenStream2 {
    let Some(svg) = &node.svg_data else {
        return quote! {};
    };
    let [vx, vy, vw, vh] = svg.view_box;
    let (iw, ih) = svg.intrinsic_size;
    let ops: Vec<_> = svg.ops.iter().map(emit_svg_op).collect();
    quote! {
        #[allow(clippy::approx_constant)]
        __tree.set_node_svg_template(
            __node_id,
            ::krui::SvgVectorTemplate {
                view_box: [#vx, #vy, #vw, #vh],
                intrinsic_size: (#iw, #ih),
                ops: ::std::sync::Arc::from(::std::vec![#(#ops),*]),
            },
        );
    }
}
