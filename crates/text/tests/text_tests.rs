//! Unit tests for `krui-text`.

#![forbid(unsafe_code)]

use krui_style::{Color, FontFamilyStack};
use krui_text::{
    build_hollow_notdef_mask, notdef_warning_count, record_notdef_glyph_warning, TextEngine,
    MAX_TEXT_CACHE_ENTRIES,
};
use std::sync::Arc;

#[test]
fn shapes_korean_cjk_rtl_and_emoji_graphemes_correctly() {
    let mut engine = TextEngine::new();
    let sample = "안녕하세요 KRui 韓글 مرحبا 👨‍👩‍👧‍👦";
    let custom_stack = FontFamilyStack::intern_custom(&["Pretendard", "Segoe UI", "sans-serif"]);
    for stack in [
        FontFamilyStack::FluentUI,
        FontFamilyStack::MaterialSans,
        FontFamilyStack::LiquidGlassSans,
        FontFamilyStack::Serif,
        FontFamilyStack::CosmicMono,
        custom_stack,
    ] {
        let shaped = engine.shape_and_rasterize_with_family(
            sample,
            18.0,
            1.4,
            600,
            stack,
            Color::WHITE,
            Some(600.0),
        );
        assert!(shaped.has_cjk_or_hangul && shaped.has_rtl);
        assert!(
            shaped.measured_width > 0.0
                && shaped.measured_height > 0.0
                && !shaped.glyphs.is_empty()
        );
        assert_eq!(shaped.char_x_offsets.len(), sample.chars().count() + 1);
    }
}

#[test]
fn color_change_reuses_shaped_cache_entry_and_alpha_masks() {
    let mut engine = TextEngine::new();
    let white = engine.shape_and_rasterize("KRui 캐시", 16.0, 1.3, 500, Color::WHITE, None);
    let red =
        engine.shape_and_rasterize("KRui 캐시", 16.0, 1.3, 500, Color::rgb(239, 68, 68), None);
    assert_eq!(engine.cached_layout_count(), 1);
    assert!(Arc::ptr_eq(&white.glyphs, &red.glyphs));
    assert!(Arc::ptr_eq(
        &white.glyphs[0].alpha_mask,
        &red.glyphs[0].alpha_mask
    ));
}

#[test]
fn lru_eviction_preserves_frequently_used_entry_across_513_unique_inserts() {
    let mut engine = TextEngine::new();
    let hot = engine.shape_and_rasterize("자주쓰는헤더", 16.0, 1.3, 500, Color::WHITE, None);
    for i in 0..MAX_TEXT_CACHE_ENTRIES {
        let _ = engine.shape_and_rasterize(
            &format!("임시항목-{i}"),
            16.0,
            1.3,
            500,
            Color::WHITE,
            None,
        );
        if i == MAX_TEXT_CACHE_ENTRIES / 2 {
            // Touch `"자주쓰는헤더"` so its LRU timestamp is refreshed!
            let touched =
                engine.shape_and_rasterize("자주쓰는헤더", 16.0, 1.3, 500, Color::WHITE, None);
            assert!(Arc::ptr_eq(&hot.glyphs, &touched.glyphs));
        }
    }
    assert_eq!(engine.cached_layout_count(), MAX_TEXT_CACHE_ENTRIES);
    // Because `"자주쓰는헤더"` was touched mid-way, `"임시항목-0"` was evicted instead of `"자주쓰는헤더"`!
    let after = engine.shape_and_rasterize("자주쓰는헤더", 16.0, 1.3, 500, Color::WHITE, None);
    assert!(
        Arc::ptr_eq(&hot.glyphs, &after.glyphs),
        "True LRU must keep recently touched entry in cache when capacity (512) is exceeded"
    );
}

#[test]
fn notdef_hollow_box_has_transparent_interior_and_visible_border_and_deduplicates_warning() {
    let mask = build_hollow_notdef_mask(8, 10);
    assert_eq!(mask.len(), 80);
    // Top-left border pixel is non-zero
    assert!(mask[0] > 0);
    // Interior pixel `(4, 5)` is 0 (hollow rectangle, not a solid gray block)
    assert_eq!(mask[5 * 8 + 4], 0);

    let before = notdef_warning_count();
    assert!(record_notdef_glyph_warning(
        FontFamilyStack::CosmicMono,
        '\u{F8FF}'
    ));
    assert!(!record_notdef_glyph_warning(
        FontFamilyStack::CosmicMono,
        '\u{F8FF}'
    ));
    assert_eq!(notdef_warning_count(), before + 1);
}

#[test]
fn strips_malicious_bidi_override_before_shaping() {
    let mut engine = TextEngine::new();
    let shaped =
        engine.shape_and_rasterize("safe\u{202E}txt.exe", 16.0, 1.2, 400, Color::WHITE, None);
    assert_eq!(shaped.sanitized_text, "safetxt.exe");
}

#[test]
fn weight_500_mixed_korean_and_latin_never_falls_back_to_symbol_font() {
    let mut engine = TextEngine::new();
    let labels = [
        "[F2] 02 Material 스토어",
        "[F3] 03 LiquidGlass 데스크",
        "[F4] 04 Cosmic HUD",
        "[F6/2] Material 3",
    ];
    let custom_stack = FontFamilyStack::intern_custom(&["Pretendard", "Segoe UI", "sans-serif"]);
    for stack in [
        FontFamilyStack::FluentUI,
        FontFamilyStack::MaterialSans,
        FontFamilyStack::LiquidGlassSans,
        FontFamilyStack::Serif,
        FontFamilyStack::CosmicMono,
        custom_stack,
    ] {
        for label in labels {
            let w400 = engine.shape_and_rasterize_with_family(
                label,
                12.0,
                1.3,
                400,
                stack,
                Color::WHITE,
                None,
            );
            let w500 = engine.shape_and_rasterize_with_family(
                label,
                12.0,
                1.3,
                500,
                stack,
                Color::WHITE,
                None,
            );
            assert!(
                !w500.glyphs.is_empty(),
                "Expected non-empty glyphs for {label}"
            );
            assert!(
                w500.glyphs.iter().all(|g| !g.is_notdef),
                "Expected zero .notdef glyphs for {label} across {stack:?}"
            );
            // Symbol/dingbat faces (Marlett, Wingdings) are purged from FontSystem so weight 500
            // resolves to real text fonts with sensible positive width alongside weight 400.
            assert!(
                w400.measured_width > 10.0 && w500.measured_width > 10.0,
                "Expected valid positive width for {label} ({stack:?})"
            );
        }
    }
}
