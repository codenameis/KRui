//! Mode A (`BevyEditorViewportHost`): KRui hosts an offscreen Bevy / custom `wgpu` 3D scene
//! inside an [`ExternalTextureId`] viewport slot with bidirectional ECS inspector sync.

use crate::ecs_sync::{EcsSyncBridge, ReactiveEcsInspector};
use krui_core::{ExternalTextureId, NodeId, ViewTree};
use krui_engine_host::{RawWgpuTargets, RawWgpuViewport, RawWgpuViewportConfig};
use krui_renderer::GpuContext;
use std::sync::Arc;

/// Configuration for [`BevyEditorViewportHost`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BevyEditorConfig {
    /// Target [`ExternalTextureId`] slot displayed by the KRui viewport node.
    pub texture_id: ExternalTextureId,
    /// Initial offscreen viewport width in pixels.
    pub width: u32,
    /// Initial offscreen viewport height in pixels.
    pub height: u32,
    /// Offscreen render target format (`Rgba8Unorm` recommended).
    pub color_format: wgpu::TextureFormat,
    /// MSAA sample count (`1` or `4`).
    pub sample_count: u32,
    /// Depth buffer format (`Some(TextureFormat::Depth32Float)` by default).
    pub depth_format: Option<wgpu::TextureFormat>,
}

impl Default for BevyEditorConfig {
    fn default() -> Self {
        Self {
            texture_id: ExternalTextureId(100),
            width: 1280,
            height: 720,
            color_format: wgpu::TextureFormat::Rgba8Unorm,
            sample_count: 1,
            depth_format: Some(wgpu::TextureFormat::Depth32Float),
        }
    }
}

/// Mode A host: renders/mounts a 3D Bevy `RenderTarget` or raw `wgpu` pass into a KRui editor UI
/// and synchronizes inspector edits bidirectionally with the ECS world.
pub struct BevyEditorViewportHost {
    viewport: RawWgpuViewport,
    inspector: ReactiveEcsInspector,
}

impl BevyEditorViewportHost {
    /// Creates a new Mode A host bound to KRui's shared [`GpuContext`].
    pub fn new(gpu: Arc<GpuContext>, config: BevyEditorConfig) -> Self {
        let viewport = RawWgpuViewport::new(
            gpu,
            config.texture_id,
            RawWgpuViewportConfig {
                width: config.width,
                height: config.height,
                color_format: config.color_format,
                sample_count: config.sample_count,
                depth_format: config.depth_format,
            },
        );
        Self {
            viewport,
            inspector: ReactiveEcsInspector::new(),
        }
    }

    /// Registers an external Bevy `RenderTarget` `wgpu::TextureView` directly on this viewport's
    /// [`ExternalTextureId`] slot without copying pixels.
    pub fn register_bevy_render_target(&self, view: &wgpu::TextureView) {
        self.viewport
            .gpu_context()
            .register_external_texture(self.viewport.texture_id(), view);
    }

    /// Returns mutable access to the underlying [`RawWgpuViewport`].
    pub fn viewport_mut(&mut self) -> &mut RawWgpuViewport {
        &mut self.viewport
    }

    /// Returns shared access to the underlying [`RawWgpuViewport`].
    pub fn viewport(&self) -> &RawWgpuViewport {
        &self.viewport
    }

    /// Returns shared access to the reactive ECS inspector.
    pub fn inspector(&self) -> &ReactiveEcsInspector {
        &self.inspector
    }

    /// Returns mutable access to the reactive ECS inspector (to queue UI edits).
    pub fn inspector_mut(&mut self) -> &mut ReactiveEcsInspector {
        &mut self.inspector
    }

    /// Executes custom `wgpu` / Bevy offscreen commands against this viewport's targets.
    pub fn render_viewport<F>(&self, label: &str, record: F)
    where
        F: FnOnce(&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder, RawWgpuTargets<'_>),
    {
        self.viewport.render(label, record);
    }

    /// Synchronizes viewport layout size with `viewport_node`, flushes queued UI -> ECS patches,
    /// and pulls refreshed ECS snapshots into KRui signals. Returns `(applied_patches, pulled)`.
    pub fn sync_frame(
        &mut self,
        tree: &mut ViewTree,
        viewport_node: NodeId,
        scale_factor: f32,
        ecs: &mut impl EcsSyncBridge,
    ) -> (usize, bool) {
        let _ = self
            .viewport
            .sync_with_node(tree, viewport_node, scale_factor);
        let applied = self.inspector.flush_to_ecs(ecs);
        let pulled = self.inspector.pull_from_ecs(ecs);
        (applied, pulled)
    }
}
