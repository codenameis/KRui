//! `component_gallery` 모든 M3 카테고리·대화형 위젯 실입력 회귀 검증 게이트.

use super::GalleryController;
use krui::*;

fn find_id(session: &EngineSession, id: &str) -> NodeId {
    session
        .tree
        .find_by_id(id)
        .unwrap_or_else(|| panic!("missing element `#{id}`"))
}

fn rect_of(session: &EngineSession, id: &str) -> LayoutRect {
    let nid = find_id(session, id);
    session
        .tree
        .node_layout(nid)
        .unwrap_or_else(|| panic!("missing layout for `#{id}`"))
}

fn switch_page(session: &mut EngineSession, ctrl: &GalleryController, page: i32) {
    let vp_id = find_id(session, "m3-doc-viewport");
    let _ = session.tree.set_scroll_offsets(vp_id, 0.0, 0.0);
    ctrl.select_doc_page(page);
    let _ = session.step_frame(1.0 / 120.0);
}

fn screen_point_for(session: &mut EngineSession, id: &str, fx: f32, fy: f32) -> (f32, f32) {
    let vp_id = find_id(session, "m3-doc-viewport");
    let vp = session.tree.node_layout(vp_id).expect("viewport layout");
    let mut r = rect_of(session, id);
    let screen_y = r.y + r.height * fy;
    if screen_y < vp.y + 8.0 || screen_y > vp.y + vp.height - 8.0 {
        let cur_scroll = session.tree.scroll_offset_y(vp_id).unwrap_or(0.0);
        let desired = (screen_y + cur_scroll - (vp.y + vp.height * 0.5)).max(0.0);
        let _ = session.tree.set_scroll_offsets(vp_id, 0.0, desired);
        let _ = session.step_frame(1.0 / 120.0);
        r = rect_of(session, id);
    }
    (r.x + r.width * fx, r.y + r.height * fy)
}

fn click_id(session: &mut EngineSession, id: &str) {
    let (x, y) = screen_point_for(session, id, 0.5, 0.5);
    click_xy(session, x, y, id);
}

fn settle_overlay(session: &mut EngineSession) {
    let _ = session.step_frame(0.45);
}

fn click_xy(session: &mut EngineSession, x: f32, y: f32, label: &str) {
    assert!(
        session.tree.dispatch_pointer_down(x, y),
        "pointer down failed on `{label}` at ({x:.1}, {y:.1})"
    );
    assert!(
        session.tree.dispatch_pointer_up_at(x, y),
        "pointer up failed on `{label}` at ({x:.1}, {y:.1})"
    );
    let _ = session.step_frame(1.0 / 120.0);
}

fn verify_overview_and_app_bars(session: &mut EngineSession, ctrl: &GalleryController) {
    switch_page(session, ctrl, 0);
    click_id(session, "ov-card-fab-menu");
    assert_eq!(ctrl.active_doc_page.get_untracked(), 28);
    click_id(session, "fab-menu-trigger");
    assert!(ctrl.fab_menu_open.get_untracked());
    assert!(ctrl.dismiss_top_overlay() && !ctrl.fab_menu_open.get_untracked());

    switch_page(session, ctrl, 1);
    click_id(session, "appbar-hamburger-btn");
    assert!(ctrl.nav_menu_open.get_untracked());
    assert!(session.tree.find_by_id("appbar-popup-menu").is_some());
    click_id(session, "appbar-menu-item-1");
    assert_eq!(ctrl.nav_menu_label.get_untracked(), "확장 레이아웃");
    assert!(!ctrl.nav_menu_open.get_untracked());
}

