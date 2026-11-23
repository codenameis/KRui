//! Embedded IDE side-panel runner streaming real `krui-renderer` framebuffers over TCP.

use crate::embed_panel_input::apply_panel_input_event;
use crate::ime_adapter::{sync_ime_host_with_sink, RecordedImeSink, WinitImeStateCache};
use crate::preview::{
    encode_rle_rgba_frame, CompileError, DevFileWatcher, DeviceFramePreset, PanelFrameState,
    PanelInputEvent, PreviewResponse, PreviewTcpClient, KRUI_EMBED_PANEL_ENV, KRUI_SERVE_ADDR_ENV,
};
use crate::session::EngineSession;
use crate::window_helpers::WindowEventHooks;
use krui_core::{ActiveBackendKind, HoveredCursorKind, InputKind, PaintScene};
use krui_signals::UiWaker;
use krui_style::ColorScheme;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct ChannelUiWaker {
    tx: mpsc::Sender<()>,
}

impl UiWaker for ChannelUiWaker {
    fn wake(&self) {
        let _ = self.tx.send(());
    }
}

/// Returns `true` when `krui serve --panel` launched this app with `KRUI_EMBED_PANEL=1`.
pub fn is_embed_panel_mode() -> bool {
    std::env::var(KRUI_EMBED_PANEL_ENV)
        .ok()
        .is_some_and(|v| v.trim() == "1")
}

/// Returned by [`EmbeddedPanelDriver::step`] once the `krui serve` TCP connection disconnects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelDisconnectedError;

impl std::fmt::Display for PanelDisconnectedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("krui serve panel TCP connection disconnected")
    }
}

impl std::error::Error for PanelDisconnectedError {}

/// Drives a real [`EngineSession`] (reactive tree, layout, `AdaptiveRenderer`, `tokio` pollers)
/// headlessly while streaming real RGBA8 `"KRFR"` frames over the `krui serve` TCP connection.
pub struct EmbeddedPanelDriver<H: WindowEventHooks> {
    pub session: EngineSession,
    pub hooks: H,
    watcher: Option<DevFileWatcher>,
    fallback_client: Option<PreviewTcpClient>,
    fallback_compile_error: Option<CompileError>,
    fallback_reload_count: u64,
    ime_cache: WinitImeStateCache,
    base_size: (u32, u32),
    is_left_down: bool,
    frame_seq: u32,
    last_krfr_bytes: Vec<u8>,
    has_emitted_first_frame: bool,
    force_refresh: bool,
    last_tick: Instant,
}

impl<H: WindowEventHooks> EmbeddedPanelDriver<H> {
    /// Connects to an explicit `krui serve` TCP address (`127.0.0.1:<port>`).
    pub fn new_connected(
        session: EngineSession,
        hot_reload_krui_path: Option<&str>,
        hooks: H,
        addr: &str,
    ) -> Option<Self> {
        let mut watcher = if cfg!(debug_assertions) {
            hot_reload_krui_path.and_then(DevFileWatcher::new)
        } else {
            None
        };
        let fallback_client = match &mut watcher {
            Some(w) => {
                if !w.has_tcp_client() {
                    let _ = w.connect_tcp(addr);
                }
                if w.has_tcp_client() {
                    None
                } else {
                    PreviewTcpClient::connect(addr)
                }
            }
            None => PreviewTcpClient::connect(addr),
        };
        if watcher.as_ref().is_none_or(|w| !w.has_tcp_client()) && fallback_client.is_none() {
            return None;
        }
        let base_size = (session.width.max(1), session.height.max(1));
        Some(Self {
            session,
            hooks,
            watcher,
            fallback_client,
            fallback_compile_error: None,
            fallback_reload_count: 0,
            ime_cache: WinitImeStateCache::default(),
            base_size,
            is_left_down: false,
            frame_seq: 0,
            last_krfr_bytes: Vec::new(),
            has_emitted_first_frame: false,
            force_refresh: true,
            last_tick: Instant::now(),
        })
    }

