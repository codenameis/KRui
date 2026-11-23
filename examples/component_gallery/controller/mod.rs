//! Material Design 3 6개 카테고리 · 47개 기본 컴포넌트 갤러리 컨트롤러.

pub mod handlers;
pub mod rows;
pub mod sections;
pub mod theme_state;

use handlers::{
    action_name_for_code, density_name_for_code, fab_action_name, nav_menu_name, split_mode_name,
    theme_name_for_index,
};
use krui::*;
pub use rows::SearchCatalogRow;
pub use sections::{
    default_doc_page_for_category, doc_page_meta, section_banner_for_index,
    OnboardingGuideController,
};

const INIT_BANNER: &str = "Material 3 기준 · 6개 카테고리 47개 기본 컴포넌트 준비 완료";
const INIT_SNACK_MSG: &str = "기본 스낵바 알림이 표시되었습니다.";
const INIT_PROGRESS_LABEL: &str = "진행률 64% (140 / 220px)";

/// 4개 디자인 시스템과 6개 M3 컴포넌트 카테고리를 통합 제어하는 MVC 컨트롤러.
#[controller]
#[derive(Clone)]
pub struct GalleryController {
    /// 활성 사이트 섹션 (`0=Home, 1=Get started, 2=Styles, 3=Components, 4=Develop, 5=Foundations, 6=Blog`).
    pub active_section: Signal<i32>,
    /// 상단 앱바용 현재 섹션 제목.
    pub section_title: Computed<String>,
    /// 활성 M3 카테고리 (`0..=5`).
    pub active_category: Signal<i32>,
    /// 활성 `m3.material.io` 드로어 문서 페이지 (`0..=24`, 기본 `4=Cards`).
    pub active_doc_page: Signal<i32>,
    /// 활성 문서 상단 세그먼트 탭 (`0=Overview, 1=Specs, 2=Guidelines, 3=Accessibility`).
    pub active_doc_tab: Signal<i32>,
    /// 현재 문서 페이지 제목 (`Cards` 등).
    pub doc_title: Computed<String>,
    /// 현재 문서 페이지 요약 설명.
    pub doc_summary: Computed<String>,
    /// 현재 문서 페이지 `Specs` 탭 상세 설명.
    pub doc_spec_note: Computed<String>,
    /// 현재 문서 페이지 `Guidelines` 탭 상세 설명.
    pub doc_guideline_note: Computed<String>,
    /// 현재 문서 페이지 `Accessibility` 탭 상세 설명.
    pub doc_a11y_note: Computed<String>,
    /// 다크 색채 스킴 활성화 여부 (`false=#FEF7FF Light`, `true=#141218 Dark`).
    pub dark_mode: Signal<bool>,
    /// 색채 스킴 선호 모드 (`0=System, 1=Light, 2=Dark`).
    pub color_scheme_mode: Signal<i32>,
    /// 상단/레일 색채 스킴 토글 버튼 표시 레이블 (`Auto` / `Light` / `Dark`).
    pub color_scheme_label: Computed<String>,
    /// M3 모션·전환 일시정지 여부.
    pub animations_paused: Signal<bool>,
    /// 활성 테마 인덱스 (`0=Material 3, 1=Tailwind v4`).
    pub theme_index: Signal<i32>,
    /// 현재 활성 순수 CSS `data-theme` 슬러그 (`"material"` 또는 `"tailwind"`).
    pub theme_slug: Computed<String>,
    /// 현재 활성 테마 표시 이름.
    pub theme_name: Signal<String>,
    /// 현재 활성 테마 Shape/FX 요약 스펙.
    pub theme_shape_spec: Computed<String>,
    /// 상단 상태 배너 메시지.
    pub status_banner: Signal<String>,
    /// `Get started` 온보딩 체크리스트 컨트롤러.
    pub guide: OnboardingGuideController,
    /// 전역 `SearchView` 모달 오버레이 열림 여부.
    pub search_overlay_open: Signal<bool>,