fn verify_actions_cards_and_overlays(session: &mut EngineSession, ctrl: &GalleryController) {
    switch_page(session, ctrl, 3);
    click_id(session, "micl-seg-week");
    assert_eq!(ctrl.density_mode.get_untracked(), 1);

    switch_page(session, ctrl, 4);
    let (cx, cy) = screen_point_for(session, "demo-card-filled", 0.5, 0.8);
    click_xy(session, cx, cy, "demo-card-filled");
    assert_eq!(ctrl.selected_list_idx.get_untracked(), 1);
    click_id(session, "micl-card-checkbox");
    assert!(ctrl.micl_card_checked.get_untracked());

    let sum_before = rect_of(session, "micl-details-demo");
    click_id(session, "micl-summary-demo");
    let closed_r = rect_of(session, "micl-details-demo");
    assert!(
        closed_r.height < sum_before.height - 8.0,
        "<details> height must shrink when closed ({} vs {})",
        closed_r.height,
        sum_before.height
    );
    click_id(session, "micl-summary-demo");
    let reopened_r = rect_of(session, "micl-details-demo");
    assert!(
        (reopened_r.height - sum_before.height).abs() < 1.0,
        "<details> height must restore when reopened ({} vs {})",
        reopened_r.height,
        sum_before.height
    );

    switch_page(session, ctrl, 9);
    verify_modal_dialog_overlay(session, ctrl);
}

fn verify_modal_dialog_overlay(session: &mut EngineSession, ctrl: &GalleryController) {
    let rel_open = session.tree.relayout_passes();
    click_id(session, "open-dialog-btn");
    assert!(ctrl.dialog_open.get_untracked());
    assert_eq!(session.tree.relayout_passes() - rel_open, 0);
    assert!(session.tree.active_transition_count() > 0);
    settle_overlay(session);
    let (dx, dy) = screen_point_for(session, "sync-dialog", 0.15, 0.2);
    let _ = session.tree.dispatch_pointer_down(dx, dy);
    let _ = session.tree.dispatch_pointer_up_at(dx, dy);
    let _ = session.step_frame(1.0 / 120.0);
    assert!(ctrl.dialog_open.get_untracked(), "dialog body must shield");
    let rel_close = session.tree.relayout_passes();
    let (bx, by) = screen_point_for(session, "scrim-dialog-backdrop", 0.05, 0.05);
    click_xy(session, bx, by, "scrim-dialog-backdrop");
    assert!(!ctrl.dialog_open.get_untracked(), "scrim backdrop dismiss");
    assert_eq!(session.tree.relayout_passes() - rel_close, 0);
    settle_overlay(session);
}

fn verify_communication_and_tabs(session: &mut EngineSession, ctrl: &GalleryController) {
    switch_page(session, ctrl, 12);
    let p_rect = rect_of(session, "micl-native-progress");
    let (scene_before, _) = session.tree.build_paint_scene(1280, 840);
    let no_prog_text = !scene_before.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::TextGlyphs {
                origin_x,
                origin_y,
                glyphs,
                ..
            } if !glyphs.is_empty()
                && *origin_x >= p_rect.x - 1.0
                && *origin_x <= p_rect.x + p_rect.width + 1.0
                && *origin_y >= p_rect.y - 1.0
                && *origin_y <= p_rect.y + p_rect.height + 1.0
        )
    });
    assert!(no_prog_text, "<progress> must not render literal text");
    click_id(session, "progress-inc-btn");
    assert!((ctrl.progress_px.get_untracked() - 160.0).abs() < 0.01);

    switch_page(session, ctrl, 19);
    click_id(session, "reopen-snackbar-btn");
    assert!(ctrl.snackbar.open.get_untracked());
    click_id(session, "snackbar-action-btn");
    assert_eq!(ctrl.snackbar.action_count.get_untracked(), 1);
    assert!(!ctrl.snackbar.open.get_untracked());

    switch_page(session, ctrl, 21);
    click_id(session, "nav-tab-1");
    assert_eq!(ctrl.active_tab.get_untracked(), 1);
    assert!(session.tree.find_by_id("nav-tab-panel-1").is_some());
}

