//! Layout computation and `PaintScene` assembly (`build_paint_scene`, `emit_node_primitives_recursive`,
//! and Top-Layer `<select>` overlay emission).

use crate::scene::{DrawPrimitive, PaintScene, ResolvedConicGradient, ResolvedRadialGradient};
use crate::view_tree::clip_shape::{resolve_node_visibility, ClipState};
use crate::view_tree::hit_test::update_world_rects_recursive;
use crate::view_tree::node::ElementNode;
pub(crate) use crate::view_tree::paint_box_layers::emit_scrollbar_thumb;
use crate::view_tree::paint_box_layers::{
    emit_base_and_shadow_boxes, emit_extra_box_layers, paint_has_extra_box_layers,
};
use crate::view_tree::paint_canvas::{
    emit_node_indicator_or_canvas_primitives, refresh_dirty_canvases,
};
use crate::view_tree::paint_text::emit_node_text_primitives;
use crate::view_tree::select_popup_paint::emit_select_popup_overlay;
use crate::view_tree::{A11yId, NodeId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_style::{Affine2, Color, DamageRect, DamageTrackingMode, DisplayMode, PositionMode};

/// Minimum highlight box height (logical px).
pub const MIN_TEXT_HIGHLIGHT_HEIGHT_PX: f32 = 12.0;

impl ViewTree {
    /// Configures whether [`Self::build_paint_scene_into`] tracks damaged bounding rectangles
    /// (`BoundingRects`) or skips rect diffing (`DirtyFlagOnly`, default for 120 FPS engine hosts).
    pub fn set_damage_tracking_mode(&mut self, mode: DamageTrackingMode) {
        self.inner.borrow_mut().damage_tracker.set_mode(mode);
    }

    /// Returns the currently configured [`DamageTrackingMode`].
    #[must_use]
    pub fn damage_tracking_mode(&self) -> DamageTrackingMode {
        self.inner.borrow().damage_tracker.mode()
    }

    /// Computes layout when dirty and refills `scene` in-place (preserving `Vec` capacity),
    /// returning `(did_relayout, dirty_rects)`.
    pub fn build_paint_scene_into(
        &mut self,
        viewport_w: u32,
        viewport_h: u32,
        scene: &mut PaintScene,
    ) -> (bool, Vec<DamageRect>) {
        let did_relayout =
            relayout_roots_if_dirty(&mut self.inner.borrow_mut(), viewport_w, viewport_h);
        refresh_dirty_canvases(self);

        let mut inner = self.inner.borrow_mut();
        let (clear_color, prop_node) = crate::view_tree::sync::resolve_canvas_clear_color(&inner);
        inner.propagated_canvas_node = prop_node;
        scene.width = viewport_w;
        scene.height = viewport_h;
        scene.clear_color = clear_color;
        scene.scale_factor = inner.scale_factor;
        scene.primitives.clear();
        crate::view_tree::stacking::emit_stacked_paint_scene(
            &mut inner,
            (viewport_w, viewport_h),
            scene,
        );
        emit_select_popup_overlay(&mut inner, scene);
        crate::view_tree::tooltip_paint::emit_hover_tooltip_overlay(
            &mut inner,
            (viewport_w as f32, viewport_h as f32),
            scene,
        );
        inner.paint_dirty = false;
        let dirty = inner.damage_tracker.compute_damage(scene);
        (did_relayout, dirty)
    }

    /// Computes layout when the tree or viewport is dirty and assembles a `PaintScene`,
    /// including top-layer `<select>` and tooltip overlays.
    pub fn build_paint_scene(&mut self, viewport_w: u32, viewport_h: u32) -> (PaintScene, bool) {
        let mut scene = PaintScene::new(viewport_w.max(1), viewport_h.max(1), Color::TRANSPARENT);
        let (did_relayout, _) = self.build_paint_scene_into(viewport_w, viewport_h, &mut scene);
        (scene, did_relayout)
    }
}

fn run_layout_pass(inner: &mut ViewTreeInner, vw: f32, vh: f32) -> bool {
    let ViewTreeInner {
        layout_engine,
        stylo,
        text_engine,
        ..
    } = inner;
    layout_engine.compute_if_dirty_with_stylo(stylo, vw, vh, |ctx, max_w| {
        let mw = if ctx.single_line { None } else { max_w };
        if let Some(spans) = ctx.rich_spans.as_deref().filter(|s| !s.is_empty()) {
            text_engine.measure_rich_text(spans, &ctx.text_style, mw)
        } else if ctx.text.is_empty() {
            (0.0, text_engine.measure_empty_line_height(&ctx.text_style))
        } else {
            text_engine.measure_styled_text(&ctx.text, &ctx.text_style, mw)
        }
    })
}

fn relayout_roots_if_dirty(inner: &mut ViewTreeInner, viewport_w: u32, viewport_h: u32) -> bool {
    let (vw, vh) = (viewport_w as f32, viewport_h as f32);
    let _ = crate::view_tree::sync::sync_viewport_and_flush_restyle(inner, vw, vh);
    let mut did_relayout = run_layout_pass(inner, vw, vh);
    if did_relayout || inner.stylo.has_container_queries() {
        for _ in 0..4 {
            if !crate::view_tree::style_bridge::resolver::sync_container_sizes_and_restyle(inner) {
                break;
            }
            did_relayout |= run_layout_pass(inner, vw, vh);
        }
    }
    if did_relayout {
        let roots = inner.roots.clone();
        for &root_id in &roots {
            update_world_rects_recursive(inner, root_id, 0.0, 0.0);
        }
        crate::view_tree::transitions::seed_initial_transition_baselines(inner);
    }
    did_relayout
}

pub(crate) fn intersect_rects(a: LayoutRect, b: LayoutRect) -> LayoutRect {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let x1 = (a.x + a.width).min(b.x + b.width).max(x0);
    let y1 = (a.y + a.height).min(b.y + b.height).max(y0);
    LayoutRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn push_flat_box(
    scene: &mut PaintScene,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    clip: ClipState,
    transform: Affine2,
    opacity: f32,
    background_color: Color,
    border_radius: f32,
) {
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: LayoutRect {
            x,
            y,
            width,
            height,
        },
        clip: clip.rect,
        clip_radius: clip.max_radius(),
        clip_radii: clip.radii,
        transform,
        opacity,
        background_color,
        background_gradient: None,
        radial_gradient: None,
        conic_gradient: None,
        border_color: Color::TRANSPARENT,
        border_width: 0.0,
        border_radius,
        corner_radii: [border_radius; 4],
        box_shadow: None,
        backdrop_blur_px: 0.0,
    });
}

