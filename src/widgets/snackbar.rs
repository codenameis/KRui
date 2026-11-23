//! 헤드리스 M3 스낵바 알림 상태 반응형 컨트롤러 (`SnackbarController`).

use krui_patterns::controller;
use krui_signals::{batch, Signal};

/// M3 `Snackbar`의 표시·메시지·액션 실행 횟수 상태를 관리하는 헤드리스 컨트롤러.
#[controller]
#[derive(Clone)]
pub struct SnackbarController {
    /// 스낵바 표시 여부.
    pub open: Signal<bool>,
    /// 본문 알림 메시지.
    pub message: Signal<String>,
    /// 우측 액션 버튼 레이블 (빈 문자열이면 숨김).
    pub action_label: Signal<String>,
    /// 액션 버튼 클릭 누적 횟수.
    pub action_count: Signal<i32>,
}

impl Default for SnackbarController {
    fn default() -> Self {
        Self::new("", "", false)
    }
}

#[controller]
impl SnackbarController {
    /// 초기 메시지, 액션 레이블, 초기 열림 상태로 새 `SnackbarController`를 생성합니다.
    pub fn new(
        initial_message: impl Into<String>,
        initial_action: impl Into<String>,
        initially_open: bool,
    ) -> Self {
        Self {
            open: Signal::new(initially_open),
            message: Signal::new(initial_message.into()),
            action_label: Signal::new(initial_action.into()),
            action_count: Signal::new(0),
        }
    }

    /// 새 메시지와 선택적 액션 레이블로 스낵바를 엽니다.
    pub fn show(&self, message: String, action_label: String) {
        let this = self.clone();
        batch(move || {
            this.message.set(message);
            this.action_label.set(action_label);
            this.open.set(true);
        });
    }

    /// 스낵바를 닫습니다.
    pub fn dismiss(&self) {
        self.open.set(false);
    }

    /// 스낵바 액션 버튼을 실행해 `action_count`를 증가시키고 닫습니다.
    pub fn trigger_action(&self) {
        let this = self.clone();
        batch(move || {
            this.action_count.update(|c| *c += 1);
            this.open.set(false);
        });
    }
}
