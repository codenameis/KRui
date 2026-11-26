//! Wave 4C CSS 3D transforms (`perspective`, `perspective-origin`, `transform-style: preserve-3d`,
//! `backface-visibility`) painting and depth-sort helpers.

use super::stacking::{emit_stacking_context, is_top_layer_node, StackEnv};
use crate::scene::{DrawPrimitive, PaintScene, ResolvedClipPath};
use crate::view_tree::clip_shape::ClipState;
use crate::view_tree::paint::{
    emit_backdrop_blur_prelayer, emit_node_self_box_primitives, emit_node_self_content_primitives,
    node_needs_group_opacity, resolve_node_paint_state, NodePaintFrame,
};
use crate::view_tree::semantics::is_node_semantically_hidden;
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_layout::LayoutRect;
use krui_style::{
    compute_projective_quad, mat4_perspective_origin, Affine2, DisplayMode, Mat4, PaintStyle,
    PositionMode, ProjectiveQuad,
};

/// Result of evaluating a node's 4x4 projective transform state for paint or hit-testing.
pub(crate) enum PerspectiveEval {
    Flat2D {
        combined_3d: Mat4,
    },
    CulledBackface,
    Project {
        combined_3d: Mat4,
        bounds: LayoutRect,
        quad: ProjectiveQuad,
    },
}

/// Control-flow decision returned to `emit_stacking_context` after evaluating 3D & layer state.
pub(crate) enum Sc3dEnter {
    Handled,
    Continue {
        child_3d: Option<Mat4>,
        pushed_layer: bool,
    },
}

#[inline]
pub(crate) fn affine2_to_mat4(a: Affine2) -> Mat4 {
    Mat4::from_matrix2d([a.a, a.b, a.c, a.d, a.tx, a.ty])
}

#[inline]
pub(crate) fn planar_has_perspective(m: Mat4) -> bool {
    let v = m.0;
    v[3].abs() > 1e-6 || v[7].abs() > 1e-6 || (v[15] - 1.0).abs() > 1e-5
}

#[inline]
pub(crate) fn compute_node_3d_mat(
    parent_3d: Option<Mat4>,
    parent_affine: Affine2,
    paint: &PaintStyle,
    wr: LayoutRect,
) -> Mat4 {
    let base = parent_3d.unwrap_or_else(|| affine2_to_mat4(parent_affine));
    base * paint.effective_mat4_for_rect(wr.x, wr.y, wr.width, wr.height)
}

/// Evaluates whether a node requires offscreen 3D projective warping, backface culling, or stays 2D flat.
pub(crate) fn evaluate_perspective_layer(
    parent_3d: Option<Mat4>,
    parent_affine: Affine2,
    paint: &PaintStyle,
    wr: LayoutRect,
) -> PerspectiveEval {
    let combined = compute_node_3d_mat(parent_3d, parent_affine, paint, wr);
    if !planar_has_perspective(combined) {
        if !paint.standalone_transform.backface_visible {
            let det = combined.0[0] * combined.0[5] - combined.0[1] * combined.0[4];
            if det <= 0.0 {
                return PerspectiveEval::CulledBackface;
            }
        }
        return PerspectiveEval::Flat2D {
            combined_3d: combined,
        };
    }
    let Some(quad) = compute_projective_quad(combined, wr.x, wr.y, wr.width, wr.height) else {
        return PerspectiveEval::CulledBackface;
    };
    if !paint.standalone_transform.backface_visible && quad.is_backface {
        return PerspectiveEval::CulledBackface;
    }
    let bounds = LayoutRect {
        x: quad.screen_aabb[0],
        y: quad.screen_aabb[1],
        width: (quad.screen_aabb[2] - quad.screen_aabb[0]).max(0.0),
        height: (quad.screen_aabb[3] - quad.screen_aabb[1]).max(0.0),
    };
    PerspectiveEval::Project {
        combined_3d: combined,
        bounds,
        quad,
    }
}

