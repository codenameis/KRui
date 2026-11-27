//! Dev-preview (`#[cfg(feature = "preview")]`) structural-path node state preservation across IR reloads.

use crate::media::ImageSource;
use crate::view_tree::grapheme::TextEditSnapshot;
use crate::view_tree::media_nodes::MediaData;
use crate::view_tree::node::{sync_node_text_context, ElementNode, ImeState};
use crate::view_tree::sparse_tables::{ScrollData, TextEditData};
use crate::view_tree::{A11yId, NodeId, ViewTree, ViewTreeInner};
use krui_a11y::{AccessibilityEngine, SemanticRole};
use krui_layout::LayoutEngine;
use krui_signals::{Effect, Scope};
use std::collections::HashMap;

/// Snapshot of a single node's interactive state during live preview IR remounting.
#[derive(Clone, Debug)]
pub struct SavedPreviewNode {
    /// Preserved input/label text content.
    pub text: String,
    /// Preserved checkbox/radio `is_checked` state.
    pub is_checked: bool,
    /// Preserved input cursor character index.
    pub cursor_char_idx: usize,
    /// Preserved selection anchor character index, if any.
    pub selection_anchor_char_idx: Option<usize>,
    /// Preserved IME composition state.
    pub ime_state: ImeState,
    /// Preserved Undo snapshot stack (`Ctrl+Z`).
    pub undo_stack: Vec<TextEditSnapshot>,
    /// Preserved Redo snapshot stack (`Ctrl+Y` / `Ctrl+Shift+Z`).
    pub redo_stack: Vec<TextEditSnapshot>,
    /// Preserved preferred horizontal caret coordinate across vertical line moves.
    pub preferred_x: Option<f32>,
    /// Preserved horizontal input scroll offset in logical pixels.
    pub scroll_x: f32,
    /// Preserved scroll offset in logical pixels.
    pub scroll_y: f32,
    /// Whether this node held keyboard/accessibility focus prior to remounting.
    pub was_focused: bool,
    /// Whether this node was pinned as `ime_target` prior to remounting.
    pub was_ime_target: bool,
    /// Preserved host-mounted image or `ExternalTexture` slot across preview IR remounts.
    pub host_image_source: Option<ImageSource>,
}

/// Collection of saved nodes indexed by full structural path key.
#[derive(Default)]
pub struct PreviewSavedNodes {
    pub(crate) by_path: HashMap<String, SavedPreviewNode>,
    pub(crate) by_parent_id: HashMap<String, SavedPreviewNode>,
}

impl PreviewSavedNodes {
    /// Removes and returns a matching `SavedPreviewNode` by exact structural `path_key` first,
    /// then by `(parent_path, tag, element_id)` under the same parent container.
    pub fn take(&mut self, element_id: Option<&str>, path_key: &str) -> Option<SavedPreviewNode> {
        if let Some(saved) = self.by_path.remove(path_key) {
            if let Some(pk) = parent_id_key_from_path(path_key, element_id) {
                self.by_parent_id.remove(&pk);
            }
            return Some(saved);
        }
        if let Some(pk) = parent_id_key_from_path(path_key, element_id) {
            if let Some(saved) = self.by_parent_id.remove(&pk) {
                return Some(saved);
            }
        }
        None
    }
}

fn parent_id_key_from_path(path_key: &str, element_id: Option<&str>) -> Option<String> {
    let id = element_id?;
    let (parent_and_for, seg) = path_key.rsplit_once('/')?;
    let (_idx, tag_and_id) = seg.split_once(':')?;
    let tag = tag_and_id
        .split_once('#')
        .map(|(t, _)| t)
        .unwrap_or(tag_and_id);
    Some(format!("{parent_and_for}::{tag}#{id}"))
}

impl ViewTree {
    /// Sets the structural path key on `node_id` (`#[cfg(feature = "preview")]`).
    pub fn set_node_preview_path_key(&mut self, node_id: NodeId, path_key: impl Into<String>) {
        let mut inner = self.inner.borrow_mut();
        if let Some(node) = inner.node_mut(node_id) {
            node.preview_path_key = path_key.into();
        }
    }

    /// Returns the structural path key of `node_id`, if alive.
    pub fn node_preview_path_key(&self, node_id: NodeId) -> Option<String> {
        self.inner
            .borrow()
            .node(node_id)
            .map(|n| n.preview_path_key.clone())
    }

    /// Sets the vertical scroll offset (`scroll_y`) of `node_id` in logical pixels.
    pub fn set_node_scroll_y(&mut self, node_id: NodeId, scroll_y: f32) -> bool {
        let mut inner = self.inner.borrow_mut();
        let Some(role) = inner.node(node_id).map(|n| n.role) else {
            return false;
        };
        if role == SemanticRole::MultilineTextInput {
            let ed = inner.ensure_text_edit(node_id);
            ed.scroll_y = scroll_y.max(0.0);
            ed.needs_caret_scroll = false;
        } else {
            inner.ensure_scroll(node_id).scroll_y = scroll_y.max(0.0);
        }
        inner.paint_dirty = true;
        true
    }

