//! Wave 2 regression tests: invariant `content_box_rect` zero-layout-shift across state/theme
//! border thickening, unified `text_clip_box_rect`, click-to-caret alignment with thick borders +
//! padding, `Shift+Click`, double-click word drag extension, and triple-click line selection.

#![forbid(unsafe_code)]

use krui::{krui, DrawPrimitive, LayoutRect, Mods, ViewTree};

fn find_glyph_run_by_y(
    scene: &krui::PaintScene,
    rect: LayoutRect,
) -> Option<(f32, f32, Option<LayoutRect>)> {
    scene.primitives.iter().find_map(|p| match p {
        DrawPrimitive::TextGlyphs {
            origin_x,
            origin_y,
            clip,
            glyphs,
            ..
        } if !glyphs.is_empty()
            && *origin_x >= rect.x
            && *origin_x <= rect.x + rect.width
            && *origin_y >= rect.y
            && *origin_y <= rect.y + rect.height =>
        {
            Some((*origin_x, *origin_y, *clip))
        }
        _ => None,
    })
}

#[test]
fn zero_layout_shift_on_focus_hover_border_thickening_and_theme_switch() {
    let view = krui!(
        r#"
        <style>
            [data-theme="fluent"] { --theme-ink: #111827; }
            [data-theme="cosmic"] { --theme-ink: #cbd5e1; }
            #inp { width: 200px; height: 40px; padding: 6px 12px; border: 1px solid #334455; font-size: 15px; color: var(--theme-ink); }
            #inp:hover { border-width: 2px; border-color: #6688aa; }
            #inp:focus { border-width: 3px; border-color: #60cdff; }
            #btn { width: 140px; height: 40px; padding: 6px 12px; border: 1px solid #334455; font-size: 15px; }
            #btn:hover { border-width: 2px; }
            #btn:focus-visible { border-width: 3px; }
        </style>
        <div id="host" data-theme="fluent" style="display: flex; flex-direction: row; width: 420px; height: 100px; gap: 12px;">
            <input id="inp" value="ZeroShiftText" />
            <button id="btn">ClickMe</button>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let (s_rest, _) = tree.build_paint_scene(420, 100);
    let relayouts_base = tree.relayout_passes();
    let host = tree.find_by_id("host").unwrap();
    let inp = tree.find_by_id("inp").unwrap();
    let btn = tree.find_by_id("btn").unwrap();
    let ri = tree.node_layout(inp).unwrap();
    let rb = tree.node_layout(btn).unwrap();

    let (ix0, iy0, iclip0) = find_glyph_run_by_y(&s_rest, ri).unwrap();
    let (bx0, by0, _) = find_glyph_run_by_y(&s_rest, rb).unwrap();

    let _ = tree.dispatch_pointer_move(ri.x + 12.0, ri.y + 12.0);
    let (s_hover, did_relayout_hover) = tree.build_paint_scene(420, 100);
    assert!(!did_relayout_hover);
    assert_eq!(tree.relayout_passes(), relayouts_base);
    let (ix1, iy1, _) = find_glyph_run_by_y(&s_hover, ri).unwrap();
    assert!((ix1 - ix0).abs() < 1e-5 && (iy1 - iy0).abs() < 1e-5);

    assert!(tree.dispatch_pointer_down(ri.x + 12.0, ri.y + 12.0));
    let _ = tree.dispatch_pointer_up_at(ri.x + 12.0, ri.y + 12.0);
    let (s_focus, did_relayout_focus) = tree.build_paint_scene(420, 100);
    assert!(!did_relayout_focus);
    assert_eq!(tree.relayout_passes(), relayouts_base);
    let (ix2, iy2, iclip2) = find_glyph_run_by_y(&s_focus, ri).unwrap();
    assert!((ix2 - ix0).abs() < 1e-5 && (iy2 - iy0).abs() < 1e-5);
    assert!((iclip2.unwrap().x - iclip0.unwrap().x - 2.0).abs() < 1e-3);

    assert_tab_focus_and_cosmic_alignment(
        &mut tree,
        host,
        btn,
        inp,
        rb,
        ri,
        (bx0, by0),
        (ix0, iy0),
        relayouts_base,
    );
}

#[allow(clippy::too_many_arguments)]
fn assert_tab_focus_and_cosmic_alignment(
    tree: &mut ViewTree,
    host: krui::NodeId,
    btn: krui::NodeId,
    inp: krui::NodeId,
    rb: krui::LayoutRect,
    ri: krui::LayoutRect,
    (bx0, by0): (f32, f32),
    (ix0, iy0): (f32, f32),
    relayouts_base: u64,
) {
    assert_eq!(tree.dispatch_key_tab(false), Some(btn));
    let (s_btn, _) = tree.build_paint_scene(420, 100);
    assert_eq!(tree.relayout_passes(), relayouts_base);
    let (bx2, by2, _) = find_glyph_run_by_y(&s_btn, rb).unwrap();
    assert!((bx2 - bx0).abs() < 1e-5 && (by2 - by0).abs() < 1e-5);

    tree.set_node_attr(host, "data-theme", "cosmic");
    let (s_cosmic, did_relayout_cosmic) = tree.build_paint_scene(420, 100);
    assert!(!did_relayout_cosmic);
    assert_eq!(tree.relayout_passes(), relayouts_base);
    let _ = inp;
    let (ix3, iy3, _) = find_glyph_run_by_y(&s_cosmic, ri).unwrap();
    assert!((ix3 - ix0).abs() < 1e-5 && (iy3 - iy0).abs() < 1e-5);
}

#[test]
fn click_to_caret_matches_shaped_glyph_with_thick_border_and_padding() {
    let view = krui!(
        r#"
        <style>
            #inp { width: 240px; height: 44px; border: 4px solid #334455; padding: 8px 14px; font-size: 16px; }
            #ta { width: 240px; height: 96px; border: 4px solid #334455; padding: 8px 14px; font-size: 16px; line-height: 1.3; }
        </style>
        <div style="display: flex; flex-direction: column; width: 280px; height: 180px; gap: 10px;">
            <input id="inp" value="ABCDEFG" />
            <textarea id="ta" value="LineZero&#10;LineOne"></textarea>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let (scene, _) = tree.build_paint_scene(280, 180);
    let inp = tree.find_by_id("inp").unwrap();
    let ta = tree.find_by_id("ta").unwrap();
    let ri = tree.node_layout(inp).unwrap();
    let rt = tree.node_layout(ta).unwrap();

    // Reset `<input>` scroll/cursor to start.
    assert!(tree.dispatch_pointer_down(ri.x + 20.0, ri.y + 20.0));
    let _ = tree.dispatch_pointer_up_at(ri.x + 20.0, ri.y + 20.0);
    assert!(tree.dispatch_cursor_home());
    let (scene2, _) = tree.build_paint_scene(280, 180);
    let (ox, oy, _) = find_glyph_run_by_y(&scene2, ri).unwrap();
    assert!(tree.dispatch_pointer_down(ox + 1.0, oy + 6.0));
    let _ = tree.dispatch_pointer_up_at(ox + 1.0, oy + 6.0);
    assert_eq!(tree.node_cursor_char_idx(inp), Some(0));

    let (tax, tay, _) = find_glyph_run_by_y(&scene, rt).unwrap();
    assert!(tree.dispatch_pointer_down(tax + 1.0, tay + 4.0));
    let _ = tree.dispatch_pointer_up_at(tax + 1.0, tay + 4.0);
    assert_eq!(tree.node_cursor_char_idx(ta), Some(0));

    assert!(tree.dispatch_pointer_down(tax + 1.0, tay + 24.0));
    let _ = tree.dispatch_pointer_up_at(tax + 1.0, tay + 24.0);
    assert_eq!(tree.node_cursor_char_idx(ta), Some(9));
}

#[test]
fn shift_click_double_click_word_drag_and_triple_click_line_selection() {
    let view = krui!(
        r#"
        <style>
            #inp { width: 320px; height: 40px; border: 2px solid #334455; padding: 6px 12px; font-size: 16px; }
            #ta { width: 320px; height: 90px; border: 2px solid #334455; padding: 6px 12px; font-size: 16px; line-height: 1.3; }
        </style>
        <div style="display: flex; flex-direction: column; width: 360px; height: 160px; gap: 10px;">
            <input id="inp" value="alpha beta gamma delta" />
            <textarea id="ta" value="first line&#10;second line"></textarea>
        </div>
        "#
    );
    let mut tree = ViewTree::new();
    tree.mount(view);
    let _ = tree.build_paint_scene(360, 160);
    let inp = tree.find_by_id("inp").unwrap();
    let ta = tree.find_by_id("ta").unwrap();
    let ri = tree.node_layout(inp).unwrap();
    let rt = tree.node_layout(ta).unwrap();

    // Put caret at char 0 of input and verify `Shift+Click` extends right.
    assert!(tree.dispatch_pointer_down(ri.x + 15.0, ri.y + 18.0));
    let _ = tree.dispatch_pointer_up_at(ri.x + 15.0, ri.y + 18.0);
    assert!(tree.dispatch_cursor_home());
    let (scene, _) = tree.build_paint_scene(360, 160);
    let (ox, oy, _) = find_glyph_run_by_y(&scene, ri).unwrap();
    assert!(tree.dispatch_pointer_down(ox + 1.0, oy + 6.0));
    let _ = tree.dispatch_pointer_up_at(ox + 1.0, oy + 6.0);
    assert!(tree.dispatch_pointer_down_with_mods(
        ox + 60.0,
        oy + 6.0,
        Mods {
            shift: true,
            ..Mods::default()
        }
    ));
    let _ = tree.dispatch_pointer_up_at(ox + 60.0, oy + 6.0);
    assert!(tree.selected_text().is_some_and(|s| s.starts_with("alpha")));

    // Double-click inside "beta" + drag, then triple-click line 1 of `<textarea>`.
    let (tax, tay, _) = find_glyph_run_by_y(&scene, rt).unwrap();
    assert_double_and_triple_click_selection(&mut tree, ox, oy, tax, tay);
}

fn assert_double_and_triple_click_selection(
    tree: &mut ViewTree,
    ox: f32,
    oy: f32,
    tax: f32,
    tay: f32,
) {
    let beta_x = ox + 58.0;
    assert!(tree.dispatch_pointer_down_with_mods_at_time(beta_x, oy + 6.0, Mods::default(), 1000));
    let _ = tree.dispatch_pointer_up_at(beta_x, oy + 6.0);
    assert!(tree.dispatch_pointer_down_with_mods_at_time(beta_x, oy + 6.0, Mods::default(), 1150));
    assert_eq!(tree.selected_text().as_deref(), Some("beta"));
    let _ = tree.dispatch_pointer_drag(ox + 120.0, oy + 6.0);
    assert!(tree.selected_text().is_some_and(|s| s.starts_with("beta ")));
    let _ = tree.dispatch_pointer_drag(ox + 8.0, oy + 6.0);
    assert_eq!(tree.selected_text().as_deref(), Some("alpha beta"));
    let _ = tree.dispatch_pointer_up_at(ox + 8.0, oy + 6.0);

    for t in [2000_u64, 2120, 2240] {
        assert!(tree.dispatch_pointer_down_with_mods_at_time(
            tax + 24.0,
            tay + 24.0,
            Mods::default(),
            t
        ));
        let _ = tree.dispatch_pointer_up_at(tax + 24.0, tay + 24.0);
    }
    assert_eq!(tree.selected_text().as_deref(), Some("second line"));
}
