//! Interactive form controls (`<label>`, `<input type="checkbox|radio|range">`, `:checked`, `:invalid`).

use crate::view_tree::focus_pointer::{transition_focus, FocusTransitionCallbacks};
use crate::view_tree::form_submit::{refresh_control_validity_if_needed, step_select_option};
use crate::view_tree::select_popup::{activate_select_keyboard, step_open_select_popup};
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_signals::batch;
use krui_style::DisplayMode;
use std::rc::Rc;

/// Default intrinsic width/height (logical px) for `<input type="checkbox">` and `<input type="radio">`.
pub const CHECKABLE_DEFAULT_SIZE_PX: f32 = 20.0;
/// Default intrinsic minimum width (logical px) for `<input type="range">`.
pub const SLIDER_DEFAULT_MIN_WIDTH_PX: f32 = 120.0;
/// Default intrinsic height (logical px) for `<input type="range">`.
pub const SLIDER_DEFAULT_HEIGHT_PX: f32 = 24.0;
/// Visual height (logical px) of the `<input type="range">` track bar.
pub const SLIDER_TRACK_HEIGHT_PX: f32 = 6.0;
/// Visual diameter (logical px) of the `<input type="range">` thumb knob.
pub const SLIDER_THUMB_SIZE_PX: f32 = 14.0;

pub(crate) type CheckedCallback = (Rc<dyn Fn(bool)>, bool);
pub(crate) type StringValCallback = (Rc<dyn Fn(String)>, String);

#[derive(Default)]
pub(crate) struct ControlActivationCallbacks {
    pub(crate) focus_cbs: FocusTransitionCallbacks,
    pub(crate) checked_cbs: Vec<CheckedCallback>,
    pub(crate) input_cb: Option<StringValCallback>,
    pub(crate) change_cb: Option<StringValCallback>,
}

impl ControlActivationCallbacks {
    pub(crate) fn invoke(self) {
        self.focus_cbs.invoke();
        for (cb, val) in self.checked_cbs {
            batch(|| cb(val));
        }
        if let Some((cb, val)) = self.input_cb {
            batch(|| cb(val));
        }
        if let Some((cb, val)) = self.change_cb {
            batch(|| cb(val));
        }
    }
}

/// Quantizes `raw` to `(min, max, step)` and clamps to `[min, max]`.
pub(crate) fn quantize_range_value(raw: f32, (min, max, step): (f32, f32, f32)) -> f32 {
    let lo = if min.is_finite() { min } else { 0.0 };
    let hi = if max.is_finite() {
        max.max(lo)
    } else {
        lo.max(100.0)
    };
    let st = if step.is_finite() {
        step.abs().max(0.000_001)
    } else {
        1.0
    };
    let safe_raw = if raw.is_finite() { raw } else { lo };
    let clamped = safe_raw.clamp(lo, hi);
    let steps = ((clamped - lo) / st).round();
    (lo + steps * st).clamp(lo, hi)
}

/// Parses `text` as a range value, falling back to `min` if empty or invalid, and quantizes to `(min, max, step)`.
pub(crate) fn parse_and_clamp_range_value(text: &str, bounds: (f32, f32, f32)) -> f32 {
    let raw = text
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .unwrap_or(bounds.0);
    quantize_range_value(raw, bounds)
}

/// Formats a quantized range `f32` value cleanly without trailing float noise.
pub(crate) fn format_range_value(val: f32) -> String {
    let rounded = (val * 10_000.0).round() / 10_000.0;
    if (rounded - rounded.round()).abs() < 1e-5 {
        format!("{}", rounded.round() as i64)
    } else {
        format!("{rounded}")
    }
}

/// Resolves the target control `NodeId` associated with a `<label>` node (via `for="..."` or first focusable descendant).
pub(crate) fn resolve_label_target_node(inner: &ViewTreeInner, label_id: NodeId) -> Option<NodeId> {
    let n = inner.node(label_id)?;
    let label_for = inner
        .control(label_id)
        .and_then(|c| c.label_for.as_deref())
        .filter(|s| !s.is_empty());
    if n._tag != "label" && label_for.is_none() {
        return None;
    }
    if let Some(target_elem_id) = label_for {
        return inner
            .nodes
            .iter()
            .find(|c| {
                c.alive() && !c.is_anchor() && c.element_id.as_deref() == Some(target_elem_id)
            })
            .map(|c| c.id);
    }
    find_first_focusable_descendant(inner, label_id)
}

fn find_first_focusable_descendant(inner: &ViewTreeInner, root_id: NodeId) -> Option<NodeId> {
    let n = inner.node(root_id)?;
    for &cid in &n.children {
        if let Some(cn) = inner.node(cid) {
            if !cn.is_anchor() && cn.role.is_focusable() {
                return Some(cid);
            }
            if let Some(found) = find_first_focusable_descendant(inner, cid) {
                return Some(found);
            }
        }
    }
    None
}

