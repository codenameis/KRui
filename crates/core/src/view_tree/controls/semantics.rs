//! Semantic HTML5 visibility & interactive behaviors (`<dialog>`, `::backdrop`, `[popover]`,
//! `popovertarget`, light-dismiss, `Escape`, `<details>`/`<summary>`, and `<label>` forwarding).

use crate::view_tree::controls::resolve_label_target_node;
use crate::view_tree::node::{sync_node_text_context, ElementNode};
use crate::view_tree::pointer_vars::set_node_attr_inner;
use crate::view_tree::select_popup::close_select_popup;
use crate::view_tree::sync::notify_selector_state_change;
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_style::{DisplayMode, LayoutStyle};

/// Returns `true` if `node_id` currently carries the HTML attribute `name` (case-insensitive).
pub(crate) fn node_has_attr(inner: &ViewTreeInner, node_id: NodeId, name: &str) -> bool {
    node_attr_value(inner, node_id, name).is_some()
}

/// Returns the trimmed string value of attribute `name` on `node_id`, if present.
pub(crate) fn node_attr_value<'a>(
    inner: &'a ViewTreeInner,
    node_id: NodeId,
    name: &str,
) -> Option<&'a str> {
    let entry = inner.sparse.pseudo_pointer.get(&node_id)?;
    entry
        .attrs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Returns `true` if `node_id` is an open modal `<dialog>` (`open` + `modal` / `show_modal_dialog`).
#[inline]
pub(crate) fn is_modal_dialog_open(inner: &ViewTreeInner, node_id: NodeId) -> bool {
    inner.node(node_id).is_some_and(|n| {
        n.alive()
            && !n.is_anchor()
            && n._tag == "dialog"
            && node_has_attr(inner, node_id, "open")
            && node_has_attr(inner, node_id, "modal")
    })
}

/// Returns `true` if `[popover]` node `node_id` is currently promoted in the open popover stack.
#[inline]
pub(crate) fn is_popover_open(inner: &ViewTreeInner, node_id: NodeId) -> bool {
    inner
        .sparse
        .pseudo_pointer
        .get(&node_id)
        .is_some_and(|e| e.popover_open)
}

/// Returns `true` when `node_id` is suppressed by HTML5 semantic state (`hidden`, closed `<dialog>`,
/// closed `[popover]`, `::backdrop` helper node, or body of closed `<details>`).
pub(crate) fn is_node_semantically_hidden(inner: &ViewTreeInner, node_id: NodeId) -> bool {
    let Some(n) = inner.node(node_id) else {
        return true;
    };
    if !n.alive() || n.is_anchor() {
        return false;
    }
    if inner.open_popovers.is_empty()
        && inner.sparse.pseudo_pointer.is_empty()
        && n._tag != "dialog"
        && n.parent
            .and_then(|p| inner.node(p))
            .is_none_or(|p| p._tag != "details")
    {
        return false;
    }
    let pe = inner.sparse.pseudo_pointer.get(&node_id);
    let mut is_pseudo = false;
    if let Some(entry) = pe {
        if entry.is_backdrop {
            return true;
        }
        is_pseudo = entry.is_pseudo;
        let mut has_popover = false;
        for (k, _) in &entry.attrs {
            if k.eq_ignore_ascii_case("hidden") {
                return true;
            }
            if k.eq_ignore_ascii_case("popover") {
                has_popover = true;
            }
        }
        if has_popover && !entry.popover_open {
            return true;
        }
    }
    if n._tag == "dialog"
        && !pe.is_some_and(|e| e.attrs.iter().any(|(k, _)| k.eq_ignore_ascii_case("open")))
    {
        return true;
    }
    if !is_pseudo && n._tag != "summary" {
        if let Some(pid) = n.parent {
            if inner.node(pid).is_some_and(|p| p._tag == "details")
                && !node_has_attr(inner, pid, "open")
            {
                return true;
            }
        }
    }
    false
}