pub(super) fn paint_has_base_box_visuals(
    p: &krui_style::PaintStyle,
    l: &krui_style::LayoutStyle,
) -> bool {
    let eff = p.effective_border_edges(l);
    let has_border = eff.top > 0.0 || eff.right > 0.0 || eff.bottom > 0.0 || eff.left > 0.0;
    p.background_color.a > 0
        || p.background_gradient.is_some()
        || p.radial_gradient.is_some()
        || p.conic_gradient.is_some()
        || has_border
        || p.box_shadow.is_some()
        || p.secondary_shadow.is_some()
        || p.shadow_stack.has_visible()
        || p.backdrop_blur_px > 0.0
}

fn paint_has_box_visuals(p: &krui_style::PaintStyle, l: &krui_style::LayoutStyle) -> bool {
    paint_has_base_box_visuals(p, l) || paint_has_extra_box_layers(p, l)
}

pub(super) fn node_needs_group_opacity(inner: &ViewTreeInner, f: &NodePaintFrame) -> bool {
    if f.effective_paint.opacity >= 0.999 {
        return false;
    }
    if f.child_len > 0 {
        return true;
    }
    let has_indicator = matches!(
        f.role,
        SemanticRole::Checkbox
            | SemanticRole::RadioButton
            | SemanticRole::Slider
            | SemanticRole::ComboBox
            | SemanticRole::Canvas
    ) || inner.sparse.media.contains_key(&f.node_id)
        || crate::view_tree::semantics::progress_fill_ratio(inner, f.node_id).is_some();
    let is_scroll = f.layout.is_scroll_container();
    paint_has_box_visuals(&f.effective_paint, &f.layout)
        && (f.flags.0 || has_indicator || is_scroll)
}

fn node_has_text_or_input(inner: &ViewTreeInner, node_id: NodeId, n: &ElementNode) -> (bool, bool) {
    if matches!(
        n.role,
        SemanticRole::Checkbox | SemanticRole::RadioButton | SemanticRole::Slider
    ) || crate::view_tree::semantics::progress_fill_ratio(inner, node_id).is_some()
    {
        return (false, false);
    }
    if n.role == SemanticRole::ComboBox {
        let has_opts = inner
            .control(node_id)
            .is_some_and(|c| !c.select_options.is_empty());
        return (!n.text.is_empty() || has_opts, false);
    }
    let ed = inner.text_edit(node_id);
    let has_preedit = ed.is_some_and(|e| !e.ime_state.preedit.is_empty());
    let has_placeholder = ed.is_some_and(|e| !e.placeholder.is_empty());
    let has_rich = inner
        .sparse
        .rich_spans
        .get(&node_id)
        .is_some_and(|s| !s.is_empty());
    let has_text = !n.text.is_empty()
        || has_rich
        || has_preedit
        || has_placeholder
        || n.role.is_text_editable();
    let showing_placeholder = n.text.is_empty() && !has_rich && !has_preedit && has_placeholder;
    (has_text, showing_placeholder)
}