    /// Stores the active root [`crate::registry::RegistryRef`] captured at mount time (`preview` feature only).
    pub fn set_preview_registry(&mut self, registry: crate::registry::RegistryRef) {
        self.inner.borrow_mut().preview_registry = Some(registry);
    }

    /// Returns the active root [`crate::registry::RegistryRef`] captured at mount time, if any.
    pub fn preview_registry(&self) -> Option<crate::registry::RegistryRef> {
        self.inner.borrow().preview_registry.clone()
    }

    /// Runs `f` inside the tree's active `root_scope`.
    pub fn run_in_root_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let scope = self.inner.borrow().root_scope;
        scope.run(|| f(self))
    }

    /// Extracts existing interactive node state, disposes the previous page `root_scope`, and allocates a fresh `root_scope`.
    pub fn take_nodes_for_preview_remount(&mut self) -> PreviewSavedNodes {
        let next_scope = Scope::new();
        let (old_scope, effs, mut scopes, saved) = {
            let mut inner = self.inner.borrow_mut();
            extract_remount_snapshot(&mut inner, next_scope)
        };
        old_scope.dispose();
        for eff in effs {
            eff.dispose();
        }
        for scope in scopes.drain(..) {
            scope.dispose();
        }
        saved
    }

    /// Restores saved interactive widget state onto `node_id` during a live preview IR reload.
    pub fn restore_saved_preview_node(&mut self, node_id: NodeId, saved: SavedPreviewNode) {
        let mut inner = self.inner.borrow_mut();
        let Some((role, is_disabled)) = inner.node(node_id).map(|n| (n.role, n.is_disabled()))
        else {
            return;
        };
        let is_focusable = role.is_focusable() && !is_disabled;
        let is_input = !is_disabled && role.is_text_editable();
        let restore_ime_target = saved.was_ime_target
            || !saved.ime_state.preedit.is_empty()
            || saved.ime_state.pending_blur_commit.is_some();
        if role.is_text_editable() {
            let char_len = saved.text.chars().count();
            if let Some(dst) = inner.node_mut(node_id) {
                dst.text = saved.text;
                dst.set_text_dirty(true);
            }
            let ed = inner.ensure_text_edit(node_id);
            ed.scroll_x = saved.scroll_x;
            ed.scroll_y = saved.scroll_y;
            ed.cursor_char_idx = saved.cursor_char_idx.min(char_len);
            ed.selection_anchor_char_idx = saved.selection_anchor_char_idx.map(|a| a.min(char_len));
            ed.ime_state = saved.ime_state;
            ed.undo_stack = saved.undo_stack;
            ed.redo_stack = saved.redo_stack;
            ed.preferred_x = saved.preferred_x;
            sync_node_text_context(&mut inner, node_id);
        } else if matches!(role, SemanticRole::Checkbox | SemanticRole::RadioButton) {
            if let Some(dst) = inner.node_mut(node_id) {
                dst.set_checked(saved.is_checked);
                if role == SemanticRole::Checkbox
                    && (dst.text.is_empty() || dst.text == "true" || dst.text == "false")
                {
                    dst.text = saved.is_checked.to_string();
                }
            }
        } else if saved.scroll_y > 0.0 || inner.sparse.scrolls.contains_key(&node_id) {
            inner.ensure_scroll(node_id).scroll_y = saved.scroll_y;
        }
        restore_saved_host_image(&mut inner, node_id, saved.host_image_source);
        if saved.was_focused && is_focusable {
            inner.a11y_engine.set_focus(Some(A11yId::from(node_id).0));
        }
        if restore_ime_target && is_input {
            inner.ime_target = Some(node_id);
        }
    }
}

