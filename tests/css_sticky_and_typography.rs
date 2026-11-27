//! Regression and conformance tests for Wave 2 CSS typography properties
//! (`white-space`, `word-break`, `overflow-wrap`, `text-overflow: ellipsis`,
//! `line-clamp` / `-webkit-line-clamp`, `font-style: italic`, `letter-spacing`)
//! and 0-relayout `position: sticky` scroll clamping + hit-testing.

#![forbid(unsafe_code)]

use krui::*;

fn extract_all_glyphs(scene: &PaintScene) -> Vec<(f32, f32, u32, u64)> {
    let mut out = Vec::new();
    for prim in &scene.primitives {
        if let DrawPrimitive::TextGlyphs {
            origin_x,
            origin_y,
            glyphs,
            ..
        } = prim
        {
            for g in glyphs.iter() {
                out.push((*origin_x + g.x, *origin_y + g.y, g.width, g.cache_key));
            }
        }
    }
    out
}

#[test]
fn white_space_nowrap_and_pre_preserve_lines_and_spaces() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <div style="display: flex; flex-direction: column; align-items: flex-start;">
            <div id="wrap" style="width: 80px; font-size: 16px;">긴 텍스트 줄바꿈 비교 테스트 문장입니다</div>
            <div id="nowrap" style="width: 80px; font-size: 16px; white-space: nowrap;">긴 텍스트 줄바꿈 비교 테스트 문장입니다</div>
            <div id="normal_ws" style="font-size: 16px;">{"A      B"}</div>
            <div id="pre_ws" style="font-size: 16px; white-space: pre;">{"A      B\nC"}</div>
        </div>
        "#
    ));
    let _ = tree.build_paint_scene(400, 300);
    let wrap_h = tree
        .node_layout(tree.find_by_id("wrap").unwrap())
        .unwrap()
        .height;
    let nowrap_h = tree
        .node_layout(tree.find_by_id("nowrap").unwrap())
        .unwrap()
        .height;
    assert!(
        nowrap_h < wrap_h * 0.6,
        "`white-space: nowrap` height ({nowrap_h}) must stay single-line vs wrapped ({wrap_h})"
    );

    let normal_rect = tree
        .node_layout(tree.find_by_id("normal_ws").unwrap())
        .unwrap();
    let pre_rect = tree
        .node_layout(tree.find_by_id("pre_ws").unwrap())
        .unwrap();
    assert!(
        pre_rect.width > normal_rect.width + 12.0,
        "`white-space: pre` width ({}) must preserve multiple spaces vs collapsed ({})",
        pre_rect.width,
        normal_rect.width
    );
    assert!(
        pre_rect.height > nowrap_h * 1.5,
        "`white-space: pre` height ({}) must preserve explicit newline vs single line ({nowrap_h})",
        pre_rect.height
    );
}

#[test]
fn korean_word_break_keep_all_vs_break_all_and_overflow_wrap() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <div>
            <div id="keep" style="width: 110px; font-size: 16px; word-break: keep-all;">가나다 마바사아자 카타파</div>
            <div id="break_all" style="width: 110px; font-size: 16px; word-break: break-all;">가나다 마바사아자 카타파</div>
            <div id="ascii_normal" style="width: 90px; font-size: 16px; word-break: normal; overflow-wrap: normal;">Supercalifragilisticexpialidocious</div>
            <div id="ascii_ow" style="width: 90px; font-size: 16px; overflow-wrap: break-word;">Supercalifragilisticexpialidocious</div>
        </div>
        "#
    ));
    let _ = tree.build_paint_scene(300, 400);
    let keep_h = tree
        .node_layout(tree.find_by_id("keep").unwrap())
        .unwrap()
        .height;
    let break_h = tree
        .node_layout(tree.find_by_id("break_all").unwrap())
        .unwrap()
        .height;
    assert!(
        keep_h > break_h * 1.25,
        "`word-break: keep-all` ({keep_h}) must keep Hangul eojeols intact onto 3 lines vs `break-all` ({break_h})"
    );

    let normal_h = tree
        .node_layout(tree.find_by_id("ascii_normal").unwrap())
        .unwrap()
        .height;
    let ow_h = tree
        .node_layout(tree.find_by_id("ascii_ow").unwrap())
        .unwrap()
        .height;
    assert!(
        ow_h > normal_h * 2.2,
        "`overflow-wrap: break-word` ({ow_h}) must wrap overlong unbroken token vs `normal` ({normal_h})"
    );
}

