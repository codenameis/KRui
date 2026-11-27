//! End-to-end Linux DRM DumbBuffer + `evdev` kiosk host (`KioskAppHost`).

use crate::commit_planner::{plan_dirtyfb_commits, UNION_BLOAT_SPLIT_RATIO_X10};
use crate::drm_sink::{DrmPixelFormat, DrmPresentSink};
use crate::evdev_input::{EvdevInputEvent, KioskInputRouter};
use crate::flush_worker::{FlushWorker, FlushWorkerStats};
use krui_core::{
    FocusableCandidate, FrameRenderStats, GpuResourceLimits, NodeId, PaintScene, ViewTree,
};
use krui_renderer::{RendererBackend, TinySkiaRenderer};
use krui_signals::{init_async_runtime_mode, poll_async_tasks, AsyncRuntimeMode, FrameClock};
use krui_style::{
    Color, ColorDepthBits, DamageTrackingMode, DisplayShape, RootProfileAttrs, TierProfile,
};
use std::time::Instant;

/// Stage-by-stage kiosk cold-start timing telemetry (microseconds).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BootStageTimings {
    /// Host initialization and tokio single-worker startup overhead (`us`).
    pub host_init_us: u64,
    /// Initial `:root` profile + Stylo cascade + Taffy layout duration (`us`).
    pub first_layout_us: u64,
    /// First `TinySkiaRenderer` rasterization pass (`us`).
    pub first_render_us: u64,
    /// First `DIRTYFB` encode + background flush completion (`us`).
    pub first_flush_us: u64,
}

/// Configuration for [`KioskAppHost`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KioskHostConfig {
    /// Physical framebuffer width (e.g. `480` on PiTFT 3.5").
    pub width: u32,
    /// Physical framebuffer height (e.g. `320` on PiTFT 3.5").
    pub height: u32,
    /// Target DRM buffer pixel format (`Rgb565` or `Xrgb8888`).
    pub format: DrmPixelFormat,
    /// `hx8357d` union-bbox split ratio (`15` = split when `union_area > 1.5x * sum_area`).
    pub split_ratio_x10: u64,
    /// Enable 2-bulsik Hangul IME automaton for keyboard/OSK input.
    pub hangul_enabled: bool,
    /// Set `@media (prefers-reduced-motion: reduce)` (`data-krui-motion="reduce"`).
    pub reduced_motion: bool,
}

impl Default for KioskHostConfig {
    fn default() -> Self {
        Self {
            width: 480,
            height: 320,
            format: DrmPixelFormat::Rgb565,
            split_ratio_x10: UNION_BLOAT_SPLIT_RATIO_X10,
            hangul_enabled: true,
            reduced_motion: false,
        }
    }
}

/// Per-frame output returned by [`KioskAppHost::render_and_submit`].
#[derive(Clone, Debug, PartialEq)]
pub struct KioskFrameReport {
    /// Background flush sequence id (`0` if scene was idle with zero damage).
    pub flush_seq: u64,
    /// Whether Taffy ran a relayout pass this frame.
    pub did_relayout: bool,
    /// Number of raw damage rects emitted by `DamageTracker`.
    pub damage_rect_count: usize,
    /// Number of planned `DIRTYFB` SPI commits submitted.
    pub commit_count: usize,
    /// CPU rasterizer frame statistics.
    pub stats: FrameRenderStats,
}

/// Self-contained embedded kiosk host pairing `TinySkiaRenderer`, partial `DamageTracker`,
/// `hx8357d` `DIRTYFB` commit planning, background `FlushWorker`, and `evdev` input routing.
pub struct KioskAppHost {
    tree: ViewTree,
    clock: FrameClock,
    renderer: TinySkiaRenderer,
    scene: PaintScene,
    worker: FlushWorker,
    input: KioskInputRouter,
    config: KioskHostConfig,
    timings: BootStageTimings,
    first_frame_done: bool,
    last_render_instant: Option<Instant>,
}