    /// Connects using `KRUI_SERVE_ADDR` in the environment.
    pub fn from_env(
        session: EngineSession,
        hot_reload_krui_path: Option<&str>,
        hooks: H,
    ) -> Option<Self> {
        let addr = std::env::var(KRUI_SERVE_ADDR_ENV).ok()?;
        Self::new_connected(session, hot_reload_krui_path, hooks, addr.trim())
    }

    fn is_disconnected(&self) -> bool {
        self.watcher
            .as_ref()
            .is_some_and(DevFileWatcher::is_tcp_disconnected)
            || self
                .fallback_client
                .as_ref()
                .is_some_and(PreviewTcpClient::is_disconnected)
    }

    /// Starts background TCP wakeup reader/writer threads on the active panel TCP socket.
    pub fn start_wakeup_listener(&mut self, wakeup: impl Fn() + Send + 'static) {
        if let Some(w) = &mut self.watcher {
            w.start_tcp_wakeup_listener(wakeup);
        } else if let Some(c) = &mut self.fallback_client {
            c.start_wakeup_listener(wakeup);
        }
    }

    fn needs_periodic_poll(&self) -> bool {
        if let Some(w) = &self.watcher {
            return w.needs_periodic_poll();
        }
        if let Some(c) = &self.fallback_client {
            return !c.has_wakeup_listener();
        }
        false
    }

    fn poll_tcp_and_inputs(&mut self) -> (bool, Vec<PanelInputEvent>) {
        if let Some(w) = &mut self.watcher {
            let tree = &mut self.session.tree;
            let changed = self
                .session
                .reactive_session
                .enter(|| w.poll_and_patch(tree).is_some());
            let inputs = w.drain_panel_inputs();
            return (changed, inputs);
        }
        if let Some(c) = &mut self.fallback_client {
            let tree = &mut self.session.tree;
            let err_slot = &mut self.fallback_compile_error;
            let cnt_slot = &mut self.fallback_reload_count;
            let changed = self
                .session
                .reactive_session
                .enter(|| c.poll(tree, err_slot, cnt_slot));
            let inputs = c.drain_panel_inputs();
            return (changed, inputs);
        }
        (false, Vec::new())
    }

    fn active_device_frame(&self) -> Option<DeviceFramePreset> {
        self.watcher
            .as_ref()
            .and_then(DevFileWatcher::device_frame)
            .or_else(|| {
                self.fallback_client
                    .as_ref()
                    .and_then(PreviewTcpClient::device_frame)
            })
    }

    fn active_overlays(&self) -> crate::preview::PreviewOverlayFlags {
        self.watcher
            .as_ref()
            .map(DevFileWatcher::overlays)
            .or_else(|| {
                self.fallback_client
                    .as_ref()
                    .map(PreviewTcpClient::overlays)
            })
            .unwrap_or_default()
    }

    fn active_reload_count(&self) -> u64 {
        self.watcher
            .as_ref()
            .map_or(self.fallback_reload_count, DevFileWatcher::reload_count)
    }

    fn apply_hook_effect(&mut self, effect: Option<Option<String>>) {
        let Some(opt) = effect else {
            return;
        };
        self.force_refresh = true;
        if opt.as_deref() == Some("__force_reload__") {
            if let Some(w) = &mut self.watcher {
                let tree = &mut self.session.tree;
                self.session.reactive_session.enter(|| {
                    let _ = w.force_reload(tree);
                });
            }
        }
    }

    fn decorate_scene(&mut self, scene: &mut PaintScene, frame_start: Instant) {
        let build_us = frame_start.elapsed().as_micros();
        if let Some(w) = &mut self.watcher {
            w.decorate_paint_scene(&self.session.tree, scene, build_us);
        } else if let Some(c) = &mut self.fallback_client {
            c.decorate_paint_scene(
                &self.session.tree,
                scene,
                self.fallback_compile_error.as_ref(),
                build_us,
            );
        }
    }

    fn send_panel_frame(&mut self, state: PanelFrameState) -> bool {
        if let Some(w) = &mut self.watcher {
            return w.send_panel_frame(state);
        }
        if let Some(c) = &mut self.fallback_client {
            return c.send_response(&PreviewResponse::PanelFrame(state)).is_ok();
        }
        false
    }