pub(crate) fn collect_radio_group_peers(inner: &ViewTreeInner, radio_id: NodeId) -> Vec<NodeId> {
    let Some(target) = inner.node(radio_id) else {
        return Vec::new();
    };
    let group_name = inner
        .control(radio_id)
        .and_then(|c| c.control_name.clone())
        .filter(|s| !s.is_empty());
    let parent_id = target.parent;
    let mut order = Vec::new();
    for &root_id in &inner.roots {
        collect_radio_peers_recursive(inner, root_id, group_name.as_deref(), parent_id, &mut order);
    }
    order
}

fn collect_radio_peers_recursive(
    inner: &ViewTreeInner,
    node_id: NodeId,
    group_name: Option<&str>,
    fallback_parent: Option<NodeId>,
    out: &mut Vec<NodeId>,
) {
    let Some(n) = inner.node(node_id) else {
        return;
    };
    if n.is_anchor() || n.style.layout.display == DisplayMode::None {
        return;
    }
    if n.role == SemanticRole::RadioButton {
        let peer_name = inner
            .control(node_id)
            .and_then(|c| c.control_name.as_deref());
        let same_group = match group_name {
            Some(g) => peer_name == Some(g),
            None => peer_name.unwrap_or("").is_empty() && n.parent == fallback_parent,
        };
        if same_group {
            out.push(node_id);
        }
    }
    for &cid in &n.children {
        collect_radio_peers_recursive(inner, cid, group_name, fallback_parent, out);
    }
}

pub(crate) fn select_radio_in_group(
    inner: &mut ViewTreeInner,
    radio_id: NodeId,
) -> Vec<CheckedCallback> {
    let peers = collect_radio_group_peers(inner, radio_id);
    let mut cbs = Vec::new();
    for pid in peers {
        let should_check = pid == radio_id;
        let mut toggled = false;
        if let Some(peer) = inner.node_mut(pid) {
            if peer.is_checked() != should_check {
                peer.set_checked(should_check);
                toggled = true;
            }
        }
        if toggled {
            if let Some(cb) = inner.control(pid).and_then(|c| c.on_checked_change.clone()) {
                cbs.push((cb, should_check));
            }
            crate::view_tree::sync::notify_selector_state_change(inner, pid);
        }
    }
    inner.paint_dirty = true;
    cbs
}

pub(crate) fn activate_control_node(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
) -> Option<ControlActivationCallbacks> {
    let target_id = resolve_label_target_node(inner, node_id).unwrap_or(node_id);
    let role = inner.node(target_id).filter(|n| !n.is_disabled())?.role;
    match role {
        SemanticRole::Checkbox => activate_checkbox_node(inner, target_id),
        SemanticRole::RadioButton => activate_radio_node(inner, target_id),
        SemanticRole::ComboBox => activate_select_keyboard(inner, target_id),
        _ => None,
    }
}

fn activate_checkbox_node(
    inner: &mut ViewTreeInner,
    target_id: NodeId,
) -> Option<ControlActivationCallbacks> {
    let (checked, val_str) = {
        let node = inner.node_mut(target_id)?;
        let next = !node.is_checked();
        node.set_checked(next);
        let vs = if next { "true" } else { "false" }.to_string();
        if node.text.is_empty() || node.text == "true" || node.text == "false" {
            node.text = vs.clone();
        }
        (next, vs)
    };
    let checked_cbs = inner
        .control(target_id)
        .and_then(|c| c.on_checked_change.clone())
        .into_iter()
        .map(|cb| (cb, checked))
        .collect();
    let (input_cb, change_cb) = extract_input_change_cbs(inner, target_id, &val_str);
    refresh_control_validity_if_needed(inner, target_id);
    crate::view_tree::sync::notify_selector_state_change(inner, target_id);
    inner.paint_dirty = true;
    Some(ControlActivationCallbacks {
        focus_cbs: FocusTransitionCallbacks::default(),
        checked_cbs,
        input_cb,
        change_cb,
    })
}

fn activate_radio_node(
    inner: &mut ViewTreeInner,
    target_id: NodeId,
) -> Option<ControlActivationCallbacks> {
    let checked_cbs = select_radio_in_group(inner, target_id);
    refresh_control_validity_if_needed(inner, target_id);
    let val_str = {
        let node = inner.node(target_id)?;
        if node.text.is_empty() {
            "on".to_string()
        } else {
            node.text.clone()
        }
    };
    let (input_cb, change_cb) = extract_input_change_cbs(inner, target_id, &val_str);
    Some(ControlActivationCallbacks {
        focus_cbs: FocusTransitionCallbacks::default(),
        checked_cbs,
        input_cb,
        change_cb,
    })
}

fn extract_input_change_cbs(
    inner: &ViewTreeInner,
    nid: NodeId,
    val_str: &str,
) -> (Option<StringValCallback>, Option<StringValCallback>) {
    inner
        .listeners(nid)
        .map(|l| {
            (
                l.on_input.clone().map(|cb| (cb, val_str.to_string())),
                l.on_change.clone().map(|cb| (cb, val_str.to_string())),
            )
        })
        .unwrap_or((None, None))
}

