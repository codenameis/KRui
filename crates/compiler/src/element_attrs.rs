//! Element attribute parsing (`ParsedElementAttrs`, `parse_element_attributes`) for `.krui` nodes.

use crate::control_attrs::{
    apply_control_or_state_attr, apply_input_type_attr, ExtractedSelectSpec,
};
use crate::expr::{parse_template_expr, TemplateExpr};
use crate::html::{RawAttr, RawHtmlNode};
use crate::interpolation::{parse_interpolated_text_strict, require_braced_template_expr};
use crate::ir::{
    CompiledAttrBinding, CompiledDynamicSelectOption, CompiledSelectChild, CompiledSelectFor,
};
use crate::{
    CompileError, CompiledBoolAttr, CompiledButtonType, CompiledInputType, CompiledSelectOption,
    CompiledTextPart, HtmlTag,
};
use krui_style::UserSelect;

#[derive(Default)]
pub(crate) struct ParsedElementAttrs {
    pub(crate) id: Option<String>,
    pub(crate) classes: Vec<String>,
    pub(crate) class_parts: Vec<CompiledTextPart>,
    pub(crate) inline_style_raw: Option<(String, usize)>,
    pub(crate) on_click: Option<TemplateExpr>,
    pub(crate) on_focus: Option<TemplateExpr>,
    pub(crate) on_blur: Option<TemplateExpr>,
    pub(crate) on_pointer_down: Option<TemplateExpr>,
    pub(crate) on_pointer_up: Option<TemplateExpr>,
    pub(crate) on_pointer_move: Option<TemplateExpr>,
    pub(crate) on_context_menu: Option<TemplateExpr>,
    pub(crate) on_wheel: Option<TemplateExpr>,
    pub(crate) on_scroll: Option<TemplateExpr>,
    pub(crate) on_input: Option<TemplateExpr>,
    pub(crate) on_change: Option<TemplateExpr>,
    pub(crate) on_keydown: Option<TemplateExpr>,
    pub(crate) on_submit: Option<TemplateExpr>,
    pub(crate) on_reset: Option<TemplateExpr>,
    pub(crate) bind_value: Option<TemplateExpr>,
    pub(crate) canvas_draw: Option<TemplateExpr>,
    pub(crate) input_type: CompiledInputType,
    pub(crate) button_type: CompiledButtonType,
    pub(crate) is_secret_input: bool,
    pub(crate) aria_label: Option<String>,
    pub(crate) aria_label_parts: Vec<CompiledTextPart>,
    pub(crate) title_parts: Vec<CompiledTextPart>,
    pub(crate) media: crate::media_attrs::ParsedMediaAttrs,
    pub(crate) value_attr: Option<(String, usize)>,
    pub(crate) placeholder_attr: Option<(String, usize)>,
    pub(crate) disabled_attr: Option<CompiledBoolAttr>,
    pub(crate) required_attr: Option<CompiledBoolAttr>,
    pub(crate) checked_attr: Option<CompiledBoolAttr>,
    pub(crate) selected_attr: Option<bool>,
    pub(crate) bind_checked: Option<TemplateExpr>,
    pub(crate) label_for: Option<String>,
    pub(crate) control_name: Option<String>,
    pub(crate) range_min: Option<f32>,
    pub(crate) range_max: Option<f32>,
    pub(crate) range_step: Option<f32>,
    pub(crate) range_bounds: Option<(f32, f32, f32)>,
    pub(crate) selectable_attr: Option<UserSelect>,
    pub(crate) html_layout_attrs: Vec<(String, String, usize)>,
    pub(crate) static_attrs: Vec<(String, String)>,
    pub(crate) dynamic_attrs: Vec<CompiledAttrBinding>,
}

