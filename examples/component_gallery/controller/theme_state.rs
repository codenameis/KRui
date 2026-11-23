//! 테마 프리셋·색채 스킴(`ColorSchemePreference`)·MICL 상태 초기화 및 전환 헬퍼.

use super::handlers::{build_gallery_init_bundle, GalleryInitBundle};
use super::rows::filter_catalog_rows;
use super::{GalleryController, INIT_BANNER, INIT_PROGRESS_LABEL, INIT_SNACK_MSG};
use krui::*;

type SchemePair = (Signal<i32>, Computed<String>);

pub(super) fn build_scheme_signals() -> SchemePair {
    let mode = Signal::new(0);
    let label = Computed::new(move || match mode.get() {
        1 => "Light".to_string(),
        2 => "Dark".to_string(),
        _ => "Auto".to_string(),
    });
    (mode, label)
}

pub(super) fn scheme_mode_name(mode: i32) -> &'static str {
    match mode {
        1 => "Light (고정)",
        2 => "Dark (고정)",
        _ => "System (자동)",
    }
}

fn attach_bundle_search_cmd(mut ctrl: GalleryController) -> GalleryController {
    let cs = ctrl.clone();
    ctrl.on_search_input_cmd = InputCommand::new(move |q| cs.apply_search(&q));
    ctrl
}

fn finish_gallery_init(b: GalleryInitBundle, scheme: SchemePair) -> GalleryController {
    attach_bundle_search_cmd(GalleryController {
        active_section: b.sec,
        section_title: b.s.section_title,
        active_category: b.cat,
        active_doc_page: b.doc,
        active_doc_tab: Signal::new(0),
        doc_title: b.s.doc_title,
        doc_summary: b.s.doc_summary,
        doc_spec_note: b.s.doc_spec_note,
        doc_guideline_note: b.s.doc_guideline_note,
        doc_a11y_note: b.s.doc_a11y_note,
        dark_mode: Signal::new(false),
        color_scheme_mode: scheme.0,
        color_scheme_label: scheme.1,
        animations_paused: Signal::new(false),
        theme_index: b.thm,
        theme_slug: b.s.theme_slug,
        theme_name: Signal::new("Material 3".to_string()),
        theme_shape_spec: b.s.theme_shape_spec,
        status_banner: Signal::new(INIT_BANNER.to_string()),
        guide: super::OnboardingGuideController::new(),
        search_overlay_open: Signal::new(false),
        date_picker: DatePickerController::new(2026, 5, 15),
        time_picker: TimePickerController::new(14, 30, false),
        carousel: CarouselController::new(4, 220.0),
        snackbar: SnackbarController::new(INIT_SNACK_MSG, "실행 취소", false),
        action_count: Signal::new(0),
        fav_selected: Signal::new(true),
        bookmark_selected: Signal::new(false),
        density_mode: Signal::new(0),
        split_menu_open: Signal::new(false),
        split_mode_label: Signal::new("즉시 저장".to_string()),
        fab_menu_open: Signal::new(false),
        unread_count: b.unread_count,
        unread_text: b.c.unread_text,
        progress_px: Signal::new(140.0),
        progress_label: Signal::new(INIT_PROGRESS_LABEL.to_string()),
        spinner_running: Signal::new(true),
        rich_tooltip_open: Signal::new(false),
        selected_list_idx: Signal::new(0),
        dialog_open: Signal::new(false),
        bottom_sheet_open: Signal::new(false),
        side_sheet_open: Signal::new(false),
        micl_dialog_open: Signal::new(false),
        micl_card_checked: Signal::new(false),
        micl_field_text: Signal::new(String::new()),
        bottom_nav_idx: Signal::new(0),
        drawer_idx: Signal::new(0),
        active_tab: Signal::new(0),
        nav_menu_open: Signal::new(false),
        nav_menu_label: Signal::new("기본 보기".to_string()),
        chk_auto_save: Signal::new(true),
        chk_analytics: Signal::new(false),
        plan_free: Signal::new(false),
        plan_pro: Signal::new(true),
        plan_ent: Signal::new(false),
        switch_dark: Signal::new(true),
        switch_haptic: Signal::new(true),
        slider_pct: b.slider_pct,
        slider_label: b.c.slider_label,
        chip_filter_a11y: Signal::new(true),
        chip_filter_ime: Signal::new(true),
        chip_input_visible: Signal::new(true),
        select_region: Signal::new("kr".to_string()),
        picker_summary: Signal::new("선택 대기: 2026-05-15".to_string()),
        user_name_text: b.user_name_text,
        password_text: Signal::new("krui-pass!".to_string()),
        name_error: b.c.name_error,
        name_counter: b.c.name_counter,
        search_query: Signal::new(String::new()),
        search_open: Signal::new(false),
        search_results: Signal::new(filter_catalog_rows("")),
        on_search_input_cmd: InputCommand::new(|_| {}),
    })
}

impl GalleryController {
    /// 기본 상태가 초기화된 `GalleryController`를 생성합니다.
    pub fn new() -> Self {
        finish_gallery_init(build_gallery_init_bundle(), build_scheme_signals())
    }

    /// 현재 선택된 `ColorSchemePreference`를 반환합니다.
    pub fn current_color_scheme_pref(&self) -> ColorSchemePreference {
        match self.color_scheme_mode.get_untracked() {
            1 => ColorSchemePreference::Light,
            2 => ColorSchemePreference::Dark,
            _ => ColorSchemePreference::System,
        }
    }

    pub(super) fn apply_toggle_dark_mode(&self) {
        let next = !self.dark_mode.get_untracked();
        let next_scheme = if next { 2 } else { 1 };
        let this = self.clone();
        batch(move || {
            this.dark_mode.set(next);
            this.color_scheme_mode.set(next_scheme);
            let mode = if next { "Dark" } else { "Light" };
            this.status_banner.set(format!("색채 스킴 전환: {mode}"));
        });
    }

    pub(super) fn apply_color_scheme_mode(&self, mode: i32) {
        let clamped = mode.clamp(0, 2);
        let dark = clamped == 2;
        let label = scheme_mode_name(clamped);
        let this = self.clone();
        batch(move || {
            this.color_scheme_mode.set(clamped);
            this.dark_mode.set(dark);
            this.status_banner
                .set(format!("ColorSchemePreference: {label}"));
        });
    }
}
