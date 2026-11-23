#![forbid(unsafe_code)]

use krui::*;
use krui_winit::ime_adapter::{logical_to_physical_ime_area, sync_ime_host_with_sink_and_offset};
use krui_winit::{RecordedImeOp, RecordedImeSink, WinitImeStateCache};

fn mount_ime_test_tree(val: Signal<String>) -> ViewTree {
    let mut tree = ViewTree::new();
    tree.set_scale_factor(1.5);
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
            .scroller {
                display: flex;
                flex-direction: column;
                width: 260px;
                height: 120px;
                overflow-x: scroll;
                overflow-y: scroll;
                transform: translate(30px, 20px);
            }
            .wide-pad {
                flex-shrink: 0;
                width: 420px;
                height: 240px;
                padding: 40px;
            }
            #styled-inp {
                width: 220px;
                height: 40px;
                color: #1e293b;
                caret-color: #ff0055;
            }
            #styled-inp::selection {
                background-color: #22c55e;
                color: #052e16;
            }
            #tight-inp {
                width: 180px;
                height: 18px;
                padding: 0px 4px;
                border: 1px solid #334155;
                font-size: 14px;
                text-box: trim-both cap alphabetic;
                caret-color: currentColor;
                color: #0ea5e9;
            }
            </style>
            <div direction="column" style="gap: 12px; padding: 12px;">
                <div id="sc" class="scroller">
                    <div class="wide-pad">
                        <input id="styled-inp" bind:value="{val}" />
                    </div>
                </div>
                <input id="tight-inp" />
            </div>
        "#
        )
    });
    tree
}

#[test]
fn a4_unified_ime_caret_selection_underline_viewport_and_zero_relayout() {
    let val = Signal::new(String::new());
    let mut tree = mount_ime_test_tree(val);
    assert!(tree.build_paint_scene(640, 400).1);

    let inp = tree.find_by_id("styled-inp").unwrap();
    let sc = tree.find_by_id("sc").unwrap();
    assert!(tree.set_focus_to_node(inp));
    let _ = tree.build_paint_scene(640, 400);

    assert!(tree.ime_preedit("김보현 gpy", Some((0, 9))));
    let (pre_scene, pre_relayout) = tree.build_paint_scene(640, 400);
    assert!(
        !pre_relayout,
        "fixed-dimension single-line <input> IME preedit = 0 relayouts"
    );
    verify_caret_color_and_underline_below_ink(&pre_scene, Color::rgb(0xff, 0x00, 0x55));

    assert!(tree.ime_preedit("김보현 gpy", Some((0, 3))));
    assert!(
        !tree.build_paint_scene(640, 400).1,
        "clause cursor move = 0 relayouts"
    );

    assert!(tree.ime_commit("김보현"));
    let _ = tree.build_paint_scene(640, 400);
    assert!(tree.set_caret_visible(false));
    assert!(
        !tree.build_paint_scene(640, 400).1,
        "caret blink off = 0 relayouts"
    );
    assert!(tree.set_caret_visible(true));
    assert!(
        !tree.build_paint_scene(640, 400).1,
        "caret blink on = 0 relayouts"
    );

    assert!(tree.select_all());
    let (sel_scene, sel_relayout) = tree.build_paint_scene(640, 400);
    assert!(!sel_relayout, "selection change = 0 relayouts");
    verify_custom_selection_box_and_glyph_color(&sel_scene);

    verify_scrolled_transformed_and_hidpi_ime_sink(&mut tree, sc);
    verify_tight_trimmed_input_underline_stays_inside_clip(&mut tree);
}

fn verify_caret_color_and_underline_below_ink(scene: &PaintScene, expected_caret: Color) {
    let has_custom_caret = scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox { background_color, rect, .. }
                if *background_color == expected_caret && rect.width <= 3.0 && rect.height >= 12.0
        )
    });
    assert!(
        has_custom_caret,
        "PaintScene must emit caret with CSS caret-color #ff0055"
    );

    let max_ink_bot = scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::TextGlyphs {
                origin_y, glyphs, ..
            } => glyphs
                .iter()
                .map(|g| *origin_y + g.y + (g.height as f32) / scene.scale_factor)
                .reduce(f32::max),
            _ => None,
        })
        .expect("preedit glyphs");
    let underline_y = scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if *background_color == krui_style::UA_PRIMARY_TEXT_COLOR && rect.height <= 3.0 => {
                Some(rect.y)
            }
            _ => None,
        })
        .expect("preedit underline");
    assert!(
        underline_y >= max_ink_bot - 0.01,
        "Hangul/descender ink bottom ({max_ink_bot}) must not be crossed by IME underline ({underline_y})"
    );
}

