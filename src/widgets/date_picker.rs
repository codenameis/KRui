//! 헤드리스 달력 날짜 선택기 반응형 컨트롤러 (`DatePickerController`).

use krui_patterns::{controller, view_model, InputCommand};
use krui_signals::{batch, Signal};

const WEEKDAY_NAMES_KO: [&str; 7] = ["일", "월", "화", "수", "목", "금", "토"];

/// 달력의 6×7(42칸) 격자에 바인딩되는 단일 날짜 셀 뷰 모델.
#[view_model]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarDayCell {
    /// 42칸 격자 내 고정 인덱스 (`0..42`, `<for key="{cell.index}">`용).
    pub index: i32,
    /// 해당 월의 일(1..=31).
    pub day: i32,
    /// 현재 표시 월 기준 오프셋 (`-1`=이전 달, `0`=이번 달, `1`=다음 달).
    pub month_offset: i32,
    /// 버튼에 표시할 1~2자리 날짜 문자열.
    pub label: String,
    /// 현재 표시 중인 월에 속하면 `true`.
    pub in_month: bool,
    /// 사용자가 선택한 날짜이면 `true`.
    pub is_selected: bool,
    /// 컨트롤러 생성 시 지정한 기준 오늘 날짜이면 `true`.
    pub is_today: bool,
}

#[cfg(feature = "preview")]
impl crate::TryFromRegistryValue for CalendarDayCell {
    fn try_from_registry_value(_value: &crate::Value) -> Option<Self> {
        None
    }
}

