//! Scrollbar hit-testing/dragging, mouse wheel scroll chaining (`<textarea>` & `overflow: scroll`),
//! Top-Layer `<select>` popup wheel scrolling, `onscroll` callbacks, and keyboard scroll.

use crate::view_tree::hit_test::{
    effective_node_world_matrix_and_rect, hit_test_any_node, hit_test_scrollable_node,
};
use crate::view_tree::node::ClickTarget;
use crate::view_tree::select_popup::try_handle_select_popup_scroll;
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_signals::batch;
use krui_style::{DisplayMode, ScrollbarWidth};
use std::rc::Rc;

/// Minimum scroll container height (logical px) required to display a vertical scrollbar.
pub const SCROLLBAR_MIN_CONTAINER_HEIGHT_PX: f32 = 24.0;
/// Minimum `max_scroll_y` threshold (logical px) required to display a vertical scrollbar.
pub const SCROLLBAR_MIN_MAX_SCROLL_PX: f32 = 0.5;
/// Top and bottom inset (logical px) of the scrollbar track inside the scroll container.
pub const SCROLLBAR_TRACK_VERTICAL_INSET_PX: f32 = 6.0;
/// Distance (logical px) from the right edge of the scroll container to the left edge of the thumb.
pub const SCROLLBAR_RIGHT_INSET_PX: f32 = 9.0;
/// Visual width (logical px) of the scrollbar thumb (`scrollbar-width: auto`).
pub const SCROLLBAR_THUMB_WIDTH_PX: f32 = 6.0;
/// Minimum visual height (logical px) of the scrollbar thumb.
pub const SCROLLBAR_THUMB_MIN_HEIGHT_PX: f32 = 18.0;
/// Opacity multiplier applied to the scrollbar thumb relative to the container's opacity.
pub const SCROLLBAR_THUMB_OPACITY_FACTOR: f32 = 0.75;
/// Corner border radius (logical px) of the scrollbar thumb.
pub const SCROLLBAR_THUMB_RADIUS_PX: f32 = 3.0;
/// Extra horizontal hit-testing slop (logical px) around the scrollbar track and thumb.
pub const SCROLLBAR_HIT_SLOP_PX: f32 = 6.0;

/// Vertical scroll step (logical px) for keyboard `ArrowUp` / `ArrowDown` inside a scroll container.
pub const KEYBOARD_SCROLL_LINE_STEP_PX: f32 = 28.0;
/// Viewport height fraction scrolled by `PageUp` / `PageDown`.
pub const KEYBOARD_SCROLL_PAGE_FRACTION: f32 = 0.85;

pub(crate) type ScrollCallback = Rc<dyn Fn()>;

/// Shared geometry for vertical scrollbar rendering and pointer hit-testing/dragging.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollbarGeometry {
    pub(crate) track_top: f32,
    pub(crate) track_rect: LayoutRect,
    pub(crate) thumb_rect: LayoutRect,
    pub(crate) thumb_radius: f32,
    pub(crate) hit_track_rect: LayoutRect,
    pub(crate) hit_thumb_rect: LayoutRect,
    pub(crate) movable_track_height: f32,
    pub(crate) max_scroll_y: f32,
}

pub(crate) fn compute_scrollbar_geometry(
    world_rect: LayoutRect,
    scroll_y: f32,
    max_scroll_y: f32,
    scrollbar_width: ScrollbarWidth,
) -> Option<ScrollbarGeometry> {
    let (thumb_w, thumb_radius, right_inset) = scrollbar_width.metrics()?;
    if world_rect.height <= SCROLLBAR_MIN_CONTAINER_HEIGHT_PX
        || max_scroll_y <= SCROLLBAR_MIN_MAX_SCROLL_PX
    {
        return None;
    }
    let track_h = (world_rect.height - SCROLLBAR_TRACK_VERTICAL_INSET_PX * 2.0).max(1.0);
    let thumb_h = (track_h * (world_rect.height / (world_rect.height + max_scroll_y)))
        .clamp(SCROLLBAR_THUMB_MIN_HEIGHT_PX.min(track_h), track_h);
    let movable_h = (track_h - thumb_h).max(0.0);
    let ratio = (scroll_y / max_scroll_y).clamp(0.0, 1.0);
    let track_top = world_rect.y + SCROLLBAR_TRACK_VERTICAL_INSET_PX;
    let thumb_x = world_rect.x + world_rect.width - right_inset;
    let thumb_y = track_top + movable_h * ratio;
    let track_rect = LayoutRect {
        x: thumb_x,
        y: track_top,
        width: thumb_w,
        height: track_h,
    };
    let thumb_rect = LayoutRect {
        x: thumb_x,
        y: thumb_y,
        width: thumb_w,
        height: thumb_h,
    };
    let hit_x = thumb_x - SCROLLBAR_HIT_SLOP_PX;
    let hit_w = thumb_w + SCROLLBAR_HIT_SLOP_PX * 2.0;
    Some(ScrollbarGeometry {
        track_top,
        track_rect,
        thumb_rect,
        thumb_radius,
        hit_track_rect: LayoutRect {
            x: hit_x,
            y: world_rect.y,
            width: hit_w,
            height: world_rect.height,
        },
        hit_thumb_rect: LayoutRect {
            x: hit_x,
            y: thumb_y,
            width: hit_w,
            height: thumb_h,
        },
        movable_track_height: movable_h,
        max_scroll_y,
    })
}

