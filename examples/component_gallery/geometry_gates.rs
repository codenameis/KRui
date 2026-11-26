//! 헤드리스 기하·실측 레퍼런스 정합 게이트 (`G1(5)`).
//!
//! `session.tree.node_layout`과 `PaintScene` 프리미티브를 직접 읽어 레일·탭·FAB·버튼·칩·모달의
//! 중심 정렬(`<= 1px`), 텍스트 무잘림, 그리고 `reference/m3_core_spec.json` 대비 핵심 치수(`+-2px`)를 검증합니다.

use super::GalleryController;
use krui::*;
use std::fs;
use std::path::Path;

const CENTER_TOL_PX: f32 = 1.0;
const REF_TOL_PX: f32 = 2.0;

struct ReferenceCoreSpec {
    measured_sections: usize,
    total_sections: usize,
    measured_doc_pages: usize,
    total_doc_pages: usize,
    rail_width: f32,
    pill_width: f32,
    pill_height: f32,
    rail_label_font_px: f32,
    rail_label_line_height: f32,
    tabbar_height: f32,
    hero_padding_px: f32,
    title_line_height: f32,
}

fn extract_json_number(haystack: &str, key: &str) -> Option<f32> {
    let pat = format!("\"{key}\":");
    let kpos = haystack.find(&pat)? + pat.len();
    let tail = haystack[kpos..].trim_start();
    let token: String = tail
        .chars()
        .skip_while(|c| *c == '"')
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    token.parse::<f32>().ok()
}

fn load_reference_core_spec() -> ReferenceCoreSpec {
    let spec_path = Path::new("examples/component_gallery/reference/m3_core_spec.json");
    let raw = fs::read_to_string(spec_path).expect("read m3_core_spec.json");

    ReferenceCoreSpec {
        measured_sections: extract_json_number(&raw, "measured_sections")
            .expect("measured_sections") as usize,
        total_sections: extract_json_number(&raw, "total_sections").expect("total_sections")
            as usize,
        measured_doc_pages: extract_json_number(&raw, "measured_doc_pages")
            .expect("measured_doc_pages") as usize,
        total_doc_pages: extract_json_number(&raw, "total_doc_pages").expect("total_doc_pages")
            as usize,
        rail_width: extract_json_number(&raw, "rail_width").expect("rail_width"),
        pill_width: extract_json_number(&raw, "pill_width").expect("pill_width"),
        pill_height: extract_json_number(&raw, "pill_height").expect("pill_height"),
        rail_label_font_px: extract_json_number(&raw, "rail_label_font_px")
            .expect("rail_label_font_px"),
        rail_label_line_height: extract_json_number(&raw, "rail_label_line_height")
            .expect("rail_label_line_height"),
        tabbar_height: extract_json_number(&raw, "tabbar_height").expect("tabbar_height"),
        hero_padding_px: extract_json_number(&raw, "hero_padding_px").expect("hero_padding_px"),
        title_line_height: extract_json_number(&raw, "title_line_height")
            .expect("title_line_height"),
    }
}

fn rect_of(session: &EngineSession, id: &str) -> LayoutRect {
    let nid = session
        .tree
        .find_by_id(id)
        .unwrap_or_else(|| panic!("missing element id `{id}` for geometry gate"));
    session
        .tree
        .node_layout(nid)
        .unwrap_or_else(|| panic!("missing layout rect for `{id}`"))
}

fn center_of(r: LayoutRect) -> (f32, f32) {
    (r.x + 0.5 * r.width, r.y + 0.5 * r.height)
}

fn find_inner_svg_rect(scene: &PaintScene, container: LayoutRect) -> Option<LayoutRect> {
    scene.primitives.iter().find_map(|p| {
        if let DrawPrimitive::CanvasBatch { viewport, .. } = p {
            let (cx, cy) = center_of(*viewport);
            let inside = cx >= container.x - 1.0
                && cx <= container.x + container.width + 1.0
                && cy >= container.y - 1.0
                && cy <= container.y + container.height + 1.0
                && viewport.width <= container.width + 1.0
                && viewport.height <= container.height + 1.0;
            if inside {
                return Some(*viewport);
            }
        }
        None
    })
}

