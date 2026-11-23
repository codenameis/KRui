//! Reverse W3C Stacking Context and Top-Layer hit-testing paired with `stacking.rs`.

use super::{
    creates_stacking_context, is_flex_or_grid, is_top_layer_node,
    stacking_toplayer::collect_top_layer_entries, StackEnv,
};
use crate::view_tree::clip_shape::{compute_padding_box_clip, ClipState};
use crate::view_tree::hit_test::{
    compute_node_hit_geometry, node_matches_role, resolve_self_or_occluded_hit,
    subtree_scroll_and_clip, HitFilter, PointerHit,
};
use crate::view_tree::hit_test_geom::{point_inside_clip, point_inside_rounded_rect};
use crate::view_tree::pointer_vars::is_pseudo_node;
use crate::view_tree::semantics::is_node_semantically_hidden;
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_layout::LayoutRect;
use krui_style::{Affine2, DisplayMode, OverflowMode, PaintStyle, PositionMode};

struct HitScEntry {
    node_id: NodeId,
    z_index: i32,
    env: StackEnv,
}

enum HitStep5 {
    Sc(HitScEntry),
    PositionedNonSc(NodeId, StackEnv),
}

#[derive(Default)]
struct HitScParts {
    neg: Vec<HitScEntry>,
    zero_pos: Vec<HitStep5>,
    pos: Vec<HitScEntry>,
}

/// Finds the topmost matching node at `(x, y)` in exact reverse W3C CSS stacking + Top-Layer order.
pub(crate) fn hit_test_stacked_scene(
    inner: &ViewTreeInner,
    x: f32,
    y: f32,
    filter: HitFilter,
) -> Option<NodeId> {
    if let Some(top_res) = hit_test_top_layer(inner, (x, y), filter) {
        return top_res;
    }
    let root_env = StackEnv {
        scroll: (0.0, 0.0),
        clip: ClipState::NONE,
        affine: Affine2::IDENTITY,
        opacity: 1.0,
        inherited_color: None,
        parent_is_flex_or_grid: false,
        any_top_layer: true,
        any_neg_z: true,
    };
    for &root_id in inner.roots.iter().rev() {
        if is_top_layer_node(inner, root_id) {
            continue;
        }
        match hit_test_stacking_context(inner, root_id, root_env, true, false, (x, y), filter) {
            Some(PointerHit::Matched(hit)) => return Some(hit),
            Some(PointerHit::Occluded(_)) => return None,
            None => {}
        }
    }
    None
}

fn hit_test_top_layer(
    inner: &ViewTreeInner,
    pt: (f32, f32),
    filter: HitFilter,
) -> Option<Option<NodeId>> {
    let entries = collect_top_layer_entries(inner);
    for entry in entries.iter().rev() {
        let env = StackEnv {
            scroll: entry.scroll,
            clip: ClipState::NONE,
            affine: entry.affine,
            opacity: 1.0,
            inherited_color: None,
            parent_is_flex_or_grid: false,
            any_top_layer: true,
            any_neg_z: true,
        };
        match hit_test_stacking_context(inner, entry.node_id, env, true, true, pt, filter) {
            Some(PointerHit::Matched(hit)) => return Some(Some(hit)),
            Some(PointerHit::Occluded(_)) => return Some(None),
            None if entry.is_dialog && filter != HitFilter::AnyVisible => return Some(None),
            None => {}
        }
    }
    None
}

fn point_inside_node_clip_pad(
    pt: (f32, f32),
    clips_self: bool,
    world_rect: LayoutRect,
    eff_paint: &PaintStyle,
    layout: &krui_style::LayoutStyle,
    eff_transform: Affine2,
) -> bool {
    if !clips_self {
        return true;
    }
    let (pad_rect, pad_radii) = compute_padding_box_clip(world_rect, eff_paint, layout);
    let Some(inv) = eff_transform.inverse() else {
        return false;
    };
    let (lx, ly) = inv.transform_point(pt.0, pt.1);
    point_inside_rounded_rect(lx, ly, pad_rect, pad_radii)
}

struct ScHitPrepared {
    is_scroll: bool,
    is_shielding: bool,
    in_child_clip: bool,
    eff_transform: Affine2,
    world_rect: LayoutRect,
    eff_paint: PaintStyle,
    eff_clip: ClipState,
    eff_scroll: (f32, f32),
    self_scroll: (f32, f32),
    next_clip: ClipState,
    eff_opacity: f32,
    parent_is_flex_or_grid: bool,
}

