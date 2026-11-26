//! `<ExternalTexture>` 위 투명 `<canvas>` 오버레이용 3D 기즈모(Translate/Rotate/Scale) 컨트롤러.

use krui_core::{CanvasContext2D, Size};
use krui_signals::Signal;
use krui_style::Color;
use std::cell::Cell;
use std::rc::Rc;

/// 기즈모 조작 모드.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GizmoMode {
    /// 3축 이동 화살표.
    Translate,
    /// 3축 회전 링.
    Rotate,
    /// 3축 스케일 핸들.
    Scale,
}

/// 기즈모 조작 축.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GizmoAxis {
    /// X축.
    X,
    /// Y축.
    Y,
    /// Z축.
    Z,
}

/// CSS 테마/토큰에서 해결한 기즈모 축 색상 팔레트(코어/위젯 내 색상 하드코딩 방지).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GizmoPalette {
    /// X축 색.
    pub x_axis: Color,
    /// Y축 색.
    pub y_axis: Color,
    /// Z축 색.
    pub z_axis: Color,
    /// 선택/활성 드래그 중 강조 색.
    pub active: Color,
    /// 축 라벨(`X`/`Y`/`Z`) 텍스트 색.
    pub label: Color,
}

/// 3D -> 2D 뷰포트 투영용 직교 기저 카메라.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraBasis3D {
    /// 카메라 위치.
    pub eye: [f32; 3],
    /// 카메라 오른쪽 단위 벡터.
    pub right: [f32; 3],
    /// 카메라 위쪽 단위 벡터.
    pub up: [f32; 3],
    /// 카메라 시선 전방 단위 벡터.
    pub forward: [f32; 3],
    /// 수직 시야각(rad).
    pub fov_y_rad: f32,
}

impl CameraBasis3D {
    /// `target`을 공전하는 오빗 카메라 기저를 생성합니다.
    pub fn orbit(
        target: [f32; 3],
        distance: f32,
        yaw_rad: f32,
        pitch_rad: f32,
        fov_y_rad: f32,
    ) -> Self {
        let d = distance.max(0.2);
        let cp = pitch_rad.clamp(-1.5, 1.5).cos();
        let sp = pitch_rad.clamp(-1.5, 1.5).sin();
        let sy = yaw_rad.sin();
        let cy = yaw_rad.cos();
        let eye = [
            target[0] + d * cp * sy,
            target[1] + d * sp,
            target[2] + d * cp * cy,
        ];
        let forward = normalize3(sub3(target, eye));
        let world_up = [0.0, 1.0, 0.0];
        let right = normalize3(cross3(forward, world_up));
        let up = cross3(right, forward);
        Self {
            eye,
            right,
            up,
            forward,
            fov_y_rad: fov_y_rad.clamp(0.2, 2.4),
        }
    }

    /// 월드 좌표를 뷰포트 로컬 `(sx, sy, depth)`로 투영합니다. 카메라 뒤면 `None`.
    pub fn project(&self, world_pos: [f32; 3], viewport: Size) -> Option<(f32, f32, f32)> {
        let rel = sub3(world_pos, self.eye);
        let z = dot3(rel, self.forward);
        if z <= 0.05 || viewport.width <= 1.0 || viewport.height <= 1.0 {
            return None;
        }
        let vx = dot3(rel, self.right);
        let vy = dot3(rel, self.up);
        let f = 1.0 / (self.fov_y_rad * 0.5).tan();
        let aspect = viewport.width / viewport.height;
        let ndc_x = (vx / z) * (f / aspect);
        let ndc_y = (vy / z) * f;
        Some((
            (ndc_x * 0.5 + 0.5) * viewport.width,
            (0.5 - ndc_y * 0.5) * viewport.height,
            z,
        ))
    }
}

#[derive(Clone, Copy)]
struct DragSession {
    axis: GizmoAxis,
    mode: GizmoMode,
    start_ptr: (f32, f32),
    axis_dir_2d: (f32, f32),
    start_pos: [f32; 3],
    start_rot: [f32; 3],
    start_scale: [f32; 3],
}

