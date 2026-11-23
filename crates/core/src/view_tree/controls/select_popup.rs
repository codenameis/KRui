//! Top-Layer `<select>` (`SemanticRole::ComboBox`) dropdown popup overlay (`OverlayStack` /
//! `SelectPopupState`), viewport upward-flip & scroll geometry, Top-Layer pointer hit-testing,
//! and keyboard navigation.

use crate::view_tree::controls::ControlActivationCallbacks;
use crate::view_tree::focus_pointer::FocusTransitionCallbacks;
use crate::view_tree::form_submit::refresh_control_validity_if_needed;
use crate::view_tree::hit_test::{effective_node_world_matrix_and_rect, transform_rect_aabb};
use crate::view_tree::node::sync_node_text_context;
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;

/// Logical-pixel height of each `<option>` row inside the `<select>` dropdown popup.
pub const SELECT_POPUP_ROW_HEIGHT_PX: f32 = 30.0;
/// Inner padding (logical px) around the option list inside the `<select>` popup card.
pub const SELECT_POPUP_PADDING_PX: f32 = 4.0;
/// Vertical gap (logical px) between the `<select>` control box and its popup card.
pub const SELECT_POPUP_GAP_PX: f32 = 4.0;
/// Maximum height (logical px) of a `<select>` popup before vertical wheel scrolling engages.
pub const SELECT_POPUP_MAX_HEIGHT_PX: f32 = 216.0;
/// Minimum width (logical px) of a `<select>` popup card.
pub const SELECT_POPUP_MIN_WIDTH_PX: f32 = 96.0;
/// Horizontal text inset (logical px) inside each popup option row.
pub const SELECT_POPUP_TEXT_INSET_X_PX: f32 = 10.0;
/// Minimum margin (logical px) kept from viewport edges when positioning a `<select>` popup.
pub const SELECT_POPUP_VIEWPORT_MARGIN_PX: f32 = 4.0;

/// Active Top-Layer `<select>` dropdown popup state.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SelectPopupState {
    pub(crate) select_id: NodeId,
    pub(crate) highlighted_index: usize,
    pub(crate) scroll_y: f32,
    pub(crate) pointer_hovered: bool,
}

/// Top-Layer overlay stack stored on `ViewTreeInner`, rendered above all DOM clip containers.
#[derive(Default)]
pub(crate) struct OverlayStack {
    pub(crate) select_popup: Option<SelectPopupState>,
}

/// Screen-space geometry for an open `<select>` dropdown popup.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SelectPopupGeometry {
    pub(crate) popup_rect: LayoutRect,
    pub(crate) inner_clip: LayoutRect,
    pub(crate) max_scroll_y: f32,
}

pub(crate) fn resolve_current_select_index(inner: &ViewTreeInner, select_id: NodeId) -> usize {
    let Some(node) = inner.node(select_id) else {
        return 0;
    };
    let Some(ctrl) = inner.control(select_id) else {
        return 0;
    };
    if ctrl.select_options.is_empty() {
        return 0;
    }
    ctrl.selected_option_idx
        .or_else(|| {
            ctrl.select_options
                .iter()
                .position(|(v, _, _)| *v == node.text)
        })
        .unwrap_or(0)
        .min(ctrl.select_options.len() - 1)
}

