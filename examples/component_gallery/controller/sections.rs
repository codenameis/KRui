//! `m3.material.io` 사이트 섹션·드로어 페이지 매핑 및 온보딩 모델 (`examples/08_m3_gallery`).

use super::handlers::category_name_for_index;
use krui::*;

/// `(category_idx, doc_title, doc_summary)` 테이블 (`page_id: 0..=40`).
const DOC_PAGES: &[(i32, &str, &str)] = &[
    (
        2,
        "Components overview",
        "Components are interactive building blocks for creating a user interface, organized by purpose.",
    ),
    (
        3,
        "App bars",
        "App bars are placed at the top or bottom of the screen to help people navigate through an app.",
    ),
    (
        1,
        "Badges",
        "Badges show notifications, counts, or status information on navigation items and icons.",
    ),
    (
        0,
        "Buttons",
        "Buttons prompt most actions in a UI across filled, elevated, tonal, outlined, text, icon, and FAB variants.",
    ),
    (
        2,
        "Cards",
        "Cards display content and actions about a single subject across elevated, filled, and outlined variants.",
    ),
    (
        2,
        "Carousel",
        "Carousels contain a collection of items that can be scrolled on and off the screen smoothly.",
    ),
    (
        4,
        "Checkbox",
        "Checkboxes let users select one or more items from a list, or turn an item on or off.",
    ),
    (
        4,
        "Chips",
        "Chips help people enter information, make selections, filter content, or trigger actions.",
    ),
    (
        4,
        "Date & time pickers",
        "Date and time pickers let people select a date, range, or time with leap-year and IME support.",
    ),
    (
        2,
        "Dialogs",
        "Dialogs provide important prompts in a user flow using Top-Layer scrims.",
    ),
    (
        2,
        "Divider",
        "Dividers are thin lines that group content in lists and containers.",
    ),
    (
        2,
        "Lists",
        "Lists are continuous, vertical indexes of text and images.",
    ),
    (
        1,
        "Loading & progress",
        "Loading and progress indicators show the status of a process in real time with zero layout shift.",
    ),
    (
        3,
        "Menus",
        "Menus display a list of choices on a temporary surface.",
    ),
    (
        3,
        "Navigation",
        "Navigation bars, drawers, and rails let people switch between UI views in an app.",
    ),
    (
        4,
        "Radio button",
        "Radio buttons let people select one option from a set of options.",
    ),
    (
        5,
        "Search",
        "Search lets people enter a keyword or phrase to get relevant information.",
    ),
    (
        2,
        "Sheets",
        "Sheets are supplementary surfaces primarily used on tablet and mobile screens.",
    ),
    (
        4,
        "Sliders",
        "Sliders let users make selections from a range of values.",
    ),
    (
        1,
        "Snackbar",
        "Snackbars show short updates about app processes at the bottom of the screen.",
    ),
    (
        4,
        "Switch",
        "Switches toggle the selection of an item on or off.",
    ),
    (
        3,
        "Tabs",
        "Tabs organize content across different screens and views.",
    ),
    (
        5,
        "Text fields",
        "Text fields let users enter text into a UI with native Korean and Japanese IME composition.",
    ),
    (
        4,
        "Time pickers",
        "Time pickers help users select and set a specific time.",
    ),
    (
        1,
        "Tooltips",
        "Tooltips display brief labels or messages when users hover, focus on, or tap an element.",
    ),
    (
        0,
        "Common buttons",
        "Common buttons prompt most actions in a UI using five emphasis levels.",
    ),
    (
        0,
        "Extended FAB",
        "Extended FABs help people take primary actions using an icon and visible text label.",
    ),
    (
        0,
        "FAB",
        "Floating action buttons help people take primary actions on screen across Small, Medium, and Large sizes.",
    ),
    (
        0,
        "FAB menu",
        "FAB menus expand from a primary FAB into related actions along a speed-dial stack.",
    ),
    (
        0,
        "Icon buttons",
        "Icon buttons help people take minor actions with one tap across Standard, Filled, Tonal, and Outlined styles.",
    ),
    (
        0,
        "Segmented buttons",
        "Segmented buttons help people select options, switch views, or sort elements.",
    ),
    (
        0,
        "Split buttons",
        "Split buttons pair a primary action with a contextual menu of related alternatives.",
    ),
    (
        4,
        "Date pickers",
        "Date pickers let people select a date or range using a calendar grid or text input.",
    ),
    (
        4,
        "Time pickers",
        "Time pickers help users choose 12H or 24H times with direct keyboard or button stepping.",
    ),
    (
        1,
        "Loading indicator",
        "Loading indicators represent an indeterminate wait state while content prepares.",
    ),
    (
        1,
        "Progress indicators",
        "Progress indicators express an unspecified wait time or display the length of a process.",
    ),
    (
        3,
        "Navigation bar",
        "Navigation bars let people switch between top-level views on compact screens.",
    ),
    (
        3,
        "Navigation drawer",
        "Navigation drawers provide ergonomic access to destinations in medium and expanded layouts.",
    ),
    (
        3,
        "Navigation rail",
        "Navigation rails provide compact access to destinations on tablet and desktop layouts.",
    ),
    (
        2,
        "Bottom sheets",
        "Bottom sheets anchor supplementary content and actions to the bottom edge of the screen.",
    ),
    (
        2,
        "Side sheets",
        "Side sheets display supplementary content and tools anchored to the trailing edge of the viewport.",
    ),
];

