//! Component prop parsing, `<slot/>` splicing, and `{prop}` expression substitution.

use crate::component_slots::SlotBindings;
use crate::expr::{parse_template_expr, BinaryOp, TemplateExpr, TemplateLit};
use crate::html::{RawAttr, RawHtmlNode, RawIfBranch};
use crate::html_lexer::scan_template_expr_hole;
use crate::interpolation::{is_valid_rust_ident, parse_interpolated_text_strict};
use crate::{CompileError, CompiledTextPart};
use std::collections::HashMap;

pub(crate) fn parse_component_props(
    source: &str,
    comp_name: &str,
    attrs: &[RawAttr],
) -> Result<HashMap<String, TemplateExpr>, CompileError> {
    let mut map = HashMap::with_capacity(attrs.len());
    for (k, v, had_equals, attr_offset) in attrs {
        if !is_valid_rust_ident(k) {
            return Err(CompileError::at_offset(
                source,
                *attr_offset,
                format!(
                    "Invalid prop name `{k}` on component `<{comp_name}>` (expected a Rust identifier)"
                ),
            ));
        }
        let expr = if !*had_equals {
            TemplateExpr::Literal(TemplateLit::Bool(true))
        } else {
            let parts = parse_interpolated_text_strict(source, *attr_offset, v)?;
            parts_to_prop_expr(parts)
        };
        map.insert(k.clone(), expr);
    }
    Ok(map)
}

fn parts_to_prop_expr(parts: Vec<CompiledTextPart>) -> TemplateExpr {
    if parts.is_empty() {
        return TemplateExpr::Literal(TemplateLit::Str(String::new()));
    }
    let mut iter = parts.into_iter().map(|p| match p {
        CompiledTextPart::Static(s) => TemplateExpr::Literal(TemplateLit::Str(s)),
        CompiledTextPart::Dynamic { expr, .. } => expr,
    });
    let first = iter
        .next()
        .unwrap_or_else(|| TemplateExpr::Literal(TemplateLit::Str(String::new())));
    iter.fold(first, |acc, next| TemplateExpr::Binary {
        op: BinaryOp::Add,
        left: Box::new(acc),
        right: Box::new(next),
    })
}

pub(crate) fn instantiate_component_body(
    nodes: &[RawHtmlNode],
    props: &HashMap<String, TemplateExpr>,
    slots: &SlotBindings,
    call_offset: usize,
) -> Vec<RawHtmlNode> {
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            RawHtmlNode::Slot {
                name: None,
                fallback,
                ..
            } => {
                if slots.default.is_empty() {
                    let empty_slots = SlotBindings::default();
                    out.extend(instantiate_component_body(
                        fallback,
                        props,
                        &empty_slots,
                        call_offset,
                    ));
                } else {
                    out.extend(slots.default.iter().cloned());
                }
            }
            RawHtmlNode::Slot {
                name: Some(n),
                fallback,
                ..
            } => match slots.named.get(n) {
                Some(list) if !list.is_empty() => out.extend(list.iter().cloned()),
                _ => {
                    let empty_slots = SlotBindings::default();
                    out.extend(instantiate_component_body(
                        fallback,
                        props,
                        &empty_slots,
                        call_offset,
                    ));
                }
            },
            RawHtmlNode::Element { .. } => {
                if let Some(elem) = try_instantiate_element(node, props, slots, call_offset) {
                    out.push(elem);
                }
            }
            other => out.push(instantiate_single_node(other, props, slots, call_offset)),
        }
    }
    out
}

fn is_pure_slot_wrapper_tree(node: &RawHtmlNode) -> bool {
    match node {
        RawHtmlNode::Slot { .. } => true,
        RawHtmlNode::Element {
            attributes,
            text_content,
            children,
            ..
        } => {
            text_content.trim().is_empty()
                && !attributes
                    .iter()
                    .any(|(k, _, _, _)| k == "id" || k.starts_with("on") || k.starts_with("bind:"))
                && !children.is_empty()
                && children.iter().all(is_pure_slot_wrapper_tree)
        }
        _ => false,
    }
}

