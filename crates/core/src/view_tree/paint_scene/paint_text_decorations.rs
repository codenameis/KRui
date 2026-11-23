//! IME composition underlines, target-clause highlights, text selection boxes, and IME
//! cursor-offset scroll resolution for `ViewTree`.

use crate::scene::{DrawPrimitive, PaintScene};
use crate::view_tree::clip_shape::ClipState;
use crate::view_tree::hit_test::effective_node_world_matrix_and_rect;
use crate::view_tree::key_routing::clamp_vertical_span;
use crate::view_tree::node::is_node_text_selectable;
use crate::view_tree::paint::{push_flat_box, MIN_TEXT_HIGHLIGHT_HEIGHT_PX};
use crate::view_tree::paint_text::{compute_text_origins, ShapedNodeTextMeta, TextDrawContext};
use crate::view_tree::pointer_capture::SelectionGranularity;
use crate::view_tree::text_selection_granularity::{
    hit_test_node_char_at_local, line_bounds_for_char, word_bounds_for_char,
};
use crate::view_tree::ViewTreeInner;
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_style::{
    Affine2, Color, CompiledStyle, PaintStyle, UA_IME_TARGET_BG_COLOR, UA_PRIMARY_TEXT_COLOR,
    UA_SELECTION_BG_COLOR, UA_SELECTION_BORDER_COLOR,
};

const UA_IME_TARGET_UNDERLINE_PX: f32 = 2.0;
const UA_IME_UNDERLINE_PX: f32 = 1.0;

#[allow(clippy::too_many_arguments)]
pub(super) fn build_text_draw_ctx(
    (world_rect, local_clip, effective_border_width): (LayoutRect, LayoutRect, f32),
    role: SemanticRole,
    showing_placeholder: bool,
    scroll: (f32, f32),
    (text_color, eff_paint): (Color, &PaintStyle),
    clip: ClipState,
    transform: Affine2,
    opacity: f32,
) -> TextDrawContext {
    TextDrawContext {
        world_rect,
        local_clip,
        effective_border_width,
        role,
        is_inp_role: role.is_text_editable(),
        showing_placeholder,
        scroll_x: scroll.0,
        scroll_y: scroll.1,
        text_color,
        caret_color: eff_paint.resolve_caret_color(text_color),
        selection_bg: eff_paint
            .selection_background
            .unwrap_or(UA_SELECTION_BG_COLOR),
        selection_fg: eff_paint.selection_color,
        custom_selection_bg: eff_paint.selection_background.is_some(),
        clip_bounds: clip.rect,
        clip_radius: clip.max_radius(),
        clip_radii: clip.radii,
        transform,
        opacity,
    }
}