fn parent_is_flex_or_grid_container(inner: &ViewTreeInner, node_id: NodeId) -> bool {
    let mut cur = inner.node(node_id).and_then(|n| n.parent);
    while let Some(pid) = cur {
        let Some(p) = inner.node(pid) else {
            return false;
        };
        if p.is_anchor() || p.style.layout.display == DisplayMode::Contents {
            cur = p.parent;
            continue;
        }
        return p.style.layout.is_flex_or_grid_container();
    }
    false
}

/// Applies `raw_layout` to `taffy_node`, forcing `DisplayMode::None` while semantically hidden
/// and blockifying inline-level items when out-of-flow or inside a Flex/Grid container.
pub(crate) fn set_effective_taffy_layout(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    taffy_node: taffy::NodeId,
    mut raw_layout: LayoutStyle,
) {
    if is_node_semantically_hidden(inner, node_id) {
        raw_layout.display = DisplayMode::None;
        inner
            .layout_engine
            .set_layout_override(taffy_node, raw_layout);
        return;
    }
    let uses_stylo = inner.node(node_id).is_some_and(|n| n.stylo_node.is_some());
    let has_explicit_override = inner.sparse.layout_slot_nodes.contains(&node_id)
        || inner.keyframes.has_node(node_id)
        || inner
            .sparse
            .active_layout_transitions
            .contains_key(&node_id);
    if uses_stylo && !has_explicit_override {
        inner.layout_engine.clear_layout_override(taffy_node);
        return;
    }
    if raw_layout.position.is_out_of_flow() || parent_is_flex_or_grid_container(inner, node_id) {
        raw_layout.blockify_in_place();
    }
    inner
        .layout_engine
        .set_layout_override(taffy_node, raw_layout);
}

/// Synchronizes Taffy visibility (`DisplayMode::None` vs authored `display`) for `node_id`, plus
/// non-`<summary>` children when `node_id` is `<details>`.
pub(crate) fn sync_semantic_visibility(inner: &mut ViewTreeInner, node_id: NodeId) {
    let Some((taffy_opt, raw_layout, tag, children)) = inner
        .node(node_id)
        .filter(|n| n.alive() && !n.is_anchor())
        .map(|n| (n.taffy_node, n.style.layout, n._tag, n.children.clone()))
    else {
        return;
    };
    if let Some(tn) = taffy_opt {
        set_effective_taffy_layout(inner, node_id, tn, raw_layout);
        sync_node_text_context(inner, node_id);
    }
    if tag == "details" {
        for cid in children {
            sync_details_child_recursive(inner, cid);
        }
    }
    inner.paint_dirty = true;
}

fn sync_details_child_recursive(inner: &mut ViewTreeInner, cid: NodeId) {
    let Some((is_anchor, tn_opt, raw_layout)) = inner
        .node(cid)
        .filter(|n| n.alive())
        .map(|n| (n.is_anchor(), n.taffy_node, n.style.layout))
    else {
        return;
    };
    if is_anchor {
        let mounted = inner
            .anchor(cid)
            .map(|a| a.mounted_nodes.clone())
            .unwrap_or_default();
        for mid in mounted {
            sync_details_child_recursive(inner, mid);
        }
        return;
    }
    if let Some(tn) = tn_opt {
        set_effective_taffy_layout(inner, cid, tn, raw_layout);
        sync_node_text_context(inner, cid);
    }
}

/// Finds the enclosing `<summary>` element (if `start_id` is inside one) and toggles `open` on its
/// parent `<details>`. Returns `true` if a `<details>` state changed.
pub(crate) fn try_toggle_details_from_click(inner: &mut ViewTreeInner, start_id: NodeId) -> bool {
    let mut cur = Some(start_id);
    let mut summary_id = None;
    for _ in 0..16 {
        let Some(nid) = cur else { break };
        let Some(n) = inner.node(nid) else { break };
        if n._tag == "summary" {
            summary_id = Some(nid);
            break;
        }
        if n._tag == "details" {
            break;
        }
        cur = n.parent;
    }
    let Some(sum_id) = summary_id else {
        return false;
    };
    let Some(details_id) = find_enclosing_details(inner, sum_id) else {
        return false;
    };
    let currently_open = node_has_attr(inner, details_id, "open");
    let next_val = (!currently_open).then(String::new);
    set_node_attr_inner(inner, details_id, "open", next_val);
    true
}

