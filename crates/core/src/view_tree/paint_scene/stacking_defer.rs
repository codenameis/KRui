//! Deferred 7-step Stacking Context emission used when a subtree contains negative `z-index`.

use super::{
    creates_stacking_context, emit_stacking_context, is_flex_or_grid, is_top_layer_node,
    node_is_scroll_and_clip, StackEnv,
};
use crate::scene::{DrawPrimitive, PaintScene};
use crate::view_tree::clip_shape::{compute_child_clip_layer, ClipState, LocalClipLayer};
use crate::view_tree::paint::{
    emit_node_self_box_primitives, emit_node_self_content_primitives, emit_scrollbar_thumb,
    prepare_node_paint_frame, NodePaintFrame,
};
use crate::view_tree::pointer_vars::is_pseudo_before_node;
use crate::view_tree::semantics::is_node_semantically_hidden;
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_layout::LayoutRect;
use krui_style::{Affine2, Color, DisplayMode, PositionMode};

struct HoistedSc {
    node_id: NodeId,
    z_index: i32,
    env: StackEnv,
}

enum Step5Item {
    Sc(HoistedSc),
    PositionedNonSc(usize),
}

enum InFlowOp {
    ScSelfContent,
    NodeBoxAndContent(usize),
    NodeBoxOnly(usize),
    NodeContentOnly(usize),
    PushClipLayer {
        bounds: LayoutRect,
        outer_clip: ClipState,
        local_clip: LocalClipLayer,
    },
    PopClipLayer,
    Scrollbar {
        world_rect: LayoutRect,
        scroll_y: f32,
        max_scroll_y: f32,
        clip: ClipState,
        transform: Affine2,
        opacity: f32,
        scrollbar_width: krui_style::ScrollbarWidth,
        scrollbar_color: Option<(Color, Color)>,
    },
}

#[derive(Default)]
struct ScParticipants {
    frames: Vec<NodePaintFrame>,
    neg_contexts: Vec<HoistedSc>,
    inflow: Vec<InFlowOp>,
    pos_non_sc_ops: Vec<Vec<InFlowOp>>,
    zero_or_positioned: Vec<Step5Item>,
    pos_contexts: Vec<HoistedSc>,
}

/// Collects and emits children of `frame.node_id` in strict W3C 7-step order (supporting `z < 0`).
pub(super) fn emit_sc_body_deferred(
    inner: &mut ViewTreeInner,
    frame: &NodePaintFrame,
    child_env: StackEnv,
    scene: &mut PaintScene,
) {
    let mut parts = ScParticipants::default();
    collect_sc_children(inner, frame.node_id, child_env, true, &mut parts, None);
    emit_sc_participants(inner, frame, &mut parts, scene);
}

fn emit_sc_participants(
    inner: &mut ViewTreeInner,
    sc_frame: &NodePaintFrame,
    parts: &mut ScParticipants,
    scene: &mut PaintScene,
) {
    parts.neg_contexts.sort_by_key(|c| c.z_index);
    for child in &parts.neg_contexts {
        emit_stacking_context(inner, child.node_id, child.env, false, scene);
    }
    emit_inflow_ops(inner, Some(sc_frame), &parts.frames, &parts.inflow, scene);
    for item in &parts.zero_or_positioned {
        match item {
            Step5Item::Sc(child) => {
                emit_stacking_context(inner, child.node_id, child.env, false, scene)
            }
            Step5Item::PositionedNonSc(ops_idx) => {
                emit_inflow_ops(
                    inner,
                    None,
                    &parts.frames,
                    &parts.pos_non_sc_ops[*ops_idx],
                    scene,
                );
            }
        }
    }
    parts.pos_contexts.sort_by_key(|c| c.z_index);
    for child in &parts.pos_contexts {
        emit_stacking_context(inner, child.node_id, child.env, false, scene);
    }
}

fn emit_inflow_ops(
    inner: &mut ViewTreeInner,
    sc_frame: Option<&NodePaintFrame>,
    frames: &[NodePaintFrame],
    ops: &[InFlowOp],
    scene: &mut PaintScene,
) {
    for op in ops {
        match *op {
            InFlowOp::ScSelfContent => {
                if let Some(f) = sc_frame.filter(|f| f.self_visible) {
                    emit_node_self_content_primitives(inner, f, scene);
                }
            }
            InFlowOp::NodeBoxAndContent(idx) => {
                let f = &frames[idx];
                if f.self_visible {
                    emit_node_self_box_primitives(f, scene);
                    emit_node_self_content_primitives(inner, f, scene);
                }
            }
            InFlowOp::NodeBoxOnly(idx) => {
                let f = &frames[idx];
                if f.self_visible {
                    emit_node_self_box_primitives(f, scene);
                }
            }
            InFlowOp::NodeContentOnly(idx) => {
                let f = &frames[idx];
                if f.self_visible {
                    emit_node_self_content_primitives(inner, f, scene);
                }
            }
            InFlowOp::PushClipLayer {
                bounds,
                outer_clip,
                local_clip,
            } => scene.primitives.push(DrawPrimitive::PushLayer {
                bounds,
                clip: outer_clip.rect,
                clip_radius: outer_clip.max_radius(),
                clip_radii: outer_clip.radii,
                local_clip: Some(local_clip),
                opacity: 1.0,
            }),
            InFlowOp::PopClipLayer => scene.primitives.push(DrawPrimitive::PopLayer),
            InFlowOp::Scrollbar {
                world_rect,
                scroll_y,
                max_scroll_y,
                clip,
                transform,
                opacity,
                scrollbar_width,
                scrollbar_color,
            } => emit_scrollbar_thumb(
                scene,
                world_rect,
                scroll_y,
                max_scroll_y,
                clip,
                transform,
                opacity,
                scrollbar_width,
                scrollbar_color,
            ),
        }
    }
}

