//! Desktop `winit` window event loop (`EventLoop::<KruiWinitEvent>`), `UiWaker` registration,
//! `softbuffer` CPU fallback presenter, and live `accesskit_winit::Adapter` integration.

use crate::ime_adapter::{ime_event_from_winit, WinitImeStateCache};
#[cfg(feature = "preview")]
use crate::preview;
use crate::session::EngineSession;
use crate::softbuffer_presenter::DesktopWindowPresenter;
pub use crate::window_helpers::{
    apply_key_action, compute_idle_control_flow, compute_window_idle_control_flow,
    key_info_from_logical_key, key_info_from_winit, touch_phase_from_winit, KruiWinitEvent,
    WindowEventHooks, CARET_BLINK_IDLE_TIMEOUT, FRAME_PACE_120HZ,
};
use crate::window_helpers::{
    map_winit_theme, mods_from_winit, run_headless_window_session, NoopWindowHooks,
    WinitProxyUiWaker,
};
use krui_core::{GpuResourceLimits, HoveredCursorKind, Mods};
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

/// Launches an interactive native desktop window for `session` (or runs headless verification
/// if `--headless` is passed on the command line).
pub fn run_app_window(title: &str, session: EngineSession, hot_reload_krui_path: Option<&str>) {
    run_app_window_with_hooks(title, session, hot_reload_krui_path, NoopWindowHooks);
}

/// Launches an interactive native desktop window with custom [`WindowEventHooks`].
pub fn run_app_window_with_hooks<H: WindowEventHooks>(
    title: &str,
    mut session: EngineSession,
    _hot_reload_krui_path: Option<&str>,
    hooks: H,
) {
    if std::env::args().any(|a| a == "--headless") {
        run_headless_window_session(title, &mut session);
        return;
    }
    #[cfg(feature = "preview")]
    if crate::embed_panel::is_embed_panel_mode() {
        use crate::embed_panel::run_embedded_panel_session as run_panel;
        run_panel(title, session, _hot_reload_krui_path, hooks);
        return;
    }

    let Ok(event_loop) = EventLoop::<KruiWinitEvent>::with_user_event().build() else {
        let _ = session.step_frame(1.0 / 120.0);
        return;
    };
    let proxy = event_loop.create_proxy();
    session.reactive_session.enter(|| {
        krui_signals::set_ui_waker(Some(Arc::new(WinitProxyUiWaker::new(proxy.clone()))));
    });

    #[cfg(feature = "preview")]
    let hot_reloader = preview::init_window_hot_reloader(_hot_reload_krui_path, &proxy);
    let now = Instant::now();
    let mut host = StandardWindowHost {
        base_title: title.to_string(),
        session,
        hooks,
        window: None,
        presenter: None,
        a11y_adapter: None,
        proxy,
        cursor_pos: (0.0, 0.0),
        is_left_mouse_down: false,
        hook_pointer_buttons: 0,
        is_window_focused: true,
        mods: Mods::default(),
        current_cursor_kind: HoveredCursorKind::Default,
        pointer_lock_active: false,
        ime_cache: WinitImeStateCache::default(),
        epoch: now,
        last_frame: now,
        last_touch_time: now,
        last_caret_activity: now,
        #[cfg(feature = "preview")]
        last_hot_reload_check: now,
        #[cfg(feature = "preview")]
        hot_reloader,
    };
    let _ = event_loop.run_app(&mut host);
    host.session
        .reactive_session
        .enter(|| krui_signals::set_ui_waker(None));
}

pub(crate) struct StandardWindowHost<H: WindowEventHooks> {
    pub(crate) base_title: String,
    pub(crate) session: EngineSession,
    pub(crate) hooks: H,
    pub(crate) window: Option<Arc<Window>>,
    pub(crate) presenter: Option<DesktopWindowPresenter>,
    pub(crate) a11y_adapter: Option<accesskit_winit::Adapter>,
    pub(crate) proxy: EventLoopProxy<KruiWinitEvent>,
    pub(crate) cursor_pos: (f32, f32),
    pub(crate) is_left_mouse_down: bool,
    pub(crate) hook_pointer_buttons: u8,
    pub(crate) is_window_focused: bool,
    pub(crate) mods: Mods,
    pub(crate) current_cursor_kind: HoveredCursorKind,
    pub(crate) pointer_lock_active: bool,
    pub(crate) ime_cache: WinitImeStateCache,
    pub(crate) epoch: Instant,
    pub(crate) last_frame: Instant,
    pub(crate) last_touch_time: Instant,
    pub(crate) last_caret_activity: Instant,
    #[cfg(feature = "preview")]
    pub(crate) last_hot_reload_check: Instant,
    #[cfg(feature = "preview")]
    pub(crate) hot_reloader: Option<preview::DevFileWatcher>,
}

