//! `component_gallery` 및 `krui-showcase` 공용 실행기 (헤드리스 120Hz 검증·스크린샷·창 실행).

use super::controller::GalleryController;
use super::{geometry_gates, interaction_gates, screenshots};
use krui::*;

pub(crate) const WINDOW_WIDTH: u32 = 1280;
pub(crate) const WINDOW_HEIGHT: u32 = 840;
/// 스피너 회전각 측정 시 변환 행렬을 계산하는 기준 박스 한 변(px).
const SPINNER_PROBE_SIZE_PX: f32 = 40.0;

struct GalleryWindowHooks {
    controller: GalleryController,
    applied_theme_idx: i32,
    applied_dark_mode: bool,
    applied_scheme_mode: i32,
    applied_anim_paused: bool,
    hide_on_close: bool,
    host_3d: Option<crate::viewport3d::Gallery3dHost>,
    last_3d_mode: Option<i32>,
}

impl GalleryWindowHooks {
    fn new(controller: GalleryController, hide_on_close: bool) -> Self {
        Self {
            applied_theme_idx: controller.theme_index.get_untracked(),
            applied_dark_mode: controller.dark_mode.get_untracked(),
            applied_scheme_mode: controller.color_scheme_mode.get_untracked(),
            applied_anim_paused: controller.animations_paused.get_untracked(),
            hide_on_close,
            controller,
            host_3d: None,
            last_3d_mode: None,
        }
    }

    fn studio_3d_active(
        &self,
    ) -> Option<(crate::controller::viewport3d_ctrl::StudioController, i32)> {
        let studio = &self.controller.w4.studio;
        if self.controller.active_section.get_untracked() != 4
            || studio.develop_subroute.get_untracked() != 2
        {
            return None;
        }
        Some((
            studio.clone(),
            studio.engine_mode.get_untracked().clamp(0, 1),
        ))
    }

    fn tick_studio_3d(&mut self, session: &mut EngineSession) -> bool {
        let Some((studio, mode)) = self.studio_3d_active() else {
            return false;
        };
        let Some(vp_node) = session.tree.find_by_id("dev-3d-viewport-slot") else {
            return false;
        };
        if self.host_3d.is_none() {
            if let Some(gpu) = session.gpu_context() {
                self.host_3d = Some(crate::viewport3d::Gallery3dHost::new(gpu));
            }
        }
        let mut redraw = false;
        if let Some(host) = self.host_3d.as_mut() {
            if host.ensure_bound(&mut session.tree, vp_node, mode)
                || self.last_3d_mode != Some(mode)
            {
                self.last_3d_mode = Some(mode);
                let summary = host.mode_summary(mode);
                session
                    .reactive_session
                    .clone()
                    .enter(|| studio.ecs_telemetry.set(summary));
                redraw = true;
            }
        }
        session
            .reactive_session
            .clone()
            .enter(|| studio.tick_auto_spin(1.0 / 60.0))
            || redraw
    }

    fn sync_theme_and_scheme(&mut self, session: &mut EngineSession) -> bool {
        let target_idx = self.controller.theme_index.get_untracked();
        let target_dark = self.controller.dark_mode.get_untracked();
        let target_scheme = self.controller.color_scheme_mode.get_untracked();
        if target_idx == self.applied_theme_idx
            && target_dark == self.applied_dark_mode
            && target_scheme == self.applied_scheme_mode
        {
            return false;
        }
        let scheme_changed =
            target_dark != self.applied_dark_mode || target_scheme != self.applied_scheme_mode;
        self.applied_theme_idx = target_idx;
        self.applied_dark_mode = target_dark;
        self.applied_scheme_mode = target_scheme;
        if scheme_changed {
            session
                .tree
                .set_color_scheme_preference(self.controller.current_color_scheme_pref());
        }
        true
    }
}

impl WindowEventHooks for GalleryWindowHooks {
    fn on_tick(&mut self, session: &mut EngineSession) -> Option<Option<String>> {
        let paused = self.controller.animations_paused.get_untracked();
        if paused != self.applied_anim_paused {
            self.applied_anim_paused = paused;
            session.tree.set_animations_paused(paused);
        }
        let mut redraw = self.sync_theme_and_scheme(session);
        redraw |= self.tick_studio_3d(session);
        redraw.then_some(None)
    }

