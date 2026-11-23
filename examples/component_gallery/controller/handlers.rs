//! 갤러리 컨트롤러용 순수 헬퍼 함수 모음 (`examples/component_gallery`).

use super::rows::filter_catalog_rows;
use super::sections::{build_site_computeds, SiteComputeds};
use krui::*;

/// 카테고리 인덱스(`0..=5`)에 대응하는 한국어 카테고리 이름을 반환합니다.
pub fn category_name_for_index(idx: i32) -> &'static str {
    match idx {
        0 => "Actions (동작)",
        1 => "Communication (알림·진행)",
        2 => "Containment (컨테이너)",
        3 => "Navigation (탐색)",
        4 => "Selection (선택·날짜·시각)",
        _ => "Text Inputs (텍스트·검색)",
    }
}

/// 순수 CSS 테마 인덱스(`0=Material 3, 1=Tailwind v4`)에 대응하는 표시 이름을 반환합니다.
pub fn theme_name_for_index(idx: i32) -> &'static str {
    match idx {
        0 => "Material 3",
        _ => "Tailwind v4",
    }
}

/// 순수 CSS 테마 인덱스(`0=material, 1=tailwind`)에 대응하는 `data-theme` 슬러그를 반환합니다.
pub fn theme_slug_for_index(idx: i32) -> &'static str {
    match idx {
        0 => "material",
        _ => "tailwind",
    }
}

/// Actions 버튼·FAB 클릭 코드(`0..=8`)에 해당하는 설명을 반환합니다.
pub fn action_name_for_code(code: i32) -> &'static str {
    match code {
        0 => "Filled Button",
        1 => "Elevated Button",
        2 => "Tonal Button",
        3 => "Outlined Button",
        4 => "Text Button",
        5 => "Small FAB",
        6 => "Medium FAB",
        7 => "Large FAB",
        _ => "Extended FAB",
    }
}

/// 밀도 세그먼트 코드(`0..=2`) 이름을 반환합니다.
pub fn density_name_for_code(mode: i32) -> &'static str {
    match mode {
        0 => "보통 (Default)",
        1 => "컴팩트 (-4px)",
        _ => "터치 여유 (+4px)",
    }
}

/// `GalleryController`의 파생 `Computed<String>` 시그널 묶음입니다.
pub struct GalleryComputeds {
    /// 배지가 0 이하일 때 빈 문자열을 반환하는 unread 배지 텍스트입니다.
    pub unread_text: Computed<String>,
    /// 사용자 이름 비어있음 검증 오류 메시지입니다.
    pub name_error: Computed<String>,
    /// 사용자 이름 글자 수 카운터 문자열입니다.
    pub name_counter: Computed<String>,
    /// 슬라이더 백분율 표시 문자열입니다.
    pub slider_label: Computed<String>,
}

/// 배지·사용자 이름·슬라이더 파생 계산 시그널을 생성합니다.
pub fn build_input_computeds(
    unread_count: Signal<i32>,
    user_name: Signal<String>,
    slider_pct: Signal<i32>,
) -> GalleryComputeds {
    let unread_text = Computed::new(move || {
        let count = unread_count.get();
        if count <= 0 {
            String::new()
        } else {
            count.to_string()
        }
    });
    let name_error = Computed::new(move || {
        if user_name.get().trim().is_empty() {
            "이름을 1자 이상 입력해 주세요.".to_string()
        } else {
            String::new()
        }
    });
    let name_counter = Computed::new(move || {
        let len = user_name.get().chars().count();
        format!("{len} / 32자")
    });
    let slider_label = Computed::new(move || format!("{}%", slider_pct.get()));
    GalleryComputeds {
        unread_text,
        name_error,
        name_counter,
        slider_label,
    }
}

/// `GalleryController::new` 초기 시그널 및 파생 `Computed` 묶음입니다.
pub struct GalleryInitBundle {
    /// 활성 섹션 시그널입니다.
    pub sec: Signal<i32>,
    /// 활성 카테고리 시그널입니다.
    pub cat: Signal<i32>,
    /// 활성 문서 페이지 시그널입니다.
    pub doc: Signal<i32>,
    /// 활성 테마 시그널입니다.
    pub thm: Signal<i32>,
    /// 읽지 않은 알림 배지 카운트입니다.
    pub unread_count: Signal<i32>,
    /// 볼륨 슬라이더 값입니다.
    pub slider_pct: Signal<i32>,
    /// 사용자 이름 입력 시그널입니다.
    pub user_name_text: Signal<String>,
    /// 입력 파생 `Computed` 묶음입니다.
    pub c: GalleryComputeds,
    /// 사이트 메타 파생 `Computed` 묶음입니다.
    pub s: SiteComputeds,
}

/// `GalleryController::new`에서 사용하는 기본 시그널 및 파생 계산기를 일괄 생성합니다.
pub fn build_gallery_init_bundle() -> GalleryInitBundle {
    let sec = Signal::new(3);
    let cat = Signal::new(2);
    let doc = Signal::new(0);
    let thm = Signal::new(0);
    let unread_count = Signal::new(4);
    let slider_pct = Signal::new(65);
    let user_name_text = Signal::new("김보현 (KRui)".to_string());
    let c = build_input_computeds(unread_count, user_name_text, slider_pct);
    let s = build_site_computeds(sec, cat, doc, thm);
    GalleryInitBundle {
        sec,
        cat,
        doc,
        thm,
        unread_count,
        slider_pct,
        user_name_text,
        c,
        s,
    }
}

/// `SplitButton` 저장 모드 코드(`0..=2`)의 레이블을 반환합니다.
pub fn split_mode_name(idx: i32) -> &'static str {
    match idx {
        0 => "즉시 저장",
        1 => "임시 보관",
        _ => "새 버전 내보내기",
    }
}

/// `FabMenuItem` 동작 코드(`0..=2`)의 이름을 반환합니다.
pub fn fab_action_name(idx: i32) -> &'static str {
    match idx {
        0 => "새 문서 작성",
        1 => "ROI 이미지 불러오기",
        _ => "테마 토큰 내보내기",
    }
}

/// `MenuItem` 네비게이션 보기 코드(`0..=2`)의 레이블을 반환합니다.
pub fn nav_menu_name(idx: i32) -> &'static str {
    match idx {
        0 => "기본 보기",
        1 => "확장 레이아웃",
        _ => "토큰 인스펙터",
    }
}

impl super::GalleryController {
    /// 검색어를 갱신하고 일치하는 M3 카탈로그 목록을 필터링합니다.
    pub fn apply_search(&self, raw: &str) {
        let filtered = filter_catalog_rows(raw);
        let is_open = !raw.trim().is_empty();
        let this = self.clone();
        let q = raw.to_string();
        batch(move || {
            this.search_query.set(q);
            this.search_open.set(is_open);
            this.search_results.set(filtered);
        });
    }
}