/// Computes `parent_3d` context handed to direct children of a container (`perspective` / `preserve-3d`).
pub(crate) fn compute_child_3d_context(
    combined_3d: Mat4,
    projected_to_offscreen: bool,
    paint: &PaintStyle,
    wr: LayoutRect,
    eff_affine: Affine2,
) -> Option<Mat4> {
    let st = paint.standalone_transform;
    let p_mat = st.perspective_px.filter(|&d| d > 1e-3).map(|d| {
        let ox = st.perspective_origin_x.resolve(wr.x, wr.width);
        let oy = st.perspective_origin_y.resolve(wr.y, wr.height);
        mat4_perspective_origin(d, ox, oy)
    });
    if st.preserve_3d {
        return Some(match p_mat {
            Some(pm) => combined_3d * pm,
            None => combined_3d,
        });
    }
    p_mat.map(|pm| {
        if projected_to_offscreen {
            pm
        } else {
            affine2_to_mat4(eff_affine) * pm
        }
    })
}

/// Enters stacking context painting with CSS 3D / `preserve-3d` / `clip-path` / group opacity handling.
pub(super) fn enter_stacking_context_3d(
    inner: &mut ViewTreeInner,
    frame: &mut NodePaintFrame,
    env: StackEnv,
    resolved_clip_path: Option<ResolvedClipPath>,
    scene: &mut PaintScene,
) -> Sc3dEnter {
    let eval_3d = evaluate_perspective_layer(
        env.parent_3d,
        env.affine,
        &frame.effective_paint,
        frame.world_rect,
    );
    if matches!(eval_3d, PerspectiveEval::CulledBackface)
        && !frame.effective_paint.standalone_transform.preserve_3d
    {
        return Sc3dEnter::Handled;
    }
    if frame.effective_paint.standalone_transform.preserve_3d {
        emit_preserve_3d_node(inner, frame, env, eval_3d, resolved_clip_path, scene);
        return Sc3dEnter::Handled;
    }
    let (combined_3d, persp_bounds) = match eval_3d {
        PerspectiveEval::Flat2D { combined_3d } => (combined_3d, None),
        PerspectiveEval::Project {
            combined_3d,
            bounds,
            ..
        } => (combined_3d, Some(bounds)),
        PerspectiveEval::CulledBackface => return Sc3dEnter::Handled,
    };
    let child_3d = compute_child_3d_context(
        combined_3d,
        persp_bounds.is_some(),
        &frame.effective_paint,
        frame.world_rect,
        frame.effective_transform,
    );
    let pushed_layer = node_needs_group_opacity(inner, frame)
        || !frame.effective_paint.filter.is_identity()
        || resolved_clip_path.is_some()
        || persp_bounds.is_some();
    if pushed_layer {
        push_sc_layer(frame, persp_bounds, combined_3d, resolved_clip_path, scene);
    }
    Sc3dEnter::Continue {
        child_3d,
        pushed_layer,
    }
}

fn push_sc_layer(
    frame: &mut NodePaintFrame,
    persp_bounds: Option<LayoutRect>,
    combined_3d: Mat4,
    resolved_clip_path: Option<ResolvedClipPath>,
    scene: &mut PaintScene,
) {
    emit_backdrop_blur_prelayer(frame, scene);
    let bounds = persp_bounds.unwrap_or_else(|| {
        if frame.draw_rect.width > 0.5 && frame.draw_rect.height > 0.5 {
            frame.draw_rect
        } else {
            frame.clip.rect.unwrap_or(frame.draw_rect)
        }
    });
    scene.primitives.push(DrawPrimitive::PushLayer {
        bounds,
        clip: frame.clip.rect,
        clip_radius: frame.clip.max_radius(),
        clip_radii: frame.clip.radii,
        local_clip: None,
        clip_path: resolved_clip_path,
        perspective: persp_bounds.map(|_| (frame.world_rect, combined_3d)),
        opacity: frame.effective_opacity,
        filter: frame.effective_paint.filter,
    });
    frame.effective_opacity = 1.0;
    if persp_bounds.is_some() {
        frame.effective_transform = Affine2::IDENTITY;
        frame.draw_rect = frame.world_rect;
        frame.clip = ClipState::NONE;
    }
}