fn verify_custom_selection_box_and_glyph_color(scene: &PaintScene) {
    let has_sel_box = scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox {
                background_color,
                border_width,
                ..
            } if *background_color == Color::rgb(0x22, 0xc5, 0x5e) && *border_width == 0.0
        )
    });
    assert!(
        has_sel_box,
        "::selection background-color must apply with zero default border"
    );
    let all_recolored = scene
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::TextGlyphs { glyphs, .. } => Some(
                glyphs
                    .iter()
                    .all(|g| g.glyph_color == Some(Color::rgb(0x05, 0x2e, 0x16))),
            ),
            _ => None,
        })
        .unwrap_or(false);
    assert!(
        all_recolored,
        "::selection color must recolor selected glyphs"
    );
}

fn verify_scrolled_transformed_and_hidpi_ime_sink(tree: &mut ViewTree, sc: NodeId) {
    // Untransformed #styled-inp sits at (12 + 40, 12 + 40) = (52, 52); translate(30px, 20px) moves it to (82, 72).
    let before = tree.focused_ime_cursor_area().expect("ime before scroll");
    assert!(
        before.x > 110.0 && before.y > 78.0,
        "translate(30px, 20px) must shift IME cursor area from (52, 52) into (82+, 72+): before={before:?}"
    );
    assert!(tree.set_scroll_offsets(sc, 25.0, 18.0));
    let _ = tree.build_paint_scene(640, 400);
    let after = tree.focused_ime_cursor_area().expect("ime after scroll");
    assert!(
        (before.x - after.x - 25.0).abs() < 0.5 && (before.y - after.y - 18.0).abs() < 0.5,
        "ancestor overflow-x/y scroll (25, 18) + transform must shift IME area: before={before:?}, after={after:?}"
    );

    let mut sink = RecordedImeSink::default();
    let mut cache = WinitImeStateCache::default();
    sync_ime_host_with_sink_and_offset(&mut sink, tree, &mut cache, (24.0, 83.0));
    let first_area = sink
        .ops
        .iter()
        .find_map(|op| match op {
            RecordedImeOp::SetCursorArea(r) => Some(*r),
            _ => None,
        })
        .expect("SetCursorArea");
    assert!((first_area.x - (after.x + 24.0)).abs() < 1e-3);
    assert!((first_area.y - (after.y + 83.0)).abs() < 1e-3);
    let (px, py, pw, ph) = logical_to_physical_ime_area(first_area, 1.5);
    assert_eq!(px, (first_area.x as f64 * 1.5).round() as i32);
    assert_eq!(py, (first_area.y as f64 * 1.5).round() as i32);
    assert!(pw >= 2 && ph >= 18);

    sink.ops.clear();
    cache.invalidate_cursor_area();
    sync_ime_host_with_sink_and_offset(&mut sink, tree, &mut cache, (24.0, 83.0));
    assert!(
        sink.ops
            .iter()
            .any(|op| matches!(op, RecordedImeOp::SetCursorArea(_))),
        "invalidate_cursor_area on ScaleFactorChanged must re-emit SetCursorArea"
    );
}

fn verify_tight_trimmed_input_underline_stays_inside_clip(tree: &mut ViewTree) {
    let tight = tree.find_by_id("tight-inp").unwrap();
    assert!(tree.set_focus_to_node(tight));
    assert!(tree.ime_preedit("한글g", None));
    let (scene, _) = tree.build_paint_scene(640, 400);

    let has_current_color_caret = scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox { background_color, .. }
                if *background_color == Color::rgb(0x0e, 0xa5, 0xe9)
        )
    });
    assert!(
        has_current_color_caret,
        "caret-color: currentColor must resolve to active text color"
    );

    let (ul_rect, ul_clip) = scene
        .primitives
        .iter()
        .rev()
        .find_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect,
                clip: Some(c),
                background_color,
                ..
            } if *background_color == krui_style::UA_PRIMARY_TEXT_COLOR && rect.height <= 3.0 => {
                Some((*rect, *c))
            }
            _ => None,
        })
        .expect("tight underline");
    assert!(
        ul_rect.y >= ul_clip.y - 0.01
            && ul_rect.y + ul_rect.height <= ul_clip.y + ul_clip.height + 0.01,
        "tight text-box-trim input underline {ul_rect:?} must stay inside clip {ul_clip:?}"
    );
}

