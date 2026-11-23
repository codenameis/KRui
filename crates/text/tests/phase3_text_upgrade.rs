//! Phase 3 Clean-Slate Upgrade tests for `krui-text`:
//! - `SwashContent::Color` (`RGBA8`) + `SwashContent::SubpixelMask` decoding
//! - `TextEngine::new_isolated()`, dynamic `FontFamilyStack::Custom`, 9 weights (`100..=900`), HiDPI invariance
//! - `shape_rich_text` multi-span wrapping and per-span styling
//! - `TextEditBuffer` grapheme navigation, `preferred_x` vertical movement, Undo/Redo, and multiline IME geometry

#![forbid(unsafe_code)]

use cosmic_text::{Placement, SwashContent, SwashImage};
use krui_style::{Color, FontFamilyStack, TextAlign, TextDirection};
use krui_text::{
    compute_ime_visuals, decode_swash_image, RichTextSpan, TextEditBuffer, TextEngine,
};

#[test]
fn swash_color_rgba_and_subpixel_mask_decode_accurately() {
    let mut color_img = SwashImage::new();
    color_img.content = SwashContent::Color;
    color_img.placement = Placement {
        left: 0,
        top: 12,
        width: 2,
        height: 1,
    };
    color_img.data = vec![240, 80, 30, 255, 10, 200, 120, 128];

    let decoded_color = decode_swash_image(&color_img);
    assert!(decoded_color.is_color);
    assert_eq!(decoded_color.alpha_mask.as_ref(), &[255, 128]);
    assert_eq!(
        decoded_color.color_rgba.as_deref(),
        Some(&[240_u8, 80, 30, 255, 10, 200, 120, 128][..])
    );

    let mut subpx_img = SwashImage::new();
    subpx_img.content = SwashContent::SubpixelMask;
    subpx_img.placement = Placement {
        left: 0,
        top: 10,
        width: 2,
        height: 1,
    };
    subpx_img.data = vec![30, 60, 90, 255, 255, 255, 0, 255];
    let decoded_subpx = decode_swash_image(&subpx_img);
    assert!(!decoded_subpx.is_color);
    assert!(decoded_subpx.color_rgba.is_none());
    assert_eq!(decoded_subpx.alpha_mask.as_ref(), &[60, 170]);

    let mut engine = TextEngine::new();
    let emoji_layout = engine.shape_and_rasterize("🎨🚀", 20.0, 1.3, 400, Color::WHITE, None);
    assert!(!emoji_layout.glyphs.is_empty());
    for g in emoji_layout.glyphs.iter().filter(|g| g.is_color) {
        let rgba = g.color_rgba.as_ref().expect("color_rgba populated");
        assert_eq!(rgba.len(), (g.width * g.height * 4) as usize);
        assert_eq!(g.alpha_mask.len(), (g.width * g.height) as usize);
    }
}

#[test]
fn isolated_engine_dynamically_resolves_and_caches_custom_font_stack() {
    let mut engine = TextEngine::new_isolated();
    assert!(engine.is_isolated());
    let initial_stacks = engine.resolved_font_stack_count();

    let runtime_stack = FontFamilyStack::intern_custom(&[
        "RuntimePreviewCustomSans",
        "Malgun Gothic",
        "Segoe UI",
        "sans-serif",
    ]);
    let shaped_custom = engine.shape_text(
        "동적 커스텀 폰트 스택 Dynamic Stack",
        16.0,
        1.4,
        400,
        runtime_stack,
        None,
        TextAlign::Left,
        TextDirection::Auto,
        1.0,
    );
    assert!(!shaped_custom.glyphs.is_empty());
    let after_first = engine.resolved_font_stack_count();
    assert_eq!(after_first, initial_stacks + 1);

    let _ = engine.shape_text(
        "두 번째 호출 캐시 재사용",
        16.0,
        1.4,
        400,
        runtime_stack,
        None,
        TextAlign::Left,
        TextDirection::Auto,
        1.0,
    );
    assert_eq!(engine.resolved_font_stack_count(), after_first);
}