/// 투명 `<canvas>` 오버레이 위에서 3D Translate/Rotate/Scale 기즈모를 그리고 조작하는 컨트롤러.
#[derive(Clone)]
pub struct GizmoController {
    /// 현재 기즈모 모드.
    pub mode: Signal<GizmoMode>,
    /// 호버/드래그 중인 활성 축.
    pub active_axis: Signal<Option<GizmoAxis>>,
    /// 월드 위치 `[x, y, z]`.
    pub position: Signal<[f32; 3]>,
    /// 오일러 회전 각도(deg) `[x, y, z]`.
    pub rotation_deg: Signal<[f32; 3]>,
    /// 스케일 `[x, y, z]`.
    pub scale: Signal<[f32; 3]>,
    /// 월드 기즈모 축 길이.
    pub axis_world_len: f32,
    drag: Rc<Cell<Option<DragSession>>>,
}

impl Default for GizmoController {
    fn default() -> Self {
        Self::new([0.0, 0.0, 0.0])
    }
}

impl GizmoController {
    /// 초기 위치로 새 `GizmoController`를 생성합니다.
    pub fn new(position: [f32; 3]) -> Self {
        Self {
            mode: Signal::new(GizmoMode::Translate),
            active_axis: Signal::new(None),
            position: Signal::new(position),
            rotation_deg: Signal::new([0.0, 0.0, 0.0]),
            scale: Signal::new([1.0, 1.0, 1.0]),
            axis_world_len: 1.25,
            drag: Rc::new(Cell::new(None)),
        }
    }

    /// 캔버스 로컬 좌표 `(lx, ly)`와 가장 가까운 축 핸들을 히트테스트합니다.
    pub fn hit_test_axis(
        &self,
        lx: f32,
        ly: f32,
        camera: &CameraBasis3D,
        viewport: Size,
    ) -> Option<GizmoAxis> {
        let pos = self.position.get_untracked();
        let (ox, oy, _) = camera.project(pos, viewport)?;
        let mut best: Option<(GizmoAxis, f32)> = None;
        for (axis, dir) in axis_basis() {
            let end = add3(pos, mul3(dir, self.axis_world_len));
            let Some((ex, ey, _)) = camera.project(end, viewport) else {
                continue;
            };
            let dist = point_to_segment_dist((lx, ly), (ox, oy), (ex, ey));
            if dist <= 14.0 && best.is_none_or(|(_, bd)| dist < bd) {
                best = Some((axis, dist));
            }
        }
        best.map(|(a, _)| a)
    }

    /// 포인터 다운 시 축 드래그를 시작합니다.
    pub fn begin_drag(
        &self,
        lx: f32,
        ly: f32,
        camera: &CameraBasis3D,
        viewport: Size,
    ) -> Option<GizmoAxis> {
        let axis = self.hit_test_axis(lx, ly, camera, viewport)?;
        let pos = self.position.get_untracked();
        let (ox, oy, _) = camera.project(pos, viewport)?;
        let end = add3(pos, mul3(axis_vec(axis), self.axis_world_len));
        let (ex, ey, _) = camera.project(end, viewport).unwrap_or((ox + 1.0, oy, 1.0));
        let (dx, dy) = (ex - ox, ey - oy);
        let len = dx.hypot(dy).max(1.0);
        self.active_axis.set(Some(axis));
        self.drag.set(Some(DragSession {
            axis,
            mode: self.mode.get_untracked(),
            start_ptr: (lx, ly),
            axis_dir_2d: (dx / len, dy / len),
            start_pos: pos,
            start_rot: self.rotation_deg.get_untracked(),
            start_scale: self.scale.get_untracked(),
        }));
        Some(axis)
    }

    /// 포인터 이동에 따라 활성 모드의 위치/회전/스케일을 갱신합니다.
    pub fn drag_to(&self, lx: f32, ly: f32) -> bool {
        let Some(s) = self.drag.get() else {
            return false;
        };
        let proj = (lx - s.start_ptr.0) * s.axis_dir_2d.0 + (ly - s.start_ptr.1) * s.axis_dir_2d.1;
        let idx = axis_index(s.axis);
        match s.mode {
            GizmoMode::Translate => {
                let mut next = s.start_pos;
                next[idx] += proj * 0.02;
                self.position.set(next);
            }
            GizmoMode::Rotate => {
                let mut next = s.start_rot;
                next[idx] += proj * 0.5;
                self.rotation_deg.set(next);
            }
            GizmoMode::Scale => {
                let mut next = s.start_scale;
                next[idx] = (next[idx] + proj * 0.01).max(0.05);
                self.scale.set(next);
            }
        }
        true
    }

