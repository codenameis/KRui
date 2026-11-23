//! Stage E5 Regression Suite:
//! - Explicit `padding: 0` distinguished from unspecified UA padding on `<input>`, checkbox, and radio
//! - Checkbox/radio `20px` size, radio `border-radius`, and indicator sizing driven by UA CSS + `accent-color`
//! - `<input type="range">` slider driven by `accent-color` instead of reusing `theme.caret_color`
//! - `scrollbar-width` (`auto | thin | none`) and `scrollbar-color` (`auto | <thumb> <track>`)
//! - Unsupported pseudo-elements (`::-webkit-slider-thumb`, etc.) warn + drop, and `@supports selector(...)` is `false`

#![forbid(unsafe_code)]

use krui::*;
use krui_compiler::compile_krui_source;

fn find_id(tree: &ViewTree, id: &str) -> NodeId {
    tree.find_by_id(id)
        .unwrap_or_else(|| panic!("expected `#{id}`"))
}

fn boxes_near(scene: &PaintScene, rect: LayoutRect) -> Vec<(LayoutRect, Color, f32)> {
    scene
        .primitives
        .iter()
        .filter_map(|p| match p {
            DrawPrimitive::StyledBox {
                rect: r,
                background_color,
                border_radius,
                ..
            } if r.x >= rect.x - 10.0
                && r.x + r.width <= rect.x + rect.width + 10.0
                && r.y >= rect.y - 10.0
                && r.y + r.height <= rect.y + rect.height + 10.0 =>
            {
                Some((*r, *background_color, *border_radius))
            }
            _ => None,
        })
        .collect()
}

fn canvas_stroke_span_near(scene: &PaintScene, rect: LayoutRect) -> Option<f32> {
    for p in &scene.primitives {
        if let DrawPrimitive::CanvasBatch {
            viewport, commands, ..
        } = p
        {
            if (viewport.x - rect.x).abs() < 1.0 && (viewport.y - rect.y).abs() < 1.0 {
                let mut xs = Vec::new();
                for cmd in commands.iter() {
                    if let CanvasCommand::StrokePath { segments, .. } = cmd {
                        for seg in segments {
                            match *seg {
                                PathSegment::MoveTo(x, _) | PathSegment::LineTo(x, _) => xs.push(x),
                                _ => {}
                            }
                        }
                    }
                }
                if xs.len() >= 2 {
                    let min_x = xs.iter().copied().fold(f32::INFINITY, f32::min);
                    let max_x = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                    return Some(max_x - min_x);
                }
            }
        }
    }
    None
}

#[test]
fn explicit_padding_zero_and_css_geometry_override_ua_checkbox_and_radio() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                #wrap { display: flex; flex-direction: row; gap: 12px; width: 320px; height: 80px; }
                #rad-sq { width: 32px; height: 32px; padding: 0px; border-radius: 0px; }
                #cb-zero { padding: 0px; }
            </style>
            <div id="wrap">
                <input id="cb-def" type="checkbox" checked />
                <input id="cb-zero" type="checkbox" checked />
                <input id="rad-def" type="radio" name="g1" checked />
                <input id="rad-sq" type="radio" name="g2" checked />
                <input id="txt-zero" value="abc" style="padding: 0px; border: none;" />
            </div>
        "#
        )
    });

    let (scene, _) = tree.build_paint_scene(320, 80);

    let cb_def = tree.node_layout(find_id(&tree, "cb-def")).unwrap();
    let cb_zero = tree.node_layout(find_id(&tree, "cb-zero")).unwrap();
    let rad_def = tree.node_layout(find_id(&tree, "rad-def")).unwrap();
    let rad_sq = tree.node_layout(find_id(&tree, "rad-sq")).unwrap();

    assert_eq!((cb_def.width, cb_def.height), (20.0, 20.0));
    assert_eq!((rad_def.width, rad_def.height), (20.0, 20.0));
    assert_eq!((rad_sq.width, rad_sq.height), (32.0, 32.0));

    // Checkmark horizontal span with `padding: 0px` must be wider than UA padded checkbox.
    let span_def = canvas_stroke_span_near(&scene, cb_def).expect("cb-def checkmark");
    let span_zero = canvas_stroke_span_near(&scene, cb_zero).expect("cb-zero checkmark");
    assert!(
        span_zero > span_def + 1.0,
        "explicit padding:0px must widen checkbox content box checkmark span (`{span_zero}` vs `{span_def}`)"
    );

    // `#rad-sq` outer box must keep author `border-radius: 0px` (not clamped to >= 10px),
    // and its checked dot inside 32x32 padding:0px must scale beyond the old 10px cap!
    let sq_boxes = boxes_near(&scene, rad_sq);
    let outer = sq_boxes
        .iter()
        .find(|(r, _, _)| (r.width - 32.0).abs() < 0.5)
        .expect("rad-sq outer box");
    assert!(
        outer.2.abs() < 0.01,
        "author `border-radius: 0px` on radio must not be clamped to 10px, got {}",
        outer.2
    );
    let dot = sq_boxes
        .iter()
        .find(|(r, _, _)| r.width < 31.5 && r.width > 4.0)
        .expect("rad-sq inner dot");
    assert!(
        dot.0.width > 12.0,
        "32x32 radio dot with padding:0px must scale past old 10px clamp, got {}",
        dot.0.width
    );
}

