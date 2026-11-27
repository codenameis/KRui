//! Internal conversion from Stylo [`ComputedValues`] to KRui [`TextStyle`] and
//! [`InheritedTextProps`].

use app_units::Au;
use krui_style::{
    FontFamilyStack, FontVariation, FontVariationList, InheritedTextProps, LineHeight,
    OverflowWrapMode, TextAlign, TextDirection, TextStyle, WhiteSpaceMode, WordBreakMode,
    MAX_FONT_VARIATIONS,
};
use style::properties::longhands::direction::computed_value::T as StyloDirection;
use style::properties::longhands::overflow_wrap::computed_value::T as StyloOverflowWrap;
use style::properties::longhands::text_wrap_mode::computed_value::T as StyloTextWrapMode;
use style::properties::longhands::white_space_collapse::computed_value::T as StyloWsCollapse;
use style::properties::longhands::word_break::computed_value::T as StyloWordBreak;
use style::properties::ComputedValues;
use style::values::computed::font::{
    FontStyle as StyloFontStyle, GenericFontFamily, SingleFontFamily,
};
use style::values::computed::TextAlign as StyloTextAlign;
use style::values::generics::font::LineHeight as StyloLineHeight;
use style::values::specified::text::TextOverflowSide;

use crate::computed_to_paint::absolute_color_to_krui;

const MIN_FONT_WEIGHT: f32 = 1.0;
const MAX_FONT_WEIGHT: f32 = 1000.0;

fn extract_font_family(style: &ComputedValues) -> FontFamilyStack {
    let families = &style.get_font().font_family.families.list;
    if families.is_empty() {
        return FontFamilyStack::FluentUI;
    }
    let mut names = Vec::with_capacity(families.len());
    for entry in families.iter() {
        match entry {
            SingleFontFamily::FamilyName(name) => {
                names.push(name.name.to_string());
            }
            SingleFontFamily::Generic(generic) => {
                let s = match generic {
                    GenericFontFamily::Serif => "serif",
                    GenericFontFamily::SansSerif => "sans-serif",
                    GenericFontFamily::Monospace => "monospace",
                    GenericFontFamily::Cursive => "cursive",
                    GenericFontFamily::Fantasy => "fantasy",
                    GenericFontFamily::SystemUi => "system-ui",
                    GenericFontFamily::None => continue,
                };
                names.push(s.to_owned());
            }
        }
    }
    if names.is_empty() {
        FontFamilyStack::FluentUI
    } else {
        FontFamilyStack::parse_css(&names.join(", "))
    }
}

fn extract_font_variations(style: &ComputedValues) -> FontVariationList {
    let mut out = FontVariationList::EMPTY;
    for entry in style.get_font().font_variation_settings.0.iter() {
        let len = out.len as usize;
        if len >= MAX_FONT_VARIATIONS {
            break;
        }
        out.items[len] = FontVariation {
            tag: entry.tag.0.to_be_bytes(),
            value: entry.value,
        };
        out.len += 1;
    }
    out
}

fn extract_wrapping_modes(
    ita: &style::properties::style_structs::InheritedText,
) -> (WhiteSpaceMode, WordBreakMode, OverflowWrapMode) {
    let white_space = match (ita.white_space_collapse, ita.text_wrap_mode) {
        (StyloWsCollapse::Preserve, StyloTextWrapMode::Nowrap) => WhiteSpaceMode::Pre,
        (StyloWsCollapse::Preserve, StyloTextWrapMode::Wrap) => WhiteSpaceMode::PreWrap,
        (StyloWsCollapse::PreserveBreaks, StyloTextWrapMode::Nowrap) => WhiteSpaceMode::NoWrap,
        (StyloWsCollapse::PreserveBreaks, StyloTextWrapMode::Wrap) => WhiteSpaceMode::PreLine,
        (StyloWsCollapse::BreakSpaces, _) => WhiteSpaceMode::BreakSpaces,
        (StyloWsCollapse::Collapse, StyloTextWrapMode::Nowrap) => WhiteSpaceMode::NoWrap,
        _ => WhiteSpaceMode::Normal,
    };
    let word_break = match ita.word_break {
        StyloWordBreak::BreakAll => WordBreakMode::BreakAll,
        StyloWordBreak::KeepAll => WordBreakMode::KeepAll,
        StyloWordBreak::Normal => WordBreakMode::Normal,
    };
    let overflow_wrap = match ita.overflow_wrap {
        StyloOverflowWrap::BreakWord => OverflowWrapMode::BreakWord,
        StyloOverflowWrap::Anywhere => OverflowWrapMode::Anywhere,
        StyloOverflowWrap::Normal => OverflowWrapMode::Normal,
    };
    (white_space, word_break, overflow_wrap)
}

