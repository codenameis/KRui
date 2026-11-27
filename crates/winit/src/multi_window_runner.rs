//! Event-loop runner and per-window dispatch helpers for [`MultiWindowApp`].

use crate::ime_adapter::{
    ime_event_from_winit, sync_window_ime_cursor_area_cached, WinitImeStateCache,
};
use crate::multi_window::{KruiMultiWindowEvent, ManagedWindow, MultiWindowApp, SessionUiWaker};
use crate::session::EngineSession;
use crate::softbuffer_presenter::DesktopWindowPresenter;
use crate::window_helpers::{
    apply_window_cursor, compute_window_idle_control_flow, key_info_from_winit, mods_from_winit,
    pointer_button_from_winit, CARET_BLINK_IDLE_TIMEOUT, FRAME_PACE_120HZ,
};
use krui_core::{GpuResourceLimits, HoveredCursorKind, Mods};
use krui_signals::{RuntimeId, UiWaker};
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::{Window, WindowId};

pub(crate) struct MultiWindowRunner {
    pub(crate) app: MultiWindowApp,
    pub(crate) proxy: EventLoopProxy<KruiMultiWindowEvent>,
}

impl MultiWindowRunner {
    fn spawn_pending_windows(&mut self, event_loop: &ActiveEventLoop) {
        let specs = std::mem::take(&mut self.app.pending_specs);
        for spec in specs {
            let attrs = Window::default_attributes()
                .with_title(&spec.title)
                .with_visible(false)
                .with_inner_size(LogicalSize::new(spec.width, spec.height));
            let Ok(win) = event_loop.create_window(attrs) else {
                continue;
            };
            let mut session = EngineSession::new_isolated_with_shared_gpu(
                spec.width,
                spec.height,
                self.app.shared_gpu.clone(),
                self.app.preference,
            );
            let runtime_id = session.reactive_session.id();
            let waker: Arc<dyn UiWaker> = Arc::new(SessionUiWaker {
                runtime_id,
                proxy: self.proxy.clone(),
            });
            let build = spec.build;
            session.reactive_session.clone().enter(|| {
                krui_signals::set_ui_waker(Some(waker));
                build(&mut session);
            });
            self.finish_window_mount(win, session, runtime_id);
        }
    }

    fn finish_window_mount(
        &mut self,
        win: Window,
        mut session: EngineSession,
        runtime_id: RuntimeId,
    ) {
        let scale = win.scale_factor();
        session.set_scale_factor(scale);
        let phys = win.inner_size();
        let logical = phys.to_logical::<u32>(scale);
        session.width = logical.width.max(1);
        session.height = logical.height.max(1);
        let arc_win = Arc::new(win);
        let win_id = arc_win.id();
        let a11y_adapter =
            accesskit_winit::Adapter::with_event_loop_proxy(&arc_win, self.proxy.clone());
        arc_win.set_visible(true);
        let presenter = DesktopWindowPresenter::try_create(
            arc_win.clone(),
            &session,
            phys.width.max(1),
            phys.height.max(1),
            GpuResourceLimits::default(),
        );
        let mut ime_cache = WinitImeStateCache::default();
        sync_window_ime_cursor_area_cached(&arc_win, &session.tree, &mut ime_cache);
        arc_win.request_redraw();
        let now = Instant::now();
        self.app.runtime_to_window.insert(runtime_id, win_id);
        self.app.windows.insert(
            win_id,
            ManagedWindow {
                window: arc_win,
                session,
                presenter,
                a11y_adapter,
                cursor_pos: (0.0, 0.0),
                is_left_mouse_down: false,
                is_window_focused: true,
                mods: Mods::default(),
                cursor_kind: HoveredCursorKind::Default,
                ime_cache,
                epoch: now,
                last_frame: now,
                last_touch_time: now,
                last_caret_activity: now,
            },
        );
    }

    fn close_window(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId) {
        if let Some(mw) = self.app.windows.remove(&window_id) {
            let rid = mw.session.reactive_session.id();
            self.app.runtime_to_window.remove(&rid);
            let isolated_session = mw.session.reactive_session.clone();
            isolated_session.enter(|| krui_signals::set_ui_waker(None));
            drop(mw);
            isolated_session.reset();
        }
        if self.app.windows.is_empty() {
            event_loop.exit();
        }
    }
}