pub(crate) fn compute_max_scroll_y(inner: &ViewTreeInner, nid: NodeId) -> f32 {
    let Some(container) = inner.node(nid) else {
        return 0.0;
    };
    let top = container.world_rect.y;
    let vh = container.world_rect.height;
    if vh <= 0.0 {
        return 0.0;
    }
    let edges = container.style.layout.effective_border_edges();
    let trail_y = container.style.layout.padding.bottom.max(0.0) + edges.bottom.max(0.0);
    let mut max_bottom = top + vh;
    let mut stack = container.children.clone();
    while let Some(cid) = stack.pop() {
        let Some(ch) = inner.node(cid) else {
            continue;
        };
        if ch.is_anchor() {
            continue;
        }
        let child_mb = if ch.parent == Some(nid) {
            ch.style.layout.margin.bottom.max(0.0)
        } else {
            0.0
        };
        max_bottom = max_bottom.max(ch.world_rect.y + ch.world_rect.height + child_mb + trail_y);
        stack.extend_from_slice(&ch.children);
    }
    (max_bottom - (top + vh)).max(0.0)
}

pub(crate) fn compute_max_scroll_x(inner: &ViewTreeInner, nid: NodeId) -> f32 {
    let Some(container) = inner.node(nid) else {
        return 0.0;
    };
    let left = container.world_rect.x;
    let vw = container.world_rect.width;
    if vw <= 0.0 {
        return 0.0;
    }
    let edges = container.style.layout.effective_border_edges();
    let trail_x = container.style.layout.padding.right.max(0.0) + edges.right.max(0.0);
    let mut max_right = left + vw;
    let mut stack = container.children.clone();
    while let Some(cid) = stack.pop() {
        let Some(ch) = inner.node(cid) else {
            continue;
        };
        if ch.is_anchor() {
            continue;
        }
        let child_mr = if ch.parent == Some(nid) {
            ch.style.layout.margin.right.max(0.0)
        } else {
            0.0
        };
        max_right = max_right.max(ch.world_rect.x + ch.world_rect.width + child_mr + trail_x);
        stack.extend_from_slice(&ch.children);
    }
    (max_right - (left + vw)).max(0.0)
}

pub(crate) fn compute_textarea_max_scroll_y(inner: &mut ViewTreeInner, nid: NodeId) -> f32 {
    let Some((world_rect, style, text, cached_h)) = inner.node(nid).map(|n| {
        (
            n.world_rect,
            n.style,
            n.text.clone(),
            n.cached_text_layout.as_ref().map(|s| s.measured_height),
        )
    }) else {
        return 0.0;
    };
    if world_rect.height <= 0.0 {
        return 0.0;
    }
    let eff_b = style.layout.effective_border_edges();
    let inner_h = (world_rect.height
        - style.layout.padding.top
        - style.layout.padding.bottom
        - eff_b.top.max(0.0)
        - eff_b.bottom.max(0.0))
    .max(1.0);
    let measured_h = match cached_h {
        Some(h) => h,
        None if text.is_empty() => 0.0,
        None => {
            let cb = crate::view_tree::content_box::content_box_rect(world_rect, &style.layout);
            let max_w = (world_rect.width > 0.0).then(|| cb.width.max(1.0));
            inner
                .text_engine
                .measure_styled_text(&text, &style.text, max_w)
                .1
        }
    };
    (measured_h - inner_h).max(0.0)
}