    /// 내장 달력 날짜 선택기 컨트롤러.
    pub date_picker: DatePickerController,
    /// 내장 시계 시각 선택기 컨트롤러.
    pub time_picker: TimePickerController,
    /// 내장 수평 캐러셀 컨트롤러 (`translateX` 0-relayout).
    pub carousel: CarouselController,
    /// 내장 스낵바 알림 컨트롤러.
    pub snackbar: SnackbarController,

    /// Actions 버튼 클릭 누적 횟수.
    pub action_count: Signal<i32>,
    /// IconButton 즐겨찾기 선택 여부.
    pub fav_selected: Signal<bool>,
    /// IconButton 북마크 선택 여부.
    pub bookmark_selected: Signal<bool>,
    /// SegmentedButton 밀도 인덱스 (`0..=2`).
    pub density_mode: Signal<i32>,
    /// SplitButton 드롭다운 열림 여부.
    pub split_menu_open: Signal<bool>,
    /// SplitButton 선택된 저장 모드.
    pub split_mode_label: Signal<String>,
    /// FabMenu 스피드 다이얼 열림 여부.
    pub fab_menu_open: Signal<bool>,

    /// Communication 배지 카운트.
    pub unread_count: Signal<i32>,
    /// Communication 배지 문자열 (`0`일 때 빈 문자열).
    pub unread_text: Computed<String>,
    /// LinearProgress 채움 폭 (`20.0..=220.0`).
    pub progress_px: Signal<f32>,
    /// LinearProgress 백분율 레이블.
    pub progress_label: Signal<String>,
    /// CircularProgress 회전 애니메이션 활성 여부.
    pub spinner_running: Signal<bool>,
    /// Rich Tooltip 열림 여부.
    pub rich_tooltip_open: Signal<bool>,

    /// ListItem 선택 인덱스 (`0..=2`).
    pub selected_list_idx: Signal<i32>,
    /// Dialog 열림 여부.
    pub dialog_open: Signal<bool>,
    /// BottomSheet 열림 여부.
    pub bottom_sheet_open: Signal<bool>,
    /// SideSheet 열림 여부.
    pub side_sheet_open: Signal<bool>,
    /// MICL `<dialog>` 쇼케이스 열림 여부.
    pub micl_dialog_open: Signal<bool>,
    /// MICL `:has(input:checked)` 선택 카드 체크 상태.
    pub micl_card_checked: Signal<bool>,
    /// MICL `:focus-within`/`:placeholder-shown` 플로팅 라벨 입력 텍스트.
    pub micl_field_text: Signal<String>,

    /// NavigationBar 활성 인덱스 (`0..=2`).
    pub bottom_nav_idx: Signal<i32>,
    /// NavigationDrawer 활성 인덱스 (`0..=2`).
    pub drawer_idx: Signal<i32>,
    /// Tabs 활성 인덱스 (`0..=2`).
    pub active_tab: Signal<i32>,
    /// Menu 열림 여부.
    pub nav_menu_open: Signal<bool>,
    /// Menu 선택 항목 레이블.
    pub nav_menu_label: Signal<String>,

    /// 자동 저장 체크박스 상태.
    pub chk_auto_save: Signal<bool>,
    /// 익명 통계 체크박스 상태.
    pub chk_analytics: Signal<bool>,
    /// 플랜 라디오: Standard.
    pub plan_free: Signal<bool>,
    /// 플랜 라디오: Pro.
    pub plan_pro: Signal<bool>,
    /// 플랜 라디오: Enterprise.
    pub plan_ent: Signal<bool>,
    /// 다크 모드 스위치 상태.
    pub switch_dark: Signal<bool>,
    /// 촉각 피드백 스위치 상태.
    pub switch_haptic: Signal<bool>,
    /// 볼륨 슬라이더 값 (`0..=100`).
    pub slider_pct: Signal<i32>,
    /// 슬라이더 백분율 파생 문자열.
    pub slider_label: Computed<String>,
    /// 접근성 필터 칩 선택 여부.
    pub chip_filter_a11y: Signal<bool>,
    /// IME 필터 칩 선택 여부.
    pub chip_filter_ime: Signal<bool>,
    /// Input 칩 표시 여부 (`✕`로 제거 가능).
    pub chip_input_visible: Signal<bool>,
    /// SelectField 선택 리전 코드.
    pub select_region: Signal<String>,
    /// DatePicker 확정 결과 요약 문자열.
    pub picker_summary: Signal<String>,

