//! Recursively lowers expanded [`RawHtmlNode`] AST subtrees into slim [`CompiledNode`] IR
//! with Stylo-ready attributes/classes/inline styles, SVG vector ops, and slot bindings.

use crate::control_attrs::{build_compiled_element_node, parse_element_text_and_placeholder};
use crate::element_attrs::{extract_select_options, parse_element_attributes};
use crate::expr::parse_template_expr;
use crate::html::{RawAttr, RawHtmlNode, RawIfBranch};
use crate::inline_style_slots::analyze_element_inline_style;
use crate::interpolation::parse_interpolated_text_strict;
use crate::line_col_at;
use crate::rich_inline::maybe_collapse_rich_inline_paragraph;
use crate::{
    CompileError, CompileWarning, CompileWarningKind, CompiledFor, CompiledIf, CompiledIfBranch,
    CompiledNode, CompiledNodeKind, HtmlTag,
};

pub(crate) fn compile_expanded_roots(
    source: &str,
    raw_nodes: Vec<RawHtmlNode>,
    warnings: &mut Vec<CompileWarning>,
) -> Result<Vec<CompiledNode>, CompileError> {
    compile_node_list(source, raw_nodes, warnings)
}

fn compile_node_list(
    source: &str,
    raw_nodes: Vec<RawHtmlNode>,
    warnings: &mut Vec<CompileWarning>,
) -> Result<Vec<CompiledNode>, CompileError> {
    let mut out = Vec::with_capacity(raw_nodes.len());
    for raw in raw_nodes {
        match raw {
            RawHtmlNode::Element {
                tag,
                custom_tag,
                tag_offset,
                attributes,
                text_content,
                text_offset,
                children,
            } => out.push(compile_element_node(
                source,
                (tag, custom_tag, tag_offset),
                attributes,
                (text_content, text_offset),
                children,
                warnings,
            )?),
            RawHtmlNode::TextRun { text, offset } => {
                let mut span = CompiledNode::from_kind(CompiledNodeKind::Element);
                span.tag = HtmlTag::Span;
                span.is_synthetic_text_run = true;
                span.text_parts = parse_interpolated_text_strict(source, offset, &text)?;
                let (sl, sc) = line_col_at(source, offset);
                span.source_line = sl as u32;
                span.source_column = sc as u32;
                out.push(span);
            }
            RawHtmlNode::If {
                tag_offset,
                branches,
            } => {
                out.push(compile_if_node(source, tag_offset, branches, warnings)?);
            }
            RawHtmlNode::For {
                tag_offset,
                item_var,
                list_expr,
                key_expr,
                body,
                empty_body,
            } => {
                out.push(compile_for_node(
                    source,
                    tag_offset,
                    (item_var, list_expr, key_expr),
                    body,
                    empty_body,
                    warnings,
                )?);
            }
            RawHtmlNode::Component {
                tag_offset, name, ..
            } => {
                return Err(CompileError::at_offset(
                    source,
                    tag_offset,
                    format!("Unexpanded component `<{name}>` encountered during IR lowering"),
                ));
            }
            RawHtmlNode::Slot { tag_offset, .. } => {
                return Err(CompileError::at_offset(
                    source,
                    tag_offset,
                    "`<slot/>` is only valid inside a component `<template>` definition",
                ));
            }
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn compile_element_node(
    source: &str,
    (tag, custom_tag, tag_offset): (HtmlTag, Option<String>, usize),
    attributes: Vec<RawAttr>,
    (text_content, text_offset): (String, usize),
    mut raw_children: Vec<RawHtmlNode>,
    warnings: &mut Vec<CompileWarning>,
) -> Result<CompiledNode, CompileError> {
    let attrs = parse_element_attributes(source, tag, &attributes)?;
    let (sl, sc) = line_col_at(source, tag_offset);
    if !matches!(tag, HtmlTag::Input | HtmlTag::Textarea | HtmlTag::Select)
        && attrs.bind_value.is_some()
    {
        return Err(CompileError::at_offset(
            source,
            tag_offset,
            format!(
                "`bind:value` is only supported on `<input>`, `<textarea>`, and `<select>`, not `<{}>`",
                tag.as_str()
            ),
        ));
    }
    let select_spec = extract_select_options(source, tag, tag_offset, &mut raw_children)?;
    let svg_data = if tag == HtmlTag::Svg {
        Some(crate::svg_compile::compile_svg_element(
            source,
            tag_offset,
            &attrs.media.svg_root,
            &text_content,
            text_offset,
        )?)
    } else {
        None
    };
    let text_parts = if tag == HtmlTag::Svg {
        (Vec::new(), Vec::new())
    } else {
        parse_element_text_and_placeholder(source, &attrs, text_content, text_offset)?
    };
    let mut analysis = analyze_element_inline_style(source, tag, &attrs)?;
    let flow_hints = (analysis.explicit_flex_dir, analysis.explicit_align_items);
    warnings.append(&mut analysis.warnings);
    let children = compile_node_list(source, raw_children, warnings)?;

    let mut node =
        build_compiled_element_node(tag, attrs, analysis, text_parts, select_spec, children);
    if let Some(ref custom_name) = custom_tag {
        warnings.push(CompileWarning {
            kind: CompileWarningKind::UnknownHtmlElement,
            suppressed: false,
            node_tag: custom_name.clone(),
            property: "tag".to_string(),
            signal_expr: format!("{sl}:{sc}"),
            message: format!(
                "Unknown HTML element `<{custom_name}>`; treating as a generic `<div>` container"
            ),
            suggested_fix:
                "Replace with a semantic HTML5 tag or PascalCase component if unintended"
                    .to_string(),
        });
    }
    node.custom_tag = custom_tag;
    node.svg_data = svg_data;
    node.source_line = sl as u32;
    node.source_column = sc as u32;
    maybe_collapse_rich_inline_paragraph(&mut node, flow_hints);
    Ok(node)
}

fn compile_if_node(
    source: &str,
    _tag_offset: usize,
    branches: Vec<RawIfBranch>,
    warnings: &mut Vec<CompileWarning>,
) -> Result<CompiledNode, CompileError> {
    let mut compiled = Vec::with_capacity(branches.len());
    for br in branches {
        let (condition_expr, condition) = match br.condition {
            Some((expr_str, off)) => {
                let parsed = parse_template_expr(source, off, &expr_str)?;
                (Some(expr_str), Some(parsed))
            }
            None => (None, None),
        };
        let children = compile_node_list(source, br.children, warnings)?;
        compiled.push(CompiledIfBranch {
            condition_expr,
            condition,
            children,
        });
    }
    Ok(CompiledNode::from_kind(CompiledNodeKind::If(CompiledIf {
        branches: compiled,
    })))
}

fn compile_for_node(
    source: &str,
    tag_offset: usize,
    (item_var, list_expr, key_expr): (String, String, String),
    raw_body: Vec<RawHtmlNode>,
    raw_empty: Vec<RawHtmlNode>,
    warnings: &mut Vec<CompileWarning>,
) -> Result<CompiledNode, CompileError> {
    let list = parse_template_expr(source, tag_offset, &list_expr)?;
    let key = parse_template_expr(source, tag_offset, &key_expr)?;
    let body = compile_node_list(source, raw_body, warnings)?;
    let empty_body = compile_node_list(source, raw_empty, warnings)?;
    Ok(CompiledNode::from_kind(CompiledNodeKind::For(Box::new(
        CompiledFor {
            item_var,
            list_expr,
            list,
            key_expr,
            key,
            body,
            empty_body,
        },
    ))))
}