    fn on_before_present(&mut self, session: &mut EngineSession, _dt_secs: f32) {
        let Some((studio, mode)) = self.studio_3d_active() else {
            return;
        };
        let Some(vp_node) = session.tree.find_by_id("dev-3d-viewport-slot") else {
            return;
        };
        let Some(host) = self.host_3d.as_mut() else {
            return;
        };
        let _ = host.sync_and_render(
            &mut session.tree,
            vp_node,
            session.scale_factor as f32,
            crate::viewport3d::Viewport3dFrameInput {
                engine_mode: mode,
                yaw_deg: studio.orbit_yaw.get_untracked(),
                pitch_deg: studio.orbit_pitch.get_untracked(),
                distance: studio.orbit_dist.get_untracked(),
                model_scale: studio.model_scale.get_untracked(),
            },
        );
    }

    fn on_key_down(
        &mut self,
        session: &mut EngineSession,
        key_info: &KeyInfo,
        _mods: Mods,
    ) -> Option<Option<String>> {
        let key_name = key_name_for_keydown(key_info);
        if key_name == "Escape" {
            if session.tree.active_select_popup().is_some() {
                return None;
            }
            let ctrl = self.controller.clone();
            let dismissed = session
                .reactive_session
                .clone()
                .enter(|| ctrl.dismiss_top_overlay());
            return dismissed.then_some(None);
        }
        let idx = match key_name.as_str() {
            "F5" => 0,
            "F6" => 1,
            _ => return None,
        };
        let ctrl = self.controller.clone();
        session
            .reactive_session
            .clone()
            .enter(|| ctrl.select_theme(idx));
        self.applied_theme_idx = idx;
        Some(None)
    }

    fn on_tray_event(&mut self, session: &mut EngineSession, ev: &TrayEvent) -> WindowCommand {
        super::desktop_tray::handle_gallery_tray_event(
            &self.controller,
            &mut self.hide_on_close,
            session,
            ev,
        )
    }

    fn take_window_command(&mut self, session: &mut EngineSession) -> WindowCommand {
        super::desktop_tray::take_gallery_window_command(&self.controller, session)
    }
}

pub(crate) fn build_gallery_session(
    preference: RenderBackendPreference,
) -> (EngineSession, GalleryController) {
    let mut session = EngineSession::new(WINDOW_WIDTH, WINDOW_HEIGHT, preference);
    let controller = GalleryController::new();
    let ctrl_for_view = controller.clone();
    session.tree.mount_with(move || {
        let controller = ctrl_for_view;
        include_krui!("examples/component_gallery/views/component_gallery.krui")
    });
    session
        .tree
        .set_color_scheme_preference(controller.current_color_scheme_pref());
    (session, controller)
}

fn verify_doc_pages_and_tabs(session: &mut EngineSession, controller: &GalleryController) {
    for page in 0..=40 {
        controller.select_doc_page(page);
        let scene = session.step_frame(1.0 / 120.0);
        assert!(
            scene.render_stats.draw_primitives_submitted > 0,
            "문서 페이지 {page} 렌더 장면이 비어 있습니다"
        );
    }
    for tab in 0..=3 {
        controller.select_doc_page(4);
        controller.set_doc_tab(tab);
        let scene = session.step_frame(1.0 / 120.0);
        assert!(
            scene.render_stats.draw_primitives_submitted > 0,
            "문서 탭 {tab} 렌더 장면이 비어 있습니다"
        );
    }
}

fn btn_bg_rgb(tree: &ViewTree, id: NodeId) -> (u8, u8, u8) {
    let c = tree.node_effective_paint(id).unwrap().background_color;
    (c.r, c.g, c.b)
}