    /// Filled/Outlined TextField 바인딩 이름 (한국어 IME 테스트 가능).
    pub user_name_text: Signal<String>,
    /// 비밀번호 TextField 바인딩 문자열.
    pub password_text: Signal<String>,
    /// 이름 비어있음 검증 오류 파생 문자열.
    pub name_error: Computed<String>,
    /// 이름 글자 수 파생 카운터.
    pub name_counter: Computed<String>,
    /// SearchBar 검색어 문자열.
    pub search_query: Signal<String>,
    /// SearchView 결과 패널 열림 여부.
    pub search_open: Signal<bool>,
    /// SearchView 필터링된 컴포넌트 목록.
    pub search_results: Signal<Vec<SearchCatalogRow>>,
    /// SearchBar `<input oninput="...">`용 `InputCommand`.
    pub on_search_input_cmd: InputCommand,
}

#[controller]
impl GalleryController {
    /// 좌측 `NavigationRail`로 사이트 최상위 섹션(`0..=6`)을 전환합니다.
    pub fn select_section(&self, sec: i32) {
        let s = sec.clamp(0, 6);
        let cat = self.active_category.get_untracked();
        self.active_section.set(s);
        self.status_banner.set(section_banner_for_index(s, cat));
    }

    /// Components 섹션으로 진입해 활성 M3 카테고리(`0..=5`)를 전환합니다.
    pub fn select_category(&self, idx: i32) {
        let c = idx.clamp(0, 5);
        let doc = default_doc_page_for_category(c);
        let this = self.clone();
        batch(move || {
            this.active_section.set(3);
            this.active_category.set(c);
            this.active_doc_page.set(doc);
            this.active_doc_tab.set(0);
            this.status_banner.set(section_banner_for_index(3, c));
        });
    }

    /// 좌측 드로어 문서 페이지(`0..=40`)를 선택해 해당 카테고리와 문서를 동기화합니다.
    pub fn select_doc_page(&self, page: i32) {
        let p = page.clamp(0, 40);
        let (cat, title, _) = doc_page_meta(p);
        let this = self.clone();
        batch(move || {
            this.active_section.set(3);
            this.active_category.set(cat);
            this.active_doc_page.set(p);
            this.active_doc_tab.set(0);
            this.status_banner.set(format!("Components / {title}"));
        });
    }

    /// 문서 상단 피일 탭(`0=Overview, 1=Specs, 2=Guidelines, 3=Accessibility`)을 전환합니다.
    pub fn set_doc_tab(&self, tab: i32) {
        self.active_doc_tab.set(tab.clamp(0, 3));
    }

    /// Material 3 Light(`#FEF7FF`) / Dark(`#141218`) 색채 스킴을 토글합니다.
    pub fn toggle_dark_mode(&self) {
        self.apply_toggle_dark_mode();
    }

    /// `ColorSchemePreference`(`Auto -> Light -> Dark`)를 순환 전환합니다.
    pub fn cycle_color_scheme_mode(&self) {
        let next = (self.color_scheme_mode.get_untracked() + 1) % 3;
        self.apply_color_scheme_mode(next);
    }

    /// `ColorSchemePreference` 모드(`0=System, 1=Light, 2=Dark`)를 직접 설정합니다.
    pub fn set_color_scheme_mode(&self, mode: i32) {
        self.apply_color_scheme_mode(mode);
    }

    /// MICL `<dialog>` 쇼케이스 열림 상태를 토글합니다.
    pub fn toggle_micl_dialog(&self) {
        self.micl_dialog_open.update(|v| *v = !*v);
    }

    /// 모션 및 컴포지터 전환 일시정지(Reduced motion)를 토글합니다.
    pub fn toggle_animations_paused(&self) {
        let next = !self.animations_paused.get_untracked();
        let this = self.clone();
        batch(move || {
            this.animations_paused.set(next);
            let msg = if next {
                "모션 일시정지됨 (Reduced motion)"
            } else {
                "모션 재생 중"
            };
            this.status_banner.set(msg.to_string());
        });
    }

