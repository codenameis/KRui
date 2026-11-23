//! Regression tests for Unit 3: CSS z-index stacking & hit-test, Switch transform sentinel,
//! Grid `minmax(0, 1fr)` column constraint, and runtime button `:disabled` child `<span>` text color inheritance.

#![forbid(unsafe_code)]

use krui::*;

#[test]
fn z_index_orders_paint_and_hit_testing_above_later_siblings() {
    let front_hits = Signal::new(0u32);
    let back_hits = Signal::new(0u32);
    let (fh, bh) = (front_hits, back_hits);

    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        let front_cmd = Command::new(move || fh.update(|n| *n += 1));
        let back_cmd = Command::new(move || bh.update(|n| *n += 1));
        krui!(
            r#"
            <style>
                #stack { position: relative; width: 200px; height: 120px; }
                #front-btn { position: absolute; top: 10px; left: 10px; width: 100px; height: 60px; background: #112233; z-index: 20; }
                #back-btn { position: absolute; top: 10px; left: 10px; width: 100px; height: 60px; background: #445566; z-index: 1; }
            </style>
            <div id="stack">
                <button id="front-btn" onclick="{front_cmd}">앞</button>
                <button id="back-btn" onclick="{back_cmd}">뒤</button>
            </div>
            "#
        )
    });

    let (scene, _) = tree.build_paint_scene(200, 120);
    let front_pos = scene
        .primitives
        .iter()
        .position(|p| matches!(p, DrawPrimitive::StyledBox { background_color, .. } if *background_color == Color::rgb(0x11, 0x22, 0x33)))
        .expect("front styled box");
    let back_pos = scene
        .primitives
        .iter()
        .position(|p| matches!(p, DrawPrimitive::StyledBox { background_color, .. } if *background_color == Color::rgb(0x44, 0x55, 0x66)))
        .expect("back styled box");
    assert!(
        front_pos > back_pos,
        "higher z-index sibling must paint after lower z-index sibling"
    );

    assert!(tree.dispatch_click(40.0, 35.0));
    assert_eq!(front_hits.get(), 1);
    assert_eq!(back_hits.get(), 0);
}

#[test]
fn switch_dynamic_checked_class_does_not_corrupt_scale_sentinel() {
    let checked = Signal::new(true);
    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        krui!(r#"<div style="width: 320px; height: 80px;"><Switch id="sw" label="알림" checked="{checked}" /></div>"#)
    });
    let (scene, _) = tree.build_paint_scene(320, 80);
    let max_box_dim = scene
        .primitives
        .iter()
        .filter_map(|p| match p {
            DrawPrimitive::StyledBox { rect, .. } => Some(rect.width.max(rect.height)),
            _ => None,
        })
        .fold(0.0_f32, f32::max);
    assert!(
        max_box_dim < 400.0,
        "Switch transform must not leak sentinel scale (-4321.25), got max box dim {max_box_dim}"
    );
}

#[test]
fn grid_1fr_columns_stay_bounded_despite_wide_child_content() {
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <style>
            #grid { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; width: 616px; }
        </style>
        <div id="grid">
            <div id="col1"><p>매우 긴 한국어 설명 문장이 첫 번째 컬럼에 들어가더라도 너비가 폭주하지 않고 균등 300px을 유지해야 합니다 가나다라마바사아자차카타파하</p></div>
            <div id="col2"><p>두 번째 컬럼</p></div>
        </div>
        "#
    ));
    let _ = tree.build_paint_scene(640, 240);
    let r1 = tree
        .node_layout(tree.find_by_id("col1").expect("col1"))
        .expect("r1");
    let r2 = tree
        .node_layout(tree.find_by_id("col2").expect("col2"))
        .expect("r2");
    assert!(
        (r1.width - 300.0).abs() < 2.0 && (r2.width - 300.0).abs() < 2.0,
        "1fr 1fr columns must each stay ~300px, got col1={} col2={}",
        r1.width,
        r2.width
    );
}

#[test]
fn disabled_button_propagates_text_color_to_child_span_at_runtime() {
    let disabled = Signal::new(false);
    let mut tree = ViewTree::new();
    tree.mount_with(move || {
        krui!(
            r#"
            <style>
                #btn { background: #112233; color: #ffffff; width: 160px; height: 44px; }
                #btn:disabled { background: #333333; color: #99aabb; }
            </style>
            <button id="btn" disabled="{disabled}"><span id="lbl">라벨텍스트</span></button>
            "#
        )
    });

    let (scene_en, _) = tree.build_paint_scene(200, 80);
    let enabled_fg = scene_en
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::TextGlyphs { color, glyphs, .. } if !glyphs.is_empty() => Some(*color),
            _ => None,
        })
        .expect("enabled span text");
    assert_eq!(enabled_fg, Color::rgb(0xff, 0xff, 0xff));

    disabled.set(true);
    let (scene_dis, _) = tree.build_paint_scene(200, 80);
    let disabled_fg = scene_dis
        .primitives
        .iter()
        .find_map(|p| match p {
            DrawPrimitive::TextGlyphs { color, glyphs, .. } if !glyphs.is_empty() => Some(*color),
            _ => None,
        })
        .expect("disabled span text");
    assert_eq!(disabled_fg, Color::rgb(0x99, 0xaa, 0xbb));
}