fn verify_pure_css_theme_switching(session: &mut EngineSession, controller: &GalleryController) {
    controller.select_doc_page(3);
    controller.set_color_scheme_mode(1);
    session
        .tree
        .set_color_scheme_preference(controller.current_color_scheme_pref());
    controller.select_theme(0);
    let _ = session.step_frame(1.0 / 120.0);
    let _ = session.step_frame(0.5);

    let btn_id = session
        .tree
        .find_by_id("btn-demo-filled")
        .expect("btn-demo-filled must mount on doc_page 3");
    assert_eq!(
        btn_bg_rgb(&session.tree, btn_id),
        (0x67, 0x50, 0xa4),
        "Material Light ACCENT must come purely from themes/material.css"
    );
    assert!(
        session
            .tree
            .node_effective_paint(btn_id)
            .unwrap()
            .border_radius
            >= 900.0,
        "Material Button must have pill border-radius (999px)"
    );

    let relayout_before = session.tree.relayout_passes();
    controller.select_theme(1);
    let _ = session.step_frame(1.0 / 120.0);
    let _ = session.step_frame(0.5);
    assert_eq!(
        session.tree.relayout_passes() - relayout_before,
        0,
        "Material -> Tailwind 순수 CSS 전환 시 Taffy relayout은 0회여야 합니다"
    );
    assert_eq!(
        btn_bg_rgb(&session.tree, btn_id),
        (0x4f, 0x46, 0xe5),
        "Tailwind Light ACCENT (#4f46e5) must activate purely via [data-theme]"
    );
    let tw_radius = session
        .tree
        .node_effective_paint(btn_id)
        .unwrap()
        .border_radius;
    assert!(
        (tw_radius - 6.0).abs() < 0.01,
        "Tailwind Button must switch to rounded-md (6px), got {tw_radius}"
    );
}

fn verify_dark_scheme_preserves_active_theme(
    session: &mut EngineSession,
    controller: &GalleryController,
) {
    let btn_id = session
        .tree
        .find_by_id("btn-demo-filled")
        .expect("btn-demo-filled must mount on doc_page 3");
    controller.set_color_scheme_mode(2);
    session
        .tree
        .set_color_scheme_preference(controller.current_color_scheme_pref());
    let _ = session.step_frame(1.0 / 120.0);
    let _ = session.step_frame(0.5);
    assert_eq!(
        btn_bg_rgb(&session.tree, btn_id),
        (0x81, 0x8c, 0xf8),
        "Dark mode toggle must preserve active Tailwind Dark token (#818cf8)"
    );

    let relayout_back = session.tree.relayout_passes();
    controller.select_theme(0);
    let _ = session.step_frame(1.0 / 120.0);
    let _ = session.step_frame(0.5);
    assert_eq!(
        session.tree.relayout_passes() - relayout_back,
        0,
        "Tailwind -> Material 복귀 전환 시 Taffy relayout은 0회여야 합니다"
    );
    assert_eq!(
        btn_bg_rgb(&session.tree, btn_id),
        (0xd0, 0xbc, 0xff),
        "Material Dark ACCENT (#d0bcff) must resolve in Dark mode"
    );
    controller.set_color_scheme_mode(0);
    session
        .tree
        .set_color_scheme_preference(controller.current_color_scheme_pref());
}

fn verify_categories_and_themes(session: &mut EngineSession, controller: &GalleryController) {
    for sec in 0..=6 {
        controller.select_section(sec);
        let scene = session.step_frame(1.0 / 120.0);
        assert!(
            scene.render_stats.draw_primitives_submitted > 0,
            "섹션 {sec} 렌더 장면이 비어 있습니다"
        );
    }
    for cat in 0..=5 {
        controller.select_category(cat);
        let scene = session.step_frame(1.0 / 120.0);
        assert!(
            scene.render_stats.draw_primitives_submitted > 0,
            "카테고리 {cat} 렌더 장면이 비어 있습니다"
        );
    }
    verify_doc_pages_and_tabs(session, controller);
    controller.open_search_overlay();
    let search_scene = session.step_frame(1.0 / 120.0);
    assert!(search_scene.render_stats.draw_primitives_submitted > 0);
    controller.close_search_overlay();
    verify_pure_css_theme_switching(session, controller);
    verify_dark_scheme_preserves_active_theme(session, controller);
}