fn verify_rail_alignment(session: &EngineSession) {
    let rail = rect_of(session, "main-site-rail");
    let (rail_cx, _) = center_of(rail);
    let mut max_dx = 0.0_f32;
    for idx in 0..=6 {
        let pill = rect_of(session, &format!("rail-pill-{idx}"));
        let label = rect_of(session, &format!("rail-label-{idx}"));
        let (pill_cx, _) = center_of(pill);
        let (label_cx, _) = center_of(label);
        let dx = (label_cx - pill_cx).abs();
        max_dx = max_dx.max(dx);
        assert!(
            dx <= CENTER_TOL_PX,
            "Rail item {idx}: |label_cx({label_cx:.2}) - pill_cx({pill_cx:.2})| > {CENTER_TOL_PX}px"
        );
        assert!(
            (pill_cx - rail_cx).abs() <= CENTER_TOL_PX,
            "Rail item {idx}: |pill_cx({pill_cx:.2}) - rail_cx({rail_cx:.2})| > {CENTER_TOL_PX}px"
        );
    }
    println!("[component_gallery] Gate 1 (Rail center): max |label_cx - pill_cx| = {max_dx:.2}px");
}

fn verify_doc_tabs_alignment_and_padding(session: &mut EngineSession) {
    let (scene, _) = session.tree.build_paint_scene(1280, 840);
    let mut max_err = 0.0_f32;
    for idx in 0..=3 {
        let tab_id = format!("doc-tab-{idx}");
        let lbl_id = format!("doc-tab-label-{idx}");
        let tab = rect_of(session, &tab_id);
        let lbl = rect_of(session, &lbl_id);
        let svg = find_inner_svg_rect(&scene, tab)
            .unwrap_or_else(|| panic!("{tab_id} missing Material Symbol <svg> CanvasBatch"));
        let (tab_cx, tab_cy) = center_of(tab);
        let (_, svg_cy) = center_of(svg);
        let (_, lbl_cy) = center_of(lbl);
        let group_cx = 0.5 * (svg.x + (lbl.x + lbl.width));
        let err = (svg_cy - tab_cy)
            .abs()
            .max((lbl_cy - tab_cy).abs())
            .max((group_cx - tab_cx).abs());
        max_err = max_err.max(err);
        assert!(
            (svg_cy - tab_cy).abs() <= CENTER_TOL_PX && (lbl_cy - tab_cy).abs() <= CENTER_TOL_PX,
            "{tab_id}: vertical center mismatch icon={svg_cy:.2}, label={lbl_cy:.2}, tab={tab_cy:.2}"
        );
        assert!(
            (group_cx - tab_cx).abs() <= CENTER_TOL_PX,
            "{tab_id}: |group_cx({group_cx:.2}) - tab_cx({tab_cx:.2})| > {CENTER_TOL_PX}px"
        );
        assert!(
            svg.x >= tab.x + 16.0 - 0.5 && lbl.x + lbl.width <= tab.x + tab.width - 16.0 + 0.5,
            "{tab_id} content [{:.1}..{:.1}] overflows tab content box [{:.1}..{:.1}]",
            svg.x,
            lbl.x + lbl.width,
            tab.x + 16.0,
            tab.x + tab.width - 16.0
        );
    }
    println!("[component_gallery] Gate 2 (Doc tabs center): max center delta = {max_err:.2}px");
}

fn assert_svg_centered_in(scene: &PaintScene, session: &EngineSession, id: &str) -> f32 {
    let btn = rect_of(session, id);
    let svg = find_inner_svg_rect(scene, btn)
        .unwrap_or_else(|| panic!("{id} missing inner <svg> CanvasBatch"));
    let (bx, by) = center_of(btn);
    let (sx, sy) = center_of(svg);
    let err = (sx - bx).abs().max((sy - by).abs());
    assert!(
        err <= CENTER_TOL_PX,
        "{id}: icon center ({sx:.2}, {sy:.2}) != container center ({bx:.2}, {by:.2})"
    );
    err
}

fn verify_fab_and_icon_button_centering(
    session: &mut EngineSession,
    controller: &GalleryController,
) {
    controller.select_doc_page(27);
    let _ = session.step_frame(1.0 / 120.0);
    let (scene, _) = session.tree.build_paint_scene(1280, 840);
    let mut max_err = 0.0_f32;
    for id in [
        "global-search-trigger",
        "rail-motion-toggle",
        "rail-dark-toggle",
        "fab-sm-primary",
        "fab-md-secondary",
        "fab-lg-tertiary",
    ] {
        max_err = max_err.max(assert_svg_centered_in(&scene, session, id));
    }
    let lg_fab = rect_of(session, "fab-lg-tertiary");
    assert!(
        (lg_fab.width - 96.0).abs() <= REF_TOL_PX && (lg_fab.height - 96.0).abs() <= REF_TOL_PX,
        "Large FAB must match M3 96dp spec: got {}x{}",
        lg_fab.width,
        lg_fab.height
    );

    controller.select_doc_page(29);
    let _ = session.step_frame(1.0 / 120.0);
    let (ib_scene, _) = session.tree.build_paint_scene(1280, 840);
    for id in [
        "icon-fav-standard",
        "icon-fav-filled",
        "icon-bm-tonal",
        "icon-bm-outlined",
    ] {
        max_err = max_err.max(assert_svg_centered_in(&ib_scene, session, id));
    }
    println!(
        "[component_gallery] Gate 3 (FAB & IconButton center): max center delta = {max_err:.2}px"
    );
}

