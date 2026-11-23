//! 오프스크린 PNG 스크린샷 덤프 도구 (`examples/component_gallery --screenshots <dir>`).

use krui::*;
use std::path::Path;

/// 단일 프레임 상태를 16프레임(~266ms) 진행시켜 전환 애니메이션·레이아웃 안정화 후 PNG 파일로 저장합니다.
fn save_frame_png(session: &mut EngineSession, dir: &Path, file_name: &str) {
    for _ in 0..16 {
        session.step_frame(1.0 / 60.0);
    }
    let (w, h) = session
        .renderer
        .framebuffer_dimensions()
        .expect("ForceTinySkia framebuffer dimensions must exist");
    let rgba = session
        .renderer
        .framebuffer_rgba()
        .expect("ForceTinySkia framebuffer rgba must exist");
    let out_path = dir.join(file_name);
    image::save_buffer(&out_path, rgba, w, h, image::ColorType::Rgba8)
        .expect("screenshot PNG save failed");
}

fn switch_theme(session: &mut EngineSession, controller: &super::GalleryController, idx: i32) {
    controller.select_theme(idx);
    let _ = session.step_frame(1.0 / 60.0);
}

fn dump_material_views(
    session: &mut EngineSession,
    controller: &super::GalleryController,
    dir: &Path,
) {
    switch_theme(session, controller, 0);
    for (sec, name) in [
        (0, "home_material.png"),
        (1, "get_started_material.png"),
        (2, "styles_material.png"),
    ] {
        controller.select_section(sec);
        save_frame_png(session, dir, name);
    }
    for cat in 0..=5 {
        controller.select_category(cat);
        save_frame_png(session, dir, &format!("cat{cat}_material.png"));
    }
}

fn dump_all_doc_pages_and_tabs(
    session: &mut EngineSession,
    controller: &super::GalleryController,
    dir: &Path,
) {
    switch_theme(session, controller, 0);
    for page in 0..=40 {
        controller.select_doc_page(page);
        save_frame_png(session, dir, &format!("doc_page_{page:02}.png"));
    }
    controller.select_doc_page(4);
    for tab in 1..=3 {
        controller.set_doc_tab(tab);
        save_frame_png(session, dir, &format!("doc_tab_{tab}.png"));
    }
    controller.set_doc_tab(0);
}

fn dump_material_overlays(
    session: &mut EngineSession,
    controller: &super::GalleryController,
    dir: &Path,
) {
    switch_theme(session, controller, 0);
    controller.select_category(2);
    controller.set_dialog_open(true);
    save_frame_png(session, dir, "overlay_dialog_material.png");
    controller.set_dialog_open(false);

    controller.set_bottom_sheet_open(true);
    save_frame_png(session, dir, "overlay_bottom_sheet_material.png");
    controller.set_bottom_sheet_open(false);

    controller.set_side_sheet_open(true);
    save_frame_png(session, dir, "overlay_side_sheet_material.png");
    controller.set_side_sheet_open(false);

    controller.select_section(0);
    controller.open_search_overlay();
    save_frame_png(session, dir, "overlay_search_material.png");
    controller.close_search_overlay();
}

fn dump_tailwind_shots(
    session: &mut EngineSession,
    controller: &super::GalleryController,
    dir: &Path,
) {
    switch_theme(session, controller, 1);
    controller.select_section(0);
    save_frame_png(session, dir, "home_tailwind.png");
    controller.select_section(2);
    save_frame_png(session, dir, "styles_tailwind.png");
    for cat in [0, 2, 4] {
        controller.select_category(cat);
        save_frame_png(session, dir, &format!("cat{cat}_tailwind.png"));
    }
}

/// 초기 카즈 문서 화면 1프레임을 PNG 파일(`--snapshot <path>`)로 저장합니다.
pub(crate) fn dump_single_snapshot(out_path: &Path) {
    if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(parent);
    }
    let (mut session, _) = super::build_gallery_session(RenderBackendPreference::ForceTinySkia);
    let dir = out_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = out_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("snapshot.png");
    save_frame_png(&mut session, dir, file_name);
}

/// 모든 주요 섹션·카테고리·순수 CSS 테마·오버레이 상태를 PNG로 저장해 시각적 검증을 수행합니다.
pub(crate) fn dump_gallery_screenshots(dir: &Path) {
    if let Err(err) = std::fs::create_dir_all(dir) {
        panic!(
            "failed to create screenshot directory {}: {err}",
            dir.display()
        );
    }
    let (mut session, controller) =
        super::build_gallery_session(RenderBackendPreference::ForceTinySkia);
    save_frame_png(&mut session, dir, "default_cards_overview_light.png");
    controller.toggle_dark_mode();
    session
        .tree
        .set_color_scheme_preference(controller.current_color_scheme_pref());
    save_frame_png(&mut session, dir, "cards_overview_dark.png");
    controller.toggle_dark_mode();
    session
        .tree
        .set_color_scheme_preference(controller.current_color_scheme_pref());
    dump_material_views(&mut session, &controller, dir);
    dump_all_doc_pages_and_tabs(&mut session, &controller, dir);
    dump_material_overlays(&mut session, &controller, dir);
    dump_tailwind_shots(&mut session, &controller, dir);
    println!(
        "[component_gallery] dumped gallery screenshots to {}",
        dir.display()
    );
}
