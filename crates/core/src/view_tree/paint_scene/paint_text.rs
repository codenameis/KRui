//! Text, IME composition, selection highlight, `<textarea>` vertical scroll, and strictly clipped
//! caret primitive emission for `ViewTree`.

use crate::scene::PaintScene;
use crate::view_tree::clip_shape::{intersect_clip_states, ClipState};
use crate::view_tree::content_box::{
    content_box_rect, effective_node_text_style, text_clip_box_rect,
};
use crate::view_tree::grapheme::{
    textarea_visible_inner_height, update_input_scroll_x, update_textarea_scroll_y,
};
use crate::view_tree::hit_test::transform_rect_aabb;
use crate::view_tree::key_routing::resolve_editable_line_caret_rect;
use crate::view_tree::node::compute_node_shaped_key;
use crate::view_tree::paint::{emit_scrollbar_thumb, intersect_rects, push_flat_box};
use crate::view_tree::paint_text_decorations::{
    build_text_draw_ctx, emit_shaped_glyph_and_ime_primitives, single_line_input_align_dx,
};
use crate::view_tree::text_edit::compose_node_display_and_ime_ranges;
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_style::{
    Affine2, AlignItems, Color, CompiledStyle, DisplayMode, JustifyContent, PaintStyle, TextAlign,
    TextDirection,
};

const UA_CARET_WIDTH_PX: f32 = 2.0;

pub(super) type ShapedNodeTextMeta = (
    usize,
    Option<(usize, usize)>,
    Option<(usize, usize)>,
    bool,
    Option<usize>,
);

#[derive(Clone, Copy)]
pub(super) struct TextDrawContext {
    pub(super) world_rect: LayoutRect,
    pub(super) local_clip: LayoutRect,
    pub(super) effective_border_width: f32,
    pub(super) role: SemanticRole,
    pub(super) is_inp_role: bool,
    pub(super) showing_placeholder: bool,
    pub(super) scroll_x: f32,
    pub(super) scroll_y: f32,
    pub(super) text_color: Color,
    pub(super) caret_color: Color,
    pub(super) selection_bg: Color,
    pub(super) selection_fg: Option<Color>,
    pub(super) custom_selection_bg: bool,
    pub(super) clip_bounds: Option<LayoutRect>,
    pub(super) clip_radius: f32,
    pub(super) clip_radii: [f32; 4],
    pub(super) transform: Affine2,
    pub(super) opacity: f32,
}

