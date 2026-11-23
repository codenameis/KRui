//! Collapses eligible text-flow containers holding bare inline runs/tags (`<b>`, `<strong>`,
//! `<em>`, `<i>`, `<code>`, unboxed `<span>`) into a single multi-span `rich_spans` paragraph node
//! so mixed inline styling wraps cleanly within one cosmic-text paragraph box.

use crate::ir::CompiledRichSpan;
use crate::{CompiledNode, CompiledNodeKind, HtmlTag};
use krui_style::{Color, FontFamilyStack};

const INLINE_BOLD_WEIGHT: u16 = 700;
const INLINE_NORMAL_WEIGHT: u16 = 400;

fn is_rich_paragraph_tag(tag: HtmlTag) -> bool {
    matches!(
        tag,
        HtmlTag::P
            | HtmlTag::H1
            | HtmlTag::H2
            | HtmlTag::H3
            | HtmlTag::H4
            | HtmlTag::H5
            | HtmlTag::H6
            | HtmlTag::Li
            | HtmlTag::Label
    )
}

fn has_child_interactions_or_bindings(c: &CompiledNode) -> bool {
    let has_events = c.on_click.is_some()
        || c.on_focus.is_some()
        || c.on_blur.is_some()
        || c.on_pointer_down.is_some()
        || c.on_pointer_up.is_some()
        || c.on_pointer_move.is_some()
        || c.on_scroll.is_some()
        || c.on_input.is_some()
        || c.on_change.is_some()
        || c.on_keydown.is_some()
        || c.on_submit.is_some()
        || c.on_reset.is_some()
        || c.bind_value.is_some()
        || c.bind_checked.is_some()
        || c.canvas_draw.is_some();
    let has_attrs = c.disabled.is_some()
        || c.required.is_some()
        || c.checked.is_some()
        || c.aria_label.is_some()
        || !c.aria_label_parts.is_empty()
        || !c.title_parts.is_empty()
        || !c.img_src.is_empty()
        || !c.link_href.is_empty()
        || c.svg_data.is_some();
    has_events
        || has_attrs
        || !c.paint_bindings.is_empty()
        || !c.layout_bindings.is_empty()
        || !c.dynamic_inline_style_parts.is_empty()
}

fn parse_simple_inline_color(style_str: Option<&str>) -> Option<Option<Color>> {
    let Some(raw) = style_str.map(str::trim).filter(|s| !s.is_empty()) else {
        return Some(None);
    };
    let clean = raw.trim_end_matches(';').trim();
    if clean.contains(';') {
        return None;
    }
    let (prop, val) = clean.split_once(':')?;
    if !prop.trim().eq_ignore_ascii_case("color") {
        return None;
    }
    Color::parse_css(val.trim()).map(Some)
}

fn has_non_flow_display(style_str: Option<&str>) -> bool {
    let Some(raw) = style_str else {
        return false;
    };
    let lower = raw.to_ascii_lowercase();
    lower.contains("display: flex")
        || lower.contains("display:flex")
        || lower.contains("display: grid")
        || lower.contains("display:grid")
        || lower.contains("display: none")
        || lower.contains("display:none")
        || lower.contains("display: contents")
        || lower.contains("display:contents")
}

fn can_collapse_inline_child(c: &CompiledNode) -> bool {
    matches!(c.kind, CompiledNodeKind::Element)
        && matches!(
            c.tag,
            HtmlTag::Span | HtmlTag::B | HtmlTag::Strong | HtmlTag::Em | HtmlTag::I | HtmlTag::Code
        )
        && c.id.is_none()
        && c.classes.is_empty()
        && c.class_parts.is_empty()
        && c.static_attrs.is_empty()
        && c.dynamic_attrs.is_empty()
        && c.children.is_empty()
        && c.rich_spans.is_empty()
        && !c.text_parts.is_empty()
        && !has_child_interactions_or_bindings(c)
        && parse_simple_inline_color(c.static_inline_style.as_deref()).is_some()
}

/// Collapses pure inline children of a text-flow block element into `node.rich_spans`.
pub(crate) fn maybe_collapse_rich_inline_paragraph(
    node: &mut CompiledNode,
    (explicit_flex_dir, explicit_align_items): (bool, bool),
) {
    if !is_rich_paragraph_tag(node.tag)
        || !node.text_parts.is_empty()
        || node.children.is_empty()
        || explicit_flex_dir
        || explicit_align_items
        || has_non_flow_display(node.static_inline_style.as_deref())
        || !node.children.iter().all(can_collapse_inline_child)
    {
        return;
    }
    let spans = std::mem::take(&mut node.children)
        .into_iter()
        .map(|ch| {
            let color = parse_simple_inline_color(ch.static_inline_style.as_deref()).flatten();
            let font_weight = match ch.tag {
                HtmlTag::B | HtmlTag::Strong => INLINE_BOLD_WEIGHT,
                _ => INLINE_NORMAL_WEIGHT,
            };
            let italic = matches!(ch.tag, HtmlTag::Em | HtmlTag::I);
            let font_family = if ch.tag == HtmlTag::Code {
                FontFamilyStack::CosmicMono
            } else {
                FontFamilyStack::FluentUI
            };
            CompiledRichSpan {
                parts: ch.text_parts,
                font_weight,
                italic,
                font_family,
                color,
            }
        })
        .collect();
    node.rich_spans = spans;
}