/// Computes Top-Layer screen-space geometry for `select_id`, auto-flipping above the `<select>`
/// box when downward placement would overflow `viewport_h` and upward space is larger.
pub(crate) fn compute_select_popup_geometry(
    inner: &ViewTreeInner,
    select_id: NodeId,
) -> Option<SelectPopupGeometry> {
    let node = inner.node(select_id)?;
    if node.is_disabled() || node.role != SemanticRole::ComboBox {
        return None;
    }
    let opt_count = inner
        .control(select_id)
        .map_or(0, |c| c.select_options.len());
    if opt_count == 0 {
        return None;
    }
    let (vw, vh) = inner.last_viewport;
    let (aff, local_rect) = effective_node_world_matrix_and_rect(inner, select_id);
    let anchor = transform_rect_aabb(aff, local_rect);
    let content_h = opt_count as f32 * SELECT_POPUP_ROW_HEIGHT_PX;
    let (popup_y, popup_h) = compute_popup_vertical_span(anchor, content_h, vh);
    let popup_w = anchor.width.max(SELECT_POPUP_MIN_WIDTH_PX);
    let max_x =
        (vw - popup_w - SELECT_POPUP_VIEWPORT_MARGIN_PX).max(SELECT_POPUP_VIEWPORT_MARGIN_PX);
    let popup_x = if vw > popup_w {
        anchor.x.clamp(SELECT_POPUP_VIEWPORT_MARGIN_PX, max_x)
    } else {
        anchor.x
    };
    let popup_rect = LayoutRect {
        x: popup_x,
        y: popup_y,
        width: popup_w,
        height: popup_h,
    };
    let inner_h = (popup_h - SELECT_POPUP_PADDING_PX * 2.0).max(1.0);
    let inner_clip = LayoutRect {
        x: popup_rect.x + 1.0,
        y: popup_rect.y + SELECT_POPUP_PADDING_PX,
        width: (popup_rect.width - 2.0).max(1.0),
        height: inner_h,
    };
    Some(SelectPopupGeometry {
        popup_rect,
        inner_clip,
        max_scroll_y: (content_h - inner_h).max(0.0),
    })
}

fn compute_popup_vertical_span(anchor: LayoutRect, content_h: f32, vh: f32) -> (f32, f32) {
    let ideal_h = content_h + SELECT_POPUP_PADDING_PX * 2.0;
    let below_top = anchor.y + anchor.height + SELECT_POPUP_GAP_PX;
    let space_below = (vh - below_top - SELECT_POPUP_VIEWPORT_MARGIN_PX).max(0.0);
    let space_above = (anchor.y - SELECT_POPUP_GAP_PX - SELECT_POPUP_VIEWPORT_MARGIN_PX).max(0.0);
    let flip_up =
        ideal_h.min(SELECT_POPUP_MAX_HEIGHT_PX) > space_below && space_above > space_below;
    let avail_h = if flip_up { space_above } else { space_below };
    let min_box_h = SELECT_POPUP_ROW_HEIGHT_PX + SELECT_POPUP_PADDING_PX * 2.0;
    let popup_h = ideal_h.min(SELECT_POPUP_MAX_HEIGHT_PX.min(avail_h.max(min_box_h)));
    let popup_y = if flip_up {
        (anchor.y - SELECT_POPUP_GAP_PX - popup_h).max(SELECT_POPUP_VIEWPORT_MARGIN_PX)
    } else {
        below_top
    };
    (popup_y, popup_h)
}

fn scroll_row_into_view(scroll_y: f32, row_idx: usize, inner_h: f32, max_scroll_y: f32) -> f32 {
    let row_top = row_idx as f32 * SELECT_POPUP_ROW_HEIGHT_PX;
    let row_bottom = row_top + SELECT_POPUP_ROW_HEIGHT_PX;
    let mut next = scroll_y.clamp(0.0, max_scroll_y);
    if row_top < next {
        next = row_top;
    } else if row_bottom > next + inner_h {
        next = row_bottom - inner_h;
    }
    next.clamp(0.0, max_scroll_y)
}

pub(crate) fn open_select_popup(inner: &mut ViewTreeInner, select_id: NodeId) -> bool {
    let Some(geom) = compute_select_popup_geometry(inner, select_id) else {
        return false;
    };
    let cur_idx = resolve_current_select_index(inner, select_id);
    let scroll_y = scroll_row_into_view(0.0, cur_idx, geom.inner_clip.height, geom.max_scroll_y);
    inner.overlay.select_popup = Some(SelectPopupState {
        select_id,
        highlighted_index: cur_idx,
        scroll_y,
        pointer_hovered: false,
    });
    inner.paint_dirty = true;
    true
}

pub(crate) fn close_select_popup(inner: &mut ViewTreeInner) -> bool {
    if inner.overlay.select_popup.take().is_some() {
        inner.paint_dirty = true;
        true
    } else {
        false
    }
}

