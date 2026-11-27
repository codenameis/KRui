//! Material Design 3 컴포넌트 `/specs` 세부 토큰 매트릭스 데이터 (`Wave 4D`).

use krui::*;

/// M3 `/specs` 토큰 매트릭스의 단일 행 뷰모델.
#[view_model]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecTokenRow {
    /// `<for key="{row.id}">`용 안정 식별자.
    pub id: i32,
    /// 토큰 분류 (`Color` | `Shape` | `Typography` | `Spacing/Elevation`).
    pub group: String,
    /// 공식 M3 / KRui CSS 변수 토큰 이름.
    pub token: String,
    /// 기준 값 (px, ms, radius 또는 역할 값).
    pub value: String,
    /// 컴포넌트 내부 적용 역할.
    pub role: String,
}

#[cfg(feature = "preview")]
impl krui::TryFromRegistryValue for SpecTokenRow {
    fn try_from_registry_value(_value: &krui::Value) -> Option<Self> {
        None
    }
}

struct TokenSeed {
    group_id: i32,
    group: &'static str,
    token: &'static str,
    value: &'static str,
    role: &'static str,
}

const fn s(
    group_id: i32,
    group: &'static str,
    token: &'static str,
    value: &'static str,
    role: &'static str,
) -> TokenSeed {
    TokenSeed {
        group_id,
        group,
        token,
        value,
        role,
    }
}

const SHARED_CORE_SEEDS: &[TokenSeed] = &[
    s(
        1,
        "Color",
        "--md-sys-color-primary",
        "#6750A4 / dark #D0BCFF",
        "주요 강조 컨테이너·활성 상태",
    ),
    s(
        1,
        "Color",
        "--md-sys-color-on-surface",
        "#1D1B20 / dark #E6E0E9",
        "기본 라벨·본문 텍스트",
    ),
    s(
        1,
        "Color",
        "--md-sys-color-outline-variant",
        "#CAC4D0 / dark #49454F",
        "카드·입력 외곽선 및 구분선",
    ),
    s(
        2,
        "Shape",
        "--md-sys-shape-corner-full",
        "999px (Tailwind 6px)",
        "버튼·배지·알약형 탭 곡률",
    ),
    s(
        2,
        "Shape",
        "--md-sys-shape-corner-medium",
        "12px",
        "카드·메뉴·스낵바 기본 곡률",
    ),
    s(
        3,
        "Typography",
        "--md-sys-typescale-label-large",
        "14px / 500",
        "버튼·칩·네비게이션 라벨",
    ),
    s(
        3,
        "Typography",
        "--md-sys-typescale-body-medium",
        "14px / 400",
        "카드 본문·리스트 보조 설명",
    ),
    s(
        4,
        "Spacing/Elevation",
        "--krui-focus-ring-width",
        "3px offset 2px",
        "키보드 포커스 아웃라인",
    ),
    s(
        4,
        "Spacing/Elevation",
        "--md-sys-elevation-level1",
        "0 1px 3px rgba(0,0,0,.15)",
        "컴포지터 Fast-Path 그림자",
    ),
];

const NAV_SEEDS: &[TokenSeed] = &[
    s(
        1,
        "Color",
        "--md-sys-color-secondary-container",
        "#E8DEF8",
        "활성 탐색 알약 배경",
    ),
    s(
        2,
        "Shape",
        "--gallery-tabbar-height",
        "80px / corner-tabbar",
        "상단 알약 탭바 높이 및 곡률",
    ),
    s(
        4,
        "Spacing/Elevation",
        "--gallery-rail-width",
        "88px",
        "좌측 사이트 Navigation Rail 폭",
    ),
];

const BUTTON_SEEDS: &[TokenSeed] = &[
    s(
        1,
        "Color",
        "--krui-accent / --krui-accent-fg",
        "theme-driven",
        "Filled 버튼 배경·글자 대비",
    ),
    s(
        2,
        "Shape",
        "--krui-btn-radius",
        "999px (M3) / 6px (TW)",
        "버튼 4모서리 및 물결 클립",
    ),
    s(
        3,
        "Typography",
        "--md-sys-typescale-label-large-size",
        "14px (line 20px)",
        "버튼 텍스트 cap-trim 수직 정렬",
    ),
    s(
        4,
        "Spacing/Elevation",
        "--krui-btn-padding",
        "10px 24px (h 40px)",
        "최소 터치 타깃 40×40px 보장",
    ),
];

const CARD_SEEDS: &[TokenSeed] = &[
    s(
        1,
        "Color",
        "--md-sys-color-surface-container-low",
        "#F7F2FA",
        "Elevated 카드 표면 배경",
    ),
    s(
        2,
        "Shape",
        "--krui-card-radius",
        "12px",
        "Elevated/Filled/Outlined 카드 반경",
    ),
    s(
        4,
        "Spacing/Elevation",
        "--krui-shadow",
        "Level 0..2 multi-shadow",
        "레이아웃 비의존 그림자 계층",
    ),
];

