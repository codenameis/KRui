//! Shared-GPU multi-window host (`MultiWindowApp`, `WindowSpec`) with isolated per-window `ReactiveSession`s.

use crate::ime_adapter::{sync_window_ime_cursor_area_cached, WinitImeStateCache};
use crate::multi_window_runner::MultiWindowRunner;
use crate::session::EngineSession;
use crate::softbuffer_presenter::DesktopWindowPresenter;
use crate::window_helpers::{cursor_icon_from_kind, touch_phase_from_winit};
use krui_core::{HoveredCursorKind, Mods, RenderBackendPreference};
use krui_renderer::GpuContext;
use krui_signals::{RuntimeId, UiWaker};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use winit::event::Touch;
use winit::event_loop::{EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

/// User event envelope routing per-session async wakes and AccessKit events across multiple windows.
#[derive(Debug)]
pub enum KruiMultiWindowEvent {
    /// Wakes only the window owning `RuntimeId`.
    WakeSession(RuntimeId),
    /// AccessKit accessibility event tagged with its destination `WindowId`.
    AccessKit(accesskit_winit::Event),
}

impl From<accesskit_winit::Event> for KruiMultiWindowEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::AccessKit(event)
    }
}

pub(crate) struct SessionUiWaker {
    pub(crate) runtime_id: RuntimeId,
    pub(crate) proxy: EventLoopProxy<KruiMultiWindowEvent>,
}

impl UiWaker for SessionUiWaker {
    fn wake(&self) {
        let _ = self
            .proxy
            .send_event(KruiMultiWindowEvent::WakeSession(self.runtime_id));
    }
}

/// Declarative launch specification for one native window inside [`MultiWindowApp`].
pub struct WindowSpec {
    /// Window title displayed in the OS title bar.
    pub title: String,
    /// Initial logical width in pixels.
    pub width: u32,
    /// Initial logical height in pixels.
    pub height: u32,
    pub(crate) build: Box<dyn FnOnce(&mut EngineSession)>,
}

impl WindowSpec {
    /// Creates a new [`WindowSpec`] that populates its isolated [`EngineSession`] via `build`.
    pub fn new(
        title: impl Into<String>,
        width: u32,
        height: u32,
        build: impl FnOnce(&mut EngineSession) + 'static,
    ) -> Self {
        Self {
            title: title.into(),
            width: width.max(1),
            height: height.max(1),
            build: Box::new(build),
        }
    }
}

pub(crate) struct ManagedWindow {
    pub(crate) window: Arc<Window>,
    pub(crate) session: EngineSession,
    pub(crate) presenter: Option<DesktopWindowPresenter>,
    pub(crate) a11y_adapter: accesskit_winit::Adapter,
    pub(crate) cursor_pos: (f32, f32),
    pub(crate) is_left_mouse_down: bool,
    pub(crate) is_window_focused: bool,
    pub(crate) mods: Mods,
    pub(crate) cursor_kind: HoveredCursorKind,
    pub(crate) ime_cache: WinitImeStateCache,
    pub(crate) epoch: Instant,
    pub(crate) last_frame: Instant,
    pub(crate) last_touch_time: Instant,
    pub(crate) last_caret_activity: Instant,
}

impl ManagedWindow {
    pub(crate) fn sync_and_redraw(&mut self) {
        sync_window_ime_cursor_area_cached(&self.window, &self.session.tree, &mut self.ime_cache);
        self.window.request_redraw();
    }