pub(crate) fn step_focused_control(
    inner: &mut ViewTreeInner,
    dir: i32,
    to_bound: Option<bool>,
) -> Option<ControlActivationCallbacks> {
    let focused_nid = inner
        .a11y_engine
        .focused_id()
        .and_then(|ak| NodeId::try_from(ak).ok())?;
    let role = inner.node(focused_nid).filter(|n| !n.is_disabled())?.role;
    match role {
        SemanticRole::Slider => step_slider_control(inner, focused_nid, dir, to_bound),
        SemanticRole::ComboBox => {
            if step_open_select_popup(inner, focused_nid, dir, to_bound) {
                Some(ControlActivationCallbacks::default())
            } else {
                step_select_option(inner, focused_nid, dir, to_bound)
            }
        }
        SemanticRole::RadioButton if dir != 0 => step_radio_group_focus(inner, focused_nid, dir),
        _ => None,
    }
}

fn step_slider_control(
    inner: &mut ViewTreeInner,
    focused_nid: NodeId,
    dir: i32,
    to_bound: Option<bool>,
) -> Option<ControlActivationCallbacks> {
    let bounds = inner
        .control(focused_nid)
        .map(|c| c.range_bounds)
        .unwrap_or((0.0, 100.0, 1.0));
    let cur = parse_and_clamp_range_value(&inner.node(focused_nid)?.text, bounds);
    let slider_dir = if dir == 1 || dir == -2 { 1.0 } else { -1.0 };
    let next = match to_bound {
        Some(false) => bounds.0,
        Some(true) => bounds.1.max(bounds.0),
        None => quantize_range_value(cur + slider_dir * bounds.2, bounds),
    };
    let next_str = format_range_value(next);
    if let Some(node) = inner.node_mut(focused_nid) {
        node.text = next_str.clone();
    }
    if let Some(ed) = inner.sparse.text_edits.get_mut(&focused_nid) {
        ed.committed_value_on_focus = next_str.clone();
    }
    let (input_cb, change_cb) = extract_input_change_cbs(inner, focused_nid, &next_str);
    inner.paint_dirty = true;
    Some(ControlActivationCallbacks {
        focus_cbs: FocusTransitionCallbacks::default(),
        checked_cbs: Vec::new(),
        input_cb,
        change_cb,
    })
}

fn step_radio_group_focus(
    inner: &mut ViewTreeInner,
    focused_nid: NodeId,
    dir: i32,
) -> Option<ControlActivationCallbacks> {
    let peers: Vec<NodeId> = collect_radio_group_peers(inner, focused_nid)
        .into_iter()
        .filter(|&pid| inner.node(pid).is_some_and(|n| !n.is_disabled()))
        .collect();
    if peers.is_empty() {
        return None;
    }
    let cur_pos = peers.iter().position(|&p| p == focused_nid).unwrap_or(0) as isize;
    let step = if dir > 0 { 1 } else { -1 };
    let next_id = peers[(cur_pos + step).rem_euclid(peers.len() as isize) as usize];
    let focus_cbs = transition_focus(inner, Some(next_id));
    let mut act = activate_control_node(inner, next_id).unwrap_or_default();
    act.focus_cbs = focus_cbs;
    Some(act)
}

pub(crate) fn apply_slider_pointer_at(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    lx: f32,
    local_rect: LayoutRect,
) -> Option<ControlActivationCallbacks> {
    let bounds = inner
        .control(nid)
        .map(|c| c.range_bounds)
        .unwrap_or((0.0, 100.0, 1.0));
    let next_str = {
        let node = inner.node_mut(nid)?;
        if node.is_disabled() || node.role != SemanticRole::Slider {
            return None;
        }
        let pad_l = node.style.layout.padding.left.min(local_rect.width * 0.25);
        let pad_r = node.style.layout.padding.right.min(local_rect.width * 0.25);
        let track_x = local_rect.x + pad_l;
        let track_w = (local_rect.width - pad_l - pad_r).max(1.0);
        let ratio = ((lx - track_x) / track_w).clamp(0.0, 1.0);
        let (min, max, step) = bounds;
        let next = quantize_range_value(min + ratio * (max.max(min) - min), (min, max, step));
        let formatted = format_range_value(next);
        if node.text == formatted {
            return Some(ControlActivationCallbacks::default());
        }
        node.text = formatted.clone();
        formatted
    };
    if let Some(ed) = inner.sparse.text_edits.get_mut(&nid) {
        ed.committed_value_on_focus = next_str.clone();
    }
    let (input_cb, change_cb) = extract_input_change_cbs(inner, nid, &next_str);
    inner.paint_dirty = true;
    Some(ControlActivationCallbacks {
        focus_cbs: FocusTransitionCallbacks::default(),
        checked_cbs: Vec::new(),
        input_cb,
        change_cb,
    })
}
