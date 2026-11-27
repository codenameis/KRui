//! 3D 에디터 분할선(`SplitPaneController`) 및 도킹 4분할 레이아웃(`DockLayoutController`).

use krui_signals::{batch, Signal};
use std::cell::Cell;
use std::rc::Rc;

/// 분할 방향.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitAxis {
    /// 좌/우 수평 분할.
    Horizontal,
    /// 상/하 수직 분할.
    Vertical,
}

/// 드래그 가능한 2패널 분할 컨트롤러.
#[derive(Clone)]
pub struct SplitPaneController {
    /// 분할 축.
    pub axis: SplitAxis,
    /// 첫 번째 패널 비율(`min_ratio..=max_ratio`).
    pub ratio: Signal<f32>,
    /// 현재 스플리터를 드래그 중인지 여부.
    pub dragging: Signal<bool>,
    min_ratio: f32,
    max_ratio: f32,
    invert_drag: bool,
    drag_start: Rc<Cell<(f32, f32)>>,
}

impl SplitPaneController {
    /// 초기 비율과 허용 범위를 지정해 분할 컨트롤러를 만듭니다.
    pub fn new(axis: SplitAxis, initial_ratio: f32, min_ratio: f32, max_ratio: f32) -> Self {
        let lo = min_ratio.clamp(0.05, 0.9);
        let hi = max_ratio.clamp(lo + 0.01, 0.95);
        Self {
            axis,
            ratio: Signal::new(initial_ratio.clamp(lo, hi)),
            dragging: Signal::new(false),
            min_ratio: lo,
            max_ratio: hi,
            invert_drag: false,
            drag_start: Rc::new(Cell::new((0.0, initial_ratio.clamp(lo, hi)))),
        }
    }

    /// 우측·하단 도킹 패널처럼 컨테이너 끝에서부터 비율을 재는 분할선에 역방향 드래그를 설정합니다.
    #[must_use]
    pub fn with_inverted_drag(mut self) -> Self {
        self.invert_drag = true;
        self
    }

    /// 스플리터 드래그를 시작합니다 (`coord`는 주축 좌표).
    pub fn begin_drag(&self, coord: f32) {
        if !coord.is_finite() {
            return;
        }
        self.drag_start.set((coord, self.ratio.get_untracked()));
        self.dragging.set(true);
    }

    /// 전체 컨테이너 픽셀 길이 `total_px`를 기준으로 새 비율을 갱신합니다.
    pub fn drag_to(&self, coord: f32, total_px: f32) {
        if !self.dragging.get_untracked()
            || !coord.is_finite()
            || !total_px.is_finite()
            || total_px <= 1.0
        {
            return;
        }
        let (start_coord, start_ratio) = self.drag_start.get();
        let delta = if self.invert_drag {
            start_coord - coord
        } else {
            coord - start_coord
        };
        let next = (start_ratio + delta / total_px).clamp(self.min_ratio, self.max_ratio);
        self.ratio.set(next);
    }

    /// 스플리터 드래그를 종료합니다.
    pub fn end_drag(&self) {
        self.dragging.set(false);
    }

    /// 스플리터 두께 `divider_px`를 제외한 두 패널의 픽셀 크기 `(first, second)`를 계산합니다.
    pub fn compute_sizes_px(&self, total_px: f32, divider_px: f32) -> (f32, f32) {
        let usable = (total_px - divider_px.max(0.0)).max(0.0);
        let first = (usable * self.ratio.get_untracked()).round();
        let second = (usable - first).max(0.0);
        (first, second)
    }
}

/// 4대 도킹 패널 식별자.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DockPane {
    /// 좌측 씬 계층(Hierarchy) 패널.
    Hierarchy,
    /// 중앙 3D 뷰포트(Viewport).
    Viewport,
    /// 우측 속성 인스펙터(Inspector) 패널.
    Inspector,
    /// 하단 에셋 브라우저(Asset Browser) 패널.
    AssetBrowser,
}

