//! # `krui-preview`
//!
//! Runtime `CompiledDocument` IR interpreter, `TemplateExpr` evaluator, structural-path state
//! preservation, and live `.krui`/`.css` file watcher (`preview` feature only).
//!
//! Shares the exact same `CompiledDocument` IR as `krui-macros` so preview rendering and
//! release macro codegen produce identical `ViewTree` nodes and `PaintScene` primitives.

#![forbid(unsafe_code)]

/// Device bezel frame wrapper for interactive mobile/tablet preview viewports.
pub mod device_frame;
/// Runtime expression evaluator and `<for>` scope registry for compiled `.krui` templates.
pub mod eval;
/// Run-length encoded framebuffer delta serializer for preview streaming.
pub mod frame_rle;
mod media_slots;
/// Interactive inspector overlay rendering inside the preview canvas.
pub mod overlay;
mod overlay_draw;
/// VS Code preview panel JSON/binary protocol helpers.
pub mod panel_wire;
/// Preflight source validation before compiling live preview documents.
pub mod preflight;
/// Hot-reload TCP frame codecs and client/server message definitions.
pub mod protocol;
mod pseudo_mount;
mod select_slots;
mod slots;
/// Stylesheet diff detection for in-place CSS hot-patching.
pub mod style_patch;
/// File-system watcher for live `.krui` and `.css` reload loops.
pub mod watcher;
mod watcher_fs;

pub use eval::{
    eval_interpolated_parts, eval_template_expr, eval_truthy, resolve_signal_from_expr,
    try_eval_bool, try_eval_template_expr, validate_template_expr, EvalError, ForItemScopeRegistry,
};
pub use frame_rle::{decode_rle_rgba_frame, encode_rle_rgba_frame, FRAME_RLE_MAGIC};
pub use krui_compiler::{
    compile_krui_source, compile_krui_source_from_path, compile_krui_source_from_path_with_options,
    compile_krui_source_strict, compile_krui_source_with_options,
    compile_krui_source_with_virtual_buffers, decode_compiled_document, encode_compiled_document,
    encode_compiled_document_with_version, CompileError, CompileOptions, CompileWarning,
    CompileWarningKind, CompiledDocument, CompiledInputType, IrCodecError, VersionedIrBundle,
    VirtualBufferMap, IR_FORMAT_VERSION, IR_WIRE_MAGIC,
};
pub use overlay::{decorate_preview_paint_scene, PreviewTcpClient};
pub use panel_wire::{PanelFrameState, PanelInputEvent, KRUI_EMBED_PANEL_ENV};
pub use preflight::validate_compiled_document_against_registry;
pub use protocol::{
    encode_length_prefixed_vec, write_length_prefixed, DeviceFramePreset, FramedTcpConnection,
    InspectedNodeWire, PreviewCommand, PreviewOverlayFlags, PreviewOverlayKind, PreviewResponse,
    PreviewTcpServer, DEFAULT_PREVIEW_PORT, KRUI_DEVICE_FRAME_ENV, KRUI_SERVE_ADDR_ENV,
    PREVIEW_PROTOCOL_VERSION,
};
pub use style_patch::{can_hot_patch_styles_only, try_hot_patch_compiled_styles_in_place};
pub use watcher::{format_structural_key, resolve_krui_path, DevFileWatcher};

use krui_compiler::{
    CompiledFor, CompiledIf, CompiledNode, CompiledNodeKind, HtmlTag, TemplateExpr,
};
use krui_core::{
    BranchMountFn, NodeId, PreviewSavedNodes, RegistryRef, ScopeRegistry, Value, ViewTree,
};
use krui_style::Color;
use select_slots::eval_with_temp_item;
use slots::{bind_event_slots, bind_paint_and_layout_slots, bind_text_and_class_slots};
use std::cell::RefCell;
use std::rc::Rc;

/// Structural identity of a mounted node across live preview reloads:
/// `(parent_path, tag, element_id, for_key, sibling_index)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuralPathKey<'a> {
    /// Structural path of the parent element/branch (`"root"` at the top level).
    pub parent_path: &'a str,
    /// HTML tag name (`"div"`, `"input"`, `"button"`, `"#if"`, `"#for"`).
    pub tag: &'a str,
    /// Optional HTML `id="..."` attribute.
    pub element_id: Option<&'a str>,
    /// Optional `<for key="{...}">` item key when inside a keyed loop.
    pub for_key: Option<&'a str>,
    /// 0-based index among siblings under the same parent/branch.
    pub sibling_index: usize,
}

