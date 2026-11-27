//! In-place CSS-only hot-patch fast path for `krui-preview`.
//!
//! When a `.css` or `<style>` edit modifies only stylesheets or embedded `@font-face`
//! assets while keeping HTML structure, inline attributes, control-flow branches, and
//! reactive `{...}` bindings identical, swaps Stylo author stylesheets in place without
//! unmounting DOM nodes.

use crate::preflight::validate_compiled_document_against_registry;
use krui_compiler::{CompileError, CompiledDocument, CompiledNode, CompiledNodeKind};
use krui_core::{RegistryRef, ViewTree};
use std::path::Path;
use std::sync::Arc;

/// Registers embedded font files, updates `unstyled` state, and applies document stylesheets onto `tree`.
pub(crate) fn apply_document_styles(
    tree: &mut ViewTree,
    doc: &CompiledDocument,
    replace_only: bool,
) {
    let base_dir = doc.source_path.as_deref().and_then(Path::parent);
    for (font_key, font_path) in &doc.embedded_font_files {
        let resolved = if font_path.is_file() {
            font_path.clone()
        } else {
            let raw = font_path.to_string_lossy();
            crate::media_slots::resolve_preview_asset_path(raw.as_ref(), base_dir)
        };
        if let Ok(bytes) = std::fs::read(&resolved) {
            tree.register_embedded_font(font_key, Arc::from(bytes));
        }
    }
    tree.set_unstyled(doc.unstyled);
    let sheet_refs: Vec<&str> = doc.stylesheets.iter().map(String::as_str).collect();
    if replace_only {
        tree.replace_author_stylesheets(&sheet_refs);
    } else {
        tree.set_document_stylesheets(&sheet_refs);
    }
}

/// Attempts to hot-patch `new_doc`'s stylesheets and `@font-face` assets onto `tree` in place
/// without unmounting DOM nodes.
///
/// - Returns `Err(CompileError)` if `new_doc` fails preflight validation against `registry`.
/// - Returns `Ok(Some(node_count))` if `old_doc` and `new_doc` have identical DOM structure and bindings.
/// - Returns `Ok(None)` if structural elements, inline styles, or bindings changed (caller runs remount).
pub fn try_hot_patch_compiled_styles_in_place(
    tree: &mut ViewTree,
    old_doc: &CompiledDocument,
    new_doc: &CompiledDocument,
    registry: &RegistryRef,
) -> Result<Option<usize>, CompileError> {
    if !cfg!(debug_assertions) {
        return Ok(None);
    }
    validate_compiled_document_against_registry(new_doc, registry.as_ref())?;
    if tree.node_count() == 0 || !can_hot_patch_styles_only(old_doc, new_doc) {
        return Ok(None);
    }
    apply_document_styles(tree, new_doc, true);
    Ok(Some(tree.node_count()))
}

/// Returns `true` if `old_doc` and `new_doc` have identical HTML element hierarchies, attributes,
/// inline styles, control-flow blocks, and reactive `{...}` bindings, differing only in stylesheets.
pub fn can_hot_patch_styles_only(old_doc: &CompiledDocument, new_doc: &CompiledDocument) -> bool {
    old_doc.unstyled == new_doc.unstyled && same_node_slice(&old_doc.roots, &new_doc.roots)
}

fn same_node_slice(a: &[CompiledNode], b: &[CompiledNode]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(x, y)| same_node_structure(x, y))
}

fn same_node_structure(old: &CompiledNode, new: &CompiledNode) -> bool {
    match (&old.kind, &new.kind) {
        (CompiledNodeKind::If(oi), CompiledNodeKind::If(ni)) => {
            oi.branches.len() == ni.branches.len()
                && oi.branches.iter().zip(ni.branches.iter()).all(|(ob, nb)| {
                    ob.condition == nb.condition && same_node_slice(&ob.children, &nb.children)
                })
        }
        (CompiledNodeKind::For(of), CompiledNodeKind::For(nf)) => {
            of.item_var == nf.item_var
                && of.list == nf.list
                && of.key == nf.key
                && same_node_slice(&of.body, &nf.body)
                && same_node_slice(&of.empty_body, &nf.empty_body)
        }
        (CompiledNodeKind::Element, CompiledNodeKind::Element) => {
            same_element_attrs_and_slots(old, new)
                && same_element_events_and_controls(old, new)
                && same_node_slice(&old.children, &new.children)
        }
        _ => false,
    }
}

fn same_element_attrs_and_slots(old: &CompiledNode, new: &CompiledNode) -> bool {
    old.tag == new.tag
        && old.id == new.id
        && old.classes == new.classes
        && old.static_inline_style == new.static_inline_style
        && old.dynamic_inline_style_parts == new.dynamic_inline_style_parts
        && old.paint_bindings == new.paint_bindings
        && old.layout_bindings == new.layout_bindings
        && old.text_parts == new.text_parts
        && old.placeholder_parts == new.placeholder_parts
        && old.title_parts == new.title_parts
        && old.aria_label == new.aria_label
        && old.aria_label_parts == new.aria_label_parts
        && old.img_src == new.img_src
        && old.img_alt == new.img_alt
        && old.link_href == new.link_href
        && old.svg_data == new.svg_data
        && old.class_parts == new.class_parts
        && old.custom_tag == new.custom_tag
        && old.static_attrs == new.static_attrs
        && old.dynamic_attrs == new.dynamic_attrs
        && old.rich_spans == new.rich_spans
}

fn same_element_events_and_controls(old: &CompiledNode, new: &CompiledNode) -> bool {
    old.on_click == new.on_click
        && old.on_focus == new.on_focus
        && old.on_blur == new.on_blur
        && old.on_pointer_down == new.on_pointer_down
        && old.on_pointer_up == new.on_pointer_up
        && old.on_pointer_move == new.on_pointer_move
        && old.on_context_menu == new.on_context_menu
        && old.on_wheel == new.on_wheel
        && old.on_scroll == new.on_scroll
        && old.on_input == new.on_input
        && old.on_change == new.on_change
        && old.on_keydown == new.on_keydown
        && old.on_submit == new.on_submit
        && old.on_reset == new.on_reset
        && old.bind_value == new.bind_value
        && old.canvas_draw == new.canvas_draw
        && old.input_type == new.input_type
        && old.button_type == new.button_type
        && old.is_secret_input == new.is_secret_input
        && old.disabled == new.disabled
        && old.required == new.required
        && old.checked == new.checked
        && old.bind_checked == new.bind_checked
        && old.label_for == new.label_for
        && old.control_name == new.control_name
        && old.range_bounds == new.range_bounds
        && old.select_options == new.select_options
        && old.select_children == new.select_children
}
