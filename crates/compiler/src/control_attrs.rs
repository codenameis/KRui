//! Control attribute parsing (`required`, `checked`, `selected`, `type`, `min`/`max`/`step`, `rows`)
//! and `CompiledNode` construction.

use crate::element_attrs::{parse_bool_attr, ParsedElementAttrs};
use crate::inline_style_slots::InlineStyleAnalysis;
use crate::interpolation::parse_interpolated_text_strict;
use crate::ir::CompiledSelectChild;
use crate::{
    CompileError, CompiledBoolAttr, CompiledButtonType, CompiledInputType, CompiledNode,
    CompiledNodeKind, CompiledSelectOption, CompiledTextPart, HtmlTag,
};

pub(crate) type ExtractedSelectSpec = (Vec<CompiledSelectOption>, Vec<CompiledSelectChild>);

pub(crate) fn apply_control_or_state_attr(
    source: &str,
    tag: HtmlTag,
    k: &str,
    v: &str,
    had_equals: bool,
    attr_offset: usize,
    out: &mut ParsedElementAttrs,
) -> Result<bool, CompileError> {
    match k {
        "disabled" => {
            out.disabled_attr = Some(parse_bool_attr(
                source,
                attr_offset,
                "disabled",
                had_equals,
                v,
            )?);
        }
        "required" => apply_required_attr(source, tag, v, had_equals, attr_offset, out)?,
        "checked" => {
            out.checked_attr = Some(parse_bool_attr(
                source,
                attr_offset,
                "checked",
                had_equals,
                v,
            )?);
        }
        "selected" => apply_selected_attr(source, tag, v, had_equals, attr_offset, out)?,
        "for" => out.label_for = Some(v.trim().to_string()),
        "name" => out.control_name = Some(v.trim().to_string()),
        "min" | "max" | "step" => apply_range_num_attr(source, k, v, attr_offset, out)?,
        "rows" => apply_rows_attr(source, tag, v, attr_offset, out)?,
        _ => return Ok(false),
    }
    record_static_attr_for_selector(k, v, out);
    Ok(true)
}

fn apply_required_attr(
    source: &str,
    tag: HtmlTag,
    v: &str,
    had_equals: bool,
    attr_offset: usize,
    out: &mut ParsedElementAttrs,
) -> Result<(), CompileError> {
    if !matches!(tag, HtmlTag::Input | HtmlTag::Textarea | HtmlTag::Select) {
        return Err(CompileError::at_offset(
            source,
            attr_offset,
            format!(
                "`required` attribute is only supported on `<input>`, `<textarea>`, or `<select>`, not `<{}>`",
                tag.as_str()
            ),
        ));
    }
    out.required_attr = Some(parse_bool_attr(
        source,
        attr_offset,
        "required",
        had_equals,
        v,
    )?);
    Ok(())
}

fn apply_selected_attr(
    source: &str,
    tag: HtmlTag,
    v: &str,
    had_equals: bool,
    attr_offset: usize,
    out: &mut ParsedElementAttrs,
) -> Result<(), CompileError> {
    if tag != HtmlTag::Option {
        return Err(CompileError::at_offset(
            source,
            attr_offset,
            format!(
                "`selected` attribute is only supported on `<option>`, not `<{}>`",
                tag.as_str()
            ),
        ));
    }
    match parse_bool_attr(source, attr_offset, "selected", had_equals, v)? {
        CompiledBoolAttr::Static(b) => out.selected_attr = Some(b),
        CompiledBoolAttr::Dynamic { .. } => {
            return Err(CompileError::at_offset(
                source,
                attr_offset,
                "`selected` on `<option>` must be a static boolean; use `bind:value` or `value` on `<select>` for dynamic selection",
            ));
        }
    }
    Ok(())
}