fn push_op_to_target(parts: &mut ScParticipants, pos_idx: Option<usize>, op: InFlowOp) {
    match pos_idx {
        Some(idx) => parts.pos_non_sc_ops[idx].push(op),
        None => parts.inflow.push(op),
    }
}

fn collect_sc_children(
    inner: &mut ViewTreeInner,
    parent_id: NodeId,
    env: StackEnv,
    emit_sc_self: bool,
    parts: &mut ScParticipants,
    pos_idx: Option<usize>,
) {
    let child_len = inner.node(parent_id).map_or(0, |n| n.children.len());
    let mut emitted_self = !emit_sc_self;
    for i in 0..child_len {
        let Some(cid) = inner
            .node(parent_id)
            .and_then(|n| n.children.get(i).copied())
        else {
            break;
        };
        if !emitted_self && !is_pseudo_before_node(inner, cid) {
            parts.inflow.push(InFlowOp::ScSelfContent);
            emitted_self = true;
        }
        visit_child_paint_node(inner, cid, env, parts, pos_idx);
    }
    if !emitted_self {
        parts.inflow.push(InFlowOp::ScSelfContent);
    }
}

fn visit_child_paint_node(
    inner: &mut ViewTreeInner,
    nid: NodeId,
    env: StackEnv,
    parts: &mut ScParticipants,
    pos_idx: Option<usize>,
) {
    if is_top_layer_node(inner, nid) || is_node_semantically_hidden(inner, nid) {
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
            collect_sc_children(inner, nid, env, false, parts, pos_idx);
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
        let entry = HoistedSc {
            node_id: nid,
            z_index: n.style.layout.z_index,
            env,
        };
        if n.style.layout.has_z_index && entry.z_index < 0 {
            parts.neg_contexts.push(entry);
        } else if n.style.layout.has_z_index && entry.z_index > 0 {
            parts.pos_contexts.push(entry);
        } else {
            parts.zero_or_positioned.push(Step5Item::Sc(entry));
        }
    } else if n.style.layout.position != PositionMode::Static {
        let new_idx = parts.pos_non_sc_ops.len();
        parts.pos_non_sc_ops.push(Vec::new());
        parts
            .zero_or_positioned
            .push(Step5Item::PositionedNonSc(new_idx));
        collect_non_sc_subtree(inner, frame, parts, Some(new_idx));
    } else {
        collect_non_sc_subtree(inner, frame, parts, pos_idx);
    }
}

type DeferredChildEnv = (StackEnv, Option<LocalClipLayer>, Option<LayoutRect>, bool);

fn build_non_sc_child_env(frame: &NodePaintFrame) -> DeferredChildEnv {
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
    let child_env = StackEnv {
        scroll: next_scroll,
        clip: next_clip,
        affine: frame.effective_transform,
        opacity: frame.effective_opacity,
        inherited_color: frame.next_inherited_text_color,
        parent_is_flex_or_grid: is_flex_or_grid(frame.layout.display),
        any_top_layer: false,
        any_neg_z: true,
    };
    (child_env, local_clip, clip_bounds, is_scroll)
}

fn collect_non_sc_subtree(
    inner: &mut ViewTreeInner,
    frame: NodePaintFrame,
    parts: &mut ScParticipants,
    pos_idx: Option<usize>,
) {
    let (child_env, local_clip, clip_bounds, is_scroll) = build_non_sc_child_env(&frame);
    let frame_idx = parts.frames.len();
    parts.frames.push(frame);
    let child_len = frame.child_len;
    let leading_before = inner
        .node(frame.node_id)
        .and_then(|n| n.children.first().copied())
        .is_some_and(|c| is_pseudo_before_node(inner, c));
    push_op_to_target(
        parts,
        pos_idx,
        if leading_before {
            InFlowOp::NodeBoxOnly(frame_idx)
        } else {
            InFlowOp::NodeBoxAndContent(frame_idx)
        },
    );
    if let Some(lc) = local_clip {
        push_op_to_target(
            parts,
            pos_idx,
            InFlowOp::PushClipLayer {
                bounds: clip_bounds.unwrap_or(frame.draw_rect),
                outer_clip: frame.clip,
                local_clip: lc,
            },
        );
    }
    for i in 0..child_len {
        let Some(cid) = inner
            .node(frame.node_id)
            .and_then(|n| n.children.get(i).copied())
        else {
            break;
        };
        visit_child_paint_node(inner, cid, child_env, parts, pos_idx);
        if leading_before && i == 0 {
            push_op_to_target(parts, pos_idx, InFlowOp::NodeContentOnly(frame_idx));
        }
    }
    if local_clip.is_some() {
        push_op_to_target(parts, pos_idx, InFlowOp::PopClipLayer);
    }
    if is_scroll {
        push_op_to_target(
            parts,
            pos_idx,
            InFlowOp::Scrollbar {
                world_rect: frame.world_rect,
                scroll_y: frame.scroll.1 .1,
                max_scroll_y: frame.scroll.2,
                clip: frame.clip,
                transform: frame.effective_transform,
                opacity: frame.effective_opacity,
                scrollbar_width: frame.layout.scrollbar_width,
                scrollbar_color: frame.effective_paint.scrollbar_color,
            },
        );
    }
}
