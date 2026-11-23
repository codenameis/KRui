//! Build-time `<Card ...>...</Card>` component and `<slot/>` expansion,
//! external `Card.krui` resolution, `<style>` merging, and cycle detection.

use crate::builtins::{load_builtin_pack_on_demand, lookup_builtin_pack, BuiltinPack};
use crate::component_slots::{
    compile_schema_defaults, merge_and_validate_props, partition_slot_children,
    validate_caller_slots, SlotBindings,
};
use crate::component_subst::instantiate_component_body;
use crate::css_collect::CollectedStylesheets;
use crate::expr::TemplateExpr;
use crate::html::{parse_html_document, RawAttr, RawHtmlNode, RawIfBranch, RawTemplateDef};
use crate::html_lexer::extract_style_and_link_blocks;
use crate::source_loader::{
    load_linked_and_companion_css, read_text_with_virtual, resolve_path_with_virtual,
    VirtualBufferMap,
};
use crate::CompileError;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
struct LoadedComponent {
    schema_defaults: Vec<(String, TemplateExpr)>,
    body: Vec<RawHtmlNode>,
    base_dir: Option<PathBuf>,
}

/// Stateful build-time component resolver and expander.
pub(crate) struct ComponentExpander<'a> {
    templates: HashMap<String, LoadedComponent>,
    loaded_builtins: HashMap<String, LoadedComponent>,
    loaded_packs: HashSet<BuiltinPack>,
    root_dir: Option<PathBuf>,
    virtual_buffers: Option<&'a VirtualBufferMap>,
    pub(crate) collector: &'a mut CollectedStylesheets,
    pub(crate) external_component_paths: &'a mut Vec<PathBuf>,
}