fn verify_selection_and_text_inputs(session: &mut EngineSession, ctrl: &GalleryController) {
    switch_page(session, ctrl, 15);
    click_id(session, "radio-plan-free-input");
    assert!(ctrl.plan_free.get_untracked() && !ctrl.plan_pro.get_untracked());

    click_id(session, "select-region-control");
    let _ = session.tree.build_paint_scene(1280, 840);
    assert!(session.tree.active_select_popup().is_some());
    let pop_r = session
        .tree
        .active_select_popup_rect()
        .expect("select-region popup rect");
    assert!(
        session
            .tree
            .dispatch_click(pop_r.x + pop_r.width * 0.5, pop_r.y + pop_r.height * 0.5),
        "select-opt-jp click failed"
    );
    let _ = session.step_frame(1.0 / 120.0);
    assert_eq!(ctrl.select_region.get_untracked(), "jp");

    switch_page(session, ctrl, 18);
    let (sx0, sy0) = screen_point_for(session, "slider-master-vol-input", 0.1, 0.5);
    let (sx1, sy1) = screen_point_for(session, "slider-master-vol-input", 0.9, 0.5);
    assert!(session.tree.dispatch_pointer_down(sx0, sy0));
    let _ = session.tree.dispatch_pointer_drag(sx1, sy1);
    let _ = session.tree.dispatch_pointer_up();
    let _ = session.step_frame(1.0 / 120.0);
    assert!(ctrl.slider_pct.get_untracked() >= 85);

    switch_page(session, ctrl, 20);
    click_id(session, "switch-dark-thumb");
    assert!(!ctrl.switch_dark.get_untracked());

    switch_page(session, ctrl, 16);
    ctrl.apply_search("Carousel");
    let _ = session.step_frame(1.0 / 120.0);
    let rows = ctrl.search_results.get_untracked();
    assert_eq!(rows.len(), 1);
    ctrl.open_search_item(rows[0].doc_page);
    let _ = session.step_frame(1.0 / 120.0);
    assert_eq!(ctrl.active_doc_page.get_untracked(), 5);
}

fn verify_navigation_drawer_and_clipboard(session: &mut EngineSession, ctrl: &GalleryController) {
    switch_page(session, ctrl, 37);
    let d0_bg = session
        .tree
        .node_effective_paint(find_id(session, "nav-drawer-item-0"))
        .expect("drawer item 0 paint")
        .background_color;
    let d1_bg = session
        .tree
        .node_effective_paint(find_id(session, "nav-drawer-item-1"))
        .expect("drawer item 1 paint")
        .background_color;
    assert!(
        d0_bg.a > 0 && d1_bg.a == 0,
        "drawer active pill must differ from unselected"
    );

    click_id(session, "nav-drawer-item-1");
    let _ = session.step_frame(0.25);
    assert_eq!(ctrl.drawer_idx.get_untracked(), 1);
    assert!(session.tree.find_by_id("nav-drawer-detail-1").is_some());
    let d0_after = session
        .tree
        .node_effective_paint(find_id(session, "nav-drawer-item-0"))
        .expect("drawer item 0 paint after")
        .background_color;
    let d1_after = session
        .tree
        .node_effective_paint(find_id(session, "nav-drawer-item-1"))
        .expect("drawer item 1 paint after")
        .background_color;
    assert!(
        d0_after.a == 0 && d1_after.a > 0,
        "drawer selection highlight must move to clicked item"
    );

    verify_modal_drawer_and_clipboard(session, ctrl);
}