fn find_enclosing_details(inner: &ViewTreeInner, summary_id: NodeId) -> Option<NodeId> {
    let mut cur = inner.node(summary_id)?.parent;
    for _ in 0..16 {
        let cid = cur?;
        let n = inner.node(cid)?;
        if !n.is_anchor() {
            return (n._tag == "details").then_some(cid);
        }
        cur = n.parent;
    }
    None
}

/// Resolves a `popovertarget` element ancestor starting from `start_id` and returns
/// `(trigger_id, target_popover_id, action)`.
pub(crate) fn resolve_popover_trigger(
    inner: &ViewTreeInner,
    start_id: NodeId,
) -> Option<(NodeId, NodeId, &str)> {
    let mut cur = Some(start_id);
    for _ in 0..8 {
        let cid = cur?;
        if let Some(target_elem_id) = node_attr_value(inner, cid, "popovertarget") {
            let tid = inner
                .nodes
                .iter()
                .find(|n| {
                    n.alive()
                        && !n.is_anchor()
                        && n.element_id.as_deref() == Some(target_elem_id.trim())
                })
                .map(|n| n.id)?;
            if !node_has_attr(inner, tid, "popover") {
                return None;
            }
            let action = node_attr_value(inner, cid, "popovertargetaction").unwrap_or("toggle");
            return Some((cid, tid, action));
        }
        cur = inner.node(cid).and_then(|n| n.parent);
    }
    None
}

/// Handles clicking a `[popovertarget]` trigger or `<summary>` after activation.
pub(crate) fn handle_semantic_post_click(inner: &mut ViewTreeInner, activated_id: NodeId) -> bool {
    let mut changed = try_toggle_details_from_click(inner, activated_id);
    if let Some((_, pop_id, action)) = resolve_popover_trigger(inner, activated_id) {
        let mode = action.trim().to_ascii_lowercase();
        let next_open = match mode.as_str() {
            "show" => true,
            "hide" => false,
            _ => !is_popover_open(inner, pop_id),
        };
        changed |= set_popover_open_state(inner, pop_id, next_open);
    }
    changed
}

/// Opens or closes `[popover]` `pop_id`, updating Taffy visibility, `open_popovers`, and selectors.
pub(crate) fn set_popover_open_state(
    inner: &mut ViewTreeInner,
    pop_id: NodeId,
    open: bool,
) -> bool {
    let was_open = is_popover_open(inner, pop_id);
    if was_open == open {
        return false;
    }
    if open {
        inner.open_popovers.retain(|&id| id != pop_id);
        inner.open_popovers.push(pop_id);
        inner.ensure_pseudo_pointer(pop_id).popover_open = true;
    } else {
        inner.open_popovers.retain(|&id| id != pop_id);
        if let Some(e) = inner.sparse.pseudo_pointer.get_mut(&pop_id) {
            e.popover_open = false;
        }
    }
    sync_semantic_visibility(inner, pop_id);
    notify_selector_state_change(inner, pop_id);
    inner.paint_dirty = true;
    true
}

/// Light-dismisses open non-manual `[popover]` elements when clicking outside them and their trigger.
pub(crate) fn dismiss_light_popovers_on_pointer_down(
    inner: &mut ViewTreeInner,
    hit: Option<NodeId>,
) -> bool {
    if inner.open_popovers.is_empty() {
        return false;
    }
    let trigger_target =
        hit.and_then(|nid| resolve_popover_trigger(inner, nid).map(|(_, tid, _)| tid));
    let to_close: Vec<NodeId> = inner
        .open_popovers
        .iter()
        .copied()
        .filter(|&pid| {
            let is_manual = node_attr_value(inner, pid, "popover")
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("manual"));
            let inside_pop = hit.is_some_and(|hid| hid == pid || is_descendant_of(inner, hid, pid));
            !is_manual && !inside_pop && trigger_target != Some(pid)
        })
        .collect();
    let mut changed = false;
    for pid in to_close {
        changed |= set_popover_open_state(inner, pid, false);
    }
    changed
}