impl ApplicationHandler<KruiMultiWindowEvent> for MultiWindowRunner {
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        for mw in self.app.windows.values_mut() {
            mw.presenter = None;
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        for mw in self.app.windows.values_mut() {
            if mw.presenter.is_none() {
                let phys = mw.window.inner_size();
                mw.presenter = DesktopWindowPresenter::try_create(
                    mw.window.clone(),
                    &mw.session,
                    phys.width.max(1),
                    phys.height.max(1),
                    GpuResourceLimits::default(),
                );
                mw.window.request_redraw();
            }
        }
        self.spawn_pending_windows(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let mut earliest_until: Option<Instant> = None;
        for mw in self.app.windows.values_mut() {
            let is_input = mw.session.tree.focused_input_rect().is_some();
            let idle = now.duration_since(mw.last_caret_activity);
            if is_input {
                let blink_on = if mw.is_window_focused && idle < CARET_BLINK_IDLE_TIMEOUT {
                    (idle.as_millis() / 500).is_multiple_of(2)
                } else {
                    mw.is_window_focused
                };
                if mw.session.tree.set_caret_visible(blink_on) {
                    mw.window.request_redraw();
                }
            }
            let is_min = mw.window.is_minimized() == Some(true)
                || mw.window.inner_size().width == 0
                || mw.window.inner_size().height == 0;
            let drag = mw.is_left_mouse_down && mw.session.tree.is_text_drag_selecting();
            let has_anim = !is_min
                && (mw
                    .session
                    .reactive_session
                    .enter(|| mw.session.clock.has_active_animations())
                    || mw.session.tree.has_active_transitions()
                    || drag);
            let dirty = !is_min && mw.session.tree.is_paint_dirty();
            if dirty || (has_anim && now.duration_since(mw.last_frame) >= FRAME_PACE_120HZ) {
                mw.window.request_redraw();
            }
            if let ControlFlow::WaitUntil(t) = compute_window_idle_control_flow(
                has_anim,
                dirty,
                is_input,
                mw.is_window_focused,
                idle,
                false,
                now,
            ) {
                earliest_until = Some(earliest_until.map_or(t, |cur| cur.min(t)));
            }
        }
        event_loop
            .set_control_flow(earliest_until.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: KruiMultiWindowEvent) {
        match event {
            KruiMultiWindowEvent::WakeSession(rid) => {
                if let Some(&wid) = self.app.runtime_to_window.get(&rid) {
                    if let Some(mw) = self.app.windows.get_mut(&wid) {
                        mw.session
                            .reactive_session
                            .enter(|| _ = krui_signals::poll_async_tasks());
                        mw.sync_and_redraw();
                    }
                }
            }
            KruiMultiWindowEvent::AccessKit(ak) => handle_accesskit_event(self, ak),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::CloseRequested) {
            self.close_window(event_loop, window_id);
            return;
        }
        let Some(mw) = self.app.windows.get_mut(&window_id) else {
            return;
        };
        mw.a11y_adapter.process_event(&mw.window, &event);
        dispatch_managed_window_event(mw, event);
    }
}

fn handle_accesskit_event(runner: &mut MultiWindowRunner, ak: accesskit_winit::Event) {
    let Some(mw) = runner.app.windows.get_mut(&ak.window_id) else {
        return;
    };
    match ak.window_event {
        accesskit_winit::WindowEvent::InitialTreeRequested => {
            let tree = &mut mw.session.tree;
            mw.a11y_adapter
                .update_if_active(|| tree.build_a11y_tree_update());
        }
        accesskit_winit::WindowEvent::ActionRequested(req) => {
            let handled = mw
                .session
                .reactive_session
                .enter(|| mw.session.tree.dispatch_a11y_action(req));
            if handled {
                mw.sync_and_redraw();
            }
        }
        accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
    }
}

fn dispatch_managed_window_event(mw: &mut ManagedWindow, event: WindowEvent) {
    match event {
        WindowEvent::Resized(size) => {
            mw.apply_resize(size.width, size.height);
            mw.sync_and_redraw();
        }
        WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
            mw.session.set_scale_factor(scale_factor);
            mw.ime_cache.invalidate_cursor_area();
            let phys = mw.window.inner_size();
            mw.apply_resize(phys.width, phys.height);
            mw.sync_and_redraw();
        }
        WindowEvent::ModifiersChanged(mods) => mw.mods = mods_from_winit(&mods),
        WindowEvent::CursorMoved { position, .. } => {
            let l = position.to_logical::<f32>(mw.session.scale_factor);
            mw.handle_pointer_move(l.x, l.y);
        }
        WindowEvent::CursorLeft { .. } => {
            let tree = &mut mw.session.tree;
            if mw
                .session
                .reactive_session
                .enter(|| tree.dispatch_pointer_leave())
            {
                mw.window.request_redraw();
            }
        }
        WindowEvent::MouseWheel { delta, .. } => handle_managed_wheel(mw, delta),
        WindowEvent::MouseInput { state, button, .. } => {
            handle_managed_mouse_button(mw, button, state == ElementState::Pressed);
        }
        WindowEvent::Touch(touch) => mw.handle_touch(touch),
        WindowEvent::Focused(focused) => handle_managed_focus(mw, focused),
        WindowEvent::Ime(ime) => {
            mw.last_caret_activity = Instant::now();
            let tree = &mut mw.session.tree;
            mw.session
                .reactive_session
                .enter(|| _ = tree.dispatch_ime_event(ime_event_from_winit(ime)));
            mw.sync_and_redraw();
        }
        WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
            mw.last_caret_activity = Instant::now();
            let _ = mw
                .session
                .handle_key_down(&key_info_from_winit(&event), mw.mods, event.repeat);
            mw.sync_and_redraw();
        }
        WindowEvent::RedrawRequested => mw.handle_redraw(),
        _ => {}
    }
}

