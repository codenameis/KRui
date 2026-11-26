//! Raw `wgpu` original-mode viewport helper (`RawWgpuViewport`) supporting custom
//! `wgpu::RenderPass` / command encoders, MSAA resolve, depth buffer, and automatic
//! zero-copy registration to [`ExternalTextureId`] on KRui's shared [`GpuContext`].

use krui_core::{ExternalTextureId, NodeId, ViewTree};
use krui_renderer::GpuContext;
use std::sync::Arc;

/// Configuration for [`RawWgpuViewport`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawWgpuViewportConfig {
    /// Physical pixel width (`>= 1`).
    pub width: u32,
    /// Physical pixel height (`>= 1`).
    pub height: u32,
    /// Resolved color texture format registered with [`ExternalTextureId`] (default `Rgba8Unorm`).
    pub color_format: wgpu::TextureFormat,
    /// MSAA sample count (`1` for single-sampled or `4` for 4x MSAA with resolve).
    pub sample_count: u32,
    /// Optional depth/stencil attachment format (default `Some(TextureFormat::Depth32Float)`).
    pub depth_format: Option<wgpu::TextureFormat>,
}

impl Default for RawWgpuViewportConfig {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            color_format: wgpu::TextureFormat::Rgba8Unorm,
            sample_count: 1,
            depth_format: Some(wgpu::TextureFormat::Depth32Float),
        }
    }
}

/// Borrowed attachment views passed to custom raw `wgpu` render closures.
pub struct RawWgpuTargets<'a> {
    /// Active render attachment view (4x MSAA view when `sample_count > 1`, else resolved view).
    pub color_view: &'a wgpu::TextureView,
    /// MSAA resolve target (`Some(&resolved_color_view)` when `sample_count > 1`).
    pub resolve_target: Option<&'a wgpu::TextureView>,
    /// Depth attachment view when `depth_format` is configured.
    pub depth_view: Option<&'a wgpu::TextureView>,
    /// Physical pixel dimensions `(width, height)`.
    pub size: (u32, u32),
    /// Resolved color format.
    pub color_format: wgpu::TextureFormat,
    /// Depth attachment format, if enabled.
    pub depth_format: Option<wgpu::TextureFormat>,
    /// MSAA sample count (`1` or `4`).
    pub sample_count: u32,
}

/// Manages offscreen raw `wgpu` color/MSAA/depth render targets registered as a KRui
/// [`ExternalTextureId`] on a shared [`GpuContext`].
pub struct RawWgpuViewport {
    gpu: Arc<GpuContext>,
    texture_id: ExternalTextureId,
    config: RawWgpuViewportConfig,
    color_tex: wgpu::Texture,
    color_view: wgpu::TextureView,
    msaa_tex: Option<(wgpu::Texture, wgpu::TextureView)>,
    depth_tex: Option<(wgpu::Texture, wgpu::TextureView)>,
}

impl RawWgpuViewport {
    /// Allocates offscreen `wgpu` textures and registers `texture_id` on `gpu`.
    pub fn new(
        gpu: Arc<GpuContext>,
        texture_id: ExternalTextureId,
        mut config: RawWgpuViewportConfig,
    ) -> Self {
        config.width = config.width.max(1);
        config.height = config.height.max(1);
        config.sample_count = normalize_sample_count(config.sample_count);
        let targets = allocate_viewport_targets(gpu.device(), &config);
        gpu.register_external_texture(texture_id, &targets.color_view);
        Self {
            gpu,
            texture_id,
            config,
            color_tex: targets.color_tex,
            color_view: targets.color_view,
            msaa_tex: targets.msaa_tex,
            depth_tex: targets.depth_tex,
        }
    }

