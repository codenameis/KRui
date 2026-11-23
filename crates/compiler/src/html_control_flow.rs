//! `<if>`/`<else-if>`/`<else>` and `<for>`/`<empty>` control-flow parsers for `.krui` markup.

use crate::html::{
    consume_closing_tag, err_at, is_if_separator_at, parse_html_nodes, parse_tag_open,
    skip_whitespace_and_comments, RawAttr, RawHtmlNode, RawIfBranch, RawTemplateDef,
};
use crate::html_lexer::{collapse_ascii_ws, scan_template_expr_hole, scan_text_until_tag};
use crate::interpolation::{is_valid_rust_ident, require_braced_binding, validate_binding_expr};
use crate::{CompileError, HtmlTag};

fn extract_test_cond(
    input: &str,
    tag: &str,
    off: usize,
    attrs: &[RawAttr],
) -> Result<(String, usize), CompileError> {
    if attrs.len() != 1 || attrs[0].0 != "test" {
        return Err(err_at(
            input,
            off,
            format!("`<{tag}>` requires `test=\"{{...}}\"`"),
        ));
    }
    let expr = require_braced_binding(input, attrs[0].3, "test", &attrs[0].1)?;
    Ok((expr.to_string(), attrs[0].3))
}

pub(crate) fn parse_if_construct(
    input: &str,
    cursor: &mut usize,
    attrs: Vec<RawAttr>,
    sc: bool,
    tag_offset: usize,
    in_text_flow: bool,
) -> Result<RawHtmlNode, CompileError> {
    let first_cond = extract_test_cond(input, "if", tag_offset, &attrs)?;
    if sc {
        return Ok(RawHtmlNode::If {
            tag_offset,
            branches: vec![RawIfBranch {
                condition: Some(first_cond),
                children: Vec::new(),
            }],
        });
    }
    let (mut branches, mut current_cond, mut seen_else) = (Vec::new(), Some(first_cond), false);
    let ptag = Some(("if", tag_offset));
    loop {
        let children = parse_html_nodes(input, cursor, ptag, true, in_text_flow)?;
        branches.push(RawIfBranch {
            condition: current_cond.take(),
            children,
        });
        skip_whitespace_and_comments(input, cursor)?;
        if !is_if_separator_at(input, *cursor) {
            break;
        }
        current_cond = parse_next_if_separator(input, cursor, &mut seen_else)?;
    }
    consume_closing_tag(input, cursor, "if", false)?;
    Ok(RawHtmlNode::If {
        tag_offset,
        branches,
    })
}

fn parse_next_if_separator(
    input: &str,
    cursor: &mut usize,
    seen_else: &mut bool,
) -> Result<Option<(String, usize)>, CompileError> {
    let (sep_raw, sep_attrs, _, sep_off) = parse_tag_open(input, cursor)?;
    let sep_name = sep_raw.to_ascii_lowercase();
    if *seen_else {
        return Err(err_at(
            input,
            sep_off,
            format!("`<{sep_name}>` cannot appear after `<else>`"),
        ));
    }
    if sep_name == "else-if" {
        Ok(Some(extract_test_cond(
            input, "else-if", sep_off, &sep_attrs,
        )?))
    } else if !sep_attrs.is_empty() {
        Err(err_at(
            input,
            sep_off,
            "`<else>` does not accept attributes",
        ))
    } else {
        *seen_else = true;
        Ok(None)
    }
}

pub(crate) fn parse_for_construct(
    input: &str,
    cursor: &mut usize,
    attrs: Vec<RawAttr>,
    sc: bool,
    tag_offset: usize,
    in_text_flow: bool,
) -> Result<RawHtmlNode, CompileError> {
    let (item_var, list_expr, key_expr) = extract_for_header_attrs(input, tag_offset, attrs)?;
    let body = if sc {
        Vec::new()
    } else {
        let ch = parse_html_nodes(
            input,
            cursor,
            Some(("for", tag_offset)),
            false,
            in_text_flow,
        )?;
        consume_closing_tag(input, cursor, "for", false)?;
        ch
    };
    let empty_body = parse_optional_empty_block(input, cursor, in_text_flow)?;
    Ok(RawHtmlNode::For {
        tag_offset,
        item_var,
        list_expr,
        key_expr,
        body,
        empty_body,
    })
}

