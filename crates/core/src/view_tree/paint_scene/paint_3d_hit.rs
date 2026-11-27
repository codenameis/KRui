//! Wave 4C CSS 3D hit-testing and inverse-homography helpers paired with `paint_3d.rs`.

use super::paint_3d::{
    compute_child_3d_context, compute_node_3d_mat, evaluate_perspective_layer, PerspectiveEval,
};
use super::stacking::{is_top_layer_node, StackEnv};
use crate::view_tree::clip_path_resolve::{point_in_resolved_clip_path, resolve_clip_path};
use crate::view_tree::clip_shape::{compute_padding_box_clip, intersect_clip_states, ClipState};
use crate::view_tree::hit_test::{node_matches_role, transform_rect_aabb, HitFilter};
use crate::view_tree::hit_test_geom::{point_inside_clip, point_inside_rounded_rect};
use crate::view_tree::paint::resolve_node_paint_state;
use crate::view_tree::{A11yId, NodeId, ViewTreeInner};

/// Walks ancestors of occluded node `occ` to find the nearest visible matching node for `filter`.
pub(super) fn resolve_occluded_ancestor(
    inner: &ViewTreeInner,
    occ: NodeId,
    filter: HitFilter,
) -> Option<NodeId> {
    let mut cur = inner.node(occ).and_then(|n| n.parent);
    while let Some(cid) = cur {
        let anc = inner.node(cid)?;
        if anc.alive()
            && !anc.is_anchor()
            && !anc.is_visibility_hidden()
            && node_matches_role(
                inner,
                anc,
                cid,
                filter,
                anc.style.layout.is_scroll_container(),
            )
        {
            return Some(cid);
        }
        cur = anc.parent;
    }
    None
}
use krui_layout::LayoutRect;
use krui_style::{inverse_project_point, Affine2, DisplayMode, Mat4, PaintStyle, PositionMode};

/// Precomputed 3D hit-test state attached to [`super::stacking_hit::ScHitPrepared`].
#[derive(Clone, Copy)]
pub(crate) struct ScHit3d {
    pub(crate) child_3d: Option<Mat4>,
    pub(crate) inv_h: Option<[f32; 9]>,
    pub(crate) self_culled: bool,
    pub(crate) culled_all: bool,
    pub(crate) preserve_3d: bool,
    pub(crate) projected: bool,
}

/// Evaluates 3D hit state for `prepare_sc_hit_node`.
pub(super) fn eval_sc_hit_3d(
    env: StackEnv,
    eff_paint: &PaintStyle,
    world_rect: LayoutRect,
    eff_transform: Affine2,
) -> ScHit3d {
    let preserve_3d = eff_paint.standalone_transform.preserve_3d;
    match evaluate_perspective_layer(env.parent_3d, env.affine, eff_paint, world_rect) {
        PerspectiveEval::CulledBackface => ScHit3d {
            child_3d: if preserve_3d {
                let m = compute_node_3d_mat(env.parent_3d, env.affine, eff_paint, world_rect);
                compute_child_3d_context(m, false, eff_paint, world_rect, eff_transform)
            } else {
                None
            },
            inv_h: None,
            self_culled: true,
            culled_all: !preserve_3d,
            preserve_3d,
            projected: false,
        },
        PerspectiveEval::Project {
            combined_3d, quad, ..
        } => ScHit3d {
            child_3d: compute_child_3d_context(
                combined_3d,
                !preserve_3d,
                eff_paint,
                world_rect,
                eff_transform,
            ),
            inv_h: Some(quad.inv_homography),
            self_culled: false,
            culled_all: false,
            preserve_3d,
            projected: true,
        },
        PerspectiveEval::Flat2D { combined_3d } => ScHit3d {
            child_3d: compute_child_3d_context(
                combined_3d,
                false,
                eff_paint,
                world_rect,
                eff_transform,
            ),
            inv_h: None,
            self_culled: false,
            culled_all: false,
            preserve_3d,
            projected: false,
        },
    }
}

/// Inverse-projects screen `(sx, sy)` through an element's 3D homography (if any) and tests box/clip-path.
pub(crate) fn point_inside_box_with_3d(
    pt: (f32, f32),
    clip: ClipState,
    eff_transform: Affine2,
    inv_homography: Option<[f32; 9]>,
    world_rect: LayoutRect,
    eff_paint: &PaintStyle,
) -> bool {
    if !point_inside_clip(pt.0, pt.1, clip) {
        return false;
    }
    if let Some(cp) = eff_paint.clip_path {
        if !point_in_resolved_clip_path(
            pt.0,
            pt.1,
            &resolve_clip_path(cp, world_rect, eff_transform),
        ) {
            return false;
        }
    }
    let local = if let Some(inv_h) = inv_homography {
        inverse_project_point(&inv_h, pt.0, pt.1)
    } else {
        eff_transform
            .inverse()
            .map(|inv| inv.transform_point(pt.0, pt.1))
    };
    let Some((lx, ly)) = local else {
        return false;
    };
    point_inside_rounded_rect(
        lx,
        ly,
        world_rect,
        eff_paint.effective_corner_radii().to_array(),
    )
}