/// 그레고리력 기준 윤년 여부를 반환합니다.
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// 지정한 연·월의 일수를 반환합니다 (`month`는 `1..=12` 범위로 보정).
pub fn days_in_month(year: i32, month: i32) -> i32 {
    match month.clamp(1, 12) {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// 그레고리력 날짜의 요일을 일요일=0 .. 토요일=6 정수로 계산합니다 (사카모토 알고리즘).
pub fn weekday_sun0(year: i32, month: i32, day: i32) -> i32 {
    const TABLE: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let m = month.clamp(1, 12);
    let y = if m < 3 { year - 1 } else { year };
    (y + y / 4 - y / 100 + y / 400 + TABLE[(m - 1) as usize] + day).rem_euclid(7)
}

fn shift_year_month(year: i32, month: i32, delta: i32) -> (i32, i32) {
    let zero_based = year * 12 + (month - 1) + delta;
    let ny = zero_based.div_euclid(12);
    let nm = zero_based.rem_euclid(12) + 1;
    (ny, nm)
}

fn parse_iso_ymd(raw: &str) -> Option<(i32, i32, i32)> {
    let mut parts = raw.trim().split('-');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: i32 = parts.next()?.parse().ok()?;
    let d: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1900..=2200).contains(&y) || !(1..=12).contains(&m) {
        return None;
    }
    let max_d = days_in_month(y, m);
    if !(1..=max_d).contains(&d) {
        return None;
    }
    Some((y, m, d))
}

/// M3 `DatePicker`의 달력 격자·윤년·월 이동·ISO 직접 입력 상태를 관리하는 헤드리스 컨트롤러.
#[controller]
#[derive(Clone)]
pub struct DatePickerController {
    /// 현재 달력 뷰의 연도.
    pub view_year: Signal<i32>,
    /// 현재 달력 뷰의 월 (`1..=12`).
    pub view_month: Signal<i32>,
    /// 선택된 연도.
    pub selected_year: Signal<i32>,
    /// 선택된 월 (`1..=12`).
    pub selected_month: Signal<i32>,
    /// 선택된 일 (`1..=31`).
    pub selected_day: Signal<i32>,
    /// 초기 기준 오늘 연도.
    pub today_year: i32,
    /// 초기 기준 오늘 월.
    pub today_month: i32,
    /// 초기 기준 오늘 일.
    pub today_day: i32,
    /// `YYYY-MM-DD` 입력칸 바인딩 값.
    pub iso_input: Signal<String>,
    /// 상단 월 표시 문자열 (예: `"2026년 5월"`).
    pub month_label: Signal<String>,
    /// 하단 선택 요약 문자열 (예: `"선택됨: 2026-05-15 (금)"`).
    pub selected_label: Signal<String>,
    /// 헤더 강조 날짜 문자열 (예: `"5월 15일 (금)"`).
    pub headline: Signal<String>,
    /// 42칸 달력 셀 목록.
    pub cells: Signal<Vec<CalendarDayCell>>,
    /// ISO 날짜 `<input oninput="...">`용 `InputCommand`.
    pub on_iso_input_cmd: InputCommand,
}

#[controller]
impl DatePickerController {
    /// 초기 날짜 `(year, month, day)`로 새 `DatePickerController`를 생성합니다.
    pub fn new(year: i32, month: i32, day: i32) -> Self {
        let m = month.clamp(1, 12);
        let d = day.clamp(1, days_in_month(year, m));
        let view_year = Signal::new(year);
        let view_month = Signal::new(m);
        let selected_year = Signal::new(year);
        let selected_month = Signal::new(m);
        let selected_day = Signal::new(d);
        let iso_input = Signal::new(format!("{year:04}-{m:02}-{d:02}"));
        let month_label = Signal::new(String::new());
        let selected_label = Signal::new(String::new());
        let headline = Signal::new(String::new());
        let cells = Signal::new(Vec::new());
        let dummy_cmd = InputCommand::new(|_| {});

        let mut ctrl = Self {
            view_year,
            view_month,
            selected_year,
            selected_month,
            selected_day,
            today_year: year,
            today_month: m,
            today_day: d,
            iso_input,
            month_label,
            selected_label,
            headline,
            cells,
            on_iso_input_cmd: dummy_cmd,
        };
        let clone_for_input = ctrl.clone();
        ctrl.on_iso_input_cmd = InputCommand::new(move |raw| {
            let _ = clone_for_input.apply_iso_input(&raw);
        });
        ctrl.rebuild_grid();
        ctrl
    }

    /// 이전 달로 이동합니다.
    pub fn prev_month(&self) {
        let (ny, nm) = shift_year_month(
            self.view_year.get_untracked(),
            self.view_month.get_untracked(),
            -1,
        );
        self.view_year.set(ny);
        self.view_month.set(nm);
        self.rebuild_grid();
    }

    /// 다음 달로 이동합니다.
    pub fn next_month(&self) {
        let (ny, nm) = shift_year_month(
            self.view_year.get_untracked(),
            self.view_month.get_untracked(),
            1,
        );
        self.view_year.set(ny);
        self.view_month.set(nm);
        self.rebuild_grid();
    }

    /// 현재 표시 중인 연·월에서 `day`일을 선택합니다.
    pub fn select_day(&self, day: i32) {
        let vy = self.view_year.get_untracked();
        let vm = self.view_month.get_untracked();
        let max_d = days_in_month(vy, vm);
        if (1..=max_d).contains(&day) {
            self.commit_selection(vy, vm, day);
        }
    }

    /// 42칸 격자 인덱스 `index`에 해당하는 날짜 셀을 선택합니다.
    pub fn select_cell(&self, index: i32) {
        let grid = self.cells.get_untracked();
        let Some(cell) = grid.get(index as usize).cloned() else {
            return;
        };
        let (ty, tm) = shift_year_month(
            self.view_year.get_untracked(),
            self.view_month.get_untracked(),
            cell.month_offset,
        );
        self.commit_selection(ty, tm, cell.day);
    }

    /// ISO `YYYY-MM-DD` 문자열 입력을 반영합니다 (템플릿 호출용).
    pub fn set_iso_date(&self, raw: String) {
        let _ = self.apply_iso_input(&raw);
    }

    /// ISO `YYYY-MM-DD` 입력을 파싱·검증해 달력을 동기화하고 유효하면 `true`를 반환합니다.
    pub fn apply_iso_input(&self, raw: &str) -> bool {
        self.iso_input.set(raw.to_string());
        let Some((y, m, d)) = parse_iso_ymd(raw) else {
            return false;
        };
        let this = self.clone();
        batch(move || {
            this.view_year.set(y);
            this.view_month.set(m);
            this.selected_year.set(y);
            this.selected_month.set(m);
            this.selected_day.set(d);
            this.rebuild_grid();
        });
        true
    }

    fn commit_selection(&self, year: i32, month: i32, day: i32) {
        let clamped_day = day.clamp(1, days_in_month(year, month));
        let this = self.clone();
        batch(move || {
            this.view_year.set(year);
            this.view_month.set(month);
            this.selected_year.set(year);
            this.selected_month.set(month);
            this.selected_day.set(clamped_day);
            this.iso_input
                .set(format!("{year:04}-{month:02}-{clamped_day:02}"));
            this.rebuild_grid();
        });
    }

    fn rebuild_grid(&self) {
        let vy = self.view_year.get_untracked();
        let vm = self.view_month.get_untracked();
        let sy = self.selected_year.get_untracked();
        let sm = self.selected_month.get_untracked();
        let sd = self.selected_day.get_untracked();

        let (py, pm) = shift_year_month(vy, vm, -1);
        let (ny, nm) = shift_year_month(vy, vm, 1);
        let prev_days = days_in_month(py, pm);
        let cur_days = days_in_month(vy, vm);
        let start_wd = weekday_sun0(vy, vm, 1);

        let mut grid = Vec::with_capacity(42);
        for idx in 0..42_i32 {
            let rel = idx - start_wd;
            let (cy, cm, cd, offset, in_month) = if rel < 0 {
                (py, pm, prev_days + rel + 1, -1, false)
            } else if rel < cur_days {
                (vy, vm, rel + 1, 0, true)
            } else {
                (ny, nm, rel - cur_days + 1, 1, false)
            };
            grid.push(CalendarDayCell {
                index: idx,
                day: cd,
                month_offset: offset,
                label: cd.to_string(),
                in_month,
                is_selected: cy == sy && cm == sm && cd == sd,
                is_today: cy == self.today_year && cm == self.today_month && cd == self.today_day,
            });
        }
        let wd_name = WEEKDAY_NAMES_KO[weekday_sun0(sy, sm, sd) as usize];
        let this = self.clone();
        batch(move || {
            this.month_label.set(format!("{vy}년 {vm}월"));
            this.selected_label
                .set(format!("선택됨: {sy:04}-{sm:02}-{sd:02} ({wd_name})"));
            this.headline.set(format!("{sm}월 {sd}일 ({wd_name})"));
            this.cells.set(grid);
        });
    }
}