fn try_instantiate_element(
    node: &RawHtmlNode,
    props: &HashMap<String, TemplateExpr>,
    slots: &SlotBindings,
    call_offset: usize,
) -> Option<RawHtmlNode> {
    let RawHtmlNode::Element {
        tag,
        custom_tag,
        attributes,
        text_content,
        children,
        ..
    } = node
    else {
        return Some(instantiate_single_node(node, props, slots, call_offset));
    };
    let instantiated_children = instantiate_component_body(children, props, slots, call_offset);
    if instantiated_children.is_empty() && is_pure_slot_wrapper_tree(node) {
        return None;
    }
    Some(RawHtmlNode::Element {
        tag: *tag,
        custom_tag: custom_tag.clone(),
        tag_offset: call_offset,
        attributes: rewrite_attrs(attributes, props, call_offset, false),
        text_content: rewrite_interpolated(text_content, props, false, true),
        text_offset: call_offset,
        children: instantiated_children,
    })
}

fn instantiate_single_node(
    node: &RawHtmlNode,
    props: &HashMap<String, TemplateExpr>,
    slots: &SlotBindings,
    call_offset: usize,
) -> RawHtmlNode {
    match node {
        RawHtmlNode::Slot { name, fallback, .. } => {
            let empty_slots = SlotBindings::default();
            RawHtmlNode::Slot {
                name: name.clone(),
                fallback: instantiate_component_body(fallback, props, &empty_slots, call_offset),
                tag_offset: call_offset,
            }
        }
        RawHtmlNode::TextRun { text, .. } => RawHtmlNode::TextRun {
            text: rewrite_interpolated(text, props, false, true),
            offset: call_offset,
        },
        RawHtmlNode::Element {
            tag,
            custom_tag,
            attributes,
            text_content,
            children,
            ..
        } => RawHtmlNode::Element {
            tag: *tag,
            custom_tag: custom_tag.clone(),
            tag_offset: call_offset,
            attributes: rewrite_attrs(attributes, props, call_offset, false),
            text_content: rewrite_interpolated(text_content, props, false, true),
            text_offset: call_offset,
            children: instantiate_component_body(children, props, slots, call_offset),
        },
        RawHtmlNode::Component {
            name,
            attributes,
            children,
            ..
        } => RawHtmlNode::Component {
            name: name.clone(),
            tag_offset: call_offset,
            attributes: rewrite_attrs(attributes, props, call_offset, true),
            children: instantiate_component_body(children, props, slots, call_offset),
        },
        RawHtmlNode::If { branches, .. } => {
            instantiate_if_node(branches, props, slots, call_offset)
        }
        RawHtmlNode::For {
            item_var,
            list_expr,
            key_expr,
            body,
            empty_body,
            ..
        } => instantiate_for_node(
            (item_var, list_expr, key_expr),
            (body, empty_body),
            props,
            slots,
            call_offset,
        ),
    }
}

fn instantiate_if_node(
    branches: &[RawIfBranch],
    props: &HashMap<String, TemplateExpr>,
    slots: &SlotBindings,
    call_offset: usize,
) -> RawHtmlNode {
    let new_branches = branches
        .iter()
        .map(|br| RawIfBranch {
            condition: br
                .condition
                .as_ref()
                .map(|(c, _)| (rewrite_raw_expr(c, props), call_offset)),
            children: instantiate_component_body(&br.children, props, slots, call_offset),
        })
        .collect();
    RawHtmlNode::If {
        tag_offset: call_offset,
        branches: new_branches,
    }
}

fn instantiate_for_node(
    (item_var, list_expr, key_expr): (&str, &str, &str),
    (body, empty_body): (&[RawHtmlNode], &[RawHtmlNode]),
    props: &HashMap<String, TemplateExpr>,
    slots: &SlotBindings,
    call_offset: usize,
) -> RawHtmlNode {
    let new_list = rewrite_raw_expr(list_expr, props);
    let mut body_props = props.clone();
    body_props.remove(item_var);
    RawHtmlNode::For {
        tag_offset: call_offset,
        item_var: item_var.to_string(),
        list_expr: new_list,
        key_expr: rewrite_raw_expr(key_expr, &body_props),
        body: instantiate_component_body(body, &body_props, slots, call_offset),
        empty_body: instantiate_component_body(empty_body, props, slots, call_offset),
    }
}

