//! Top-Layer native `title="..."` plain tooltip overlay rendered above all DOM clip containers.

use crate::scene::{DrawPrimitive, PaintScene};
use crate::view_tree::hit_test::{effective_node_world_matrix_and_rect, transform_rect_aabb};
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_layout::LayoutRect;
use krui_style::{Affine2, UA_POPUP_BORDER_COLOR, UA_TOOLTIP_BG_COLOR, UA_TOOLTIP_FG_COLOR};

const TOOLTIP_FONT_SIZE_PX: f32 = 12.0;
const TOOLTIP_LINE_HEIGHT_PX: f32 = 16.0;
const TOOLTIP_FONT_WEIGHT: u16 = 500;
const TOOLTIP_PAD_X_PX: f32 = 10.0;
const TOOLTIP_PAD_Y_PX: f32 = 6.0;
const TOOLTIP_GAP_PX: f32 = 6.0;
const TOOLTIP_MAX_WIDTH_PX: f32 = 280.0;
const TOOLTIP_VIEWPORT_MARGIN_PX: f32 = 6.0;
const TOOLTIP_BORDER_WIDTH_PX: f32 = 1.0;
const TOOLTIP_CORNER_RADIUS_PX: f32 = 6.0;

/// Emits a viewport-clamped plain tooltip in the Top-Layer (`clip: None`) when the hovered
/// node or any ancestor has a non-empty `title="..."` attribute.
pub(crate) fn emit_hover_tooltip_overlay(
    inner: &mut ViewTreeInner,
    (viewport_w, viewport_h): (f32, f32),
    scene: &mut PaintScene,
) {
    if inner.overlay.select_popup.is_some() || inner.pressed_node.is_some() {
        return;
    }
    let Some((anchor_id, title_text)) = find_hovered_title_anchor(inner) else {
        return;
    };
    let Some(font_family) = inner.node(anchor_id).map(|n| n.style.text.font_family) else {
        return;
    };
    let (mat, local_rect) = effective_node_world_matrix_and_rect(inner, anchor_id);
    let anchor_aabb = transform_rect_aabb(mat, local_rect);
    let bg = UA_TOOLTIP_BG_COLOR;
    let fg = UA_TOOLTIP_FG_COLOR;

    let tooltip_text_style = krui_style::TextStyle {
        font_size: TOOLTIP_FONT_SIZE_PX,
        font_weight: TOOLTIP_FONT_WEIGHT,
        line_height: krui_style::LineHeight::Px(TOOLTIP_LINE_HEIGHT_PX),
        font_family,
        ..krui_style::TextStyle::default()
    };
    let shaped = inner.text_engine.shape_styled_text(
        &title_text,
        &tooltip_text_style,
        Some(TOOLTIP_MAX_WIDTH_PX),
        inner.scale_factor,
    );
    let box_rect = compute_tooltip_box_rect(
        anchor_aabb,
        shaped.measured_width,
        shaped.measured_height,
        (viewport_w.max(1.0), viewport_h.max(1.0)),
    );
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: box_rect,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        background_color: bg,
        background_gradient: None,
        radial_gradient: None,
        border_color: UA_POPUP_BORDER_COLOR,
        border_width: TOOLTIP_BORDER_WIDTH_PX,
        border_radius: TOOLTIP_CORNER_RADIUS_PX,
        corner_radii: [TOOLTIP_CORNER_RADIUS_PX; 4],
        box_shadow: None,
        backdrop_blur_px: 0.0,
    });
    scene.primitives.push(DrawPrimitive::TextGlyphs {
        origin_x: box_rect.x + TOOLTIP_PAD_X_PX,
        origin_y: box_rect.y + TOOLTIP_PAD_Y_PX,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        color: fg,
        glyphs: shaped.glyphs,
    });
}

fn find_hovered_title_anchor(inner: &ViewTreeInner) -> Option<(NodeId, String)> {
    let mut cur = inner.hovered_node;
    while let Some(nid) = cur {
        if let Some(title) = inner
            .control(nid)
            .and_then(|c| c.title.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some((nid, title.to_string()));
        }
        cur = inner.node(nid).and_then(|n| n.parent);
    }
    None
}

fn compute_tooltip_box_rect(
    anchor: LayoutRect,
    text_w: f32,
    text_h: f32,
    (vp_w, vp_h): (f32, f32),
) -> LayoutRect {
    let box_w = text_w.max(12.0) + TOOLTIP_PAD_X_PX * 2.0;
    let box_h = text_h.max(TOOLTIP_LINE_HEIGHT_PX) + TOOLTIP_PAD_Y_PX * 2.0;
    let margin = TOOLTIP_VIEWPORT_MARGIN_PX;

    let centered_x = anchor.x + (anchor.width - box_w) * 0.5;
    let max_x = (vp_w - box_w - margin).max(margin);
    let box_x = centered_x.clamp(margin, max_x);

    let below_y = anchor.y + anchor.height + TOOLTIP_GAP_PX;
    let box_y = if below_y + box_h + margin <= vp_h {
        below_y
    } else {
        (anchor.y - box_h - TOOLTIP_GAP_PX).max(margin)
    };
    LayoutRect {
        x: box_x,
        y: box_y,
        width: box_w,
        height: box_h,
    }
}
