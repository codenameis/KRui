//! 헤드리스 M3 시각 선택기 반응형 컨트롤러 (`TimePickerController`).

use krui_patterns::{controller, InputCommand};
use krui_signals::{batch, Signal};

fn parse_hm(raw: &str) -> Option<(i32, i32)> {
    let mut parts = raw.trim().split(':');
    let h: i32 = parts.next()?.parse().ok()?;
    let m: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(0..=23).contains(&h) || !(0..=59).contains(&m) {
        return None;
    }
    Some((h, m))
}

/// M3 `TimePicker`의 시·분·12h/24h 모드·직접 `HH:MM` 입력을 관리하는 헤드리스 컨트롤러.
#[controller]
#[derive(Clone)]
pub struct TimePickerController {
    /// 현재 시 (`0..=23`).
    pub hour24: Signal<i32>,
    /// 현재 분 (`0..=59`).
    pub minute: Signal<i32>,
    /// `true`이면 24시간제, `false`이면 12시간제(AM/PM).
    pub is_24h: Signal<bool>,
    /// 오후(12..=23시) 여부.
    pub is_pm: Signal<bool>,
    /// 다이얼/스핀 박스에 표시되는 2자리 시 문자열.
    pub hour_text: Signal<String>,
    /// 다이얼/스핀 박스에 표시되는 2자리 분 문자열.
    pub minute_text: Signal<String>,
    /// `"AM"` 또는 `"PM"` 문자열.
    pub period_text: Signal<String>,
    /// `HH:MM` 직접 입력칸 바인딩 문자열 (24시간 기준).
    pub hm_value: Signal<String>,
    /// 현재 시각의 한국어 요약 레이블.
    pub formatted_label: Signal<String>,
    /// `<input oninput="...">`용 `InputCommand`.
    pub on_hm_input_cmd: InputCommand,
}

#[controller]
impl TimePickerController {
    /// 초기 시(`0..=23`), 분(`0..=59`), 24시간제 여부로 새 `TimePickerController`를 생성합니다.
    pub fn new(hour24: i32, minute: i32, is_24h: bool) -> Self {
        let h = hour24.rem_euclid(24);
        let m = minute.rem_euclid(60);
        let mut ctrl = Self {
            hour24: Signal::new(h),
            minute: Signal::new(m),
            is_24h: Signal::new(is_24h),
            is_pm: Signal::new(h >= 12),
            hour_text: Signal::new(String::new()),
            minute_text: Signal::new(String::new()),
            period_text: Signal::new(String::new()),
            hm_value: Signal::new(format!("{h:02}:{m:02}")),
            formatted_label: Signal::new(String::new()),
            on_hm_input_cmd: InputCommand::new(|_| {}),
        };
        let clone_for_input = ctrl.clone();
        ctrl.on_hm_input_cmd = InputCommand::new(move |raw| {
            let _ = clone_for_input.apply_hm_input(&raw);
        });
        ctrl.sync_formatted();
        ctrl
    }

    /// 시간을 1시간 증가시킵니다 (`23` → `0`).
    pub fn inc_hour(&self) {
        let next = (self.hour24.get_untracked() + 1).rem_euclid(24);
        self.set_time(next, self.minute.get_untracked(), true);
    }

    /// 시간을 1시간 감소시킵니다 (`0` → `23`).
    pub fn dec_hour(&self) {
        let next = (self.hour24.get_untracked() - 1).rem_euclid(24);
        self.set_time(next, self.minute.get_untracked(), true);
    }

    /// 분을 1분 증가시킵니다 (`59` → `0`).
    pub fn inc_minute(&self) {
        self.step_minute(1);
    }

    /// 분을 1분 감소시킵니다 (`0` → `59`).
    pub fn dec_minute(&self) {
        self.step_minute(-1);
    }

    /// 분을 `delta`만큼 증감합니다 (`0..60` 순환).
    pub fn step_minute(&self, delta: i32) {
        let next = (self.minute.get_untracked() + delta).rem_euclid(60);
        self.set_time(self.hour24.get_untracked(), next, true);
    }

    /// 오전(AM, `0..=11`)으로 전환합니다.
    pub fn set_am(&self) {
        let h = self.hour24.get_untracked();
        if h >= 12 {
            self.set_time(h - 12, self.minute.get_untracked(), true);
        }
    }

    /// 오후(PM, `12..=23`)로 전환합니다.
    pub fn set_pm(&self) {
        let h = self.hour24.get_untracked();
        if h < 12 {
            self.set_time(h + 12, self.minute.get_untracked(), true);
        }
    }

    /// 12시간제와 24시간제 표시 모드를 토글합니다.
    pub fn toggle_24h(&self) {
        self.is_24h.update(|flag| *flag = !*flag);
        self.sync_formatted();
    }

    /// 자주 쓰는 시·분 프리셋을 한 번에 적용합니다.
    pub fn select_preset(&self, hour24: i32, minute: i32) {
        self.set_time(hour24.rem_euclid(24), minute.rem_euclid(60), true);
    }

    /// `HH:MM` 문자열 입력을 반영합니다 (템플릿 호출용).
    pub fn set_hm_text(&self, raw: String) {
        let _ = self.apply_hm_input(&raw);
    }

    /// `HH:MM` 입력을 파싱·검증해 시각을 동기화하고 유효하면 `true`를 반환합니다.
    pub fn apply_hm_input(&self, raw: &str) -> bool {
        self.hm_value.set(raw.to_string());
        let Some((h, m)) = parse_hm(raw) else {
            return false;
        };
        self.set_time(h, m, false);
        true
    }

    fn set_time(&self, h: i32, m: i32, sync_hm_input: bool) {
        let this = self.clone();
        batch(move || {
            this.hour24.set(h);
            this.minute.set(m);
            this.is_pm.set(h >= 12);
            if sync_hm_input {
                this.hm_value.set(format!("{h:02}:{m:02}"));
            }
            this.sync_formatted();
        });
    }

    fn sync_formatted(&self) {
        let h24 = self.hour24.get_untracked();
        let m = self.minute.get_untracked();
        let is_24 = self.is_24h.get_untracked();
        let pm = h24 >= 12;
        let display_h = if is_24 {
            h24
        } else {
            match h24 % 12 {
                0 => 12,
                v => v,
            }
        };
        let period = if pm { "PM" } else { "AM" };
        let label = if is_24 {
            format!("{h24:02}:{m:02} (24H)")
        } else {
            let ko_period = if pm { "오후" } else { "오전" };
            format!("{ko_period} {display_h:02}:{m:02}")
        };
        let this = self.clone();
        batch(move || {
            this.is_pm.set(pm);
            this.hour_text.set(format!("{display_h:02}"));
            this.minute_text.set(format!("{m:02}"));
            this.period_text.set(period.to_string());
            this.formatted_label.set(label);
        });
    }
}
