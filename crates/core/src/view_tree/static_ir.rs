//! D1 Static UI descriptors emitted as `.rdata` slices by `krui-macros` and mounted via a single
//! table-driven loop with zero runtime markup parser (`AGENTS.md` Rule 2).

use crate::view_tree::{ButtonKind, InputKind, NodeId, ViewTree};
pub use krui_ui_types::static_ir::{
    StaticAttrDesc, StaticButtonKind, StaticDocumentDesc, StaticInputKind, StaticNodeDesc,
    StaticNodeKind, StaticSelectOptDesc,
};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

type StaticImgMap = HashMap<(usize, usize), Option<crate::DecodedImage>>;

static STATIC_IMG_CACHE: OnceLock<Mutex<StaticImgMap>> = OnceLock::new();

/// Decodes and caches a compile-time embedded static `<img>` asset on first mount.
pub fn mount_static_image_bytes(tree: &mut ViewTree, node_id: NodeId, bytes: &'static [u8]) {
    let key = (bytes.as_ptr() as usize, bytes.len());
    let decoded = {
        let cache = STATIC_IMG_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .entry(key)
            .or_insert_with(|| crate::DecodedImage::from_encoded_bytes(bytes).ok())
            .clone()
    };
    if let Some(img) = decoded {
        tree.set_node_decoded_image(node_id, img);
    }
}

/// Applies document-level `unstyled` + stylesheets (when top mount) and mounts `doc.nodes`.
#[inline(never)]
pub fn mount_static_document(
    tree: &mut ViewTree,
    root_parent: Option<NodeId>,
    doc: &StaticDocumentDesc,
    is_top_mount: bool,
) -> Vec<NodeId> {
    if is_top_mount {
        tree.set_unstyled(doc.unstyled);
        tree.set_document_stylesheets(doc.stylesheets);
    }
    mount_static_nodes(tree, root_parent, doc.nodes)
}

/// Mounts a pre-order slice of [`StaticNodeDesc`] entries under `root_parent`, returning all
/// allocated [`NodeId`]s indexed identically to `nodes`.
#[inline(never)]
pub fn mount_static_nodes(
    tree: &mut ViewTree,
    root_parent: Option<NodeId>,
    nodes: &[StaticNodeDesc],
) -> Vec<NodeId> {
    let mut ids = Vec::with_capacity(nodes.len());
    for desc in nodes {
        let parent = desc
            .parent_slot
            .and_then(|slot| ids.get(slot).copied())
            .or(root_parent);
        let nid = match desc.kind {
            StaticNodeKind::ControlFlowAnchor => tree.create_anchor_node(parent),
            StaticNodeKind::Element => mount_static_element(tree, parent, desc),
        };
        ids.push(nid);
    }
    ids
}

fn mount_static_element(
    tree: &mut ViewTree,
    parent: Option<NodeId>,
    desc: &StaticNodeDesc,
) -> NodeId {
    let nid = tree.create_element_node(
        parent,
        desc.tag,
        desc.id.map(str::to_string),
        desc.aria_label.map(str::to_string),
        desc.is_button,
        desc.is_input,
        desc.is_canvas,
        desc.is_secret_input,
    );
    #[cfg(feature = "preview")]
    if desc.source_line != 0 || desc.source_column != 0 {
        tree.set_node_source_span(nid, desc.source_line, desc.source_column);
    }
    if desc.is_synthetic_text_run {
        tree.mark_synthetic_text_run(nid);
    }
    if let Some(ct) = desc.custom_tag {
        tree.set_custom_tag(nid, ct);
    }
    for attr in desc.attrs {
        tree.set_node_attr(nid, attr.name, attr.value);
    }
    if let Some(css) = desc.inline_style {
        tree.set_inline_style(nid, css);
    }
    apply_static_control_meta(tree, nid, desc);
    apply_static_text_and_media(tree, nid, desc);
    if !desc.classes.is_empty() {
        tree.set_element_classes(nid, desc.classes);
    }
    if let Some(v) = desc.disabled {
        tree.set_disabled(nid, v);
    }
    if let Some(v) = desc.required {
        tree.set_required(nid, v);
    }
    if let Some(v) = desc.checked {
        tree.set_checked(nid, v);
    }
    nid
}

