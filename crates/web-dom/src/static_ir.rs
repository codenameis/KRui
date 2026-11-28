//! Compile-time static IR mounts (`mount_static_document`, `mount_static_nodes`, `mount_static_image_bytes`).

use crate::backend::DomNodeId;
use crate::tree::WebDomTree;
use crate::ua_css::build_ua_stylesheet;
pub use krui_ui_types::static_ir::{
    StaticAttrDesc, StaticButtonKind, StaticDocumentDesc, StaticInputKind, StaticNodeDesc,
    StaticNodeKind, StaticSelectOptDesc,
};
use std::rc::Rc;

/// Mounts a top-level compiled [`StaticDocumentDesc`] (UA CSS + stylesheets + pre-order nodes).
pub fn mount_static_document(
    tree: &mut WebDomTree,
    parent: Option<DomNodeId>,
    doc: &StaticDocumentDesc,
    is_top_mount: bool,
) -> Vec<DomNodeId> {
    if is_top_mount {
        let mut guard = tree.inner.borrow_mut();
        if !guard.ua_injected {
            let ua_css = build_ua_stylesheet(doc.unstyled);
            guard.backend.inject_stylesheet(&ua_css, true);
            guard.ua_injected = true;
        }
        let mut bundled = String::new();
        for &sheet in doc.stylesheets {
            if !guard.injected_sheets.iter().any(|s| s == sheet) {
                if !bundled.is_empty() {
                    bundled.push('\n');
                }
                bundled.push_str(sheet);
                guard.injected_sheets.push(sheet.to_string());
            }
        }
        if !bundled.is_empty() {
            guard.backend.inject_stylesheet(&bundled, false);
        }
    }
    mount_static_nodes(tree, parent, doc.nodes)
}

/// Mounts a pre-order slice of static [`StaticNodeDesc`] nodes under `parent`.
pub fn mount_static_nodes(
    tree: &mut WebDomTree,
    parent: Option<DomNodeId>,
    descs: &[StaticNodeDesc],
) -> Vec<DomNodeId> {
    let mut created: Vec<DomNodeId> = Vec::with_capacity(descs.len());
    let mut top_roots: Vec<DomNodeId> = Vec::new();
    let slots = { Rc::clone(&tree.inner.borrow().slots) };
    let mut pending_slot_mounts: Vec<(DomNodeId, String)> = Vec::new();

    for desc in descs {
        let node_parent = desc
            .parent_slot
            .and_then(|idx| created.get(idx).copied())
            .or(parent);
        let id = instantiate_static_node(tree, node_parent, desc, &mut pending_slot_mounts);
        created.push(id);
        if desc.parent_slot.is_none() {
            top_roots.push(id);
        }
    }

    for (nid, html_id) in pending_slot_mounts {
        crate::slots::notify_slot_mounted(&slots, nid, &html_id);
    }
    let _ = top_roots;
    created
}

fn instantiate_static_node(
    tree: &mut WebDomTree,
    parent: Option<DomNodeId>,
    desc: &StaticNodeDesc,
    pending_slots: &mut Vec<(DomNodeId, String)>,
) -> DomNodeId {
    if desc.kind == StaticNodeKind::ControlFlowAnchor {
        let id = tree.inner.borrow_mut().backend.create_comment("anchor");
        tree.record_created_node(id, parent, "#anchor", false, None);
        tree.inner.borrow_mut().backend.append_child(parent, id);
        return id;
    }
    if desc.is_synthetic_text_run {
        let text = desc.static_text.unwrap_or("");
        let id = tree.inner.borrow_mut().backend.create_text(text);
        tree.record_created_node(id, parent, "#text", false, None);
        tree.inner.borrow_mut().backend.append_child(parent, id);
        return id;
    }
    let is_svg = desc.tag == "svg";
    let is_value_control = desc.is_input || desc.tag == "textarea" || desc.tag == "select";
    let id = {
        let mut guard = tree.inner.borrow_mut();
        if is_svg {
            guard.backend.create_svg_element("svg")
        } else {
            guard.backend.create_element(desc.tag)
        }
    };
    tree.record_created_node(id, parent, desc.tag, is_value_control, desc.id);
    apply_static_node_properties(tree, id, desc, is_value_control);
    tree.inner.borrow_mut().backend.append_child(parent, id);
    if let Some(html_id) = desc.id {
        pending_slots.push((id, html_id.to_string()));
    }
    id
}

