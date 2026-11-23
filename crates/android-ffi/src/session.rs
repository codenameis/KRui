//! Safe Android lifecycle session (`AndroidSession`) and handle table (zero `unsafe`).

use crate::ime_bridge::{
    composing_cursor_bytes, compute_utf8_delete_surrounding, editor_action_from_android_code,
    input_kind_to_android_code, touch_phase_from_android_action, AndroidTextInputHost,
};
use crate::preview_client::AndroidPreviewBridge;
use krui_core::{
    utf16_to_utf8_range, GpuResourceLimits, ImeEvent, RenderBackendPreference, ViewTree,
    ViewportInsets,
};
use krui_renderer::AdaptiveRenderer;
use krui_signals::{poll_async_tasks, FrameClock, ReactiveSession};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

static NEXT_SESSION_ID: AtomicI64 = AtomicI64::new(1);

thread_local! {
    static SESSIONS: RefCell<HashMap<i64, AndroidSession>> = RefCell::new(HashMap::new());
}

/// Complete Android-hosted KRui session owning an isolated `ReactiveSession`, `ViewTree`,
/// `FrameClock`, recoverable `AdaptiveRenderer`, `AndroidTextInputHost`, and optional TCP preview.
pub struct AndroidSession {
    /// Isolated reactive runtime owning all signals, effects, and async tasks for this session.
    pub reactive_session: ReactiveSession,
    /// Platform-neutral UI tree, layout engine, touch tracker, and IME state machine.
    pub tree: ViewTree,
    /// Animation and inertial fling clock tied to this session.
    pub clock: FrameClock,
    /// Surface renderer (`None` between `on_surface_destroyed` and `on_surface_created`).
    pub renderer: Option<AdaptiveRenderer>,
    /// Safe state mirror implementing `TextInputHost` for `KruiView.kt`.
    pub ime_host: AndroidTextInputHost,
    preview: AndroidPreviewBridge,
    prefer_gpu: bool,
    scale_factor: f32,
    logical_width: u32,
    logical_height: u32,
    insets: ViewportInsets,
    surface_alive: bool,
    argb_scratch: Vec<i32>,
}

impl AndroidSession {
    /// Creates a new isolated [`AndroidSession`] with safe software fallback when `prefer_gpu` is false.
    pub fn new(width_px: u32, height_px: u32, scale_factor: f32, prefer_gpu: bool) -> Self {
        let scale = sanitize_scale(scale_factor);
        let (logical_width, logical_height) = compute_logical_size(width_px, height_px, scale);
        let pref = backend_preference(prefer_gpu);
        let reactive_session = ReactiveSession::new();
        let (tree, clock, renderer, ime_host) = reactive_session.enter(|| {
            let renderer = AdaptiveRenderer::new(
                logical_width,
                logical_height,
                pref,
                GpuResourceLimits::default(),
            );
            let mut tree = ViewTree::new();
            tree.set_scale_factor(scale);
            let mut ime_host = AndroidTextInputHost::default();
            tree.sync_text_input_host(&mut ime_host);
            (tree, FrameClock::new(), renderer, ime_host)
        });
        Self {
            reactive_session,
            tree,
            clock,
            renderer: Some(renderer),
            ime_host,
            preview: AndroidPreviewBridge::default(),
            prefer_gpu,
            scale_factor: scale,
            logical_width,
            logical_height,
            insets: ViewportInsets::default(),
            surface_alive: true,
            argb_scratch: Vec::new(),
        }
    }

    /// Returns the current logical viewport dimensions `(width, height)`.
    pub const fn logical_size(&self) -> (u32, u32) {
        (self.logical_width, self.logical_height)
    }

    /// Returns the active DPI scale factor (`>= 1.0`).
    pub const fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Returns `true` when a surface/renderer backing is currently active.
    pub const fn is_surface_alive(&self) -> bool {
        self.surface_alive
    }

    /// Creates or restores the backing renderer when Android `SurfaceHolder.surfaceCreated` fires.
    pub fn on_surface_created(&mut self, width_px: u32, height_px: u32, scale_factor: f32) -> bool {
        self.update_dimensions(width_px, height_px, scale_factor);
        let (w, h, pref) = (
            self.logical_width,
            self.logical_height,
            backend_preference(self.prefer_gpu),
        );
        let tree = &mut self.tree;
        let insets = self.insets;
        let ime_host = &mut self.ime_host;
        if self.renderer.is_none() {
            let renderer = self
                .reactive_session
                .enter(|| AdaptiveRenderer::new(w, h, pref, GpuResourceLimits::default()));
            self.renderer = Some(renderer);
        }
        self.reactive_session.enter(|| {
            tree.set_viewport_insets(insets, h as f32);
            tree.sync_text_input_host(ime_host);
        });
        self.surface_alive = true;
        true
    }