/// Hierarchy(좌) · Viewport(중) · Inspector(우) · Asset Browser(하)를 묶어 관리하는 도킹 컨트롤러.
#[derive(Clone)]
pub struct DockLayoutController {
    /// 좌측 Hierarchy 패널 비율(전체 폭 대비).
    pub left_split: SplitPaneController,
    /// 우측 Inspector 패널 비율(전체 폭 대비, 우측 기준).
    pub right_split: SplitPaneController,
    /// 하단 Asset Browser 패널 비율(전체 높이 대비, 하단 기준).
    pub bottom_split: SplitPaneController,
    /// 좌측 Hierarchy 패널 표시 여부.
    pub show_hierarchy: Signal<bool>,
    /// 우측 Inspector 패널 표시 여부.
    pub show_inspector: Signal<bool>,
    /// 하단 Asset Browser 패널 표시 여부.
    pub show_assets: Signal<bool>,
    /// 현재 활성/포커스된 도킹 패널.
    pub active_pane: Signal<DockPane>,
}

impl Default for DockLayoutController {
    fn default() -> Self {
        Self::new()
    }
}

impl DockLayoutController {
    /// 기본 3D 에디터 도킹 비율(`left=0.20`, `right=0.24`, `bottom=0.26`)로 초기화합니다.
    pub fn new() -> Self {
        Self {
            left_split: SplitPaneController::new(SplitAxis::Horizontal, 0.20, 0.10, 0.42),
            right_split: SplitPaneController::new(SplitAxis::Horizontal, 0.24, 0.12, 0.45)
                .with_inverted_drag(),
            bottom_split: SplitPaneController::new(SplitAxis::Vertical, 0.26, 0.10, 0.55)
                .with_inverted_drag(),
            show_hierarchy: Signal::new(true),
            show_inspector: Signal::new(true),
            show_assets: Signal::new(true),
            active_pane: Signal::new(DockPane::Viewport),
        }
    }

    /// 지정 패널의 표시/숨김을 전환합니다(뷰포트는 항상 표시).
    pub fn toggle_pane(&self, pane: DockPane) {
        match pane {
            DockPane::Hierarchy => self
                .show_hierarchy
                .set(!self.show_hierarchy.get_untracked()),
            DockPane::Inspector => self
                .show_inspector
                .set(!self.show_inspector.get_untracked()),
            DockPane::AssetBrowser => self.show_assets.set(!self.show_assets.get_untracked()),
            DockPane::Viewport => {}
        }
    }

    /// 모든 사이드/하단 패널을 끄는 전체 화면 뷰포트 토글(다시 호출 시 복원).
    pub fn focus_viewport_only(&self, solo: bool) {
        let sh = self.show_hierarchy;
        let si = self.show_inspector;
        let sa = self.show_assets;
        let act = self.active_pane;
        batch(move || {
            sh.set(!solo);
            si.set(!solo);
            sa.set(!solo);
            act.set(DockPane::Viewport);
        });
    }

    /// 현재 표시 상태와 분할 비율로 중앙 3D 뷰포트 사각형 `(x, y, width, height)`를 계산합니다.
    pub fn viewport_rect(
        &self,
        total_w: f32,
        total_h: f32,
        divider_px: f32,
    ) -> (f32, f32, f32, f32) {
        let div = divider_px.max(0.0);
        let left_w = if self.show_hierarchy.get_untracked() {
            (total_w * self.left_split.ratio.get_untracked()).round() + div
        } else {
            0.0
        };
        let right_w = if self.show_inspector.get_untracked() {
            (total_w * self.right_split.ratio.get_untracked()).round() + div
        } else {
            0.0
        };
        let bottom_h = if self.show_assets.get_untracked() {
            (total_h * self.bottom_split.ratio.get_untracked()).round() + div
        } else {
            0.0
        };
        let vp_w = (total_w - left_w - right_w).max(1.0);
        let vp_h = (total_h - bottom_h).max(1.0);
        (left_w, 0.0, vp_w, vp_h)
    }
}
