//! Focus transition callbacks (`auto-commit -> onchange -> onblur -> onfocus`),
//! `FocusOrigin` tracking, enabled tab-order collection, and disabled-node focus cleanup.

use crate::view_tree::events::{mark_focus_entered, take_pending_blur_change, BlurChangeCallback};
use crate::view_tree::node::FocusOrigin;
use crate::view_tree::select_popup::close_select_popup;
use crate::view_tree::text_edit::{auto_commit_ime_on_blur, InputCallbackPair};
use crate::view_tree::{A11yId, NodeId, ViewTreeInner};
use krui_signals::batch;
use krui_style::DisplayMode;
use std::rc::Rc;

/// Deferred callbacks produced by a focus transition (`old_focus -> new_focus`).
#[derive(Default)]
pub(crate) struct FocusTransitionCallbacks {
    pub(crate) ime_blur_input: Option<InputCallbackPair>,
    pub(crate) blur_change: Option<BlurChangeCallback>,
    pub(crate) on_blur: Option<Rc<dyn Fn()>>,
    pub(crate) on_focus: Option<Rc<dyn Fn()>>,
}

impl FocusTransitionCallbacks {
    /// Invokes pending transition callbacks in `on_input(preedit) -> on_change -> on_blur -> on_focus` order.
    pub(crate) fn invoke(self) {
        if let Some((Some(on_input), val)) = self.ime_blur_input {
            batch(|| on_input(val));
        }
        if let Some((cb, val)) = self.blur_change {
            batch(|| cb(val));
        }
        if let Some(blur_cb) = self.on_blur {
            batch(|| blur_cb());
        }
        if let Some(focus_cb) = self.on_focus {
            batch(|| focus_cb());
        }
    }
}

/// Transitions keyboard/accessibility focus to `new_focus` using programmatic focus origin.
pub(crate) fn transition_focus(
    inner: &mut ViewTreeInner,
    new_focus: Option<NodeId>,
) -> FocusTransitionCallbacks {
    transition_focus_with_origin(inner, new_focus, FocusOrigin::Programmatic)
}

/// Transitions keyboard/accessibility focus with explicit `origin` (`Pointer`, `Keyboard`, `Programmatic`).
pub(crate) fn transition_focus_with_origin(
    inner: &mut ViewTreeInner,
    new_focus: Option<NodeId>,
    origin: FocusOrigin,
) -> FocusTransitionCallbacks {
    let old_focus = inner
        .a11y_engine
        .focused_id()
        .and_then(|id| NodeId::try_from(id).ok());
    if old_focus != new_focus
        && inner
            .overlay
            .select_popup
            .as_ref()
            .is_some_and(|p| Some(p.select_id) != new_focus)
    {
        close_select_popup(inner);
    }
    inner
        .a11y_engine
        .set_focus(new_focus.map(|id| A11yId::from(id).0));
    inner.sync_focus_origin_and_selection(new_focus, origin);
    let ime_blur_input = auto_commit_ime_on_blur(inner, old_focus, new_focus);
    let blur_change = take_pending_blur_change(inner, old_focus, new_focus);
    let (on_blur, on_focus) = collect_focus_listeners(inner, old_focus, new_focus);
    if old_focus != new_focus {
        if let Some(old_id) = old_focus {
            crate::view_tree::transitions::commit_node_transitions_from_style(inner, old_id);
            crate::view_tree::sync::notify_selector_state_change(inner, old_id);
        }
        if let Some(new_id) = new_focus {
            crate::view_tree::transitions::commit_node_transitions_from_style(inner, new_id);
            crate::view_tree::sync::notify_selector_state_change(inner, new_id);
        }
    }
    FocusTransitionCallbacks {
        ime_blur_input,
        blur_change,
        on_blur,
        on_focus,
    }
}

type FocusListenerPair = (Option<Rc<dyn Fn()>>, Option<Rc<dyn Fn()>>);

fn collect_focus_listeners(
    inner: &mut ViewTreeInner,
    old_focus: Option<NodeId>,
    new_focus: Option<NodeId>,
) -> FocusListenerPair {
    if old_focus == new_focus {
        return (None, None);
    }
    mark_focus_entered(inner, new_focus);
    let blur_cb = old_focus
        .filter(|&id| inner.node(id).is_some())
        .and_then(|id| inner.listeners(id)?.on_blur.clone());
    let focus_cb = new_focus
        .filter(|&id| inner.node(id).is_some())
        .and_then(|id| inner.listeners(id)?.on_focus.clone());
    (blur_cb, focus_cb)
}

/// Collects live, visible, non-disabled focusable node IDs (`A11yId`) in pre-order document traversal.
pub(crate) fn collect_enabled_focusable_order(inner: &ViewTreeInner) -> Vec<u64> {
    let mut order = Vec::new();
    for &root_id in &inner.roots {
        collect_focusable_subtree(inner, root_id, &mut order);
    }
    order
}

fn collect_focusable_subtree(inner: &ViewTreeInner, node_id: NodeId, out: &mut Vec<u64>) {
    let Some(n) = inner.node(node_id) else {
        return;
    };
    if n.is_anchor()
        || n.style.layout.display == DisplayMode::None
        || (n.is_pointer_events_none() && n.style.paint.opacity <= 0.0)
    {
        return;
    }
    if !n.is_visibility_hidden() && !n.is_disabled() && n.role.is_focusable() {
        out.push(A11yId::from(node_id).0);
    }
    for &child_id in &n.children {
        collect_focusable_subtree(inner, child_id, out);
    }
}

/// Applies a static or reactive `disabled` state change to `node_id`.
pub(crate) fn apply_disabled_state_change(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    disabled: bool,
) -> Option<FocusTransitionCallbacks> {
    let changed = inner.node_mut(node_id).is_some_and(|node| {
        if node.is_disabled() != disabled {
            node.set_disabled(disabled);
            true
        } else {
            false
        }
    });
    if !changed {
        return None;
    }
    inner.paint_dirty = true;
    crate::view_tree::sync::notify_selector_state_change(inner, node_id);
    if !disabled {
        return None;
    }
    if let Some(ed) = inner.sparse.text_edits.get_mut(&node_id) {
        ed.selection_anchor_char_idx = None;
    }
    if inner
        .overlay
        .select_popup
        .as_ref()
        .is_some_and(|p| p.select_id == node_id)
    {
        close_select_popup(inner);
    }
    if inner.hovered_node == Some(node_id) {
        inner.hovered_node = None;
    }
    if inner.pressed_node == Some(node_id) {
        inner.pressed_node = None;
    }
    if inner.selecting_node == Some(node_id) {
        inner.selecting_node = None;
    }
    inner.interaction.release_node(node_id);
    if inner.a11y_engine.focused_id() == Some(A11yId::from(node_id).0) {
        Some(transition_focus(inner, None))
    } else {
        None
    }
}
