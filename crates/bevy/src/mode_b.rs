//! Mode B (`BevyInPassHudHost`): shares Bevy's existing `wgpu::Device`/`Queue` via
//! `krui-engine-host` and composites KRui HUD overlays on top of a 3D frame (`LoadOp::Load`).

use krui_core::{DynamicSlotOverride, FrameRenderStats, Mods, PaintScene, PointerButton, ViewTree};
use krui_engine_host::{EngineHostConfig, EngineUiHost};
use krui_renderer::GpuContext;
use krui_style::DynamicSlotId;
use std::sync::Arc;

/// Configuration for [`BevyInPassHudHost`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BevyHudConfig {
    /// Initial physical/logical HUD viewport width.
    pub width: u32,
    /// Initial physical/logical HUD viewport height.
    pub height: u32,
    /// Display scale factor (`1.0..=4.0`).
    pub scale_factor: f32,
    /// Swapchain / 3D render target format (`wgpu::TextureFormat`).
    pub target_format: wgpu::TextureFormat,
}

impl Default for BevyHudConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            scale_factor: 1.0,
            target_format: wgpu::TextureFormat::Rgba8Unorm,
        }
    }
}

/// Mode B host: shares Bevy's `wgpu::Device`/`Queue` and composites KRui HUD UI in-pass
/// over Bevy's rendered 3D scene texture (`LoadOp::Load`).
pub struct BevyInPassHudHost {
    host: EngineUiHost,
}

impl BevyInPassHudHost {
    /// Initializes an in-pass Bevy HUD host bound to Bevy/Tokio's external `tokio::runtime::Handle`.
    pub fn new(
        tree: ViewTree,
        tokio_handle: tokio::runtime::Handle,
        config: BevyHudConfig,
    ) -> Self {
        let host = EngineUiHost::new_with_tokio_handle(
            tree,
            tokio_handle,
            EngineHostConfig {
                width: config.width.max(1),
                height: config.height.max(1),
                scale_factor: config.scale_factor.clamp(1.0, 4.0),
                target_format: config.target_format,
                initial_primitive_capacity: 256,
            },
        );
        Self { host }
    }

    /// Attaches Bevy's `wgpu::Device` & `wgpu::Queue` without opening a second GPU context.
    pub fn attach_render_device(&mut self, device: wgpu::Device, queue: wgpu::Queue) {
        self.host.attach_wgpu_device(device, queue);
    }

    /// Attaches a pre-built shared `Arc<GpuContext>`.
    pub fn attach_gpu_context(&mut self, context: Arc<GpuContext>) {
        self.host.attach_gpu_context(context);
    }

    /// Updates HUD viewport dimensions and DPI scale factor.
    pub fn set_viewport(&mut self, width: u32, height: u32, scale_factor: f32) {
        self.host.set_viewport(width, height, scale_factor);
    }

    /// Forwards a pointer button press or release from Bevy's window input loop.
    pub fn forward_pointer_button(
        &mut self,
        x: f32,
        y: f32,
        button: PointerButton,
        pressed: bool,
        mods: Mods,
        timestamp_ms: u64,
    ) -> bool {
        self.host
            .tree_mut()
            .dispatch_pointer_button(x, y, button, pressed, mods, timestamp_ms)
    }

    /// Forwards cursor movement `(x, y)` from Bevy's window input loop.
    pub fn forward_cursor_moved(&mut self, x: f32, y: f32) -> bool {
        self.host.inject_pointer_move(x, y)
    }

    /// Forwards mouse wheel deltas `(dx, dy)` from Bevy's window input loop.
    pub fn forward_mouse_wheel(&mut self, x: f32, y: f32, dx: f32, dy: f32, mods: Mods) -> bool {
        self.host.tree_mut().dispatch_wheel(x, y, dx, dy, mods)
    }

    /// Maps a zero-relayout dynamic HUD slot index in the scene.
    pub fn bind_dynamic_slot(&mut self, id: DynamicSlotId, primitive_idx: usize) {
        self.host.bind_dynamic_slot(id, primitive_idx);
    }

    /// Overrides transform/opacity/color on a dynamic HUD slot with 0 relayouts.
    pub fn set_dynamic_slot_override(&mut self, id: DynamicSlotId, ovr: DynamicSlotOverride) {
        self.host.set_dynamic_slot_override(id, ovr);
    }

    /// Advances animations/transitions and updates the ping-pong `PaintScene`.
    pub fn tick(&mut self, dt_secs: f32) -> (&PaintScene, bool) {
        self.host.tick(dt_secs)
    }

    /// Composites the latest published HUD [`PaintScene`] onto Bevy's 3D target view
    /// using `wgpu::LoadOp::Load` so 3D scene contents underneath stay intact.
    pub fn composite_over_3d_view(&mut self, view: &wgpu::TextureView) -> Option<FrameRenderStats> {
        self.host.render_overlay_to_view(view)
    }

    /// Returns total Taffy relayout passes executed by the HUD `ViewTree`.
    pub fn relayout_passes(&self) -> u64 {
        self.host.relayout_passes()
    }

    /// Returns backing ping-pong scene capacities `[buf0, buf1]`.
    pub fn scene_pool_capacities(&self) -> [usize; 2] {
        self.host.scene_pool_capacities()
    }

    /// Returns mutable access to the hosted HUD [`ViewTree`].
    pub fn tree_mut(&mut self) -> &mut ViewTree {
        self.host.tree_mut()
    }
}