fn apply_static_node_properties(
    tree: &mut WebDomTree,
    id: DomNodeId,
    desc: &StaticNodeDesc,
    is_value_control: bool,
) {
    let mut guard = tree.inner.borrow_mut();
    let backend = guard.backend.as_mut();
    if let Some(html_id) = desc.id {
        backend.set_attr(id, "id", html_id);
    }
    if let Some(ct) = desc.custom_tag {
        backend.set_attr(id, "data-krui-tag", ct);
    }
    if !desc.classes.is_empty() {
        backend.set_classes(id, desc.classes);
    }
    for attr in desc.attrs {
        backend.set_attr(id, attr.name, attr.value);
    }
    if let Some(style_css) = desc.inline_style {
        backend.set_inline_style(id, style_css);
    }
    if let Some(aria) = desc.aria_label.or(desc.static_aria_label_slot) {
        backend.set_attr(id, "aria-label", aria);
    }
    if let Some(title) = desc.title {
        backend.set_attr(id, "title", title);
    }
    if let Some(ph) = desc.placeholder {
        backend.set_attr(id, "placeholder", ph);
    }
    if let Some(lf) = desc.label_for {
        backend.set_attr(id, "for", lf);
    }
    if let Some(cn) = desc.control_name {
        backend.set_attr(id, "name", cn);
    }
    if let Some(href) = desc.link_href {
        backend.set_attr(id, "href", href);
    }
    apply_static_control_properties(backend, id, desc, is_value_control);
}

fn apply_static_control_properties(
    backend: &mut dyn crate::backend::DomBackend,
    id: DomNodeId,
    desc: &StaticNodeDesc,
    is_value_control: bool,
) {
    if desc.is_button {
        let btn_type = match desc.button_kind {
            StaticButtonKind::Auto => None,
            StaticButtonKind::Button => Some("button"),
            StaticButtonKind::Submit => Some("submit"),
            StaticButtonKind::Reset => Some("reset"),
        };
        if let Some(t) = btn_type {
            backend.set_attr(id, "type", t);
        }
    }
    apply_static_input_kind(backend, id, desc.input_kind);
    if desc.is_secret_input {
        backend.set_attr(id, "type", "password");
    }
    if let Some(dis) = desc.disabled {
        backend.set_disabled(id, dis);
    }
    if let Some(req) = desc.required {
        if req {
            backend.set_attr(id, "required", "true");
        }
    }
    if let Some(chk) = desc.checked {
        backend.set_checked(id, chk);
    }
    if !desc.select_options.is_empty() {
        let opts: Vec<_> = desc
            .select_options
            .iter()
            .map(|o| (o.value.to_string(), o.label.to_string(), o.selected))
            .collect();
        backend.set_select_options(id, &opts);
    }
    if let Some(text) = desc.static_text {
        if is_value_control {
            backend.set_input_value(id, text);
        } else {
            backend.set_text(id, text);
        }
    }
}

fn apply_static_input_kind(
    backend: &mut dyn crate::backend::DomBackend,
    id: DomNodeId,
    kind: StaticInputKind,
) {
    match kind {
        StaticInputKind::None => {}
        StaticInputKind::Checkbox => backend.set_attr(id, "type", "checkbox"),
        StaticInputKind::Radio => backend.set_attr(id, "type", "radio"),
        StaticInputKind::Text => backend.set_attr(id, "type", "text"),
        StaticInputKind::Password => backend.set_attr(id, "type", "password"),
        StaticInputKind::Number => backend.set_attr(id, "type", "number"),
        StaticInputKind::Email => backend.set_attr(id, "type", "email"),
        StaticInputKind::Search => backend.set_attr(id, "type", "search"),
        StaticInputKind::Range(min, max, step) => {
            backend.set_attr(id, "type", "range");
            backend.set_attr(id, "min", &min.to_string());
            backend.set_attr(id, "max", &max.to_string());
            backend.set_attr(id, "step", &step.to_string());
        }
    }
}

/// Decodes MIME type heuristically and sets compiled-in image bytes onto an `<img>` element.
pub fn mount_static_image_bytes(tree: &mut WebDomTree, id: DomNodeId, bytes: &'static [u8]) {
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else {
        "application/octet-stream"
    };
    tree.inner
        .borrow_mut()
        .backend
        .set_image_bytes(id, mime, bytes);
}