/// 드로어 문서 페이지 ID(`0..=40`)에 대응하는 `(카테고리, 제목, 요약)`을 반환합니다.
pub fn doc_page_meta(page_id: i32) -> (i32, &'static str, &'static str) {
    let idx = page_id.clamp(0, (DOC_PAGES.len() - 1) as i32) as usize;
    DOC_PAGES[idx]
}

/// 페이지 ID(`0..=40`)별 `Specs / Guidelines / Accessibility` 탭 요약 메타를 반환합니다.
pub fn doc_page_tab_notes(page_id: i32) -> (&'static str, &'static str, &'static str) {
    let cat = doc_page_meta(page_id).0;
    match cat {
        0 => (
            "Container height 40dp, pill corner radius (--krui-btn-radius: 999px), FAB corner 16dp (--krui-fab-radius), State Layer hover/pressed opacity.",
            "Do: Use Filled for primary emphasis, Tonal for medium actions, and Outlined/Text for secondary choices. Don't trigger hover scale jumps.",
            "Enter/Space activates buttons; 2px Focus Ring (--krui-focus-ring-color) marks keyboard focus; clear SemanticRole::Button exposed to AccessKit.",
        ),
        1 => (
            "Badge radius 999px (--krui-badge-bg #B3261E), Progress track 4dp / transform rotation 220deg/s, Snackbar radius 4dp (--krui-snackbar-bg).",
            "Do: Keep Snackbars brief and single-line with at most one action. Reserve Tooltips for short helper hints rather than core navigation labels.",
            "Motion pause toggle halts CircularProgress immediately via ViewTree::set_animations_paused and stops the 120Hz driver when off-page.",
        ),
        2 => (
            "Card radius 12dp (--md-sys-shape-corner-medium), Dialog/Sheet radius 28dp (--krui-dialog-radius), Elevated surface with multi-layer --krui-shadow.",
            "Do: Match card elevation to hierarchy (Elevated, Filled, Outlined). Close modal dialogs and sheets cleanly on scrim click or Escape.",
            "Modal dialogs and sheets trap visual backdrop in the Top Layer, keep Tab order deterministic, and preserve inner child :hover propagation.",
        ),
        3 => (
            "Nav active pill radius 999px (--krui-nav-indicator-radius), Bar height 64-80dp, Rail width 88px, Drawer width 224px using --krui-surface tokens.",
            "Do: Keep destination labels concise and consistent between NavigationBar, NavigationRail, and NavigationDrawer.",
            "Arrow/Tab focus moves cleanly across destination items; active selection state and unread badges mirror into SemanticRole properties.",
        ),
        4 => (
            "Control target 40-48dp, Chip corner 8dp (--krui-chip-radius), Switch pill track with standard cubic-bezier thumb slide.",
            "Do: Group mutually exclusive options in Radio groups and boolean settings in Checkbox or Switch controls.",
            "Checkbox, Radio, Switch, Slider, and Pickers update Selection/Checked state atomically and preserve leap-year calendar accuracy.",
        ),
        _ => (
            "TextField bottom stroke 1px -> 2px focus (--krui-input-focus-bottom-color), SearchBar pill height 56dp, surface fill --krui-input-bg.",
            "Do: Provide clear supporting text or live error messages beneath TextField inputs and filter SearchView only when search text or trigger opens it.",
            "Full Korean, Japanese, and English OS IME composition remains anchored to ime_target across focus switches; password inputs disable composition.",
        ),
    }
}

