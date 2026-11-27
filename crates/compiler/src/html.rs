//! Build-time HTML, `<template>` / `<Card>` component, `<slot/>`, and control-flow parser.

use crate::html_control_flow::{
    is_template_open_at, parse_component_node, parse_element_body_and_close, parse_for_construct,
    parse_if_construct, parse_slot_node, parse_template_definition,
};
use crate::html_lexer::{normalize_inline_children, scan_quoted_attr_value, scan_text_until_tag};
use crate::{CompileError, HtmlTag};

/// Raw parsed attribute tuple: `(name, value, had_equals, byte_offset)`.
pub type RawAttr = (String, String, bool, usize);
/// Raw parsed opening tag tuple: `(raw_tag_name, attributes, self_closing, byte_offset)`.
pub type ParsedTagOpen = (String, Vec<RawAttr>, bool, usize);

/// Inline `<template name="Card">...</template>` component definition inside a `.krui` file.
#[derive(Clone, Debug)]
pub struct RawTemplateDef {
    pub name: String,
    pub tag_offset: usize,
    pub end_offset: usize,
    pub default_props: Vec<RawAttr>,
    pub body: Vec<RawHtmlNode>,
}

/// Branch inside an unexpanded `<if>` / `<else-if>` / `<else>` AST node.
#[derive(Clone, Debug)]
pub struct RawIfBranch {
    pub condition: Option<(String, usize)>,
    pub children: Vec<RawHtmlNode>,
}

/// Unexpanded HTML/component/control-flow AST node.
#[derive(Clone, Debug)]
pub enum RawHtmlNode {
    Element {
        tag: HtmlTag,
        custom_tag: Option<String>,
        tag_offset: usize,
        attributes: Vec<RawAttr>,
        text_content: String,
        text_offset: usize,
        children: Vec<RawHtmlNode>,
    },
    TextRun {
        text: String,
        offset: usize,
    },
    Component {
        name: String,
        tag_offset: usize,
        attributes: Vec<RawAttr>,
        children: Vec<RawHtmlNode>,
    },
    Slot {
        name: Option<String>,
        fallback: Vec<RawHtmlNode>,
        tag_offset: usize,
    },
    If {
        tag_offset: usize,
        branches: Vec<RawIfBranch>,
    },
    For {
        tag_offset: usize,
        item_var: String,
        list_expr: String,
        key_expr: String,
        body: Vec<RawHtmlNode>,
        empty_body: Vec<RawHtmlNode>,
    },
}

pub(crate) fn err_at(input: &str, offset: usize, msg: impl Into<String>) -> CompileError {
    CompileError::at_offset(input, offset, msg)
}

/// Parses a `.krui` HTML body into inline `<template name="...">` definitions and root `RawHtmlNode`s.
pub fn parse_html_document(
    input: &str,
) -> Result<(Vec<RawTemplateDef>, Vec<RawHtmlNode>), CompileError> {
    let mut cursor = 0;
    let mut templates = Vec::new();
    let mut raw_roots = Vec::new();

    loop {
        skip_comments_only(input, &mut cursor)?;
        if cursor >= input.len() {
            break;
        }
        if input[cursor..].starts_with("</") {
            let preview: String = input[cursor..].chars().take(24).collect();
            return Err(err_at(
                input,
                cursor,
                format!("Unexpected closing tag `{preview}` at root level"),
            ));
        }
        if is_template_open_at(input, cursor) {
            let tpl = parse_template_definition(input, &mut cursor)?;
            if templates
                .iter()
                .any(|t: &RawTemplateDef| t.name == tpl.name)
            {
                return Err(err_at(
                    input,
                    tpl.tag_offset,
                    format!("Duplicate `<template name=\"{}\">` definition", tpl.name),
                ));
            }
            templates.push(tpl);
        } else if input[cursor..].starts_with('<') {
            raw_roots.push(parse_single_node(input, &mut cursor, false)?);
        } else {
            let text_offset = cursor;
            let next_lt = scan_text_until_tag(input, cursor)?;
            push_or_merge_text_run(&mut raw_roots, &input[cursor..next_lt], text_offset);
            cursor = next_lt;
        }
    }
    Ok((templates, normalize_inline_children(raw_roots, false)))
}

fn skip_ws(input: &str, cursor: &mut usize) {
    while let Some(ch) = input[*cursor..].chars().next() {
        if ch.is_whitespace() {
            *cursor += ch.len_utf8();
        } else {
            break;
        }
    }
}

fn skip_comments_only(input: &str, cursor: &mut usize) -> Result<(), CompileError> {
    while input[*cursor..].starts_with("<!--") {
        let comment_start = *cursor;
        if let Some(end) = input[*cursor + 4..].find("-->") {
            *cursor += 4 + end + 3;
        } else {
            return Err(err_at(input, comment_start, "Unclosed HTML comment `<!--`"));
        }
    }
    Ok(())
}

