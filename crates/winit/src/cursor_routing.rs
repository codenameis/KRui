//! Pointer routing helpers for `StandardWindowHost` (pointer capture bezel mapping,
//! `CursorLeft`, multi-click timestamps, mouse wheel, and drag release).

use crate::window_helpers::{cursor_icon_from_kind, mouse_button_code};
use crate::window_loop::{StandardWindowHost, WindowEventHooks};
use winit::dpi::PhysicalPosition;
use winit::event::{MouseButton, MouseScrollDelta};

impl<H: WindowEventHooks> StandardWindowHost<H> {
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
        let mapped = self.map_cursor_for_capture_or_hover();
        let is_down = self.is_left_mouse_down;
        if is_down {
            self.last_caret_activity = std::time::Instant::now();
        }
        let tree = &mut self.session.tree;
        let (drag_changed, hover_changed, new_cursor) = self.session.reactive_session.enter(|| {
            let drag = is_down && mapped.is_some_and(|(mx, my)| tree.dispatch_pointer_drag(mx, my));
            let hover = match mapped {
                Some((mx, my)) => tree.dispatch_pointer_move(mx, my),
                None => tree.dispatch_pointer_move(-10_000.0, -10_000.0),
            };
            (drag, hover, tree.hovered_cursor_kind())
        });
        let hook_changed = mapped.is_some_and(|(mx, my)| {
            self.hooks
                .on_pointer_moved(&mut self.session, mx, my, self.mods)
        });

        if new_cursor != self.current_cursor_kind {
            self.current_cursor_kind = new_cursor;
            if let Some(win) = &self.window {
                win.set_cursor(cursor_icon_from_kind(new_cursor));
            }
        }
        if drag_changed || hover_changed || hook_changed {
            if let Some(win) = &self.window {
                win.request_redraw();
            }
        }
    }

    pub(crate) fn handle_cursor_left(&mut self) {
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
    }

    pub(crate) fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta) {
        let dy = match delta {
            MouseScrollDelta::LineDelta(_, y) => y * 28.0,
            MouseScrollDelta::PixelDelta(pos) => pos.to_logical::<f32>(self.session.scale_factor).y,
        };
        if let Some((mx, my)) = self.map_cursor_to_tree() {
            if let Some(hook_effect) =
                self.hooks
                    .on_mouse_wheel(&mut self.session, mx, my, dy, self.mods)
            {
                self.apply_hook_effect(Some(hook_effect));
                return;
            }
            let tree = &mut self.session.tree;
            if self
                .session
                .reactive_session
                .enter(|| tree.dispatch_scroll(mx, my, dy))
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
        if pressed {
            self.last_caret_activity = std::time::Instant::now();
        } else if code == 0 {
            self.is_left_mouse_down = false;
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
                tree.dispatch_pointer_down_with_mods_at_time(mx, my, mods, timestamp_ms);
            });
            let tick_effect = self.hooks.on_tick(&mut self.session);
            self.apply_hook_effect(tick_effect);
            self.sync_and_redraw();
        }
    }

    pub(crate) fn handle_left_mouse_released(&mut self) {
        self.is_left_mouse_down = false;
        let tree = &mut self.session.tree;
        self.session
            .reactive_session
            .enter(|| tree.end_pointer_drag());
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
