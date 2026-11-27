//! Zero-relayout W3C CSS `position: sticky` scrollport clamping shared by `PaintScene`
//! assembly and all reverse hit-testing paths (`hit_test.rs`, `stacking_hit.rs`, `paint_3d_hit.rs`).

use crate::view_tree::content_box::{content_box_rect, text_clip_box_rect};
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_layout::LayoutRect;
use krui_style::{Dimension, DisplayMode, InsetEdges, PositionMode};

/// Clamps `scrolled_rect` (`node.world_rect - scroll_offset`) for `position: sticky` elements
/// against their nearest scrollport and containing block, returning `(clamped_rect, sticky_delta)`.
///
/// Children of a sticky node subtract `sticky_delta` from their inherited `scroll_offset` so the
/// entire sticky subtree moves together with 0 Taffy relayouts.
pub(crate) fn resolve_sticky_world_rect(
    inner: &ViewTreeInner,
    node_id: NodeId,
    scroll_offset: (f32, f32),
    scrolled_rect: LayoutRect,
) -> (LayoutRect, (f32, f32)) {
    let Some(n) = inner.node(node_id) else {
        return (scrolled_rect, (0.0, 0.0));
    };
    if n.style.layout.position != PositionMode::Sticky {
        return (scrolled_rect, (0.0, 0.0));
    }
    let inset = n.style.layout.inset;
    if matches!(
        (inset.top, inset.right, inset.bottom, inset.left),
        (
            Dimension::Auto,
            Dimension::Auto,
            Dimension::Auto,
            Dimension::Auto
        )
    ) {
        return (scrolled_rect, (0.0, 0.0));
    }
    let (cb_id, sc_id) = find_containing_and_scroll_ancestors(inner, n.parent);
    let scrollport = resolve_scrollport_rect(inner, sc_id, scroll_offset);
    let cb_box = resolve_containing_block_rect(inner, cb_id, sc_id, scroll_offset);
    let clamped = clamp_rect_to_sticky_bounds(scrolled_rect, inset, scrollport, cb_box);
    let delta = (clamped.x - scrolled_rect.x, clamped.y - scrolled_rect.y);
    (clamped, delta)
}

fn find_containing_and_scroll_ancestors(
    inner: &ViewTreeInner,
    mut cur: Option<NodeId>,
) -> (Option<NodeId>, Option<NodeId>) {
    let mut cb_id = None;
    let mut sc_id = None;
    while let Some(aid) = cur {
        let Some(anc) = inner.node(aid) else {
            break;
        };
        if !anc.is_anchor() && anc.style.layout.display != DisplayMode::Contents {
            if cb_id.is_none() {
                cb_id = Some(aid);
            }
            if anc.style.layout.is_scroll_container() {
                sc_id = Some(aid);
                break;
            }
        }
        cur = anc.parent;
    }
    (cb_id, sc_id)
}

fn resolve_scrollport_rect(
    inner: &ViewTreeInner,
    sc_id: Option<NodeId>,
    scroll_offset: (f32, f32),
) -> LayoutRect {
    if let Some(sid) = sc_id {
        if let Some(sc_node) = inner.node(sid) {
            let self_sc = inner
                .scroll(sid)
                .map_or((0.0, 0.0), |s| (s.scroll_x, s.scroll_y));
            let mut wr = sc_node.world_rect;
            wr.x -= scroll_offset.0 - self_sc.0;
            wr.y -= scroll_offset.1 - self_sc.1;
            return text_clip_box_rect(wr, &sc_node.style.layout, 0.0);
        }
    }
    LayoutRect {
        x: 0.0,
        y: 0.0,
        width: inner.last_viewport.0.max(1.0),
        height: inner.last_viewport.1.max(1.0),
    }
}

fn resolve_containing_block_rect(
    inner: &ViewTreeInner,
    cb_id: Option<NodeId>,
    sc_id: Option<NodeId>,
    scroll_offset: (f32, f32),
) -> Option<LayoutRect> {
    let cid = cb_id.filter(|c| Some(*c) != sc_id)?;
    let cb_node = inner.node(cid)?;
    let mut wr = cb_node.world_rect;
    wr.x -= scroll_offset.0;
    wr.y -= scroll_offset.1;
    Some(content_box_rect(wr, &cb_node.style.layout))
}

fn resolve_inset_opt(dim: Dimension, basis: f32) -> Option<f32> {
    match dim {
        Dimension::Px(v) => Some(v),
        Dimension::Percent(p) => Some(basis * (p * 0.01)),
        Dimension::Auto => None,
    }
}

fn clamp_rect_to_sticky_bounds(
    mut rect: LayoutRect,
    inset: InsetEdges,
    sp: LayoutRect,
    cb: Option<LayoutRect>,
) -> LayoutRect {
    if let Some(top_off) = resolve_inset_opt(inset.top, sp.height) {
        let min_y = sp.y + top_off;
        if rect.y < min_y {
            rect.y = match cb {
                Some(c) => min_y.min((c.y + c.height - rect.height).max(c.y)),
                None => min_y,
            };
        }
    } else if let Some(bot_off) = resolve_inset_opt(inset.bottom, sp.height) {
        let max_y = sp.y + sp.height - bot_off - rect.height;
        if rect.y > max_y {
            rect.y = match cb {
                Some(c) => max_y.max(c.y),
                None => max_y,
            };
        }
    }
    if let Some(left_off) = resolve_inset_opt(inset.left, sp.width) {
        let min_x = sp.x + left_off;
        if rect.x < min_x {
            rect.x = match cb {
                Some(c) => min_x.min((c.x + c.width - rect.width).max(c.x)),
                None => min_x,
            };
        }
    } else if let Some(right_off) = resolve_inset_opt(inset.right, sp.width) {
        let max_x = sp.x + sp.width - right_off - rect.width;
        if rect.x > max_x {
            rect.x = match cb {
                Some(c) => max_x.max(c.x),
                None => max_x,
            };
        }
    }
    rect
}