pub(crate) fn skip_whitespace_and_comments(
    input: &str,
    cursor: &mut usize,
) -> Result<(), CompileError> {
    loop {
        skip_ws(input, cursor);
        if input[*cursor..].starts_with("<!--") {
            skip_comments_only(input, cursor)?;
            continue;
        }
        break;
    }
    Ok(())
}

fn push_or_merge_text_run(nodes: &mut Vec<RawHtmlNode>, slice: &str, offset: usize) {
    if slice.is_empty() {
        return;
    }
    if let Some(RawHtmlNode::TextRun { text, .. }) = nodes.last_mut() {
        text.push_str(slice);
    } else {
        nodes.push(RawHtmlNode::TextRun {
            text: slice.to_string(),
            offset,
        });
    }
}

pub(crate) fn parse_html_nodes(
    input: &str,
    cursor: &mut usize,
    parent_tag: Option<(&str, usize)>,
    stop_on_if_separators: bool,
    in_text_flow: bool,
) -> Result<Vec<RawHtmlNode>, CompileError> {
    let mut nodes = Vec::new();
    loop {
        skip_comments_only(input, cursor)?;
        if *cursor >= input.len() {
            if let Some((parent, open_offset)) = parent_tag {
                return Err(err_at(
                    input,
                    open_offset,
                    format!("Unclosed HTML tag `<{parent}>`"),
                ));
            }
            break;
        }
        if input[*cursor..].starts_with("</") {
            break;
        }
        if stop_on_if_separators && is_if_separator_at(input, *cursor) {
            break;
        }
        if input[*cursor..].starts_with('<') {
            nodes.push(parse_single_node(input, cursor, in_text_flow)?);
        } else {
            let text_offset = *cursor;
            let next_lt = scan_text_until_tag(input, *cursor)?;
            push_or_merge_text_run(&mut nodes, &input[*cursor..next_lt], text_offset);
            *cursor = next_lt;
        }
    }
    Ok(normalize_inline_children(nodes, in_text_flow))
}

pub(crate) fn is_if_separator_at(input: &str, pos: usize) -> bool {
    let rest = &input[pos..];
    if !rest.starts_with('<') || rest.starts_with("</") || rest.starts_with("<!--") {
        return false;
    }
    let after_lt = &rest[1..];
    let mut end = 0;
    for (idx, ch) in after_lt.char_indices() {
        if ch.is_whitespace() || ch == '>' || ch == '/' {
            end = idx;
            break;
        }
        end = idx + ch.len_utf8();
    }
    let name = after_lt[..end].to_ascii_lowercase();
    name == "else" || name == "else-if"
}

pub(crate) fn parse_tag_open(
    input: &str,
    cursor: &mut usize,
) -> Result<ParsedTagOpen, CompileError> {
    let tag_offset = *cursor;
    *cursor += 1;
    let name_start = *cursor;
    *cursor = input[name_start..]
        .find(|ch: char| ch.is_whitespace() || ch == '>' || ch == '/')
        .map_or(input.len(), |rel| name_start + rel);
    let raw_tag = input[name_start..*cursor].to_string();
    if raw_tag.is_empty() {
        return Err(err_at(input, tag_offset, "Empty HTML tag `< >`"));
    }
    let is_component = raw_tag
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase());

    let (mut attributes, mut self_closing) = (Vec::new(), false);
    loop {
        skip_ws(input, cursor);
        if *cursor >= input.len() {
            return Err(err_at(
                input,
                tag_offset,
                format!("Unexpected EOF inside `<{raw_tag}>`"),
            ));
        }
        if input[*cursor..].starts_with("/>") {
            *cursor += 2;
            self_closing = true;
            break;
        }
        if input[*cursor..].starts_with('>') {
            *cursor += 1;
            break;
        }
        let attr_start = *cursor;
        let attr = parse_single_tag_attr(input, cursor, attr_start, is_component)?;
        if attributes.iter().any(|(ex, _, _, _)| ex == &attr.0) {
            return Err(err_at(
                input,
                attr_start,
                format!("Duplicate HTML attribute `{}` on `<{raw_tag}>`", attr.0),
            ));
        }
        attributes.push(attr);
    }
    Ok((raw_tag, attributes, self_closing, tag_offset))
}

