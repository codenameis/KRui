//! Safe-area & software-keyboard (`IME`) viewport insets for mobile and notched displays.

use crate::view_tree::scroll::compute_max_scroll_y;
use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_layout::LayoutRect;

const IME_FOCUS_CLEARANCE_PX: f32 = 8.0;

/// Screen-edge safe-area and software keyboard obstruction insets in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewportInsets {
    /// Top system bar or display cutout inset (logical px).
    pub top: f32,
    /// Right system edge or display cutout inset (logical px).
    pub right: f32,
    /// Bottom navigation bar or home indicator inset (logical px).
    pub bottom: f32,
    /// Left system edge or display cutout inset (logical px).
    pub left: f32,
    /// Software keyboard (`IME`) height from the bottom edge of the viewport (logical px).
    pub ime_bottom: f32,
}

impl ViewportInsets {
    /// Returns the total bottom obstruction in logical pixels (`max(bottom, ime_bottom)`).
    pub fn bottom_obstruction(&self) -> f32 {
        self.bottom.max(self.ime_bottom).max(0.0)
    }
}

impl ViewTree {
    /// Returns the current [`ViewportInsets`] stored on this `ViewTree`.
    pub fn viewport_insets(&self) -> ViewportInsets {
        self.inner.borrow().viewport_insets
    }

    /// Updates safe-area and IME insets for `viewport_height` (logical px) and auto-scrolls
    /// any scrollable ancestor so a focused text input stays visible above `ime_bottom`.
    pub fn set_viewport_insets(&mut self, insets: ViewportInsets, viewport_height: f32) {
        let sanitized = ViewportInsets {
            top: insets.top.max(0.0),
            right: insets.right.max(0.0),
            bottom: insets.bottom.max(0.0),
            left: insets.left.max(0.0),
            ime_bottom: insets.ime_bottom.max(0.0),
        };
        let callbacks = {
            let mut inner = self.inner.borrow_mut();
            if inner.viewport_insets != sanitized {
                inner.viewport_insets = sanitized;
                inner.paint_dirty = true;
            }
            if sanitized.ime_bottom > 0.0 {
                scroll_focused_input_above_ime(&mut inner, sanitized, viewport_height)
            } else {
                clamp_over_scrolled_containers(&mut inner)
            }
        };
        for cb in callbacks {
            cb();
        }
    }
}

fn find_focused_input(inner: &ViewTreeInner) -> Option<(NodeId, LayoutRect, Option<NodeId>)> {
    let ak_id = inner.a11y_engine.focused_id()?;
    let nid = NodeId::try_from(ak_id).ok()?;
    let node = inner.node(nid)?;
    if !node.role.is_text_editable() {
        return None;
    }
    Some((nid, node.world_rect, node.parent))
}

fn find_nearest_scrollable_ancestor(
    inner: &ViewTreeInner,
    start_parent: Option<NodeId>,
) -> Option<(NodeId, f32)> {
    let mut nearest: Option<NodeId> = None;
    let mut total_scroll_y = 0.0_f32;
    let mut cur = start_parent;
    while let Some(anc_id) = cur {
        let Some(anc) = inner.node(anc_id) else {
            break;
        };
        if anc.style.layout.is_scroll_container() {
            if nearest.is_none() {
                nearest = Some(anc_id);
            }
            total_scroll_y += inner.scroll(anc_id).map(|s| s.scroll_y).unwrap_or(0.0);
        }
        cur = anc.parent;
    }
    nearest.map(|id| (id, total_scroll_y))
}

fn scroll_focused_input_above_ime(
    inner: &mut ViewTreeInner,
    insets: ViewportInsets,
    viewport_height: f32,
) -> Vec<std::rc::Rc<dyn Fn()>> {
    let Some((_, focused_rect, start_parent)) = find_focused_input(inner) else {
        return Vec::new();
    };
    let Some((anc_id, total_anc_scroll_y)) = find_nearest_scrollable_ancestor(inner, start_parent)
    else {
        return Vec::new();
    };
    let visible_bottom = (viewport_height - insets.bottom_obstruction()).max(0.0);
    let screen_bottom = focused_rect.y + focused_rect.height - total_anc_scroll_y;
    if screen_bottom <= visible_bottom {
        return Vec::new();
    }
    let needed = screen_bottom + IME_FOCUS_CLEARANCE_PX - visible_bottom;
    let max_scroll = compute_max_scroll_y(inner, anc_id) + insets.ime_bottom;
    let sc = inner.ensure_scroll(anc_id);
    sc.cached_max_scroll_y = max_scroll;
    let next_scroll = (sc.scroll_y + needed).clamp(0.0, max_scroll);
    if (sc.scroll_y - next_scroll).abs() <= 0.01 {
        return Vec::new();
    }
    sc.scroll_y = next_scroll;
    inner.paint_dirty = true;
    inner
        .listeners(anc_id)
        .and_then(|l| l.on_scroll.clone())
        .into_iter()
        .collect()
}

fn clamp_over_scrolled_containers(inner: &mut ViewTreeInner) -> Vec<std::rc::Rc<dyn Fn()>> {
    let scroll_ids: Vec<NodeId> = inner.sparse.scrolls.keys().copied().collect();
    let mut callbacks = Vec::new();
    for nid in scroll_ids {
        let natural_max = compute_max_scroll_y(inner, nid);
        let sc = inner.ensure_scroll(nid);
        sc.cached_max_scroll_y = natural_max;
        if sc.scroll_y > natural_max {
            sc.scroll_y = natural_max;
            inner.paint_dirty = true;
            if let Some(cb) = inner.listeners(nid).and_then(|l| l.on_scroll.clone()) {
                callbacks.push(cb);
            }
        }
    }
    callbacks
}
