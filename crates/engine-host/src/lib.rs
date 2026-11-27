//! 3D game/simulation engine in-pass UI host (`krui-engine-host`) with shared `wgpu::Device`,
//! external `tokio::runtime::Handle` reuse, ping-pong `PaintScenePool`, and 0-relayout fast-path.

#![forbid(unsafe_code)]

mod raw_wgpu;

pub use raw_wgpu::{RawWgpuTargets, RawWgpuViewport, RawWgpuViewportConfig};

use krui_core::{
    DynamicSlotBuffer, DynamicSlotOverride, ExternalTextureId, FrameRenderStats, GpuResourceLimits,
    PaintScene, PaintScenePool, ViewTree,
};
use krui_renderer::{GpuContext, WgpuRenderer};
use krui_signals::{init_async_runtime_mode, AsyncRuntimeMode, FrameClock};
use krui_style::{
    Color, ColorDepthBits, DamageTrackingMode, DisplayShape, DynamicSlotId, RootProfileAttrs,
    TierProfile,
};
use std::sync::Arc;

/// Configuration for [`EngineUiHost`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineHostConfig {
    /// Target viewport width in physical/logical pixels.
    pub width: u32,
    /// Target viewport height in physical/logical pixels.
    pub height: u32,
    /// Display DPI scale factor (`1.0..=4.0`).
    pub scale_factor: f32,
    /// Color format of the external engine render target (`wgpu::TextureFormat`).
    pub target_format: wgpu::TextureFormat,
    /// Initial `PaintScene` primitive capacity reserved across ping-pong scene buffers.
    pub initial_primitive_capacity: usize,
}

impl Default for EngineHostConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            scale_factor: 1.0,
            target_format: wgpu::TextureFormat::Rgba8Unorm,
            initial_primitive_capacity: 256,
        }
    }
}

/// Embedded UI host for 120 FPS 3D engines, reusing the host's GPU device and Tokio runtime.
pub struct EngineUiHost {
    tree: ViewTree,
    clock: FrameClock,
    scene_pool: PaintScenePool,
    dynamic_slots: DynamicSlotBuffer,
    renderer: Option<WgpuRenderer>,
    config: EngineHostConfig,
}

impl EngineUiHost {
    /// Initializes an engine UI host wired to an external [`tokio::runtime::Handle`] (`ExternalHandle`)
    /// with `DamageTrackingMode::DirtyFlagOnly` and a pre-reserved ping-pong `PaintScenePool`.
    pub fn new_with_tokio_handle(
        mut tree: ViewTree,
        tokio_handle: tokio::runtime::Handle,
        config: EngineHostConfig,
    ) -> Self {
        let _ = init_async_runtime_mode(AsyncRuntimeMode::ExternalHandle(tokio_handle));
        tree.set_scale_factor(config.scale_factor);
        tree.set_damage_tracking_mode(DamageTrackingMode::DirtyFlagOnly);
        tree.apply_root_profile_attrs(RootProfileAttrs {
            tier: TierProfile::FullGpu,
            shape: DisplayShape::Rectangular,
            color_depth: ColorDepthBits::Bpp24OrMore,
            reduced_motion: false,
        });
        let scene_pool = PaintScenePool::new(
            config.width,
            config.height,
            Color::TRANSPARENT,
            config.initial_primitive_capacity,
        );
        Self {
            tree,
            clock: FrameClock::new(),
            scene_pool,
            dynamic_slots: DynamicSlotBuffer::default(),
            renderer: None,
            config,
        }
    }

    /// Attaches an external engine `wgpu::Device` + `wgpu::Queue` without opening another adapter.
    pub fn attach_wgpu_device(&mut self, device: wgpu::Device, queue: wgpu::Queue) {
        self.renderer = Some(WgpuRenderer::from_device_with_size(
            device,
            queue,
            self.config.target_format,
            self.config.width,
            self.config.height,
            GpuResourceLimits::default(),
        ));
    }

    /// Attaches a pre-built shared `Arc<GpuContext>` to avoid duplicate shader compilations.
    pub fn attach_gpu_context(&mut self, context: Arc<GpuContext>) {
        self.renderer = Some(WgpuRenderer::from_context(
            context,
            self.config.target_format,
            self.config.width,
            self.config.height,
            GpuResourceLimits::default(),
        ));
    }

    /// Registers an external engine `wgpu::TextureView` for `<ExternalTexture>` HUD panels.
    pub fn register_external_texture(
        &self,
        id: ExternalTextureId,
        view: &wgpu::TextureView,
    ) -> bool {
        if let Some(renderer) = &self.renderer {
            renderer.register_external_texture(id, view);
            true
        } else {
            false
        }
    }

    /// Unregisters a previously registered external GPU texture slot.
    pub fn unregister_external_texture(&self, id: ExternalTextureId) {
        if let Some(renderer) = &self.renderer {
            renderer.unregister_external_texture(id);
        }
    }

    /// Updates host viewport size and DPI scale factor.
    pub fn set_viewport(&mut self, width: u32, height: u32, scale_factor: f32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.config.scale_factor = scale_factor.clamp(1.0, 4.0);
        self.tree.set_scale_factor(self.config.scale_factor);
    }

    /// Injects pointer press/move/release events from the game engine's input system.
    pub fn inject_pointer_down(&mut self, x: f32, y: f32) -> bool {
        self.tree.dispatch_pointer_down(x, y)
    }