    /// Updates logical size when Android `SurfaceHolder.surfaceChanged` fires.
    pub fn on_surface_resized(&mut self, width_px: u32, height_px: u32, scale_factor: f32) -> bool {
        self.update_dimensions(width_px, height_px, scale_factor);
        let (h, insets) = (self.logical_height, self.insets);
        let tree = &mut self.tree;
        let ime_host = &mut self.ime_host;
        self.reactive_session.enter(|| {
            tree.set_viewport_insets(insets, h as f32);
            tree.sync_text_input_host(ime_host);
        });
        true
    }

    /// Drops the GPU/SW surface renderer cleanly while preserving `ViewTree`, `Signal`s, and IME state.
    pub fn on_surface_destroyed(&mut self) -> bool {
        self.renderer = None;
        self.surface_alive = false;
        true
    }

    /// Applies Android `WindowInsetsCompat` (physical px) scaled into logical viewport insets.
    pub fn on_insets_changed(
        &mut self,
        top_px: f32,
        right_px: f32,
        bottom_px: f32,
        left_px: f32,
        ime_bottom_px: f32,
    ) -> bool {
        let s = self.scale_factor.max(1.0);
        self.insets = ViewportInsets {
            top: (top_px.max(0.0)) / s,
            right: (right_px.max(0.0)) / s,
            bottom: (bottom_px.max(0.0)) / s,
            left: (left_px.max(0.0)) / s,
            ime_bottom: (ime_bottom_px.max(0.0)) / s,
        };
        let (h, insets) = (self.logical_height as f32, self.insets);
        let tree = &mut self.tree;
        let ime_host = &mut self.ime_host;
        self.reactive_session.enter(|| {
            tree.set_viewport_insets(insets, h);
            tree.sync_text_input_host(ime_host);
            tree.is_paint_dirty()
        })
    }

    /// Routes an Android `MotionEvent` into [`ViewTree::dispatch_touch`] in logical pixels.
    pub fn on_touch(
        &mut self,
        pointer_id: u64,
        action_masked: i32,
        x_px: f32,
        y_px: f32,
        dt_seconds: f32,
    ) -> bool {
        let Some(phase) = touch_phase_from_android_action(action_masked) else {
            return false;
        };
        let s = self.scale_factor.max(1.0);
        let lx = x_px / s;
        let ly = y_px / s;
        let tree = &mut self.tree;
        let clock = &mut self.clock;
        let ime_host = &mut self.ime_host;
        self.reactive_session.enter(|| {
            let handled = tree.dispatch_touch(clock, pointer_id, phase, lx, ly, dt_seconds);
            tree.sync_text_input_host(ime_host);
            handled
        })
    }

    /// Advances async tasks, live TCP preview, animations, layout, and rendering for one frame.
    pub fn step_frame(&mut self, dt_seconds: f32) -> bool {
        let (w, h) = (self.logical_width, self.logical_height);
        let tree = &mut self.tree;
        let clock = &mut self.clock;
        let preview = &mut self.preview;
        let renderer = &mut self.renderer;
        let ime_host = &mut self.ime_host;
        self.reactive_session.enter(|| {
            let _ = poll_async_tasks();
            let _ = preview.poll(tree);
            if clock.has_active_animations() {
                let _ = clock.tick(dt_seconds.clamp(0.0001, 0.05));
            }
            let (scene, _) = tree.build_paint_scene(w, h);
            if let Some(r) = renderer.as_mut() {
                let _ = r.render_scene(&scene);
            }
            tree.sync_text_input_host(ime_host);
            true
        })
    }

    /// Polls async work and preview commands and reports whether another frame needs scheduling.
    pub fn poll_needs_frame(&mut self) -> bool {
        let tree = &mut self.tree;
        let clock = &self.clock;
        let preview = &mut self.preview;
        self.reactive_session.enter(|| {
            let _ = poll_async_tasks();
            let _ = preview.poll(tree);
            tree.is_paint_dirty() || clock.has_active_animations()
        })
    }