    fn render_and_emit_frame(&mut self, dt: f32) {
        let frame_start = Instant::now();
        let active_frame = self.active_device_frame();
        if self
            .session
            .reactive_session
            .enter(|| self.session.clock.has_active_animations())
            || self.session.tree.has_active_transitions()
        {
            let tree = &mut self.session.tree;
            let clock = &mut self.session.clock;
            self.session.reactive_session.enter(|| {
                if clock.has_active_animations() {
                    let _ = clock.tick(dt);
                }
                let _ = tree.tick_transitions(dt);
            });
        }
        let (lw, lh) =
            active_frame.map_or(self.base_size, DeviceFramePreset::safe_area_layout_size);
        let tree = &mut self.session.tree;
        let (mut scene, _) = self
            .session
            .reactive_session
            .enter(|| tree.build_paint_scene(lw, lh));
        self.decorate_scene(&mut scene, frame_start);

        let _ = self.session.renderer.render_scene(&scene);
        let (fb_w, fb_h) = self
            .session
            .renderer
            .framebuffer_dimensions()
            .unwrap_or_else(|| {
                let scale = scene.scale_factor.clamp(1.0, 4.0);
                (
                    ((scene.width as f32 * scale).round() as u32).max(1),
                    ((scene.height as f32 * scale).round() as u32).max(1),
                )
            });
        let phys_w = fb_w.clamp(1, u16::MAX as u32) as u16;
        let phys_h = fb_h.clamp(1, u16::MAX as u32) as u16;
        self.frame_seq = self.frame_seq.wrapping_add(1);
        let krfr = self.encode_changed_krfr(phys_w, phys_h);

        let frame_us = frame_start.elapsed().as_micros() as u64;
        let mut ime_sink = RecordedImeSink::default();
        sync_ime_host_with_sink(&mut ime_sink, &self.session.tree, &mut self.ime_cache);
        let state = self.build_panel_frame_state(phys_w, phys_h, frame_us, active_frame, krfr);
        let _ = self.send_panel_frame(state);
        self.has_emitted_first_frame = true;
        self.force_refresh = false;
    }

    fn encode_changed_krfr(&mut self, phys_w: u16, phys_h: u16) -> Vec<u8> {
        let Some(fb) = self.session.renderer.framebuffer_rgba() else {
            return Vec::new();
        };
        let encoded = encode_rle_rgba_frame(self.frame_seq, phys_w, phys_h, fb);
        if self.has_emitted_first_frame && !self.force_refresh && encoded == self.last_krfr_bytes {
            Vec::new()
        } else {
            self.last_krfr_bytes = encoded.clone();
            encoded
        }
    }

    fn build_panel_frame_state(
        &self,
        width: u16,
        height: u16,
        frame_us: u64,
        active_frame: Option<DeviceFramePreset>,
        krfr_bytes: Vec<u8>,
    ) -> PanelFrameState {
        let is_password = self.session.tree.focused_input_kind() == Some(InputKind::Password);
        let ime_focused = self.session.tree.focused_input_rect().is_some() && !is_password;
        let caret_rect = if ime_focused {
            self.session.tree.focused_ime_cursor_area().map(|r| {
                let (ox, oy) = active_frame.map_or((0.0, 0.0), DeviceFramePreset::content_origin);
                (r.x + ox, r.y + oy, r.width, r.height)
            })
        } else {
            None
        };
        PanelFrameState {
            seq: self.frame_seq,
            width,
            height,
            backend_name: match self.session.renderer.backend_kind() {
                ActiveBackendKind::Wgpu => "Wgpu".to_string(),
                ActiveBackendKind::TinySkiaCpuFallback => "TinySkia".to_string(),
            },
            theme_code: match self.session.tree.effective_color_scheme() {
                ColorScheme::Dark => "dark".to_string(),
                ColorScheme::Light => "light".to_string(),
            },
            device_frame_code: active_frame.map(|f| device_frame_slug(f).to_string()),
            overlays: self.active_overlays(),
            node_count: self.session.tree.node_count(),
            relayout_passes: self.session.tree.relayout_passes(),
            reload_count: self.active_reload_count(),
            frame_us,
            cursor_kind: cursor_slug(self.session.tree.hovered_cursor_kind()).to_string(),
            ime_focused,
            is_password,
            caret_rect,
            krfr_bytes,
        }
    }

