//! Frame-budget telemetry (`FrameTelemetry`) and `EngineSession` runner.

use krui_core::{
    FrameRenderStats, GpuResourceLimits, NodeId, RenderBackendPreference, ViewFragment, ViewTree,
};
use krui_renderer::AdaptiveRenderer;
use krui_signals::FrameClock;
use std::time::Instant;

/// Returns `true` only when both the `preview` feature AND debug assertions are enabled.
///
/// Hot-patching, file watching, and live reload are strictly Debug-only; release builds
/// keep preview hot-reload inert even if `--features preview` is passed, and default
/// release builds exclude `krui_compiler` from the dependency graph completely.
pub const fn is_dev_hot_reload_enabled() -> bool {
    cfg!(all(feature = "preview", debug_assertions))
}

/// Per-frame timing and budget report (Section 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameTelemetry {
    pub frame_number: u64,
    pub total_frame_us: u64,
    pub layout_ran: bool,
    pub total_relayout_passes: u64,
    pub render_stats: FrameRenderStats,
}

impl FrameTelemetry {
    /// 120 FPS frame budget in microseconds (8,333 us = 8.33 ms).
    pub const BUDGET_120FPS_US: u64 = 8_333;
    /// 60 FPS frame budget in microseconds (16,667 us = 16.67 ms).
    pub const BUDGET_60FPS_US: u64 = 16_667;

    /// Returns `true` if `total_frame_us` is within the 120 FPS budget (`<= 8,333 us`).
    pub const fn meets_120fps_budget(&self) -> bool {
        self.total_frame_us <= Self::BUDGET_120FPS_US
    }

    /// Returns `true` if `total_frame_us` is within the 60 FPS budget (`<= 16,667 us`).
    pub const fn meets_60fps_budget(&self) -> bool {
        self.total_frame_us <= Self::BUDGET_60FPS_US
    }
}

/// High-level KRui engine session combining `ReactiveSession`, `ViewTree`, `FrameClock`, and `AdaptiveRenderer`.
pub struct EngineSession {
    pub reactive_session: krui_signals::ReactiveSession,
    pub tree: ViewTree,
    pub clock: FrameClock,
    pub renderer: AdaptiveRenderer,
    pub width: u32,
    pub height: u32,
    /// Display DPI scale factor (default `1.0`). Layout runs in logical pixels while
    /// surface rendering runs at physical resolution.
    pub scale_factor: f64,
}

impl EngineSession {
    /// Creates a new `EngineSession` bound to the current active [`krui_signals::ReactiveSession`].
    pub fn new(width: u32, height: u32, preference: RenderBackendPreference) -> Self {
        let reactive_session = krui_signals::ReactiveSession::current();
        let renderer =
            AdaptiveRenderer::new(width, height, preference, GpuResourceLimits::default());
        let mut tree = reactive_session.enter(ViewTree::new);
        let _ = tree.sync_viewport_and_media(width.max(1) as f32, height.max(1) as f32);
        let clock = reactive_session.enter(FrameClock::new);
        Self {
            reactive_session,
            tree,
            clock,
            renderer,
            width,
            height,
            scale_factor: 1.0,
        }
    }

    /// Creates an `EngineSession` owning a brand-new isolated [`krui_signals::ReactiveSession`].
    pub fn new_isolated(width: u32, height: u32, preference: RenderBackendPreference) -> Self {
        let reactive_session = krui_signals::ReactiveSession::new();
        reactive_session.enter(|| {
            let renderer =
                AdaptiveRenderer::new(width, height, preference, GpuResourceLimits::default());
            let mut tree = ViewTree::new();
            let _ = tree.sync_viewport_and_media(width.max(1) as f32, height.max(1) as f32);
            Self {
                reactive_session: reactive_session.clone(),
                tree,
                clock: FrameClock::new(),
                renderer,
                width,
                height,
                scale_factor: 1.0,
            }
        })
    }

    /// Creates an isolated `EngineSession` sharing an existing `Arc<krui_renderer::GpuContext>`
    /// (falling back to `fallback_preference` when `gpu_context` is `None`).
    pub fn new_isolated_with_shared_gpu(
        width: u32,
        height: u32,
        gpu_context: Option<std::sync::Arc<krui_renderer::GpuContext>>,
        fallback_preference: RenderBackendPreference,
    ) -> Self {
        let reactive_session = krui_signals::ReactiveSession::new();
        reactive_session.enter(|| {
            let limits = GpuResourceLimits::default();
            let renderer = match gpu_context {
                Some(ctx) => AdaptiveRenderer::from_shared_gpu_context(ctx, width, height, limits),
                None => AdaptiveRenderer::new(width, height, fallback_preference, limits),
            };
            let mut tree = ViewTree::new();
            let _ = tree.sync_viewport_and_media(width.max(1) as f32, height.max(1) as f32);
            Self {
                reactive_session: reactive_session.clone(),
                tree,
                clock: FrameClock::new(),
                renderer,
                width,
                height,
                scale_factor: 1.0,
            }
        })
    }