impl<'a> StructuralPathKey<'a> {
    /// Formats this structural identity into a canonical path string.
    pub fn to_path_string(&self) -> String {
        let mut out = String::with_capacity(self.parent_path.len() + 24);
        out.push_str(self.parent_path);
        out.push('/');
        if let Some(fk) = self.for_key {
            out.push_str("for[");
            out.push_str(fk);
            out.push_str("]/");
        }
        out.push_str(&self.sibling_index.to_string());
        out.push(':');
        out.push_str(self.tag);
        if let Some(id) = self.element_id {
            out.push('#');
            out.push_str(id);
        }
        out
    }
}

/// Patches a node's text literal in-place during a dev preview session.
pub fn hot_patch_text_literal(tree: &mut ViewTree, node_id: NodeId, new_literal: &str) {
    if !cfg!(debug_assertions) {
        return;
    }
    tree.set_static_text(node_id, new_literal);
}

/// Patches a node's background color in-place during a dev preview session.
pub fn hot_patch_background_color(tree: &mut ViewTree, node_id: NodeId, new_color: Color) {
    if !cfg!(debug_assertions) {
        return;
    }
    let css = format!(
        "background-color: rgba({}, {}, {}, {:.6});",
        new_color.r,
        new_color.g,
        new_color.b,
        new_color.a as f32 / 255.0
    );
    tree.set_inline_style(node_id, &css);
}

/// Mounts a `CompiledDocument` IR onto `tree` with an empty `ScopeRegistry`.
pub fn mount_compiled_document(tree: &mut ViewTree, doc: &CompiledDocument) -> Vec<NodeId> {
    mount_compiled_document_with_registry(tree, doc, Rc::new(ScopeRegistry::new()))
}

/// Mounts a `CompiledDocument` IR onto `tree`, evaluating all `{...}` template expressions,
/// `<if>`/`<else-if>`/`<else>` branches, and `<for each="{item in list}" key="{item.id}">` loops
/// against `registry` inside `tree`'s `root_scope`.
pub fn mount_compiled_document_with_registry(
    tree: &mut ViewTree,
    doc: &CompiledDocument,
    registry: RegistryRef,
) -> Vec<NodeId> {
    tree.set_preview_registry(registry.clone());
    let base_dir = doc.source_path.as_deref().and_then(|p| p.parent());
    media_slots::with_preview_doc_base_dir(base_dir, || {
        tree.run_in_root_scope(|tree| {
            style_patch::apply_document_styles(tree, doc, false);
            tree.begin_mount_batch();
            let mut roots = Vec::with_capacity(doc.roots.len());
            for (idx, root) in doc.roots.iter().enumerate() {
                roots.push(mount_node_with_ctx(
                    tree, None, root, &registry, "root", None, idx, None,
                ));
            }
            tree.end_mount_batch();
            roots
        })
    })
}

/// Atomically validates all bindings and branches in `doc` against `registry`, and **only if
/// validation succeeds** tears down the previous `ViewTree` scope and remounts `doc` while
/// preserving interactive `<input>` state (`text`, caret, selection, IME, focus) and scroll offsets
/// by [`StructuralPathKey`].
///
/// If validation fails, returns `Err(CompileError)` with the previous `ViewTree`, `root_scope`,
/// and reactive `Effect`s completely untouched.
pub fn try_reload_compiled_document_with_registry(
    tree: &mut ViewTree,
    doc: &CompiledDocument,
    registry: RegistryRef,
) -> Result<usize, CompileError> {
    if !cfg!(debug_assertions) {
        return Ok(tree.node_count());
    }
    validate_compiled_document_against_registry(doc, registry.as_ref())?;
    Ok(reload_compiled_document_unchecked(tree, doc, registry))
}

/// Reloads a `CompiledDocument` IR into `tree`, disposing the old page `Scope` and all previous
/// `Effect`s while preserving interactive `<input>` state (`text`, caret, selection, IME, focus)
/// and scroll offsets by [`StructuralPathKey`].
///
/// If `doc` fails preflight validation against `registry`, leaves `tree` untouched and returns
/// the existing `tree.node_count()`.
pub fn reload_compiled_document_with_registry(
    tree: &mut ViewTree,
    doc: &CompiledDocument,
    registry: RegistryRef,
) -> usize {
    match try_reload_compiled_document_with_registry(tree, doc, registry) {
        Ok(count) => count,
        Err(_) => tree.node_count(),
    }
}

