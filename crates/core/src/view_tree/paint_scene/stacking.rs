//! W3C CSS 2.2 Appendix E 7-step Stacking Context painting.
//! Reverse hit-testing lives in `stacking_hit.rs`; Top-Layer overlays in `stacking_toplayer.rs`;
//! deferred negative `z-index` collection in `stacking_defer.rs`; direct emission in `stacking_direct.rs`.

use super::paint_3d::{self, Sc3dEnter};
#[path = "stacking_defer.rs"]
mod stacking_defer;
#[path = "stacking_direct.rs"]
mod stacking_direct;
#[path = "stacking_hit.rs"]
mod stacking_hit;
#[path = "stacking_toplayer.rs"]
mod stacking_toplayer;

pub(crate) use stacking_hit::hit_test_stacked_scene;
use stacking_toplayer::emit_top_layer_pass;
pub(crate) use stacking_toplayer::is_top_layer_node;

use crate::scene::{DrawPrimitive, PaintScene};
use crate::view_tree::clip_path_resolve::resolve_clip_path;
use crate::view_tree::clip_shape::{compute_child_clip_layer, ClipState, LocalClipLayer};
use crate::view_tree::node::ElementNode;
use crate::view_tree::paint::{
    emit_node_self_box_primitives, emit_node_self_content_primitives, emit_scrollbar_thumb,
    prepare_node_paint_frame, NodePaintFrame,
};
use crate::view_tree::semantics::is_node_semantically_hidden;
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_style::{
    Affine2, Color, CssFilterEffect, DisplayMode, Mat4, PaintStyle, PositionMode, Transform2D,
};

#[derive(Clone, Copy)]
pub(super) struct HoistedClipWrap {
    pub(super) bounds: LayoutRect,
    pub(super) outer_clip: ClipState,
    pub(super) local_clip: LocalClipLayer,
}

#[derive(Clone, Copy)]
pub(super) struct StackEnv {
    pub(super) scroll: (f32, f32),
    pub(super) clip: ClipState,
    pub(super) hoisted_clip: Option<HoistedClipWrap>,
    pub(super) affine: Affine2,
    pub(super) opacity: f32,
    pub(super) inherited_color: Option<Color>,
    pub(super) parent_is_flex_or_grid: bool,
    pub(super) any_top_layer: bool,
    pub(super) any_neg_z: bool,
    pub(super) parent_3d: Option<Mat4>,
}

impl StackEnv {
    pub(super) fn top_layer(scroll: (f32, f32), affine: Affine2) -> Self {
        Self {
            scroll,
            clip: ClipState::NONE,
            hoisted_clip: None,
            affine,
            opacity: 1.0,
            inherited_color: None,
            parent_is_flex_or_grid: false,
            any_top_layer: true,
            any_neg_z: true,
            parent_3d: None,
        }
    }
}

pub(super) fn emit_with_hoisted_clip(
    wrap: Option<HoistedClipWrap>,
    scene: &mut PaintScene,
    emit: impl FnOnce(&mut PaintScene),
) {
    if let Some(w) = wrap {
        scene.primitives.push(DrawPrimitive::PushLayer {
            bounds: w.bounds,
            clip: w.outer_clip.rect,
            clip_radius: w.outer_clip.max_radius(),
            clip_radii: w.outer_clip.radii,
            local_clip: Some(w.local_clip),
            clip_path: None,
            perspective: None,
            opacity: 1.0,
            filter: CssFilterEffect::IDENTITY,
        });
        emit(scene);
        scene.primitives.push(DrawPrimitive::PopLayer);
    } else {
        emit(scene);
    }
}

pub(super) fn creates_stacking_context(
    n: &ElementNode,
    eff_paint: &PaintStyle,
    is_root: bool,
    is_top_layer: bool,
    parent_is_flex_or_grid: bool,
) -> bool {
    let l = &n.style.layout;
    is_root
        || is_top_layer
        || l.position == PositionMode::Fixed
        || (l.position != PositionMode::Static && l.has_z_index)
        || (parent_is_flex_or_grid && l.has_z_index)
        || eff_paint.opacity < 0.999
        || eff_paint.has_non_identity_transform()
        || eff_paint.standalone_transform.perspective_px.is_some()
        || eff_paint.standalone_transform.preserve_3d
        || eff_paint.clip_path.is_some()
        || eff_paint.backdrop_blur_px > 0.0
        || !eff_paint.filter.is_identity()
}

pub(super) fn is_flex_or_grid(d: DisplayMode) -> bool {
    matches!(
        d,
        DisplayMode::Flex | DisplayMode::InlineFlex | DisplayMode::Grid | DisplayMode::InlineGrid
    )
}

pub(super) fn node_is_scroll_and_clip(
    layout: &krui_style::LayoutStyle,
    _role: SemanticRole,
) -> (bool, bool) {
    (layout.is_scroll_container(), layout.clips_overflow())
}

#[derive(Clone, Copy)]
struct SceneFastFlags {
    any_top_layer: bool,
    any_neg_z: bool,
}