    /// 전역 `SearchView` 모달 오버레이를 엽니다.
    pub fn open_search_overlay(&self) {
        self.search_overlay_open.set(true);
    }

    /// 전역 `SearchView` 모달 오버레이를 닫습니다.
    pub fn close_search_overlay(&self) {
        self.search_overlay_open.set(false);
    }

    /// 순수 CSS 디자인 시스템(`0=Material 3, 1=Tailwind v4`) 중 하나를 선택합니다.
    pub fn select_theme(&self, idx: i32) {
        let clamped = idx.clamp(0, 1);
        let name = theme_name_for_index(clamped);
        let this = self.clone();
        batch(move || {
            this.theme_index.set(clamped);
            this.theme_name.set(name.to_string());
            this.status_banner
                .set(format!("순수 CSS 테마 전환: {name}"));
        });
    }

    /// Actions 버튼 또는 FAB 클릭을 기록합니다.
    pub fn record_action(&self, code: i32) {
        let next = self.action_count.get_untracked() + 1;
        self.action_count.set(next);
        self.status_banner.set(format!(
            "{} 실행됨 (누적 {next}회)",
            action_name_for_code(code)
        ));
    }

    /// 즐겨찾기 `IconButton` 선택 상태를 토글합니다.
    pub fn toggle_fav(&self) {
        self.fav_selected.update(|v| *v = !*v);
    }

    /// 북마크 `IconButton` 선택 상태를 토글합니다.
    pub fn toggle_bookmark(&self) {
        self.bookmark_selected.update(|v| *v = !*v);
    }

    /// `SegmentedButton` 밀도 모드(`0..=2`)를 선택합니다.
    pub fn set_density(&self, mode: i32) {
        let m = mode.clamp(0, 2);
        self.density_mode.set(m);
        self.status_banner
            .set(format!("밀도 변경: {}", density_name_for_code(m)));
    }

    /// `SplitButton` 보조 메뉴를 토글합니다.
    pub fn toggle_split_menu(&self) {
        self.split_menu_open.update(|v| *v = !*v);
    }

    /// `SplitButton` 저장 모드를 선택합니다 (`0..=2`).
    pub fn pick_split_mode(&self, idx: i32) {
        let label = split_mode_name(idx);
        let this = self.clone();
        batch(move || {
            this.split_mode_label.set(label.to_string());
            this.split_menu_open.set(false);
            this.status_banner.set(format!("SplitButton 모드: {label}"));
        });
    }

    /// `FabMenu` 스피드 다이얼을 여닫습니다.
    pub fn toggle_fab_menu(&self) {
        self.fab_menu_open.update(|v| *v = !*v);
    }

    /// `FabMenuItem` 항목을 실행하고 메뉴를 닫습니다 (`0..=2`).
    pub fn pick_fab_action(&self, idx: i32) {
        let name = fab_action_name(idx);
        let this = self.clone();
        batch(move || {
            this.fab_menu_open.set(false);
            this.status_banner.set(format!("FAB 메뉴 실행: {name}"));
        });
    }

    /// 배지 알림 카운트를 1 증가시킵니다 (`0..=12` 순환).
    pub fn bump_badge(&self) {
        self.unread_count.update(|n| *n = (*n + 1) % 13);
    }

    /// `LinearProgress` 폭을 ±20px 증감합니다 (`20..=220px`).
    pub fn step_progress(&self, increase: bool) {
        let delta_px = if increase { 20.0 } else { -20.0 };
        let next = (self.progress_px.get_untracked() + delta_px).clamp(20.0, 220.0);
        let pct = ((next / 220.0) * 100.0).round() as i32;
        self.progress_px.set(next);
        self.progress_label
            .set(format!("진행률 {pct}% ({next:.0} / 220px)"));
    }

    /// `CircularProgress` 회전 애니메이션을 일시정지/재개합니다.
    pub fn toggle_spinner(&self) {
        self.spinner_running.update(|v| *v = !*v);
    }

    /// `Snackbar`를 다시 표시합니다.
    pub fn reopen_snackbar(&self) {
        self.snackbar.show(
            "변경 사항이 테마 토큰에 저장되었습니다.".to_string(),
            "확인".to_string(),
        );
    }

