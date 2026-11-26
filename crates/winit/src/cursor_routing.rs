//! Pointer routing helpers for `StandardWindowHost` (pointer capture bezel mapping,
//! `CursorLeft`, multi-click timestamps, mouse wheel, and drag release).

use crate::ime_adapter::sync_window_ime_cursor_area_cached_with_offset;
use crate::window_helpers::{apply_window_cursor, mouse_button_code, pointer_button_from_winit};
use crate::window_loop::{StandardWindowHost, WindowEventHooks};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{MouseButton, MouseScrollDelta};
use winit::window::CursorGrabMode;

impl<H: WindowEventHooks> StandardWindowHost<H> {
    fn current_ime_content_origin(&self) -> (f32, f32) {
        #[cfg(feature = "preview")]
        {
            self.hot_reloader
                .as_ref()
                .and_then(|w| w.device_frame())
                .map_or((0.0, 0.0), |f| f.content_origin())
        }
        #[cfg(not(feature = "preview"))]
        {
            (0.0, 0.0)
        }
    }

    pub(crate) fn sync_ime_only(&mut self) {
        let offset = self.current_ime_content_origin();
        if let Some(win) = &self.window {
            sync_window_ime_cursor_area_cached_with_offset(
                win,
                &self.session.tree,
                &mut self.ime_cache,
                offset,
            );
        }
    }

    pub(crate) fn sync_and_redraw(&mut self) {
        self.sync_ime_only();
        if let Some(win) = &self.window {
            win.request_redraw();
        }
    }

