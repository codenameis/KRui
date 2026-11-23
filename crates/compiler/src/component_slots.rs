//! Component prop schema validation, default-prop merging, and named `<slot name="...">` partitioning.

use crate::component_subst::parse_component_props;
use crate::expr::TemplateExpr;
use crate::html::{RawAttr, RawHtmlNode, RawIfBranch};
use crate::html_control_flow::is_valid_slot_name;
use crate::CompileError;
use std::collections::{HashMap, HashSet};

/// Expanded children partitioned into default `<slot/>` and `<slot name="..."/>` buckets.
#[derive(Clone, Debug, Default)]
pub(crate) struct SlotBindings {
    pub(crate) default: Vec<RawHtmlNode>,
    pub(crate) named: HashMap<String, Vec<RawHtmlNode>>,
}

/// Unexpanded caller children grouped into default nodes and named slot entries `(name, offset, nodes)`.
#[derive(Clone, Debug, Default)]
pub(crate) struct RawSlotChildren {
    pub(crate) default: Vec<RawHtmlNode>,
    pub(crate) named: Vec<(String, usize, Vec<RawHtmlNode>)>,
}

/// Pre-parses `<template>` header default props into deterministic `(name, TemplateExpr)` pairs.
pub(crate) fn compile_schema_defaults(
    source: &str,
    comp_name: &str,
    default_props: &[RawAttr],
) -> Result<Vec<(String, TemplateExpr)>, CompileError> {
    if default_props.is_empty() {
        return Ok(Vec::new());
    }
    let map = parse_component_props(source, comp_name, default_props)?;
    Ok(default_props
        .iter()
        .filter_map(|(k, _, _, _)| map.get(k).map(|expr| (k.clone(), expr.clone())))
        .collect())
}

/// Validates caller props against `schema_defaults` (when declared on `<template>`) and fills omitted defaults.
pub(crate) fn merge_and_validate_props(
    caller_source: &str,
    comp_name: &str,
    schema_defaults: &[(String, TemplateExpr)],
    caller_attrs: &[RawAttr],
) -> Result<HashMap<String, TemplateExpr>, CompileError> {
    let mut map = parse_component_props(caller_source, comp_name, caller_attrs)?;
    if schema_defaults.is_empty() {
        return Ok(map);
    }
    let allowed: HashSet<&str> = schema_defaults.iter().map(|(k, _)| k.as_str()).collect();
    for (k, _, _, attr_offset) in caller_attrs {
        if !allowed.contains(k.as_str()) {
            let mut names: Vec<&str> = schema_defaults.iter().map(|(n, _)| n.as_str()).collect();
            names.sort_unstable();
            return Err(CompileError::at_offset(
                caller_source,
                *attr_offset,
                format!(
                    "Unknown prop `{k}` on component `<{comp_name}>` (allowed props: {})",
                    names.join(", ")
                ),
            ));
        }
    }
    for (dk, dexpr) in schema_defaults {
        map.entry(dk.clone()).or_insert_with(|| dexpr.clone());
    }
    Ok(map)
}

/// Separates direct caller children with a `slot="name"` attribute into named buckets and strips the `slot` attr.
pub(crate) fn partition_slot_children(
    source: &str,
    children: Vec<RawHtmlNode>,
) -> Result<RawSlotChildren, CompileError> {
    let mut out = RawSlotChildren::default();
    let mut named_index: HashMap<String, usize> = HashMap::new();
    for mut node in children {
        let extracted = extract_slot_attr_from_node(source, &mut node)?;
        let Some((slot_name, attr_off)) = extracted else {
            out.default.push(node);
            continue;
        };
        if let Some(&idx) = named_index.get(&slot_name) {
            out.named[idx].2.push(node);
        } else {
            let idx = out.named.len();
            named_index.insert(slot_name.clone(), idx);
            out.named.push((slot_name, attr_off, vec![node]));
        }
    }
    Ok(out)
}

fn extract_slot_attr_from_node(
    source: &str,
    node: &mut RawHtmlNode,
) -> Result<Option<(String, usize)>, CompileError> {
    let attrs = match node {
        RawHtmlNode::Element { attributes, .. } | RawHtmlNode::Component { attributes, .. } => {
            attributes
        }
        _ => return Ok(None),
    };
    let Some(pos) = attrs.iter().position(|(k, _, _, _)| k == "slot") else {
        return Ok(None);
    };
    let (_, raw_val, had_eq, attr_off) = attrs.remove(pos);
    let trimmed = raw_val.trim();
    if !had_eq || !is_valid_slot_name(trimmed) {
        return Err(CompileError::at_offset(
            source,
            attr_off,
            format!("Invalid `slot=\"{trimmed}\"` attribute; expected a non-empty slot name"),
        ));
    }
    Ok(Some((trimmed.to_string(), attr_off)))
}

/// Ensures every `slot="..."` passed by the caller matches a `<slot name="..."/>` in the component body.
pub(crate) fn validate_caller_slots(
    source: &str,
    comp_name: &str,
    named: &[(String, usize, Vec<RawHtmlNode>)],
    body: &[RawHtmlNode],
) -> Result<(), CompileError> {
    if named.is_empty() {
        return Ok(());
    }
    let mut declared = HashSet::new();
    collect_declared_named_slots(body, &mut declared);
    for (slot_name, off, _) in named {
        if !declared.contains(slot_name.as_str()) {
            let mut list: Vec<&str> = declared.iter().map(String::as_str).collect();
            list.sort_unstable();
            let hint = if list.is_empty() {
                "none".to_string()
            } else {
                list.join(", ")
            };
            return Err(CompileError::at_offset(
                source,
                *off,
                format!("Unknown slot `{slot_name}` on component `<{comp_name}>` (declared named slots: {hint})"),
            ));
        }
    }
    Ok(())
}

fn collect_declared_named_slots(nodes: &[RawHtmlNode], out: &mut HashSet<String>) {
    for node in nodes {
        match node {
            RawHtmlNode::Slot { name, fallback, .. } => {
                if let Some(n) = name {
                    out.insert(n.clone());
                }
                collect_declared_named_slots(fallback, out);
            }
            RawHtmlNode::Element { children, .. } | RawHtmlNode::Component { children, .. } => {
                collect_declared_named_slots(children, out);
            }
            RawHtmlNode::If { branches, .. } => collect_slots_in_if(branches, out),
            RawHtmlNode::For {
                body, empty_body, ..
            } => {
                collect_declared_named_slots(body, out);
                collect_declared_named_slots(empty_body, out);
            }
            _ => {}
        }
    }
}

fn collect_slots_in_if(branches: &[RawIfBranch], out: &mut HashSet<String>) {
    for br in branches {
        collect_declared_named_slots(&br.children, out);
    }
}
