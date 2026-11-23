#![forbid(unsafe_code)]

use krui::__private::core::{DynSignal, ScopeRegistry};
use krui::*;

fn assert_approx(actual: f32, expected: f32, tol: f32, ctx: &str) {
    assert!(
        (actual - expected).abs() <= tol,
        "{ctx}: actual={actual:.2}, expected={expected:.2} (diff={:.2}, tol={tol})",
        (actual - expected).abs()
    );
}

fn assert_centered_y(box_rect: LayoutRect, item_rect: LayoutRect, ctx: &str) {
    let expected_center_y = box_rect.y + box_rect.height * 0.5;
    let actual_center_y = item_rect.y + item_rect.height * 0.5;
    assert_approx(actual_center_y, expected_center_y, 0.5, ctx);
}

fn assert_centered_x(box_rect: LayoutRect, item_rect: LayoutRect, ctx: &str) {
    let expected_center_x = box_rect.x + box_rect.width * 0.5;
    let actual_center_x = item_rect.x + item_rect.width * 0.5;
    assert_approx(actual_center_x, expected_center_x, 0.5, ctx);
}

const MATRIX_KRUI_SRC: &str = r#"
<link rel="krui-preset" href="krui://preset/unstyled" />
<style>
    .stage { display: flex; flex-direction: column; gap: 4px; width: 480px; }
    .row-box {
        display: flex; flex-direction: row;
        align-items: center; justify-content: center;
        width: 100px; height: 60px; flex-shrink: 0;
    }
    .col-box {
        display: flex; flex-direction: column;
        align-items: center; justify-content: center;
        width: 120px; height: 80px; flex-shrink: 0;
    }
    .grid-box {
        display: grid; grid-template-columns: 1fr; grid-template-rows: 1fr;
        align-items: center; justify-items: center;
        width: 100px; height: 60px;
    }
    .ib { display: inline-block; height: 20px; padding: 0px 6px; }
    .blk { display: block; height: 20px; padding: 0px 6px; }
    .iflx { display: inline-flex; align-items: center; height: 22px; padding: 0px 6px; }
    .igrd { display: inline-grid; align-items: center; height: 22px; padding: 0px 6px; }
    .self-start { align-self: flex-start; height: 20px; }
    .self-end { align-self: flex-end; height: 20px; }
    .self-center { align-self: center; height: 20px; }
    .contents-wrap { display: contents; }
    .rel-host { position: relative; width: 100px; height: 60px; }
    .abs-span { position: absolute; top: 10px; left: 12px; width: 40px; height: 18px; }
</style>
<div class="stage">
    <div id="r-inl-box" class="row-box"><span id="r-inl">span</span></div>
    <div id="r-ib-box" class="row-box"><span id="r-ib" class="ib">x</span></div>
    <div id="r-blk-box" class="row-box"><span id="r-blk" class="blk">x</span></div>
    <div id="r-iflx-box" class="row-box"><span id="r-iflx" class="iflx"><span>fx</span></span></div>
    <div id="r-igrd-box" class="row-box"><span id="r-igrd" class="igrd"><span>gd</span></span></div>
    <div id="r-ss-box" class="row-box"><span id="r-ss" class="self-start">s</span></div>
    <div id="r-se-box" class="row-box"><span id="r-se" class="self-end">e</span></div>
    <div id="r-sc-box" class="row-box"><span id="r-sc" class="self-center">c</span></div>

    <div id="c-inl-box" class="col-box"><span id="c-inl">span</span></div>
    <div id="c-ib-box" class="col-box"><span id="c-ib" class="ib">x</span></div>
    <div id="c-blk-box" class="col-box"><span id="c-blk" class="blk">x</span></div>
    <div id="c-iflx-box" class="col-box"><span id="c-iflx" class="iflx"><span>fx</span></span></div>
    <div id="c-ss-box" class="col-box"><span id="c-ss" class="self-start">s</span></div>
    <div id="c-se-box" class="col-box"><span id="c-se" class="self-end">e</span></div>

    <div id="g-inl-box" class="grid-box"><span id="g-inl">grid</span></div>
    <div id="g-ib-box" class="grid-box"><span id="g-ib" class="ib">g</span></div>
    <div id="g-iflx-box" class="grid-box"><span id="g-iflx" class="iflx"><span>gf</span></span></div>

    <div id="cts-box" class="row-box"><div class="contents-wrap"><span id="cts-inl">cts</span></div></div>
    <div id="abs-host" class="rel-host"><span id="abs-inl" class="abs-span">abs</span></div>