pub(crate) fn compute_shaped_width_key(
    role: SemanticRole,
    style: &CompiledStyle,
    rect_width: f32,
) -> (Option<f32>, u32) {
    let is_single_line = matches!(
        role,
        SemanticRole::Button
            | SemanticRole::ComboBox
            | SemanticRole::TextInput
            | SemanticRole::PasswordInput
    );
    if rect_width > 0.0 && !is_single_line {
        let cb = content_box_rect(
            LayoutRect {
                x: 0.0,
                y: 0.0,
                width: rect_width,
                height: 0.0,
            },
            &style.layout,
        );
        let max_w = cb.width.max(1.0);
        (Some(max_w), (max_w * 100.0).round() as u32)
    } else {
        (None, 0)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_node_text_primitives(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    style: &CompiledStyle,
    effective_paint: &PaintStyle,
    role: SemanticRole,
    world_rect: LayoutRect,
    flags: (bool, bool),
    text_color: Color,
    clip_tx: (ClipState, Affine2, f32),
    scene: &mut PaintScene,
) {
    let (showing_placeholder, is_focused) = flags;
    let effective_border_width = effective_paint.border_width;
    let (outer_clip_state, effective_transform, effective_opacity) = clip_tx;
    let Some(meta) = ensure_node_shaped_text_layout(
        inner,
        node_id,
        style,
        role,
        world_rect,
        showing_placeholder,
    ) else {
        return;
    };
    let (scroll_x, scroll_y, max_scroll_y) = update_node_input_scroll(
        inner,
        node_id,
        role,
        showing_placeholder,
        meta.0,
        world_rect,
        style,
    );
    let local_clip = text_clip_box_rect(world_rect, &style.layout, effective_border_width);
    let text_clip = compute_text_clip(
        role.is_text_editable() || style.layout.clips_overflow(),
        local_clip,
        effective_transform,
        outer_clip_state,
    );
    let ctx = build_text_draw_ctx(
        (world_rect, local_clip, effective_border_width),
        role,
        showing_placeholder,
        (scroll_x, scroll_y),
        (text_color, effective_paint),
        text_clip,
        effective_transform,
        effective_opacity,
    );
    emit_shaped_and_caret_if_needed(inner, node_id, style, meta, is_focused, ctx, scene);
    emit_textarea_scrollbar_if_needed(
        scene,
        max_scroll_y,
        outer_clip_state,
        style.layout.scrollbar_width,
        effective_paint.scrollbar_color,
        ctx,
    );
}

fn emit_textarea_scrollbar_if_needed(
    scene: &mut PaintScene,
    max_scroll_y: f32,
    outer_clip: ClipState,
    scrollbar_width: krui_style::ScrollbarWidth,
    scrollbar_color: Option<(Color, Color)>,
    ctx: TextDrawContext,
) {
    if ctx.role == SemanticRole::MultilineTextInput && max_scroll_y > 0.5 {
        emit_scrollbar_thumb(
            scene,
            ctx.world_rect,
            ctx.scroll_y,
            max_scroll_y,
            outer_clip,
            ctx.transform,
            ctx.opacity,
            scrollbar_width,
            scrollbar_color,
        );
    }
}

fn compute_text_clip(
    is_inp_role: bool,
    local_clip: LayoutRect,
    transform: Affine2,
    outer_clip: ClipState,
) -> ClipState {
    if is_inp_role {
        let aabb = transform_rect_aabb(transform, local_clip);
        let _ = intersect_rects;
        intersect_clip_states(outer_clip, aabb, [0.0; 4])
    } else {
        outer_clip
    }
}

fn emit_shaped_and_caret_if_needed(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    style: &CompiledStyle,
    meta: ShapedNodeTextMeta,
    is_focused: bool,
    ctx: TextDrawContext,
    scene: &mut PaintScene,
) {
    let dx0 = single_line_input_align_dx(style, ctx.role, ctx.world_rect, 0.0);
    let cursor_x = inner
        .node(node_id)
        .and_then(|n| n.cached_text_layout.as_ref())
        .map_or(dx0, |shaped| {
            emit_shaped_glyph_and_ime_primitives(scene, shaped, style, meta, ctx)
        });
    if is_focused && inner.caret_visible && ctx.is_inp_role {
        let empty_lh = inner.text_engine.measure_empty_line_height(&style.text);
        let shaped_for_caret = (!ctx.showing_placeholder)
            .then(|| {
                inner
                    .node(node_id)
                    .and_then(|n| n.cached_text_layout.as_ref())
            })
            .flatten();
        let crect = resolve_editable_line_caret_rect(
            ctx.world_rect,
            style,
            ctx.effective_border_width,
            ctx.role,
            shaped_for_caret,
            meta.0,
            cursor_x,
            ctx.scroll_y,
            empty_lh,
            UA_CARET_WIDTH_PX,
        );
        push_flat_box(
            scene,
            crect.x,
            crect.y,
            crect.width,
            crect.height,
            ClipState::from_opt(ctx.clip_bounds, ctx.clip_radius, ctx.clip_radii),
            ctx.transform,
            ctx.opacity,
            ctx.caret_color,
            1.0,
        );
    }
}

fn update_node_input_scroll(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    role: SemanticRole,
    showing_placeholder: bool,
    active_cursor_char: usize,
    world_rect: LayoutRect,
    style: &CompiledStyle,
) -> (f32, f32, f32) {
    if !role.is_text_editable() {
        return (0.0, 0.0, 0.0);
    }
    if showing_placeholder {
        if let Some(ed) = inner.sparse.text_edits.get_mut(&node_id) {
            ed.scroll_x = 0.0;
            ed.scroll_y = 0.0;
            ed.needs_caret_scroll = false;
        }
        return (0.0, 0.0, 0.0);
    }
    let fallback_lh = inner.text_engine.measure_empty_line_height(&style.text);
    let (raw_x, measured_w, line_top, line_h, measured_h) = inner
        .node(node_id)
        .and_then(|n| n.cached_text_layout.as_ref())
        .map(|s| {
            let ln = s.line_for_char(active_cursor_char);
            (
                s.cursor_x_for_char(active_cursor_char),
                s.measured_width,
                ln.line_top,
                ln.line_height.max(1.0),
                s.measured_height,
            )
        })
        .unwrap_or((0.0, 0.0, 0.0, fallback_lh.max(1.0), 0.0));
    let ed = inner.ensure_text_edit(node_id);
    if role.is_single_line_input() {
        ed.scroll_x = update_input_scroll_x(ed.scroll_x, raw_x, measured_w, world_rect, style);
        ed.scroll_y = 0.0;
        ed.needs_caret_scroll = false;
        (ed.scroll_x, 0.0, 0.0)
    } else {
        ed.scroll_x = 0.0;
        ed.scroll_y = update_textarea_scroll_y(
            ed.scroll_y,
            ed.needs_caret_scroll,
            line_top,
            line_h,
            measured_h,
            world_rect,
            style,
        );
        ed.needs_caret_scroll = false;
        let inner_h = textarea_visible_inner_height(world_rect, style);
        let max_scroll_y = (measured_h.max(line_top + line_h) - inner_h).max(0.0);
        (0.0, ed.scroll_y, max_scroll_y)
    }
}

pub(crate) fn compute_text_origin_x_offset(
    style: &CompiledStyle,
    role: SemanticRole,
    rect_width: f32,
    measured_width: f32,
) -> f32 {
    let cb = content_box_rect(
        LayoutRect {
            x: 0.0,
            y: 0.0,
            width: rect_width,
            height: 0.0,
        },
        &style.layout,
    );
    let is_single_line = matches!(
        role,
        SemanticRole::Button
            | SemanticRole::ComboBox
            | SemanticRole::TextInput
            | SemanticRole::PasswordInput
    );
    let shaped_had_max_w = rect_width > 0.0 && !is_single_line;
    let is_flex = style.layout.display == DisplayMode::Flex;
    let align_offset = if shaped_had_max_w
        && (style.text.text_align != TextAlign::Left || style.text.direction == TextDirection::Rtl)
    {
        0.0
    } else {
        match style.text.text_align {
            TextAlign::Center => ((cb.width - measured_width) * 0.5).max(0.0),
            TextAlign::Right => (cb.width - measured_width).max(0.0),
            TextAlign::Left if style.text.direction == TextDirection::Rtl => {
                (cb.width - measured_width).max(0.0)
            }
            TextAlign::Left
                if !style.text.text_align_explicit && is_flex && !role.is_text_editable() =>
            {
                match style.layout.justify_content {
                    JustifyContent::Center => ((cb.width - measured_width) * 0.5).max(0.0),
                    JustifyContent::FlexEnd => (cb.width - measured_width).max(0.0),
                    _ => 0.0,
                }
            }
            TextAlign::Left => 0.0,
        }
    };
    cb.x + align_offset
}

pub(super) fn compute_text_origins(
    style: &CompiledStyle,
    role: SemanticRole,
    world_rect: LayoutRect,
    shaped: &krui_text::ShapedTextLayout,
) -> (f32, f32) {
    let cb = content_box_rect(world_rect, &style.layout);
    let origin_x = world_rect.x
        + compute_text_origin_x_offset(style, role, world_rect.width, shaped.measured_width);
    let is_single_line = matches!(
        role,
        SemanticRole::Button
            | SemanticRole::ComboBox
            | SemanticRole::TextInput
            | SemanticRole::PasswordInput
    );
    let is_flex_centered =
        style.layout.display == DisplayMode::Flex && style.layout.align_items == AlignItems::Center;
    let origin_y = if is_single_line || (!role.is_text_editable() && is_flex_centered) {
        cb.y + (cb.height - shaped.measured_height) * 0.5
    } else {
        cb.y
    };
    (origin_x, origin_y)
}

fn ensure_node_shaped_text_layout(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    style: &CompiledStyle,
    role: SemanticRole,
    world_rect: LayoutRect,
    showing_placeholder: bool,
) -> Option<ShapedNodeTextMeta> {
    let current_scale_pct = (inner.scale_factor.clamp(1.0, 4.0) * 100.0).round() as u32;
    let (max_w, width_key) = compute_shaped_width_key(role, style, world_rect.width);
    let rich_opt = (!showing_placeholder)
        .then(|| inner.sparse.rich_spans.get(&node_id).cloned())
        .flatten()
        .filter(|s| !s.is_empty());
    let rich_hash = rich_opt
        .as_deref()
        .map_or(0_u64, crate::view_tree::layout_modes::hash_rich_spans);
    let eff_text = effective_node_text_style(role, style.text);
    let shaped_key = compute_node_shaped_key(&eff_text, current_scale_pct, width_key)
        ^ rich_hash.rotate_left(17);
    let (display_string, meta, need_reshape) =
        extract_node_display_and_meta(inner, node_id, showing_placeholder, shaped_key)?;

    if need_reshape && (rich_opt.is_some() || !display_string.is_empty()) {
        let scale = inner.scale_factor;
        let shaped = match rich_opt.as_deref() {
            Some(spans) => inner
                .text_engine
                .shape_styled_rich_text(spans, &eff_text, max_w, scale),
            None => inner
                .text_engine
                .shape_styled_text(&display_string, &eff_text, max_w, scale),
        };
        if let Some(node) = inner.node_mut(node_id) {
            node.cached_text_layout = Some(shaped);
            node.last_shaped_key = shaped_key;
            node.set_text_dirty(false);
        }
    } else if display_string.is_empty() && rich_opt.is_none() {
        if let Some(node) = inner.node_mut(node_id) {
            node.cached_text_layout = None;
            node.last_shaped_key = shaped_key;
            node.set_text_dirty(false);
        }
    }
    Some(meta)
}

fn extract_node_display_and_meta(
    inner: &ViewTreeInner,
    node_id: NodeId,
    showing_placeholder: bool,
    shaped_key: u64,
) -> Option<(String, ShapedNodeTextMeta, bool)> {
    let n = inner.node(node_id)?;
    let composed = compose_node_display_and_ime_ranges(inner, node_id);
    let ed = inner.text_edit(node_id);
    let disp = if showing_placeholder {
        ed.map(|e| e.placeholder.clone()).unwrap_or_default()
    } else {
        composed.display_string
    };
    let reshape =
        n.text_dirty() || n.cached_text_layout.is_none() || n.last_shaped_key != shaped_key;
    let meta = (
        composed.active_caret_char,
        composed.preedit_range,
        composed.clause_range,
        n.is_secret() && !showing_placeholder,
        ed.and_then(|e| e.selection_anchor_char_idx),
    );
    Some((disp, meta, reshape))
}
