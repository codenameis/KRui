//! Direct (non-negative-`z-index`) Stacking Context body emission split out of [`super`].

use super::{
    creates_stacking_context, emit_stacking_context, emit_with_hoisted_clip, is_flex_or_grid,
    is_top_layer_node, node_is_scroll_and_clip, HoistedClipWrap, StackEnv,
};
use crate::scene::{DrawPrimitive, PaintScene};
use crate::view_tree::clip_shape::{compute_child_clip_layer, LocalClipLayer};
use crate::view_tree::paint::{
    emit_node_self_box_primitives, emit_node_self_content_primitives, emit_scrollbar_thumb,
    prepare_node_paint_frame, NodePaintFrame,
};
use crate::view_tree::pointer_vars::is_pseudo_before_node;
use crate::view_tree::semantics::is_node_semantically_hidden;
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_layout::LayoutRect;
use krui_style::{DisplayMode, PositionMode};

#[derive(Default)]
struct DirectStepQueues {
    zero_pos: Vec<(NodeId, StackEnv, bool)>,
    pos: Vec<(NodeId, i32, StackEnv)>,
}

pub(super) fn emit_sc_body_direct(
    inner: &mut ViewTreeInner,
    sc_frame: &NodePaintFrame,
    child_env: StackEnv,
    scene: &mut PaintScene,
) {
    let mut queues = DirectStepQueues::default();
    emit_direct_children(
        inner,
        sc_frame.node_id,
        child_env,
        Some(sc_frame),
        &mut queues,
        scene,
        true,
    );
    for (nid, mut env, is_sc) in queues.zero_pos {
        let wrap = env.hoisted_clip.take();
        emit_with_hoisted_clip(wrap, scene, |s| {
            if is_sc {
                emit_stacking_context(inner, nid, env, false, s);
            } else {
                emit_positioned_non_sc_direct(inner, nid, env, s);
            }
        });
    }
    if !queues.pos.is_empty() {
        queues.pos.sort_by_key(|c| c.1);
        for (nid, _, mut env) in queues.pos {
            let wrap = env.hoisted_clip.take();
            emit_with_hoisted_clip(wrap, scene, |s| {
                emit_stacking_context(inner, nid, env, false, s);
            });
        }
    }
}

fn emit_direct_children(
    inner: &mut ViewTreeInner,
    parent_id: NodeId,
    env: StackEnv,
    sc_self: Option<&NodePaintFrame>,
    queues: &mut DirectStepQueues,
    scene: &mut PaintScene,
    emit_inflow: bool,
) {
    let child_len = inner.node(parent_id).map_or(0, |n| n.children.len());
    let mut emitted_self = sc_self.is_none();
    for i in 0..child_len {
        let Some(cid) = inner
            .node(parent_id)
            .and_then(|n| n.children.get(i).copied())
        else {
            break;
        };
        if !emitted_self && !is_pseudo_before_node(inner, cid) {
            if let Some(f) = sc_self.filter(|f| f.self_visible) {
                emit_node_self_content_primitives(inner, f, scene);
            }
            emitted_self = true;
        }
        visit_direct_child(inner, cid, env, queues, scene, emit_inflow);
    }
    if !emitted_self {
        if let Some(f) = sc_self.filter(|f| f.self_visible) {
            emit_node_self_content_primitives(inner, f, scene);
        }
    }
}

fn visit_direct_child(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    env: StackEnv,
    queues: &mut DirectStepQueues,
    scene: &mut PaintScene,
    emit_inflow: bool,
) {
    if env.any_top_layer && is_top_layer_node(inner, nid) {
        return;
    }
    if is_node_semantically_hidden(inner, nid) {
        return;
    }
    let Some((is_contents, has_kids)) = inner.node(nid).map(|n| {
        (
            !n.is_anchor() && n.style.layout.display == DisplayMode::Contents,
            !n.children.is_empty(),
        )
    }) else {
        return;
    };
    if is_contents {
        if has_kids {
            emit_direct_children(inner, nid, env, None, queues, scene, emit_inflow);
        }
        return;
    }
    let Some(frame) = prepare_node_paint_frame(
        inner,
        nid,
        env.scroll,
        env.clip,
        env.affine,
        (env.opacity, env.inherited_color),
        false,
    ) else {
        return;
    };
    let Some(n) = inner.node(nid) else { return };
    if creates_stacking_context(
        n,
        &frame.effective_paint,
        false,
        false,
        env.parent_is_flex_or_grid,
    ) {
        if n.style.layout.has_z_index && n.style.layout.z_index > 0 {
            queues.pos.push((nid, n.style.layout.z_index, env));
        } else {
            queues.zero_pos.push((nid, env, true));
        }
    } else if n.style.layout.position != PositionMode::Static {
        queues.zero_pos.push((nid, env, false));
        let next_env = next_env_for_frame(&frame, env).0;
        if frame.child_len > 0 {
            emit_direct_children(inner, nid, next_env, None, queues, scene, false);
        }
    } else {
        emit_direct_non_sc_subtree(inner, &frame, env, queues, scene, emit_inflow);
    }
}