#[test]
fn accent_color_drives_checkbox_radio_and_slider_independently_of_caret_color() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                #form {
                    display: flex;
                    flex-direction: column;
                    gap: 10px;
                    width: 300px;
                    height: 180px;
                    accent-color: #ec4899;
                    caret-color: #ef4444;
                }
                #rng-green {
                    accent-color: #10b981;
                }
            </style>
            <div id="form">
                <input id="cb-pink" type="checkbox" checked />
                <input id="rad-pink" type="radio" checked />
                <input id="rng-pink" type="range" min="0" max="100" value="50" />
                <input id="rng-green" type="range" min="0" max="100" value="50" />
                <input id="txt-caret" value="hi" />
            </div>
        "#
        )
    });

    let txt_id = find_id(&tree, "txt-caret");
    let _ = tree.build_paint_scene(300, 180);
    let tr = tree.node_layout(txt_id).unwrap();
    assert!(tree.dispatch_click(tr.x + 4.0, tr.y + 4.0));
    let (scene, _) = tree.build_paint_scene(300, 180);

    let pink = Color::rgb(0xec, 0x48, 0x99);
    let green = Color::rgb(0x10, 0xb9, 0x81);
    let red_caret = Color::rgb(0xef, 0x44, 0x44);

    let rng_pink_rect = tree.node_layout(find_id(&tree, "rng-pink")).unwrap();
    let rng_green_rect = tree.node_layout(find_id(&tree, "rng-green")).unwrap();
    let cb_pink_rect = tree.node_layout(find_id(&tree, "cb-pink")).unwrap();
    let rad_pink_rect = tree.node_layout(find_id(&tree, "rad-pink")).unwrap();

    let pink_slider_boxes = boxes_near(&scene, rng_pink_rect);
    assert!(
        pink_slider_boxes.iter().any(|(_, c, _)| *c == pink),
        "inherited accent-color #ec4899 must tint slider fill/thumb: {pink_slider_boxes:?}"
    );
    assert!(
        pink_slider_boxes.iter().all(|(_, c, _)| *c != red_caret),
        "slider must never reuse caret-color!"
    );

    let green_slider_boxes = boxes_near(&scene, rng_green_rect);
    assert!(
        green_slider_boxes.iter().any(|(_, c, _)| *c == green),
        "element accent-color #10b981 must override inherited accent-color: {green_slider_boxes:?}"
    );

    let cb_boxes = boxes_near(&scene, cb_pink_rect);
    assert!(
        cb_boxes.iter().any(|(_, c, _)| *c == pink),
        "checked checkbox must use accent-color #ec4899: {cb_boxes:?}"
    );

    let rad_boxes = boxes_near(&scene, rad_pink_rect);
    assert!(
        rad_boxes.iter().any(|(_, c, _)| *c == pink),
        "checked radio must use accent-color #ec4899: {rad_boxes:?}"
    );
}