</div>
"#;

fn mount_macro_matrix() -> ViewTree {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                .stage { display: flex; flex-direction: column; gap: 4px; width: 480px; }
                .row-box {
                    display: flex; flex-direction: row;
                    align-items: center; justify-content: center;
                    width: 100px; height: 60px; flex-shrink: 0;
                }
                .col-box {
                    display: flex; flex-direction: column;
                    align-items: center; justify-content: center;
                    width: 120px; height: 80px; flex-shrink: 0;
                }
                .grid-box {
                    display: grid; grid-template-columns: 1fr; grid-template-rows: 1fr;
                    align-items: center; justify-items: center;
                    width: 100px; height: 60px;
                }
                .ib { display: inline-block; height: 20px; padding: 0px 6px; }
                .blk { display: block; height: 20px; padding: 0px 6px; }
                .iflx { display: inline-flex; align-items: center; height: 22px; padding: 0px 6px; }
                .igrd { display: inline-grid; align-items: center; height: 22px; padding: 0px 6px; }
                .self-start { align-self: flex-start; height: 20px; }
                .self-end { align-self: flex-end; height: 20px; }
                .self-center { align-self: center; height: 20px; }
                .contents-wrap { display: contents; }
                .rel-host { position: relative; width: 100px; height: 60px; }
                .abs-span { position: absolute; top: 10px; left: 12px; width: 40px; height: 18px; }
            </style>
            <div class="stage">
                <div id="r-inl-box" class="row-box"><span id="r-inl">span</span></div>
                <div id="r-ib-box" class="row-box"><span id="r-ib" class="ib">x</span></div>
                <div id="r-blk-box" class="row-box"><span id="r-blk" class="blk">x</span></div>
                <div id="r-iflx-box" class="row-box"><span id="r-iflx" class="iflx"><span>fx</span></span></div>
                <div id="r-igrd-box" class="row-box"><span id="r-igrd" class="igrd"><span>gd</span></span></div>
                <div id="r-ss-box" class="row-box"><span id="r-ss" class="self-start">s</span></div>
                <div id="r-se-box" class="row-box"><span id="r-se" class="self-end">e</span></div>
                <div id="r-sc-box" class="row-box"><span id="r-sc" class="self-center">c</span></div>

                <div id="c-inl-box" class="col-box"><span id="c-inl">span</span></div>
                <div id="c-ib-box" class="col-box"><span id="c-ib" class="ib">x</span></div>
                <div id="c-blk-box" class="col-box"><span id="c-blk" class="blk">x</span></div>
                <div id="c-iflx-box" class="col-box"><span id="c-iflx" class="iflx"><span>fx</span></span></div>
                <div id="c-ss-box" class="col-box"><span id="c-ss" class="self-start">s</span></div>
                <div id="c-se-box" class="col-box"><span id="c-se" class="self-end">e</span></div>

                <div id="g-inl-box" class="grid-box"><span id="g-inl">grid</span></div>
                <div id="g-ib-box" class="grid-box"><span id="g-ib" class="ib">g</span></div>
                <div id="g-iflx-box" class="grid-box"><span id="g-iflx" class="iflx"><span>gf</span></span></div>

                <div id="cts-box" class="row-box"><div class="contents-wrap"><span id="cts-inl">cts</span></div></div>
                <div id="abs-host" class="rel-host"><span id="abs-inl" class="abs-span">abs</span></div>
            </div>
            "#
        )
    });
    let mut renderer = TinySkiaRenderer::new(600, 1400, GpuResourceLimits::default());
    let (scene, _) = tree.build_paint_scene(600, 1400);
    renderer.render_scene(&scene);
    tree
}

fn rect_of(tree: &ViewTree, id: &str) -> LayoutRect {
    let nid = tree
        .find_by_id(id)
        .unwrap_or_else(|| panic!("missing #{id}"));
    tree.node_layout(nid)
        .unwrap_or_else(|| panic!("missing layout for #{id}"))
}