fn extract_for_header_attrs(
    input: &str,
    tag_offset: usize,
    attrs: Vec<RawAttr>,
) -> Result<(String, String, String), CompileError> {
    let (mut each_attr, mut key_attr) = (None, None);
    for (k, v, _, off) in attrs {
        match k.as_str() {
            "each" => each_attr = Some((v, off)),
            "key" => key_attr = Some((v, off)),
            o => return Err(err_at(input, off, format!("Unknown `<for>` attr `{o}`"))),
        }
    }
    let Some((each_raw, each_off)) = each_attr else {
        return Err(err_at(
            input,
            tag_offset,
            "`<for>` requires `each=\"{item in list}\"`",
        ));
    };
    let Some((key_raw, key_off)) = key_attr else {
        return Err(err_at(
            input,
            tag_offset,
            "`<for>` requires `key=\"{item.id}\"` attribute",
        ));
    };
    let trimmed_each = each_raw.trim();
    if !trimmed_each.starts_with('{') || !trimmed_each.ends_with('}') {
        return Err(err_at(
            input,
            each_off,
            "Attribute `each` on `<for>` requires `{item in list}` braces",
        ));
    }
    let close =
        scan_template_expr_hole(trimmed_each, 0).map_err(|msg| err_at(input, each_off, msg))?;
    if close + 1 != trimmed_each.len() {
        return Err(err_at(input, each_off, "Invalid `<for each>` syntax"));
    }
    let each_inner = &trimmed_each[1..close];
    let Some((item_raw, list_raw)) = each_inner.split_once(" in ") else {
        return Err(err_at(input, each_off, "Invalid `<for each>` syntax"));
    };
    let (item_var, list_expr) = (item_raw.trim().to_string(), list_raw.trim().to_string());
    if !is_valid_rust_ident(&item_var) || list_expr.is_empty() {
        return Err(err_at(
            input,
            each_off,
            format!("Invalid `<for each>` item variable `{item_var}`; expected a Rust identifier"),
        ));
    }
    validate_binding_expr(input, each_off, &list_expr)?;
    let key_expr = require_braced_binding(input, key_off, "key", &key_raw)?.to_string();
    Ok((item_var, list_expr, key_expr))
}

fn parse_optional_empty_block(
    input: &str,
    cursor: &mut usize,
    in_text_flow: bool,
) -> Result<Vec<RawHtmlNode>, CompileError> {
    let saved = *cursor;
    skip_whitespace_and_comments(input, cursor)?;
    let rest = &input[*cursor..];
    if rest.len() >= 6
        && rest.as_bytes()[..6].eq_ignore_ascii_case(b"<empty")
        && rest[6..].starts_with(|c: char| c == '>' || c == '/' || c.is_whitespace())
    {
        let (_, e_attrs, e_sc, e_off) = parse_tag_open(input, cursor)?;
        if !e_attrs.is_empty() {
            return Err(err_at(input, e_off, "`<empty>` does not accept attributes"));
        }
        if !e_sc {
            let empty_body =
                parse_html_nodes(input, cursor, Some(("empty", e_off)), false, in_text_flow)?;
            consume_closing_tag(input, cursor, "empty", false)?;
            return Ok(empty_body);
        }
        return Ok(Vec::new());
    }
    *cursor = saved;
    Ok(Vec::new())
}

pub(crate) fn is_template_open_at(input: &str, pos: usize) -> bool {
    let rest = &input[pos..];
    rest.len() >= 9
        && rest.as_bytes()[..9].eq_ignore_ascii_case(b"<template")
        && rest[9..]
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace() || c == '>' || c == '/')
}

pub(crate) fn is_valid_component_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && is_valid_rust_ident(name)
}

pub(crate) fn is_valid_slot_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub(crate) fn parse_template_definition(
    input: &str,
    cursor: &mut usize,
) -> Result<RawTemplateDef, CompileError> {
    let (_, attrs, sc, tag_offset) = parse_tag_open(input, cursor)?;
    let (comp_name, default_props) = extract_template_header_attrs(input, tag_offset, attrs)?;
    let body = if sc {
        Vec::new()
    } else {
        let ch = parse_html_nodes(input, cursor, Some(("template", tag_offset)), false, false)?;
        consume_closing_tag(input, cursor, "template", false)?;
        ch
    };
    Ok(RawTemplateDef {
        name: comp_name,
        tag_offset,
        default_props,
        body,
    })
}