fn resolve_highlight_vertical_span(
    origin_y: f32,
    ry: f32,
    rh: f32,
    ctx: TextDrawContext,
) -> (f32, f32) {
    let min_h = if ctx.is_inp_role {
        MIN_TEXT_HIGHLIGHT_HEIGHT_PX.min(ctx.local_clip.height.max(1.0))
    } else {
        MIN_TEXT_HIGHLIGHT_HEIGHT_PX
    };
    let h = rh.max(min_h);
    let y = (origin_y + ry + rh * 0.5) - h * 0.5;
    if ctx.is_inp_role {
        clamp_vertical_span(y, h, ctx.local_clip)
    } else {
        (y, h)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_text_selection_and_clause_boxes(
    scene: &mut PaintScene,
    shaped: &krui_text::ShapedTextLayout,
    origin_x: f32,
    origin_y: f32,
    active_cursor_char: usize,
    clause_range: Option<(usize, usize)>,
    sel_anchor: Option<usize>,
    ctx: TextDrawContext,
) {
    if let Some((cl_start, cl_end)) = clause_range {
        for (rx, ry, rw, rh) in shaped.selection_rects_for_range(cl_start, cl_end) {
            let (box_y, box_h) = resolve_highlight_vertical_span(origin_y, ry, rh, ctx);
            push_flat_box(
                scene,
                origin_x + rx,
                box_y,
                rw.max(2.0),
                box_h,
                ClipState::from_opt(ctx.clip_bounds, ctx.clip_radius, ctx.clip_radii),
                ctx.transform,
                ctx.opacity,
                UA_IME_TARGET_BG_COLOR,
                2.0,
            );
        }
    }
    if let Some(anchor) = sel_anchor.filter(|&a| a != active_cursor_char) {
        for (rx, ry, rw, rh) in shaped.selection_rects_for_range(anchor, active_cursor_char) {
            let (box_y, box_h) = resolve_highlight_vertical_span(origin_y, ry, rh, ctx);
            push_selection_highlight_box(scene, (origin_x + rx, box_y, rw.max(2.0), box_h), ctx);
        }
    }
}

fn push_selection_highlight_box(
    scene: &mut PaintScene,
    (x, y, width, height): (f32, f32, f32, f32),
    ctx: TextDrawContext,
) {
    let (border_color, border_width) = if ctx.custom_selection_bg {
        (Color::TRANSPARENT, 0.0)
    } else {
        (UA_SELECTION_BORDER_COLOR, 1.0)
    };
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: LayoutRect {
            x,
            y,
            width,
            height,
        },
        clip: ctx.clip_bounds,
        clip_radius: ctx.clip_radius,
        clip_radii: ctx.clip_radii,
        transform: ctx.transform,
        opacity: ctx.opacity,
        background_color: ctx.selection_bg,
        background_gradient: None,
        radial_gradient: None,
        border_color,
        border_width,
        border_radius: 3.0,
        corner_radii: [3.0; 4],
        box_shadow: None,
        backdrop_blur_px: 0.0,
    });
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_ime_underlines(
    scene: &mut PaintScene,
    shaped: &krui_text::ShapedTextLayout,
    origin_x: f32,
    origin_y: f32,
    (pre_start, pre_end): (usize, usize),
    clause_range: Option<(usize, usize)>,
    scale_factor: f32,
    ctx: TextDrawContext,
) {
    let visuals =
        krui_text::compute_ime_visuals(shaped, pre_start..pre_end, clause_range.map(|(a, b)| a..b));
    let inv_scale = 1.0 / scale_factor.max(1.0);
    for seg in visuals.underline_segments {
        let thickness = if seg.is_target_clause {
            UA_IME_TARGET_UNDERLINE_PX
        } else {
            UA_IME_UNDERLINE_PX
        };
        let underline_y = compute_segment_underline_y(
            shaped,
            origin_y,
            &seg,
            (pre_start, pre_end),
            thickness,
            inv_scale,
            ctx,
        );
        push_flat_box(
            scene,
            origin_x + seg.x,
            underline_y,
            seg.width.max(2.0),
            thickness,
            ClipState::from_opt(ctx.clip_bounds, ctx.clip_radius, ctx.clip_radii),
            ctx.transform,
            ctx.opacity,
            UA_PRIMARY_TEXT_COLOR,
            0.0,
        );
    }
}

fn compute_segment_underline_y(
    shaped: &krui_text::ShapedTextLayout,
    origin_y: f32,
    seg: &krui_text::ImeUnderlineSegment,
    (pre_start, pre_end): (usize, usize),
    thickness: f32,
    inv_scale: f32,
    ctx: TextDrawContext,
) -> f32 {
    let line_opt = shaped.lines.get(seg.line_index);
    let mut ideal = if let Some(ln) = line_opt {
        origin_y + ln.baseline_y + ln.descent
    } else {
        origin_y + seg.y + seg.height_hint - thickness
    };
    if let Some(ln) = line_opt {
        let (ls, le) = (
            pre_start.max(ln.start_char) as u32,
            pre_end.min(ln.end_char) as u32,
        );
        for g in shaped.glyphs.iter() {
            if g.char_end > ls && g.char_start < le {
                let ink_bot = origin_y + g.y + (g.height as f32) * inv_scale;
                ideal = ideal.max(ink_bot + 1.0);
            }
        }
        if let Some(next_ln) = shaped.lines.get(seg.line_index + 1) {
            ideal = ideal.min(origin_y + next_ln.line_top - thickness);
        }
    }
    if ctx.is_inp_role && ctx.local_clip.height > 0.0 {
        let max_y = (ctx.local_clip.y + ctx.local_clip.height - thickness).max(ctx.local_clip.y);
        ideal.clamp(ctx.local_clip.y, max_y)
    } else {
        ideal
    }
}

pub(crate) fn single_line_input_align_dx(
    style: &CompiledStyle,
    role: SemanticRole,
    eff_rect: LayoutRect,
    measured_w: f32,
) -> f32 {
    if !role.is_single_line_input() {
        return 0.0;
    }
    let zero_rect = LayoutRect {
        x: 0.0,
        y: 0.0,
        width: eff_rect.width.max(0.0),
        height: 0.0,
    };
    let cb = crate::view_tree::content_box::content_box_rect(zero_rect, &style.layout);
    let origin_x = crate::view_tree::paint_text::compute_text_origin_x_offset(
        style,
        role,
        eff_rect.width.max(0.0),
        measured_w,
    );
    (origin_x - cb.x).max(0.0)
}

pub(crate) fn apply_text_selection_drag(inner: &mut ViewTreeInner, x: f32, y: f32) -> bool {
    let Some(nid) = inner.selecting_node else {
        return false;
    };
    if !inner.node(nid).is_some_and(is_node_text_selectable) {
        return false;
    }
    let (world_affine, local_rect) = effective_node_world_matrix_and_rect(inner, nid);
    let Some((lx, ly)) = world_affine.inverse().map(|inv| inv.transform_point(x, y)) else {
        return false;
    };
    let Some((drag_char, char_count, text, is_single_line)) =
        hit_test_node_char_at_local(inner, nid, lx, ly, local_rect)
    else {
        return false;
    };
    let gran = inner.interaction.selection_granularity;
    let ed = inner.ensure_text_edit(nid);
    let (next_anchor, next_cursor) = resolve_drag_selection_range(
        gran,
        drag_char,
        char_count,
        &text,
        is_single_line,
        ed.selection_anchor_char_idx,
    );
    if ed.selection_anchor_char_idx != next_anchor || ed.cursor_char_idx != next_cursor {
        ed.selection_anchor_char_idx = next_anchor;
        ed.cursor_char_idx = next_cursor;
        ed.preferred_x = None;
        ed.needs_caret_scroll = true;
        inner.caret_visible = true;
        inner.paint_dirty = true;
        true
    } else {
        false
    }
}

fn resolve_drag_selection_range(
    gran: SelectionGranularity,
    drag_char: usize,
    char_count: usize,
    text: &str,
    is_single_line: bool,
    current_anchor: Option<usize>,
) -> (Option<usize>, usize) {
    match gran {
        SelectionGranularity::Char => (current_anchor, drag_char),
        SelectionGranularity::Word {
            anchor_start,
            anchor_end,
        } => {
            let span = word_bounds_for_char(text, drag_char);
            compute_granularity_drag_span((anchor_start, anchor_end), span)
        }
        SelectionGranularity::Line {
            anchor_start,
            anchor_end,
        } => {
            if is_single_line {
                (Some(0), char_count)
            } else {
                let span = line_bounds_for_char(text, drag_char);
                compute_granularity_drag_span((anchor_start, anchor_end), span)
            }
        }
    }
}

fn compute_granularity_drag_span(
    (anchor_start, anchor_end): (usize, usize),
    (span_start, span_end): (usize, usize),
) -> (Option<usize>, usize) {
    if span_start < anchor_start {
        (Some(anchor_end), span_start)
    } else if span_end > anchor_end {
        (Some(anchor_start), span_end)
    } else {
        (Some(anchor_start), anchor_end)
    }
}

fn resolve_shaped_text_layout_positions(
    shaped: &krui_text::ShapedTextLayout,
    style: &CompiledStyle,
    active_cursor_char: usize,
    ctx: TextDrawContext,
) -> (f32, f32, f32) {
    let (base_origin_x, base_origin_y) =
        compute_text_origins(style, ctx.role, ctx.world_rect, shaped);
    let (origin_x, origin_y) = (base_origin_x - ctx.scroll_x, base_origin_y - ctx.scroll_y);
    let measured_w = if ctx.showing_placeholder {
        0.0
    } else {
        shaped.measured_width
    };
    let align_dx = single_line_input_align_dx(style, ctx.role, ctx.world_rect, measured_w);
    let cursor_x = if ctx.showing_placeholder {
        align_dx
    } else {
        (shaped.cursor_x_for_char(active_cursor_char) + align_dx - ctx.scroll_x).max(0.0)
    };
    (origin_x, origin_y, cursor_x)
}

pub(super) fn emit_shaped_glyph_and_ime_primitives(
    scene: &mut PaintScene,
    shaped: &krui_text::ShapedTextLayout,
    style: &CompiledStyle,
    meta: ShapedNodeTextMeta,
    ctx: TextDrawContext,
) -> f32 {
    let (active_cursor_char, preedit_range, clause_range, is_secret, sel_anchor) = meta;
    let (origin_x, origin_y, cursor_x) =
        resolve_shaped_text_layout_positions(shaped, style, active_cursor_char, ctx);
    emit_text_selection_and_clause_boxes(
        scene,
        shaped,
        origin_x,
        origin_y,
        active_cursor_char,
        if is_secret { None } else { clause_range },
        sel_anchor,
        ctx,
    );
    let glyphs = recolor_selected_glyphs_if_needed(
        &shaped.glyphs,
        sel_anchor,
        active_cursor_char,
        ctx.selection_fg,
    );
    scene.primitives.push(DrawPrimitive::TextGlyphs {
        origin_x,
        origin_y,
        clip: ctx.clip_bounds,
        clip_radius: ctx.clip_radius,
        clip_radii: ctx.clip_radii,
        transform: ctx.transform,
        opacity: ctx.opacity,
        color: ctx.text_color,
        glyphs,
    });
    if !is_secret {
        if let Some(pre_range) = preedit_range {
            emit_ime_underlines(
                scene,
                shaped,
                origin_x,
                origin_y,
                pre_range,
                clause_range,
                scene.scale_factor,
                ctx,
            );
        }
    }
    cursor_x
}

fn recolor_selected_glyphs_if_needed(
    glyphs: &std::sync::Arc<[krui_text::RasterizedGlyph]>,
    sel_anchor: Option<usize>,
    active_cursor_char: usize,
    selection_fg: Option<Color>,
) -> std::sync::Arc<[krui_text::RasterizedGlyph]> {
    let Some(fg) = selection_fg else {
        return glyphs.clone();
    };
    let Some(anchor) = sel_anchor.filter(|&a| a != active_cursor_char) else {
        return glyphs.clone();
    };
    let (s, e) = (
        anchor.min(active_cursor_char) as u32,
        anchor.max(active_cursor_char) as u32,
    );
    let mut out = glyphs.to_vec();
    for g in &mut out {
        if g.char_end > s && g.char_start < e {
            g.glyph_color = Some(fg);
        }
    }
    std::sync::Arc::from(out)
}