/// Extracts resolved typography settings from a Stylo [`ComputedValues`] snapshot.
#[must_use]
pub(crate) fn extract_text_style(style: &ComputedValues) -> (TextStyle, InheritedTextProps) {
    let font = style.get_font();
    let ita = style.get_inherited_text();
    let font_size = font.font_size.computed_size().px();
    let font_weight = font
        .font_weight
        .value()
        .round()
        .clamp(MIN_FONT_WEIGHT, MAX_FONT_WEIGHT) as u16;
    let italic = font.font_style != StyloFontStyle::NORMAL;
    let letter_spacing_px = ita
        .letter_spacing
        .0
        .to_pixel_length(Au::from_f32_px(font_size))
        .px();
    let (white_space, word_break, overflow_wrap) = extract_wrapping_modes(ita);
    let to = &style.get_text().text_overflow;
    let text_overflow_ellipsis =
        to.second == TextOverflowSide::Ellipsis || to.first == TextOverflowSide::Ellipsis;
    let line_clamp = crate::servo_ext::extract_line_clamp(style);
    let line_height = match font.line_height {
        StyloLineHeight::Normal => LineHeight::Normal,
        StyloLineHeight::Number(n) => LineHeight::Factor(n.0.max(0.0)),
        StyloLineHeight::Length(len) => LineHeight::Px(len.0.px().max(0.0)),
    };
    let font_family = extract_font_family(style);
    let direction = match style.get_inherited_box().direction {
        StyloDirection::Rtl => TextDirection::Rtl,
        StyloDirection::Ltr => TextDirection::Ltr,
    };
    let text_align = match (style.clone_text_align(), direction) {
        (StyloTextAlign::Center | StyloTextAlign::MozCenter, _) => TextAlign::Center,
        (StyloTextAlign::Right | StyloTextAlign::MozRight, _) => TextAlign::Right,
        (StyloTextAlign::Start, TextDirection::Rtl) => TextAlign::Right,
        (StyloTextAlign::End, TextDirection::Ltr | TextDirection::Auto) => TextAlign::Right,
        _ => TextAlign::Left,
    };
    let variations = extract_font_variations(style);
    let color = absolute_color_to_krui(&ita.clone_color());
    let (text_box_trim, text_box_edge) = crate::servo_ext::extract_text_box(style);

    let text = TextStyle {
        font_size,
        font_weight,
        line_height,
        font_family,
        text_align,
        text_align_explicit: style.clone_text_align() != StyloTextAlign::Start,
        direction,
        text_box_trim,
        text_box_edge,
        variations,
        italic,
        letter_spacing_px,
        white_space,
        word_break,
        overflow_wrap,
        text_overflow_ellipsis,
        line_clamp,
    };

    let inherited = build_inherited_text_props(&text, color);
    (text, inherited)
}

fn build_inherited_text_props(text: &TextStyle, color: krui_style::Color) -> InheritedTextProps {
    InheritedTextProps {
        color: Some(color),
        font_size: Some(text.font_size),
        font_weight: Some(text.font_weight),
        line_height: Some(text.line_height),
        font_family: Some(text.font_family),
        text_align: Some(text.text_align),
        direction: Some(text.direction),
        text_box_edge: Some(text.text_box_edge),
        variations: Some(text.variations),
        italic: Some(text.italic),
        letter_spacing_px: Some(text.letter_spacing_px),
        white_space: Some(text.white_space),
        word_break: Some(text.word_break),
        overflow_wrap: Some(text.overflow_wrap),
        ..InheritedTextProps::EMPTY
    }
}