fn glyph_run_bounds(
    origin_x: f32,
    origin_y: f32,
    glyphs: &[RasterizedGlyph],
) -> (f32, f32, f32, f32) {
    let min_x = glyphs
        .iter()
        .map(|g| origin_x + g.x)
        .fold(f32::INFINITY, f32::min);
    let max_x = glyphs
        .iter()
        .map(|g| origin_x + g.x + g.width as f32)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = glyphs
        .iter()
        .map(|g| origin_y + g.y)
        .fold(f32::INFINITY, f32::min);
    let max_y = glyphs
        .iter()
        .map(|g| origin_y + g.y + g.height as f32)
        .fold(f32::NEG_INFINITY, f32::max);
    (min_x, max_x, min_y, max_y)
}

fn assert_glyphs_inside_box(scene: &PaintScene, id: &str, r: LayoutRect, pad_x: f32) {
    let scale = scene.scale_factor.max(1.0);
    let mut found_text = false;
    for prim in &scene.primitives {
        let DrawPrimitive::TextGlyphs {
            origin_x,
            origin_y,
            glyphs,
            ..
        } = prim
        else {
            continue;
        };
        if glyphs.is_empty()
            || *origin_x < r.x - 1.0
            || *origin_x > r.x + r.width + 1.0
            || *origin_y < r.y - 1.0
            || *origin_y > r.y + r.height + 1.0
        {
            continue;
        }
        found_text = true;
        let (min_gx, max_raw_x, min_gy, max_raw_y) = glyph_run_bounds(*origin_x, *origin_y, glyphs);
        let max_gx = min_gx + (max_raw_x - min_gx) / scale;
        let max_gy = min_gy + (max_raw_y - min_gy) / scale;
        assert!(
            min_gx >= r.x + pad_x - 1.5
                && max_gx <= r.x + r.width - pad_x + 1.5
                && min_gy >= r.y
                && max_gy <= r.y + r.height,
            "{id}: text [{min_gx:.1}..{max_gx:.1}, {min_gy:.1}..{max_gy:.1}] overflows box [{:.1}..{:.1}, {:.1}..{:.1}] (pad_x={pad_x})",
            r.x,
            r.x + r.width,
            r.y,
            r.y + r.height
        );
    }
    assert!(
        found_text,
        "{id}: expected rendered TextGlyphs inside control"
    );
}

fn verify_text_unclipped_across_controls(
    session: &mut EngineSession,
    controller: &GalleryController,
) {
    controller.select_doc_page(3);
    let _ = session.step_frame(1.0 / 120.0);
    let (scene, _) = session.tree.build_paint_scene(1280, 840);
    for (id, pad_x) in [
        ("btn-demo-filled", 12.0),
        ("btn-demo-tonal", 12.0),
        ("btn-demo-outlined", 12.0),
        ("fab-ext-primary", 12.0),
        ("seg-density-0", 8.0),
        ("seg-density-1", 8.0),
        ("seg-density-2", 8.0),
        ("micl-seg-day", 10.0),
        ("micl-seg-week", 10.0),
        ("micl-seg-month", 10.0),
        ("rail-scheme-mode-btn", 6.0),
        ("toc-theme-material", 8.0),
        ("toc-theme-tailwind", 8.0),
    ] {
        assert_glyphs_inside_box(&scene, id, rect_of(session, id), pad_x);
    }

    controller.set_dialog_open(true);
    let _ = session.step_frame(1.0 / 120.0);
    let _ = session.step_frame(0.45);
    let (dlg_scene, _) = session.tree.build_paint_scene(1280, 840);
    assert_glyphs_inside_box(
        &dlg_scene,
        "sync-dialog",
        rect_of(session, "sync-dialog"),
        16.0,
    );
    controller.set_dialog_open(false);
    let _ = session.step_frame(1.0 / 120.0);
    let _ = session.step_frame(0.30);
    verify_before_state_layer_and_category4(session, controller);
    println!(
        "[component_gallery] Gate 4 (Text unclipped + ::before state layer): 14 controls + modal verified"
    );
}