    /// Synchronizes `ime_host` and returns `true` when Android's soft keyboard should be active.
    pub fn should_show_keyboard(&mut self) -> bool {
        let tree = &mut self.tree;
        let ime_host = &mut self.ime_host;
        self.reactive_session.enter(|| {
            tree.sync_text_input_host(ime_host);
            ime_host.keyboard_visible
        })
    }

    /// Synchronizes `ime_host` and returns the Android `INPUT_KIND_*` integer code.
    pub fn input_kind_code(&mut self) -> i32 {
        let tree = &mut self.tree;
        let ime_host = &mut self.ime_host;
        self.reactive_session.enter(|| {
            tree.sync_text_input_host(ime_host);
            input_kind_to_android_code(ime_host.input_kind)
        })
    }

    /// Synchronizes `ime_host` and returns the stable focused node identity (`0` when unfocused).
    pub fn focused_node_key(&mut self) -> i64 {
        let tree = &mut self.tree;
        let ime_host = &mut self.ime_host;
        self.reactive_session.enter(|| {
            tree.sync_text_input_host(ime_host);
            ime_host.focused_node_key.unwrap_or(0) as i64
        })
    }

    /// Returns packed physical framebuffer dimensions `(width << 32) | height`, or `0` when unavailable.
    pub fn packed_framebuffer_size(&self) -> i64 {
        let Some((w, h)) = self
            .renderer
            .as_ref()
            .and_then(AdaptiveRenderer::framebuffer_dimensions)
        else {
            return 0;
        };
        ((i64::from(w)) << 32) | i64::from(h)
    }