/// Normalizes a runtime structural path key by replacing concrete `for[...]` item keys with `for[*]`.
pub fn normalize_preview_path_key(raw: &str) -> String {
    if !raw.contains("for[") {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(pos) = rest.find("for[") {
        out.push_str(&rest[..pos]);
        out.push_str("for[*]");
        let after_open = &rest[pos + 4..];
        if let Some(close) = after_open.find(']') {
            rest = &after_open[close + 1..];
        } else {
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}

fn restore_saved_host_image(
    inner: &mut crate::view_tree::ViewTreeInner,
    node_id: NodeId,
    host_image: Option<ImageSource>,
) {
    let Some(src) = host_image else {
        return;
    };
    let should_apply = matches!(src, ImageSource::External { .. })
        || inner
            .media(node_id)
            .and_then(|m| m.image_source.as_ref())
            .is_none();
    if should_apply {
        let media = inner.ensure_media(node_id);
        media.image_source = Some(src);
        media.host_mounted_image = true;
        if let Some(dst) = inner.node_mut(node_id) {
            dst.set_text_dirty(true);
        }
        sync_node_text_context(inner, node_id);
    }
}

fn collect_saved_preview_nodes(
    old_nodes: Vec<ElementNode>,
    old_edits: &HashMap<NodeId, TextEditData>,
    old_scrolls: &HashMap<NodeId, ScrollData>,
    old_media: &HashMap<NodeId, MediaData>,
    focused_nid: Option<NodeId>,
    old_ime_target: Option<NodeId>,
) -> PreviewSavedNodes {
    let mut by_path = HashMap::new();
    let mut by_parent_id = HashMap::new();
    for node in old_nodes
        .into_iter()
        .filter(|n| n.alive() && !n.is_anchor())
    {
        let ed = old_edits.get(&node.id);
        let ime_state = ed.map(|e| e.ime_state.clone()).unwrap_or_default();
        let was_focused = focused_nid == Some(node.id);
        let was_ime_target = old_ime_target == Some(node.id)
            || !ime_state.preedit.is_empty()
            || ime_state.pending_blur_commit.is_some();
        let scroll_y = if node.role == SemanticRole::MultilineTextInput {
            ed.map(|e| e.scroll_y).unwrap_or(0.0)
        } else {
            old_scrolls.get(&node.id).map(|s| s.scroll_y).unwrap_or(0.0)
        };
        let host_image_source = old_media.get(&node.id).and_then(|m| {
            let is_host = m.host_mounted_image
                || matches!(m.image_source, Some(ImageSource::External { .. }));
            is_host.then(|| m.image_source.clone()).flatten()
        });
        let is_checked = node.is_checked();
        let saved = SavedPreviewNode {
            text: node.text,
            is_checked,
            cursor_char_idx: ed.map(|e| e.cursor_char_idx).unwrap_or(0),
            selection_anchor_char_idx: ed.and_then(|e| e.selection_anchor_char_idx),
            ime_state,
            undo_stack: ed.map(|e| e.undo_stack.clone()).unwrap_or_default(),
            redo_stack: ed.map(|e| e.redo_stack.clone()).unwrap_or_default(),
            preferred_x: ed.and_then(|e| e.preferred_x),
            scroll_x: ed.map(|e| e.scroll_x).unwrap_or(0.0),
            scroll_y,
            was_focused,
            was_ime_target,
            host_image_source,
        };
        if !node.preview_path_key.is_empty() {
            if let Some(pk) =
                parent_id_key_from_path(&node.preview_path_key, node.element_id.as_deref())
            {
                by_parent_id.insert(pk, saved.clone());
            }
            by_path.insert(node.preview_path_key, saved);
        }
    }
    PreviewSavedNodes {
        by_path,
        by_parent_id,
    }
}

fn extract_remount_snapshot(
    inner: &mut ViewTreeInner,
    next_scope: Scope,
) -> (Scope, Vec<Effect>, Vec<Scope>, PreviewSavedNodes) {
    inner.touch.reset();
    let focused_nid = inner
        .a11y_engine
        .focused_id()
        .and_then(|ak| NodeId::try_from(ak).ok());
    let old_ime = inner.ime_target;
    let old_scope = std::mem::replace(&mut inner.root_scope, next_scope);
    let mut to_dispose = Vec::new();
    for idx in 0..inner.nodes.len() {
        if inner.nodes[idx].alive() {
            inner.generations[idx] = inner.generations[idx].wrapping_add(1);
            if let Some(tid) = inner.nodes[idx].text_stylo_node.take() {
                to_dispose.push(tid);
            }
            if let Some(sid) = inner.nodes[idx].stylo_node.take() {
                to_dispose.push(sid);
            }
        }
    }
    for sid in to_dispose {
        inner
            .stylo
            .apply_mutation(krui_stylo_bridge::StyloDomMutation::DisposeSubtree(sid));
    }
    inner.stylo_to_view.clear();
    inner.propagated_canvas_node = None;
    inner.keyframes.clear_all();
    inner.fixed_nodes.clear();
    inner.open_popovers.clear();
    inner.mount_batch_depth = 0;
    inner.pending_mount_selector_eval = false;
    inner.stylo_dirty = true;
    let old_nodes = std::mem::take(&mut inner.nodes);
    let old_edits = std::mem::take(&mut inner.sparse.text_edits);
    let old_scrolls = std::mem::take(&mut inner.sparse.scrolls);
    let mut old_media = std::mem::take(&mut inner.sparse.media);
    let (effs, mut scopes) = inner.sparse.clear_all_for_reset();
    inner.free_nodes.clear();
    inner.roots.clear();
    inner.overlay.select_popup = None;
    inner.layout_engine = LayoutEngine::new();
    inner.a11y_engine = AccessibilityEngine::new();
    inner.hovered_node = None;
    inner.pressed_node = None;
    inner.selecting_node = None;
    inner.scrollbar_drag = None;
    inner.ime_target = None;
    inner.interaction.reset();
    inner.paint_dirty = true;
    for m in old_media.values_mut() {
        if let Some(s) = m.load_scope.take() {
            scopes.push(s);
        }
    }
    let saved = collect_saved_preview_nodes(
        old_nodes,
        &old_edits,
        &old_scrolls,
        &old_media,
        focused_nid,
        old_ime,
    );
    (old_scope, effs, scopes, saved)
}