fn verify_matrix_geometry(tree: &ViewTree) {
    for (box_id, item_id) in [
        ("r-inl-box", "r-inl"),
        ("r-ib-box", "r-ib"),
        ("r-blk-box", "r-blk"),
        ("r-iflx-box", "r-iflx"),
        ("r-igrd-box", "r-igrd"),
        ("r-sc-box", "r-sc"),
        ("cts-box", "cts-inl"),
    ] {
        let b = rect_of(tree, box_id);
        let it = rect_of(tree, item_id);
        assert_centered_y(b, it, item_id);
        assert_centered_x(b, it, item_id);
    }
    let rss_b = rect_of(tree, "r-ss-box");
    assert_approx(rect_of(tree, "r-ss").y, rss_b.y, 0.5, "r-ss top");
    let rse_b = rect_of(tree, "r-se-box");
    let rse = rect_of(tree, "r-se");
    assert_approx(rse.y + rse.height, rse_b.y + rse_b.height, 0.5, "r-se end");

    for (box_id, item_id) in [
        ("c-inl-box", "c-inl"),
        ("c-ib-box", "c-ib"),
        ("c-blk-box", "c-blk"),
        ("c-iflx-box", "c-iflx"),
    ] {
        let b = rect_of(tree, box_id);
        let it = rect_of(tree, item_id);
        assert_centered_x(b, it, item_id);
        assert_centered_y(b, it, item_id);
    }
    let css_b = rect_of(tree, "c-ss-box");
    assert_approx(rect_of(tree, "c-ss").x, css_b.x, 0.5, "c-ss start");
    let cse_b = rect_of(tree, "c-se-box");
    let cse = rect_of(tree, "c-se");
    assert_approx(cse.x + cse.width, cse_b.x + cse_b.width, 0.5, "c-se end");

    for (box_id, item_id) in [
        ("g-inl-box", "g-inl"),
        ("g-ib-box", "g-ib"),
        ("g-iflx-box", "g-iflx"),
    ] {
        let b = rect_of(tree, box_id);
        let it = rect_of(tree, item_id);
        assert_centered_x(b, it, item_id);
        assert_centered_y(b, it, item_id);
    }

    let abs_host = rect_of(tree, "abs-host");
    let abs_inl = rect_of(tree, "abs-inl");
    assert_approx(abs_inl.x, abs_host.x + 12.0, 0.5, "abs-inl x");
    assert_approx(abs_inl.y, abs_host.y + 10.0, 0.5, "abs-inl y");
    assert_approx(abs_inl.width, 40.0, 0.5, "abs-inl width");
    assert_approx(abs_inl.height, 18.0, 0.5, "abs-inl height");
}

#[test]
fn flex_and_grid_item_blockification_matrix_matches_spec_and_preview() {
    let macro_tree = mount_macro_matrix();
    verify_matrix_geometry(&macro_tree);

    let doc = krui_compiler::compile_krui_source(MATRIX_KRUI_SRC).expect("preview compile");
    let mut preview_tree = ViewTree::new();
    let _ = krui_preview::mount_compiled_document(&mut preview_tree, &doc);
    let _ = preview_tree.build_paint_scene(600, 1400);
    verify_matrix_geometry(&preview_tree);

    for id in [
        "r-inl", "r-ib", "r-blk", "r-iflx", "r-igrd", "c-inl", "c-ib", "g-inl", "g-ib", "cts-inl",
        "abs-inl",
    ] {
        let mr = rect_of(&macro_tree, id);
        let pr = rect_of(&preview_tree, id);
        assert_approx(mr.x, pr.x, 0.01, &format!("{id} macro vs preview x"));
        assert_approx(mr.y, pr.y, 0.01, &format!("{id} macro vs preview y"));
        assert_approx(
            mr.width,
            pr.width,
            0.01,
            &format!("{id} macro vs preview w"),
        );
        assert_approx(
            mr.height,
            pr.height,
            0.01,
            &format!("{id} macro vs preview h"),
        );
    }
}

#[test]
fn runtime_class_patch_blockifies_inline_child_when_container_becomes_flex() {
    let mode = Signal::new(String::from("as-block"));
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                .host { width: 120px; height: 64px; }
                .as-block { display: block; }
                .as-flex { display: flex; align-items: center; justify-content: center; }
                .chip { display: inline-block; height: 20px; padding: 0px 8px; }
            </style>
            <div id="host" class="host {mode}"><span id="chip" class="chip">tag</span></div>
            "#
        )
    });
    let _ = tree.build_paint_scene(300, 200);
    assert_approx(rect_of(&tree, "chip").y, 0.0, 0.5, "block mode top");

    mode.set("as-flex".to_string());
    let _ = tree.build_paint_scene(300, 200);
    assert_centered_y(
        rect_of(&tree, "host"),
        rect_of(&tree, "chip"),
        "flex mode y",
    );
    assert_centered_x(
        rect_of(&tree, "host"),
        rect_of(&tree, "chip"),
        "flex mode x",
    );
}

