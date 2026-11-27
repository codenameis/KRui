#![forbid(unsafe_code)]

use crate::view_tree::{NodeId, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_style::{
    Dimension, DisplayMode, LayoutStyle, LineHeight, PositionMode, TextStyle, WhiteSpaceMode,
};

pub(crate) const CARET_RIGHT_RESERVE_PX: f32 = 2.0;

pub(crate) fn effective_node_text_style(role: SemanticRole, mut t: TextStyle) -> TextStyle {
    if role.is_single_line_input() {
        t.white_space = WhiteSpaceMode::Pre;
    } else if role == SemanticRole::MultilineTextInput {
        t.white_space = WhiteSpaceMode::PreWrap;
    }
    t
}

pub(crate) fn content_box_rect(world_rect: LayoutRect, layout: &LayoutStyle) -> LayoutRect {
    let b = layout.effective_border_edges();
    let (bl, br, bt, bb) = (
        b.left.max(0.0),
        b.right.max(0.0),
        b.top.max(0.0),
        b.bottom.max(0.0),
    );
    let pad = layout.resolve_padding_px(world_rect.width);
    let x = world_rect.x + bl + pad.left;
    let y = world_rect.y + bt + pad.top;
    let width = (world_rect.width - bl - br - pad.left - pad.right).max(0.0);
    let height = (world_rect.height - bt - bb - pad.top - pad.bottom).max(0.0);
    LayoutRect {
        x,
        y,
        width,
        height,
    }
}

pub(crate) fn text_clip_box_rect(
    world_rect: LayoutRect,
    layout: &LayoutStyle,
    effective_paint_bw: f32,
) -> LayoutRect {
    let b = layout.effective_border_edges();
    let ebw = effective_paint_bw.max(0.0);
    let (bl, br, bt, bb) = (
        b.left.max(ebw).max(0.0),
        b.right.max(ebw).max(0.0),
        b.top.max(ebw).max(0.0),
        b.bottom.max(ebw).max(0.0),
    );
    LayoutRect {
        x: world_rect.x + bl,
        y: world_rect.y + bt,
        width: (world_rect.width - bl - br).max(0.0),
        height: (world_rect.height - bt - bb).max(0.0),
    }
}

pub(crate) fn sync_node_text_context(inner: &mut ViewTreeInner, node_id: NodeId) {
    sync_node_text_context_inner(inner, node_id, false);
}

pub(crate) fn sync_node_text_context_for_ime_preedit(inner: &mut ViewTreeInner, node_id: NodeId) {
    sync_node_text_context_inner(inner, node_id, true);
}

fn has_in_flow_child(inner: &ViewTreeInner, children: &[NodeId]) -> bool {
    children.iter().copied().any(|cid| {
        inner.node(cid).is_some_and(|child| {
            child.alive()
                && !child.is_anchor()
                && child.style.layout.display != DisplayMode::None
                && !matches!(
                    child.style.layout.position,
                    PositionMode::Absolute | PositionMode::Fixed
                )
        })
    })
}

fn sync_node_text_context_inner(inner: &mut ViewTreeInner, node_id: NodeId, is_ime_preedit: bool) {
    let Some((taffy_node, has_children, role, style)) = inner.node(node_id).and_then(|n| {
        (!n.is_anchor()).then_some((
            n.taffy_node?,
            has_in_flow_child(inner, &n.children),
            n.role,
            n.style,
        ))
    }) else {
        return;
    };
    if crate::view_tree::media_nodes::try_sync_media_intrinsic_context(inner, node_id, taffy_node) {
        return;
    }
    if has_children
        || matches!(
            role,
            SemanticRole::Checkbox | SemanticRole::RadioButton | SemanticRole::Slider
        )
    {
        inner.layout_engine.set_text_context(taffy_node, None);
        return;
    }
    let composed = crate::view_tree::text_edit::compose_node_display_and_ime_ranges(inner, node_id);
    let placeholder = inner
        .text_edit(node_id)
        .map_or("", |e| e.placeholder.as_str());
    let effective_text = if composed.display_string.is_empty() && !placeholder.is_empty() {
        placeholder.to_string()
    } else {
        composed.display_string
    };
    let single_line = role.is_single_line_input() || role == SemanticRole::ComboBox;
    let min_line_height =
        role.is_text_editable() || matches!(role, SemanticRole::Button | SemanticRole::ComboBox);
    let rich_spans = inner.sparse.rich_spans.get(&node_id).cloned();
    let has_rich = rich_spans.as_ref().is_some_and(|s| !s.is_empty());
    if effective_text.is_empty() && !has_rich && !min_line_height {
        inner.layout_engine.set_text_context(taffy_node, None);
        return;
    }
    // `taffy::TaffyTree::set_node_context` unconditionally marks the node dirty.
    let definite_w = !matches!(style.layout.width, Dimension::Auto);
    let definite_h = !matches!(style.layout.height, Dimension::Auto);
    let invariant_single_line_h = role.is_single_line_input()
        && style.text.text_box_trim == krui_style::TextBoxTrim::None
        && matches!(
            style.text.line_height,
            LineHeight::Normal | LineHeight::Px(_) | LineHeight::Factor(_)
        );
    let fixed_ime_box = is_ime_preedit
        && role.is_text_editable()
        && definite_w
        && (definite_h || invariant_single_line_h);
    if fixed_ime_box {
        return;
    }
    let ctx = krui_layout::TextContext {
        text: effective_text,
        text_style: effective_node_text_style(role, style.text),
        min_line_height,
        single_line,
        intrinsic_size: None,
        rich_spans,
    };
    inner.layout_engine.set_text_context(taffy_node, Some(ctx));
}