const SELECTION_SEEDS: &[TokenSeed] = &[
    s(
        1,
        "Color",
        "--krui-input-bg",
        "--md-sys-color-surface",
        "선택·피커 입력 컨테이너 배경",
    ),
    s(
        2,
        "Shape",
        "--krui-chip-radius",
        "8px",
        "Filter/Input 칩 모서리 반경",
    ),
    s(
        3,
        "Typography",
        "--md-sys-typescale-title-medium-size",
        "16px",
        "날짜·시각 피커 헤더 타이틀",
    ),
    s(
        4,
        "Spacing/Elevation",
        "--krui-touch-min",
        "40px",
        "체크박스·스위치 히트 영역",
    ),
];

const OVERLAY_SEEDS: &[TokenSeed] = &[
    s(
        1,
        "Color",
        "--md-sys-color-scrim",
        "rgba(0,0,0,0.32)",
        "Top-Layer 모달·시트 스크림",
    ),
    s(
        2,
        "Shape",
        "--krui-dialog-radius",
        "28px",
        "모달 다이얼로그·바텀시트 곡률",
    ),
    s(
        4,
        "Spacing/Elevation",
        "--krui-backdrop-blur",
        "12px",
        "오프스크린 Dual-Kawase 블러",
    ),
];

const DEFAULT_SEEDS: &[TokenSeed] = &[
    s(
        1,
        "Color",
        "--krui-surface / --krui-card",
        "prefers-color-scheme",
        "라이트/다크 자동 반응 표면",
    ),
    s(
        2,
        "Shape",
        "--md-sys-shape-corner-extra-large",
        "28px",
        "문서 뷰포트·히어로 섹션 반경",
    ),
    s(
        3,
        "Typography",
        "--md-sys-typescale-hero-size",
        "clamp(28px, 8vw, 57px)",
        "반응형 디스플레이 제목",
    ),
];

fn page_specific_seeds(doc_page: i32) -> &'static [TokenSeed] {
    match doc_page {
        1 | 14 | 15 | 16 => NAV_SEEDS,
        3 | 25 | 26 | 27 | 28 | 29 | 30 => BUTTON_SEEDS,
        4 => CARD_SEEDS,
        6 | 7 | 8 | 19 | 20 => SELECTION_SEEDS,
        9 | 17 | 18 => OVERLAY_SEEDS,
        _ => DEFAULT_SEEDS,
    }
}

/// 현재 선택된 문서 페이지(`doc_page`)와 그룹 필터(`0=All, 1=Color, 2=Shape, 3=Typography, 4=Spacing/Elevation`)용 토큰 행을 반환합니다.
pub fn build_spec_rows(doc_page: i32, filter_group: i32) -> Vec<SpecTokenRow> {
    let mut rows = Vec::new();
    let mut next_id = 1_i32;
    let combined = page_specific_seeds(doc_page)
        .iter()
        .chain(SHARED_CORE_SEEDS.iter());
    for row in combined {
        if filter_group == 0 || row.group_id == filter_group {
            rows.push(SpecTokenRow {
                id: next_id,
                group: row.group.to_string(),
                token: row.token.to_string(),
                value: row.value.to_string(),
                role: row.role.to_string(),
            });
            next_id += 1;
        }
    }
    rows
}

/// Wave 4 `/specs` 필터 및 `/styles` · `/foundations` 하위 문서 라우트 상태 컨트롤러.
#[controller]
#[derive(Clone)]
pub struct Wave4RoutesController {
    /// Specs 탭 토큰 필터 (`0=All, 1=Color, 2=Shape, 3=Typography, 4=Spacing/Elevation`).
    pub spec_filter_group: Signal<i32>,
    /// Specs 탭 현재 페이지·필터 기준 M3 토큰 매트릭스 행 목록.
    pub spec_rows: Signal<Vec<SpecTokenRow>>,
    /// Styles 섹션 하위 라우트 (`0..=5`, `5=CSS Engine Showcase`).
    pub styles_subroute: Signal<i32>,
    /// Foundations 섹션 하위 라우트 (`0..=3`: Layout/Interaction/A11y/Tokens).
    pub foundations_subroute: Signal<i32>,
    /// `Develop` 섹션 2D Canvas Studio & 3D Viewport(Raw `wgpu` + Bevy) 통합 컨트롤러.
    pub studio: super::viewport3d_ctrl::StudioController,
}

#[controller]
impl Wave4RoutesController {
    /// Styles 섹션 하위 문서 라우트(`0..=5`)를 선택합니다.
    pub fn set_styles_subroute(&self, idx: i32) {
        self.styles_subroute.set(idx.clamp(0, 5));
    }

    /// Foundations 섹션 하위 문서 라우트(`0..=3`)를 선택합니다.
    pub fn set_foundations_subroute(&self, idx: i32) {
        self.foundations_subroute.set(idx.clamp(0, 3));
    }
}

impl Wave4RoutesController {
    /// 기본 상태가 초기화된 [`Wave4RoutesController`]를 생성합니다.
    pub fn new() -> Self {
        Self {
            spec_filter_group: Signal::new(0),
            spec_rows: Signal::new(build_spec_rows(0, 0)),
            styles_subroute: Signal::new(0),
            foundations_subroute: Signal::new(0),
            studio: super::viewport3d_ctrl::StudioController::new(),
        }
    }

    /// 활성 문서 페이지 및 현재 필터 기준으로 토큰 매트릭스를 갱신합니다.
    pub fn refresh_for_page(&self, doc_page: i32) {
        let g = self.spec_filter_group.get_untracked();
        self.spec_rows.set(build_spec_rows(doc_page, g));
    }
}