    pub(crate) fn apply_resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            let logical =
                PhysicalSize::new(width, height).to_logical::<u32>(self.session.scale_factor);
            self.session.width = logical.width.max(1);
            self.session.height = logical.height.max(1);
            let _ = self
                .session
                .tree
                .sync_viewport_and_media(self.session.width as f32, self.session.height as f32);
            if let Some(presenter) = &mut self.presenter {
                presenter.resize(width, height);
            }
        }
    }
    pub(crate) fn sync_window_cursor_and_pointer_lock(&mut self) {
        let want_lock = self.is_window_focused && self.session.tree.pointer_lock_node().is_some();
        if want_lock != self.pointer_lock_active {
            self.pointer_lock_active = want_lock;
            if let Some(win) = &self.window {
                if want_lock {
                    if win.set_cursor_grab(CursorGrabMode::Locked).is_err() {
                        let _ = win.set_cursor_grab(CursorGrabMode::Confined);
                    }
                    win.set_cursor_visible(false);
                } else {
                    let _ = win.set_cursor_grab(CursorGrabMode::None);
                    win.set_cursor_visible(true);
                }
            }
        }
        let next_cursor = self.session.tree.hovered_cursor_kind();
        if !self.pointer_lock_active && next_cursor != self.current_cursor_kind {
            self.current_cursor_kind = next_cursor;
            if let Some(win) = &self.window {
                apply_window_cursor(win, next_cursor, false);
            }
        }
    }

    pub(crate) fn handle_device_mouse_motion(&mut self, dx: f64, dy: f64) {
        if !self.is_window_focused || !self.pointer_lock_active {
            return;
        }
        let mods = self.mods;
        let tree = &mut self.session.tree;
        if self
            .session
            .reactive_session
            .enter(|| tree.dispatch_relative_pointer_motion(dx as f32, dy as f32, mods))
        {
            if let Some(win) = &self.window {
                win.request_redraw();
            }
        }
    }
    pub(crate) fn map_logical_point_to_tree(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        #[cfg(feature = "preview")]
        if let Some(frame) = self.hot_reloader.as_ref().and_then(|w| w.device_frame()) {
            return frame.map_window_to_screen_point(x, y);
        }
        Some((x, y))
    }

    pub(crate) fn map_cursor_to_tree(&self) -> Option<(f32, f32)> {
        self.map_logical_point_to_tree(self.cursor_pos.0, self.cursor_pos.1)
    }

    pub(crate) fn map_cursor_for_capture_or_hover(&self) -> Option<(f32, f32)> {
        if self.session.tree.captured_pointer_node().is_some() {
            #[cfg(feature = "preview")]
            if let Some(frame) = self.hot_reloader.as_ref().and_then(|w| w.device_frame()) {
                return Some(
                    frame
                        .map_window_to_screen_point_unclamped(self.cursor_pos.0, self.cursor_pos.1),
                );
            }
            return Some(self.cursor_pos);
        }
        self.map_cursor_to_tree()
    }

    pub(crate) fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        let logical = position.to_logical::<f32>(self.session.scale_factor);
        self.cursor_pos = (logical.x, logical.y);
        if self.pointer_lock_active {
            return;
        }
        let mapped = self.map_cursor_for_capture_or_hover();
        let is_down = self.is_left_mouse_down;
        if is_down {
            self.last_caret_activity = std::time::Instant::now();
        }
        let mods = self.mods;
        let tree = &mut self.session.tree;
        let (drag_changed, hover_changed) = self.session.reactive_session.enter(|| {
            let drag = is_down && mapped.is_some_and(|(mx, my)| tree.dispatch_pointer_drag(mx, my));
            let hover = match mapped {
                Some((mx, my)) => tree.dispatch_pointer_move_with_mods(mx, my, mods),
                None => tree.dispatch_pointer_move_with_mods(-10_000.0, -10_000.0, mods),
            };
            (drag, hover)
        });
        let hook_changed = mapped.is_some_and(|(mx, my)| {
            self.hooks
                .on_pointer_moved(&mut self.session, mx, my, self.mods)
        });
        self.sync_window_cursor_and_pointer_lock();
        if drag_changed || hover_changed || hook_changed {
            if let Some(win) = &self.window {
                win.request_redraw();
            }
        }
    }

    pub(crate) fn handle_cursor_left(&mut self) {
        if self.pointer_lock_active {
            return;
        }
        let tree = &mut self.session.tree;
        if self
            .session
            .reactive_session
            .enter(|| tree.dispatch_pointer_leave())
        {
            if let Some(win) = &self.window {
                win.request_redraw();
            }
        }
        self.sync_window_cursor_and_pointer_lock();
    }

    pub(crate) fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta) {
        let (dx, dy) = match delta {
            MouseScrollDelta::LineDelta(x, y) => (x * 28.0, y * 28.0),
            MouseScrollDelta::PixelDelta(pos) => {
                let l = pos.to_logical::<f32>(self.session.scale_factor);
                (l.x, l.y)
            }
        };
        if let Some((mx, my)) = self.map_cursor_to_tree() {
            if let Some(hook_effect) =
                self.hooks
                    .on_mouse_wheel(&mut self.session, mx, my, dy, self.mods)
            {
                self.apply_hook_effect(Some(hook_effect));
                return;
            }
            let mods = self.mods;
            let tree = &mut self.session.tree;
            if self
                .session
                .reactive_session
                .enter(|| tree.dispatch_wheel(mx, my, dx, dy, mods))
            {
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            }
        }
    }

    pub(crate) fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        let Some(code) = mouse_button_code(button) else {
            return;
        };
        let Some(ptr_btn) = pointer_button_from_winit(button) else {
            return;
        };
        if pressed {
            self.last_caret_activity = std::time::Instant::now();
        }
        let hook_pos = if pressed {
            self.map_cursor_to_tree()
        } else {
            self.map_cursor_for_capture_or_hover()
                .or_else(|| self.map_cursor_to_tree())
        };
        if let Some((mx, my)) = hook_pos {
            if let Some(hook_effect) =
                self.hooks
                    .on_pointer_button(&mut self.session, mx, my, code, pressed, self.mods)
            {
                self.apply_hook_effect(Some(hook_effect));
                return;
            }
        }
        if code == 0 {
            if pressed {
                self.handle_left_mouse_pressed();
            } else {
                self.handle_left_mouse_released();
            }
            self.sync_window_cursor_and_pointer_lock();
            return;
        }
        let timestamp_ms = self.epoch.elapsed().as_millis() as u64;
        if let Some((mx, my)) = hook_pos {
            let mods = self.mods;
            let tree = &mut self.session.tree;
            self.session.reactive_session.enter(|| {
                let _ = tree.dispatch_pointer_button(mx, my, ptr_btn, pressed, mods, timestamp_ms);
            });
            let tick_effect = self.hooks.on_tick(&mut self.session);
            self.apply_hook_effect(tick_effect);
            self.sync_window_cursor_and_pointer_lock();
            self.sync_and_redraw();
        }
    }

    pub(crate) fn handle_left_mouse_pressed(&mut self) {
        self.is_left_mouse_down = true;
        self.last_caret_activity = std::time::Instant::now();
        let timestamp_ms = self.epoch.elapsed().as_millis() as u64;
        if let Some((mx, my)) = self.map_cursor_to_tree() {
            let mods = self.mods;
            let tree = &mut self.session.tree;
            self.session.reactive_session.enter(|| {
                let _ = tree.dispatch_pointer_button(
                    mx,
                    my,
                    krui_core::PointerButton::Primary,
                    true,
                    mods,
                    timestamp_ms,
                );
            });
            let tick_effect = self.hooks.on_tick(&mut self.session);
            self.apply_hook_effect(tick_effect);
            self.sync_and_redraw();
        }
    }

    pub(crate) fn handle_left_mouse_released(&mut self) {
        self.is_left_mouse_down = false;
        let mods = self.mods;
        let released_at = self.map_cursor_for_capture_or_hover();
        let tree = &mut self.session.tree;
        self.session.reactive_session.enter(|| {
            if let Some((mx, my)) = released_at {
                let _ = tree.dispatch_pointer_button(
                    mx,
                    my,
                    krui_core::PointerButton::Primary,
                    false,
                    mods,
                    0,
                );
            } else {
                tree.end_pointer_drag();
            }
        });
        let tick_effect = self.hooks.on_tick(&mut self.session);
        self.apply_hook_effect(tick_effect);
        self.sync_and_redraw();
    }

    pub(crate) fn handle_focus_change(&mut self, focused: bool) {
        self.is_window_focused = focused;
        if focused {
            self.last_caret_activity = std::time::Instant::now();
        } else {
            self.is_left_mouse_down = false;
            self.mods = krui_core::Mods::default();
            let tree = &mut self.session.tree;
            self.session
                .reactive_session
                .enter(|| tree.dispatch_window_focus_lost());
        }
        let _ = self.session.tree.set_caret_visible(focused);
        self.sync_window_cursor_and_pointer_lock();
        self.sync_and_redraw();
    }

    pub(crate) fn handle_touch_event(&mut self, touch: winit::event::Touch) {
        let now = std::time::Instant::now();
        let dt = now
            .duration_since(self.last_touch_time)
            .as_secs_f32()
            .clamp(0.0001, 0.05);
        self.last_touch_time = now;
        let scale = self.session.scale_factor.max(1.0);
        let lx = (touch.location.x / scale) as f32;
        let ly = (touch.location.y / scale) as f32;
        let Some((mx, my)) = self.map_logical_point_to_tree(lx, ly) else {
            return;
        };
        let phase = crate::window_helpers::touch_phase_from_winit(touch.phase);
        if phase == krui_core::TouchPhaseKind::Started {
            self.last_caret_activity = now;
        }
        let tree = &mut self.session.tree;
        let clock = &mut self.session.clock;
        self.session.reactive_session.enter(|| {
            let _ = tree.dispatch_touch(clock, touch.id, phase, mx, my, dt);
        });
        let tick_effect = self.hooks.on_tick(&mut self.session);
        self.apply_hook_effect(tick_effect);
        self.sync_and_redraw();
    }
}