fn mount_textarea_and_flex_align_tree(notes: Signal<String>) -> ViewTree {
    let mut tree = ViewTree::new();
    tree.set_scale_factor(1.5);
    tree.mount_with(|| {
        krui!(
            r#"
            <style>
            #auto-h-inp { width: 220px; }
            #multi-ta {
                width: 220px;
                height: 72px;
                padding: 6px;
                font-size: 15px;
                line-height: 1.4;
                caret-color: #eab308;
            }
            #multi-ta::selection {
                background-color: #a855f7;
                color: #faf5ff;
            }
            .flex-left {
                display: flex;
                justify-content: center;
                text-align: left;
                width: 200px;
                height: 36px;
                padding: 0px 12px;
                font-family: system-ui;
            }
            </style>
            <div direction="column" style="gap: 10px; padding: 12px;">
                <input id="auto-h-inp" />
                <textarea id="multi-ta" bind:value="{notes}"></textarea>
                <div id="flex-left-box" class="flex-left">좌측</div>
            </div>
        "#
        )
    });
    tree
}

#[test]
fn a4_multiline_textarea_auto_height_input_and_explicit_flex_text_align_left() {
    let notes = Signal::new("첫번째 줄\n두번째 줄\n세번째 줄\n네번째 줄\n".to_string());
    let mut tree = mount_textarea_and_flex_align_tree(notes);
    let (init_scene, _) = tree.build_paint_scene(480, 320);
    verify_flex_justify_center_respects_explicit_text_align_left(&tree, &init_scene);

    let auto_inp = tree.find_by_id("auto-h-inp").unwrap();
    assert!(tree.set_focus_to_node(auto_inp));
    let _ = tree.build_paint_scene(480, 320);
    assert!(tree.ime_preedit("한글 조합", Some((0, 6))));
    assert!(
        !tree.build_paint_scene(480, 320).1,
        "auto-height single-line <input> with explicit width = 0 relayouts on preedit"
    );
    assert!(tree.ime_commit("한글"));
    let _ = tree.build_paint_scene(480, 320);

    verify_multiline_textarea_ime_and_selection(&mut tree);
}

fn verify_flex_justify_center_respects_explicit_text_align_left(
    tree: &ViewTree,
    scene: &PaintScene,
) {
    assert_eq!(
        FontFamilyStack::parse_css("system-ui").custom_families(),
        Some(["system-ui"].as_slice())
    );
    let fl = tree
        .node_layout(tree.find_by_id("flex-left-box").unwrap())
        .unwrap();
    let ox = scene
        .primitives
        .iter()
        .rev()
        .find_map(|p| match p {
            DrawPrimitive::TextGlyphs { origin_x, .. } => Some(*origin_x),
            _ => None,
        })
        .expect("flex-left text");
    assert!(
        (ox - (fl.x + 12.0)).abs() <= 1.5,
        "explicit text-align: left on flex box must override justify-content: center: ox={ox}, fl={fl:?}"
    );
}

fn verify_multiline_textarea_ime_and_selection(tree: &mut ViewTree) {
    let ta = tree.find_by_id("multi-ta").unwrap();
    let ta_rect = tree.node_layout(ta).unwrap();
    assert!(tree.set_focus_to_node(ta));
    tree.dispatch_cursor_end();
    let _ = tree.build_paint_scene(480, 320);

    assert!(tree.ime_preedit("다섯번째 gpy", Some((0, 12))));
    let (ta_scene, ta_relayout) = tree.build_paint_scene(480, 320);
    assert!(
        !ta_relayout,
        "fixed-dimension <textarea> IME preedit = 0 relayouts"
    );
    let ime_area = tree.focused_ime_cursor_area().expect("textarea ime area");
    assert!(
        ime_area.y >= ta_rect.y && ime_area.y + ime_area.height <= ta_rect.y + ta_rect.height + 1.0,
        "scrolled <textarea> IME cursor area {ime_area:?} must stay inside {ta_rect:?}"
    );
    let has_ta_caret = ta_scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox { background_color, .. }
                if *background_color == Color::rgb(0xea, 0xb3, 0x08)
        )
    });
    assert!(has_ta_caret, "<textarea> must apply CSS caret-color");

    assert!(tree.ime_commit("다섯번째"));
    let _ = tree.build_paint_scene(480, 320);
    assert!(tree.select_all());
    let (sel_scene, sel_relayout) = tree.build_paint_scene(480, 320);
    assert!(!sel_relayout, "<textarea> select_all = 0 relayouts");
    let sel_boxes = sel_scene
        .primitives
        .iter()
        .filter(|p| {
            matches!(
                p,
                DrawPrimitive::StyledBox { background_color, .. }
                    if *background_color == Color::rgb(0xa8, 0x55, 0xf7)
            )
        })
        .count();
    assert!(
        sel_boxes >= 2,
        "multi-line <textarea>::selection must paint per-line boxes, got {sel_boxes}"
    );
}