    /// 리치 `Tooltip` 표시 상태를 토글합니다.
    pub fn toggle_rich_tooltip(&self) {
        self.rich_tooltip_open.update(|v| *v = !*v);
    }

    /// `ListItem`을 선택합니다.
    pub fn select_list_item(&self, idx: i32) {
        self.selected_list_idx.set(idx);
    }

    /// `Dialog` 열림 상태를 설정합니다.
    pub fn set_dialog_open(&self, open: bool) {
        self.dialog_open.set(open);
    }

    /// `Dialog` 확인 버튼을 처리합니다.
    pub fn confirm_dialog(&self) {
        self.dialog_open.set(false);
        self.status_banner
            .set("Dialog 확인됨 — 설정이 반영되었습니다.".to_string());
    }

    /// `BottomSheet` 열림 상태를 설정합니다.
    pub fn set_bottom_sheet_open(&self, open: bool) {
        self.bottom_sheet_open.set(open);
    }

    /// `SideSheet` 열림 상태를 설정합니다.
    pub fn set_side_sheet_open(&self, open: bool) {
        self.side_sheet_open.set(open);
    }

    /// 하단 `NavigationBarItem`을 선택합니다.
    pub fn set_bottom_nav(&self, idx: i32) {
        self.bottom_nav_idx.set(idx);
    }

    /// `NavigationDrawerItem`을 선택합니다.
    pub fn set_drawer_item(&self, idx: i32) {
        self.drawer_idx.set(idx);
    }

    /// `Tabs` 활성 탭을 변경합니다.
    pub fn set_active_tab(&self, idx: i32) {
        self.active_tab.set(idx);
    }

    /// `Menu` 열림 상태를 토글합니다.
    pub fn toggle_nav_menu(&self) {
        self.nav_menu_open.update(|v| *v = !*v);
    }

    /// `MenuItem`을 선택합니다 (`0..=2`).
    pub fn pick_nav_menu_item(&self, idx: i32) {
        let label = nav_menu_name(idx);
        let this = self.clone();
        batch(move || {
            this.nav_menu_label.set(label.to_string());
            this.nav_menu_open.set(false);
        });
    }

    /// 다크 모드 스위치를 토글합니다.
    pub fn toggle_switch_dark(&self) {
        self.switch_dark.update(|v| *v = !*v);
    }

    /// 햅틱 스위치를 토글합니다.
    pub fn toggle_switch_haptic(&self) {
        self.switch_haptic.update(|v| *v = !*v);
    }

    /// 접근성 필터 칩을 토글합니다.
    pub fn toggle_chip_a11y(&self) {
        self.chip_filter_a11y.update(|v| *v = !*v);
    }

    /// IME 필터 칩을 토글합니다.
    pub fn toggle_chip_ime(&self) {
        self.chip_filter_ime.update(|v| *v = !*v);
    }

    /// 입력 칩을 숨깁니다.
    pub fn remove_input_chip(&self) {
        self.chip_input_visible.set(false);
    }

    /// 입력 칩을 다시 표시합니다.
    pub fn restore_input_chip(&self) {
        self.chip_input_visible.set(true);
    }

    /// `DatePicker` 적용 버튼을 처리합니다.
    pub fn confirm_date_picker(&self) {
        let sel = self.date_picker.selected_label.get_untracked();
        self.picker_summary.set(format!("적용 완료: {sel}"));
        self.status_banner.set(format!("날짜 확정: {sel}"));
    }

    /// `DatePicker` 취소 버튼을 처리합니다.
    pub fn cancel_date_picker(&self) {
        self.picker_summary
            .set("날짜 변경이 취소되었습니다.".to_string());
    }

    /// `SearchView`에서 선택한 컴포넌트 카테고리로 바로 이동합니다.
    pub fn open_search_item(&self, cat_idx: i32) {
        self.search_open.set(false);
        self.search_overlay_open.set(false);
        self.select_category(cat_idx);
    }

    /// `SearchBar` 검색어를 비웁니다.
    pub fn clear_search(&self) {
        self.apply_search("");
    }
}