pub(crate) fn apply_input_type_attr(
    source: &str,
    tag: HtmlTag,
    v: &str,
    attr_offset: usize,
    out: &mut ParsedElementAttrs,
) -> Result<(), CompileError> {
    if tag == HtmlTag::Button {
        out.button_type = match v.trim().to_ascii_lowercase().as_str() {
            "button" => CompiledButtonType::Button,
            "submit" => CompiledButtonType::Submit,
            "reset" => CompiledButtonType::Reset,
            other => {
                return Err(CompileError::at_offset(
                    source,
                    attr_offset,
                    format!(
                        "Unsupported `<button type=\"{other}\">`; expected `button`, `submit`, or `reset`"
                    ),
                ));
            }
        };
        return Ok(());
    }
    if tag != HtmlTag::Input {
        return Err(CompileError::at_offset(
            source,
            attr_offset,
            format!(
                "`type` attribute is only supported on `<input>` and `<button>`, not `<{}>`",
                tag.as_str()
            ),
        ));
    }
    let (kind, secret) = match v.trim().to_ascii_lowercase().as_str() {
        "text" => (CompiledInputType::Text, false),
        "password" => (CompiledInputType::Password, true),
        "number" => (CompiledInputType::Number, false),
        "email" => (CompiledInputType::Email, false),
        "search" => (CompiledInputType::Search, false),
        "checkbox" => (CompiledInputType::Checkbox, false),
        "radio" => (CompiledInputType::Radio, false),
        "range" => (CompiledInputType::Range, false),
        other => {
            return Err(CompileError::at_offset(
                source,
                attr_offset,
                format!(
                    "Unsupported `<input type=\"{other}\">`; expected `text`, `password`, `number`, `email`, `search`, `checkbox`, `radio`, or `range`"
                ),
            ));
        }
    };
    out.input_type = kind;
    out.is_secret_input = secret;
    Ok(())
}

fn apply_range_num_attr(
    source: &str,
    k: &str,
    v: &str,
    attr_offset: usize,
    out: &mut ParsedElementAttrs,
) -> Result<(), CompileError> {
    let val = v
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| {
            CompileError::at_offset(
                source,
                attr_offset,
                format!("Invalid numeric attribute `{k}=\"{v}\"`; expected a finite number"),
            )
        })?;
    match k {
        "min" => out.range_min = Some(val),
        "max" => out.range_max = Some(val),
        "step" => out.range_step = Some(val),
        _ => {}
    }
    Ok(())
}

fn apply_rows_attr(
    source: &str,
    tag: HtmlTag,
    v: &str,
    attr_offset: usize,
    out: &mut ParsedElementAttrs,
) -> Result<(), CompileError> {
    if tag != HtmlTag::Textarea {
        return Err(CompileError::at_offset(
            source,
            attr_offset,
            format!(
                "`rows` attribute is only supported on `<textarea>`, not `<{}>`",
                tag.as_str()
            ),
        ));
    }
    let rows = v
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|&r| r >= 1)
        .ok_or_else(|| {
            CompileError::at_offset(
                source,
                attr_offset,
                format!("Invalid `rows=\"{v}\"` on `<textarea>`; expected a positive integer"),
            )
        })?;
    out.html_layout_attrs.push((
        "min-height".to_string(),
        format!("{}px", rows * 22 + 16),
        attr_offset,
    ));
    Ok(())
}

/// Accepts standard HTML5/MICL attributes (`role`, `open`, `popover*`, `tabindex`, `autofocus`,
/// `hidden`, `readonly`, `data-*`, `aria-*`) so external semantic templates compile cleanly.
pub(crate) fn apply_extra_html5_attr(
    source: &str,
    attr_offset: usize,
    k: &str,
    v: &str,
    had_equals: bool,
    out: &mut ParsedElementAttrs,
) -> Result<bool, CompileError> {
    let lower = k.to_ascii_lowercase();
    let is_html5 = lower.starts_with("data-")
        || lower.starts_with("aria-")
        || matches!(
            lower.as_str(),
            "role"
                | "open"
                | "modal"
                | "popover"
                | "popovertarget"
                | "popovertargetaction"
                | "tabindex"
                | "autofocus"
                | "hidden"
                | "readonly"
                | "indeterminate"
                | "multiple"
                | "novalidate"
                | "inert"
        );
    if !is_html5 {
        return Ok(false);
    }
    let stored = if !had_equals {
        String::new()
    } else {
        v.to_string()
    };
    if matches!(
        lower.as_str(),
        "open" | "modal" | "readonly" | "indeterminate"
    ) && had_equals
        && stored.trim().eq_ignore_ascii_case("false")
    {
        return Ok(true);
    }
    if stored.contains('{') {
        let parts = parse_interpolated_text_strict(source, attr_offset, &stored)?;
        out.dynamic_attrs
            .push(crate::ir::CompiledAttrBinding { name: lower, parts });
    } else {
        out.static_attrs.push((lower, stored));
    }
    Ok(true)
}