fn verify_before_state_layer_and_category4(
    session: &mut EngineSession,
    controller: &GalleryController,
) {
    let seg = rect_of(session, "micl-seg-day");
    let _ = session
        .tree
        .dispatch_pointer_down(seg.x + 12.0, seg.y + 16.0);
    let (press_scene, relayout) = session.tree.build_paint_scene(1280, 840);
    let _ = session.tree.dispatch_pointer_up();
    let has_before_state_layer = press_scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox {
                rect,
                radial_gradient: Some(_),
                ..
            } if (rect.x - seg.x).abs() <= 2.0 && (rect.y - seg.y).abs() <= 2.0
        )
    });
    assert!(
        !relayout && has_before_state_layer,
        "D10 ::before state layer must render without relayout"
    );
    controller.select_category(4);
    let _ = session.step_frame(1.0 / 120.0);
    assert_eq!(
        controller.active_doc_page.get_untracked(),
        6,
        "Category 4 (Selection) must open first Selection drawer item Checkbox(6), not Switch(20)"
    );
    assert!(
        !controller.section_title.get_untracked().is_empty(),
        "section_title signal must stay populated"
    );
}

fn verify_reference_dimensions(session: &mut EngineSession, controller: &GalleryController) {
    let spec = load_reference_core_spec();
    let unmeasured = (spec.total_sections - spec.measured_sections)
        + (spec.total_doc_pages - spec.measured_doc_pages);
    println!(
        "[component_gallery] Reference Spec Coverage: sections={}/{}, doc_pages={}/{}, unmeasured={}",
        spec.measured_sections,
        spec.total_sections,
        spec.measured_doc_pages,
        spec.total_doc_pages,
        unmeasured
    );
    assert_eq!(unmeasured, 0, "expected 0 unmeasured m3.material.io pages");
    assert!((spec.rail_label_font_px - 12.0).abs() <= REF_TOL_PX);

    controller.select_section(3);
    controller.select_doc_page(4);
    let _ = session.step_frame(1.0 / 120.0);

    let rail = rect_of(session, "main-site-rail");
    let pill = rect_of(session, "rail-pill-3");
    let lbl = rect_of(session, "rail-label-3");
    let header = rect_of(session, "m3-doc-header");
    let title = rect_of(session, "m3-doc-title");
    let tabs = rect_of(session, "m3-doc-tabs");

    let mut max_ref_err = 0.0_f32;
    for (name, actual, expected) in [
        ("rail_width", rail.width, spec.rail_width),
        ("pill_width", pill.width, spec.pill_width),
        ("pill_height", pill.height, spec.pill_height),
        ("rail_label_height", lbl.height, spec.rail_label_line_height),
        ("tabbar_height", tabs.height, spec.tabbar_height),
        (
            "header_padding_top",
            title.y - header.y,
            spec.hero_padding_px,
        ),
        (
            "header_padding_left",
            title.x - header.x,
            spec.hero_padding_px,
        ),
        ("title_typescale_lh", title.height, spec.title_line_height),
    ] {
        let diff = (actual - expected).abs();
        max_ref_err = max_ref_err.max(diff);
        assert!(
            diff <= REF_TOL_PX,
            "Reference dimension `{name}` mismatch: actual={actual:.2}px vs m3.material.io={expected:.2}px (tol +- {REF_TOL_PX}px)"
        );
    }
    println!(
        "[component_gallery] Gate 5 (Reference dimensions): max |actual - m3.material.io| = {max_ref_err:.2}px"
    );
}

/// 모든 갤러리 기하·정렬·레퍼런스 치수 게이트를 실행합니다.
pub(crate) fn verify_gallery_geometry_gates(
    session: &mut EngineSession,
    controller: &GalleryController,
) {
    session.resize(super::WINDOW_WIDTH, super::WINDOW_HEIGHT);
    controller.select_theme(0);
    controller.set_color_scheme_mode(1);
    session
        .tree
        .set_color_scheme_preference(controller.current_color_scheme_pref());
    controller.select_section(3);
    controller.select_doc_page(4);
    controller.set_doc_tab(0);
    let _ = session.step_frame(1.0 / 120.0);

    verify_reference_dimensions(session, controller);
    verify_rail_alignment(session);
    verify_doc_tabs_alignment_and_padding(session);
    verify_fab_and_icon_button_centering(session, controller);
    verify_text_unclipped_across_controls(session, controller);

    controller.set_color_scheme_mode(0);
    session
        .tree
        .set_color_scheme_preference(controller.current_color_scheme_pref());
}