fn prepare_sc_hit_node(
    inner: &ViewTreeInner,
    node_id: NodeId,
    env: StackEnv,
    is_top_layer: bool,
    pt: (f32, f32),
) -> Option<ScHitPrepared> {
    if !is_top_layer && is_top_layer_node(inner, node_id) {
        return None;
    }
    let n = inner.node(node_id)?;
    if !n.alive()
        || n.is_anchor()
        || n.style.layout.display == DisplayMode::None
        || is_pseudo_node(inner, node_id)
        || is_node_semantically_hidden(inner, node_id)
    {
        return None;
    }
    let (eff_scroll, eff_clip) = if n.style.layout.position == PositionMode::Fixed {
        ((0.0, 0.0), ClipState::NONE)
    } else if is_top_layer {
        (env.scroll, ClipState::NONE)
    } else {
        (env.scroll, env.clip)
    };
    let (eff_opacity, eff_transform, world_rect, eff_paint, is_shielding) =
        compute_node_hit_geometry(
            inner,
            n,
            node_id,
            eff_scroll,
            eff_clip.rect,
            env.affine,
            env.opacity,
        )?;
    let (is_scroll, self_scroll, next_clip) = subtree_scroll_and_clip(
        inner,
        n,
        node_id,
        world_rect,
        eff_paint,
        eff_transform,
        eff_clip,
    );
    let clips_self = is_scroll
        || n.style.layout.effective_overflow_y() == OverflowMode::Hidden
        || n.style.layout.effective_overflow_x() == OverflowMode::Hidden;
    let in_child_clip = point_inside_node_clip_pad(
        pt,
        clips_self,
        world_rect,
        &eff_paint,
        &n.style.layout,
        eff_transform,
    );
    Some(ScHitPrepared {
        is_scroll,
        is_shielding,
        in_child_clip,
        eff_transform,
        world_rect,
        eff_paint,
        eff_clip,
        eff_scroll,
        self_scroll,
        next_clip,
        eff_opacity,
        parent_is_flex_or_grid: is_flex_or_grid(n.style.layout.display),
    })
}

fn hit_test_stacking_context(
    inner: &ViewTreeInner,
    node_id: NodeId,
    env: StackEnv,
    _is_root: bool,
    is_top_layer: bool,
    pt: (f32, f32),
    filter: HitFilter,
) -> Option<PointerHit> {
    let prep = prepare_sc_hit_node(inner, node_id, env, is_top_layer, pt)?;
    let n = inner.node(node_id)?;
    let mut parts = HitScParts::default();
    if prep.in_child_clip {
        let child_env = StackEnv {
            scroll: (
                prep.eff_scroll.0 + prep.self_scroll.0,
                prep.eff_scroll.1 + prep.self_scroll.1,
            ),
            clip: prep.next_clip,
            affine: prep.eff_transform,
            opacity: prep.eff_opacity,
            inherited_color: None,
            parent_is_flex_or_grid: prep.parent_is_flex_or_grid,
            ..env
        };
        collect_hit_sc_parts(inner, &n.children, child_env, &mut parts);
    }
    resolve_sc_reverse_hit(
        inner,
        node_id,
        &n.children,
        prep.eff_clip,
        (prep.is_scroll, prep.is_shielding, prep.in_child_clip),
        (prep.eff_transform, prep.world_rect, &prep.eff_paint),
        (
            prep.eff_scroll,
            prep.self_scroll,
            prep.next_clip,
            prep.eff_opacity,
        ),
        &mut parts,
        pt,
        filter,
    )
}