impl<H: WindowEventHooks> StandardWindowHost<H> {
    pub(crate) fn apply_hook_effect(&mut self, effect: Option<Option<String>>) {
        let Some(_new_path_opt) = effect else {
            return;
        };
        #[cfg(feature = "preview")]
        if let Some(new_path) = _new_path_opt {
            if new_path == "__force_reload__" {
                if let Some(w) = &mut self.hot_reloader {
                    let tree = &mut self.session.tree;
                    self.session.reactive_session.enter(|| {
                        let _ = w.force_reload(tree);
                    });
                }
            } else if !new_path.is_empty() {
                self.hot_reloader = preview::init_window_hot_reloader(Some(&new_path), &self.proxy);
            }
        }
        self.sync_and_redraw();
    }

    fn handle_key_pressed(&mut self, event: KeyEvent) {
        self.last_caret_activity = Instant::now();
        let key_info = key_info_from_winit(&event);
        let rs = self.session.reactive_session.clone();
        if let Some(hook_effect) = rs.enter(|| {
            self.hooks
                .on_key_down(&mut self.session, &key_info, self.mods)
        }) {
            self.apply_hook_effect(Some(hook_effect));
            return;
        }
        let _ = self
            .session
            .handle_key_down(&key_info, self.mods, event.repeat);
        self.sync_window_cursor_and_pointer_lock();
        self.sync_and_redraw();
    }

    fn handle_redraw(&mut self) {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_frame)
            .as_secs_f32()
            .clamp(1.0 / 240.0, 0.05);
        self.last_frame = now;
        let frame_start = Instant::now();
        #[cfg(feature = "preview")]
        let (lw, lh) = self
            .hot_reloader
            .as_ref()
            .and_then(|w| w.device_frame())
            .map_or((self.session.width, self.session.height), |f| {
                f.safe_area_layout_size()
            });
        #[cfg(not(feature = "preview"))]
        let (lw, lh) = (self.session.width, self.session.height);
        let is_drag_sel = self.is_left_mouse_down;
        let session = &mut self.session;
        session.reactive_session.enter(|| {
            let _ = session.tree.sync_viewport_and_media(lw as f32, lh as f32);
            if session.clock.has_active_animations() {
                let _ = session.clock.tick(dt);
            }
            if session.tree.has_active_transitions() {
                let _ = session.tree.tick_transitions(dt);
            }
            if is_drag_sel && session.tree.is_text_drag_selecting() {
                let _ = session.tree.tick_pointer_drag_autoscroll(dt);
            }
        });
        let mut scene = self.build_frame_scene(frame_start);
        self.hooks.on_before_present(&mut self.session, dt);
        if self.session.tree.is_paint_dirty() {
            scene = self.build_frame_scene(frame_start);
        }
        if let Some(presenter) = &mut self.presenter {
            let _ = presenter.present_scene(&scene);
        } else {
            let _ = self.session.renderer.render_scene(&scene);
        }
        self.sync_ime_only();
        if let Some(adapter) = &mut self.a11y_adapter {
            adapter.update_if_active(|| self.session.tree.build_a11y_tree_update());
        }
        self.update_window_title_after_frame(frame_start.elapsed().as_micros());
    }

    fn build_frame_scene(&mut self, _frame_start: Instant) -> krui_core::PaintScene {
        #[cfg(feature = "preview")]
        {
            let mut scene = self.build_preview_or_standard_scene();
            if let Some(watcher) = &mut self.hot_reloader {
                let build_us = _frame_start.elapsed().as_micros();
                watcher.decorate_paint_scene(&self.session.tree, &mut scene, build_us);
            }
            scene
        }
        #[cfg(not(feature = "preview"))]
        {
            self.session
                .reactive_session
                .enter(|| {
                    self.session
                        .tree
                        .build_paint_scene(self.session.width, self.session.height)
                })
                .0
        }
    }

    #[cfg(feature = "preview")]
    fn build_preview_or_standard_scene(&mut self) -> krui_core::PaintScene {
        let (lw, lh) = self
            .hot_reloader
            .as_ref()
            .and_then(|w| w.device_frame())
            .map_or((self.session.width, self.session.height), |f| {
                f.safe_area_layout_size()
            });
        let tree = &mut self.session.tree;
        self.session
            .reactive_session
            .enter(|| tree.build_paint_scene(lw, lh).0)
    }

    fn update_window_title_after_frame(&mut self, frame_us: u128) {
        #[cfg(feature = "preview")]
        let reload_count = self.hot_reloader.as_ref().map_or(0, |w| w.reload_count());
        #[cfg(not(feature = "preview"))]
        let reload_count = 0_u64;
        if let Some(new_title) = self
            .hooks
            .on_after_frame(&self.session, frame_us, reload_count)
        {
            if let Some(win) = &self.window {
                win.set_title(&new_title);
            }
        }
    }

    fn poll_preview_watcher_if_due(&mut self, _now: Instant) {
        #[cfg(feature = "preview")]
        if _now.duration_since(self.last_hot_reload_check).as_millis() >= 150 {
            self.last_hot_reload_check = _now;
            if let Some(watcher) = &mut self.hot_reloader {
                let tree = &mut self.session.tree;
                let reloaded = self
                    .session
                    .reactive_session
                    .enter(|| watcher.poll_and_patch(tree).is_some());
                if reloaded {
                    let tick_effect = self.hooks.on_tick(&mut self.session);
                    self.apply_hook_effect(tick_effect);
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
            }
        }
    }
}