fn reload_compiled_document_unchecked(
    tree: &mut ViewTree,
    doc: &CompiledDocument,
    registry: RegistryRef,
) -> usize {
    tree.set_preview_registry(registry.clone());
    let saved_rc = Rc::new(RefCell::new(tree.take_nodes_for_preview_remount()));
    let base_dir = doc.source_path.as_deref().and_then(|p| p.parent());
    media_slots::with_preview_doc_base_dir(base_dir, || {
        tree.run_in_root_scope(|tree| {
            style_patch::apply_document_styles(tree, doc, true);
            tree.begin_mount_batch();
            for (idx, root) in doc.roots.iter().enumerate() {
                mount_node_with_ctx(
                    tree,
                    None,
                    root,
                    &registry,
                    "root",
                    None,
                    idx,
                    Some(&saved_rc),
                );
            }
            tree.end_mount_batch();
        });
    });
    tree.node_count()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn mount_node_with_ctx(
    tree: &mut ViewTree,
    parent: Option<NodeId>,
    node: &CompiledNode,
    registry: &RegistryRef,
    parent_path: &str,
    for_key: Option<&str>,
    sibling_index: usize,
    saved: Option<&Rc<RefCell<PreviewSavedNodes>>>,
) -> NodeId {
    match &node.kind {
        CompiledNodeKind::If(cif) => {
            let path = StructuralPathKey {
                parent_path,
                tag: "#if",
                element_id: None,
                for_key,
                sibling_index,
            }
            .to_path_string();
            mount_if_node(tree, parent, cif, registry, &path, saved)
        }
        CompiledNodeKind::For(cfor) => {
            let path = StructuralPathKey {
                parent_path,
                tag: "#for",
                element_id: None,
                for_key,
                sibling_index,
            }
            .to_path_string();
            mount_for_node(tree, parent, cfor, registry, &path, saved)
        }
        CompiledNodeKind::Element => pseudo_mount::mount_element_node(
            tree,
            parent,
            node,
            registry,
            parent_path,
            for_key,
            sibling_index,
            saved,
        ),
    }
}

fn mount_if_node(
    tree: &mut ViewTree,
    parent: Option<NodeId>,
    cif: &CompiledIf,
    registry: &RegistryRef,
    if_path: &str,
    saved: Option<&Rc<RefCell<PreviewSavedNodes>>>,
) -> NodeId {
    let conditions: Vec<Option<TemplateExpr>> =
        cif.branches.iter().map(|b| b.condition.clone()).collect();
    let reg_for_cond = registry.clone();
    let branch_selector = move || -> Option<usize> {
        for (idx, cond_opt) in conditions.iter().enumerate() {
            match cond_opt {
                Some(expr) => {
                    if eval_truthy(&eval_template_expr(expr, reg_for_cond.as_ref())) {
                        return Some(idx);
                    }
                }
                None => return Some(idx),
            }
        }
        None
    };

    let doc_base = media_slots::current_preview_doc_base_dir();
    let mut branch_mounters: Vec<BranchMountFn> = Vec::with_capacity(cif.branches.len());
    for (br_idx, br) in cif.branches.iter().enumerate() {
        let children = krui_compiler::ir::clone_compiled_nodes_stack_safe(&br.children);
        let reg_br = registry.clone();
        let br_path = format!("{if_path}/branch[{br_idx}]");
        let saved_br = saved.cloned();
        let br_base = doc_base.clone();
        branch_mounters.push(Rc::new(
            move |tree: &mut ViewTree, parent: Option<NodeId>| {
                media_slots::with_preview_doc_base_dir(br_base.as_deref(), || {
                    let mut ids = Vec::with_capacity(children.len());
                    for (ch_idx, ch) in children.iter().enumerate() {
                        ids.push(mount_node_with_ctx(
                            tree,
                            parent,
                            ch,
                            &reg_br,
                            &br_path,
                            None,
                            ch_idx,
                            saved_br.as_ref(),
                        ));
                    }
                    ids
                })
            },
        ));
    }

    tree.mount_if(parent, branch_selector, branch_mounters)
}

fn mount_for_node(
    tree: &mut ViewTree,
    parent: Option<NodeId>,
    cfor: &CompiledFor,
    registry: &RegistryRef,
    for_path: &str,
    saved: Option<&Rc<RefCell<PreviewSavedNodes>>>,
) -> NodeId {
    let list_expr = cfor.list.clone();
    let reg_list = registry.clone();
    let items_fn = move || -> Vec<Value> {
        match eval_template_expr(&list_expr, reg_list.as_ref()) {
            Value::List(items) => items,
            _ => Vec::new(),
        }
    };

    let key_expr = cfor.key.clone();
    let item_var_key = cfor.item_var.clone();
    let reg_key = registry.clone();
    let key_fn = move |item_val: &Value| -> String {
        eval_with_temp_item(&key_expr, reg_key.as_ref(), &item_var_key, item_val).to_string()
    };

    let doc_base = media_slots::current_preview_doc_base_dir();
    let item_mounter = build_for_item_mounter(cfor, registry, for_path, saved, doc_base.clone());
    let empty_mounter =
        build_for_empty_mounter(&cfor.empty_body, registry, for_path, saved, doc_base);
    tree.mount_for(parent, items_fn, key_fn, item_mounter, empty_mounter)
}

type ForItemMountFn =
    Box<dyn Fn(&mut ViewTree, Option<NodeId>, krui_signals::Signal<Value>) -> Vec<NodeId>>;

fn build_for_item_mounter(
    cfor: &CompiledFor,
    registry: &RegistryRef,
    for_path: &str,
    saved: Option<&Rc<RefCell<PreviewSavedNodes>>>,
    item_base: Option<std::path::PathBuf>,
) -> ForItemMountFn {
    let body_nodes = krui_compiler::ir::clone_compiled_nodes_stack_safe(&cfor.body);
    let item_var_body = cfor.item_var.clone();
    let key_expr_body = cfor.key.clone();
    let reg_body = registry.clone();
    let for_path_body = for_path.to_string();
    let saved_body = saved.cloned();
    Box::new(
        move |tree: &mut ViewTree,
              parent: Option<NodeId>,
              item_sig: krui_signals::Signal<Value>| {
            media_slots::with_preview_doc_base_dir(item_base.as_deref(), || {
                let item_reg: RegistryRef = std::rc::Rc::new(ForItemScopeRegistry::new(
                    reg_body.clone(),
                    item_var_body.clone(),
                    item_sig,
                ));
                let k_str = eval_template_expr(&key_expr_body, item_reg.as_ref()).to_string();
                let mut ids = Vec::with_capacity(body_nodes.len());
                for (ch_idx, ch) in body_nodes.iter().enumerate() {
                    ids.push(mount_node_with_ctx(
                        tree,
                        parent,
                        ch,
                        &item_reg,
                        &for_path_body,
                        Some(&k_str),
                        ch_idx,
                        saved_body.as_ref(),
                    ));
                }
                ids
            })
        },
    )
}

fn build_for_empty_mounter(
    empty_body: &[CompiledNode],
    registry: &RegistryRef,
    for_path: &str,
    saved: Option<&Rc<RefCell<PreviewSavedNodes>>>,
    doc_base: Option<std::path::PathBuf>,
) -> Option<BranchMountFn> {
    if empty_body.is_empty() {
        return None;
    }
    let empty_children = krui_compiler::ir::clone_compiled_nodes_stack_safe(empty_body);
    let reg_empty = registry.clone();
    let empty_path = format!("{for_path}/empty");
    let saved_empty = saved.cloned();
    Some(Rc::new(
        move |tree: &mut ViewTree, parent: Option<NodeId>| {
            media_slots::with_preview_doc_base_dir(doc_base.as_deref(), || {
                let mut ids = Vec::with_capacity(empty_children.len());
                for (ch_idx, ch) in empty_children.iter().enumerate() {
                    ids.push(mount_node_with_ctx(
                        tree,
                        parent,
                        ch,
                        &reg_empty,
                        &empty_path,
                        None,
                        ch_idx,
                        saved_empty.as_ref(),
                    ));
                }
                ids
            })
        },
    ))
}

#[cfg_attr(debug_assertions, inline(never))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_and_bind_element_node(
    tree: &mut ViewTree,
    parent: Option<NodeId>,
    node: &CompiledNode,
    registry: &RegistryRef,
    parent_path: &str,
    for_key: Option<&str>,
    sibling_index: usize,
    saved: Option<&Rc<RefCell<PreviewSavedNodes>>>,
) -> (NodeId, String) {
    let tag_str = node.tag.as_str();
    let path_str = StructuralPathKey {
        parent_path,
        tag: tag_str,
        element_id: node.id.as_deref(),
        for_key,
        sibling_index,
    }
    .to_path_string();

    let node_id = tree.create_element_node(
        parent,
        tag_str,
        node.id.clone(),
        node.aria_label.clone(),
        node.tag == HtmlTag::Button,
        node.tag == HtmlTag::Input,
        node.tag == HtmlTag::Canvas,
        node.is_secret_input,
    );
    tree.set_node_preview_path_key(node_id, path_str.clone());
    tree.set_node_source_span(node_id, node.source_line, node.source_column);
    bind_text_and_class_slots(tree, node_id, node, registry);
    bind_paint_and_layout_slots(tree, node_id, node, registry);
    bind_event_slots(tree, node_id, node, registry);
    pseudo_mount::bind_custom_tag_and_attrs(tree, node_id, node, registry);
    if let Some(saved_node) = saved.and_then(|s| s.borrow_mut().take(node.id.as_deref(), &path_str))
    {
        tree.restore_saved_preview_node(node_id, saved_node);
    }
    (node_id, path_str)
}
