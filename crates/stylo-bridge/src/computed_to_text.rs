//! Internal conversion from Stylo [`ComputedValues`] to KRui [`TextStyle`] and
//! [`InheritedTextProps`].

use krui_style::{
    FontFamilyStack, FontVariation, FontVariationList, InheritedTextProps, LineHeight, TextAlign,
    TextDirection, TextStyle, MAX_FONT_VARIATIONS,
};
use style::properties::longhands::direction::computed_value::T as StyloDirection;
use style::properties::ComputedValues;
use style::values::computed::font::{GenericFontFamily, SingleFontFamily};
use style::values::computed::TextAlign as StyloTextAlign;
use style::values::generics::font::LineHeight as StyloLineHeight;

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

/// Extracts resolved typography settings from a Stylo [`ComputedValues`] snapshot.
#[must_use]
pub(crate) fn extract_text_style(style: &ComputedValues) -> (TextStyle, InheritedTextProps) {
    let font = style.get_font();
    let font_size = font.font_size.computed_size().px();
    let font_weight = font
        .font_weight
        .value()
        .round()
        .clamp(MIN_FONT_WEIGHT, MAX_FONT_WEIGHT) as u16;
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
    let color = absolute_color_to_krui(&style.get_inherited_text().clone_color());
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
    };

    let inherited = InheritedTextProps {
        color: Some(color),
        font_size: Some(font_size),
        font_weight: Some(font_weight),
        line_height: Some(line_height),
        font_family: Some(font_family),
        text_align: Some(text_align),
        direction: Some(direction),
        text_box_edge: Some(text_box_edge),
        variations: Some(variations),
        ..InheritedTextProps::EMPTY
    };

    (text, inherited)
}