    /// Updates the display DPI scale factor (`ScaleFactorChanged`).
    pub fn set_scale_factor(&mut self, scale_factor: f64) {
        if scale_factor.is_finite() && scale_factor > 0.0 {
            self.scale_factor = scale_factor;
            self.tree.set_scale_factor(scale_factor as f32);
        }
    }

    /// Resizes the logical viewport and updates active `@media` breakpoints and `vw`/`dvh` bindings.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        let _ = self
            .tree
            .sync_viewport_and_media(self.width as f32, self.height as f32);
    }

    /// Replaces the current root view with `fragment` and triggers a smooth spring-based
    /// slide + fade-in page transition driven by `self.clock`.
    pub fn transition_to_view(&mut self, fragment: ViewFragment) -> Vec<NodeId> {
        let tree = &mut self.tree;
        let clock = &mut self.clock;
        self.reactive_session
            .enter(|| tree.transition_to(clock, fragment))
    }

    /// Evaluates `build` inside a fresh page `Scope` and triggers a smooth spring-based
    /// slide + fade-in page transition driven by `self.clock`.
    pub fn transition_to_view_with(
        &mut self,
        build: impl FnOnce(&mut FrameClock) -> ViewFragment,
    ) -> Vec<NodeId> {
        let tree = &mut self.tree;
        let clock = &mut self.clock;
        self.reactive_session
            .enter(|| tree.transition_to_with(clock, build))
    }

    /// Routes and executes a key-down event against `self.tree` with zero hardcoded global key hijacks.
    pub fn handle_key_down(
        &mut self,
        key_info: &krui_core::KeyInfo,
        mods: krui_core::Mods,
        repeat: bool,
    ) -> bool {
        let tree = &mut self.tree;
        self.reactive_session.enter(|| {
            let action = tree.dispatch_routed_key(key_info, mods, repeat);
            crate::window_helpers::apply_key_action(tree, action)
        })
    }

    /// Advances the frame clock by `dt_seconds`, runs layout (only if dirty), renders the scene,
    /// and returns `FrameTelemetry` for frame-budget verification.
    pub fn step_frame(&mut self, dt_seconds: f32) -> FrameTelemetry {
        self.step_frame_internal(dt_seconds, false)
    }

    /// Advances the frame clock by `dt_seconds`, renders the scene, and waits for GPU completion
    /// (`device.poll(wgpu::Maintain::Wait)`) before recording `total_frame_us`.
    pub fn step_frame_with_gpu_wait(&mut self, dt_seconds: f32) -> FrameTelemetry {
        self.step_frame_internal(dt_seconds, true)
    }

    /// Dispatches an `accesskit::ActionRequest` (`Focus`, `Click`, `SetValue`) to the active `ViewTree`.
    pub fn dispatch_accesskit_action(
        &mut self,
        request: &krui_core::accesskit::ActionRequest,
    ) -> bool {
        let tree = &mut self.tree;
        self.reactive_session
            .enter(|| tree.dispatch_accesskit_action(request))
    }

    fn step_frame_internal(&mut self, dt_seconds: f32, wait_for_gpu: bool) -> FrameTelemetry {
        let start = Instant::now();
        let clock = &mut self.clock;
        let tree = &mut self.tree;
        let renderer = &mut self.renderer;
        let (width, height) = (self.width, self.height);
        self.reactive_session.enter(|| {
            let tick = clock.tick(dt_seconds);
            let _ = tree.sync_viewport_and_media(width.max(1) as f32, height.max(1) as f32);
            let _ = tree.tick_transitions(dt_seconds);
            let (scene, layout_ran) = tree.build_paint_scene(width, height);
            let render_stats = renderer.render_scene(&scene);
            if wait_for_gpu {
                renderer.finish_gpu_work();
            }
            let total_frame_us = start.elapsed().as_micros() as u64;

            FrameTelemetry {
                frame_number: tick.frame_number,
                total_frame_us,
                layout_ran,
                total_relayout_passes: tree.relayout_passes(),
                render_stats,
            }
        })
    }
}
