//! 인스펙터 수치 스크러버(`DragFloat`/`Vec3`), `ColorPickerController`, `ContextMenuController`.

use krui_core::{Mods, Size};
use krui_signals::{batch, Signal};
use std::cell::Cell;
use std::rc::Rc;

/// 드래그 스크러빙·정밀(`Shift`)·스냅(`Ctrl`) 및 텍스트 입력 파싱을 지원하는 실수 컨트롤러.
#[derive(Clone)]
pub struct DragFloatController {
    /// 현재 수치.
    pub value: Signal<f32>,
    /// UI 표시용 서식 문자열.
    pub formatted: Signal<String>,
    /// 현재 드래그 중인지 여부.
    pub dragging: Signal<bool>,
    min: f32,
    max: f32,
    step: f32,
    decimals: usize,
    drag_start: Rc<Cell<(f32, f32)>>,
}

impl DragFloatController {
    /// 초기값과 최소/최대/기본 스텝·소수점 자릿수로 생성합니다.
    pub fn new(initial: f32, min: f32, max: f32, step: f32, decimals: usize) -> Self {
        let lo = min.min(max);
        let hi = max.max(lo);
        let v = initial.clamp(lo, hi);
        let dec = decimals.min(6);
        Self {
            value: Signal::new(v),
            formatted: Signal::new(format!("{v:.dec$}")),
            dragging: Signal::new(false),
            min: lo,
            max: hi,
            step: step.abs().max(0.0001),
            decimals: dec,
            drag_start: Rc::new(Cell::new((0.0, v))),
        }
    }

    /// 값을 범위 내로 클램프하여 설정합니다.
    pub fn set_value(&self, next: f32) {
        if !next.is_finite() {
            return;
        }
        let clamped = next.clamp(self.min, self.max);
        let dec = self.decimals;
        let val_sig = self.value;
        let fmt_sig = self.formatted;
        batch(move || {
            val_sig.set(clamped);
            fmt_sig.set(format!("{clamped:.dec$}"));
        });
    }

    /// 마우스 스크러빙 드래그를 시작합니다.
    pub fn begin_scrub(&self, pointer_x: f32) {
        self.drag_start.set((pointer_x, self.value.get_untracked()));
        self.dragging.set(true);
    }

    /// `Shift`(0.1x 정밀)·`Ctrl`(스텝 스냅)을 반영해 드래그 값을 갱신합니다.
    pub fn scrub_to(&self, pointer_x: f32, mods: Mods) {
        if !self.dragging.get_untracked() {
            return;
        }
        let (start_x, start_val) = self.drag_start.get();
        let sens = if mods.shift { 0.1 } else { 1.0 };
        let mut raw = start_val + (pointer_x - start_x) * self.step * sens;
        if mods.ctrl {
            let snap_unit = (self.step * 10.0).max(0.1);
            raw = (raw / snap_unit).round() * snap_unit;
        }
        self.set_value(raw);
    }

    /// 스크러빙 드래그를 종료합니다.
    pub fn end_scrub(&self) {
        self.dragging.set(false);
    }

    /// 직접 입력된 문자열을 파싱해 반영합니다. 잘못된 입력이면 무시하고 `false`를 반환합니다.
    pub fn parse_input(&self, text: &str) -> bool {
        let Ok(parsed) = text.trim().parse::<f32>() else {
            return false;
        };
        if !parsed.is_finite() {
            return false;
        }
        self.set_value(parsed);
        true
    }
}

/// `X / Y / Z` 3축 수치 스크러버 묶음 컨트롤러.
#[derive(Clone)]
pub struct Vec3Controller {
    /// X축 스크러버.
    pub x: DragFloatController,
    /// Y축 스크러버.
    pub y: DragFloatController,
    /// Z축 스크러버.
    pub z: DragFloatController,
}

impl Vec3Controller {
    /// 초기 `[x, y, z]` 벡터와 스텝으로 생성합니다.
    pub fn new(initial: [f32; 3], step: f32, decimals: usize) -> Self {
        Self {
            x: DragFloatController::new(initial[0], -100_000.0, 100_000.0, step, decimals),
            y: DragFloatController::new(initial[1], -100_000.0, 100_000.0, step, decimals),
            z: DragFloatController::new(initial[2], -100_000.0, 100_000.0, step, decimals),
        }
    }