    /// Converts the latest rendered RGBA8 framebuffer into Android `ARGB_8888` `i32` pixels in-place
    /// and invokes `f(width, height, pixels)`.
    pub fn with_framebuffer_argb<R>(&mut self, f: impl FnOnce(u32, u32, &[i32]) -> R) -> Option<R> {
        let r = self.renderer.as_mut()?;
        let (w, h) = r.framebuffer_dimensions()?;
        let rgba = r.framebuffer_rgba()?;
        let pixel_count = (w as usize).checked_mul(h as usize)?;
        if rgba.len() < pixel_count.checked_mul(4)? {
            return None;
        }
        self.argb_scratch.clear();
        self.argb_scratch.reserve(pixel_count);
        for &[r, g, b, a] in rgba[..pixel_count * 4].as_chunks::<4>().0 {
            let argb =
                (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
            self.argb_scratch.push(argb as i32);
        }
        Some(f(w, h, &self.argb_scratch))
    }

    /// Dispatches Android `InputConnection.setComposingText(text, newCursorPosition)`.
    pub fn set_composing_text(&mut self, text: &str, new_cursor_pos: i32) -> bool {
        let cursor = composing_cursor_bytes(text, new_cursor_pos);
        self.dispatch_ime_synced(ImeEvent::SetComposing {
            text: text.to_string(),
            cursor,
        })
    }

    /// Dispatches Android `InputConnection.commitText(text, newCursorPosition)`.
    pub fn commit_text(&mut self, text: &str, _new_cursor_pos: i32) -> bool {
        self.dispatch_ime_synced(ImeEvent::Commit(text.to_string()))
    }

    /// Dispatches `InputConnection.deleteSurroundingText` using UTF-16 code-unit lengths.
    pub fn delete_surrounding_text_utf16(&mut self, before_u16: i32, after_u16: i32) -> bool {
        let tree = &mut self.tree;
        let ime_host = &mut self.ime_host;
        self.reactive_session.enter(|| {
            if tree.has_active_ime_preedit() {
                let _ = tree.ime_finish_composing();
            }
            let Some(state) = tree.focused_editing_state() else {
                return false;
            };
            let (before_bytes, after_bytes) =
                compute_utf8_delete_surrounding(&state, before_u16, after_u16);
            let ok = tree.dispatch_ime_event(ImeEvent::DeleteSurrounding {
                before_bytes,
                after_bytes,
            });
            tree.sync_text_input_host(ime_host);
            ok
        })
    }

    /// Dispatches `InputConnection.setSelection(start, end)` using UTF-16 code-unit offsets.
    pub fn set_selection_utf16(&mut self, start_u16: i32, end_u16: i32) -> bool {
        let tree = &mut self.tree;
        let ime_host = &mut self.ime_host;
        self.reactive_session.enter(|| {
            if tree.has_active_ime_preedit() {
                let _ = tree.ime_finish_composing();
            }
            let Some(state) = tree.focused_editing_state() else {
                return false;
            };
            let s_u16 = start_u16.max(0) as usize;
            let e_u16 = end_u16.max(0) as usize;
            let range = utf16_to_utf8_range(&state.text, s_u16..e_u16);
            let ok = tree.dispatch_ime_event(ImeEvent::SetSelection(range));
            tree.sync_text_input_host(ime_host);
            ok
        })
    }

    /// Dispatches Android `InputConnection.finishComposingText`.
    pub fn finish_composing_text(&mut self) -> bool {
        self.dispatch_ime_synced(ImeEvent::FinishComposing)
    }

    /// Dispatches Android `InputConnection.performEditorAction(imeAction)`.
    pub fn perform_editor_action(&mut self, ime_action: i32) -> bool {
        self.dispatch_ime_synced(ImeEvent::EditorAction(editor_action_from_android_code(
            ime_action,
        )))
    }

    /// Connects to a `krui serve --android` host TCP server (`false` when built without `preview`).
    pub fn connect_preview(&mut self, addr: &str) -> bool {
        self.preview.connect(addr)
    }

    /// Returns the number of live preview hot-reloads applied in this session.
    pub fn preview_reload_count(&self) -> u64 {
        self.preview.reload_count()
    }

    fn dispatch_ime_synced(&mut self, event: ImeEvent) -> bool {
        let tree = &mut self.tree;
        let ime_host = &mut self.ime_host;
        self.reactive_session.enter(|| {
            let ok = tree.dispatch_ime_event(event);
            tree.sync_text_input_host(ime_host);
            ok
        })
    }

    fn update_dimensions(&mut self, width_px: u32, height_px: u32, scale_factor: f32) {
        let scale = sanitize_scale(scale_factor);
        self.scale_factor = scale;
        let (lw, lh) = compute_logical_size(width_px, height_px, scale);
        self.logical_width = lw;
        self.logical_height = lh;
        let tree = &mut self.tree;
        self.reactive_session.enter(|| {
            tree.set_scale_factor(scale);
        });
    }
}

fn sanitize_scale(scale_factor: f32) -> f32 {
    if scale_factor.is_finite() && scale_factor >= 1.0 {
        scale_factor
    } else {
        1.0
    }
}

fn compute_logical_size(width_px: u32, height_px: u32, scale: f32) -> (u32, u32) {
    let lw = ((width_px.max(1) as f32) / scale).round().max(1.0) as u32;
    let lh = ((height_px.max(1) as f32) / scale).round().max(1.0) as u32;
    (lw, lh)
}

const fn backend_preference(prefer_gpu: bool) -> RenderBackendPreference {
    if prefer_gpu {
        RenderBackendPreference::Auto
    } else {
        RenderBackendPreference::ForceTinySkia
    }
}

/// Registers an existing [`AndroidSession`] in the safe thread-local handle table and returns its ID.
pub fn register_session(session: AndroidSession) -> i64 {
    let id = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    SESSIONS.with(|map| {
        if let Ok(mut guard) = map.try_borrow_mut() {
            guard.insert(id, session);
        }
    });
    id
}

/// Allocates and registers a new [`AndroidSession`], returning its positive opaque handle ID.
pub fn create_registered_session(
    width_px: i32,
    height_px: i32,
    scale_factor: f32,
    prefer_gpu: bool,
) -> i64 {
    let w = width_px.max(1) as u32;
    let h = height_px.max(1) as u32;
    register_session(AndroidSession::new(w, h, scale_factor, prefer_gpu))
}

/// Removes and drops a registered [`AndroidSession`], returning `true` if the ID existed.
pub fn destroy_registered_session(session_id: i64) -> bool {
    if session_id <= 0 {
        return false;
    }
    let removed = SESSIONS.with(|map| {
        map.try_borrow_mut()
            .ok()
            .and_then(|mut guard| guard.remove(&session_id))
    });
    let Some(session) = removed else {
        return false;
    };
    let reactive = session.reactive_session.clone();
    drop(session);
    reactive.reset();
    true
}

/// Safely borrows a registered [`AndroidSession`] by `session_id`, returning `None` on stale/re-entrant access.
pub fn with_registered_session<R>(
    session_id: i64,
    f: impl FnOnce(&mut AndroidSession) -> R,
) -> Option<R> {
    if session_id <= 0 {
        return None;
    }
    SESSIONS.with(|map| {
        let mut guard = map.try_borrow_mut().ok()?;
        let session = guard.get_mut(&session_id)?;
        Some(f(session))
    })
}