const PSEUDO_KRUI_SRC: &str = r#"
    <link rel="krui-preset" href="krui://preset/unstyled" />
    <style>
        .stage { display: flex; gap: 12px; padding: 10px; }
        .btn { position: relative; width: 80px; height: 40px; background-color: #1e293b; }
        .btn::before {
            content: "";
            position: absolute;
            inset: 0px;
            background-color: #ef4444;
        }
        .suppressed::before { content: none; }
        .normal-off::before { content: normal; }
        .grp > button::before {
            content: "";
            position: absolute;
            inset: 0px;
            background-color: #22c55e;
        }
    </style>
    <div class="stage">
        <button id="drawn" class="btn {dyn_cls}">A</button>
        <button id="none-btn" class="btn suppressed">B</button>
        <button id="normal-btn" class="btn normal-off">C</button>
        <div class="grp">
            <if test="{show_grp}">
                <button id="grp-btn" class="btn">D</button>
            </if>
        </div>
    </div>
"#;

fn count_box_color_near(scene: &PaintScene, r: LayoutRect, want: Color) -> usize {
    scene
        .primitives
        .iter()
        .filter(|p| {
            matches!(
                p,
                DrawPrimitive::StyledBox { rect, background_color, .. }
                    if (rect.x - r.x).abs() < 1.0
                        && (rect.y - r.y).abs() < 1.0
                        && *background_color == want
            )
        })
        .count()
}

#[test]
fn pseudo_before_content_empty_string_paints_and_none_suppresses_without_warnings() {
    let doc = krui_compiler::compile_krui_source(PSEUDO_KRUI_SRC).expect("compile pseudo test");
    let content_warnings: Vec<_> = doc
        .warnings
        .iter()
        .filter(|w| w.property == "content" || w.message.contains("content"))
        .collect();
    assert!(
        content_warnings.is_empty(),
        "expected zero content warnings, got: {content_warnings:#?}"
    );

    let dyn_cls = Signal::new(String::from("extra"));
    let show_grp = Signal::new(true);
    let mut reg = ScopeRegistry::new();
    reg.insert_signal("dyn_cls", DynSignal::from_signal(dyn_cls));
    reg.insert_signal("show_grp", DynSignal::from_signal(show_grp));
    let reg: std::rc::Rc<dyn krui::__private::core::Registry> = std::rc::Rc::new(reg);

    let mut tree = ViewTree::new();
    let _ = krui_preview::mount_compiled_document_with_registry(&mut tree, &doc, reg);
    let (scene, _) = tree.build_paint_scene(400, 120);

    let red = Color::parse_css("#ef4444").expect("red");
    let green = Color::parse_css("#22c55e").expect("green");
    assert_eq!(
        count_box_color_near(&scene, rect_of(&tree, "drawn"), red),
        1
    );
    assert_eq!(
        count_box_color_near(&scene, rect_of(&tree, "none-btn"), red),
        0
    );
    assert_eq!(
        count_box_color_near(&scene, rect_of(&tree, "normal-btn"), red),
        0
    );
    assert_eq!(
        count_box_color_near(&scene, rect_of(&tree, "grp-btn"), green),
        1
    );
}

#[test]
fn gallery_stylesheet_emits_zero_unsupported_content_warnings() {
    let gallery_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/component_gallery/views/component_gallery.krui");
    let gallery_src = std::fs::read_to_string(&gallery_path).expect("read gallery");
    let doc = krui_compiler::compile_krui_source_from_path(&gallery_path, &gallery_src)
        .expect("compile gallery");
    let content_warnings: Vec<_> = doc
        .warnings
        .iter()
        .filter(|w| w.property == "content" || w.message.contains("`content`"))
        .collect();
    assert!(
        content_warnings.is_empty(),
        "gallery must not drop `content` with warning: {content_warnings:#?}"
    );
}