#[derive(Clone, Copy)]
pub(super) struct NodePaintFrame {
    pub(super) node_id: NodeId,
    pub(super) layout: krui_style::LayoutStyle,
    pub(super) effective_paint: krui_style::PaintStyle,
    pub(super) role: SemanticRole,
    pub(super) world_rect: LayoutRect,
    pub(super) draw_rect: LayoutRect,
    pub(super) self_visible: bool,
    pub(super) child_len: usize,
    pub(super) scroll: ((f32, f32), (f32, f32), f32),
    pub(super) flags: (bool, bool, bool),
    pub(super) clip: ClipState,
    pub(super) radial_gradient: Option<ResolvedRadialGradient>,
    pub(super) conic_gradient: Option<ResolvedConicGradient>,
    pub(super) effective_transform: Affine2,
    pub(super) effective_opacity: f32,
    pub(super) next_inherited_text_color: Option<Color>,
}

/// Emits backdrop blur onto parent framebuffer before entering an `opacity < 1.0` group layer.
pub(super) fn emit_backdrop_blur_prelayer(f: &mut NodePaintFrame, scene: &mut PaintScene) {
    if f.self_visible && f.effective_paint.backdrop_blur_px > 0.5 {
        scene.primitives.push(DrawPrimitive::StyledBox {
            rect: f.world_rect,
            clip: f.clip.rect,
            clip_radius: f.clip.max_radius(),
            clip_radii: f.clip.radii,
            transform: f.effective_transform,
            opacity: f.effective_opacity,
            background_color: Color::TRANSPARENT,
            background_gradient: None,
            radial_gradient: None,
            conic_gradient: None,
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            border_radius: f.effective_paint.border_radius,
            corner_radii: f.effective_paint.effective_corner_radii().to_array(),
            box_shadow: None,
            backdrop_blur_px: f.effective_paint.backdrop_blur_px,
        });
        f.effective_paint.backdrop_blur_px = 0.0;
    }
}