#[allow(clippy::too_many_arguments)]
fn resolve_sc_reverse_hit(
    inner: &ViewTreeInner,
    node_id: NodeId,
    children: &[NodeId],
    eff_clip: ClipState,
    (is_scroll, is_shielding, in_child_clip): (bool, bool, bool),
    (eff_transform, world_rect, eff_paint): (Affine2, LayoutRect, &PaintStyle),
    (eff_scroll, self_scroll, next_clip, eff_opacity): ((f32, f32), (f32, f32), ClipState, f32),
    parts: &mut HitScParts,
    pt: (f32, f32),
    filter: HitFilter,
) -> Option<PointerHit> {
    let n = inner.node(node_id)?;
    parts.pos.sort_by_key(|e| e.z_index);
    for entry in parts.pos.iter().rev() {
        if let Some(h) =
            hit_test_stacking_context(inner, entry.node_id, entry.env, false, false, pt, filter)
        {
            return Some(h);
        }
    }
    for item in parts.zero_pos.iter().rev() {
        let hit = match item {
            HitStep5::Sc(e) => {
                hit_test_stacking_context(inner, e.node_id, e.env, false, false, pt, filter)
            }
            HitStep5::PositionedNonSc(nid, env) => {
                hit_test_non_sc_inflow_node(inner, *nid, *env, pt, filter)
            }
        };
        if hit.is_some() {
            return hit;
        }
    }
    let child_env = StackEnv {
        scroll: (eff_scroll.0 + self_scroll.0, eff_scroll.1 + self_scroll.1),
        clip: next_clip,
        affine: eff_transform,
        opacity: eff_opacity,
        inherited_color: None,
        parent_is_flex_or_grid: is_flex_or_grid(n.style.layout.display),
        any_top_layer: true,
        any_neg_z: true,
    };
    let mut inflow_occ = None;
    if in_child_clip {
        for &cid in children.iter().rev() {
            match hit_test_non_sc_inflow_child(inner, cid, child_env, pt, filter) {
                Some(PointerHit::Matched(m)) => return Some(PointerHit::Matched(m)),
                Some(PointerHit::Occluded(o)) if inflow_occ.is_none() => inflow_occ = Some(o),
                _ => {}
            }
        }
    }
    let inside_self = point_inside_self_box(pt, eff_clip, eff_transform, world_rect, eff_paint);
    if inside_self && node_matches_role(inner, n, node_id, filter, is_scroll) {
        return Some(PointerHit::Matched(node_id));
    }
    if let Some(occ) = inflow_occ {
        return Some(PointerHit::Occluded(occ));
    }
    parts.neg.sort_by_key(|e| e.z_index);
    for entry in parts.neg.iter().rev() {
        if let Some(h) =
            hit_test_stacking_context(inner, entry.node_id, entry.env, false, false, pt, filter)
        {
            return Some(h);
        }
    }
    (inside_self && is_shielding).then_some(PointerHit::Occluded(node_id))
}

fn point_inside_self_box(
    pt: (f32, f32),
    clip: ClipState,
    transform: Affine2,
    world_rect: LayoutRect,
    eff_paint: &PaintStyle,
) -> bool {
    if !point_inside_clip(pt.0, pt.1, clip) {
        return false;
    }
    let Some(inv) = transform.inverse() else {
        return false;
    };
    let (lx, ly) = inv.transform_point(pt.0, pt.1);
    point_inside_rounded_rect(
        lx,
        ly,
        world_rect,
        eff_paint.effective_corner_radii().to_array(),
    )
}

fn collect_hit_sc_parts(
    inner: &ViewTreeInner,
    children: &[NodeId],
    env: StackEnv,
    parts: &mut HitScParts,
) {
    for &cid in children {
        if is_top_layer_node(inner, cid) || is_node_semantically_hidden(inner, cid) {
            continue;
        }
        let Some(n) = inner.node(cid) else { continue };
        if !n.alive() || is_pseudo_node(inner, cid) {
            continue;
        }
        if n.is_anchor() || n.style.layout.display == DisplayMode::Contents {
            if n.is_anchor() || n.style.layout.display == DisplayMode::Contents {
                collect_hit_sc_parts(inner, &n.children, env, parts);
            }
            continue;
        }
        if n.style.layout.display == DisplayMode::None {
            continue;
        }
        let eff_scroll = if n.style.layout.position == PositionMode::Fixed {
            (0.0, 0.0)
        } else {
            env.scroll
        };
        let eff_clip = if n.style.layout.position == PositionMode::Fixed {
            ClipState::NONE
        } else {
            env.clip
        };
        let Some((eff_op, eff_tf, w_rect, eff_paint, _)) = compute_node_hit_geometry(
            inner,
            n,
            cid,
            eff_scroll,
            eff_clip.rect,
            env.affine,
            env.opacity,
        ) else {
            continue;
        };
        if creates_stacking_context(n, &eff_paint, false, false, env.parent_is_flex_or_grid) {
            let entry = HitScEntry {
                node_id: cid,
                z_index: n.style.layout.z_index,
                env,
            };
            if n.style.layout.has_z_index && entry.z_index < 0 {
                parts.neg.push(entry);
            } else if n.style.layout.has_z_index && entry.z_index > 0 {
                parts.pos.push(entry);
            } else {
                parts.zero_pos.push(HitStep5::Sc(entry));
            }
            continue;
        }
        if n.style.layout.position != PositionMode::Static {
            parts.zero_pos.push(HitStep5::PositionedNonSc(cid, env));
        }
        let (is_scroll, self_scroll, next_clip) =
            subtree_scroll_and_clip(inner, n, cid, w_rect, eff_paint, eff_tf, eff_clip);
        let next_env = StackEnv {
            scroll: if is_scroll {
                (eff_scroll.0 + self_scroll.0, eff_scroll.1 + self_scroll.1)
            } else {
                eff_scroll
            },
            clip: next_clip,
            affine: eff_tf,
            opacity: eff_op,
            inherited_color: None,
            parent_is_flex_or_grid: is_flex_or_grid(n.style.layout.display),
            ..env
        };
        collect_hit_sc_parts(inner, &n.children, next_env, parts);
    }
}