#[derive(Clone, Copy)]
struct AncestorCursor {
    scroll: (f32, f32),
    aff: Affine2,
    p3d: Option<Mat4>,
    clip: ClipState,
}

fn step_ancestor_3d(
    inner: &ViewTreeInner,
    cid: NodeId,
    is_target: bool,
    pt: (f32, f32),
    cur: &mut AncestorCursor,
) -> Option<bool> {
    let n = inner.node(cid)?;
    if n.style.layout.display == DisplayMode::Contents {
        return (!is_target).then_some(false).or(Some(false));
    }
    if n.style.layout.position == PositionMode::Fixed {
        *cur = AncestorCursor {
            scroll: (0.0, 0.0),
            aff: cur.aff,
            p3d: None,
            clip: ClipState::NONE,
        };
    } else if is_top_layer_node(inner, cid) {
        cur.clip = ClipState::NONE;
    }
    let dis = n.is_disabled();
    let foc = inner.a11y_engine.focused_id() == Some(A11yId::from(cid).0) && !dis;
    let ep = inner.active_transition_paint(cid, resolve_node_paint_state(inner, cid, n, dis, foc));
    let mut wr = n.world_rect;
    (wr.x, wr.y) = (wr.x - cur.scroll.0, wr.y - cur.scroll.1);
    let (wr, sticky_d) = super::sticky_pos::resolve_sticky_world_rect(inner, cid, cur.scroll, wr);
    cur.scroll = (cur.scroll.0 - sticky_d.0, cur.scroll.1 - sticky_d.1);
    let eval = evaluate_perspective_layer(cur.p3d, cur.aff, &ep, wr);
    let next_aff = cur.aff * ep.effective_affine_for_rect(wr.x, wr.y, wr.width, wr.height);
    if is_target {
        let inv_h = match eval {
            PerspectiveEval::CulledBackface => return Some(false),
            PerspectiveEval::Project { quad, .. } => Some(quad.inv_homography),
            PerspectiveEval::Flat2D { .. } => None,
        };
        return Some(point_inside_box_with_3d(
            pt, cur.clip, next_aff, inv_h, wr, &ep,
        ));
    }
    let projected = matches!(eval, PerspectiveEval::Project { .. });
    let combined = match eval {
        PerspectiveEval::CulledBackface if !ep.standalone_transform.preserve_3d => return None,
        PerspectiveEval::CulledBackface => compute_node_3d_mat(cur.p3d, cur.aff, &ep, wr),
        PerspectiveEval::Flat2D { combined_3d } | PerspectiveEval::Project { combined_3d, .. } => {
            combined_3d
        }
    };
    if n.style.layout.clips_overflow() {
        let (pad_rect, pad_radii) = compute_padding_box_clip(wr, &ep, &n.style.layout);
        cur.clip =
            intersect_clip_states(cur.clip, transform_rect_aabb(next_aff, pad_rect), pad_radii);
    }
    let p3d_flat = projected && !ep.standalone_transform.preserve_3d;
    cur.p3d = compute_child_3d_context(combined, p3d_flat, &ep, wr, next_aff);
    cur.aff = if p3d_flat {
        Affine2::IDENTITY
    } else {
        next_aff
    };
    if n.style.layout.is_scroll_container() {
        if let Some(sc) = inner.scroll(cid) {
            cur.scroll = (cur.scroll.0 + sc.scroll_x, cur.scroll.1 + sc.scroll_y);
        }
    }
    None
}

/// Evaluates cumulative ancestor 3D/2D transform for `node_id` (used by `hit_test::node_contains_screen_point`).
pub(crate) fn node_contains_point_3d(
    inner: &ViewTreeInner,
    node_id: NodeId,
    x: f32,
    y: f32,
) -> bool {
    let mut chain = Vec::with_capacity(8);
    let mut ptr = Some(node_id);
    while let Some(cid) = ptr {
        let Some(n) = inner.node(cid) else { break };
        chain.push(cid);
        ptr = n.parent;
    }
    chain.reverse();
    let mut cur = AncestorCursor {
        scroll: (0.0, 0.0),
        aff: Affine2::IDENTITY,
        p3d: None,
        clip: ClipState::NONE,
    };
    for (idx, &cid) in chain.iter().enumerate() {
        let is_target = idx + 1 == chain.len();
        if inner
            .node(cid)
            .is_some_and(|n| n.style.layout.display == DisplayMode::Contents)
        {
            continue;
        }
        if let Some(hit) = step_ancestor_3d(inner, cid, is_target, (x, y), &mut cur) {
            return hit;
        }
    }
    false
}