pub(crate) fn try_start_scrollbar_pointer_down(
    inner: &mut ViewTreeInner,
    x: f32,
    y: f32,
) -> Option<Option<ScrollCallback>> {
    let nid = hit_test_scrollable_node(inner, x, y)?;
    let (aff, rect) = effective_node_world_matrix_and_rect(inner, nid);
    let (lx, ly) = aff.inverse()?.transform_point(x, y);
    let (scroll_y, max_scroll_y) = inner
        .scroll(nid)
        .map(|s| (s.scroll_y, s.cached_max_scroll_y))?;
    let sw = inner
        .node(nid)
        .map_or(ScrollbarWidth::Auto, |n| n.style.layout.scrollbar_width);
    let geom = compute_scrollbar_geometry(rect, scroll_y, max_scroll_y, sw)?;
    if !geom.hit_track_rect.contains(lx, ly) {
        return None;
    }
    if geom.hit_thumb_rect.contains(lx, ly) {
        let grab = (ly - geom.thumb_rect.y).clamp(0.0, geom.thumb_rect.height);
        inner.scrollbar_drag = Some((nid, grab));
        return Some(None);
    }
    let grab = geom.thumb_rect.height * 0.5;
    let ratio = ((ly - geom.track_top - grab) / geom.movable_track_height.max(1.0)).clamp(0.0, 1.0);
    let new_scroll = (ratio * geom.max_scroll_y).clamp(0.0, geom.max_scroll_y);
    inner.scrollbar_drag = Some((nid, grab));
    let mut dirty = false;
    {
        let sc = inner.ensure_scroll(nid);
        if (sc.scroll_y - new_scroll).abs() > 0.01 {
            sc.scroll_y = new_scroll;
            dirty = true;
        }
    }
    if dirty {
        inner.paint_dirty = true;
        Some(inner.listeners(nid).and_then(|l| l.on_scroll.clone()))
    } else {
        Some(None)
    }
}

pub(crate) fn apply_scrollbar_pointer_drag(
    inner: &mut ViewTreeInner,
    x: f32,
    y: f32,
) -> Option<(Option<ScrollCallback>, bool)> {
    let (nid, grab) = inner.scrollbar_drag?;
    let (aff, rect) = effective_node_world_matrix_and_rect(inner, nid);
    let (_lx, ly) = aff.inverse()?.transform_point(x, y);
    let (scroll_y, max_scroll_y) = inner
        .scroll(nid)
        .map(|s| (s.scroll_y, s.cached_max_scroll_y))?;
    let sw = inner
        .node(nid)
        .map_or(ScrollbarWidth::Auto, |n| n.style.layout.scrollbar_width);
    let geom = compute_scrollbar_geometry(rect, scroll_y, max_scroll_y, sw)?;
    let ratio = ((ly - geom.track_top - grab) / geom.movable_track_height.max(1.0)).clamp(0.0, 1.0);
    let new_scroll = (ratio * geom.max_scroll_y).clamp(0.0, geom.max_scroll_y);
    let mut dirty = false;
    {
        let sc = inner.ensure_scroll(nid);
        if (sc.scroll_y - new_scroll).abs() > 0.01 {
            sc.scroll_y = new_scroll;
            dirty = true;
        }
    }
    if dirty {
        inner.paint_dirty = true;
        let cb = inner.listeners(nid).and_then(|l| l.on_scroll.clone());
        Some((cb, true))
    } else {
        Some((None, false))
    }
}