fn handle_managed_wheel(mw: &mut ManagedWindow, delta: MouseScrollDelta) {
    let (dx, dy) = match delta {
        MouseScrollDelta::LineDelta(x, y) => (x * 28.0, y * 28.0),
        MouseScrollDelta::PixelDelta(p) => {
            let l = p.to_logical::<f32>(mw.session.scale_factor);
            (l.x, l.y)
        }
    };
    let (cx, cy) = mw.cursor_pos;
    let mods = mw.mods;
    let tree = &mut mw.session.tree;
    if mw
        .session
        .reactive_session
        .enter(|| tree.dispatch_wheel(cx, cy, dx, dy, mods))
    {
        mw.window.request_redraw();
    }
}

fn handle_managed_mouse_button(mw: &mut ManagedWindow, button: MouseButton, pressed: bool) {
    let Some(ptr_btn) = pointer_button_from_winit(button) else {
        return;
    };
    let (cx, cy) = mw.cursor_pos;
    let mods = mw.mods;
    let ms = mw.epoch.elapsed().as_millis() as u64;
    if button == MouseButton::Left {
        mw.is_left_mouse_down = pressed;
    }
    if pressed {
        mw.last_caret_activity = Instant::now();
    }
    let tree = &mut mw.session.tree;
    mw.session
        .reactive_session
        .enter(|| _ = tree.dispatch_pointer_button(cx, cy, ptr_btn, pressed, mods, ms));
    let next_cursor = mw.session.tree.hovered_cursor_kind();
    if next_cursor != mw.cursor_kind {
        mw.cursor_kind = next_cursor;
        apply_window_cursor(&mw.window, next_cursor, false);
    }
    mw.sync_and_redraw();
}

fn handle_managed_focus(mw: &mut ManagedWindow, focused: bool) {
    mw.is_window_focused = focused;
    if focused {
        mw.last_caret_activity = Instant::now();
    } else {
        mw.is_left_mouse_down = false;
        mw.mods = Mods::default();
        let tree = &mut mw.session.tree;
        mw.session
            .reactive_session
            .enter(|| tree.dispatch_window_focus_lost());
    }
    let _ = mw.session.tree.set_caret_visible(focused);
    mw.sync_and_redraw();
}