    /// Advances one headless panel tick: drains TCP commands, polls `tokio` tasks, and streams a
    /// `"KRFR"` frame when dirty or animating. Returns `Ok(true)` if a frame was emitted or `Err(PanelDisconnectedError)` on EOF.
    pub fn step(&mut self) -> Result<bool, PanelDisconnectedError> {
        if self.is_disconnected() {
            return Err(PanelDisconnectedError);
        }
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_tick)
            .as_secs_f32()
            .clamp(1.0 / 240.0, 0.05);
        self.last_tick = now;

        let async_active = self
            .session
            .reactive_session
            .enter(krui_signals::poll_async_tasks);
        let (tcp_changed, inputs) = self.poll_tcp_and_inputs();
        if self.is_disconnected() {
            return Err(PanelDisconnectedError);
        }
        let active_frame = self.active_device_frame();
        let had_inputs = !inputs.is_empty();
        for ev in inputs {
            let outcome = apply_panel_input_event(
                &mut self.session,
                &mut self.hooks,
                active_frame,
                &mut self.base_size,
                &mut self.is_left_down,
                ev,
            );
            if outcome.force_refresh {
                self.force_refresh = true;
            }
            self.apply_hook_effect(outcome.hook_effect);
        }
        let tick_effect = self.hooks.on_tick(&mut self.session);
        self.apply_hook_effect(tick_effect);

        let has_anim = self
            .session
            .reactive_session
            .enter(|| self.session.clock.has_active_animations())
            || self.session.tree.has_active_transitions();
        let dirty = !self.has_emitted_first_frame
            || self.force_refresh
            || tcp_changed
            || had_inputs
            || async_active
            || has_anim
            || self.session.tree.is_paint_dirty();
        if dirty {
            self.render_and_emit_frame(dt);
        }
        Ok(dirty)
    }
}

fn device_frame_slug(frame: DeviceFramePreset) -> &'static str {
    match frame {
        DeviceFramePreset::Iphone15 => "iphone-15",
        DeviceFramePreset::Pixel8 => "pixel-8",
        DeviceFramePreset::Rpi800x480 => "rpi-800x480",
    }
}

fn cursor_slug(kind: HoveredCursorKind) -> &'static str {
    match kind {
        HoveredCursorKind::Text => "text",
        HoveredCursorKind::Pointer => "pointer",
        HoveredCursorKind::Default => "default",
    }
}

/// Runs the real application headlessly in embedded IDE panel mode (`KRUI_EMBED_PANEL=1`).
pub fn run_embedded_panel_session<H: WindowEventHooks>(
    _title: &str,
    session: EngineSession,
    hot_reload_krui_path: Option<&str>,
    hooks: H,
) {
    let Some(mut driver) = EmbeddedPanelDriver::from_env(session, hot_reload_krui_path, hooks)
    else {
        return;
    };
    let (wake_tx, wake_rx) = mpsc::channel();
    let wake_tx_tcp = wake_tx.clone();
    driver.start_wakeup_listener(move || {
        let _ = wake_tx_tcp.send(());
    });
    driver.session.reactive_session.enter(|| {
        krui_signals::set_ui_waker(Some(Arc::new(ChannelUiWaker { tx: wake_tx })));
    });
    while driver.step().is_ok() {
        let has_anim = driver
            .session
            .reactive_session
            .enter(|| driver.session.clock.has_active_animations())
            || driver.session.tree.has_active_transitions();
        if has_anim {
            let _ = wake_rx.recv_timeout(crate::window_helpers::FRAME_PACE_120HZ);
        } else if driver.needs_periodic_poll() {
            let _ = wake_rx.recv_timeout(Duration::from_millis(20));
        } else {
            let _ = wake_rx.recv();
        }
        while wake_rx.try_recv().is_ok() {}
    }
    driver
        .session
        .reactive_session
        .enter(|| krui_signals::set_ui_waker(None));
}
