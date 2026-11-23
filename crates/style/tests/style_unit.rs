//! Unit tests for pure value types in `krui-style` (`FontFamilyStack`, `GridTrackList`,
//! `BoxShadow`, `InheritedTextProps`, `TransitionCurve`, `HtmlTag`, and BiDi sanitization).

#![forbid(unsafe_code)]

use krui_style::{
    is_bidi_control_char, sanitize_untrusted_text, BoxShadow, Color, FontFamilyStack, GridTrack,
    GridTrackList, HtmlTag, InheritedTextProps, LayoutStyle, PaintStyle, TextAlign, TextDirection,
    TextStyle, TransitionCurve,
};
use std::collections::HashSet;

#[test]
fn font_family_presets_and_custom_interning_work() {
    assert_eq!(
        FontFamilyStack::parse_css("Georgia, serif"),
        FontFamilyStack::Serif
    );
    assert_eq!(
        FontFamilyStack::parse_css("\"Sitka Text\", \"Batang\", serif"),
        FontFamilyStack::Serif
    );
    assert_eq!(FontFamilyStack::parse_css("serif"), FontFamilyStack::Serif);
    assert_eq!(
        FontFamilyStack::parse_css("sans-serif"),
        FontFamilyStack::FluentUI
    );
    assert_eq!(
        FontFamilyStack::parse_css("monospace"),
        FontFamilyStack::CosmicMono
    );

    let custom_a =
        FontFamilyStack::parse_css("\"Pretendard\", 'Inter', \"Apple SD Gothic Neo\", sans-serif");
    let custom_b = FontFamilyStack::intern_custom(&[
        "Pretendard",
        "Inter",
        "Apple SD Gothic Neo",
        "sans-serif",
    ]);
    assert_eq!(custom_a, custom_b);
    assert_eq!(
        custom_a.custom_families(),
        Some(&["Pretendard", "Inter", "Apple SD Gothic Neo", "sans-serif"][..])
    );

    let mut set = HashSet::new();
    assert!(set.insert(custom_a));
    assert!(!set.insert(custom_b));
}

#[test]
fn grid_track_list_box_shadow_and_inherited_props_work() {
    let tracks = GridTrackList::from_slice(&[
        GridTrack::Px(240.0),
        GridTrack::MinMaxPxFr {
            min_px: 180.0,
            max_fr: 1.0,
        },
        GridTrack::Fr(2.0),
        GridTrack::Percent(25.0),
        GridTrack::Auto,
    ]);
    assert_eq!(tracks.len(), 5);
    assert!(!tracks.is_empty());

    let layout = LayoutStyle {
        grid_template_columns: tracks,
        grid_template_rows: GridTrackList::repeat_fr(3, 1.0),
        ..LayoutStyle::default()
    };
    assert_eq!(layout.grid_template_columns.as_slice().len(), 5);
    assert_eq!(
        layout.grid_template_rows.as_slice(),
        &[GridTrack::Fr(1.0); 3]
    );

    let shadow = BoxShadow {
        offset_x: 0.0,
        offset_y: 4.0,
        blur_radius: 16.0,
        spread_radius: 2.5,
        color: Color::rgba(0, 0, 0, 120),
        inset: true,
    };
    assert!(shadow.inset);
    assert!((shadow.spread_radius - 2.5).abs() < 1e-6);

    let parent = InheritedTextProps {
        color: Some(Color::rgb(10, 20, 30)),
        font_size: Some(18.0),
        font_family: Some(FontFamilyStack::LiquidGlassSans),
        direction: Some(TextDirection::Rtl),
        ..InheritedTextProps::EMPTY
    };
    let child = InheritedTextProps {
        font_size: Some(14.0),
        text_align: Some(TextAlign::Center),
        ..InheritedTextProps::EMPTY
    }
    .inherit_from(&parent);

    let mut text = TextStyle::default();
    let mut paint = PaintStyle::default();
    child.apply_to_style(&mut text, &mut paint);
    assert_eq!(paint.text_color, Color::rgb(10, 20, 30));
    assert!((text.font_size - 14.0).abs() < 1e-6);
    assert_eq!(text.font_family, FontFamilyStack::LiquidGlassSans);
    assert_eq!(text.text_align, TextAlign::Center);
    assert_eq!(text.direction, TextDirection::Rtl);
}

#[test]
fn sanitization_easing_and_html_tag_work() {
    let clean = sanitize_untrusted_text("invoice\u{202E}fdp.exe\0");
    assert_eq!(clean, "invoicefdp.exe");
    assert!(!clean.chars().any(is_bidi_control_char));

    assert_eq!(TransitionCurve::default(), TransitionCurve::Ease);
    let steps = TransitionCurve::parse_css("steps(4, jump-none)").expect("steps");
    assert!((steps.sample(0.1) - 0.0).abs() < 1e-4);
    assert!((steps.sample(0.6) - (2.0 / 3.0)).abs() < 1e-4);
    let lin = TransitionCurve::parse_css("linear(0, 0.25 75%, 1)").expect("linear()");
    assert!((lin.sample(0.375) - 0.125).abs() < 1e-4);

    assert_eq!(HtmlTag::parse_element_tag("div"), Ok((HtmlTag::Div, false)));
    assert_eq!(
        HtmlTag::parse_element_tag("md-card"),
        Ok((HtmlTag::Custom, true))
    );
    assert!(HtmlTag::parse_element_tag("script").is_err());
}