fn extract_template_header_attrs(
    input: &str,
    tag_offset: usize,
    attrs: Vec<RawAttr>,
) -> Result<(String, Vec<RawAttr>), CompileError> {
    let mut name_attr: Option<(String, usize)> = None;
    let mut default_props: Vec<RawAttr> = Vec::with_capacity(attrs.len());
    for (k, v, had_eq, attr_off) in attrs {
        if k == "name" {
            if name_attr.is_some() {
                return Err(err_at(
                    input,
                    attr_off,
                    "Duplicate `name` attribute on `<template>`",
                ));
            }
            name_attr = Some((v.trim().to_string(), attr_off));
            continue;
        }
        if !is_valid_rust_ident(&k) {
            return Err(err_at(
                input,
                attr_off,
                format!("Invalid prop `{k}` in `<template>` (expected a Rust identifier)"),
            ));
        }
        if default_props.iter().any(|(prev, _, _, _)| prev == &k) {
            return Err(err_at(
                input,
                attr_off,
                format!("Duplicate default prop `{k}` on `<template>`"),
            ));
        }
        default_props.push((k, v, had_eq, attr_off));
    }
    let Some((comp_name, name_off)) = name_attr else {
        return Err(err_at(
            input,
            tag_offset,
            "`<template>` requires a `name=\"ComponentName\"` attribute",
        ));
    };
    if !is_valid_component_name(&comp_name) {
        return Err(err_at(
            input,
            name_off,
            format!(
                "Component `<template name=\"{comp_name}\">` must start with an uppercase ASCII letter"
            ),
        ));
    }
    Ok((comp_name, default_props))
}

pub(crate) fn parse_component_node(
    input: &str,
    cursor: &mut usize,
    name: String,
    attributes: Vec<RawAttr>,
    sc: bool,
    tag_offset: usize,
    in_text_flow: bool,
) -> Result<RawHtmlNode, CompileError> {
    if !is_valid_component_name(&name) {
        return Err(err_at(
            input,
            tag_offset,
            format!("Invalid component tag `<{name}>`"),
        ));
    }
    let children = if sc {
        Vec::new()
    } else {
        let ch = parse_html_nodes(
            input,
            cursor,
            Some((&name, tag_offset)),
            false,
            in_text_flow,
        )?;
        consume_closing_tag(input, cursor, &name, true)?;
        ch
    };
    Ok(RawHtmlNode::Component {
        name,
        tag_offset,
        attributes,
        children,
    })
}

pub(crate) fn parse_slot_node(
    input: &str,
    cursor: &mut usize,
    attrs: Vec<RawAttr>,
    sc: bool,
    tag_offset: usize,
) -> Result<RawHtmlNode, CompileError> {
    let name = match attrs.as_slice() {
        [] => None,
        [(k, v, true, attr_off)] if k == "name" => {
            let trimmed = v.trim();
            if !is_valid_slot_name(trimmed) {
                return Err(err_at(
                    input,
                    *attr_off,
                    format!("Invalid `<slot name=\"{trimmed}\">`; expected non-empty ASCII alphanumeric/`_`/`-` identifier"),
                ));
            }
            Some(trimmed.to_string())
        }
        [(_, _, _, attr_off), ..] => {
            return Err(err_at(
                input,
                *attr_off,
                "`<slot>` only supports an optional `name=\"...\"` attribute",
            ));
        }
    };
    let fallback = if sc {
        Vec::new()
    } else {
        let ch = parse_html_nodes(input, cursor, Some(("slot", tag_offset)), false, false)?;
        consume_closing_tag(input, cursor, "slot", false)?;
        ch
    };
    Ok(RawHtmlNode::Slot {
        name,
        fallback,
        tag_offset,
    })
}

pub(crate) fn parse_element_body_and_close(
    input: &str,
    cursor: &mut usize,
    tag: HtmlTag,
    close_tag_name: &str,
    tag_offset: usize,
) -> Result<(String, usize, Vec<RawHtmlNode>), CompileError> {
    let (content_start, elem_tf, ptag) = (
        *cursor,
        tag.is_text_flow_container(),
        Some((close_tag_name, tag_offset)),
    );
    let (mut text_content, mut text_offset, mut children) =
        (String::new(), content_start, Vec::new());
    skip_whitespace_and_comments(input, cursor)?;
    if *cursor < input.len() && !input[*cursor..].starts_with('<') {
        text_offset = *cursor;
        let next_lt = scan_text_until_tag(input, *cursor)?;
        let mut after_text = next_lt;
        skip_whitespace_and_comments(input, &mut after_text)?;
        if input[after_text..].starts_with("</") {
            let trimmed = input[content_start..next_lt].trim();
            text_content = if matches!(tag, HtmlTag::Textarea | HtmlTag::Pre) {
                trimmed.to_string()
            } else {
                collapse_ascii_ws(trimmed)
            };
            *cursor = after_text;
        } else {
            *cursor = content_start;
            children = parse_html_nodes(input, cursor, ptag, false, elem_tf)?;
        }
    } else if !input[*cursor..].starts_with("</") {
        *cursor = content_start;
        children = parse_html_nodes(input, cursor, ptag, false, elem_tf)?;
    }
    consume_closing_tag(input, cursor, close_tag_name, false)?;
    Ok((text_content, text_offset, children))
}