    /// 현재 `[x, y, z]` 값을 반환합니다.
    pub fn get(&self) -> [f32; 3] {
        [
            self.x.value.get_untracked(),
            self.y.value.get_untracked(),
            self.z.value.get_untracked(),
        ]
    }

    /// `[x, y, z]` 세 축 값을 일괄 갱신합니다.
    pub fn set(&self, v: [f32; 3]) {
        self.x.set_value(v[0]);
        self.y.set_value(v[1]);
        self.z.set_value(v[2]);
    }
}

/// HSVA + `#RRGGBB` / `#RRGGBBAA` 상호 변환 `ColorPickerController`.
#[derive(Clone)]
pub struct ColorPickerController {
    /// 색상(Hue, `0.0..360.0`).
    pub hue: Signal<f32>,
    /// 채도(Saturation, `0.0..=1.0`).
    pub saturation: Signal<f32>,
    /// 명도(Value, `0.0..=1.0`).
    pub value: Signal<f32>,
    /// 불투명도(`0.0..=1.0`).
    pub alpha: Signal<f32>,
    /// 동기화된 16진수 색상 문자열.
    pub hex: Signal<String>,
}

impl ColorPickerController {
    /// RGBA8 배열로 `ColorPickerController`를 생성합니다.
    pub fn from_rgba_u8(rgba: [u8; 4]) -> Self {
        let (h, s, v) = rgb_to_hsv(rgba[0], rgba[1], rgba[2]);
        let a = rgba[3] as f32 / 255.0;
        let hex_str = format_hex_rgba(rgba);
        Self {
            hue: Signal::new(h),
            saturation: Signal::new(s),
            value: Signal::new(v),
            alpha: Signal::new(a),
            hex: Signal::new(hex_str),
        }
    }

    /// 2D 채도·명도 패드 좌표(`0..=1`)를 갱신합니다.
    pub fn set_sv(&self, s: f32, v: f32) {
        let s = s.clamp(0.0, 1.0);
        let v = v.clamp(0.0, 1.0);
        let h = self.hue.get_untracked();
        let a = self.alpha.get_untracked();
        self.commit_hsva(h, s, v, a);
    }

    /// 색상 각도(`0..360`)를 갱신합니다.
    pub fn set_hue(&self, h: f32) {
        let h = h.rem_euclid(360.0);
        self.commit_hsva(
            h,
            self.saturation.get_untracked(),
            self.value.get_untracked(),
            self.alpha.get_untracked(),
        );
    }

    /// 알파(`0..=1`)를 갱신합니다.
    pub fn set_alpha(&self, a: f32) {
        self.commit_hsva(
            self.hue.get_untracked(),
            self.saturation.get_untracked(),
            self.value.get_untracked(),
            a.clamp(0.0, 1.0),
        );
    }

    /// `#RGB`, `#RRGGBB`, `#RRGGBBAA` 문자열을 파싱해 HSVA와 Hex를 동기화합니다.
    pub fn set_hex(&self, input: &str) -> bool {
        let Some(rgba) = parse_hex_rgba(input) else {
            return false;
        };
        let (h, s, v) = rgb_to_hsv(rgba[0], rgba[1], rgba[2]);
        let a = rgba[3] as f32 / 255.0;
        self.commit_hsva(h, s, v, a);
        true
    }

    /// 현재 RGBA8 배열을 반환합니다.
    pub fn rgba_u8(&self) -> [u8; 4] {
        let (r, g, b) = hsv_to_rgb(
            self.hue.get_untracked(),
            self.saturation.get_untracked(),
            self.value.get_untracked(),
        );
        let a = (self.alpha.get_untracked().clamp(0.0, 1.0) * 255.0).round() as u8;
        [r, g, b, a]
    }

    /// 정규화된 `[f32; 4]` RGBA 배열을 반환합니다.
    pub fn rgba_f32(&self) -> [f32; 4] {
        let [r, g, b, a] = self.rgba_u8();
        [
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        ]
    }

    fn commit_hsva(&self, h: f32, s: f32, v: f32, a: f32) {
        let (r, g, b) = hsv_to_rgb(h, s, v);
        let a8 = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
        let hex_str = format_hex_rgba([r, g, b, a8]);
        let (hs, ss, vs, al, hx) = (self.hue, self.saturation, self.value, self.alpha, self.hex);
        batch(move || {
            hs.set(h);
            ss.set(s);
            vs.set(v);
            al.set(a);
            hx.set(hex_str);
        });
    }
}