pub(crate) fn parse_element_attributes(
    source: &str,
    tag: HtmlTag,
    attributes: &[RawAttr],
) -> Result<ParsedElementAttrs, CompileError> {
    let mut out = ParsedElementAttrs::default();
    let mut explicit_value_offset: Option<usize> = None;
    for (k, v, had_equals, attr_offset) in attributes {
        apply_single_element_attr(
            source,
            tag,
            k,
            v,
            *had_equals,
            *attr_offset,
            &mut explicit_value_offset,
            &mut out,
        )?;
    }
    if out.bind_value.is_some() {
        if let Some(val_off) = explicit_value_offset {
            return Err(CompileError::at_offset(
                source,
                val_off,
                "Cannot specify both `bind:value` and `value` on the same `<input>` element",
            ));
        }
    }
    if out.input_type == CompiledInputType::Range
        || out.range_min.is_some()
        || out.range_max.is_some()
        || out.range_step.is_some()
    {
        let min_v = out.range_min.unwrap_or(0.0);
        let max_v = out.range_max.unwrap_or(100.0).max(min_v);
        let step_v = out.range_step.unwrap_or(1.0).max(0.000_001);
        out.range_bounds = Some((min_v, max_v, step_v));
    }
    crate::media_attrs::finalize_media_attrs(source, tag, attributes, &mut out)?;
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn apply_single_element_attr(
    source: &str,
    tag: HtmlTag,
    k: &str,
    v: &str,
    had_equals: bool,
    attr_offset: usize,
    explicit_value_offset: &mut Option<usize>,
    out: &mut ParsedElementAttrs,
) -> Result<(), CompileError> {
    if apply_event_or_binding_attr(source, k, v, attr_offset, out)?
        || apply_control_or_state_attr(source, tag, k, v, had_equals, attr_offset, out)?
        || crate::media_attrs::apply_media_attr(source, tag, k, v, attr_offset, out)?
    {
        return Ok(());
    }
    match k {
        "id" => apply_id_attr(source, v, attr_offset, out)?,
        "class" => apply_class_attr(source, v, attr_offset, out)?,
        "style" => out.inline_style_raw = Some((v.to_string(), attr_offset)),
        "type" => apply_input_type_attr(source, tag, v, attr_offset, out)?,
        "aria-label" => apply_aria_label_attr(source, v, attr_offset, out)?,
        "title" => {
            out.title_parts = parse_interpolated_text_strict(source, attr_offset, v)?;
        }
        "href" if tag == HtmlTag::A => {
            out.media.link_href = parse_interpolated_text_strict(source, attr_offset, v)?;
        }
        "value" => {
            *explicit_value_offset = Some(attr_offset);
            if out.value_attr.is_none() {
                out.value_attr = Some((v.to_string(), attr_offset));
            }
        }
        "placeholder" => out.placeholder_attr = Some((v.to_string(), attr_offset)),
        "selectable" => out.selectable_attr = Some(parse_selectable_attr(source, attr_offset, v)?),
        "dir" => out.html_layout_attrs.push((
            "text-direction".to_string(),
            v.trim().to_ascii_lowercase(),
            attr_offset,
        )),
        "direction" => out.html_layout_attrs.push((
            "flex-direction".to_string(),
            v.trim().to_ascii_lowercase(),
            attr_offset,
        )),
        "width" | "height" | "gap" => apply_html_dim_attr(k, v, attr_offset, out),
        other => {
            if !crate::control_attrs::apply_extra_html5_attr(
                source,
                attr_offset,
                k,
                v,
                had_equals,
                out,
            )? {
                return Err(CompileError::at_offset(
                    source,
                    attr_offset,
                    format!("Unknown HTML attribute `{other}` on `<{}>`", tag.as_str()),
                ));
            }
        }
    }
    crate::control_attrs::record_static_attr_for_selector(k, v, out);
    Ok(())
}

fn apply_id_attr(
    source: &str,
    v: &str,
    attr_offset: usize,
    out: &mut ParsedElementAttrs,
) -> Result<(), CompileError> {
    if v.contains('{') || v.contains('}') {
        return Err(CompileError::at_offset(
            source,
            attr_offset,
            "`id` attribute must be a static string; dynamic `{...}` bindings are not allowed on `id`",
        ));
    }
    out.id = Some(v.to_string());
    Ok(())
}

fn apply_class_attr(
    source: &str,
    v: &str,
    attr_offset: usize,
    out: &mut ParsedElementAttrs,
) -> Result<(), CompileError> {
    let parts = parse_interpolated_text_strict(source, attr_offset, v)?;
    out.classes = collect_static_classes(&parts);
    out.class_parts = parts;
    Ok(())
}

fn collect_static_classes(parts: &[CompiledTextPart]) -> Vec<String> {
    let mut static_classes = Vec::new();
    let mut cur = String::new();
    let mut has_dyn = false;
    for part in parts {
        match part {
            CompiledTextPart::Dynamic { .. } => {
                has_dyn = true;
                cur.clear();
            }
            CompiledTextPart::Static(s) => {
                for ch in s.chars() {
                    if ch.is_ascii_whitespace() {
                        if !has_dyn && !cur.is_empty() {
                            static_classes.push(std::mem::take(&mut cur));
                        }
                        cur.clear();
                        has_dyn = false;
                    } else if !has_dyn {
                        cur.push(ch);
                    }
                }
            }
        }
    }
    if !has_dyn && !cur.is_empty() {
        static_classes.push(cur);
    }
    static_classes
}

fn apply_html_dim_attr(k: &str, v: &str, attr_offset: usize, out: &mut ParsedElementAttrs) {
    let raw_v = v.trim();
    let normalized = if raw_v != "0" && raw_v.parse::<f32>().is_ok() {
        format!("{raw_v}px")
    } else {
        raw_v.to_string()
    };
    out.html_layout_attrs
        .push((k.to_string(), normalized, attr_offset));
}

fn apply_event_or_binding_attr(
    source: &str,
    k: &str,
    v: &str,
    attr_offset: usize,
    out: &mut ParsedElementAttrs,
) -> Result<bool, CompileError> {
    if let Some(slot) = match k {
        "onclick" | "on:click" => Some(&mut out.on_click),
        "onfocus" | "on:focus" => Some(&mut out.on_focus),
        "onblur" | "on:blur" => Some(&mut out.on_blur),
        "onpointerdown" | "on:pointerdown" => Some(&mut out.on_pointer_down),
        "onpointerup" | "on:pointerup" => Some(&mut out.on_pointer_up),
        "onpointermove" | "on:pointermove" => Some(&mut out.on_pointer_move),
        "oncontextmenu" | "on:contextmenu" => Some(&mut out.on_context_menu),
        "onwheel" | "on:wheel" => Some(&mut out.on_wheel),
        "onscroll" | "on:scroll" => Some(&mut out.on_scroll),
        "oninput" | "on:input" => Some(&mut out.on_input),
        "onchange" | "on:change" => Some(&mut out.on_change),
        "onkeydown" | "on:keydown" => Some(&mut out.on_keydown),
        "onsubmit" | "on:submit" => Some(&mut out.on_submit),
        "onreset" | "on:reset" => Some(&mut out.on_reset),
        "ondraw" | "on:draw" | "draw" => Some(&mut out.canvas_draw),
        _ => None,
    } {
        *slot = Some(require_braced_template_expr(source, attr_offset, k, v)?.1);
        return Ok(true);
    }
    match k {
        "bind:value" | "bind-value" => {
            let (sig, ast) = require_braced_template_expr(source, attr_offset, k, v)?;
            out.value_attr = Some((format!("{{{sig}}}"), attr_offset));
            out.bind_value = Some(ast);
        }
        "bind:checked" | "bind-checked" => {
            let (raw, expr) = require_braced_template_expr(source, attr_offset, k, v)?;
            out.checked_attr = Some(CompiledBoolAttr::Dynamic {
                raw,
                expr: expr.clone(),
            });
            out.bind_checked = Some(expr);
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn apply_aria_label_attr(
    source: &str,
    v: &str,
    attr_offset: usize,
    out: &mut ParsedElementAttrs,
) -> Result<(), CompileError> {
    let parts = parse_interpolated_text_strict(source, attr_offset, v)?;
    if parts
        .iter()
        .all(|p| matches!(p, CompiledTextPart::Static(_)))
    {
        let mut combined = String::new();
        for p in &parts {
            if let CompiledTextPart::Static(s) = p {
                combined.push_str(s);
            }
        }
        out.aria_label = Some(combined);
    }
    out.aria_label_parts = parts;
    Ok(())
}

pub(crate) fn parse_bool_attr(
    source: &str,
    attr_offset: usize,
    attr_name: &str,
    had_equals: bool,
    v: &str,
) -> Result<CompiledBoolAttr, CompileError> {
    let trimmed = v.trim();
    if !had_equals
        || trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("true")
        || trimmed.eq_ignore_ascii_case(attr_name)
    {
        Ok(CompiledBoolAttr::Static(true))
    } else if trimmed.eq_ignore_ascii_case("false") {
        Ok(CompiledBoolAttr::Static(false))
    } else if trimmed.starts_with("{") && trimmed.ends_with("}") {
        let (raw, expr) = require_braced_template_expr(source, attr_offset, attr_name, trimmed)?;
        Ok(CompiledBoolAttr::Dynamic { raw, expr })
    } else {
        Err(CompileError::at_offset(
            source,
            attr_offset,
            format!("Invalid `{attr_name}` attribute value `{v}`"),
        ))
    }
}

fn parse_selectable_attr(
    source: &str,
    attr_offset: usize,
    v: &str,
) -> Result<UserSelect, CompileError> {
    match v.trim().to_ascii_lowercase().as_str() {
        "" | "true" | "text" => Ok(UserSelect::Text),
        "all" => Ok(UserSelect::All),
        "false" | "none" => Ok(UserSelect::None),
        other => Err(CompileError::at_offset(
            source,
            attr_offset,
            format!("Invalid `selectable` attribute value `{other}`; expected `true`, `false`, `text`, `all`, or `none`"),
        )),
    }
}

pub(crate) fn extract_select_options(
    source: &str,
    tag: HtmlTag,
    tag_offset: usize,
    raw_children: &mut Vec<RawHtmlNode>,
) -> Result<ExtractedSelectSpec, CompileError> {
    if tag == HtmlTag::Option {
        return Err(CompileError::at_offset(
            source,
            tag_offset,
            "`<option>` is only valid as a direct child of `<select>`",
        ));
    }
    if tag != HtmlTag::Select {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut static_options = Vec::with_capacity(raw_children.len());
    let mut select_children = Vec::with_capacity(raw_children.len());
    let mut has_dynamic = false;
    for ch in raw_children.drain(..) {
        if push_select_child(source, ch, &mut static_options, &mut select_children)? {
            has_dynamic = true;
        }
    }
    if !has_dynamic {
        select_children.clear();
    }
    Ok((static_options, select_children))
}

fn push_select_child(
    source: &str,
    ch: RawHtmlNode,
    static_options: &mut Vec<CompiledSelectOption>,
    select_children: &mut Vec<CompiledSelectChild>,
) -> Result<bool, CompileError> {
    match ch {
        node @ RawHtmlNode::Element {
            tag: HtmlTag::Option,
            ..
        } => {
            let (dyn_opt, static_opt) = parse_select_option_node(source, node)?;
            let is_dyn = match static_opt {
                Some(st) => {
                    static_options.push(st);
                    false
                }
                None => true,
            };
            select_children.push(CompiledSelectChild::Option(dyn_opt));
            Ok(is_dyn)
        }
        RawHtmlNode::For {
            tag_offset: for_off,
            item_var,
            list_expr,
            key_expr,
            body,
            empty_body,
        } => {
            select_children.push(CompiledSelectChild::For(parse_select_for_child(
                source,
                for_off,
                (item_var, list_expr, key_expr),
                body,
                empty_body,
            )?));
            Ok(true)
        }
        other => Err(CompileError::at_offset(
            source,
            raw_node_offset(&other),
            "`<select>` may only contain `<option>` or `<for>` elements",
        )),
    }
}

fn parse_select_for_child(
    source: &str,
    for_off: usize,
    (item_var, list_expr, key_expr): (String, String, String),
    body: Vec<RawHtmlNode>,
    empty_body: Vec<RawHtmlNode>,
) -> Result<CompiledSelectFor, CompileError> {
    if !empty_body.is_empty() {
        return Err(CompileError::at_offset(
            source,
            for_off,
            "`<empty>` is not supported inside `<select><for>`",
        ));
    }
    let list = parse_template_expr(source, for_off, &list_expr)?;
    let key = parse_template_expr(source, for_off, &key_expr)?;
    let mut options = Vec::with_capacity(body.len());
    for bnode in body {
        let (dopt, _) = parse_select_option_node(source, bnode)?;
        options.push(dopt);
    }
    Ok(CompiledSelectFor {
        item_var,
        list_expr,
        list,
        key_expr,
        key,
        options,
    })
}

fn raw_node_offset(node: &RawHtmlNode) -> usize {
    match node {
        RawHtmlNode::Element { tag_offset: o, .. }
        | RawHtmlNode::If { tag_offset: o, .. }
        | RawHtmlNode::For { tag_offset: o, .. }
        | RawHtmlNode::Slot { tag_offset: o, .. }
        | RawHtmlNode::Component { tag_offset: o, .. } => *o,
        RawHtmlNode::TextRun { offset: o, .. } => *o,
    }
}

fn parse_select_option_node(
    source: &str,
    node: RawHtmlNode,
) -> Result<(CompiledDynamicSelectOption, Option<CompiledSelectOption>), CompileError> {
    let RawHtmlNode::Element {
        tag: HtmlTag::Option,
        attributes,
        text_content,
        text_offset,
        children,
        tag_offset: opt_off,
        ..
    } = node
    else {
        return Err(CompileError::at_offset(
            source,
            raw_node_offset(&node),
            "`<for>` inside `<select>` may only contain `<option>` elements",
        ));
    };
    if !children.is_empty() {
        return Err(CompileError::at_offset(
            source,
            opt_off,
            "`<option>` may only contain plain text, not nested child elements",
        ));
    }
    let opt_attrs = parse_element_attributes(source, HtmlTag::Option, &attributes)?;
    let label_parts = parse_interpolated_text_strict(source, text_offset, text_content.trim())?;
    let value_parts = match opt_attrs.value_attr {
        Some((v, v_off)) => parse_interpolated_text_strict(source, v_off, &v)?,
        None => label_parts.clone(),
    };
    let selected = opt_attrs.selected_attr.unwrap_or(false);
    let static_opt = collapse_static_parts(&value_parts)
        .zip(collapse_static_parts(&label_parts))
        .map(|(value, label)| CompiledSelectOption {
            value,
            label,
            selected,
        });
    Ok((
        CompiledDynamicSelectOption {
            value_parts,
            label_parts,
            selected,
        },
        static_opt,
    ))
}

fn collapse_static_parts(parts: &[CompiledTextPart]) -> Option<String> {
    let mut out = String::new();
    for p in parts {
        match p {
            CompiledTextPart::Static(s) => out.push_str(s),
            CompiledTextPart::Dynamic { .. } => return None,
        }
    }
    Some(out)
}