impl KioskAppHost {
    /// Initializes kiosk async mode, applies `CpuKiosk` + `Bpp16Rgb565` profile attributes,
    /// enables `BoundingRects` damage tracking, and spawns the background DRM flush worker.
    pub fn new<S>(mut tree: ViewTree, sink: S, config: KioskHostConfig) -> Self
    where
        S: DrmPresentSink + Send + 'static,
    {
        let init_start = Instant::now();
        let _ = init_async_runtime_mode(AsyncRuntimeMode::SingleWorkerKiosk);
        tree.apply_root_profile_attrs(RootProfileAttrs {
            tier: TierProfile::CpuKiosk,
            shape: DisplayShape::Rectangular,
            color_depth: match config.format {
                DrmPixelFormat::Rgb565 => ColorDepthBits::Bpp16Rgb565,
                DrmPixelFormat::Xrgb8888 => ColorDepthBits::Bpp24OrMore,
            },
            reduced_motion: config.reduced_motion,
        });
        tree.set_damage_tracking_mode(DamageTrackingMode::BoundingRects {
            split_area_ratio_x10: config.split_ratio_x10.min(u16::MAX as u64) as u16,
            max_rects_per_commit: 8,
        });
        let renderer =
            TinySkiaRenderer::new(config.width, config.height, GpuResourceLimits::default());
        let scene = PaintScene::new(config.width, config.height, Color::BLACK);
        let worker = FlushWorker::spawn(sink, config.format);
        let mut input = KioskInputRouter::default();
        input.set_hangul_enabled(config.hangul_enabled, &mut tree);
        let timings = BootStageTimings {
            host_init_us: init_start.elapsed().as_micros() as u64,
            ..Default::default()
        };
        Self {
            tree,
            clock: FrameClock::new(),
            renderer,
            scene,
            worker,
            input,
            config,
            timings,
            first_frame_done: false,
            last_render_instant: None,
        }
    }

    /// Evaluates layout/paint if dirty, rasterizes only damaged sub-rects, plans `DIRTYFB`
    /// commits, and hands off encoding/SPI transfer to [`FlushWorker`] without blocking UI work.
    pub fn render_and_submit(&mut self) -> KioskFrameReport {
        let now = Instant::now();
        let dt_secs = self
            .last_render_instant
            .replace(now)
            .map_or(0.0, |prev| (now - prev).as_secs_f32().clamp(0.0, 0.1));
        let _ = poll_async_tasks();
        self.clock.tick(dt_secs);
        let _ = self.tree.tick_transitions(dt_secs);
        let layout_t0 = Instant::now();
        let (did_relayout, dirty_rects) = self.tree.build_paint_scene_into(
            self.config.width,
            self.config.height,
            &mut self.scene,
        );
        let layout_us = layout_t0.elapsed().as_micros() as u64;
        if dirty_rects.is_empty() && self.first_frame_done {
            return KioskFrameReport {
                flush_seq: 0,
                did_relayout,
                damage_rect_count: 0,
                commit_count: 0,
                stats: FrameRenderStats::default(),
            };
        }

        let plan = plan_dirtyfb_commits(
            &dirty_rects,
            self.config.width,
            self.config.height,
            self.config.split_ratio_x10,
        );
        let render_t0 = Instant::now();
        let stats = self
            .renderer
            .render_scene_damaged(&self.scene, &plan.commits);
        let render_us = render_t0.elapsed().as_micros() as u64;

        let flush_seq = if let Some(rgba8) = self.renderer.framebuffer_rgba() {
            self.worker
                .submit_frame(rgba8, self.config.width, self.config.height, &plan.commits)
        } else {
            0
        };

        if !self.first_frame_done {
            self.first_frame_done = true;
            self.timings.first_layout_us = layout_us;
            self.timings.first_render_us = render_us;
        }
        KioskFrameReport {
            flush_seq,
            did_relayout,
            damage_rect_count: dirty_rects.len(),
            commit_count: plan.commits.len(),
            stats,
        }
    }

    /// Blocks until `flush_seq` finishes committing on the background worker.
    pub fn wait_flushed(&mut self, flush_seq: u64) -> FlushWorkerStats {
        let stats = self.worker.wait_until_flushed(flush_seq);
        if self.timings.first_flush_us == 0 && stats.completed_seq > 0 {
            self.timings.first_flush_us = stats.last_flush_us;
        }
        stats
    }

