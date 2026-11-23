//! 헤드리스 M3 캐러셀 수평 슬라이드 반응형 컨트롤러 (`CarouselController`).

use krui_patterns::controller;
use krui_signals::{batch, Signal};

/// M3 `Carousel`의 0-relayout `translateX` 오프셋과 페이지 인덱스를 관리하는 헤드리스 컨트롤러.
#[controller]
#[derive(Clone)]
pub struct CarouselController {
    /// 현재 활성 슬라이드 인덱스 (`0..item_count`).
    pub active_index: Signal<i32>,
    /// 전체 슬라이드 개수 (최소 1).
    pub item_count: Signal<i32>,
    /// 카드 1칸 이동 폭 (논리 px).
    pub step_px: Signal<f32>,
    /// Fast-Path `transform: translateX({offset_px}px)` 바인딩 값 (`-active_index * step_px`).
    pub offset_px: Signal<f32>,
    /// 현재 페이지 표시 문자열 (예: `"(1 / 4)"`).
    pub indicator_label: Signal<String>,
}

#[controller]
impl CarouselController {
    /// 전체 항목 수 `item_count`와 슬라이드 이동 폭 `step_px`로 새 `CarouselController`를 생성합니다.
    pub fn new(item_count: i32, step_px: f32) -> Self {
        let count = item_count.max(1);
        let step = step_px.max(1.0);
        Self {
            active_index: Signal::new(0),
            item_count: Signal::new(count),
            step_px: Signal::new(step),
            offset_px: Signal::new(0.0),
            indicator_label: Signal::new(format!("(1 / {count})")),
        }
    }

    /// 이전 슬라이드로 이동합니다 (첫 페이지에서 마지막으로 순환).
    pub fn prev(&self) {
        let count = self.item_count.get_untracked().max(1);
        let next = (self.active_index.get_untracked() - 1).rem_euclid(count);
        self.select(next);
    }

    /// 다음 슬라이드로 이동합니다 (마지막 페이지에서 처음으로 순환).
    pub fn next(&self) {
        let count = self.item_count.get_untracked().max(1);
        let next = (self.active_index.get_untracked() + 1).rem_euclid(count);
        self.select(next);
    }

    /// 지정한 인덱스 `index`로 슬라이드를 이동합니다 (Taffy relayout 0회).
    pub fn select(&self, index: i32) {
        let count = self.item_count.get_untracked().max(1);
        let clamped = index.clamp(0, count - 1);
        let offset = -(clamped as f32) * self.step_px.get_untracked();
        let this = self.clone();
        batch(move || {
            this.active_index.set(clamped);
            this.offset_px.set(offset);
            this.indicator_label
                .set(format!("({} / {count})", clamped + 1));
        });
    }
}