#[test]
fn text_overflow_ellipsis_and_webkit_line_clamp() {
    reset_reactive_runtime();
    let mut tree_ellip = ViewTree::new();
    tree_ellip.mount(krui!(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <div id="ellip" style="width: 90px; font-size: 16px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">매우 긴 한 줄 말줄임표 테스트 문장입니다</div>
        "#
    ));
    let (scene_ellip, _) = tree_ellip.build_paint_scene(300, 100);
    let glyphs = extract_all_glyphs(&scene_ellip);
    assert!(!glyphs.is_empty(), "Ellipsized line must still emit glyphs");
    let max_right = glyphs
        .iter()
        .map(|(x, _, w, _)| *x + *w as f32)
        .fold(0.0_f32, f32::max);
    assert!(
        max_right <= 94.0,
        "`text-overflow: ellipsis` glyphs must stop within the 90px box (max_right={max_right})"
    );
    assert!(
        glyphs.len() < 14,
        "`text-overflow: ellipsis` must truncate trailing glyphs and append ellipsis (got {} glyphs)",
        glyphs.len()
    );

    let mut tree_clamp = ViewTree::new();
    tree_clamp.mount(krui!(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <div>
            <p id="clamped_wk" style="width: 100px; font-size: 16px; overflow: hidden; -webkit-line-clamp: 2;">첫째줄 문장입니다 둘째줄 문장입니다 셋째줄 문장입니다 넷째줄 문장입니다</p>
            <p id="clamped_std" style="width: 100px; font-size: 16px; overflow: hidden; line-clamp: 2;">첫째줄 문장입니다 둘째줄 문장입니다 셋째줄 문장입니다 넷째줄 문장입니다</p>
            <p id="unclamped" style="width: 100px; font-size: 16px;">첫째줄 문장입니다 둘째줄 문장입니다 셋째줄 문장입니다 넷째줄 문장입니다</p>
        </div>
        "#
    ));
    let _ = tree_clamp.build_paint_scene(300, 400);
    let wk_h = tree_clamp
        .node_layout(tree_clamp.find_by_id("clamped_wk").unwrap())
        .unwrap()
        .height;
    let std_h = tree_clamp
        .node_layout(tree_clamp.find_by_id("clamped_std").unwrap())
        .unwrap()
        .height;
    let full_h = tree_clamp
        .node_layout(tree_clamp.find_by_id("unclamped").unwrap())
        .unwrap()
        .height;
    assert!(
        wk_h < full_h * 0.65 && (wk_h - std_h).abs() < 1.0,
        "`-webkit-line-clamp: 2` ({wk_h}) and `line-clamp: 2` ({std_h}) must clamp height vs unclamped ({full_h})"
    );
}

#[test]
fn font_style_italic_and_letter_spacing_affect_shaping() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <div style="display: flex; flex-direction: column; align-items: flex-start;">
            <span id="ls0" style="font-size: 16px; letter-spacing: 0px;">ABCDEFGH</span>
            <span id="ls6" style="font-size: 16px; letter-spacing: 6px;">ABCDEFGH</span>
        </div>
        "#
    ));
    let _ = tree.build_paint_scene(400, 200);
    let w0 = tree
        .node_layout(tree.find_by_id("ls0").unwrap())
        .unwrap()
        .width;
    let w6 = tree
        .node_layout(tree.find_by_id("ls6").unwrap())
        .unwrap()
        .width;
    assert!(
        w6 >= w0 + 36.0,
        "`letter-spacing: 6px` width ({w6}) must exceed 0px width ({w0}) by >= 36px"
    );

    let mut tree_norm = ViewTree::new();
    tree_norm.mount(krui!(
        r#"<link rel="krui-preset" href="krui://preset/unstyled" /><span style="font-family: Arial, sans-serif; font-size: 20px; font-style: normal;">Hello</span>"#
    ));
    let (scene_norm, _) = tree_norm.build_paint_scene(200, 80);

    let mut tree_ital = ViewTree::new();
    tree_ital.mount(krui!(
        r#"<link rel="krui-preset" href="krui://preset/unstyled" /><span style="font-family: Arial, sans-serif; font-size: 20px; font-style: italic;">Hello</span>"#
    ));
    let (scene_ital, _) = tree_ital.build_paint_scene(200, 80);
    assert_ne!(
        extract_all_glyphs(&scene_norm),
        extract_all_glyphs(&scene_ital),
        "`font-style: italic` must rasterize distinct italic glyphs vs `font-style: normal`"
    );
}

