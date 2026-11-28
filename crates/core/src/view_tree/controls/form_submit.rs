//! `<select>` option selection and `<form>` validation (`required`, `type="email|number"`),
//! submission (`onsubmit`), and reset (`onreset`) execution.

use crate::view_tree::controls::{
    collect_radio_group_peers, format_range_value, CheckedCallback, ControlActivationCallbacks,
    StringValCallback,
};
use crate::view_tree::focus_pointer::{transition_focus, FocusTransitionCallbacks};
use crate::view_tree::node::sync_node_text_context;
use crate::view_tree::{InputKind, NodeId, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_signals::batch;
use krui_style::DisplayMode;
pub use krui_ui_types::ButtonKind;
use std::rc::Rc;

#[derive(Default)]
pub(crate) struct FormActionCallbacks {
    pub(crate) focus_cbs: FocusTransitionCallbacks,
    pub(crate) checked_cbs: Vec<CheckedCallback>,
    pub(crate) input_cbs: Vec<StringValCallback>,
    pub(crate) change_cbs: Vec<StringValCallback>,
    pub(crate) form_cb: Option<Rc<dyn Fn()>>,
    pub(crate) handled: bool,
}

impl FormActionCallbacks {
    pub(crate) fn invoke(self) {
        self.focus_cbs.invoke();
        for (cb, val) in self.checked_cbs {
            batch(|| cb(val));
        }
        for (cb, val) in self.input_cbs {
            batch(|| cb(val));
        }
        for (cb, val) in self.change_cbs {
            batch(|| cb(val));
        }
        if let Some(cb) = self.form_cb {
            batch(|| cb());
        }
    }
}

/// Returns the human-readable `<option>` label for the currently selected value of a `<select>` (`ComboBox`) node.
pub(crate) fn select_display_label(inner: &ViewTreeInner, nid: NodeId) -> String {
    let Some(node) = inner.node(nid) else {
        return String::new();
    };
    let Some(ctrl) = inner.control(nid) else {
        return node.text.clone();
    };
    if ctrl.select_options.is_empty() {
        return node.text.clone();
    }
    if let Some((_, label, _)) = ctrl
        .select_options
        .iter()
        .find(|(val, _, _)| *val == node.text)
    {
        return label.clone();
    }
    if let Some(idx) = ctrl.selected_option_idx {
        if let Some((_, label, _)) = ctrl.select_options.get(idx) {
            return label.clone();
        }
    }
    ctrl.select_options
        .first()
        .map(|(_, label, _)| label.clone())
        .unwrap_or_else(|| node.text.clone())
}

pub(crate) fn step_select_option(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    step_dir: i32,
    to_bound: Option<bool>,
) -> Option<ControlActivationCallbacks> {
    if inner.node(nid).is_none_or(|n| n.is_disabled()) {
        return None;
    }
    let cur_text = inner.node(nid)?.text.clone();
    let (next_idx, val) = {
        let ctrl = inner.control(nid)?;
        if ctrl.select_options.is_empty() {
            return None;
        }
        let len = ctrl.select_options.len();
        let cur_idx = ctrl
            .selected_option_idx
            .or_else(|| {
                ctrl.select_options
                    .iter()
                    .position(|(v, _, _)| *v == cur_text)
            })
            .unwrap_or(0)
            .min(len - 1);
        let next = match to_bound {
            Some(false) => 0,
            Some(true) => len - 1,
            None => {
                let delta: isize = if step_dir > 0 { 1 } else { -1 };
                (cur_idx as isize + delta).clamp(0, (len - 1) as isize) as usize
            }
        };
        (next, ctrl.select_options[next].0.clone())
    };
    inner.ensure_control(nid).selected_option_idx = Some(next_idx);
    if let Some(node) = inner.node_mut(nid) {
        node.text = val.clone();
        node.set_text_dirty(true);
    }
    if let Some(ed) = inner.sparse.text_edits.get_mut(&nid) {
        ed.committed_value_on_focus = val.clone();
    }
    let (input_cb, change_cb) = inner.listeners(nid).map_or((None, None), |l| {
        (
            l.on_input.clone().map(|cb| (cb, val.clone())),
            l.on_change.clone().map(|cb| (cb, val)),
        )
    });
    refresh_control_validity_if_needed(inner, nid);
    sync_node_text_context(inner, nid);
    inner.paint_dirty = true;
    Some(ControlActivationCallbacks {
        input_cb,
        change_cb,
        ..ControlActivationCallbacks::default()
    })
}

pub(crate) fn find_enclosing_form(inner: &ViewTreeInner, mut cur_id: NodeId) -> Option<NodeId> {
    loop {
        let node = inner.node(cur_id)?;
        if node.role == SemanticRole::Form {
            return Some(cur_id);
        }
        cur_id = node.parent?;
    }
}

fn is_valid_email_address(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains(char::is_whitespace) {
        return false;
    }
    let mut parts = trimmed.split('@');
    let local = parts.next().unwrap_or("");
    let domain = parts.next().unwrap_or("");
    !local.is_empty() && !domain.is_empty() && parts.next().is_none()
}

pub(crate) fn validate_control_node(inner: &ViewTreeInner, nid: NodeId) -> bool {
    let Some(node) = inner.node(nid) else {
        return true;
    };
    if node.is_disabled() || node.is_anchor() {
        return true;
    }
    let is_required = inner.control(nid).is_some_and(|c| c.is_required);
    if is_required {
        match node.role {
            SemanticRole::Checkbox => {
                if !node.is_checked() {
                    return false;
                }
            }
            SemanticRole::RadioButton => {
                let any_checked = collect_radio_group_peers(inner, nid)
                    .into_iter()
                    .any(|pid| inner.node(pid).is_some_and(|p| p.is_checked()));
                if !any_checked {
                    return false;
                }
            }
            _ => {
                if node.text.trim().is_empty() {
                    return false;
                }
            }
        }
    }
    let trimmed = node.text.trim();
    if !trimmed.is_empty() && node.role.is_single_line_input() {
        let input_kind = inner
            .control(nid)
            .map(|c| c.input_kind)
            .unwrap_or(InputKind::Text);
        match input_kind {
            InputKind::Email if !is_valid_email_address(trimmed) => return false,
            InputKind::Number if !trimmed.parse::<f64>().is_ok_and(|n| n.is_finite()) => {
                return false;
            }
            _ => {}
        }
    }
    true
}

pub(crate) fn refresh_control_validity_if_needed(inner: &mut ViewTreeInner, nid: NodeId) {
    if inner.node(nid).is_some_and(|n| n.is_invalid()) && validate_control_node(inner, nid) {
        if let Some(node) = inner.node_mut(nid) {
            node.set_invalid(false);
            inner.paint_dirty = true;
        }
        crate::view_tree::sync::notify_selector_state_change(inner, nid);
    }
}

fn collect_form_controls(inner: &ViewTreeInner, node_id: NodeId, out: &mut Vec<NodeId>) {
    let Some(n) = inner.node(node_id) else {
        return;
    };
    if n.is_anchor() || n.style.layout.display == DisplayMode::None {
        return;
    }
    if n.role.is_text_editable()
        || matches!(
            n.role,
            SemanticRole::Checkbox
                | SemanticRole::RadioButton
                | SemanticRole::Slider
                | SemanticRole::ComboBox
        )
    {
        out.push(node_id);
    }
    for &cid in &n.children {
        collect_form_controls(inner, cid, out);
    }
}

pub(crate) fn try_submit_enclosing_form(
    inner: &mut ViewTreeInner,
    trigger_id: NodeId,
) -> FormActionCallbacks {
    let Some(form_id) = find_enclosing_form(inner, trigger_id) else {
        return FormActionCallbacks::default();
    };
    if inner.node(form_id).is_none_or(|f| f.is_disabled()) {
        return FormActionCallbacks::default();
    }
    let mut controls = Vec::new();
    collect_form_controls(inner, form_id, &mut controls);
    let mut first_invalid = None;
    for cid in controls {
        let valid = validate_control_node(inner, cid);
        let changed_invalid = if let Some(cnode) = inner.node_mut(cid) {
            let next_invalid = !valid;
            if cnode.is_invalid() != next_invalid {
                cnode.set_invalid(next_invalid);
                true
            } else {
                false
            }
        } else {
            false
        };
        if changed_invalid {
            crate::view_tree::sync::notify_selector_state_change(inner, cid);
        }
        if !valid && first_invalid.is_none() {
            first_invalid = Some(cid);
        }
    }
    inner.paint_dirty = true;
    if let Some(invalid_nid) = first_invalid {
        let focus_cbs = transition_focus(inner, Some(invalid_nid));
        return FormActionCallbacks {
            focus_cbs,
            handled: true,
            ..FormActionCallbacks::default()
        };
    }
    let form_cb = inner.control(form_id).and_then(|c| c.on_submit.clone());
    FormActionCallbacks {
        form_cb,
        handled: true,
        ..FormActionCallbacks::default()
    }
}

pub(crate) fn try_reset_enclosing_form(
    inner: &mut ViewTreeInner,
    trigger_id: NodeId,
) -> FormActionCallbacks {
    let Some(form_id) = find_enclosing_form(inner, trigger_id) else {
        return FormActionCallbacks::default();
    };
    if inner.node(form_id).is_none_or(|f| f.is_disabled()) {
        return FormActionCallbacks::default();
    }
    let mut controls = Vec::new();
    collect_form_controls(inner, form_id, &mut controls);
    let mut out = FormActionCallbacks {
        handled: true,
        ..FormActionCallbacks::default()
    };
    for cid in controls {
        reset_single_control(inner, cid, &mut out);
    }
    out.form_cb = inner.control(form_id).and_then(|c| c.on_reset.clone());
    inner.paint_dirty = true;
    out
}

fn reset_single_control(inner: &mut ViewTreeInner, cid: NodeId, out: &mut FormActionCallbacks) {
    let Some(role) = inner.node(cid).map(|n| n.role) else {
        return;
    };
    let cleared_invalid = if let Some(node) = inner.node_mut(cid) {
        let was = node.is_invalid();
        node.set_invalid(false);
        was
    } else {
        false
    };
    if cleared_invalid {
        crate::view_tree::sync::notify_selector_state_change(inner, cid);
    }
    let need_sync = match role {
        SemanticRole::Checkbox | SemanticRole::RadioButton => {
            let init_chk = inner
                .control(cid)
                .and_then(|c| c.initial_checked)
                .unwrap_or(false);
            let mut toggled = false;
            let toggled_chk = if let Some(node) = inner.node_mut(cid) {
                if node.is_checked() != init_chk {
                    node.set_checked(init_chk);
                    toggled = true;
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if toggled_chk {
                crate::view_tree::sync::notify_selector_state_change(inner, cid);
            }
            if toggled {
                if let Some(cb) = inner.control(cid).and_then(|c| c.on_checked_change.clone()) {
                    out.checked_cbs.push((cb, init_chk));
                }
            }
            false
        }
        SemanticRole::ComboBox => reset_combobox_node(inner, cid, out),
        _ => reset_text_node(inner, cid, role, out),
    };
    if need_sync {
        sync_node_text_context(inner, cid);
    }
}

fn reset_combobox_node(
    inner: &mut ViewTreeInner,
    cid: NodeId,
    out: &mut FormActionCallbacks,
) -> bool {
    let (init_val, next_sel_idx) = {
        let Some(ctrl) = inner.control(cid) else {
            return false;
        };
        let init_idx = ctrl
            .select_options
            .iter()
            .position(|(_, _, sel)| *sel)
            .unwrap_or(0);
        let val = ctrl
            .select_options
            .get(init_idx)
            .map(|(v, _, _)| v.clone())
            .or_else(|| ctrl.initial_text.clone())
            .unwrap_or_default();
        (val, (!ctrl.select_options.is_empty()).then_some(init_idx))
    };
    inner.ensure_control(cid).selected_option_idx = next_sel_idx;
    if inner.node(cid).is_none_or(|n| n.text == init_val) {
        return false;
    }
    if let Some(node) = inner.node_mut(cid) {
        node.text = init_val.clone();
        node.set_text_dirty(true);
    }
    if let Some(ed) = inner.sparse.text_edits.get_mut(&cid) {
        ed.committed_value_on_focus = init_val.clone();
    }
    if let Some(l) = inner.listeners(cid) {
        if let Some(cb) = l.on_input.clone() {
            out.input_cbs.push((cb, init_val.clone()));
        }
        if let Some(cb) = l.on_change.clone() {
            out.change_cbs.push((cb, init_val));
        }
    }
    true
}

fn reset_text_node(
    inner: &mut ViewTreeInner,
    cid: NodeId,
    role: SemanticRole,
    out: &mut FormActionCallbacks,
) -> bool {
    let init_txt = inner
        .control(cid)
        .and_then(|c| c.initial_text.clone())
        .unwrap_or_else(|| {
            if role == SemanticRole::Slider {
                let min_v = inner.control(cid).map(|c| c.range_bounds.0).unwrap_or(0.0);
                format_range_value(min_v)
            } else {
                String::new()
            }
        });
    if inner.node(cid).is_none_or(|n| n.text == init_txt) {
        return false;
    }
    let char_len = init_txt.chars().count();
    if let Some(node) = inner.node_mut(cid) {
        node.text = init_txt.clone();
        node.set_text_dirty(true);
    }
    if let Some(ed) = inner.sparse.text_edits.get_mut(&cid) {
        ed.committed_value_on_focus = init_txt.clone();
        ed.cursor_char_idx = char_len;
        ed.selection_anchor_char_idx = None;
        ed.preferred_x = None;
        ed.needs_caret_scroll = true;
    }
    if let Some(cb) = inner.listeners(cid).and_then(|l| l.on_input.clone()) {
        out.input_cbs.push((cb, init_txt));
    }
    true
}

pub(crate) fn trigger_button_form_action(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
) -> FormActionCallbacks {
    let Some(node) = inner.node(node_id) else {
        return FormActionCallbacks::default();
    };
    if node.is_disabled() || node.role != SemanticRole::Button {
        return FormActionCallbacks::default();
    }
    let kind = inner
        .control(node_id)
        .map(|c| c.button_kind)
        .unwrap_or(ButtonKind::Auto);
    match kind {
        ButtonKind::Button => FormActionCallbacks::default(),
        ButtonKind::Submit | ButtonKind::Auto => try_submit_enclosing_form(inner, node_id),
        ButtonKind::Reset => try_reset_enclosing_form(inner, node_id),
    }
}

pub(crate) fn sync_select_selected_option(inner: &mut ViewTreeInner, nid: NodeId) {
    let Some((cur_text, has_dyn)) = inner
        .node(nid)
        .map(|n| (n.text.clone(), n.has_dynamic_text_binding()))
    else {
        return;
    };
    let (next_idx, assign_val) = {
        let ctrl = inner.ensure_control(nid);
        if ctrl.select_options.is_empty() {
            ctrl.selected_option_idx = None;
            return;
        }
        if !cur_text.is_empty() {
            if let Some(idx) = ctrl
                .select_options
                .iter()
                .position(|(v, _, _)| *v == cur_text)
            {
                ctrl.selected_option_idx = Some(idx);
                return;
            }
            if has_dyn {
                ctrl.selected_option_idx = None;
                return;
            }
        }
        let fb = ctrl
            .select_options
            .iter()
            .position(|(_, _, sel)| *sel)
            .unwrap_or(0);
        let val = ctrl.select_options[fb].0.clone();
        if ctrl.initial_text.is_none() {
            ctrl.initial_text = Some(val.clone());
        }
        (fb, (!has_dyn || cur_text.is_empty()).then_some(val))
    };
    inner.ensure_control(nid).selected_option_idx = Some(next_idx);
    if let Some(val) = assign_val {
        if let Some(node) = inner.node_mut(nid) {
            node.text = val.clone();
        }
        if let Some(ed) = inner.sparse.text_edits.get_mut(&nid) {
            ed.committed_value_on_focus = val;
        }
    }
}