impl<'a> ComponentExpander<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        source: &str,
        inline_templates: Vec<RawTemplateDef>,
        krui_path: Option<&Path>,
        virtual_buffers: Option<&'a VirtualBufferMap>,
        collector: &'a mut CollectedStylesheets,
        external_component_paths: &'a mut Vec<PathBuf>,
    ) -> Result<Self, CompileError> {
        let root_dir = krui_path.and_then(|p| p.parent()).map(Path::to_path_buf);
        let mut templates = HashMap::with_capacity(inline_templates.len());
        for tpl in inline_templates {
            let schema_defaults = compile_schema_defaults(source, &tpl.name, &tpl.default_props)?;
            templates.insert(
                tpl.name,
                LoadedComponent {
                    schema_defaults,
                    body: tpl.body,
                    base_dir: root_dir.clone(),
                },
            );
        }
        Ok(Self {
            templates,
            loaded_builtins: HashMap::new(),
            loaded_packs: HashSet::new(),
            root_dir,
            virtual_buffers,
            collector,
            external_component_paths,
        })
    }

    /// Expands all uppercase `<Component>` tags recursively across `nodes`.
    pub(crate) fn expand_root_nodes(
        &mut self,
        caller_source: &str,
        nodes: Vec<RawHtmlNode>,
    ) -> Result<Vec<RawHtmlNode>, CompileError> {
        let cur_dir = self.root_dir.clone();
        let mut active_stack = Vec::new();
        self.expand_node_list(caller_source, nodes, cur_dir.as_deref(), &mut active_stack)
    }

    fn expand_node_list(
        &mut self,
        caller_source: &str,
        nodes: Vec<RawHtmlNode>,
        cur_dir: Option<&Path>,
        active_stack: &mut Vec<String>,
    ) -> Result<Vec<RawHtmlNode>, CompileError> {
        let mut out = Vec::with_capacity(nodes.len());
        for node in nodes {
            match node {
                RawHtmlNode::Component {
                    name,
                    tag_offset,
                    attributes,
                    children,
                } => {
                    let expanded = self.expand_single_component(
                        caller_source,
                        (name, tag_offset, attributes, children),
                        cur_dir,
                        active_stack,
                    )?;
                    out.extend(expanded);
                }
                other => out.push(self.expand_non_component(
                    caller_source,
                    other,
                    cur_dir,
                    active_stack,
                )?),
            }
        }
        Ok(out)
    }

    fn expand_non_component(
        &mut self,
        caller_source: &str,
        node: RawHtmlNode,
        cur_dir: Option<&Path>,
        active_stack: &mut Vec<String>,
    ) -> Result<RawHtmlNode, CompileError> {
        match node {
            RawHtmlNode::Element {
                tag,
                custom_tag,
                tag_offset,
                attributes,
                text_content,
                text_offset,
                children,
            } => Ok(RawHtmlNode::Element {
                tag,
                custom_tag,
                tag_offset,
                attributes,
                text_content,
                text_offset,
                children: self.expand_node_list(caller_source, children, cur_dir, active_stack)?,
            }),
            RawHtmlNode::If {
                tag_offset,
                branches,
            } => Ok(RawHtmlNode::If {
                tag_offset,
                branches: self.expand_if_branches(
                    caller_source,
                    branches,
                    cur_dir,
                    active_stack,
                )?,
            }),
            RawHtmlNode::For {
                tag_offset,
                item_var,
                list_expr,
                key_expr,
                body,
                empty_body,
            } => Ok(RawHtmlNode::For {
                tag_offset,
                item_var,
                list_expr,
                key_expr,
                body: self.expand_node_list(caller_source, body, cur_dir, active_stack)?,
                empty_body: self.expand_node_list(
                    caller_source,
                    empty_body,
                    cur_dir,
                    active_stack,
                )?,
            }),
            other => Ok(other),
        }
    }

    fn expand_if_branches(
        &mut self,
        caller_source: &str,
        branches: Vec<RawIfBranch>,
        cur_dir: Option<&Path>,
        active_stack: &mut Vec<String>,
    ) -> Result<Vec<RawIfBranch>, CompileError> {
        let mut exp_branches = Vec::with_capacity(branches.len());
        for br in branches {
            exp_branches.push(RawIfBranch {
                condition: br.condition,
                children: self.expand_node_list(
                    caller_source,
                    br.children,
                    cur_dir,
                    active_stack,
                )?,
            });
        }
        Ok(exp_branches)
    }

    fn expand_single_component(
        &mut self,
        caller_source: &str,
        (name, tag_offset, attributes, caller_children): (
            String,
            usize,
            Vec<RawAttr>,
            Vec<RawHtmlNode>,
        ),
        cur_dir: Option<&Path>,
        active_stack: &mut Vec<String>,
    ) -> Result<Vec<RawHtmlNode>, CompileError> {
        if active_stack.iter().any(|s| s == &name) {
            let mut chain = active_stack.join(" -> ");
            chain.push_str(" -> ");
            chain.push_str(&name);
            return Err(CompileError::at_offset(
                caller_source,
                tag_offset,
                format!("Recursive component cycle detected: `{chain}`"),
            ));
        }
        let raw_slots = partition_slot_children(caller_source, caller_children)?;
        let loaded = self.resolve_or_load_component(caller_source, &name, tag_offset, cur_dir)?;
        validate_caller_slots(caller_source, &name, &raw_slots.named, &loaded.body)?;
        let props =
            merge_and_validate_props(caller_source, &name, &loaded.schema_defaults, &attributes)?;
        let slot_bindings =
            self.expand_slot_bindings(caller_source, raw_slots, cur_dir, active_stack)?;
        let instantiated =
            instantiate_component_body(&loaded.body, &props, &slot_bindings, tag_offset);

        active_stack.push(name);
        let next_dir = loaded.base_dir.as_deref().or(cur_dir);
        let result = self.expand_node_list(caller_source, instantiated, next_dir, active_stack);
        active_stack.pop();
        result
    }

    fn expand_slot_bindings(
        &mut self,
        caller_source: &str,
        raw: crate::component_slots::RawSlotChildren,
        cur_dir: Option<&Path>,
        active_stack: &mut Vec<String>,
    ) -> Result<SlotBindings, CompileError> {
        let default = self.expand_node_list(caller_source, raw.default, cur_dir, active_stack)?;
        let mut named = HashMap::with_capacity(raw.named.len());
        for (slot_name, _, nodes) in raw.named {
            let expanded = self.expand_node_list(caller_source, nodes, cur_dir, active_stack)?;
            named.insert(slot_name, expanded);
        }
        Ok(SlotBindings { default, named })
    }

    fn resolve_or_load_component(
        &mut self,
        caller_source: &str,
        name: &str,
        tag_offset: usize,
        cur_dir: Option<&Path>,
    ) -> Result<LoadedComponent, CompileError> {
        if let Some(existing) = self.templates.get(name) {
            return Ok(existing.clone());
        }
        if let Some(comp_path) = find_external_component_file(
            name,
            cur_dir,
            self.root_dir.as_deref(),
            self.virtual_buffers,
        ) {
            return self.load_external_component_file(caller_source, tag_offset, name, &comp_path);
        }
        if let Some(builtin) = self.loaded_builtins.get(name) {
            return Ok(builtin.clone());
        }
        if let Some((pack, pack_src)) = lookup_builtin_pack(name) {
            let mut parsed = HashMap::new();
            load_builtin_pack_on_demand(
                pack,
                pack_src,
                &mut self.loaded_packs,
                self.collector,
                &mut parsed,
            )?;
            for (tpl_name, (schema_defaults, body)) in parsed {
                self.loaded_builtins.insert(
                    tpl_name,
                    LoadedComponent {
                        schema_defaults,
                        body,
                        base_dir: None,
                    },
                );
            }
            if let Some(builtin) = self.loaded_builtins.get(name) {
                return Ok(builtin.clone());
            }
        }
        Err(CompileError::at_offset(
            caller_source,
            tag_offset,
            format!(
                "Unknown component `<{name}>` (define `<template name=\"{name}\">` or create `{name}.krui` / `components/{name}.krui` / `icons/{name}.krui`)"
            ),
        ))
    }

    fn load_external_component_file(
        &mut self,
        caller_source: &str,
        tag_offset: usize,
        name: &str,
        comp_path: &Path,
    ) -> Result<LoadedComponent, CompileError> {
        let (canonical, comp_src) =
            read_component_source(caller_source, tag_offset, comp_path, self.virtual_buffers)?;
        if !self.external_component_paths.contains(&canonical) {
            self.external_component_paths.push(canonical.clone());
        }
        let html_text = self.merge_component_styles(name, &canonical, &comp_src)?;
        let attach = |mut err: CompileError| {
            if err.file.is_none() {
                err.file = Some(canonical.clone());
            }
            err
        };
        let (sub_templates, mut body) = parse_html_document(&html_text).map_err(attach)?;
        stamp_component_scope_on_nodes(&mut body, name);
        let comp_dir = canonical.parent().map(Path::to_path_buf);
        for mut sub in sub_templates {
            stamp_component_scope_on_nodes(&mut sub.body, name);
            let schema_defaults = compile_schema_defaults(&comp_src, &sub.name, &sub.default_props)
                .map_err(attach)?;
            self.templates.entry(sub.name).or_insert(LoadedComponent {
                schema_defaults,
                body: sub.body,
                base_dir: comp_dir.clone(),
            });
        }
        if body.is_empty() {
            if let Some(existing) = self.templates.get(name) {
                return Ok(existing.clone());
            }
        }
        let loaded = LoadedComponent {
            schema_defaults: Vec::new(),
            body,
            base_dir: comp_dir,
        };
        self.templates.insert(name.to_string(), loaded.clone());
        Ok(loaded)
    }

    fn merge_component_styles(
        &mut self,
        comp_name: &str,
        canonical: &Path,
        comp_src: &str,
    ) -> Result<String, CompileError> {
        let attach = |mut err: CompileError| {
            if err.file.is_none() {
                err.file = Some(canonical.to_path_buf());
            }
            err
        };
        let (styles, links, html_text) = extract_style_and_link_blocks(comp_src).map_err(attach)?;
        load_linked_and_companion_css(
            comp_src,
            &links,
            Some(canonical),
            self.virtual_buffers,
            Some(comp_name),
            self.collector,
        )
        .map_err(attach)?;
        for (css_text, start) in &styles {
            self.collector.push_component_css(
                comp_src,
                *start,
                css_text,
                Some(canonical),
                Some(comp_name),
                self.virtual_buffers,
            )?;
        }
        Ok(html_text)
    }
}