fn parse_single_tag_attr(
    input: &str,
    cursor: &mut usize,
    attr_start: usize,
    preserve_attr_case: bool,
) -> Result<RawAttr, CompileError> {
    for (rel_idx, ch) in input[attr_start..].char_indices() {
        let abs_idx = attr_start + rel_idx;
        if ch.is_whitespace() || ch == '=' || ch == '>' || input[abs_idx..].starts_with("/>") {
            *cursor = abs_idx;
            break;
        }
        *cursor = abs_idx + ch.len_utf8();
    }
    let attr_raw = &input[attr_start..*cursor];
    if attr_raw.is_empty() {
        return Err(err_at(input, attr_start, "Invalid attribute syntax"));
    }
    let attr_name = if preserve_attr_case {
        attr_raw.to_string()
    } else {
        attr_raw.to_ascii_lowercase()
    };
    skip_ws(input, cursor);

    if !input[*cursor..].starts_with('=') {
        if preserve_attr_case
            || matches!(
                attr_name.as_str(),
                "disabled"
                    | "selectable"
                    | "checked"
                    | "required"
                    | "selected"
                    | "open"
                    | "modal"
                    | "readonly"
                    | "indeterminate"
                    | "hidden"
                    | "popover"
                    | "autofocus"
                    | "multiple"
                    | "novalidate"
                    | "inert"
            )
            || attr_name.starts_with("data-")
        {
            return Ok((attr_name, "true".to_string(), false, attr_start));
        }
        return Err(err_at(
            input,
            attr_start,
            format!("Attribute `{attr_raw}` must have a quoted value"),
        ));
    }
    *cursor += 1;
    skip_ws(input, cursor);
    let val_offset = *cursor;
    let Some(quote_ch @ ('"' | '\'')) = input[*cursor..].chars().next() else {
        return Err(err_at(
            input,
            val_offset,
            format!("Unquoted attribute value for `{attr_raw}`; must use quotes"),
        ));
    };
    let (attr_val, next_pos) = scan_quoted_attr_value(input, val_offset, quote_ch)?;
    *cursor = next_pos;
    Ok((attr_name, attr_val, true, attr_start))
}

pub(crate) fn consume_closing_tag(
    input: &str,
    cursor: &mut usize,
    exp: &str,
    case_sensitive: bool,
) -> Result<(), CompileError> {
    if !input[*cursor..].starts_with("</") {
        return Err(err_at(input, *cursor, format!("Expected `</{exp}>`")));
    }
    let off = *cursor;
    *cursor += 2;
    let start = *cursor;
    let Some(rel_gt) = input[start..].find('>') else {
        return Err(err_at(input, off, format!("Unclosed `</{exp}`")));
    };
    *cursor = start + rel_gt;
    let raw_close = input[start..*cursor].trim();
    let matched = if case_sensitive {
        raw_close == exp
    } else {
        raw_close.eq_ignore_ascii_case(exp)
    };
    if !matched {
        return Err(err_at(
            input,
            off,
            format!("Mismatched `</{raw_close}>` for `<{exp}>`"),
        ));
    }
    *cursor += 1;
    Ok(())
}

fn parse_single_node(
    input: &str,
    cursor: &mut usize,
    in_text_flow: bool,
) -> Result<RawHtmlNode, CompileError> {
    let (raw_tag, attrs, sc, off) = parse_tag_open(input, cursor)?;
    if raw_tag
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
    {
        return parse_component_node(input, cursor, raw_tag, attrs, sc, off, in_text_flow);
    }
    let lower = raw_tag.to_ascii_lowercase();
    match lower.as_str() {
        "if" => parse_if_construct(input, cursor, attrs, sc, off, in_text_flow),
        "else" | "else-if" => Err(err_at(
            input,
            off,
            format!("`<{lower}>` is only valid inside `<if>`"),
        )),
        "for" => parse_for_construct(input, cursor, attrs, sc, off, in_text_flow),
        "empty" => Err(err_at(
            input,
            off,
            "`<empty>` is only valid immediately after `<for>`",
        )),
        "slot" => parse_slot_node(input, cursor, attrs, sc, off),
        "template" => Err(err_at(
            input,
            off,
            "`<template>` component definitions are only valid at document root level",
        )),
        _ => parse_standard_element(input, cursor, &lower, attrs, sc, off),
    }
}

fn parse_standard_element(
    input: &str,
    cursor: &mut usize,
    tag_name: &str,
    attributes: Vec<RawAttr>,
    self_closing: bool,
    tag_offset: usize,
) -> Result<RawHtmlNode, CompileError> {
    let (tag, is_custom_fallback) =
        HtmlTag::parse_element_tag(tag_name).map_err(|e| err_at(input, tag_offset, e))?;
    let custom_tag = is_custom_fallback.then(|| tag_name.to_string());
    if self_closing
        || matches!(
            tag,
            HtmlTag::Input
                | HtmlTag::Img
                | HtmlTag::Hr
                | HtmlTag::Br
                | HtmlTag::Col
                | HtmlTag::Source
                | HtmlTag::Wbr
        )
    {
        return Ok(RawHtmlNode::Element {
            tag,
            custom_tag,
            tag_offset,
            attributes,
            text_content: String::new(),
            text_offset: *cursor,
            children: Vec::new(),
        });
    }
    if tag == HtmlTag::Svg {
        let (text_content, text_offset) =
            crate::svg_compile::consume_svg_inner_body(input, cursor, tag_offset)?;
        return Ok(RawHtmlNode::Element {
            tag,
            custom_tag,
            tag_offset,
            attributes,
            text_content,
            text_offset,
            children: Vec::new(),
        });
    }
    let close_name = custom_tag.as_deref().unwrap_or(tag.as_str());
    let (text_content, text_offset, children) =
        parse_element_body_and_close(input, cursor, tag, close_name, tag_offset)?;
    Ok(RawHtmlNode::Element {
        tag,
        custom_tag,
        tag_offset,
        attributes,
        text_content,
        text_offset,
        children,
    })
}