fn verify_spinner_pause_behavior(session: &mut EngineSession, controller: &GalleryController) {
    controller.select_doc_page(12);
    let _ = session.step_frame(1.0 / 120.0);
    let spinner_id = session
        .tree
        .find_by_id("circular-progress-spinner")
        .expect("page 12 pure-CSS @keyframes spinner must mount");
    assert!(session.tree.active_keyframe_count() >= 1);
    let deg_before = spinner_rotation_deg(session, spinner_id);
    for _ in 0..10 {
        let _ = session.step_frame(1.0 / 120.0);
    }
    let deg_after = spinner_rotation_deg(session, spinner_id);
    assert!(
        (deg_after - deg_before).abs() > 0.01,
        "Loading & progress 페이지 활성 중 pure-CSS 스피너가 회전해야 합니다"
    );
    controller.toggle_animations_paused();
    session.tree.set_animations_paused(true);
    assert_eq!(session.tree.active_keyframe_count(), 0);
    let paused_deg = spinner_rotation_deg(session, spinner_id);
    for _ in 0..10 {
        let _ = session.step_frame(1.0 / 120.0);
    }
    let still_deg = spinner_rotation_deg(session, spinner_id);
    assert!(
        (still_deg - paused_deg).abs() <= f32::EPSILON,
        "모션 일시정지 상태에서 pure-CSS 스피너 회전이 멈춰야 합니다"
    );
    controller.toggle_animations_paused();
    session.tree.set_animations_paused(false);
}

/// 스피너의 합성 변환(`transform` 목록 포함)에서 회전각(도)을 읽습니다.
fn spinner_rotation_deg(session: &EngineSession, id: NodeId) -> f32 {
    let paint = session
        .tree
        .node_effective_paint(id)
        .expect("spinner paint");
    let m = paint.effective_affine_for_rect(0.0, 0.0, SPINNER_PROBE_SIZE_PX, SPINNER_PROBE_SIZE_PX);
    m.b.atan2(m.a).to_degrees()
}

fn verify_responsive_pwa_breakpoints(session: &mut EngineSession, controller: &GalleryController) {
    controller.select_doc_page(4);
    session.resize(412, 915);
    let _ = session.step_frame(1.0 / 120.0);
    let rail = session.tree.find_by_id("main-site-rail").expect("rail");
    let drawer = session
        .tree
        .find_by_id("m3-components-drawer")
        .expect("drawer");
    let bottom = session
        .tree
        .find_by_id("compact-bottom-nav")
        .expect("compact bottom nav");
    assert!(session.tree.node_layout(rail).unwrap().width < 0.5);
    assert!(session.tree.node_layout(bottom).unwrap().height > 20.0);

    session.resize(720, 900);
    let _ = session.step_frame(1.0 / 120.0);
    assert!(session.tree.node_layout(rail).unwrap().width > 20.0);
    assert!(session.tree.node_layout(drawer).unwrap().width < 0.5);
    assert!(session.tree.node_layout(bottom).unwrap().height < 0.5);

    session.resize(1024, 840);
    let _ = session.step_frame(1.0 / 120.0);
    assert!(session.tree.node_layout(rail).unwrap().width > 20.0);
    assert!(session.tree.node_layout(drawer).unwrap().width > 100.0);

    session.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
    let _ = session.step_frame(1.0 / 120.0);
    assert!(session.tree.node_layout(drawer).unwrap().width > 150.0);

    let card_id = session
        .tree
        .find_by_id("micl-selectable-card")
        .expect("micl selectable card");
    controller.set_color_scheme_mode(1);
    session
        .tree
        .set_color_scheme_preference(controller.current_color_scheme_pref());
    let _ = session.step_frame(1.0 / 120.0);
    let _ = session.step_frame(0.25);
    let light_bg = session
        .tree
        .node_effective_paint(card_id)
        .unwrap()
        .background_color;
    controller.set_color_scheme_mode(2);
    session
        .tree
        .set_color_scheme_preference(controller.current_color_scheme_pref());
    let _ = session.step_frame(1.0 / 120.0);
    let _ = session.step_frame(0.25);
    let dark_bg = session
        .tree
        .node_effective_paint(card_id)
        .unwrap()
        .background_color;
    assert_ne!(
        light_bg, dark_bg,
        "light-dark() card background must update on ColorSchemePreference"
    );
    controller.set_color_scheme_mode(0);
    session
        .tree
        .set_color_scheme_preference(controller.current_color_scheme_pref());
}