/// Records static standard HTML attributes into `out.static_attrs` for `[attr]` CSS selector matching.
pub(crate) fn record_static_attr_for_selector(k: &str, v: &str, out: &mut ParsedElementAttrs) {
    let lower = k.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "id" | "type" | "for" | "name" | "placeholder" | "min" | "max" | "step" | "rows" | "value"
    ) && !v.contains('{')
        && !out.static_attrs.iter().any(|(ak, _)| ak == &lower)
    {
        out.static_attrs.push((lower, v.to_string()));
    }
}

pub(crate) fn parse_element_text_and_placeholder(
    source: &str,
    attrs: &ParsedElementAttrs,
    text_content: String,
    text_offset: usize,
) -> Result<(Vec<CompiledTextPart>, Vec<CompiledTextPart>), CompileError> {
    let (effective_text, text_err_offset) = if !text_content.is_empty() {
        (text_content, text_offset)
    } else if let Some((ref val_str, val_off)) = attrs.value_attr {
        (val_str.clone(), val_off)
    } else {
        (String::new(), text_offset)
    };
    let text_parts = parse_interpolated_text_strict(source, text_err_offset, &effective_text)?;
    let placeholder_parts = match &attrs.placeholder_attr {
        Some((ph_str, ph_off)) => parse_interpolated_text_strict(source, *ph_off, ph_str)?,
        None => Vec::new(),
    };
    Ok((text_parts, placeholder_parts))
}

pub(crate) fn build_compiled_element_node(
    tag: HtmlTag,
    mut attrs: ParsedElementAttrs,
    style_analysis: InlineStyleAnalysis,
    (text_parts, placeholder_parts): (Vec<CompiledTextPart>, Vec<CompiledTextPart>),
    (select_options, select_children): ExtractedSelectSpec,
    children: Vec<CompiledNode>,
) -> CompiledNode {
    let mut node = blank_compiled_element(tag, children);
    node.id = attrs.id.take();
    node.classes = std::mem::take(&mut attrs.classes);
    node.class_parts = std::mem::take(&mut attrs.class_parts);
    node.static_inline_style = style_analysis.static_inline_style;
    node.dynamic_inline_style_parts = style_analysis.dynamic_inline_style_parts;
    node.paint_bindings = style_analysis.paint_bindings;
    node.layout_bindings = style_analysis.layout_bindings;
    node.text_parts = text_parts;
    node.placeholder_parts = placeholder_parts;
    node.title_parts = std::mem::take(&mut attrs.title_parts);
    node.img_src = std::mem::take(&mut attrs.media.img_src);
    node.img_alt = std::mem::take(&mut attrs.media.img_alt);
    node.link_href = std::mem::take(&mut attrs.media.link_href);
    node.label_for = attrs.label_for.take();
    node.control_name = attrs.control_name.take();
    node.range_bounds = attrs.range_bounds;
    node.select_options = select_options;
    node.select_children = select_children;
    node.aria_label = attrs.aria_label.take();
    node.aria_label_parts = std::mem::take(&mut attrs.aria_label_parts);
    attach_element_events_and_attrs(&mut node, attrs);
    node
}

fn blank_compiled_element(tag: HtmlTag, children: Vec<CompiledNode>) -> CompiledNode {
    let mut node = CompiledNode::from_kind(CompiledNodeKind::Element);
    node.tag = tag;
    node.button_type = CompiledButtonType::Button;
    node.children = children;
    node
}

fn attach_element_events_and_attrs(node: &mut CompiledNode, attrs: ParsedElementAttrs) {
    node.input_type = attrs.input_type;
    node.button_type = attrs.button_type;
    node.is_secret_input = attrs.is_secret_input;
    node.on_click = attrs.on_click;
    node.on_focus = attrs.on_focus;
    node.on_blur = attrs.on_blur;
    node.on_pointer_down = attrs.on_pointer_down;
    node.on_pointer_up = attrs.on_pointer_up;
    node.on_pointer_move = attrs.on_pointer_move;
    node.on_scroll = attrs.on_scroll;
    node.on_input = attrs.on_input;
    node.on_change = attrs.on_change;
    node.on_keydown = attrs.on_keydown;
    node.on_submit = attrs.on_submit;
    node.on_reset = attrs.on_reset;
    node.bind_value = attrs.bind_value;
    node.canvas_draw = attrs.canvas_draw;
    node.disabled = attrs.disabled_attr;
    node.required = attrs.required_attr;
    node.checked = attrs.checked_attr;
    node.bind_checked = attrs.bind_checked;
    node.static_attrs = attrs.static_attrs;
    node.dynamic_attrs = attrs.dynamic_attrs;
}