fn is_descendant_of(inner: &ViewTreeInner, mut node: NodeId, ancestor: NodeId) -> bool {
    for _ in 0..64 {
        let Some(pid) = inner.node(node).and_then(|n| n.parent) else {
            return false;
        };
        if pid == ancestor {
            return true;
        }
        node = pid;
    }
    false
}

/// Handles `Escape` on Top-Layer overlays before falling through to normal focus clearing:
/// 1) `<select>` popup, 2) topmost auto `[popover]`, 3) topmost open `<dialog open>`.
pub(crate) fn handle_escape_top_layer(inner: &mut ViewTreeInner) -> bool {
    if close_select_popup(inner) {
        return true;
    }
    let pop_to_close = inner.open_popovers.iter().rev().copied().find(|&pid| {
        node_attr_value(inner, pid, "popover")
            .is_none_or(|v| !v.trim().eq_ignore_ascii_case("manual"))
    });
    if let Some(pid) = pop_to_close {
        return set_popover_open_state(inner, pid, false);
    }
    let open_dlg = inner
        .nodes
        .iter()
        .rev()
        .find(|n| n.style.layout.display != DisplayMode::None && is_modal_dialog_open(inner, n.id))
        .map(|n| n.id);
    if let Some(dlg_id) = open_dlg {
        set_node_attr_inner(inner, dlg_id, "modal", None);
        set_node_attr_inner(inner, dlg_id, "open", None);
        return true;
    }
    false
}

/// Returns both a `<label>` node and its associated control target when `nid_opt` is a `<label>`.
pub(crate) fn collect_press_notification_pair(
    inner: &ViewTreeInner,
    nid_opt: Option<NodeId>,
) -> [Option<NodeId>; 2] {
    let target = nid_opt.and_then(|id| resolve_label_target_node(inner, id));
    [nid_opt, target.filter(|&tid| Some(tid) != nid_opt)]
}

/// Extracts `on_click` callback from a `<label>`'s forwarded control target when different from `pressed`.
pub(crate) fn take_forwarded_label_click(
    inner: &ViewTreeInner,
    pressed: NodeId,
) -> Option<std::rc::Rc<dyn Fn()>> {
    let tid = resolve_label_target_node(inner, pressed)?;
    if tid == pressed || inner.node(tid).is_some_and(ElementNode::is_disabled) {
        return None;
    }
    inner.listeners(tid)?.on_click.clone()
}

impl ViewTree {
    /// Opens `<dialog>` `dialog_id` in non-modal normal flow (`dialog.show()`).
    pub fn show_dialog(&mut self, dialog_id: NodeId) -> bool {
        let mut inner = self.inner.borrow_mut();
        if !inner.node(dialog_id).is_some_and(|n| n._tag == "dialog") {
            return false;
        }
        set_node_attr_inner(&mut inner, dialog_id, "modal", None);
        set_node_attr_inner(&mut inner, dialog_id, "open", Some(String::new()));
        true
    }

    /// Opens `<dialog>` `dialog_id` in the modal HTML5 Top-Layer (`dialog.showModal()`).
    pub fn show_modal_dialog(&mut self, dialog_id: NodeId) -> bool {
        let mut inner = self.inner.borrow_mut();
        if !inner.node(dialog_id).is_some_and(|n| n._tag == "dialog") {
            return false;
        }
        set_node_attr_inner(&mut inner, dialog_id, "modal", Some(String::new()));
        set_node_attr_inner(&mut inner, dialog_id, "open", Some(String::new()));
        true
    }

    /// Closes an open modal or non-modal `<dialog>` (`dialog.close()`).
    pub fn close_dialog(&mut self, dialog_id: NodeId) -> bool {
        let mut inner = self.inner.borrow_mut();
        if !inner.node(dialog_id).is_some_and(|n| n._tag == "dialog") {
            return false;
        }
        set_node_attr_inner(&mut inner, dialog_id, "modal", None);
        set_node_attr_inner(&mut inner, dialog_id, "open", None);
        true
    }

    /// Dispatches an `Escape` key press (dismissing `<select>`, `[popover]`, or modal `<dialog>`,
    /// or clearing focus if no Top-Layer overlay is active).
    pub fn dispatch_key_escape(&mut self) -> bool {
        self.clear_focus()
    }
}