/// 컨텍스트 메뉴 단일 항목 정의.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextMenuItem {
    /// 콜백/액션 식별자.
    pub id: String,
    /// 표시 텍스트.
    pub label: String,
    /// 단축키 표기(예: `"Ctrl+D"`).
    pub shortcut: String,
    /// 비활성 여부.
    pub disabled: bool,
}

/// 뷰포트 경계 클램핑을 지원하는 우클릭 컨텍스트 메뉴 컨트롤러.
#[derive(Clone)]
pub struct ContextMenuController {
    /// 열림 여부.
    pub open: Signal<bool>,
    /// 클램프된 메뉴 X 좌표.
    pub x: Signal<f32>,
    /// 클램프된 메뉴 Y 좌표.
    pub y: Signal<f32>,
    /// 현재 메뉴 항목 목록.
    pub items: Signal<Vec<ContextMenuItem>>,
}

impl Default for ContextMenuController {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextMenuController {
    /// 닫힌 상태의 `ContextMenuController`를 만듭니다.
    pub fn new() -> Self {
        Self {
            open: Signal::new(false),
            x: Signal::new(0.0),
            y: Signal::new(0.0),
            items: Signal::new(Vec::new()),
        }
    }

    /// `pos` 위치에 메뉴를 열되 `viewport` 밖으로 넘치지 않게 `menu` 크기 기준으로 클램프합니다.
    pub fn open_at(
        &self,
        pos: (f32, f32),
        viewport: Size,
        menu: Size,
        items: Vec<ContextMenuItem>,
    ) {
        let cx = pos.0.clamp(0.0, (viewport.width - menu.width).max(0.0));
        let cy = pos.1.clamp(0.0, (viewport.height - menu.height).max(0.0));
        let (op, xs, ys, it) = (self.open, self.x, self.y, self.items);
        batch(move || {
            it.set(items);
            xs.set(cx);
            ys.set(cy);
            op.set(true);
        });
    }

    /// 메뉴를 닫습니다.
    pub fn close(&self) {
        self.open.set(false);
    }

    /// 활성 항목을 선택하면 메뉴를 닫고 해당 ID를 반환합니다. 비활성 항목이면 `None`을 반환합니다.
    pub fn select(&self, id: &str) -> Option<String> {
        let valid = self
            .items
            .get_untracked()
            .into_iter()
            .find(|item| item.id == id && !item.disabled)?;
        self.open.set(false);
        Some(valid.id)
    }
}

fn format_hex_rgba(rgba: [u8; 4]) -> String {
    if rgba[3] == 255 {
        format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2])
    } else {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            rgba[0], rgba[1], rgba[2], rgba[3]
        )
    }
}

fn parse_hex_rgba(input: &str) -> Option<[u8; 4]> {
    let hex = input.trim().strip_prefix('#').unwrap_or(input.trim());
    let byte = |s: &str| u8::from_str_radix(s, 16).ok();
    match hex.len() {
        3 => {
            let b = hex.as_bytes();
            let dup = |idx: usize| -> Option<u8> {
                let d = (b[idx] as char).to_digit(16)? as u8;
                Some((d << 4) | d)
            };
            Some([dup(0)?, dup(1)?, dup(2)?, 255])
        }
        6 => Some([byte(&hex[0..2])?, byte(&hex[2..4])?, byte(&hex[4..6])?, 255]),
        8 => Some([
            byte(&hex[0..2])?,
            byte(&hex[2..4])?,
            byte(&hex[4..6])?,
            byte(&hex[6..8])?,
        ]),
        _ => None,
    }
}

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (rf, gf, bf) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;
    let h = if delta <= 1e-6 {
        0.0
    } else if (max - rf).abs() <= 1e-6 {
        60.0 * (((gf - bf) / delta).rem_euclid(6.0))
    } else if (max - gf).abs() <= 1e-6 {
        60.0 * (((bf - rf) / delta) + 2.0)
    } else {
        60.0 * (((rf - gf) / delta) + 4.0)
    };
    let s = if max <= 1e-6 { 0.0 } else { delta / max };
    (h.rem_euclid(360.0), s, max)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v.clamp(0.0, 1.0) * s.clamp(0.0, 1.0);
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = v.clamp(0.0, 1.0) - c;
    let (r1, g1, b1) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}