fn verify_carousel_and_inputs(session: &mut EngineSession, controller: &GalleryController) {
    controller.select_doc_page(5);
    let _ = session.step_frame(1.0 / 120.0);
    let slide_relayout_before = session.tree.relayout_passes();
    for i in 1..=30 {
        controller.carousel.offset_px.set(-(i as f32) * 7.0);
        let _ = session.step_frame(1.0 / 120.0);
    }
    assert_eq!(
        session.tree.relayout_passes() - slide_relayout_before,
        0,
        "Carousel translateX 슬라이드 중 Taffy relayout은 0회여야 합니다"
    );
    let snap_relayout_before = session.tree.relayout_passes();
    controller.carousel.next();
    let _ = session.step_frame(1.0 / 120.0);
    let _ = session.step_frame(0.5);
    assert_eq!(
        session.tree.relayout_passes() - snap_relayout_before,
        1,
        "Carousel 인디케이터 텍스트 갱신 시 정확히 1회 relayout 되어야 합니다"
    );
    controller.date_picker.next_month();
    controller.time_picker.inc_hour();
    controller.apply_search("DatePicker");
    for _ in 0..30 {
        let _ = session.step_frame(1.0 / 120.0);
    }
}

fn run_headless_verification() {
    let (mut session, controller) = build_gallery_session(RenderBackendPreference::Auto);
    assert_eq!(
        controller.active_doc_page.get_untracked(),
        0,
        "초기 시작 문서는 Components overview(0)여야 합니다"
    );
    assert!(
        !controller.snackbar.open.get_untracked(),
        "초기 로드 시 Snackbar는 닫혀 있어야 합니다"
    );
    let _ = session.step_frame(1.0 / 120.0);
    verify_categories_and_themes(&mut session, &controller);
    verify_spinner_pause_behavior(&mut session, &controller);

    controller.select_doc_page(12);
    let _ = session.step_frame(1.0 / 120.0);
    let spin_relayout_before = session.tree.relayout_passes();
    for _ in 0..60 {
        let _ = session.step_frame(1.0 / 120.0);
    }
    assert_eq!(
        session.tree.relayout_passes() - spin_relayout_before,
        0,
        "pure-CSS CircularProgress 60프레임 회전 중 Taffy relayout은 0회여야 합니다"
    );

    verify_responsive_pwa_breakpoints(&mut session, &controller);
    verify_carousel_and_inputs(&mut session, &controller);
    geometry_gates::verify_gallery_geometry_gates(&mut session, &controller);
    interaction_gates::verify_gallery_interactions(&mut session, &controller);
    println!(
        "[component_gallery] Headless 120-Frame Verified | Theme: {} | Nodes: {} | Catalog: {}",
        controller.theme_name.get_untracked(),
        session.tree.node_count(),
        controller.search_results.get_untracked().len()
    );
}

pub(crate) fn run_cli() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--snapshot") {
        if let Some(p) = args.get(i + 1) {
            screenshots::dump_single_snapshot(std::path::Path::new(p));
        }
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--screenshots") {
        let dir = args.get(i + 1).map_or("screenshots", String::as_str);
        return screenshots::dump_gallery_screenshots(std::path::Path::new(dir));
    }
    if args.iter().any(|a| a == "--headless") {
        return run_headless_verification();
    }
    let (session, ctrl) = build_gallery_session(RenderBackendPreference::Auto);
    let (opts, hide) = super::desktop_tray::build_gallery_window_options(&args, &ctrl);
    super::desktop_tray::run_gallery_window(session, GalleryWindowHooks::new(ctrl, hide), opts);
}