    pub(crate) fn apply_resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let logical = winit::dpi::PhysicalSize::new(width, height)
            .to_logical::<u32>(self.session.scale_factor);
        self.session.width = logical.width.max(1);
        self.session.height = logical.height.max(1);
        if let Some(presenter) = &mut self.presenter {
            presenter.resize(width, height);
        }
    }

    pub(crate) fn handle_redraw(&mut self) {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_frame)
            .as_secs_f32()
            .clamp(1.0 / 240.0, 0.05);
        self.last_frame = now;
        let is_drag = self.is_left_mouse_down;
        let session = &mut self.session;
        let (w, h) = (session.width, session.height);
        let scene = session.reactive_session.enter(|| {
            if session.clock.has_active_animations() {
                let _ = session.clock.tick(dt);
            }
            if session.tree.has_active_transitions() {
                let _ = session.tree.tick_transitions(dt);
            }
            if is_drag && session.tree.is_text_drag_selecting() {
                let _ = session.tree.tick_pointer_drag_autoscroll(dt);
            }
            session.tree.build_paint_scene(w, h).0
        });
        if let Some(presenter) = &mut self.presenter {
            let _ = presenter.present_scene(&scene);
        } else {
            let _ = session.renderer.render_scene(&scene);
        }
        sync_window_ime_cursor_area_cached(&self.window, &session.tree, &mut self.ime_cache);
        self.a11y_adapter
            .update_if_active(|| session.tree.build_a11y_tree_update());
    }

    pub(crate) fn handle_touch(&mut self, touch: Touch) {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_touch_time)
            .as_secs_f32()
            .clamp(0.0001, 0.05);
        self.last_touch_time = now;
        let scale = self.session.scale_factor.max(1.0);
        let x = (touch.location.x / scale) as f32;
        let y = (touch.location.y / scale) as f32;
        let phase = touch_phase_from_winit(touch.phase);
        if phase == krui_core::TouchPhaseKind::Started {
            self.last_caret_activity = now;
        }
        let tree = &mut self.session.tree;
        let clock = &mut self.session.clock;
        self.session.reactive_session.enter(|| {
            let _ = tree.dispatch_touch(clock, touch.id, phase, x, y, dt);
        });
        self.sync_and_redraw();
    }

    pub(crate) fn handle_pointer_move(&mut self, lx: f32, ly: f32) {
        self.cursor_pos = (lx, ly);
        let is_down = self.is_left_mouse_down;
        if is_down {
            self.last_caret_activity = Instant::now();
        }
        let tree = &mut self.session.tree;
        let (drag, hover, next_cursor) = self.session.reactive_session.enter(|| {
            let d = is_down && tree.dispatch_pointer_drag(lx, ly);
            let h = tree.dispatch_pointer_move(lx, ly);
            (d, h, tree.hovered_cursor_kind())
        });
        if next_cursor != self.cursor_kind {
            self.cursor_kind = next_cursor;
            self.window.set_cursor(cursor_icon_from_kind(next_cursor));
        }
        if drag || hover {
            self.window.request_redraw();
        }
    }
}

/// Multi-window desktop application host sharing one `Arc<GpuContext>` across isolated `EngineSession`s.
pub struct MultiWindowApp {
    pub(crate) shared_gpu: Option<Arc<GpuContext>>,
    pub(crate) preference: RenderBackendPreference,
    pub(crate) pending_specs: Vec<WindowSpec>,
    pub(crate) windows: HashMap<WindowId, ManagedWindow>,
    pub(crate) runtime_to_window: HashMap<RuntimeId, WindowId>,
}

impl MultiWindowApp {
    /// Creates a multi-window host probing at most one shared `Arc<GpuContext>`.
    pub fn new(preference: RenderBackendPreference) -> Self {
        let shared_gpu = match preference {
            RenderBackendPreference::ForceTinySkia | RenderBackendPreference::CpuFallback => None,
            RenderBackendPreference::Auto | RenderBackendPreference::ForceWgpu => {
                GpuContext::try_new()
            }
        };
        Self {
            shared_gpu,
            preference,
            pending_specs: Vec::new(),
            windows: HashMap::new(),
            runtime_to_window: HashMap::new(),
        }
    }

    /// Returns the shared `Arc<GpuContext>` if GPU initialization succeeded.
    pub fn shared_gpu_context(&self) -> Option<Arc<GpuContext>> {
        self.shared_gpu.clone()
    }

    /// Queues a window specification to be spawned when the event loop starts or headlessly built.
    pub fn add_window(&mut self, spec: WindowSpec) {
        self.pending_specs.push(spec);
    }

    /// Instantiates all queued windows into isolated headless [`EngineSession`]s sharing `shared_gpu`.
    pub fn build_headless_sessions(&mut self) -> Vec<EngineSession> {
        let specs = std::mem::take(&mut self.pending_specs);
        specs
            .into_iter()
            .map(|spec| {
                let mut session = EngineSession::new_isolated_with_shared_gpu(
                    spec.width,
                    spec.height,
                    self.shared_gpu.clone(),
                    self.preference,
                );
                let build = spec.build;
                session
                    .reactive_session
                    .clone()
                    .enter(|| build(&mut session));
                session
            })
            .collect()
    }

    /// Runs the native multi-window event loop until all windows are closed.
    pub fn run(mut self) {
        let Ok(event_loop) = EventLoop::<KruiMultiWindowEvent>::with_user_event().build() else {
            let _ = self.build_headless_sessions();
            return;
        };
        let proxy = event_loop.create_proxy();
        let mut runner = MultiWindowRunner { app: self, proxy };
        let _ = event_loop.run_app(&mut runner);
    }
}