/// 카테고리 인덱스(`0..=5`)에 대응하는 기본 드로어 문서 페이지 ID를 반환합니다.
pub fn default_doc_page_for_category(cat: i32) -> i32 {
    match cat {
        0 => 3,
        1 => 12,
        2 => 4,
        3 => 1,
        4 => 6,
        _ => 22,
    }
}

/// `Get started` 페이지의 대화형 온보딩 체크리스트 컨트롤러입니다.
#[controller]
#[derive(Clone)]
pub struct OnboardingGuideController {
    /// 1단계: 4대 테마 토큰 이해 여부.
    pub step_tokens: Signal<bool>,
    /// 2단계: `.krui` 선언형 DSL 구문 이해 여부.
    pub step_dsl: Signal<bool>,
    /// 3단계: Debug 전용 핫리로드 확인 여부.
    pub step_preview: Signal<bool>,
    /// 완료 단계 요약 문자열 (`0 / 3` .. `3 / 3`).
    pub progress_label: Computed<String>,
}

#[controller]
impl OnboardingGuideController {
    /// 기본 상태의 `OnboardingGuideController`를 생성합니다.
    pub fn new() -> Self {
        let step_tokens = Signal::new(true);
        let step_dsl = Signal::new(true);
        let step_preview = Signal::new(false);
        let progress_label = Computed::new(move || {
            let done = usize::from(step_tokens.get())
                + usize::from(step_dsl.get())
                + usize::from(step_preview.get());
            format!("온보딩 진행 {done} / 3 단계")
        });
        Self {
            step_tokens,
            step_dsl,
            step_preview,
            progress_label,
        }
    }

    /// 모든 온보딩 단계를 완료 상태로 전환합니다.
    pub fn complete_all(&self) {
        let this = self.clone();
        batch(move || {
            this.step_tokens.set(true);
            this.step_dsl.set(true);
            this.step_preview.set(true);
        });
    }
}

/// 사이트 최상위 섹션 인덱스(`0..=6`)의 이름을 반환합니다.
pub fn section_name_for_index(idx: i32) -> &'static str {
    match idx {
        0 => "Home (m3.material.io)",
        1 => "Get started",
        2 => "Styles (Color · Typography · Shape)",
        3 => "Components (47종)",
        4 => "Develop (KRui Architecture)",
        5 => "Foundations (Tokens & IME)",
        _ => "Blog & Releases",
    }
}

