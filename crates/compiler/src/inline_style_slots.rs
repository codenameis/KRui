//! Inline `style="..."` and HTML presentational attribute analyzer.
//!
//! Splits declarations into static inline CSS passed directly to Stylo, fast-path compositor
//! [`PaintSlotBinding`]s, localized [`LayoutSlotBinding`]s (with `LayoutPerformanceCliff` warnings),
//! and dynamic reactive `style="..."` string templates.

use crate::element_attrs::ParsedElementAttrs;
use crate::expr::TemplateExpr;
use crate::html_lexer::scan_template_expr_hole;
use crate::interpolation::{extract_px_hole, extract_single_hole, parse_interpolated_text_strict};
use crate::{
    CompileError, CompileWarning, CompileWarningKind, CompiledTextPart, HtmlTag, LayoutSlot,
    LayoutSlotBinding, PaintSlot, PaintSlotBinding,
};
use krui_style::UserSelect;

pub(crate) struct InlineStyleAnalysis {
    pub(crate) static_inline_style: Option<String>,
    pub(crate) dynamic_inline_style_parts: Vec<CompiledTextPart>,
    pub(crate) paint_bindings: Vec<PaintSlotBinding>,
    pub(crate) layout_bindings: Vec<LayoutSlotBinding>,
    pub(crate) warnings: Vec<CompileWarning>,
    pub(crate) explicit_flex_dir: bool,
    pub(crate) explicit_align_items: bool,
}

pub(crate) fn analyze_element_inline_style(
    source: &str,
    tag: HtmlTag,
    attrs: &ParsedElementAttrs,
) -> Result<InlineStyleAnalysis, CompileError> {
    let mut static_decls = build_static_media_and_select_decls(tag, attrs);
    let mut dynamic_decls: Vec<(usize, String)> = Vec::new();
    let mut paint_bindings = Vec::new();
    let mut layout_bindings = Vec::new();
    let mut warnings = Vec::new();
    let mut explicit_flex_dir = false;
    let mut explicit_align_items = false;

    process_html_layout_attrs(
        source,
        tag,
        attrs,
        &mut static_decls,
        &mut dynamic_decls,
        &mut paint_bindings,
        &mut layout_bindings,
        &mut warnings,
    )?;

    if let Some((raw_style, attr_off)) = &attrs.inline_style_raw {
        for (decl_str, rel_off) in split_inline_decls(source, *attr_off, raw_style)? {
            let trimmed = decl_str.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Some(colon) = trimmed.find(':') else {
                static_decls.push(trimmed.to_string());
                continue;
            };
            let prop = trimmed[..colon].trim().to_ascii_lowercase();
            let val = trimmed[colon + 1..].trim();
            if prop == "flex-direction" {
                explicit_flex_dir = true;
            } else if prop == "align-items" {
                explicit_align_items = true;
            }
            if !val.contains('{') && !val.contains('}') {
                static_decls.push(format!("{prop}: {val}"));
                continue;
            }
            let decl_off = *attr_off + rel_off + colon + 1;
            if try_classify_fast_slot(
                source,
                decl_off,
                tag,
                (&prop, val),
                &mut paint_bindings,
                &mut layout_bindings,
                &mut warnings,
            )? {
                continue;
            }
            let _ = parse_interpolated_text_strict(source, decl_off, val)?;
            if is_layout_cliff_prop(&prop) && is_frame_driven_signal(val, val) {
                warnings.push(build_layout_cliff_warning(tag, &prop, val));
            }
            dynamic_decls.push((decl_off, format!("{prop}: {val}")));
        }
    }

    let (static_inline_style, dynamic_inline_style_parts) =
        finalize_inline_style_parts(source, static_decls, dynamic_decls)?;
    Ok(InlineStyleAnalysis {
        static_inline_style,
        dynamic_inline_style_parts,
        paint_bindings,
        layout_bindings,
        warnings,
        explicit_flex_dir,
        explicit_align_items,
    })
}

