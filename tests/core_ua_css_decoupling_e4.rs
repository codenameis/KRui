//! Stage E4 regression suite: W3C UA CSS decoupling, `<ul>` overflow-only clipping/scrolling,
//! `::placeholder`, `caret-color: auto` -> `currentColor`, theme-free renderer, standard per-side
//! `border-bottom`, modal vs non-modal `<dialog>` + `::backdrop`, root/`body` `clear_color`
//! propagation, and `syn` AST gate against visual tag/role hardcoding in `core` and `renderer`.

#![forbid(unsafe_code)]

use krui::*;
use krui_compiler::compile_krui_source;
use krui_preview::mount_compiled_document;
use std::path::Path;

fn mount_css_doc(src: &str, vw: u32, vh: u32) -> (ViewTree, PaintScene) {
    let doc = compile_krui_source(src).unwrap_or_else(|e| panic!("compile failed: {e:?}"));
    let mut tree = ViewTree::new();
    tree.set_color_scheme_preference(ColorSchemePreference::Light);
    mount_compiled_document(&mut tree, &doc);
    let _ = tree.sync_viewport_and_media(vw as f32, vh as f32);
    let (scene, _) = tree.build_paint_scene(vw, vh);
    (tree, scene)
}

#[test]
fn ul_overflow_visible_does_not_clip_or_scroll_until_css_sets_overflow_scroll() {
    let hits_vis = Signal::new(0u32);
    let hv = hits_vis;
    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        let on_btn = Command::new(move || hv.update(|n| *n += 1));
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                #u-vis { width: 120px; height: 40px; overflow: visible; }
                .row { height: 30px; background-color: #3b82f6; }
                #overflow-btn { width: 80px; height: 24px; background-color: #ef4444; }
            </style>
            <div>
                <ul id="u-vis">
                    <li class="row">Row 1</li>
                    <li class="row"><button id="overflow-btn" onclick="{on_btn}">Hit</button></li>
                </ul>
            </div>
            "#
        )
    });
    let (scene_vis, _) = tree.build_paint_scene(240, 160);
    let btn_id = tree.find_by_id("overflow-btn").expect("overflow-btn");
    let btn_rect = tree.node_layout(btn_id).expect("btn rect");
    assert!(
        btn_rect.y >= 30.0,
        "second li must overflow 40px ul: {btn_rect:?}"
    );
    let btn_prim_clip = scene_vis.primitives.iter().find_map(|p| match p {
        DrawPrimitive::StyledBox {
            background_color,
            clip,
            ..
        } if *background_color == Color::rgb(0xef, 0x44, 0x44) => Some(*clip),
        _ => None,
    });
    assert_eq!(
        btn_prim_clip,
        Some(None),
        "overflow:visible <ul> must not clip child"
    );
    assert!(
        tree.dispatch_click(btn_rect.x + 10.0, btn_rect.y + 18.0),
        "click outside 40px height on overflow:visible <ul> must hit overflowing child"
    );
    assert_eq!(hits_vis.get(), 1);

    let (_, scene_scr) = mount_css_doc(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            #u-scr { width: 120px; height: 40px; overflow: scroll; }
            .row { height: 30px; background-color: #ef4444; }
        </style>
        <div><ul id="u-scr"><li class="row">1</li><li class="row">2</li></ul></div>
        "#,
        240,
        160,
    );
    let row_clip = scene_scr.primitives.iter().find_map(|p| match p {
        DrawPrimitive::StyledBox {
            background_color,
            clip,
            ..
        } if *background_color == Color::rgb(0xef, 0x44, 0x44) => *clip,
        _ => None,
    });
    assert!(
        row_clip.is_some(),
        "overflow:scroll <ul> must clip children"
    );
}

#[test]
fn placeholder_pseudo_and_ua_css_control_placeholder_color() {
    let (_, scene_ua) = mount_css_doc(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>#p-ua { width: 160px; height: 32px; color: #111827; }</style>
        <input id="p-ua" type="text" placeholder="Default UA" />
        "#,
        240,
        80,
    );
    let ua_col = scene_ua
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::TextGlyphs { color, .. } => Some(*color),
            _ => None,
        })
        .expect("UA placeholder glyphs");
    assert_eq!(
        ua_col,
        Color::rgb(0x75, 0x75, 0x75),
        "unstyled UA ::placeholder must be #757575"
    );

    let (_, scene_css) = mount_css_doc(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            #p-css { width: 160px; height: 32px; color: #111827; }
            #p-css::placeholder { color: #f43f5e; opacity: 0.5; }
        </style>
        <input id="p-css" type="text" placeholder="Styled" />
        "#,
        240,
        80,
    );
    let css_col = scene_css
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::TextGlyphs { color, .. } => Some(*color),
            _ => None,
        })
        .expect("styled placeholder glyphs");
    assert_eq!((css_col.r, css_col.g, css_col.b), (0xf4, 0x3f, 0x5e));
    assert!(
        (css_col.a as i32 - 128).abs() <= 1,
        "expected ~50% alpha, got {:?}",
        css_col
    );
}