fn node_center_z(inner: &ViewTreeInner, nid: NodeId, env: StackEnv) -> f32 {
    let Some(n) = inner.node(nid) else {
        return 0.0;
    };
    let eff_scroll = if n.style.layout.position == PositionMode::Fixed {
        (0.0, 0.0)
    } else {
        env.scroll
    };
    let mut wr = n.world_rect;
    wr.x -= eff_scroll.0;
    wr.y -= eff_scroll.1;
    let raw = resolve_node_paint_state(inner, nid, n, n.is_disabled(), false);
    let paint = inner.active_transition_paint(nid, raw);
    let m = compute_node_3d_mat(env.parent_3d, env.affine, &paint, wr);
    if let Some(q) = compute_projective_quad(m, wr.x, wr.y, wr.width, wr.height) {
        return q.center_z;
    }
    let (cx, cy) = (wr.x + 0.5 * wr.width, wr.y + 0.5 * wr.height);
    let sz = m.0[2] * cx + m.0[6] * cy + m.0[14];
    let sw = m.0[3] * cx + m.0[7] * cy + m.0[15];
    if sw.abs() > 1e-4 {
        sz / sw
    } else {
        sz
    }
}

fn collect_preserve_3d_nodes(
    inner: &ViewTreeInner,
    parent_id: NodeId,
    env: StackEnv,
    out: &mut Vec<(NodeId, f32)>,
) {
    let child_len = inner.node(parent_id).map_or(0, |n| n.children.len());
    for i in 0..child_len {
        let Some(cid) = inner
            .node(parent_id)
            .and_then(|n| n.children.get(i).copied())
        else {
            break;
        };
        if (env.any_top_layer && is_top_layer_node(inner, cid))
            || is_node_semantically_hidden(inner, cid)
        {
            continue;
        }
        let Some((is_contents, display_none)) = inner.node(cid).map(|n| {
            (
                n.is_anchor() || n.style.layout.display == DisplayMode::Contents,
                n.style.layout.display == DisplayMode::None,
            )
        }) else {
            continue;
        };
        if is_contents {
            collect_preserve_3d_nodes(inner, cid, env, out);
        } else if !display_none {
            out.push((cid, node_center_z(inner, cid, env)));
        }
    }
}

/// Returns direct children of a `preserve-3d` container sorted back-to-front by `center_z`.
pub(super) fn sorted_preserve_3d_children(
    inner: &ViewTreeInner,
    parent_id: NodeId,
    child_env: StackEnv,
) -> Vec<NodeId> {
    let mut list = Vec::new();
    collect_preserve_3d_nodes(inner, parent_id, child_env, &mut list);
    list.sort_by(|a, b| a.1.total_cmp(&b.1));
    list.into_iter().map(|(cid, _)| cid).collect()
}

fn emit_preserve_3d_node(
    inner: &mut ViewTreeInner,
    frame: &mut NodePaintFrame,
    env: StackEnv,
    eval: PerspectiveEval,
    resolved_clip_path: Option<ResolvedClipPath>,
    scene: &mut PaintScene,
) {
    let (combined_3d, opt_bounds, culled) = match eval {
        PerspectiveEval::Flat2D { combined_3d } => (combined_3d, None, false),
        PerspectiveEval::Project {
            combined_3d,
            bounds,
            ..
        } => (combined_3d, Some(bounds), false),
        PerspectiveEval::CulledBackface => (
            compute_node_3d_mat(
                env.parent_3d,
                env.affine,
                &frame.effective_paint,
                frame.world_rect,
            ),
            None,
            true,
        ),
    };
    let (outer_clip, outer_affine) = (frame.clip, frame.effective_transform);
    let projected = opt_bounds.is_some();
    if !culled && frame.self_visible {
        if let Some(bounds) = opt_bounds {
            push_sc_layer(frame, Some(bounds), combined_3d, resolved_clip_path, scene);
        }
        emit_node_self_box_primitives(frame, scene);
        emit_node_self_content_primitives(inner, frame, scene);
        if projected {
            scene.primitives.push(DrawPrimitive::PopLayer);
        }
    }
    let child_3d = compute_child_3d_context(
        combined_3d,
        false,
        &frame.effective_paint,
        frame.world_rect,
        outer_affine,
    );
    let child_env = StackEnv {
        clip: outer_clip,
        hoisted_clip: None,
        affine: if projected {
            Affine2::IDENTITY
        } else {
            outer_affine
        },
        opacity: if projected {
            env.opacity * frame.effective_paint.opacity.clamp(0.0, 1.0)
        } else {
            frame.effective_opacity
        },
        inherited_color: frame.next_inherited_text_color,
        parent_3d: child_3d,
        ..env
    };
    for cid in sorted_preserve_3d_children(inner, frame.node_id, child_env) {
        emit_stacking_context(inner, cid, child_env, false, scene);
    }
}