fn hit_test_non_sc_inflow_child(
    inner: &ViewTreeInner,
    nid: NodeId,
    env: StackEnv,
    pt: (f32, f32),
    filter: HitFilter,
) -> Option<PointerHit> {
    if is_top_layer_node(inner, nid) || is_node_semantically_hidden(inner, nid) {
        return None;
    }
    let n = inner.node(nid)?;
    if !n.alive()
        || n.is_anchor()
        || n.style.layout.display == DisplayMode::None
        || is_pseudo_node(inner, nid)
    {
        return None;
    }
    if n.style.layout.display == DisplayMode::Contents {
        return hit_test_non_sc_list_rev(inner, &n.children, env, pt, filter);
    }
    if n.style.layout.position != PositionMode::Static {
        return None;
    }
    let eff_paint = inner.active_transition_paint(
        nid,
        crate::view_tree::paint::resolve_node_paint_state(inner, nid, n, n.is_disabled(), false),
    );
    if creates_stacking_context(n, &eff_paint, false, false, env.parent_is_flex_or_grid) {
        return None;
    }
    hit_test_non_sc_inflow_node(inner, nid, env, pt, filter)
}

fn hit_test_non_sc_list_rev(
    inner: &ViewTreeInner,
    kids: &[NodeId],
    env: StackEnv,
    pt: (f32, f32),
    filter: HitFilter,
) -> Option<PointerHit> {
    let mut occ = None;
    for &cid in kids.iter().rev() {
        match hit_test_non_sc_inflow_child(inner, cid, env, pt, filter) {
            Some(PointerHit::Matched(m)) => return Some(PointerHit::Matched(m)),
            Some(PointerHit::Occluded(o)) if occ.is_none() => occ = Some(o),
            _ => {}
        }
    }
    occ.map(PointerHit::Occluded)
}

fn hit_test_non_sc_inflow_node(
    inner: &ViewTreeInner,
    nid: NodeId,
    env: StackEnv,
    pt: (f32, f32),
    filter: HitFilter,
) -> Option<PointerHit> {
    let n = inner.node(nid)?;
    let (eff_op, eff_tf, w_rect, eff_paint, is_shielding) = compute_node_hit_geometry(
        inner,
        n,
        nid,
        env.scroll,
        env.clip.rect,
        env.affine,
        env.opacity,
    )?;
    let (is_scroll, self_scroll, next_clip) =
        subtree_scroll_and_clip(inner, n, nid, w_rect, eff_paint, eff_tf, env.clip);
    let clips_self = is_scroll
        || n.style.layout.effective_overflow_y() == OverflowMode::Hidden
        || n.style.layout.effective_overflow_x() == OverflowMode::Hidden;
    let in_child_clip =
        point_inside_node_clip_pad(pt, clips_self, w_rect, &eff_paint, &n.style.layout, eff_tf);
    let child_occ = if in_child_clip {
        let next_env = StackEnv {
            scroll: (env.scroll.0 + self_scroll.0, env.scroll.1 + self_scroll.1),
            clip: next_clip,
            affine: eff_tf,
            opacity: eff_op,
            inherited_color: None,
            parent_is_flex_or_grid: is_flex_or_grid(n.style.layout.display),
            ..env
        };
        match hit_test_non_sc_list_rev(inner, &n.children, next_env, pt, filter) {
            Some(PointerHit::Matched(m)) => return Some(PointerHit::Matched(m)),
            Some(PointerHit::Occluded(o)) => Some(o),
            None => None,
        }
    } else {
        None
    };
    resolve_self_or_occluded_hit(
        inner,
        n,
        nid,
        pt,
        filter,
        (is_scroll, is_shielding, child_occ),
        (env.clip, eff_tf, w_rect, &eff_paint),
    )
}