    /// 드래그를 종료합니다.
    pub fn end_drag(&self) {
        self.drag.set(None);
        self.active_axis.set(None);
    }

    /// 투명 `<canvas>` 컨텍스트에 기즈모 축선·핸들·`X/Y/Z` 텍스트 라벨을 그립니다.
    pub fn draw_to_canvas(
        &self,
        ctx: &mut CanvasContext2D,
        viewport: Size,
        camera: &CameraBasis3D,
        palette: &GizmoPalette,
    ) {
        let pos = self.position.get_untracked();
        let Some((ox, oy, _)) = camera.project(pos, viewport) else {
            return;
        };
        let active = self.active_axis.get_untracked();
        let mode = self.mode.get_untracked();
        for (axis, dir) in axis_basis() {
            let end = add3(pos, mul3(dir, self.axis_world_len));
            let Some((ex, ey, _)) = camera.project(end, viewport) else {
                continue;
            };
            let color = if active == Some(axis) {
                palette.active
            } else {
                axis_color(axis, palette)
            };
            ctx.begin_path();
            ctx.move_to(ox, oy);
            ctx.line_to(ex, ey);
            ctx.stroke_path(2.5, color);
            draw_axis_handle(ctx, (ex, ey), axis, mode, color, palette.label);
        }
    }
}

fn draw_axis_handle(
    ctx: &mut CanvasContext2D,
    (ex, ey): (f32, f32),
    axis: GizmoAxis,
    mode: GizmoMode,
    color: Color,
    label_color: Color,
) {
    match mode {
        GizmoMode::Translate => {
            ctx.begin_path();
            ctx.arc(ex, ey, 5.0, 0.0, std::f32::consts::TAU);
            ctx.fill_path(color);
        }
        GizmoMode::Rotate => {
            ctx.begin_path();
            ctx.arc(ex, ey, 7.0, 0.0, std::f32::consts::TAU);
            ctx.stroke_path(2.0, color);
        }
        GizmoMode::Scale => ctx.fill_rect(ex - 4.5, ey - 4.5, 9.0, 9.0, color),
    }
    let tag = match axis {
        GizmoAxis::X => "X",
        GizmoAxis::Y => "Y",
        GizmoAxis::Z => "Z",
    };
    ctx.draw_text(tag, ex + 8.0, ey - 6.0, 12.0, label_color);
}

fn axis_basis() -> [(GizmoAxis, [f32; 3]); 3] {
    [
        (GizmoAxis::X, [1.0, 0.0, 0.0]),
        (GizmoAxis::Y, [0.0, 1.0, 0.0]),
        (GizmoAxis::Z, [0.0, 0.0, 1.0]),
    ]
}

fn axis_vec(axis: GizmoAxis) -> [f32; 3] {
    match axis {
        GizmoAxis::X => [1.0, 0.0, 0.0],
        GizmoAxis::Y => [0.0, 1.0, 0.0],
        GizmoAxis::Z => [0.0, 0.0, 1.0],
    }
}

fn axis_index(axis: GizmoAxis) -> usize {
    match axis {
        GizmoAxis::X => 0,
        GizmoAxis::Y => 1,
        GizmoAxis::Z => 2,
    }
}

fn axis_color(axis: GizmoAxis, p: &GizmoPalette) -> Color {
    match axis {
        GizmoAxis::X => p.x_axis,
        GizmoAxis::Y => p.y_axis,
        GizmoAxis::Z => p.z_axis,
    }
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul3(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = dot3(v, v).sqrt().max(1e-6);
    [v[0] / len, v[1] / len, v[2] / len]
}

fn point_to_segment_dist((px, py): (f32, f32), (ax, ay): (f32, f32), (bx, by): (f32, f32)) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    if len2 <= 1e-6 {
        return (px - ax).hypot(py - ay);
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
    (px - (ax + dx * t)).hypot(py - (ay + dy * t))
}