fn rewrite_attrs(
    attrs: &[RawAttr],
    props: &HashMap<String, TemplateExpr>,
    call_offset: usize,
    target_is_component: bool,
) -> Vec<RawAttr> {
    let mut out = Vec::with_capacity(attrs.len());
    for (k, v, had_eq, _) in attrs {
        if !target_is_component && should_omit_empty_optional_attr(k, v, *had_eq, props) {
            continue;
        }
        let force_braces = attr_requires_braces(k);
        let new_v = if *had_eq {
            rewrite_interpolated(v, props, force_braces, !target_is_component)
        } else {
            v.clone()
        };
        out.push((k.clone(), new_v, *had_eq, call_offset));
    }
    out
}

fn should_omit_empty_optional_attr(
    k: &str,
    v: &str,
    had_eq: bool,
    props: &HashMap<String, TemplateExpr>,
) -> bool {
    if !had_eq || !is_omittable_optional_attr(k) {
        return false;
    }
    let trimmed = v.trim();
    let Some(prop_name) = trimmed
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .map(str::trim)
    else {
        return false;
    };
    matches!(
        props.get(prop_name),
        Some(TemplateExpr::Literal(TemplateLit::Str(s))) if s.is_empty()
    )
}

fn is_omittable_optional_attr(k: &str) -> bool {
    let kl = k.to_ascii_lowercase();
    kl.starts_with("on")
        || kl.starts_with("bind:")
        || kl.starts_with("bind-")
        || matches!(
            kl.as_str(),
            "id" | "title"
                | "aria-label"
                | "placeholder"
                | "href"
                | "value"
                | "checked"
                | "name"
                | "for"
                | "rows"
                | "min"
                | "max"
                | "step"
                | "draw"
        )
}

fn attr_requires_braces(k: &str) -> bool {
    let kl = k.to_ascii_lowercase();
    kl.starts_with("on") || kl.starts_with("bind:") || kl.starts_with("bind-") || kl == "draw"
}

fn rewrite_raw_expr(raw: &str, props: &HashMap<String, TemplateExpr>) -> String {
    if props.is_empty() {
        return raw.to_string();
    }
    match parse_template_expr(raw, 0, raw) {
        Ok(ast) => ast.substitute_props(props).to_string(),
        Err(_) => raw.to_string(),
    }
}

fn rewrite_interpolated(
    input: &str,
    props: &HashMap<String, TemplateExpr>,
    force_braces: bool,
    unbox_bool: bool,
) -> String {
    if props.is_empty() || !input.contains("{") {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i..].starts_with("{{") {
            out.push_str("{{");
            i += 2;
            continue;
        }
        if input[i..].starts_with("{") {
            if let Some(next_i) =
                try_rewrite_hole(input, i, props, force_braces, unbox_bool, &mut out)
            {
                i = next_i;
                continue;
            }
        }
        let ch = input[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn try_rewrite_hole(
    input: &str,
    i: usize,
    props: &HashMap<String, TemplateExpr>,
    force_braces: bool,
    unbox_bool: bool,
    out: &mut String,
) -> Option<usize> {
    let close = scan_template_expr_hole(input, i).ok()?;
    let inner = input[i + 1..close].trim();
    let (has_frame, clean) = match inner.strip_prefix("frame:") {
        Some(rest) => (true, rest.trim()),
        None => (false, inner),
    };
    let ast = parse_template_expr(clean, 0, clean).ok()?;
    let sub = ast.substitute_props(props);
    if !force_braces && !has_frame {
        match &sub {
            TemplateExpr::Literal(TemplateLit::Str(s)) => {
                push_escaped_static_text(out, s);
                return Some(close + 1);
            }
            TemplateExpr::Literal(TemplateLit::Bool(b)) if unbox_bool => {
                out.push_str(if *b { "true" } else { "false" });
                return Some(close + 1);
            }
            _ => {}
        }
    }
    if has_frame {
        out.push_str(&format!("{{frame:{sub}}}"));
    } else {
        out.push_str(&format!("{{{sub}}}"));
    }
    Some(close + 1)
}

fn push_escaped_static_text(out: &mut String, s: &str) {
    let escaped = s
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace("{", "{{")
        .replace("}", "}}");
    out.push_str(&escaped);
}