impl<H: WindowEventHooks> StandardWindowHost<H> {
    fn ensure_presenter_for_window(&mut self, arc_win: &Arc<Window>) {
        if self.presenter.is_none() {
            let phys = arc_win.inner_size();
            self.presenter = DesktopWindowPresenter::try_create(
                arc_win.clone(),
                &self.session,
                phys.width.max(1),
                phys.height.max(1),
                GpuResourceLimits::default(),
            );
        }
    }
}

impl<H: WindowEventHooks> ApplicationHandler<KruiWinitEvent> for StandardWindowHost<H> {
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.presenter = None;
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(arc_win) = self.window.clone() {
            self.ensure_presenter_for_window(&arc_win);
            arc_win.request_redraw();
            return;
        }
        #[cfg(feature = "preview")]
        if let Some(frame) = self.hot_reloader.as_ref().and_then(|w| w.device_frame()) {
            let (ow, oh) = frame.outer_window_size();
            self.session.width = ow;
            self.session.height = oh;
        }
        let attrs = Window::default_attributes()
            .with_title(&self.base_title)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(self.session.width, self.session.height));
        let Ok(win) = event_loop.create_window(attrs) else {
            return;
        };
        let scale_factor = win.scale_factor();
        self.session.set_scale_factor(scale_factor);
        let phys_size = win.inner_size();
        let logical = phys_size.to_logical::<u32>(scale_factor);
        self.session.width = logical.width.max(1);
        self.session.height = logical.height.max(1);
        if let Some(theme) = win.theme() {
            self.session
                .tree
                .set_system_color_scheme(map_winit_theme(theme));
        }
        let _ = self
            .session
            .tree
            .sync_viewport_and_media(self.session.width as f32, self.session.height as f32);

        let arc_win = Arc::new(win);
        let a11y_adapter =
            accesskit_winit::Adapter::with_event_loop_proxy(&arc_win, self.proxy.clone());
        arc_win.set_visible(true);
        self.presenter = DesktopWindowPresenter::try_create(
            arc_win.clone(),
            &self.session,
            phys_size.width.max(1),
            phys_size.height.max(1),
            GpuResourceLimits::default(),
        );
        self.a11y_adapter = Some(a11y_adapter);
        self.window = Some(arc_win);
        self.sync_and_redraw();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let tick_effect = self.hooks.on_tick(&mut self.session);
        self.apply_hook_effect(tick_effect);

        let now = Instant::now();
        self.poll_preview_watcher_if_due(now);

        let is_input_focused = self.session.tree.focused_input_rect().is_some();
        let idle_since_input = now.duration_since(self.last_caret_activity);
        if is_input_focused {
            let blink_on = if self.is_window_focused && idle_since_input < CARET_BLINK_IDLE_TIMEOUT
            {
                (idle_since_input.as_millis() / 500).is_multiple_of(2)
            } else {
                self.is_window_focused
            };
            if self.session.tree.set_caret_visible(blink_on) {
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            }
        }

        #[cfg(feature = "preview")]
        let has_watcher = self
            .hot_reloader
            .as_ref()
            .is_some_and(|w| w.needs_periodic_poll());
        #[cfg(not(feature = "preview"))]
        let has_watcher = false;

        let is_drag_autoscrolling =
            self.is_left_mouse_down && self.session.tree.is_text_drag_selecting();
        let has_anim = self
            .session
            .reactive_session
            .enter(|| self.session.clock.has_active_animations())
            || self.session.tree.has_active_transitions()
            || is_drag_autoscrolling;
        let paint_dirty = self.session.tree.is_paint_dirty();
        if paint_dirty || (has_anim && now.duration_since(self.last_frame) >= FRAME_PACE_120HZ) {
            if let Some(win) = &self.window {
                win.request_redraw();
            }
        }
        let flow = compute_window_idle_control_flow(
            has_anim,
            paint_dirty,
            is_input_focused,
            self.is_window_focused,
            idle_since_input,
            has_watcher,
            now,
        );
        event_loop.set_control_flow(flow);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: KruiWinitEvent) {
        match event {
            KruiWinitEvent::WakeUi => {
                self.session.reactive_session.enter(|| {
                    let _ = krui_signals::poll_async_tasks();
                });
                #[cfg(feature = "preview")]
                if let Some(watcher) = &mut self.hot_reloader {
                    let tree = &mut self.session.tree;
                    let reloaded = self
                        .session
                        .reactive_session
                        .enter(|| watcher.poll_and_patch(tree).is_some());
                    if reloaded {
                        let tick_effect = self.hooks.on_tick(&mut self.session);
                        self.apply_hook_effect(tick_effect);
                    }
                }
                let tick_effect = self.hooks.on_tick(&mut self.session);
                self.apply_hook_effect(tick_effect);
                self.sync_and_redraw();
            }
            KruiWinitEvent::AccessKit(ak_event) => match ak_event.window_event {
                accesskit_winit::WindowEvent::InitialTreeRequested => {
                    if let Some(adapter) = &mut self.a11y_adapter {
                        adapter.update_if_active(|| self.session.tree.build_a11y_tree_update());
                    }
                }
                accesskit_winit::WindowEvent::ActionRequested(req) => {
                    let tree = &mut self.session.tree;
                    let handled = self
                        .session
                        .reactive_session
                        .enter(|| tree.dispatch_a11y_action(req));
                    if handled {
                        let tick_effect = self.hooks.on_tick(&mut self.session);
                        self.apply_hook_effect(tick_effect);
                        self.sync_and_redraw();
                    }
                }
                accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
            },
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if let (Some(win), Some(adapter)) = (&self.window, &mut self.a11y_adapter) {
            adapter.process_event(win, &event);
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.apply_resize(size.width, size.height);
                self.sync_and_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.session.set_scale_factor(scale_factor);
                self.ime_cache.invalidate_cursor_area();
                if let Some(size) = self.window.as_ref().map(|w| w.inner_size()) {
                    self.apply_resize(size.width, size.height);
                }
                self.sync_and_redraw();
            }
            WindowEvent::ModifiersChanged(mods) => self.mods = mods_from_winit(&mods),
            WindowEvent::CursorMoved { position, .. } => self.handle_cursor_moved(position),
            WindowEvent::CursorLeft { .. } => self.handle_cursor_left(),
            WindowEvent::MouseWheel { delta, .. } => self.handle_mouse_wheel(delta),
            WindowEvent::MouseInput { state, button, .. } => {
                self.handle_mouse_button(button, state == ElementState::Pressed);
            }
            WindowEvent::Touch(touch) => self.handle_touch_event(touch),
            WindowEvent::Focused(focused) => self.handle_focus_change(focused),
            WindowEvent::Ime(ime_ev) => {
                self.last_caret_activity = Instant::now();
                let tree = &mut self.session.tree;
                self.session.reactive_session.enter(|| {
                    let _ = tree.dispatch_ime_event(ime_event_from_winit(ime_ev));
                });
                self.sync_and_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.handle_key_pressed(event)
            }
            WindowEvent::ThemeChanged(theme) => {
                self.session
                    .tree
                    .set_system_color_scheme(map_winit_theme(theme));
                self.sync_and_redraw();
            }
            WindowEvent::RedrawRequested => self.handle_redraw(),
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _: &ActiveEventLoop,
        _: winit::event::DeviceId,
        ev: winit::event::DeviceEvent,
    ) {
        if let winit::event::DeviceEvent::MouseMotion { delta: (dx, dy) } = ev {
            self.handle_device_mouse_motion(dx, dy);
        }
    }
}