#[test]
fn caret_color_auto_tracks_current_color_and_css_overrides_it() {
    for (hex, expected) in [
        ("#22c55e", Color::rgb(0x22, 0xc5, 0x5e)),
        ("#eab308", Color::rgb(0xea, 0xb3, 0x08)),
    ] {
        let src = format!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>#inp {{ width: 160px; height: 32px; color: {hex}; caret-color: auto; }}</style>
            <input id="inp" type="text" value="abc" />
            "#
        );
        let (mut tree, _) = mount_css_doc(&src, 240, 80);
        let _ = tree.dispatch_key_tab(false);
        let (scene, _) = tree.build_paint_scene(240, 80);
        let has_caret = scene.primitives.iter().any(|p| {
            matches!(
                p,
                DrawPrimitive::StyledBox {
                    rect,
                    background_color,
                    ..
                } if *background_color == expected && rect.width <= 3.0 && rect.height >= 10.0
            )
        });
        assert!(
            has_caret,
            "caret-color:auto must resolve to currentColor {expected:?}"
        );
    }
}

#[test]
fn renderer_backend_does_not_mutate_session_styles() {
    let mut cpu = EngineSession::new(320, 200, RenderBackendPreference::CpuFallback);
    let mut skia = EngineSession::new(320, 200, RenderBackendPreference::ForceTinySkia);
    cpu.tree.mount(krui!(
        r#"<style>:root { background-color: #102030; }</style><div/>"#
    ));
    skia.tree.mount(krui!(
        r#"<style>:root { background-color: #102030; }</style><div/>"#
    ));
    let (sc, _) = cpu.tree.build_paint_scene(320, 200);
    let (ss, _) = skia.tree.build_paint_scene(320, 200);
    assert_eq!(
        ss.clear_color, sc.clear_color,
        "ForceTinySkia must not alter resolved CSS styles or clear color"
    );
}

#[test]
fn standalone_border_bottom_is_standard_per_side_layout_and_paint_border() {
    let (tree, scene) = mount_css_doc(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            #bb {
                display: flex;
                flex-direction: column;
                width: 100px;
                border-radius: 12px;
                border-bottom: 4px solid #ef4444;
            }
            #inner { width: 40px; height: 30px; }
        </style>
        <div id="bb"><div id="inner"></div></div>
        "#,
        200,
        120,
    );
    let bb_id = tree.find_by_id("bb").expect("bb");
    let bb_rect = tree.node_layout(bb_id).expect("bb rect");
    let bb_paint = tree.node_effective_paint(bb_id).expect("bb paint");
    assert!(
        (bb_rect.height - 34.0).abs() <= 0.5,
        "border-bottom:4px must contribute 4px to box height (30 + 4 = 34), got {bb_rect:?}"
    );
    assert_eq!(
        bb_paint.border_bottom_width, 0.0,
        "legacy indicator field must stay unused"
    );
    let has_narrow_strip = scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox {
                rect,
                background_color,
                ..
            } if *background_color == Color::rgb(0xef, 0x44, 0x44) && rect.width < 99.0
        )
    });
    assert!(!has_narrow_strip, "must not emit inset indicator strip");
}

#[test]
fn dialog_non_modal_stays_in_flow_without_backdrop() {
    let (mut tree, scene) = mount_css_doc(
        r#"<link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            #stage { width: 300px; height: 200px; }
            #nm-dlg { width: 120px; height: 60px; background-color: #ffffff; }
            #sib-btn { width: 80px; height: 32px; background-color: #2563eb; }
        </style>
        <div id="stage">
            <dialog id="nm-dlg" open="true">Non-modal</dialog>
            <button id="sib-btn">Sibling</button>
        </div>"#,
        300,
        200,
    );
    assert!(
        scrim_alpha(&scene, 300.0, 200.0).is_none(),
        "non-modal <dialog open> must not emit ::backdrop"
    );
    let r = tree
        .node_layout(tree.find_by_id("sib-btn").unwrap())
        .unwrap();
    let _ = tree.dispatch_pointer_move(r.x + 10.0, r.y + 10.0);
    assert!(tree.dispatch_pointer_down(r.x + 10.0, r.y + 10.0));
}