#[test]
fn nine_font_weights_produce_distinct_cache_keys() {
    let mut weight_engine = TextEngine::new();
    for weight in [100_u16, 200, 300, 400, 500, 600, 700, 800, 900] {
        let layout = weight_engine.shape_text(
            "KRui 9단계 웨이트 검증",
            18.0,
            1.3,
            weight,
            FontFamilyStack::FluentUI,
            None,
            TextAlign::Left,
            TextDirection::Auto,
            1.0,
        );
        assert!(layout.measured_width > 0.0);
    }
    assert_eq!(
        weight_engine.cached_layout_count(),
        9,
        "All 9 CSS font weights (100..=900) must occupy distinct cache keys"
    );
}

#[test]
fn hidpi_scale_factors_keep_logical_dimensions_invariant_while_scaling_bitmaps() {
    let mut engine = TextEngine::new();
    let sample = "HiDPI 논리 크기 불변 검증 120fps";
    let (mw, mh) = engine.measure_text(sample, 16.0, 1.4, 500, FontFamilyStack::FluentUI, None);
    let shape_at = |eng: &mut TextEngine, scale: f32| {
        eng.shape_text(
            sample,
            16.0,
            1.4,
            500,
            FontFamilyStack::FluentUI,
            None,
            TextAlign::Left,
            TextDirection::Auto,
            scale,
        )
    };
    let s1 = shape_at(&mut engine, 1.0);
    let s2 = shape_at(&mut engine, 2.0);
    let s3 = shape_at(&mut engine, 3.0);
    assert_eq!(engine.cached_layout_count(), 3);
    assert!((s1.measured_width - s2.measured_width).abs() < 0.01);
    assert!((s1.measured_width - s3.measured_width).abs() < 0.01);
    assert!((s1.measured_height - s2.measured_height).abs() < 0.01);
    assert!((s1.measured_width.ceil() - mw).abs() <= 1.0);
    assert!((s1.measured_height.ceil() - mh).abs() <= 1.0);
    assert!(s2.glyphs[0].width > s1.glyphs[0].width);
    assert!(s3.glyphs[0].width > s2.glyphs[0].width);
}

fn build_sample_rich_spans(code_color: Color) -> [RichTextSpan; 3] {
    [
        RichTextSpan {
            text: "안녕하세요 ".to_string(),
            font_weight: 400,
            italic: false,
            font_family: FontFamilyStack::FluentUI,
            color: None,
        },
        RichTextSpan {
            text: "보현님 ".to_string(),
            font_weight: 700,
            italic: true,
            font_family: FontFamilyStack::FluentUI,
            color: None,
        },
        RichTextSpan {
            text: "krui-v2-clean-slate".to_string(),
            font_weight: 500,
            italic: false,
            font_family: FontFamilyStack::CosmicMono,
            color: Some(code_color),
        },
    ]
}

#[test]
fn shape_rich_text_wraps_multi_span_paragraph_and_preserves_span_metadata() {
    let mut engine = TextEngine::new();
    let code_color = Color::rgba(56, 189, 248, 255);
    let spans = build_sample_rich_spans(code_color);

    let unwrapped = engine.shape_rich_text(
        &spans,
        16.0,
        1.4,
        None,
        TextAlign::Left,
        TextDirection::Auto,
        1.0,
    );
    let wrapped = engine.shape_rich_text(
        &spans,
        16.0,
        1.4,
        Some(95.0),
        TextAlign::Left,
        TextDirection::Auto,
        1.0,
    );

    assert!(wrapped.lines.len() >= 2, "Expected rich paragraph to wrap");
    assert!(wrapped.measured_height > unwrapped.measured_height);
    assert!(wrapped
        .glyphs
        .iter()
        .any(|g| g.span_index == 0 && g.glyph_color.is_none()));
    assert!(wrapped
        .glyphs
        .iter()
        .any(|g| g.span_index == 1 && g.glyph_color.is_none()));
    assert!(wrapped
        .glyphs
        .iter()
        .any(|g| g.span_index == 2 && g.glyph_color == Some(code_color)));
}