fn verify_modal_drawer_and_clipboard(session: &mut EngineSession, ctrl: &GalleryController) {
    let rel_drawer = session.tree.relayout_passes();
    click_id(session, "open-modal-drawer-btn");
    assert!(ctrl.modal_drawer_open.get_untracked());
    assert_eq!(session.tree.relayout_passes() - rel_drawer, 0);
    assert!(session.tree.active_transition_count() > 0);
    settle_overlay(session);
    click_id(session, "modal-nav-drawer-item-2");
    assert_eq!(ctrl.drawer_idx.get_untracked(), 2);
    assert!(!ctrl.modal_drawer_open.get_untracked());
    settle_overlay(session);

    click_id(session, "open-modal-drawer-btn");
    assert!(ctrl.modal_drawer_open.get_untracked());
    settle_overlay(session);
    let (mx, my) = screen_point_for(session, "scrim-modal-drawer-backdrop", 0.85, 0.5);
    click_xy(session, mx, my, "scrim-modal-drawer-backdrop");
    assert!(!ctrl.modal_drawer_open.get_untracked());
    settle_overlay(session);

    switch_page(session, ctrl, 17);
    let prev_clip = get_clipboard_text();
    let rel_sheet = session.tree.relayout_passes();
    click_id(session, "open-bottom-sheet-btn");
    assert!(ctrl.bottom_sheet_open.get_untracked());
    assert_eq!(session.tree.relayout_passes() - rel_sheet, 0);
    assert!(session.tree.active_transition_count() > 0);
    settle_overlay(session);
    click_id(session, "copy-theme-tokens-btn");
    assert!(!ctrl.bottom_sheet_open.get_untracked());
    settle_overlay(session);
    assert!(get_clipboard_text().contains("\"data_theme\""));
    if !prev_clip.is_empty() {
        set_clipboard_text(&prev_clip);
    }
}

fn verify_wave4_specs_and_subroutes(session: &mut EngineSession, ctrl: &GalleryController) {
    switch_page(session, ctrl, 3);
    click_id(session, "doc-tab-1");
    assert_eq!(ctrl.active_doc_tab.get_untracked(), 1);
    assert!(session.tree.find_by_id("m3-spec-matrix-table").is_some());
    click_id(session, "spec-filter-1");
    assert_eq!(ctrl.w4.spec_filter_group.get_untracked(), 1);
    assert!(!ctrl.w4.spec_rows.get_untracked().is_empty());

    let vp_id = find_id(session, "m3-doc-viewport");
    let _ = session.tree.set_scroll_offsets(vp_id, 0.0, 0.0);
    ctrl.select_section(2);
    let _ = session.step_frame(1.0 / 120.0);
    click_id(session, "styles-sub-5");
    assert_eq!(ctrl.w4.styles_subroute.get_untracked(), 5);
    assert!(session.tree.find_by_id("w4-showcase-conic").is_some());
    assert!(session.tree.find_by_id("w4-showcase-multistop").is_some());
    assert!(session.tree.find_by_id("w4-showcase-clip").is_some());
    assert!(session.tree.find_by_id("w4-showcase-3d").is_some());

    let _ = session.tree.set_scroll_offsets(vp_id, 0.0, 0.0);
    ctrl.select_section(5);
    let _ = session.step_frame(1.0 / 120.0);
    click_id(session, "foundations-sub-2");
    assert_eq!(ctrl.w4.foundations_subroute.get_untracked(), 2);
}

fn verify_develop_canvas_studio(session: &mut EngineSession, ctrl: &GalleryController) {
    let vp_id = find_id(session, "m3-doc-viewport");
    let _ = session.tree.set_scroll_offsets(vp_id, 0.0, 0.0);
    ctrl.select_section(4);
    let _ = session.step_frame(1.0 / 120.0);
    click_id(session, "dev-sub-1");
    let studio = &ctrl.w4.studio;
    assert_eq!(studio.develop_subroute.get_untracked(), 1);
    let before_count = studio.canvas.shapes_snapshot().len();
    click_id(session, "canvas-add-rect");
    let shapes_after = studio.canvas.shapes_snapshot();
    assert_eq!(shapes_after.len(), before_count + 1);
    let added = shapes_after.last().cloned().expect("added rect");
    let _ = screen_point_for(session, "studio-2d-canvas", 0.5, 0.5);
    let canvas_rect = rect_of(session, "studio-2d-canvas");
    let sx = canvas_rect.x + added.x + added.w * 0.4;
    let sy = canvas_rect.y + added.y + added.h * 0.4;
    assert!(session.tree.dispatch_pointer_down(sx, sy));
    assert!(session
        .tree
        .dispatch_pointer_move_with_mods(sx + 60.0, sy + 35.0, Mods::default()));
    assert!(session.tree.dispatch_pointer_up_at(sx + 60.0, sy + 35.0));
    let _ = session.step_frame(1.0 / 120.0);
    let moved = studio
        .canvas
        .shapes_snapshot()
        .into_iter()
        .find(|s| s.id == added.id)
        .unwrap();
    assert!((moved.x - added.x - 60.0).abs() < 2.0 && (moved.y - added.y - 35.0).abs() < 2.0);
    click_id(session, "canvas-color-2");
    assert_eq!(
        studio
            .canvas
            .shapes_snapshot()
            .iter()
            .find(|s| s.id == added.id)
            .unwrap()
            .color_idx,
        2
    );
    click_id(session, "canvas-del");
    assert_eq!(studio.canvas.shapes_snapshot().len(), before_count);
}