#[test]
fn position_sticky_clamps_during_scroll_with_zero_relayouts_and_accurate_hit_test() {
    reset_reactive_runtime();
    let clicks = Signal::new(0_i32);
    let on_hdr_click = move || clicks.set(clicks.get() + 1);
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <div id="scroller" style="width: 200px; height: 120px; overflow-y: scroll;">
            <div style="height: 24px;">상단 배너</div>
            <div id="section" style="height: 220px;">
                <button id="sticky_hdr" onclick="{on_hdr_click}"
                        style="position: sticky; top: 10px; width: 180px; height: 30px; background-color: #2563eb; border: none;">
                    고정 헤더
                </button>
                <div style="height: 180px;">섹션 본문</div>
            </div>
            <div style="height: 240px;">다음 섹션</div>
        </div>
        "#
    ));
    let (scene0, _) = tree.build_paint_scene(220, 150);
    let hdr_box_y = |scene: &PaintScene| -> Option<f32> {
        scene.primitives.iter().find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if *background_color == Color::parse_css("#2563eb").unwrap() => Some(rect.y),
            _ => None,
        })
    };
    assert!(
        (hdr_box_y(&scene0).unwrap() - 24.0).abs() < 1.5,
        "Before scrolling past threshold, sticky `top: 10px` must stay at static flow y=24 (got {:?})",
        hdr_box_y(&scene0)
    );

    let relayouts_before = tree.relayout_passes();
    assert!(tree.dispatch_pointer_move(50.0, 60.0));
    assert!(tree.dispatch_wheel(50.0, 60.0, 0.0, -60.0, Mods::default()));
    let (scene1, _) = tree.build_paint_scene(220, 150);
    assert_eq!(
        tree.relayout_passes(),
        relayouts_before,
        "Scrolling a `position: sticky` element must trigger 0 Taffy relayout passes"
    );
    assert!(
        (hdr_box_y(&scene1).unwrap() - 10.0).abs() < 1.5,
        "At scroll_y=60, sticky header must clamp to `top: 10px` (got {:?})",
        hdr_box_y(&scene1)
    );
    let _ = tree.dispatch_pointer_move(40.0, 22.0);
    let _ = tree.dispatch_pointer_down(40.0, 22.0);
    let _ = tree.dispatch_pointer_up();
    assert_eq!(
        clicks.get(),
        1,
        "Clicking pinned sticky header at y=22 must hit it"
    );

    assert!(tree.dispatch_wheel(50.0, 60.0, 0.0, -210.0, Mods::default()));
    let (scene2, _) = tree.build_paint_scene(220, 150);
    if let Some(y_pushed) = hdr_box_y(&scene2) {
        assert!(
            y_pushed < -10.0,
            "Once parent section scrolls past top, sticky header must scroll away with section bottom (got {y_pushed})"
        );
    }
    let _ = tree.dispatch_pointer_move(40.0, 22.0);
    let _ = tree.dispatch_pointer_down(40.0, 22.0);
    let _ = tree.dispatch_pointer_up();
    assert_eq!(
        clicks.get(),
        1,
        "Scrolled-away sticky header must no longer intercept clicks at y=22"
    );
}