/// 섹션과 컴포넌트 카테고리에 맞는 상단 상태 배너 문자열을 생성합니다.
pub fn section_banner_for_index(section: i32, category: i32) -> String {
    match section {
        0 => "Material 3 · Tailwind v4 순수 CSS 컴포넌트 갤러리".to_string(),
        1 => "Get started · .krui DSL, 순수 CSS 테마팩, Debug 라이브 프리뷰 안내".to_string(),
        2 => "Styles · 순수 CSS 토큰(--md-sys-* / --krui-*) 비교".to_string(),
        3 => format!("Components / {}", category_name_for_index(category)),
        4 => "Develop · Debug 전용 핫패치 & 릴리즈 0% 파서 분리".to_string(),
        5 => "Foundations · Zero-Relayout 컴포지터 전환 & OS 네이티브 IME".to_string(),
        _ => "Blog · Material Design 3 1:1 KRui 포팅 노트".to_string(),
    }
}

/// 테마 인덱스별 대표 둥글기·블러·그림자 요약 메타데이터를 반환합니다.
pub fn theme_shape_summary(idx: i32) -> &'static str {
    match idx {
        0 => "btn 999px (Pill) · card 12px · 포인터 물결 · 순수 CSS",
        _ => "btn 6px (rounded-md) · card 12px · 1px Ring · 순수 CSS",
    }
}

/// 사이트 섹션 제목, 문서 제목/요약 및 현재 테마 스펙 요약용 파생 시그널 묶음입니다.
pub struct SiteComputeds {
    /// 상단 앱바·헤더에 표시되는 현재 섹션 제목입니다.
    pub section_title: Computed<String>,
    /// 현재 선택된 드로어 문서 페이지 제목 (`Cards`, `Buttons`, ...).
    pub doc_title: Computed<String>,
    /// 현재 선택된 드로어 문서 페이지 설명.
    pub doc_summary: Computed<String>,
    /// 현재 페이지의 `Specs` 탭 토큰 요약 설명입니다.
    pub doc_spec_note: Computed<String>,
    /// 현재 페이지의 `Guidelines` 탭 가이드라인 요약 설명입니다.
    pub doc_guideline_note: Computed<String>,
    /// 현재 페이지의 `Accessibility` 탭 접근성 요약 설명입니다.
    pub doc_a11y_note: Computed<String>,
    /// 현재 활성 테마의 `data-theme` 슬러그(`"material"` 또는 `"tailwind"`).
    pub theme_slug: Computed<String>,
    /// 현재 활성 테마의 Shape/FX 요약 설명입니다.
    pub theme_shape_spec: Computed<String>,
}

/// `GalleryController`용 사이트 섹션·문서·스타일 파생 `Computed`를 생성합니다.
pub fn build_site_computeds(
    active_section: Signal<i32>,
    active_category: Signal<i32>,
    active_doc_page: Signal<i32>,
    theme_index: Signal<i32>,
) -> SiteComputeds {
    let section_title = Computed::new(move || {
        let sec = active_section.get();
        if sec == 3 {
            format!("Components · {}", doc_page_meta(active_doc_page.get()).1)
        } else {
            let _ = active_category.get();
            section_name_for_index(sec).to_string()
        }
    });
    let doc_title = Computed::new(move || doc_page_meta(active_doc_page.get()).1.to_string());
    let doc_summary = Computed::new(move || doc_page_meta(active_doc_page.get()).2.to_string());
    let doc_spec_note =
        Computed::new(move || doc_page_tab_notes(active_doc_page.get()).0.to_string());
    let doc_guideline_note =
        Computed::new(move || doc_page_tab_notes(active_doc_page.get()).1.to_string());
    let doc_a11y_note =
        Computed::new(move || doc_page_tab_notes(active_doc_page.get()).2.to_string());
    let theme_slug =
        Computed::new(move || super::handlers::theme_slug_for_index(theme_index.get()).to_string());
    let theme_shape_spec =
        Computed::new(move || theme_shape_summary(theme_index.get()).to_string());
    SiteComputeds {
        section_title,
        doc_title,
        doc_summary,
        doc_spec_note,
        doc_guideline_note,
        doc_a11y_note,
        theme_slug,
        theme_shape_spec,
    }
}