    /// Reallocates offscreen targets when `(width, height)` changes and re-registers `texture_id`.
    /// Returns `true` when targets were resized.
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        let nw = width.max(1);
        let nh = height.max(1);
        if self.config.width == nw && self.config.height == nh {
            return false;
        }
        self.config.width = nw;
        self.config.height = nh;
        let targets = allocate_viewport_targets(self.gpu.device(), &self.config);
        self.color_tex = targets.color_tex;
        self.color_view = targets.color_view;
        self.msaa_tex = targets.msaa_tex;
        self.depth_tex = targets.depth_tex;
        self.gpu
            .register_external_texture(self.texture_id, &self.color_view);
        true
    }

    /// Synchronizes this viewport's physical size and `ExternalTexture` binding with `node_id`'s
    /// latest laid-out bounds in `tree`. Returns `true` when the viewport was resized.
    pub fn sync_with_node(
        &mut self,
        tree: &mut ViewTree,
        node_id: NodeId,
        scale_factor: f32,
    ) -> bool {
        let Some(rect) = tree.node_layout(node_id) else {
            return false;
        };
        let scale = scale_factor.clamp(1.0, 4.0);
        let pw = (rect.width * scale).round().max(1.0) as u32;
        let ph = (rect.height * scale).round().max(1.0) as u32;
        let resized = self.resize(pw, ph);
        tree.mount_external_texture(node_id, self.texture_id, pw, ph);
        resized
    }

    /// Returns the shared [`GpuContext`].
    pub fn gpu_context(&self) -> &Arc<GpuContext> {
        &self.gpu
    }

    /// Returns a reference to the shared `wgpu::Device`.
    pub fn device(&self) -> &wgpu::Device {
        self.gpu.device()
    }

    /// Returns a reference to the shared `wgpu::Queue`.
    pub fn queue(&self) -> &wgpu::Queue {
        self.gpu.queue()
    }

    /// Returns the bound [`ExternalTextureId`].
    pub fn texture_id(&self) -> ExternalTextureId {
        self.texture_id
    }

    /// Returns the physical texture size `(width, height)`.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Returns the resolved color texture format.
    pub fn color_format(&self) -> wgpu::TextureFormat {
        self.config.color_format
    }

    /// Returns the depth attachment format, if enabled.
    pub fn depth_format(&self) -> Option<wgpu::TextureFormat> {
        self.config.depth_format
    }

    /// Returns the normalized MSAA sample count (`1` or `4`).
    pub fn sample_count(&self) -> u32 {
        self.config.sample_count
    }

    /// Returns the resolved single-sampled `TextureView` registered with KRui.
    pub fn resolved_color_view(&self) -> &wgpu::TextureView {
        &self.color_view
    }

    /// Records custom `wgpu` commands targeting this viewport and submits them to the shared queue.
    pub fn render<F>(&self, label: &str, record: F)
    where
        F: FnOnce(&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder, RawWgpuTargets<'_>),
    {
        let mut encoder = self
            .gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
        let (color_view, resolve_target) = match &self.msaa_tex {
            Some((_, msaa_view)) => (msaa_view, Some(&self.color_view)),
            None => (&self.color_view, None),
        };
        let depth_view = self.depth_tex.as_ref().map(|(_, dv)| dv);
        let targets = RawWgpuTargets {
            color_view,
            resolve_target,
            depth_view,
            size: (self.config.width, self.config.height),
            color_format: self.config.color_format,
            depth_format: self.config.depth_format,
            sample_count: self.config.sample_count,
        };
        record(self.gpu.device(), self.gpu.queue(), &mut encoder, targets);
        self.gpu.queue().submit(std::iter::once(encoder.finish()));
    }

    /// Opens a configured `wgpu::RenderPass` (handling MSAA resolve & depth attachment) and submits.
    pub fn render_pass<F>(
        &self,
        label: &str,
        clear_color: wgpu::Color,
        clear_depth: Option<f32>,
        draw: F,
    ) where
        F: FnOnce(&mut wgpu::RenderPass<'_>),
    {
        self.render(label, |_device, _queue, encoder, targets| {
            let depth_att = targets
                .depth_view
                .map(|view| wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: match clear_depth {
                            Some(d) => wgpu::LoadOp::Clear(d),
                            None => wgpu::LoadOp::Load,
                        },
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: targets.color_view,
                    resolve_target: targets.resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: depth_att,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            draw(&mut pass);
        });
    }
}

impl Drop for RawWgpuViewport {
    fn drop(&mut self) {
        self.gpu.unregister_external_texture(self.texture_id);
    }
}

fn normalize_sample_count(count: u32) -> u32 {
    if count >= 4 {
        4
    } else {
        1
    }
}

struct AllocatedTargets {
    color_tex: wgpu::Texture,
    color_view: wgpu::TextureView,
    msaa_tex: Option<(wgpu::Texture, wgpu::TextureView)>,
    depth_tex: Option<(wgpu::Texture, wgpu::TextureView)>,
}

fn allocate_viewport_targets(
    device: &wgpu::Device,
    cfg: &RawWgpuViewportConfig,
) -> AllocatedTargets {
    let extent = wgpu::Extent3d {
        width: cfg.width.max(1),
        height: cfg.height.max(1),
        depth_or_array_layers: 1,
    };
    let color_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("KRui RawWgpu Color Target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: cfg.color_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color_tex.create_view(&wgpu::TextureViewDescriptor::default());
    let msaa_tex = (cfg.sample_count > 1).then(|| {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("KRui RawWgpu MSAA Target"),
            size: extent,
            mip_level_count: 1,
            sample_count: cfg.sample_count,
            dimension: wgpu::TextureDimension::D2,
            format: cfg.color_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    });
    let depth_tex = cfg.depth_format.map(|df| {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("KRui RawWgpu Depth Target"),
            size: extent,
            mip_level_count: 1,
            sample_count: cfg.sample_count,
            dimension: wgpu::TextureDimension::D2,
            format: df,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    });
    AllocatedTargets {
        color_tex,
        color_view,
        msaa_tex,
        depth_tex,
    }
}