fn compute_frame_inherited_text_color(
    effective_paint: &mut krui_style::PaintStyle,
    inherited_text_color: Option<Color>,
) -> Option<Color> {
    if effective_paint.inherits_text_color {
        if let Some(inh_c) = inherited_text_color {
            effective_paint.text_color = inh_c;
        }
    }
    if effective_paint.provides_inherited_text_color {
        Some(effective_paint.text_color)
    } else if effective_paint.inherits_text_color {
        inherited_text_color
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_node_paint_frame(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    scroll_offset: (f32, f32),
    clip_state: ClipState,
    parent_affine: Affine2,
    (parent_opacity, inherited_text_color): (f32, Option<Color>),
    ignore_parent_clip: bool,
) -> Option<NodePaintFrame> {
    if crate::view_tree::semantics::is_node_semantically_hidden(inner, node_id) {
        return None;
    }
    let n = inner.node(node_id)?;
    if n.is_anchor() || n.style.layout.display == DisplayMode::None {
        return None;
    }
    let vis_hidden = n.is_visibility_hidden();
    let (scroll_offset, clip) = if n.style.layout.position == PositionMode::Fixed {
        ((0.0, 0.0), ClipState::NONE)
    } else if ignore_parent_clip {
        (scroll_offset, ClipState::NONE)
    } else {
        (scroll_offset, clip_state)
    };
    let (has_text, showing_placeholder) = node_has_text_or_input(inner, node_id, n);
    let mut world_rect = n.world_rect;
    world_rect.x -= scroll_offset.0;
    world_rect.y -= scroll_offset.1;
    let (layout, role, child_len) = (n.style.layout, n.role, n.children.len());
    let (is_focused, raw_paint) = {
        let is_f =
            inner.a11y_engine.focused_id() == Some(A11yId::from(node_id).0) && !n.is_disabled();
        (
            is_f,
            resolve_node_paint_state(inner, node_id, n, n.is_disabled(), is_f),
        )
    };
    let (effective_paint, next_inherited_text_color, effective_opacity, effective_transform) =
        compute_paint_and_transform(
            inner,
            node_id,
            raw_paint,
            world_rect,
            parent_affine,
            (parent_opacity, inherited_text_color),
        )?;
    finish_node_paint_frame(
        inner,
        node_id,
        (layout, role, child_len),
        world_rect,
        clip,
        scroll_offset,
        (has_text && !vis_hidden, showing_placeholder, is_focused),
        vis_hidden,
        (
            effective_paint,
            effective_transform,
            effective_opacity,
            next_inherited_text_color,
        ),
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_node_paint_frame(
    inner: &ViewTreeInner,
    node_id: NodeId,
    (layout, role, child_len): (krui_style::LayoutStyle, SemanticRole, usize),
    world_rect: LayoutRect,
    clip: ClipState,
    scroll_offset: (f32, f32),
    flags: (bool, bool, bool),
    vis_hidden: bool,
    (effective_paint, effective_transform, effective_opacity, next_inherited_text_color): (
        krui_style::PaintStyle,
        Affine2,
        f32,
        Option<Color>,
    ),
) -> Option<NodePaintFrame> {
    let clips_self = layout.clips_overflow();
    let (draw_rect, self_visible) = resolve_node_visibility(
        clips_self,
        child_len == 0,
        world_rect,
        clip.rect,
        effective_transform,
        &effective_paint,
    )?;
    let radial_gradient = crate::view_tree::pointer_vars::resolve_radial_gradient_for_node(
        inner,
        node_id,
        world_rect,
        &effective_paint,
    );
    let conic_gradient = crate::view_tree::pointer_vars::resolve_conic_gradient_for_node(
        inner,
        node_id,
        world_rect,
        &effective_paint,
    );
    let scroll_pair = inner.scroll(node_id).map_or(((0.0, 0.0), 0.0), |s| {
        ((s.scroll_x, s.scroll_y), s.cached_max_scroll_y)
    });
    Some(NodePaintFrame {
        node_id,
        layout,
        effective_paint,
        role,
        world_rect,
        draw_rect,
        self_visible: self_visible && !vis_hidden,
        child_len,
        scroll: (scroll_offset, scroll_pair.0, scroll_pair.1),
        flags,
        clip,
        radial_gradient,
        conic_gradient,
        effective_transform,
        effective_opacity,
        next_inherited_text_color,
    })
}

fn compute_paint_and_transform(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    raw_paint: krui_style::PaintStyle,
    world_rect: LayoutRect,
    parent_affine: Affine2,
    (parent_opacity, inherited_text_color): (f32, Option<Color>),
) -> Option<(krui_style::PaintStyle, Option<Color>, f32, Affine2)> {
    let mut effective_paint = inner.resolve_transitioned_node_paint(node_id, raw_paint);
    if inner.propagated_canvas_node == Some(node_id)
        && effective_paint.background_gradient.is_none()
        && effective_paint.radial_gradient.is_none()
        && effective_paint.conic_gradient.is_none()
    {
        effective_paint.background_color = Color::TRANSPARENT;
    }
    let next_inherited =
        compute_frame_inherited_text_color(&mut effective_paint, inherited_text_color);
    let effective_opacity = (parent_opacity * effective_paint.opacity).clamp(0.0, 1.0);
    if effective_opacity <= 0.0 {
        return None;
    }
    let effective_transform = parent_affine * node_local_affine(&effective_paint, world_rect);
    Some((
        effective_paint,
        next_inherited,
        effective_opacity,
        effective_transform,
    ))
}

pub(crate) fn resolve_node_paint_state(
    _inner: &ViewTreeInner,
    _node_id: NodeId,
    n: &ElementNode,
    _is_disabled: bool,
    _is_focused: bool,
) -> krui_style::PaintStyle {
    n.style.paint
}

fn node_local_affine(paint: &krui_style::PaintStyle, r: LayoutRect) -> Affine2 {
    paint.effective_affine_for_rect(r.x, r.y, r.width, r.height)
}

pub(super) fn emit_node_self_box_primitives(f: &NodePaintFrame, scene: &mut PaintScene) {
    emit_base_and_shadow_boxes(
        f,
        paint_has_base_box_visuals(&f.effective_paint, &f.layout),
        scene,
    );
    emit_extra_box_layers(
        f.world_rect,
        f.clip,
        f.effective_transform,
        f.effective_opacity,
        &f.effective_paint,
        &f.layout,
        scene,
    );
}

pub(super) fn emit_node_self_content_primitives(
    inner: &mut ViewTreeInner,
    f: &NodePaintFrame,
    scene: &mut PaintScene,
) {
    let (has_text, showing_placeholder, is_focused) = f.flags;
    if has_text {
        let tc = if showing_placeholder {
            let base = f
                .effective_paint
                .placeholder_color
                .unwrap_or(f.effective_paint.text_color);
            let op = f.effective_paint.placeholder_opacity.clamp(0.0, 1.0);
            Color {
                a: ((base.a as f32) * op).round() as u8,
                ..base
            }
        } else {
            f.effective_paint.text_color
        };
        if let Some(style) = inner.node(f.node_id).map(|n| n.style) {
            emit_node_text_primitives(
                inner,
                f.node_id,
                &style,
                &f.effective_paint,
                f.role,
                f.world_rect,
                (showing_placeholder, is_focused),
                tc,
                (f.clip, f.effective_transform, f.effective_opacity),
                scene,
            );
        }
    }
    emit_node_indicator_or_canvas_primitives(inner, f, f.role == SemanticRole::Canvas, scene);
}