#[test]
fn scrollbar_width_and_scrollbar_color_style_and_hide_scrollbars() {
    reset_reactive_runtime();
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                #row { display: flex; flex-direction: row; gap: 16px; width: 420px; height: 120px; }
                .sc { display: flex; flex-direction: column; width: 120px; height: 80px; overflow: scroll; }
                .tall { width: 100px; height: 260px; flex-shrink: 0; }
                #sc-thin { scrollbar-width: thin; scrollbar-color: #ef4444 #dbeafe; }
                #sc-none { scrollbar-width: none; }
            </style>
            <div id="row">
                <div id="sc-auto" class="sc"><div class="tall" /></div>
                <div id="sc-thin" class="sc"><div class="tall" /></div>
                <div id="sc-none" class="sc"><div class="tall" /></div>
            </div>
        "#
        )
    });

    let (scene, _) = tree.build_paint_scene(420, 120);

    let r_auto = tree.node_layout(find_id(&tree, "sc-auto")).unwrap();
    let r_thin = tree.node_layout(find_id(&tree, "sc-thin")).unwrap();
    let r_none = tree.node_layout(find_id(&tree, "sc-none")).unwrap();

    let auto_thumbs: Vec<_> = boxes_near(&scene, r_auto)
        .into_iter()
        .filter(|(r, _, _)| r.width < 20.0 && r.x > r_auto.x + 80.0)
        .collect();
    assert!(
        !auto_thumbs.is_empty(),
        "scrollbar-width:auto must emit thumb"
    );

    let thin_boxes: Vec<_> = boxes_near(&scene, r_thin)
        .into_iter()
        .filter(|(r, _, _)| r.width < 20.0 && r.x > r_thin.x + 80.0)
        .collect();
    assert!(
        thin_boxes
            .iter()
            .any(|(_, c, _)| *c == Color::rgb(0xef, 0x44, 0x44)),
        "scrollbar-color thumb #ef4444 must be emitted: {thin_boxes:?}"
    );
    assert!(
        thin_boxes
            .iter()
            .any(|(_, c, _)| *c == Color::rgb(0xdb, 0xea, 0xfe)),
        "scrollbar-color track #dbeafe must be emitted: {thin_boxes:?}"
    );
    let thin_thumb_w = thin_boxes
        .iter()
        .find(|(_, c, _)| *c == Color::rgb(0xef, 0x44, 0x44))
        .unwrap()
        .0
        .width;
    assert!(
        thin_thumb_w < auto_thumbs[0].0.width,
        "scrollbar-width:thin ({thin_thumb_w}) must be narrower than auto ({})",
        auto_thumbs[0].0.width
    );

    let none_boxes: Vec<_> = boxes_near(&scene, r_none)
        .into_iter()
        .filter(|(r, _, _)| r.width < 20.0 && r.x > r_none.x + 80.0)
        .collect();
    assert!(
        none_boxes.is_empty(),
        "scrollbar-width:none must suppress scrollbar thumb/track, got {none_boxes:?}"
    );

    // Wheel scrolling must still work on `scrollbar-width: none`!
    assert!(tree.dispatch_scroll(r_none.x + 20.0, r_none.y + 20.0, -40.0));
}

#[test]
fn unsupported_vendor_pseudo_elements_warn_drop_and_supports_selector_is_false() {
    let src = r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            input::-webkit-slider-thumb { background-color: #ef4444; }
            input::-moz-range-thumb { background-color: #ef4444; }
            div::-webkit-scrollbar { width: 8px; }
            @supports selector(::-webkit-slider-thumb) {
                #probe { background-color: #ef4444; }
            }
            @supports not selector(::-webkit-slider-thumb) {
                #probe { color: #22c55e; }
            }
            @supports (accent-color: #ec4899) and (scrollbar-width: thin) {
                #probe-ok { color: #3b82f6; }
            }
        </style>
        <div>
            <div id="probe">A</div>
            <div id="probe-ok">B</div>
        </div>
    "#;
    let mut tree = ViewTree::new();
    let _ = tree.take_style_diagnostics();
    let doc = compile_krui_source(src)
        .expect("vendor pseudo-elements must drop cleanly without CompileError");
    for s in &doc.stylesheets {
        let _ = tree.add_author_stylesheet(s);
    }
    assert!(
        tree.take_style_diagnostics().len() >= 3,
        "expected diagnostics for dropped vendor pseudo-elements"
    );

    tree.mount_with(|| {
        krui!(
            r#"
            <style>
                @supports selector(::-webkit-slider-thumb) {
                    #probe { background-color: #ef4444; }
                }
                @supports not selector(::-webkit-slider-thumb) {
                    #probe { color: #22c55e; }
                }
                @supports (accent-color: #ec4899) and (scrollbar-width: thin) {
                    #probe-ok { color: #3b82f6; }
                }
            </style>
            <div>
                <div id="probe">A</div>
                <div id="probe-ok">B</div>
            </div>
        "#
        )
    });
    let _ = tree.build_paint_scene(300, 200);
    let probe = tree
        .node_computed_style(find_id(&tree, "probe"))
        .expect("probe style");
    let probe_ok = tree
        .node_computed_style(find_id(&tree, "probe-ok"))
        .expect("probe-ok style");
    assert_ne!(
        probe.paint.background_color,
        Color::rgb(0xef, 0x44, 0x44),
        "@supports selector(::-webkit-slider-thumb) must evaluate to false"
    );
    assert_eq!(
        probe.paint.text_color,
        Color::rgb(0x22, 0xc5, 0x5e),
        "@supports not selector(::-webkit-slider-thumb) must evaluate to true"
    );
    assert_eq!(
        probe_ok.paint.text_color,
        Color::rgb(0x3b, 0x82, 0xf6),
        "@supports must recognize accent-color and scrollbar-width"
    );
}