    /// Dispatches one `evdev` hardware input event into the managed `ViewTree`.
    pub fn dispatch_evdev(
        &mut self,
        event: EvdevInputEvent,
        focusables: &[FocusableCandidate],
        current_focus: Option<NodeId>,
    ) -> bool {
        self.input.dispatch(
            &mut self.tree,
            &mut self.clock,
            event,
            focusables,
            current_focus,
        )
    }

    /// Provides mutable access to the hosted [`ViewTree`].
    pub fn tree_mut(&mut self) -> &mut ViewTree {
        &mut self.tree
    }

    /// Provides mutable access to [`KioskInputRouter`] (calibration / IME toggle).
    pub fn input_mut(&mut self) -> &mut KioskInputRouter {
        &mut self.input
    }

    /// Returns measured cold-boot stage timings in microseconds.
    pub fn boot_timings(&self) -> BootStageTimings {
        self.timings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drm_sink::MockDrmSink;
    use krui_signals::Signal;
    use krui_style::{CompiledStyle, Dimension, InsetEdges, LayoutStyle, PaintStyle, PositionMode};

    fn abs_box(x: f32, y: f32, w: f32, h: f32, color: Color) -> CompiledStyle {
        CompiledStyle {
            layout: LayoutStyle {
                position: PositionMode::Absolute,
                inset: InsetEdges {
                    left: Dimension::Px(x),
                    top: Dimension::Px(y),
                    ..InsetEdges::AUTO
                },
                width: Dimension::Px(w),
                height: Dimension::Px(h),
                ..LayoutStyle::default()
            },
            paint: PaintStyle {
                background_color: color,
                ..PaintStyle::default()
            },
            ..CompiledStyle::default()
        }
    }

    #[test]
    fn kiosk_host_submits_full_first_frame_then_partial_and_split_commits() {
        let sink = MockDrmSink::new();
        let commits = sink.log_handle();
        let mut tree = ViewTree::new();
        let root = tree.create_compiled_node(
            None,
            "div",
            None,
            None,
            abs_box(0.0, 0.0, 480.0, 320.0, Color::BLACK),
            false,
            false,
            false,
            false,
        );
        let tl_node = tree.create_compiled_node(
            Some(root),
            "div",
            None,
            None,
            abs_box(10.0, 10.0, 40.0, 20.0, Color::WHITE),
            false,
            false,
            false,
            false,
        );
        let br_node = tree.create_compiled_node(
            Some(root),
            "div",
            None,
            None,
            abs_box(420.0, 280.0, 40.0, 20.0, Color::WHITE),
            false,
            false,
            false,
            false,
        );
        let sig_tl = Signal::new(1.0_f32);
        let sig_br = Signal::new(1.0_f32);
        tree.bind_paint_opacity(tl_node, sig_tl);
        tree.bind_paint_opacity(br_node, sig_br);

        let mut host = KioskAppHost::new(tree, sink, KioskHostConfig::default());
        let first = host.render_and_submit();
        assert_eq!(first.commit_count, 1);
        let _ = host.wait_flushed(first.flush_seq);
        assert_eq!(commits.lock().unwrap()[0].payload_bytes, 480 * 320 * 2);

        // Idle frame emits 0 commits (0% SPI/CPU burn).
        let idle = host.render_and_submit();
        assert_eq!(idle.commit_count, 0);

        // Small local change emits 1 small partial commit strictly below full screen size.
        sig_tl.set(0.5);
        let partial = host.render_and_submit();
        assert_eq!(partial.commit_count, 1);
        let _ = host.wait_flushed(partial.flush_seq);
        assert!(commits.lock().unwrap().last().unwrap().payload_bytes < 480 * 320 * 2);

        // Distant changes split into 2 DIRTYFB commits instead of bloating to union bbox.
        sig_tl.set(0.8);
        sig_br.set(0.4);
        let split = host.render_and_submit();
        assert_eq!(split.commit_count, 2);
        let _ = host.wait_flushed(split.flush_seq);
        assert_eq!(commits.lock().unwrap().len(), 4);
    }
}