fn apply_static_control_meta(tree: &mut ViewTree, nid: NodeId, desc: &StaticNodeDesc) {
    if let Some(target_id) = desc.label_for {
        tree.set_label_for(nid, target_id);
    }
    if let Some(name) = desc.control_name {
        tree.set_control_name(nid, name);
    }
    match desc.button_kind {
        StaticButtonKind::Auto => {}
        StaticButtonKind::Button => tree.set_button_kind(nid, ButtonKind::Button),
        StaticButtonKind::Submit => tree.set_button_kind(nid, ButtonKind::Submit),
        StaticButtonKind::Reset => tree.set_button_kind(nid, ButtonKind::Reset),
    }
    for opt in desc.select_options {
        tree.add_select_option(nid, opt.value, opt.label, opt.selected);
    }
    match desc.input_kind {
        StaticInputKind::None => {}
        StaticInputKind::Checkbox => tree.set_checkbox_role(nid),
        StaticInputKind::Radio => tree.set_radio_role(nid),
        StaticInputKind::Range(min, max, step) => tree.set_range_bounds(nid, min, max, step),
        StaticInputKind::Text => tree.set_input_kind(nid, InputKind::Text),
        StaticInputKind::Password => tree.set_input_kind(nid, InputKind::Password),
        StaticInputKind::Number => tree.set_input_kind(nid, InputKind::Number),
        StaticInputKind::Email => tree.set_input_kind(nid, InputKind::Email),
        StaticInputKind::Search => tree.set_input_kind(nid, InputKind::Search),
    }
}

fn apply_static_text_and_media(tree: &mut ViewTree, nid: NodeId, desc: &StaticNodeDesc) {
    if let Some(text) = desc.static_text {
        tree.set_static_text(nid, text);
    }
    if let Some(ph) = desc.placeholder {
        tree.set_placeholder(nid, ph);
    }
    if let Some(label) = desc.static_aria_label_slot {
        tree.set_aria_label(nid, label);
    }
    if let Some(title) = desc.title {
        tree.set_title(nid, title);
    }
    if let Some(href) = desc.link_href {
        tree.set_link_href(nid, href);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_ir_mounts_nested_elements_and_anchors_in_preorder() {
        static ATTRS: [StaticAttrDesc; 1] = [StaticAttrDesc {
            name: "data-role",
            value: "card",
        }];
        static CLASSES: [&str; 2] = ["p-4", "card"];
        static NODES: [StaticNodeDesc; 4] = [
            StaticNodeDesc {
                id: Some("root-box"),
                classes: &CLASSES,
                attrs: &ATTRS,
                ..StaticNodeDesc::BLANK
            },
            StaticNodeDesc {
                parent_slot: Some(0),
                tag: "p",
                id: Some("child-p"),
                static_text: Some("hello"),
                ..StaticNodeDesc::BLANK
            },
            StaticNodeDesc {
                parent_slot: Some(0),
                ..StaticNodeDesc::ANCHOR
            },
            StaticNodeDesc {
                parent_slot: Some(0),
                tag: "button",
                id: Some("tail-btn"),
                is_button: true,
                disabled: Some(true),
                ..StaticNodeDesc::BLANK
            },
        ];
        static DOC: StaticDocumentDesc = StaticDocumentDesc {
            unstyled: true,
            stylesheets: &[],
            nodes: &NODES,
        };
        let mut tree = ViewTree::new();
        let ids = mount_static_document(&mut tree, None, &DOC, true);
        assert_eq!(ids.len(), 4);
        assert_eq!(tree.find_by_id("root-box"), Some(ids[0]));
        assert_eq!(tree.find_by_id("child-p"), Some(ids[1]));
        assert_eq!(tree.find_by_id("tail-btn"), Some(ids[3]));
    }
}