#[test]
fn text_edit_buffer_grapheme_zwj_navigation_backspace_undo_redo_and_clipboard() {
    let sample = "한글🇰🇷👨‍👩‍👧‍👦끝";
    let mut buf = TextEditBuffer::with_text(sample, false);
    assert_eq!(buf.grapheme_len(), 5);
    let len_all = buf.char_len();
    assert_eq!(buf.cursor(), len_all);

    assert!(buf.move_left(false));
    assert_eq!(buf.cursor(), len_all - 1);
    assert!(buf.move_left(false));
    assert_eq!(buf.cursor(), len_all - 1 - 7);
    assert!(buf.move_left(false));
    assert_eq!(buf.cursor(), 2);

    assert!(buf.delete_forward());
    assert_eq!(buf.text(), "한글👨‍👩‍👧‍👦끝");
    assert!(buf.move_right(false));
    assert!(buf.backspace());
    assert_eq!(buf.text(), "한글끝");

    assert!(buf.undo() && buf.text() == "한글👨‍👩‍👧‍👦끝");
    assert!(buf.undo() && buf.text() == sample);
    assert!(buf.redo() && buf.text() == "한글👨‍👩‍👧‍👦끝");

    buf.set_cursor(0, false);
    buf.move_right(true);
    buf.move_right(true);
    assert_eq!(buf.copy_selection().as_deref(), Some("한글"));
    assert_eq!(buf.cut_selection().as_deref(), Some("한글"));
    assert_eq!(buf.text(), "👨‍👩‍👧‍👦끝");
    buf.move_end(false);
    assert!(buf.paste(" 최고"));
    assert_eq!(buf.text(), "👨‍👩‍👧‍👦끝 최고");
}

#[test]
fn text_edit_buffer_vertical_preferred_x_column_preservation() {
    let mut engine = TextEngine::new();
    let text = "첫번째줄은매우길게작성합니다\n짧음\n세번째줄도다시매우길게작성합니다";
    let layout = engine.shape_text(
        text,
        16.0,
        1.4,
        400,
        FontFamilyStack::FluentUI,
        Some(400.0),
        TextAlign::Left,
        TextDirection::Auto,
        1.0,
    );
    assert_eq!(layout.lines.len(), 3);

    let mut buf = TextEditBuffer::with_text(text, true);
    buf.set_cursor(10, false);
    let initial_x = layout.cursor_x_for_char(buf.cursor());
    assert!(initial_x > 60.0);

    assert!(buf.move_vertical(1, &layout, false));
    assert_eq!(layout.line_for_char(buf.cursor()).line_index, 1);
    assert_eq!(buf.preferred_x(), Some(initial_x));

    assert!(buf.move_vertical(1, &layout, false));
    assert_eq!(layout.line_for_char(buf.cursor()).line_index, 2);
    let recovered_x = layout.cursor_x_for_char(buf.cursor());
    assert!(
        (recovered_x - initial_x).abs() < 20.0,
        "Expected preferred_x recovery near {initial_x}, got {recovered_x}"
    );
}

#[test]
fn multiline_ime_visuals_splits_underlines_onto_line2() {
    let mut engine = TextEngine::new();
    let ime_doc = "첫째줄 텍스트\n한국어조합중";
    let ime_layout = engine.shape_text(
        ime_doc,
        16.0,
        1.4,
        400,
        FontFamilyStack::FluentUI,
        Some(300.0),
        TextAlign::Left,
        TextDirection::Auto,
        1.0,
    );
    let pre_start = "첫째줄 텍스트\n".chars().count();
    let pre_end = ime_doc.chars().count();
    let visuals = compute_ime_visuals(
        &ime_layout,
        pre_start..pre_end,
        Some((pre_start + 3)..pre_end),
    );
    assert!(!visuals.underline_segments.is_empty());
    for seg in &visuals.underline_segments {
        assert_eq!(seg.line_index, 1, "IME underline must be on line index 1");
        assert!(seg.y > 0.0, "Line 2 IME underline must have y > 0.0");
    }
    assert!(visuals
        .underline_segments
        .iter()
        .any(|s| s.is_target_clause));
    assert!(visuals.candidate_cursor_rect.1 > 0.0);
}
