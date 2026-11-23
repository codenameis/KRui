//! `SearchView` 실시간 컴포넌트 카탈로그 검색용 뷰 모델 행 (`examples/08_m3_gallery`).

use krui::view_model;

/// M3 컴포넌트 카탈로그 검색 결과 한 행.
#[view_model]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchCatalogRow {
    /// 고유 식별 번호 (`<for key="{item.id}">`용).
    pub id: i32,
    /// 컴포넌트 태그 이름 (예: `"DatePicker"`).
    pub name: String,
    /// 소속 M3 카테고리 이름.
    pub category: String,
    /// 클릭 시 이동할 카테고리 인덱스 (`0..=5`).
    pub category_idx: i32,
    /// 한국어 요약 설명.
    pub summary: String,
}

#[cfg(feature = "preview")]
impl krui::TryFromRegistryValue for SearchCatalogRow {
    fn try_from_registry_value(_value: &krui::Value) -> Option<Self> {
        None
    }
}

const CATALOG_ENTRIES: &[(i32, &str, &str, i32, &str)] = &[
    (
        1,
        "Button / IconButton",
        "Actions",
        0,
        "5가지 M3 버튼 변형과 토글 아이콘 버튼",
    ),
    (
        2,
        "SegmentedButton / SplitButton",
        "Actions",
        0,
        "세그먼트 선택 그룹 및 드롭다운 분할 버튼",
    ),
    (
        3,
        "Fab / ExtendedFab / FabMenu",
        "Actions",
        0,
        "크기·색상별 플로팅 액션 버튼 및 스피드 다이얼",
    ),
    (
        4,
        "Badge / Progress",
        "Communication",
        1,
        "점·숫자 배지, 선형 및 120Hz 회전 원형 진행률",
    ),
    (
        5,
        "Snackbar / Tooltip",
        "Communication",
        1,
        "하단 알림 스낵바와 일반·리치 툴팁",
    ),
    (
        6,
        "Card / Divider / List",
        "Containment",
        2,
        "Elevated/Filled/Outlined 카드, 구분선, 목록",
    ),
    (
        7,
        "Carousel",
        "Containment",
        2,
        "0-relayout translateX 수평 카드 캐러셀",
    ),
    (
        8,
        "Dialog / BottomSheet / SideSheet",
        "Containment",
        2,
        "모달 대화상자, 하단 시트, 측면 시트",
    ),
    (
        9,
        "TopAppBar / BottomAppBar / Toolbar",
        "Navigation",
        3,
        "상·하단 앱 바 및 도킹/플로팅 툴바",
    ),
    (
        10,
        "NavigationBar / Rail / Drawer",
        "Navigation",
        3,
        "바·레일·드로어 탐색 패턴",
    ),
    (
        11,
        "Tabs / Menu",
        "Navigation",
        3,
        "Primary/Secondary 탭과 메뉴 목록",
    ),
    (
        12,
        "CheckboxField / RadioField / Switch",
        "Selection",
        4,
        "체크박스·라디오 그룹·Fast-Path 스위치",
    ),
    (
        13,
        "SliderField / Chip / SelectField",
        "Selection",
        4,
        "범위 슬라이더·4종 칩·드롭다운 선택",
    ),
    (
        14,
        "DatePicker / TimePicker",
        "Selection",
        4,
        "윤년 달력 선택기 및 12H/24H 시각 선택기",
    ),
    (
        15,
        "TextField / SearchBar / SearchView",
        "Text Inputs",
        5,
        "한국어 IME 지원 입력칸·검색바·검색 뷰",
    ),
];

/// 검색어 `query`로 필터링된 M3 카탈로그 행 목록을 반환합니다.
pub fn filter_catalog_rows(query: &str) -> Vec<SearchCatalogRow> {
    let q = query.trim().to_ascii_lowercase();
    CATALOG_ENTRIES
        .iter()
        .filter(|(_, name, cat, _, sum)| {
            q.is_empty()
                || name.to_ascii_lowercase().contains(&q)
                || cat.to_ascii_lowercase().contains(&q)
                || sum.to_ascii_lowercase().contains(&q)
        })
        .map(|(id, name, cat, cat_idx, sum)| SearchCatalogRow {
            id: *id,
            name: (*name).to_string(),
            category: (*cat).to_string(),
            category_idx: *cat_idx,
            summary: (*sum).to_string(),
        })
        .collect()
}
