//! 4-way D-pad and rotary bezel spatial focus navigation (`kiosk-spi`, `watch-wear`).

use crate::NodeId;
use krui_layout::LayoutRect;

/// Directional or sequential focus navigation command for rotary bezels, D-pads, and encoder wheels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialNavDirection {
    /// Move focus to the nearest candidate above the current node.
    Up,
    /// Move focus to the nearest candidate below the current node.
    Down,
    /// Move focus to the nearest candidate left of the current node.
    Left,
    /// Move focus to the nearest candidate right of the current node.
    Right,
    /// Advance focus clockwise / forward along logical tab order (wraps around).
    Next,
    /// Move focus counter-clockwise / backward along logical tab order (wraps around).
    Prev,
}

/// Focusable widget candidate with its resolved bounding rect and optional tab-order hint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusableCandidate {
    /// Target focusable node id.
    pub node: NodeId,
    /// Computed global bounding rectangle in logical pixels.
    pub rect: LayoutRect,
    /// Logical sort key (`tabindex` or DOM order) used by `Next` / `Prev` rotation.
    pub order_hint: usize,
}

/// Computes the next focused [`NodeId`] in `dir`, or returns `None` if no valid target exists.
pub fn next_spatial_focus(
    candidates: &[FocusableCandidate],
    current: Option<NodeId>,
    dir: SpatialNavDirection,
) -> Option<NodeId> {
    if candidates.is_empty() {
        return None;
    }
    match dir {
        SpatialNavDirection::Next | SpatialNavDirection::Prev => {
            step_rotary_focus(candidates, current, dir == SpatialNavDirection::Next)
        }
        _ => step_directional_focus(candidates, current, dir),
    }
}

fn step_rotary_focus(
    candidates: &[FocusableCandidate],
    current: Option<NodeId>,
    forward: bool,
) -> Option<NodeId> {
    let mut order: Vec<(usize, usize)> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (c.order_hint, i))
        .collect();
    order.sort_unstable();
    let cur_pos = current.and_then(|id| {
        order
            .iter()
            .position(|&(_, idx)| candidates[idx].node == id)
    });
    let next_pos = match cur_pos {
        None => {
            if forward {
                0
            } else {
                order.len() - 1
            }
        }
        Some(pos) if forward => (pos + 1) % order.len(),
        Some(pos) => (pos + order.len() - 1) % order.len(),
    };
    Some(candidates[order[next_pos].1].node)
}

fn step_directional_focus(
    candidates: &[FocusableCandidate],
    current: Option<NodeId>,
    dir: SpatialNavDirection,
) -> Option<NodeId> {
    let cur_idx = current.and_then(|id| candidates.iter().position(|c| c.node == id));
    let Some(cur_idx) = cur_idx else {
        return Some(candidates[0].node);
    };
    let (cx, cy) = rect_center(candidates[cur_idx].rect);
    let mut best: Option<(f32, NodeId)> = None;
    for (idx, cand) in candidates.iter().enumerate() {
        if idx == cur_idx {
            continue;
        }
        let (tx, ty) = rect_center(cand.rect);
        let (primary, perp) = match dir {
            SpatialNavDirection::Up => (cy - ty, (tx - cx).abs()),
            SpatialNavDirection::Down => (ty - cy, (tx - cx).abs()),
            SpatialNavDirection::Left => (cx - tx, (ty - cy).abs()),
            SpatialNavDirection::Right => (tx - cx, (ty - cy).abs()),
            SpatialNavDirection::Next | SpatialNavDirection::Prev => continue,
        };
        if primary <= 0.5 {
            continue;
        }
        let score = primary * primary + 6.25 * perp * perp;
        if best.is_none_or(|(best_score, _)| score < best_score) {
            best = Some((score, cand.node));
        }
    }
    best.map(|(_, node)| node)
}

#[inline]
fn rect_center(r: LayoutRect) -> (f32, f32) {
    (r.x + r.width * 0.5, r.y + r.height * 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_cand(raw: u32, x: f32, y: f32, order_hint: usize) -> FocusableCandidate {
        FocusableCandidate {
            node: NodeId::new(raw, 1),
            rect: LayoutRect {
                x,
                y,
                width: 80.0,
                height: 40.0,
            },
            order_hint,
        }
    }

    #[test]
    fn directional_and_rotary_focus_navigation() {
        let a = mk_cand(1, 0.0, 0.0, 0);
        let b = mk_cand(2, 100.0, 0.0, 1);
        let c = mk_cand(3, 0.0, 60.0, 2);
        let d = mk_cand(4, 100.0, 60.0, 3);
        let grid = [a, b, c, d];

        assert_eq!(
            next_spatial_focus(&grid, Some(a.node), SpatialNavDirection::Right),
            Some(b.node)
        );
        assert_eq!(
            next_spatial_focus(&grid, Some(a.node), SpatialNavDirection::Down),
            Some(c.node)
        );
        assert_eq!(
            next_spatial_focus(&grid, Some(d.node), SpatialNavDirection::Up),
            Some(b.node)
        );
        assert_eq!(
            next_spatial_focus(&grid, Some(d.node), SpatialNavDirection::Left),
            Some(c.node)
        );
        assert_eq!(
            next_spatial_focus(&grid, Some(d.node), SpatialNavDirection::Next),
            Some(a.node)
        );
        assert_eq!(
            next_spatial_focus(&grid, Some(a.node), SpatialNavDirection::Prev),
            Some(d.node)
        );
    }
}