fn build_static_media_and_select_decls(tag: HtmlTag, attrs: &ParsedElementAttrs) -> Vec<String> {
    let mut out = Vec::new();
    if tag == HtmlTag::Svg {
        if let Some(w) = attrs.media.svg_root.width_px {
            out.push(format!("width: {w}px"));
        }
        if let Some(h) = attrs.media.svg_root.height_px {
            out.push(format!("height: {h}px"));
        }
    }
    if let Some(sel) = attrs.selectable_attr {
        let val = match sel {
            UserSelect::None => "none",
            UserSelect::Text => "text",
            UserSelect::All => "all",
            UserSelect::Auto => "auto",
        };
        out.push(format!("user-select: {val}"));
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn process_html_layout_attrs(
    source: &str,
    tag: HtmlTag,
    attrs: &ParsedElementAttrs,
    static_decls: &mut Vec<String>,
    dynamic_decls: &mut Vec<(usize, String)>,
    paint_bindings: &mut Vec<PaintSlotBinding>,
    layout_bindings: &mut Vec<LayoutSlotBinding>,
    warnings: &mut Vec<CompileWarning>,
) -> Result<(), CompileError> {
    for (k, v, off) in &attrs.html_layout_attrs {
        let css_prop = if k == "text-direction" {
            "direction"
        } else {
            k.as_str()
        };
        if !v.contains('{') && !v.contains('}') {
            static_decls.push(format!("{css_prop}: {v}"));
            continue;
        }
        if try_classify_fast_slot(
            source,
            *off,
            tag,
            (css_prop, v.as_str()),
            paint_bindings,
            layout_bindings,
            warnings,
        )? {
            continue;
        }
        let _ = parse_interpolated_text_strict(source, *off, v)?;
        dynamic_decls.push((*off, format!("{css_prop}: {v}")));
    }
    Ok(())
}

fn finalize_inline_style_parts(
    source: &str,
    static_decls: Vec<String>,
    dynamic_decls: Vec<(usize, String)>,
) -> Result<(Option<String>, Vec<CompiledTextPart>), CompileError> {
    if dynamic_decls.is_empty() {
        let static_str = if static_decls.is_empty() {
            None
        } else {
            Some(static_decls.join("; "))
        };
        return Ok((static_str, Vec::new()));
    }
    let mut parts = Vec::new();
    if !static_decls.is_empty() {
        parts.push(CompiledTextPart::Static(format!(
            "{}; ",
            static_decls.join("; ")
        )));
    }
    let total = dynamic_decls.len();
    for (idx, (off, decl)) in dynamic_decls.into_iter().enumerate() {
        parts.extend(parse_interpolated_text_strict(source, off, &decl)?);
        if idx + 1 < total {
            parts.push(CompiledTextPart::Static("; ".to_string()));
        }
    }
    Ok((None, merge_adjacent_static_parts(parts)))
}

fn merge_adjacent_static_parts(parts: Vec<CompiledTextPart>) -> Vec<CompiledTextPart> {
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        if let (Some(CompiledTextPart::Static(prev)), CompiledTextPart::Static(next)) =
            (out.last_mut(), &part)
        {
            prev.push_str(next);
        } else {
            out.push(part);
        }
    }
    out
}

fn split_inline_decls<'a>(
    source: &str,
    attr_off: usize,
    raw: &'a str,
) -> Result<Vec<(&'a str, usize)>, CompileError> {
    let bytes = raw.as_bytes();
    let (mut out, mut start, mut i, mut paren, mut quote) = (Vec::new(), 0, 0, 0usize, None::<u8>);
    while i < bytes.len() {
        if let Some(q) = quote {
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if bytes[i] == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match bytes[i] {
            b'"' | b'\'' => {
                quote = Some(bytes[i]);
                i += 1;
            }
            b'{' => {
                i = scan_template_expr_hole(raw, i)
                    .map_err(|msg| CompileError::at_offset(source, attr_off + i, msg))?
                    + 1;
            }
            b'(' => {
                paren += 1;
                i += 1;
            }
            b')' => {
                paren = paren.saturating_sub(1);
                i += 1;
            }
            b';' if paren == 0 => {
                out.push((&raw[start..i], start));
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    if quote.is_some() {
        return Err(CompileError::at_offset(
            source,
            attr_off,
            "Unclosed string quote inside `style`",
        ));
    }
    if start < raw.len() {
        out.push((&raw[start..], start));
    }
    Ok(out)
}

fn try_classify_fast_slot(
    _source: &str,
    _decl_off: usize,
    tag: HtmlTag,
    (prop, val): (&str, &str),
    paint: &mut Vec<PaintSlotBinding>,
    layout: &mut Vec<LayoutSlotBinding>,
    warnings: &mut Vec<CompileWarning>,
) -> Result<bool, CompileError> {
    if prop == "opacity" {
        if let Some((sig, expr)) = extract_single_hole(val)? {
            paint.push(PaintSlotBinding {
                slot: PaintSlot::Opacity,
                signal_ident: sig,
                expr,
            });
            return Ok(true);
        }
        return Ok(false);
    }
    if prop == "transform" {
        return try_classify_transform_slots(val, paint);
    }
    if let Some(slot) = match prop {
        "width" => Some(LayoutSlot::WidthPx),
        "height" => Some(LayoutSlot::HeightPx),
        "top" => Some(LayoutSlot::TopPx),
        "left" => Some(LayoutSlot::LeftPx),
        "right" => Some(LayoutSlot::RightPx),
        "bottom" => Some(LayoutSlot::BottomPx),
        "margin" => Some(LayoutSlot::MarginPx),
        "padding" => Some(LayoutSlot::PaddingPx),
        "gap" => Some(LayoutSlot::GapPx),
        _ => None,
    } {
        if let Some((sig, expr)) = extract_px_hole(val)? {
            if is_frame_driven_signal(val, &sig) {
                warnings.push(build_slot_cliff_warning(tag, prop, &sig));
            }
            layout.push(LayoutSlotBinding {
                slot,
                signal_ident: sig,
                expr,
            });
            return Ok(true);
        }
    }
    Ok(false)
}

fn try_classify_transform_slots(
    val: &str,
    paint: &mut Vec<PaintSlotBinding>,
) -> Result<bool, CompileError> {
    let trimmed = val.trim();
    if let Some((sig, expr)) = extract_single_hole(trimmed)? {
        paint.push(PaintSlotBinding {
            slot: PaintSlot::FastTransform,
            signal_ident: sig,
            expr,
        });
        return Ok(true);
    }
    let mut parsed = Vec::new();
    let mut cursor = trimmed;
    while !cursor.is_empty() {
        let Some(open) = cursor.find('(') else {
            return Ok(false);
        };
        let fn_name = cursor[..open].trim().to_ascii_lowercase();
        let Some(close) = find_matching_paren(cursor, open) else {
            return Ok(false);
        };
        let arg = cursor[open + 1..close].trim();
        if !push_transform_fn_slots(&fn_name, arg, &mut parsed)? {
            return Ok(false);
        }
        cursor = cursor[close + 1..].trim();
    }
    if parsed.is_empty() {
        return Ok(false);
    }
    paint.extend(parsed);
    Ok(true)
}

fn push_transform_fn_slots(
    fn_name: &str,
    arg: &str,
    out: &mut Vec<PaintSlotBinding>,
) -> Result<bool, CompileError> {
    match fn_name {
        "translatex" => Ok(push_single(
            extract_px_hole(arg)?,
            PaintSlot::TranslateX,
            out,
        )),
        "translatey" => Ok(push_single(
            extract_px_hole(arg)?,
            PaintSlot::TranslateY,
            out,
        )),
        "scalex" => Ok(push_single(
            extract_single_hole(arg)?,
            PaintSlot::ScaleX,
            out,
        )),
        "scaley" => Ok(push_single(
            extract_single_hole(arg)?,
            PaintSlot::ScaleY,
            out,
        )),
        "scale" => Ok(push_single(
            extract_single_hole(arg)?,
            PaintSlot::ScaleUniform,
            out,
        )),
        "rotate" => {
            let no_deg = arg.strip_suffix("deg").unwrap_or(arg).trim();
            Ok(push_single(
                extract_single_hole(no_deg)?,
                PaintSlot::RotateDeg,
                out,
            ))
        }
        "translate" => {
            let parts: Vec<&str> = arg.split(',').map(str::trim).collect();
            if parts.len() != 2 {
                return Ok(false);
            }
            Ok(
                push_single(extract_px_hole(parts[0])?, PaintSlot::TranslateX, out)
                    && push_single(extract_px_hole(parts[1])?, PaintSlot::TranslateY, out),
            )
        }
        _ => Ok(false),
    }
}

fn push_single(
    opt: Option<(String, TemplateExpr)>,
    slot: PaintSlot,
    out: &mut Vec<PaintSlotBinding>,
) -> bool {
    let Some((sig, expr)) = opt else {
        return false;
    };
    out.push(PaintSlotBinding {
        slot,
        signal_ident: sig,
        expr,
    });
    true
}

fn build_slot_cliff_warning(tag: HtmlTag, prop: &str, sig: &str) -> CompileWarning {
    let (msg, fix) = match prop {
        "left" => (
            format!(
                "Layout Performance Cliff on <{}>: dynamic `left` triggers relayout",
                tag.as_str()
            ),
            format!("transform: translateX({{{sig}}}px)"),
        ),
        "top" => (
            format!(
                "Layout Performance Cliff on <{}>: dynamic `top` triggers relayout",
                tag.as_str()
            ),
            format!("transform: translateY({{{sig}}}px)"),
        ),
        _ => (
            format!(
                "Layout Performance Cliff on <{}>: dynamic `{prop}` triggers relayout",
                tag.as_str()
            ),
            "Use transform/opacity compositor slots where layout geometry need not change".into(),
        ),
    };
    CompileWarning {
        kind: CompileWarningKind::LayoutPerformanceCliff,
        suppressed: false,
        node_tag: tag.as_str().to_string(),
        property: prop.to_string(),
        signal_expr: sig.to_string(),
        message: msg,
        suggested_fix: fix,
    }
}

fn build_layout_cliff_warning(tag: HtmlTag, prop: &str, raw_val: &str) -> CompileWarning {
    CompileWarning {
        kind: CompileWarningKind::LayoutPerformanceCliff,
        suppressed: false,
        node_tag: tag.as_str().to_string(),
        property: prop.to_string(),
        signal_expr: raw_val.to_string(),
        message: format!(
            "Layout Performance Cliff on <{}>: dynamic `{prop}` triggers relayout",
            tag.as_str()
        ),
        suggested_fix:
            "Prefer compositor properties (`transform`, `opacity`) when animation geometry permits"
                .into(),
    }
}

fn is_frame_driven_signal(raw_val: &str, signal_expr: &str) -> bool {
    let lower_raw = raw_val.to_ascii_lowercase();
    if lower_raw.contains("{frame:") || signal_ident_looks_frame_driven(signal_expr) {
        return true;
    }
    let mut rest = lower_raw.as_str();
    while let Some((open, after)) = rest.split_once('{') {
        let _ = open;
        let Some((inner, next)) = after.split_once('}') else {
            break;
        };
        if signal_ident_looks_frame_driven(inner) {
            return true;
        }
        rest = next;
    }
    false
}

fn signal_ident_looks_frame_driven(expr: &str) -> bool {
    let lower = expr.trim().to_ascii_lowercase();
    let tail = lower.rsplit('.').next().unwrap_or(&lower);
    let matches_pattern = |s: &str| {
        [
            "frame:", "frame_", "anim_", "spring_", "shake_", "nudge_", "tick_",
        ]
        .iter()
        .any(|p| s.starts_with(p))
            || ["_frame", "_anim", "_spring"]
                .iter()
                .any(|suf| s.ends_with(suf))
    };
    matches_pattern(&lower) || matches_pattern(tail)
}

fn is_layout_cliff_prop(prop: &str) -> bool {
    const PROPS: &str = "width height min-width max-width min-height max-height \
        top left right bottom margin margin-top margin-right margin-bottom margin-left \
        padding padding-top padding-right padding-bottom padding-left gap row-gap column-gap font-size";
    PROPS.split_ascii_whitespace().any(|p| p == prop)
}

fn find_matching_paren(s: &str, open_idx: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 1usize;
    let mut i = open_idx + 1;
    while i < b.len() {
        match b[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}