fn find_scrollable_ancestor_or_default(inner: &ViewTreeInner) -> Option<NodeId> {
    let check_chain = |start: Option<NodeId>| -> Option<NodeId> {
        let mut cur = start;
        while let Some(nid) = cur {
            let n = inner.node(nid)?;
            let is_scroll = n.style.layout.is_scroll_container();
            let max_y = inner
                .scroll(nid)
                .map(|s| s.cached_max_scroll_y)
                .unwrap_or(0.0);
            if is_scroll && max_y > 0.0 {
                return Some(nid);
            }
            cur = n.parent;
        }
        None
    };
    let focused_nid = inner
        .a11y_engine
        .focused_id()
        .and_then(|id| NodeId::try_from(id).ok());
    if let Some(found) = check_chain(focused_nid).or_else(|| check_chain(inner.hovered_node)) {
        return Some(found);
    }
    let mut stack: Vec<NodeId> = inner.roots.iter().rev().copied().collect();
    while let Some(nid) = stack.pop() {
        let Some(n) = inner.node(nid) else {
            continue;
        };
        if n.style.layout.display == DisplayMode::None {
            continue;
        }
        let is_scroll = n.style.layout.is_scroll_container();
        let max_y = inner
            .scroll(nid)
            .map(|s| s.cached_max_scroll_y)
            .unwrap_or(0.0);
        if !n.is_anchor() && is_scroll && max_y > 0.0 {
            return Some(nid);
        }
        for &cid in n.children.iter().rev() {
            stack.push(cid);
        }
    }
    None
}

fn apply_scroll_step_on_node(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    remaining_delta: &mut f32,
    scroll_cbs: &mut Vec<ScrollCallback>,
) -> bool {
    let Some((role, is_disabled, is_scroll)) = inner.node(nid).map(|n| {
        (
            n.role,
            n.is_disabled(),
            n.style.layout.is_scroll_container(),
        )
    }) else {
        return false;
    };
    if role == SemanticRole::MultilineTextInput && !is_disabled {
        let max_scroll = compute_textarea_max_scroll_y(inner, nid);
        if max_scroll <= 0.0 {
            return false;
        }
        let ed = inner.ensure_text_edit(nid);
        let old_scroll = ed.scroll_y.clamp(0.0, max_scroll);
        let next_scroll = (old_scroll - *remaining_delta).clamp(0.0, max_scroll);
        let applied = next_scroll - old_scroll;
        if applied.abs() > 0.01 {
            ed.scroll_y = next_scroll;
            ed.needs_caret_scroll = false;
            *remaining_delta += applied;
            if let Some(cb) = inner.listeners(nid).and_then(|l| l.on_scroll.clone()) {
                scroll_cbs.push(cb);
            }
            return true;
        }
        return false;
    }
    if is_scroll {
        let max_scroll = inner
            .scroll(nid)
            .map(|s| s.cached_max_scroll_y)
            .unwrap_or(0.0);
        if max_scroll <= 0.0 {
            return false;
        }
        let sc = inner.ensure_scroll(nid);
        let old_scroll = sc.scroll_y.clamp(0.0, max_scroll);
        let next_scroll = (old_scroll - *remaining_delta).clamp(0.0, max_scroll);
        let applied = next_scroll - old_scroll;
        if applied.abs() > 0.01 {
            sc.scroll_y = next_scroll;
            *remaining_delta += applied;
            if let Some(cb) = inner.listeners(nid).and_then(|l| l.on_scroll.clone()) {
                scroll_cbs.push(cb);
            }
            return true;
        }
    }
    false
}

impl ViewTree {
    /// Attaches a scroll callback (`onscroll`) to `node_id`.
    pub fn set_on_scroll(&mut self, node_id: NodeId, callback: impl ClickTarget) {
        let cb = callback.into_click_fn();
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_some() {
            inner.ensure_listeners(node_id).on_scroll = Some(cb);
        }
    }

    /// Returns the current vertical scroll offset (`scroll_y` in logical px) of `node_id`.
    pub fn scroll_offset_y(&self, node_id: NodeId) -> Option<f32> {
        let inner = self.inner.borrow();
        let node = inner.node(node_id)?;
        if node.role == SemanticRole::MultilineTextInput {
            Some(inner.text_edit(node_id).map(|e| e.scroll_y).unwrap_or(0.0))
        } else {
            Some(inner.scroll(node_id).map(|s| s.scroll_y).unwrap_or(0.0))
        }
    }

    /// Dispatches mouse wheel scrolling (`delta_y` in pixels) with ancestor scroll chaining:
    /// checks any open Top-Layer `<select>` popup first, then walks from the deepest hit node up
    /// its ancestor chain passing unconsumed wheel delta to outer scroll containers.
    pub fn dispatch_scroll(&mut self, x: f32, y: f32, delta_y: f32) -> bool {
        self.inner.borrow_mut().touch.cancel_fling();
        self.dispatch_scroll_internal(x, y, delta_y)
    }

