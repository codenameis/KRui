//! Dev-preview mounting helpers for `custom_tag` and static/dynamic HTML attributes.

use crate::eval::eval_interpolated_parts;
use crate::PreviewSavedNodes;
use krui_compiler::{CompiledNode, CompiledTextPart};
use krui_core::{NodeId, RegistryRef, ViewTree};
use std::cell::RefCell;
use std::rc::Rc;

fn is_boolean_html_attr(name: &str) -> bool {
    matches!(
        name,
        "open" | "readonly" | "hidden" | "autofocus" | "multiple" | "novalidate" | "inert"
    )
}

pub(crate) fn bind_custom_tag_and_attrs(
    tree: &mut ViewTree,
    node_id: NodeId,
    node: &CompiledNode,
    registry: &RegistryRef,
) {
    if let Some(ref ct) = node.custom_tag {
        tree.set_custom_tag(node_id, ct);
    }
    for (k, v) in &node.static_attrs {
        tree.set_node_attr(node_id, k, v);
    }
    for binding in &node.dynamic_attrs {
        let name = binding.name.clone();
        let parts = binding.parts.clone();
        let reg = registry.clone();
        let has_dyn = parts
            .iter()
            .any(|p| matches!(p, CompiledTextPart::Dynamic { .. }));
        if !has_dyn {
            let val = eval_interpolated_parts(&parts, reg.as_ref(), false);
            if !(is_boolean_html_attr(&name) && val.trim().eq_ignore_ascii_case("false")) {
                tree.set_node_attr(node_id, &name, val);
            }
            continue;
        }
        tree.bind_dynamic_attr(node_id, name.clone(), move || {
            let val = eval_interpolated_parts(&parts, reg.as_ref(), false);
            if is_boolean_html_attr(&name) && val.trim().eq_ignore_ascii_case("false") {
                None
            } else {
                Some(val)
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn mount_element_node(
    tree: &mut ViewTree,
    parent: Option<NodeId>,
    node: &CompiledNode,
    registry: &RegistryRef,
    parent_path: &str,
    for_key: Option<&str>,
    sibling_index: usize,
    saved: Option<&Rc<RefCell<PreviewSavedNodes>>>,
) -> NodeId {
    let (node_id, path_str) = crate::spawn_and_bind_element_node(
        tree,
        parent,
        node,
        registry,
        parent_path,
        for_key,
        sibling_index,
        saved,
    );
    if node.is_synthetic_text_run {
        tree.mark_synthetic_text_run(node_id);
    }
    for (ch_idx, child) in node.children.iter().enumerate() {
        crate::mount_node_with_ctx(
            tree,
            Some(node_id),
            child,
            registry,
            &path_str,
            None,
            ch_idx,
            saved,
        );
    }
    node_id
}
