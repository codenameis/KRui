//! `Develop` section Studio controller uniting the 2D Canva Canvas and Dual 3D Viewport.

use super::canvas_studio::CanvasStudioController;
use krui::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Combined controller for the `Develop` section (`Overview | 2D Canvas Studio | 3D Viewport`).
#[controller]
#[derive(Clone)]
pub struct StudioController {
    /// Active `Develop` sub-tab (`0=Overview, 1=2D Canvas Studio, 2=3D Viewport`).
    pub develop_subroute: Signal<i32>,
    /// Canva-style 2D vector shape editor controller.
    pub canvas: CanvasStudioController,
    /// Active 3D viewport mode (`0=Raw wgpu Direct, 1=Bevy 0.15 ECS`).
    pub engine_mode: Signal<i32>,
    /// Orbit yaw angle (degrees) driven by horizontal mouse drag or auto-spin.
    pub orbit_yaw: Signal<f32>,
    /// Orbit pitch angle (degrees) driven by vertical mouse drag (`-75..=75`).
    pub orbit_pitch: Signal<f32>,
    /// Camera distance from model center (`1.8..=7.0`).
    pub orbit_dist: Signal<f32>,
    /// Uniform model scale (`0.5..=2.0`).
    pub model_scale: Signal<f32>,
    /// Continuous gentle turntable spin flag.
    pub auto_spin: Signal<bool>,
    /// Shared open `.obj` model statistics label.
    pub model_stats: Signal<String>,
    /// Live engine & Bevy ECS inspector readout updated per frame.
    pub ecs_telemetry: Signal<String>,
    /// Formatted orbit camera status string.
    pub camera_readout: Computed<String>,
    /// Pointer drag orbit handlers + scroll zoom handler for the 3D viewport.
    pub on_3d_down: PointerHandler,
    pub on_3d_move: PointerHandler,
    pub on_3d_up: PointerHandler,
    pub on_3d_wheel: WheelHandler,
    dragging_flag: Signal<bool>,
}

#[controller]
impl StudioController {
    /// Switches between `0=Overview`, `1=2D Canvas Studio`, and `2=3D Viewport`.
    pub fn set_develop_subroute(&self, idx: i32) {
        self.develop_subroute.set(idx.clamp(0, 2));
    }

    /// Switches 3D engine backend (`0=Raw wgpu Direct`, `1=Bevy 0.15 ECS`).
    pub fn set_engine_mode(&self, mode: i32) {
        self.engine_mode.set(mode.clamp(0, 1));
    }

    /// Toggles automatic turntable rotation.
    pub fn toggle_auto_spin(&self) {
        self.auto_spin.update(|v| *v = !*v);
    }

    /// Resets 3D orbit camera angles, zoom distance, and model scale.
    pub fn reset_camera(&self) {
        let this = self.clone();
        batch(move || {
            this.orbit_yaw.set(35.0);
            this.orbit_pitch.set(18.0);
            this.orbit_dist.set(3.0);
            this.model_scale.set(1.0);
        });
    }

    /// Decreases uniform model scale by 0.1.
    pub fn scale_down(&self) {
        self.model_scale.update(|s| *s = (*s - 0.1).clamp(0.5, 2.0));
    }

    /// Increases uniform model scale by 0.1.
    pub fn scale_up(&self) {
        self.model_scale.update(|s| *s = (*s + 0.1).clamp(0.5, 2.0));
    }
}

fn build_orbit_handlers(
    orbit_yaw: Signal<f32>,
    orbit_pitch: Signal<f32>,
    orbit_dist: Signal<f32>,
    dragging_flag: Signal<bool>,
) -> (PointerHandler, PointerHandler, PointerHandler, WheelHandler) {
    let last_pt: Rc<RefCell<Option<(f32, f32)>>> = Rc::new(RefCell::new(None));
    let pt_down = last_pt.clone();
    let on_down = PointerHandler::new(move |ev: PointerEvent| {
        *pt_down.borrow_mut() = Some((ev.local_x, ev.local_y));
        dragging_flag.set(true);
    });
    let pt_move = last_pt.clone();
    let on_move = PointerHandler::new(move |ev: PointerEvent| {
        let mut guard = pt_move.borrow_mut();
        let Some((px, py)) = *guard else {
            return;
        };
        let (dx, dy) = (ev.local_x - px, ev.local_y - py);
        *guard = Some((ev.local_x, ev.local_y));
        batch(move || {
            orbit_yaw.update(|y| *y = (*y + dx * 0.65).rem_euclid(360.0));
            orbit_pitch.update(|p| *p = (*p + dy * 0.55).clamp(-75.0, 75.0));
        });
    });
    let on_up = PointerHandler::new(move |_| {
        *last_pt.borrow_mut() = None;
        dragging_flag.set(false);
    });
    let on_wheel = WheelHandler::new(move |ev: WheelEvent| {
        orbit_dist.update(|d| *d = (*d - ev.delta_y * 0.004).clamp(1.8, 7.0));
        true
    });
    (on_down, on_move, on_up, on_wheel)
}

impl StudioController {
    /// Creates a new [`StudioController`] with starter 2D board and default 3D orbit state.
    pub fn new() -> Self {
        let orbit_yaw = Signal::new(35.0_f32);
        let orbit_pitch = Signal::new(18.0_f32);
        let orbit_dist = Signal::new(3.0_f32);
        let model_scale = Signal::new(1.0_f32);
        let dragging_flag = Signal::new(false);
        let camera_readout = Computed::new(move || {
            format!(
                "Yaw {:.0}° · Pitch {:.0}° · Zoom {:.2} · Scale {:.2}x (드래그 회전 / 휠 줌)",
                orbit_yaw.get(),
                orbit_pitch.get(),
                orbit_dist.get(),
                model_scale.get()
            )
        });
        let (on_3d_down, on_3d_move, on_3d_up, on_3d_wheel) =
            build_orbit_handlers(orbit_yaw, orbit_pitch, orbit_dist, dragging_flag);
        Self {
            develop_subroute: Signal::new(0),
            canvas: CanvasStudioController::new(),
            engine_mode: Signal::new(0),
            orbit_yaw,
            orbit_pitch,
            orbit_dist,
            model_scale,
            auto_spin: Signal::new(true),
            model_stats: Signal::new(
                "Model: open_teapot.obj (Utah Teapot · 1,044v / 1,860 tris)".to_string(),
            ),
            ecs_telemetry: Signal::new(
                "Raw wgpu Direct (4x MSAA + Depth32Float) · GPU 대기 중".to_string(),
            ),
            camera_readout,
            on_3d_down,
            on_3d_move,
            on_3d_up,
            on_3d_wheel,
            dragging_flag,
        }
    }

    /// Advances turntable yaw slightly when auto-spin is enabled and pointer is not dragging.
    pub fn tick_auto_spin(&self, dt_secs: f32) -> bool {
        if !self.auto_spin.get_untracked() || self.dragging_flag.get_untracked() {
            return false;
        }
        self.orbit_yaw
            .update(|y| *y = (*y + dt_secs.clamp(0.001, 0.05) * 28.0).rem_euclid(360.0));
        true
    }
}