fn emit_positioned_non_sc_direct(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    env: StackEnv,
    scene: &mut PaintScene,
) {
    let Some(frame) = prepare_node_paint_frame(
        inner,
        nid,
        env.scroll,
        env.clip,
        env.affine,
        (env.opacity, env.inherited_color),
        false,
    ) else {
        return;
    };
    let mut dummy = DirectStepQueues::default();
    emit_direct_non_sc_subtree(inner, &frame, env, &mut dummy, scene, true);
}

type DirectChildEnv = (StackEnv, Option<LocalClipLayer>, Option<LayoutRect>, bool);

fn next_env_for_frame(frame: &NodePaintFrame, env: StackEnv) -> DirectChildEnv {
    let (is_scroll, clips_self) = node_is_scroll_and_clip(&frame.layout, frame.role);
    let next_scroll = if is_scroll {
        (
            frame.scroll.0 .0 + frame.scroll.1 .0,
            frame.scroll.0 .1 + frame.scroll.1 .1,
        )
    } else {
        frame.scroll.0
    };
    let (next_clip, local_clip, clip_bounds) = compute_child_clip_layer(
        clips_self,
        frame.world_rect,
        &frame.effective_paint,
        &frame.layout,
        frame.effective_transform,
        frame.clip,
    );
    let hoisted_clip = local_clip.map_or(env.hoisted_clip, |lc| {
        Some(HoistedClipWrap {
            bounds: clip_bounds.unwrap_or(frame.draw_rect),
            outer_clip: frame.clip,
            local_clip: lc,
        })
    });
    let child_env = StackEnv {
        scroll: next_scroll,
        clip: next_clip,
        hoisted_clip,
        affine: frame.effective_transform,
        opacity: frame.effective_opacity,
        inherited_color: frame.next_inherited_text_color,
        parent_is_flex_or_grid: is_flex_or_grid(frame.layout.display),
        ..env
    };
    (child_env, local_clip, clip_bounds, is_scroll)
}

fn emit_direct_non_sc_subtree(
    inner: &mut ViewTreeInner,
    frame: &NodePaintFrame,
    env: StackEnv,
    queues: &mut DirectStepQueues,
    scene: &mut PaintScene,
    emit_inflow: bool,
) {
    let (child_env, local_clip, clip_bounds, is_scroll) = next_env_for_frame(frame, env);
    if !emit_inflow {
        if frame.child_len > 0 {
            emit_direct_children(inner, frame.node_id, child_env, None, queues, scene, false);
        }
        return;
    }
    let leading_before = frame.child_len > 0
        && inner
            .node(frame.node_id)
            .and_then(|n| n.children.first().copied())
            .is_some_and(|c| is_pseudo_before_node(inner, c));
    if frame.self_visible {
        emit_node_self_box_primitives(frame, scene);
        if !leading_before {
            emit_node_self_content_primitives(inner, frame, scene);
        }
    }
    if let Some(lc) = local_clip {
        scene.primitives.push(DrawPrimitive::PushLayer {
            bounds: clip_bounds.unwrap_or(frame.draw_rect),
            clip: frame.clip.rect,
            clip_radius: frame.clip.max_radius(),
            clip_radii: frame.clip.radii,
            local_clip: Some(lc),
            clip_path: None,
            perspective: None,
            opacity: 1.0,
            filter: krui_style::CssFilterEffect::IDENTITY,
        });
    }
    if frame.child_len > 0 {
        emit_direct_children(
            inner,
            frame.node_id,
            child_env,
            leading_before.then_some(frame),
            queues,
            scene,
            true,
        );
    }
    if local_clip.is_some() {
        scene.primitives.push(DrawPrimitive::PopLayer);
    }
    if is_scroll {
        emit_scrollbar_thumb(
            scene,
            frame.world_rect,
            frame.scroll.1 .1,
            frame.scroll.2,
            frame.clip,
            frame.effective_transform,
            frame.effective_opacity,
            frame.layout.scrollbar_width,
            frame.effective_paint.scrollbar_color,
        );
    }
}