    /// Injects pointer move events from the game engine's input system.
    pub fn inject_pointer_move(&mut self, x: f32, y: f32) -> bool {
        self.tree.dispatch_pointer_move(x, y)
    }

    /// Injects pointer release events from the game engine's input system.
    pub fn inject_pointer_up(&mut self, x: f32, y: f32) -> bool {
        let moved = self.tree.dispatch_pointer_move(x, y);
        let released = self.tree.dispatch_pointer_up();
        moved || released
    }

    /// Maps a [`DynamicSlotId`] to a primitive index in the assembled scene.
    pub fn bind_dynamic_slot(&mut self, id: DynamicSlotId, primitive_idx: usize) {
        self.dynamic_slots.bind_primitive(id, primitive_idx);
    }

    /// Updates a high-frequency HUD transform/opacity/color override for zero-relayout injection.
    pub fn set_dynamic_slot_override(&mut self, id: DynamicSlotId, ovr: DynamicSlotOverride) {
        self.dynamic_slots.set_override(id, ovr);
    }

    /// Advances frame clock, CSS transitions, and async pollers by `dt_secs`, then updates the
    /// ping-pong back buffer in-place (preserving `Vec` capacity) and publishes the front snapshot.
    pub fn tick(&mut self, dt_secs: f32) -> (&PaintScene, bool) {
        let _ = krui_signals::poll_async_tasks();
        self.clock.tick(dt_secs);
        let _ = self.tree.tick_transitions(dt_secs);
        let back = self.scene_pool.acquire_back(
            self.config.width,
            self.config.height,
            Color::TRANSPARENT,
            self.config.scale_factor,
        );
        let (did_relayout, _) =
            self.tree
                .build_paint_scene_into(self.config.width, self.config.height, back);
        self.dynamic_slots.apply_to_scene(back);
        (self.scene_pool.publish_back(), did_relayout)
    }

    /// Composites the latest published front [`PaintScene`] onto the engine's `wgpu::TextureView`
    /// using `LoadOp::Load` so 3D scene contents underneath remain intact.
    pub fn render_overlay_to_view(&mut self, view: &wgpu::TextureView) -> Option<FrameRenderStats> {
        let renderer = self.renderer.as_mut()?;
        let scene = self.scene_pool.front();
        Some(renderer.render_overlay_to_view(
            scene,
            view,
            (self.config.width, self.config.height),
            self.config.scale_factor,
        ))
    }

    /// Returns mutable access to the hosted [`ViewTree`].
    pub fn tree_mut(&mut self) -> &mut ViewTree {
        &mut self.tree
    }

    /// Returns backing primitive capacities `[buf0, buf1]` from the ping-pong [`PaintScenePool`].
    pub fn scene_pool_capacities(&self) -> [usize; 2] {
        self.scene_pool.capacities()
    }

    /// Returns cumulative Taffy layout passes performed on this host's `ViewTree`.
    pub fn relayout_passes(&self) -> u64 {
        self.tree.relayout_passes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use krui_style::{Affine2, CompiledStyle, Dimension, LayoutStyle, PaintStyle};

    fn hud_box(w: f32, h: f32, color: Color) -> CompiledStyle {
        CompiledStyle {
            layout: LayoutStyle {
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
    fn engine_host_120_frames_steady_state_zero_reallocs_and_zero_relayouts() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio current_thread runtime");

        let mut tree = ViewTree::new();
        let root = tree.create_compiled_node(
            None,
            "div",
            None,
            None,
            hud_box(1280.0, 720.0, Color::BLACK),
            false,
            false,
            false,
            false,
        );
        for _ in 0..8 {
            tree.create_compiled_node(
                Some(root),
                "div",
                None,
                None,
                hud_box(120.0, 32.0, Color::WHITE),
                false,
                false,
                false,
                false,
            );
        }

        let mut host = EngineUiHost::new_with_tokio_handle(
            tree,
            rt.handle().clone(),
            EngineHostConfig::default(),
        );
        host.bind_dynamic_slot(DynamicSlotId(0), 1);

        // Warmup ping & pong buffers.
        for _ in 0..2 {
            let _ = host.tick(1.0 / 120.0);
        }
        let baseline_caps = host.scene_pool_capacities();
        let baseline_relayouts = host.relayout_passes();

        for frame in 0..120 {
            host.set_dynamic_slot_override(
                DynamicSlotId(0),
                DynamicSlotOverride {
                    transform: Some(Affine2::translation(frame as f32 * 0.5, 0.0)),
                    opacity: Some(0.9),
                    background_color: None,
                },
            );
            let (scene, did_relayout) = host.tick(1.0 / 120.0);
            assert!(
                !did_relayout,
                "frame {frame} unexpectedly triggered relayout"
            );
            assert!(scene.primitives.len() >= 2);
        }

        assert_eq!(host.scene_pool_capacities(), baseline_caps);
        assert_eq!(host.relayout_passes(), baseline_relayouts);
    }

    #[test]
    fn engine_host_tick_delivers_spawn_async_updates() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("tokio runtime");
        let tree = ViewTree::new();
        let mut host = EngineUiHost::new_with_tokio_handle(
            tree,
            rt.handle().clone(),
            EngineHostConfig::default(),
        );
        let sig = krui_signals::Signal::new(0_i32);
        let _handle = krui_signals::spawn_tokio_task(async { 42 }, move |v| sig.set(v));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while sig.get() != 42 && std::time::Instant::now() < deadline {
            let _ = host.tick(1.0 / 60.0);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(sig.get(), 42);
    }
}