pub(crate) fn toggle_select_popup_on_click(inner: &mut ViewTreeInner, select_id: NodeId) -> bool {
    if inner
        .overlay
        .select_popup
        .as_ref()
        .is_some_and(|p| p.select_id == select_id)
    {
        close_select_popup(inner)
    } else {
        open_select_popup(inner, select_id)
    }
}

pub(crate) fn commit_select_option_index(
    inner: &mut ViewTreeInner,
    select_id: NodeId,
    option_idx: usize,
) -> Option<ControlActivationCallbacks> {
    let (val, changed, input_cb, change_cb) = {
        let is_disabled = inner.node(select_id).is_none_or(|n| n.is_disabled());
        if is_disabled {
            return None;
        }
        let val = inner
            .control(select_id)
            .and_then(|c| c.select_options.get(option_idx))
            .map(|(v, _, _)| v.clone())?;
        inner.ensure_control(select_id).selected_option_idx = Some(option_idx);
        let prev_val = inner.node(select_id).map(|n| n.text.clone())?;
        let changed = prev_val != val;
        if let Some(node) = inner.node_mut(select_id) {
            node.text = val.clone();
            node.set_text_dirty(true);
        }
        if let Some(ed) = inner.sparse.text_edits.get_mut(&select_id) {
            ed.committed_value_on_focus = val.clone();
        }
        let (icb, ccb) = if changed {
            inner
                .listeners(select_id)
                .map(|l| {
                    (
                        l.on_input.clone().map(|cb| (cb, val.clone())),
                        l.on_change.clone().map(|cb| (cb, val.clone())),
                    )
                })
                .unwrap_or((None, None))
        } else {
            (None, None)
        };
        (val, changed, icb, ccb)
    };
    let _ = val;
    close_select_popup(inner);
    if changed {
        refresh_control_validity_if_needed(inner, select_id);
        sync_node_text_context(inner, select_id);
    }
    inner.paint_dirty = true;
    Some(ControlActivationCallbacks {
        focus_cbs: FocusTransitionCallbacks::default(),
        checked_cbs: Vec::new(),
        input_cb,
        change_cb,
    })
}

pub(crate) fn activate_select_keyboard(
    inner: &mut ViewTreeInner,
    select_id: NodeId,
) -> Option<ControlActivationCallbacks> {
    if let Some(highlighted) = inner
        .overlay
        .select_popup
        .as_ref()
        .filter(|p| p.select_id == select_id)
        .map(|p| p.highlighted_index)
    {
        commit_select_option_index(inner, select_id, highlighted)
    } else if open_select_popup(inner, select_id) {
        Some(ControlActivationCallbacks::default())
    } else {
        None
    }
}

pub(crate) fn step_open_select_popup(
    inner: &mut ViewTreeInner,
    select_id: NodeId,
    dir: i32,
    to_bound: Option<bool>,
) -> bool {
    let Some(geom) = compute_select_popup_geometry(inner, select_id) else {
        return false;
    };
    let len = inner
        .control(select_id)
        .map(|c| c.select_options.len())
        .unwrap_or(0);
    if len == 0 {
        return false;
    }
    let Some(popup) = inner
        .overlay
        .select_popup
        .as_mut()
        .filter(|p| p.select_id == select_id)
    else {
        return false;
    };
    let cur = popup.highlighted_index.min(len - 1);
    let next = match to_bound {
        Some(false) => 0,
        Some(true) => len - 1,
        None => {
            let delta: isize = if dir > 0 { 1 } else { -1 };
            (cur as isize + delta).clamp(0, (len - 1) as isize) as usize
        }
    };
    popup.highlighted_index = next;
    popup.scroll_y = scroll_row_into_view(
        popup.scroll_y,
        next,
        geom.inner_clip.height,
        geom.max_scroll_y,
    );
    inner.paint_dirty = true;
    true
}