fn stamp_component_scope_on_nodes(nodes: &mut [RawHtmlNode], scope: &str) {
    for node in nodes {
        match node {
            RawHtmlNode::Element {
                attributes,
                children,
                ..
            } => {
                if let Some((_, val, _, _)) = attributes
                    .iter_mut()
                    .find(|(k, _, _, _)| k.eq_ignore_ascii_case("data-krui-scope"))
                {
                    if !val.split_whitespace().any(|t| t == scope) {
                        val.push(' ');
                        val.push_str(scope);
                    }
                } else {
                    attributes.push(("data-krui-scope".to_string(), scope.to_string(), true, 0));
                }
                stamp_component_scope_on_nodes(children, scope);
            }
            RawHtmlNode::If { branches, .. } => {
                for br in branches {
                    stamp_component_scope_on_nodes(&mut br.children, scope);
                }
            }
            RawHtmlNode::For {
                body, empty_body, ..
            } => {
                stamp_component_scope_on_nodes(body, scope);
                stamp_component_scope_on_nodes(empty_body, scope);
            }
            RawHtmlNode::Component { .. }
            | RawHtmlNode::Slot { .. }
            | RawHtmlNode::TextRun { .. } => {}
        }
    }
}

fn read_component_source(
    caller_source: &str,
    tag_offset: usize,
    comp_path: &Path,
    virtual_buffers: Option<&VirtualBufferMap>,
) -> Result<(PathBuf, String), CompileError> {
    let canonical = resolve_path_with_virtual(comp_path, virtual_buffers).map_err(|e| {
        CompileError::at_offset(
            caller_source,
            tag_offset,
            format!("Failed to resolve component `{}`: {e}", comp_path.display()),
        )
    })?;
    let comp_src = read_text_with_virtual(&canonical, virtual_buffers).map_err(|e| {
        CompileError::at_offset(
            caller_source,
            tag_offset,
            format!("Failed to read component `{}`: {e}", canonical.display()),
        )
    })?;
    Ok((canonical, comp_src))
}

fn component_file_exists(path: &Path, virtual_buffers: Option<&VirtualBufferMap>) -> bool {
    path.is_file() || crate::source_loader::lookup_virtual_buffer(virtual_buffers, path).is_some()
}

fn find_external_component_file(
    name: &str,
    cur_dir: Option<&Path>,
    root_dir: Option<&Path>,
    virtual_buffers: Option<&VirtualBufferMap>,
) -> Option<PathBuf> {
    let filename = format!("{name}.krui");
    for dir in [cur_dir, root_dir].into_iter().flatten() {
        let direct = dir.join(&filename);
        if component_file_exists(&direct, virtual_buffers) {
            return Some(direct);
        }
        for sub_name in ["components", "icons"] {
            let sub = dir.join(sub_name).join(&filename);
            if component_file_exists(&sub, virtual_buffers) {
                return Some(sub);
            }
        }
    }
    None
}
