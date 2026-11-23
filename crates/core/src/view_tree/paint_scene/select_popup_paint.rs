//! Top-Layer `<select>` (`SemanticRole::ComboBox`) dropdown popup scene emission (`ThemeTokens` only).

use crate::scene::{DrawPrimitive, PaintScene};
use crate::view_tree::clip_shape::ClipState;
use crate::view_tree::paint::{emit_scrollbar_thumb, push_flat_box};
use crate::view_tree::select_popup::{
    compute_select_popup_geometry, resolve_current_select_index, SelectPopupGeometry,
    SelectPopupState, SELECT_POPUP_PADDING_PX, SELECT_POPUP_ROW_HEIGHT_PX,
    SELECT_POPUP_TEXT_INSET_X_PX,
};
use crate::view_tree::ViewTreeInner;
use krui_style::{
    Affine2, ScrollbarWidth, UA_AUTO_ACCENT_COLOR, UA_POPUP_BG_COLOR, UA_POPUP_BORDER_COLOR,
    UA_PRIMARY_TEXT_COLOR, UA_SECONDARY_TEXT_COLOR,
};

const SELECT_POPUP_ACTIVE_ROW_OPACITY: f32 = 0.22;
const SELECT_POPUP_SELECTED_ROW_OPACITY: f32 = 0.11;
const SELECT_POPUP_ROW_RADIUS_PX: f32 = 4.0;
const SELECT_POPUP_BORDER_WIDTH_PX: f32 = 1.0;
const SELECT_POPUP_CORNER_RADIUS_PX: f32 = 6.0;

/// Emits the active Top-Layer `<select>` dropdown popup above all DOM containers (`clip: None`).
pub(crate) fn emit_select_popup_overlay(inner: &mut ViewTreeInner, scene: &mut PaintScene) {
    let Some(popup) = inner.overlay.select_popup.clone() else {
        return;
    };
    let Some(geom) = compute_select_popup_geometry(inner, popup.select_id) else {
        inner.overlay.select_popup = None;
        return;
    };
    let scroll_y = popup.scroll_y.clamp(0.0, geom.max_scroll_y);
    if let Some(p) = inner.overlay.select_popup.as_mut() {
        p.scroll_y = scroll_y;
    }
    scene.primitives.push(DrawPrimitive::StyledBox {
        rect: geom.popup_rect,
        clip: None,
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        background_color: UA_POPUP_BG_COLOR,
        background_gradient: None,
        radial_gradient: None,
        border_color: UA_POPUP_BORDER_COLOR,
        border_width: SELECT_POPUP_BORDER_WIDTH_PX,
        border_radius: SELECT_POPUP_CORNER_RADIUS_PX,
        corner_radii: [SELECT_POPUP_CORNER_RADIUS_PX; 4],
        box_shadow: None,
        backdrop_blur_px: 0.0,
    });
    emit_select_popup_rows(inner, &popup, geom, scroll_y, scene);
    let (sw, sc) = inner
        .node(popup.select_id)
        .map_or((ScrollbarWidth::Auto, None), |n| {
            (
                n.style.layout.scrollbar_width,
                n.style.paint.scrollbar_color,
            )
        });
    emit_scrollbar_thumb(
        scene,
        geom.popup_rect,
        scroll_y,
        geom.max_scroll_y,
        ClipState::NONE,
        Affine2::IDENTITY,
        1.0,
        sw,
        sc,
    );
}

fn emit_select_popup_rows(
    inner: &mut ViewTreeInner,
    popup: &SelectPopupState,
    geom: SelectPopupGeometry,
    scroll_y: f32,
    scene: &mut PaintScene,
) {
    let Some(text_style) = inner.node(popup.select_id).map(|n| n.style.text) else {
        return;
    };
    let selected_idx = resolve_current_select_index(inner, popup.select_id);
    let options = inner
        .control(popup.select_id)
        .map(|c| c.select_options.clone())
        .unwrap_or_default();
    for (idx, (val, label, _)) in options.iter().enumerate() {
        let row_y = geom.inner_clip.y + idx as f32 * SELECT_POPUP_ROW_HEIGHT_PX - scroll_y;
        if row_y + SELECT_POPUP_ROW_HEIGHT_PX <= geom.inner_clip.y
            || row_y >= geom.inner_clip.y + geom.inner_clip.height
        {
            continue;
        }
        let disp = if label.is_empty() { val } else { label };
        emit_single_select_popup_row(
            inner,
            geom,
            row_y,
            disp,
            (idx == popup.highlighted_index, idx == selected_idx),
            &text_style,
            scene,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_single_select_popup_row(
    inner: &mut ViewTreeInner,
    geom: SelectPopupGeometry,
    row_y: f32,
    disp: &str,
    (is_hi, is_sel): (bool, bool),
    text_style: &krui_style::TextStyle,
    scene: &mut PaintScene,
) {
    if is_hi || is_sel {
        let row_alpha = if is_hi {
            SELECT_POPUP_ACTIVE_ROW_OPACITY
        } else {
            SELECT_POPUP_SELECTED_ROW_OPACITY
        };
        push_flat_box(
            scene,
            geom.popup_rect.x + SELECT_POPUP_PADDING_PX,
            row_y,
            (geom.popup_rect.width - SELECT_POPUP_PADDING_PX * 2.0).max(1.0),
            SELECT_POPUP_ROW_HEIGHT_PX,
            ClipState::from_opt(Some(geom.inner_clip), 0.0, [0.0; 4]),
            Affine2::IDENTITY,
            row_alpha,
            UA_AUTO_ACCENT_COLOR,
            SELECT_POPUP_ROW_RADIUS_PX,
        );
    }
    if disp.is_empty() {
        return;
    }
    let text_color = if is_hi || is_sel {
        UA_PRIMARY_TEXT_COLOR
    } else {
        UA_SECONDARY_TEXT_COLOR
    };
    let mut row_text_style = *text_style;
    row_text_style.text_align = krui_style::TextAlign::Left;
    let shaped =
        inner
            .text_engine
            .shape_styled_text(disp, &row_text_style, None, inner.scale_factor);
    let text_y = row_y + ((SELECT_POPUP_ROW_HEIGHT_PX - shaped.measured_height) * 0.5).max(2.0);
    scene.primitives.push(DrawPrimitive::TextGlyphs {
        origin_x: geom.popup_rect.x + SELECT_POPUP_TEXT_INSET_X_PX,
        origin_y: text_y,
        clip: Some(geom.inner_clip),
        clip_radius: 0.0,
        clip_radii: [0.0; 4],
        transform: Affine2::IDENTITY,
        opacity: 1.0,
        color: text_color,
        glyphs: shaped.glyphs,
    });
}