fn verify_develop_3d_viewport(session: &mut EngineSession, ctrl: &GalleryController) {
    let vp_id = find_id(session, "m3-doc-viewport");
    let _ = session.tree.set_scroll_offsets(vp_id, 0.0, 0.0);
    click_id(session, "dev-sub-2");
    let studio = &ctrl.w4.studio;
    assert_eq!(studio.develop_subroute.get_untracked(), 2);
    assert!(session.tree.find_by_id("dev-3d-viewport-slot").is_some());
    click_id(session, "vp3d-mode-bevy");
    assert_eq!(studio.engine_mode.get_untracked(), 1);
    click_id(session, "vp3d-mode-wgpu");
    assert_eq!(studio.engine_mode.get_untracked(), 0);
    let yaw0 = studio.orbit_yaw.get_untracked();
    let dist0 = studio.orbit_dist.get_untracked();
    let (hx, hy) = screen_point_for(session, "dev-3d-viewport-hit", 0.5, 0.5);
    assert!(session.tree.dispatch_pointer_down(hx, hy));
    assert!(session
        .tree
        .dispatch_pointer_move_with_mods(hx + 80.0, hy + 20.0, Mods::default()));
    assert!(session.tree.dispatch_pointer_up_at(hx + 80.0, hy + 20.0));
    let _ = session
        .tree
        .dispatch_wheel(hx, hy, 0.0, -120.0, Mods::default());
    let _ = session.step_frame(1.0 / 120.0);
    assert!((studio.orbit_yaw.get_untracked() - yaw0).abs() > 10.0);
    assert!((studio.orbit_dist.get_untracked() - dist0).abs() > 0.1);
    click_id(session, "vp3d-reset-cam");
    assert!((studio.orbit_yaw.get_untracked() - 35.0).abs() < 0.1);
}

pub(crate) fn verify_gallery_interactions(
    session: &mut EngineSession,
    controller: &GalleryController,
) {
    verify_overview_and_app_bars(session, controller);
    verify_actions_cards_and_overlays(session, controller);
    verify_communication_and_tabs(session, controller);
    verify_selection_and_text_inputs(session, controller);
    verify_navigation_drawer_and_clipboard(session, controller);
    verify_wave4_specs_and_subroutes(session, controller);
    verify_develop_canvas_studio(session, controller);
    verify_develop_3d_viewport(session, controller);
    click_id(session, "rail-tray-btn");
    assert!(controller.tray_hide_requested.get_untracked());
    assert_eq!(
        super::desktop_tray::take_gallery_window_command(controller, session),
        WindowCommand::HideToTray
    );
    assert!(!controller.tray_hide_requested.get_untracked());
    assert_eq!(
        super::desktop_tray::take_gallery_window_command(controller, session),
        WindowCommand::None
    );
    controller.clear_search();
    switch_page(session, controller, 0);
    println!("[component_gallery] Gate 5 (Interactive controls, Switch thumb, Slider, Radio, Progress, Drawer, Clipboard, Scrim, Search, Wave 4 Specs/Subroutes, Develop 2D/3D Studio): all verified");
}
