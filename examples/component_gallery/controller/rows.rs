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
    /// 소속 카테고리 인덱스 (`0..=5`).
    pub category_idx: i32,
    /// 클릭 시 이동할 정확 문서 페이지 인덱스 (`0..=40`).
    pub doc_page: i32,
    /// 한국어 요약 설명.
    pub summary: String,
}

#[cfg(feature = "preview")]
impl krui::TryFromRegistryValue for SearchCatalogRow {
    fn try_from_registry_value(_value: &krui::Value) -> Option<Self> {
        None
    }
}

const CATALOG_ENTRIES: &[(i32, &str, &str, i32, i32, &str)] = &[
    (
        1,
        "Button (Common buttons)",
        "Actions",
        0,
        25,
        "5가지 강조 계층 버튼(Elevated·Filled·Tonal·Outlined·Text)",
    ),
    (
        2,
        "IconButton",
        "Actions",
        0,
        29,
        "토글 가능한 4가지 변형 아이콘 버튼",
    ),
    (
        3,
        "SegmentedButton",
        "Actions",
        0,
        30,
        "단일/그룹 밀도 및 보기 전환 세그먼트 버튼",
    ),
    (
        4,
        "SplitButton",
        "Actions",
        0,
        31,
        "기본 동작 + 보조 메뉴 드롭다운 분할 버튼",
    ),
    (
        5,
        "Fab",
        "Actions",
        0,
        27,
        "Small·Medium·Large 크기별 플로팅 액션 버튼",
    ),
    (
        6,
        "ExtendedFab",
        "Actions",
        0,
        26,
        "아이콘과 레이블이 결합된 확장 플로팅 버튼",
    ),
    (
        7,
        "FabMenu",
        "Actions",
        0,
        28,
        "스피드 다이얼 형태로 펼쳐지는 FAB 메뉴",
    ),
    (
        8,
        "Badge",
        "Communication",
        1,
        2,
        "점(Dot)·숫자 카운트 알림 배지",
    ),
    (
        9,
        "LinearProgress / CircularProgress",
        "Communication",
        1,
        12,
        "선형 진행률 바와 120Hz 0-relayout 원형 스피너",
    ),
    (
        10,
        "Snackbar",
        "Communication",
        1,
        19,
        "하단 작업 알림 및 실행 취소 액션 스낵바",
    ),
    (
        11,
        "Tooltip",
        "Communication",
        1,
        24,
        "호버 일반 툴팁 및 액션 포함 리치 툴팁",
    ),
    (
        12,
        "Card",
        "Containment",
        2,
        4,
        "Elevated/Filled/Outlined 카드와 MICL selectable/details",
    ),
    (
        13,
        "Carousel",
        "Containment",
        2,
        5,
        "0-relayout translateX 수평 카드 캐러셀",
    ),
    (
        14,
        "Dialog",
        "Containment",
        2,
        9,
        "Top-Layer 스크림 모달 대화상자 및 MICL dialog",
    ),
    (
        15,
        "Divider",
        "Containment",
        2,
        10,
        "전체·인셋·수직 1dp 구분선",
    ),
    (
        16,
        "List",
        "Containment",
        2,
        11,
        "선행 아이콘·보조 텍스트·선택 상태 지원 목록",
    ),
    (
        17,
        "BottomSheet",
        "Containment",
        2,
        39,
        "뷰포트 하단 도킹 보조 시트",
    ),
    (
        18,
        "SideSheet",
        "Containment",
        2,
        40,
        "우측 가장자리 인스펙터 측면 시트",
    ),
    (
        19,
        "TopAppBar / BottomAppBar / Toolbar",
        "Navigation",
        3,
        1,
        "상·하단 앱 바 및 도킹/플로팅 툴바",
    ),
    (
        20,
        "Menu",
        "Navigation",
        3,
        13,
        "임시 표면 팝업 메뉴 및 단축키 힌트",
    ),
    (
        21,
        "NavigationBar / Rail / Drawer",
        "Navigation",
        3,
        14,
        "바·레일·드로어 다중 탐색 패턴",
    ),
    (
        22,
        "Tabs",
        "Navigation",
        3,
        21,
        "전환 가능한 Primary/Secondary 피어 탭",
    ),
    (
        23,
        "CheckboxField",
        "Selection",
        4,
        6,
        "양방향 bind_checked 다중 선택 체크박스",
    ),
    (
        24,
        "Chip",
        "Selection",
        4,
        7,
        "Assist·Filter·Input(제거 가능)·Suggestion 칩",
    ),
    (
        25,
        "DatePicker",
        "Selection",
        4,
        32,
        "윤년 6×7 달력 격자 및 ISO 날짜 선택기",
    ),
    (
        26,
        "TimePicker",
        "Selection",
        4,
        33,
        "12H/24H 전환 시·분 스테퍼 및 HH:MM 입력기",
    ),
    (
        27,
        "RadioField / SelectField",
        "Selection",
        4,
        15,
        "상호 배타 라디오 그룹 및 Top-Layer 드롭다운 선택",
    ),
    (
        28,
        "SliderField",
        "Selection",
        4,
        18,
        "단계별 드래그/키보드 조작 범위 슬라이더",
    ),
    (
        29,
        "Switch",
        "Selection",
        4,
        20,
        "스프링 썸 이동 0-relayout 토글 스위치",
    ),
    (
        30,
        "SearchBar / SearchView",
        "Text Inputs",
        5,
        16,
        "실시간 M3 카탈로그 필터링 검색바·검색 뷰",
    ),
    (
        31,
        "TextField",
        "Text Inputs",
        5,
        22,
        "Filled·Outlined 한국어 IME 지원 텍스트 입력 필드",
    ),
];

/// 검색어 `query`로 필터링된 M3 카탈로그 행 목록을 반환합니다.
pub fn filter_catalog_rows(query: &str) -> Vec<SearchCatalogRow> {
    let q = query.trim().to_ascii_lowercase();
    CATALOG_ENTRIES
        .iter()
        .filter(|(_, name, cat, _, _, sum)| {
            q.is_empty()
                || name.to_ascii_lowercase().contains(&q)
                || cat.to_ascii_lowercase().contains(&q)
                || sum.to_ascii_lowercase().contains(&q)
        })
        .map(|(id, name, cat, cat_idx, doc_page, sum)| SearchCatalogRow {
            id: *id,
            name: (*name).to_string(),
            category: (*cat).to_string(),
            category_idx: *cat_idx,
            doc_page: *doc_page,
            summary: (*sum).to_string(),
        })
        .collect()
}