fn scan_scene_fast_flags(inner: &ViewTreeInner) -> SceneFastFlags {
    let mut any_top_layer = !inner.open_popovers.is_empty();
    let mut any_neg_z = false;
    for n in &inner.nodes {
        if !n.alive() {
            continue;
        }
        if !any_top_layer && !n.is_anchor() && n._tag == "dialog" {
            any_top_layer = true;
        }
        if !any_neg_z && n.style.layout.has_z_index && n.style.layout.z_index < 0 {
            any_neg_z = true;
        }
        if any_top_layer && any_neg_z {
            break;
        }
    }
    SceneFastFlags {
        any_top_layer,
        any_neg_z,
    }
}

/// Emits all document nodes and Top-Layer overlays into `scene` in W3C CSS stacking order.
pub(crate) fn emit_stacked_paint_scene(
    inner: &mut ViewTreeInner,
    viewport: (u32, u32),
    scene: &mut PaintScene,
) {
    let flags = scan_scene_fast_flags(inner);
    let root_env = StackEnv {
        scroll: (0.0, 0.0),
        clip: ClipState::NONE,
        hoisted_clip: None,
        affine: Affine2::IDENTITY,
        opacity: 1.0,
        inherited_color: None,
        parent_is_flex_or_grid: false,
        any_top_layer: flags.any_top_layer,
        any_neg_z: flags.any_neg_z,
        parent_3d: None,
    };
    let root_len = inner.roots.len();
    for i in 0..root_len {
        let Some(&root_id) = inner.roots.get(i) else {
            break;
        };
        if !flags.any_top_layer || !is_top_layer_node(inner, root_id) {
            emit_stacking_context(inner, root_id, root_env, false, scene);
        }
    }
    if flags.any_top_layer {
        emit_top_layer_pass(inner, viewport, scene);
    }
}

fn sc_has_negative_z_descendant(inner: &ViewTreeInner, parent_id: NodeId, is_fg: bool) -> bool {
    let len = inner.node(parent_id).map_or(0, |n| n.children.len());
    for i in 0..len {
        let Some(cid) = inner
            .node(parent_id)
            .and_then(|n| n.children.get(i).copied())
        else {
            break;
        };
        if is_top_layer_node(inner, cid) || is_node_semantically_hidden(inner, cid) {
            continue;
        }
        let Some(n) = inner.node(cid) else { continue };
        if n.is_anchor() || n.style.layout.display == DisplayMode::Contents {
            if sc_has_negative_z_descendant(inner, cid, is_fg) {
                return true;
            }
            continue;
        }
        if n.style.layout.display == DisplayMode::None {
            continue;
        }
        let l = &n.style.layout;
        if l.has_z_index && l.z_index < 0 && (l.position != PositionMode::Static || is_fg) {
            return true;
        }
        let stops_here = l.position == PositionMode::Fixed
            || (l.position != PositionMode::Static && l.has_z_index)
            || (is_fg && l.has_z_index)
            || n.style.paint.opacity < 0.999
            || n.style.paint.transform != Transform2D::IDENTITY
            || n.style.paint.backdrop_blur_px > 0.0
            || !n.style.paint.filter.is_identity();
        if !stops_here
            && !n.children.is_empty()
            && sc_has_negative_z_descendant(inner, cid, is_flex_or_grid(l.display))
        {
            return true;
        }
    }
    false
}

pub(super) fn emit_stacking_context(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    env: StackEnv,
    is_top_layer: bool,
    scene: &mut PaintScene,
) {
    if !is_top_layer && env.any_top_layer && is_top_layer_node(inner, node_id) {
        return;
    }
    let Some(mut frame) = prepare_node_paint_frame(
        inner,
        node_id,
        env.scroll,
        env.clip,
        env.affine,
        (env.opacity, env.inherited_color),
        is_top_layer,
    ) else {
        return;
    };
    let resolved_clip_path = frame
        .effective_paint
        .clip_path
        .map(|cp| resolve_clip_path(cp, frame.world_rect, frame.effective_transform));
    let Sc3dEnter::Continue {
        child_3d,
        pushed_layer,
    } = paint_3d::enter_stacking_context_3d(inner, &mut frame, env, resolved_clip_path, scene)
    else {
        return;
    };
    if frame.self_visible {
        emit_node_self_box_primitives(&frame, scene);
    }
    if frame.child_len == 0 {
        if frame.self_visible {
            emit_node_self_content_primitives(inner, &frame, scene);
        }
    } else {
        emit_sc_clipped_body(inner, &frame, env, child_3d, scene);
    }
    if pushed_layer {
        scene.primitives.push(DrawPrimitive::PopLayer);
    }
}

fn emit_sc_clipped_body(
    inner: &mut ViewTreeInner,
    frame: &NodePaintFrame,
    env: StackEnv,
    child_3d: Option<Mat4>,
    scene: &mut PaintScene,
) {
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
    let child_env = StackEnv {
        scroll: next_scroll,
        clip: next_clip,
        hoisted_clip: None,
        affine: frame.effective_transform,
        opacity: frame.effective_opacity,
        inherited_color: frame.next_inherited_text_color,
        parent_is_flex_or_grid: is_flex_or_grid(frame.layout.display),
        any_top_layer: env.any_top_layer,
        any_neg_z: env.any_neg_z,
        parent_3d: child_3d,
    };
    if env.any_neg_z
        && sc_has_negative_z_descendant(inner, frame.node_id, child_env.parent_is_flex_or_grid)
    {
        stacking_defer::emit_sc_body_deferred(inner, frame, child_env, scene);
    } else {
        stacking_direct::emit_sc_body_direct(inner, frame, child_env, scene);
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