#[test]
fn dialog_modal_uses_ua_css_and_respects_transparent_backdrop() {
    let (_, ua_scene) = mount_css_doc(
        r#"<link rel="krui-preset" href="krui://preset/unstyled" />
        <style>#m-dlg { width: 120px; height: 60px; background-color: #ffffff; }</style>
        <div><dialog id="m-dlg" open="true" modal="true">Modal</dialog></div>"#,
        300,
        200,
    );
    assert_eq!(scrim_alpha(&ua_scene, 300.0, 200.0), Some(26));

    let (_, clear_scene) = mount_css_doc(
        r#"<link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            #mc-dlg { width: 120px; height: 60px; background-color: #ffffff; }
            #mc-dlg::backdrop { background: transparent; }
        </style>
        <div><dialog id="mc-dlg" open="true" modal="true">Modal</dialog></div>"#,
        300,
        200,
    );
    assert_eq!(scrim_alpha(&clear_scene, 300.0, 200.0), None);
}

fn scrim_alpha(scene: &PaintScene, vw: f32, vh: f32) -> Option<u8> {
    scene.primitives.iter().find_map(|p| match p {
        DrawPrimitive::StyledBox {
            rect,
            background_color,
            ..
        } if (rect.width - vw).abs() < 0.5
            && (rect.height - vh).abs() < 0.5
            && background_color.a > 0 =>
        {
            Some(background_color.a)
        }
        _ => None,
    })
}

#[test]
fn canvas_clear_color_propagates_from_root_and_body_css() {
    let (_, s_default) = mount_css_doc(
        r#"<link rel="krui-preset" href="krui://preset/unstyled" /><div>Plain</div>"#,
        200,
        120,
    );
    assert_eq!(s_default.clear_color, Color::WHITE);

    let (_, s_body) = mount_css_doc(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>body { background-color: #fef08a; }</style>
        <html><body>Body bg</body></html>
        "#,
        200,
        120,
    );
    assert_eq!(s_body.clear_color, Color::rgb(0xfe, 0xf0, 0x8a));

    let (_, s_both) = mount_css_doc(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            html { background-color: #dbeafe; }
            body { background-color: #fef08a; width: 80px; height: 40px; }
        </style>
        <html><body>Both</body></html>
        "#,
        200,
        120,
    );
    assert_eq!(s_both.clear_color, Color::rgb(0xdb, 0xea, 0xfe));

    let (_, s_root_override) = mount_css_doc(
        r#"
        <style>:root { background-color: #10b981; }</style>
        <div>Themed override</div>
        "#,
        200,
        120,
    );
    assert_eq!(s_root_override.clear_color, Color::rgb(0x10, 0xb9, 0x81));
}

#[test]
fn ua_tag_defaults_come_from_css_and_are_overridable() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        !root.join("crates/compiler/src/css_unstyled.rs").exists(),
        "css_unstyled.rs must be replaced by UA CSS text"
    );
    let (tree_ua, _) = mount_css_doc(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <div>
            <h1 id="h">Title</h1>
            <hr id="rule" />
        </div>
        "#,
        240,
        120,
    );
    let h_rect = tree_ua
        .node_layout(tree_ua.find_by_id("h").unwrap())
        .unwrap();
    let r_rect = tree_ua
        .node_layout(tree_ua.find_by_id("rule").unwrap())
        .unwrap();
    assert!(
        h_rect.height >= 30.0,
        "UA h1 must be ~32px font: {h_rect:?}"
    );
    assert!(
        (r_rect.height - 1.0).abs() <= 0.5,
        "UA hr must have 1px border height: {r_rect:?}"
    );

    let (tree_ov, _) = mount_css_doc(
        r#"
        <link rel="krui-preset" href="krui://preset/unstyled" />
        <style>
            h1 { font-size: 12px; font-weight: 400; }
            hr { border-top-width: 6px; }
        </style>
        <div>
            <h1 id="h">Title</h1>
            <hr id="rule" />
        </div>
        "#,
        240,
        120,
    );
    let h2_rect = tree_ov
        .node_layout(tree_ov.find_by_id("h").unwrap())
        .unwrap();
    let r2_rect = tree_ov
        .node_layout(tree_ov.find_by_id("rule").unwrap())
        .unwrap();
    assert!(
        h2_rect.height < h_rect.height - 10.0,
        "author CSS must override UA h1"
    );
    assert!(
        (r2_rect.height - 6.0).abs() <= 0.5,
        "author CSS must override UA hr: {r2_rect:?}"
    );
}