pub(crate) fn try_handle_select_popup_pointer_move(
    inner: &mut ViewTreeInner,
    x: f32,
    y: f32,
) -> Option<bool> {
    let select_id = inner.overlay.select_popup.as_ref()?.select_id;
    let Some(geom) = compute_select_popup_geometry(inner, select_id) else {
        close_select_popup(inner);
        return None;
    };
    if !geom.popup_rect.contains(x, y) {
        if let Some(popup) = inner.overlay.select_popup.as_mut() {
            if popup.pointer_hovered {
                popup.pointer_hovered = false;
                inner.paint_dirty = true;
            }
        }
        return None;
    }
    let opt_len = inner
        .control(select_id)
        .map(|c| c.select_options.len())
        .unwrap_or(0);
    let mut changed = false;
    if inner.hovered_node.take().is_some() {
        changed = true;
    }
    if let Some(popup) = inner.overlay.select_popup.as_mut() {
        let rel_y = (y - geom.inner_clip.y + popup.scroll_y).max(0.0);
        let row =
            ((rel_y / SELECT_POPUP_ROW_HEIGHT_PX).floor() as usize).min(opt_len.saturating_sub(1));
        if popup.highlighted_index != row || !popup.pointer_hovered {
            popup.highlighted_index = row;
            popup.pointer_hovered = true;
            changed = true;
        }
    }
    if changed {
        inner.paint_dirty = true;
    }
    Some(changed)
}

pub(crate) fn try_handle_select_popup_scroll(
    inner: &mut ViewTreeInner,
    x: f32,
    y: f32,
    delta_y: f32,
) -> Option<bool> {
    let select_id = inner.overlay.select_popup.as_ref()?.select_id;
    let geom = compute_select_popup_geometry(inner, select_id)?;
    if !geom.popup_rect.contains(x, y) {
        return None;
    }
    let popup = inner.overlay.select_popup.as_mut()?;
    let next = (popup.scroll_y - delta_y).clamp(0.0, geom.max_scroll_y);
    if (popup.scroll_y - next).abs() > 0.01 {
        popup.scroll_y = next;
        inner.paint_dirty = true;
        Some(true)
    } else {
        Some(false)
    }
}

pub(crate) fn hit_select_popup_row(
    inner: &ViewTreeInner,
    x: f32,
    y: f32,
) -> Option<(NodeId, Option<usize>)> {
    let popup = inner.overlay.select_popup.as_ref()?;
    let geom = compute_select_popup_geometry(inner, popup.select_id)?;
    if !geom.popup_rect.contains(x, y) {
        return None;
    }
    let opt_len = inner
        .control(popup.select_id)
        .map(|c| c.select_options.len())
        .unwrap_or(0);
    if opt_len == 0 || !geom.inner_clip.contains(x, y) {
        return Some((popup.select_id, None));
    }
    let rel_y = (y - geom.inner_clip.y + popup.scroll_y).max(0.0);
    let row = (rel_y / SELECT_POPUP_ROW_HEIGHT_PX).floor() as usize;
    Some((popup.select_id, (row < opt_len).then_some(row)))
}

impl ViewTree {
    /// Returns `true` if `id`'s Top-Layer `<select>` dropdown popup is currently open.
    pub fn is_select_popup_open(&self, id: NodeId) -> bool {
        self.inner
            .borrow()
            .overlay
            .select_popup
            .as_ref()
            .is_some_and(|p| p.select_id == id)
    }

    /// Returns the `NodeId` of the `<select>` whose Top-Layer popup is currently open, if any.
    pub fn active_select_popup(&self) -> Option<NodeId> {
        self.inner
            .borrow()
            .overlay
            .select_popup
            .as_ref()
            .map(|p| p.select_id)
    }

    /// Returns the screen-space `LayoutRect` of the currently open `<select>` popup, if any.
    pub fn active_select_popup_rect(&self) -> Option<LayoutRect> {
        let inner = self.inner.borrow();
        let select_id = inner.overlay.select_popup.as_ref()?.select_id;
        compute_select_popup_geometry(&inner, select_id).map(|g| g.popup_rect)
    }
}
