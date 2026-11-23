//! C3 Regression Suite: Multi-line 2D text hit-testing, per-line selection rectangles,
//! RTL visual caret progression, `dir` / `text-align`, and interactive scrollbar/keyboard scroll.

#![forbid(unsafe_code)]

use krui::*;
use krui_core::{DrawPrimitive, FocusState, KeyAction, KeyInfo, Mods};
use krui_text::TextEngine;
use std::cell::Cell;
use std::rc::Rc;

#[test]
fn wrapped_text_hit_test_per_line() {
    let mut engine = TextEngine::new();
    let text = "Hello world first line here and second line follows right after";
    let shaped = engine.shape_and_rasterize(text, 16.0, 1.3, 400, Color::BLACK, Some(160.0));
    assert!(shaped.lines.len() >= 2);
    let (line0, line1) = (&shaped.lines[0], &shaped.lines[1]);
    assert_eq!(line0.start_char, 0);
    assert!(line1.start_char > 0);
    assert_eq!(shaped.hit_test(2.0, line0.line_top + 4.0), 0);
    assert_eq!(shaped.hit_test(2.0, line1.line_top + 4.0), line1.start_char);

    let view = krui!(
        r#"
        <style>
            div.root { width: 220px; height: 200px; padding: 10px; }
            p.para { width: 160px; font-size: 16px; line-height: 1.3; user-select: text; }
        </style>
        <div class="root">
            <p class="para">Hello world first line here and second line follows right after</p>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    let _roots = view.mount(&mut tree, None);
    let _ = tree.build_paint_scene(220, 200);

    tree.dispatch_pointer_down(12.0, 10.0 + line1.line_top + 4.0);
    tree.dispatch_pointer_drag(140.0, 10.0 + line1.line_top + 4.0);
    tree.end_pointer_drag();

    let selected = tree.selected_text().expect("selection on line 2");
    let expected_prefix: String = text.chars().skip(line1.start_char).take(4).collect();
    assert!(selected.starts_with(&expected_prefix));
}

#[test]
fn multi_line_selection_paints_per_line() {
    let view = krui!(
        r#"
        <style>
            div.root { width: 240px; height: 220px; padding: 10px; }
            p.para { width: 160px; font-size: 16px; line-height: 1.3; user-select: text; }
        </style>
        <div class="root">
            <p class="para">Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda</p>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    let _roots = view.mount(&mut tree, None);
    let _ = tree.build_paint_scene(240, 220);

    tree.dispatch_click(25.0, 20.0);
    assert!(
        tree.select_all(),
        "select_all should succeed on focused selectable paragraph"
    );

    let (scene, _) = tree.build_paint_scene(240, 220);
    let sel_color = krui_style::UA_SELECTION_BG_COLOR;
    let highlight_boxes: Vec<_> = scene
        .primitives
        .iter()
        .filter_map(|p| match *p {
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if background_color == sel_color && rect.width > 0.0 => Some(rect),
            _ => None,
        })
        .collect();

    assert!(
        highlight_boxes.len() >= 2,
        "multi-line selection must emit at least 2 per-line highlight boxes, got {:?}",
        highlight_boxes
    );
    assert!(
        highlight_boxes[1].y > highlight_boxes[0].y + 8.0,
        "second selection box must be on a lower line than the first: {:?}",
        highlight_boxes
    );
}

#[test]
fn rtl_caret_matches_visual_order() {
    let mut engine = TextEngine::new();
    let rtl_text = "שלום";
    let shaped = engine.shape_and_rasterize(rtl_text, 18.0, 1.3, 400, Color::BLACK, None);
    let char_count = rtl_text.chars().count();
    let (x0, x_last) = (
        shaped.cursor_x_for_char(0),
        shaped.cursor_x_for_char(char_count),
    );
    assert!(x0 > x_last);
    for i in 0..char_count {
        assert!(shaped.cursor_x_for_char(i) >= shaped.cursor_x_for_char(i + 1));
    }
    assert_eq!(shaped.hit_test(x0 + 2.0, 4.0), 0);
    assert_eq!(shaped.hit_test(0.0, 4.0), char_count);

    let view = krui!(
        r#"
        <style>
            div.root { width: 300px; height: 120px; padding: 10px; }
            input.rtl_inp { width: 240px; height: 36px; padding: 6px; font-size: 16px; }
        </style>
        <div class="root">
            <input class="rtl_inp" dir="rtl" value="שלום" />
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    let _roots = view.mount(&mut tree, None);
    let _ = tree.build_paint_scene(300, 120);
    tree.dispatch_key_tab(false);
    tree.dispatch_cursor_home();
    let _ = tree.build_paint_scene(300, 120);
    let home_caret = tree.focused_ime_cursor_area().expect("caret at RTL home");
    tree.dispatch_cursor_end();
    let _ = tree.build_paint_scene(300, 120);
    let end_caret = tree.focused_ime_cursor_area().expect("caret at RTL end");
    assert!(home_caret.x > end_caret.x);
}

#[test]
fn scrollbar_drag_updates_scroll_y() {
    let scroll_fires = Rc::new(Cell::new(0u32));
    let sc = scroll_fires.clone();
    let on_scroll = Command::new(move || sc.set(sc.get() + 1));

    let view = krui!(
        r#"
        <style>
            div.viewport { width: 200px; height: 100px; overflow-y: scroll; padding: 4px; }
            div.row { width: 160px; height: 50px; margin: 4px; }
        </style>
        <div id="vp" class="viewport" onscroll="{on_scroll}">
            <div class="row">Row 1</div>
            <div class="row">Row 2</div>
            <div class="row">Row 3</div>
            <div class="row">Row 4</div>
            <div class="row">Row 5</div>
            <div class="row">Row 6</div>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    let _roots = view.mount(&mut tree, None);
    let _ = tree.build_paint_scene(200, 100);

    let vp_id = tree.find_by_id("vp").expect("find viewport");
    assert!(tree.dispatch_pointer_down(195.0, 12.0));
    assert!(tree.dispatch_pointer_drag(195.0, 80.0));
    tree.end_pointer_drag();

    let scroll_y = tree.scroll_offset_y(vp_id).unwrap_or(0.0);
    assert!(
        scroll_y > 40.0,
        "dragging scrollbar thumb down must increase scroll_y, got {scroll_y}"
    );
    assert!(
        scroll_fires.get() >= 1,
        "onscroll callback must fire when dragging scrollbar thumb"
    );
}

#[test]
fn page_down_scrolls_without_moving_focus() {
    let view = krui!(
        r#"
        <style>
            div.viewport { width: 220px; height: 100px; overflow-y: scroll; padding: 4px; }
            button.item { width: 180px; height: 44px; margin: 4px; }
        </style>
        <div id="vp" class="viewport">
            <button class="item">Button 1</button>
            <button class="item">Button 2</button>
            <button class="item">Button 3</button>
            <button class="item">Button 4</button>
            <button class="item">Button 5</button>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    let _roots = view.mount(&mut tree, None);
    let _ = tree.build_paint_scene(220, 100);

    let vp_id = tree.find_by_id("vp").expect("find viewport");
    let btn1 = tree.dispatch_key_tab(false).expect("focus Button 1");
    assert_eq!(tree.focused_node_id(), Some(btn1));

    let action = tree.dispatch_routed_key(&KeyInfo::PageDown, Mods::default(), false);
    assert_eq!(action, KeyAction::PageDown);
    assert!(tree.dispatch_page_down());

    let after_page_down = tree.scroll_offset_y(vp_id).unwrap_or(0.0);
    assert!(
        after_page_down > 50.0,
        "PageDown should scroll viewport by ~85% of height, got {after_page_down}"
    );
    assert_eq!(
        tree.focused_node_id(),
        Some(btn1),
        "PageDown must not change keyboard focus"
    );

    let down_action = krui_core::route_key(
        &KeyInfo::ArrowDown,
        Mods::default(),
        &FocusState {
            is_input_focused: false,
            has_ime_preedit: false,
            is_ime_enabled: false,
        },
    );
    assert_eq!(down_action, KeyAction::ScrollDown);
    assert!(tree.dispatch_scroll_down());
    assert_eq!(tree.focused_node_id(), Some(btn1));
}

#[test]
fn css_flex_wrap_and_textarea_multiline_ime_underline_end_to_end() {
    let notes = Signal::new("첫번째 줄입니다\n".to_string());
    let view = krui!(
        r#"
        <style>
            div.shell { width: 260px; height: 220px; display: flex; flex-direction: column; gap: 8px; }
            div.chips { display: flex; flex-direction: row; flex-wrap: wrap; width: 240px; gap: 8px; }
            span.chip { width: 90px; height: 24px; }
            textarea#ed { width: 240px; height: 100px; font-size: 16px; line-height: 1.4; padding: 6px; }
        </style>
        <div class="shell">
            <div class="chips">
                <span id="c1" class="chip">A</span>
                <span id="c2" class="chip">B</span>
                <span id="c3" class="chip">C</span>
            </div>
            <textarea id="ed" bind:value="{notes}"></textarea>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    let _roots = view.mount(&mut tree, None);
    let _ = tree.build_paint_scene(260, 220);

    let c1 = tree
        .node_layout(tree.find_by_id("c1").expect("c1"))
        .expect("r1");
    let c3 = tree
        .node_layout(tree.find_by_id("c3").expect("c3"))
        .expect("r3");
    assert!(
        c3.y >= c1.y + 24.0,
        "Third 90px chip must wrap onto second row"
    );

    let ed = tree.find_by_id("ed").expect("ed");
    let ed_rect = tree.node_layout(ed).expect("ed_rect");
    tree.dispatch_pointer_down(ed_rect.x + 12.0, ed_rect.y + 40.0);
    tree.end_pointer_drag();
    assert!(tree.ime_preedit("조합중", Some((0, 9))));
    let (scene, _) = tree.build_paint_scene(260, 220);

    let underline_y = scene.primitives.iter().find_map(|p| match p {
        DrawPrimitive::StyledBox {
            rect,
            background_color,
            ..
        } if *background_color == krui_style::UA_PRIMARY_TEXT_COLOR && rect.height <= 3.0 => {
            Some(rect.y)
        }
        _ => None,
    });
    let y = underline_y.expect("textarea IME underline emitted");
    assert!(
        y > ed_rect.y + 6.0 + 22.0,
        "Line 2 IME underline must draw below line 0 (got y={y}, ed_top={})",
        ed_rect.y
    );
}