    pub(crate) fn dispatch_scroll_internal(&mut self, x: f32, y: f32, delta_y: f32) -> bool {
        if delta_y.abs() <= 0.001 {
            return false;
        }
        let (scroll_cbs, changed) = {
            let mut inner = self.inner.borrow_mut();
            if let Some(popup_handled) = try_handle_select_popup_scroll(&mut inner, x, y, delta_y) {
                return popup_handled;
            }
            let mut cur =
                hit_test_any_node(&inner, x, y).or_else(|| hit_test_scrollable_node(&inner, x, y));
            let mut remaining = delta_y;
            let mut cbs = Vec::new();
            let mut any_changed = false;
            while let Some(nid) = cur {
                if remaining.abs() <= 0.01 {
                    break;
                }
                if apply_scroll_step_on_node(&mut inner, nid, &mut remaining, &mut cbs) {
                    any_changed = true;
                }
                cur = inner.node(nid).and_then(|n| n.parent);
            }
            if any_changed {
                inner.paint_dirty = true;
            }
            (cbs, any_changed)
        };
        for cb in scroll_cbs {
            batch(|| cb());
        }
        changed
    }

    fn apply_keyboard_scroll_by(&mut self, page_mode: bool, down: bool) -> bool {
        let (scroll_cb, changed) = {
            let mut inner = self.inner.borrow_mut();
            let Some(nid) = find_scrollable_ancestor_or_default(&inner) else {
                return false;
            };
            let Some(rect_h) = inner.node(nid).map(|n| n.world_rect.height) else {
                return false;
            };
            let step = if page_mode {
                (rect_h * KEYBOARD_SCROLL_PAGE_FRACTION).max(KEYBOARD_SCROLL_LINE_STEP_PX)
            } else {
                KEYBOARD_SCROLL_LINE_STEP_PX
            };
            let signed_delta = if down { step } else { -step };
            let mut moved = false;
            {
                let sc = inner.ensure_scroll(nid);
                let next_scroll = (sc.scroll_y + signed_delta).clamp(0.0, sc.cached_max_scroll_y);
                if (sc.scroll_y - next_scroll).abs() > 0.01 {
                    sc.scroll_y = next_scroll;
                    moved = true;
                }
            }
            if moved {
                inner.paint_dirty = true;
                (inner.listeners(nid).and_then(|l| l.on_scroll.clone()), true)
            } else {
                (None, false)
            }
        };
        if let Some(cb) = scroll_cb {
            batch(|| cb());
        }
        changed
    }

    /// Scrolls the active scroll container down by one page (`PageDown`) without moving keyboard focus.
    pub fn dispatch_page_down(&mut self) -> bool {
        self.apply_keyboard_scroll_by(true, true)
    }

    /// Scrolls the active scroll container up by one page (`PageUp`) without moving keyboard focus.
    pub fn dispatch_page_up(&mut self) -> bool {
        self.apply_keyboard_scroll_by(true, false)
    }

    /// Scrolls the active scroll container down by one line (`ArrowDown`) without moving keyboard focus.
    pub fn dispatch_scroll_down(&mut self) -> bool {
        self.apply_keyboard_scroll_by(false, true)
    }

    /// Scrolls the active scroll container up by one line (`ArrowUp`) without moving keyboard focus.
    pub fn dispatch_scroll_up(&mut self) -> bool {
        self.apply_keyboard_scroll_by(false, false)
    }

    /// Programmatically sets `(scroll_x, scroll_y)` (clamped to `[0, max_scroll]`) on an
    /// `overflow-x`/`overflow-y` scroll container `node_id`.
    pub fn set_scroll_offsets(&mut self, node_id: NodeId, scroll_x: f32, scroll_y: f32) -> bool {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_none_or(|n| n.is_anchor()) {
            return false;
        }
        let max_x = compute_max_scroll_x(&inner, node_id);
        let max_y = compute_max_scroll_y(&inner, node_id);
        let sc = inner.ensure_scroll(node_id);
        sc.cached_max_scroll_x = max_x;
        sc.cached_max_scroll_y = max_y;
        let nx = scroll_x.clamp(0.0, max_x);
        let ny = scroll_y.clamp(0.0, max_y);
        if (sc.scroll_x - nx).abs() > 0.01 || (sc.scroll_y - ny).abs() > 0.01 {
            sc.scroll_x = nx;
            sc.scroll_y = ny;
            inner.paint_dirty = true;
            true
        } else {
            false
        }
    }
}
