//! Hardware-accelerated `wgpu::Surface` presenter sharing `Arc<GpuContext>`.

use crate::gpu_context::GpuContext;
use crate::wgpu_compositor::WgpuFrameResources;
use krui_core::{ExternalTextureId, FrameRenderStats, GpuResourceLimits, PaintScene};
use std::sync::Arc;

/// Hardware-accelerated `wgpu::Surface` presenter for desktop windows (`winit::window::Window`).
pub struct WindowSurfacePresenter {
    surface: wgpu::Surface<'static>,
    context: Arc<GpuContext>,
    config: wgpu::SurfaceConfiguration,
    resources: WgpuFrameResources,
    limits: GpuResourceLimits,
}

impl WindowSurfacePresenter {
    /// Creates a new `WindowSurfacePresenter` with `wgpu::PowerPreference::HighPerformance`.
    pub fn try_new(
        window_target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        limits: GpuResourceLimits,
    ) -> Option<Self> {
        Self::try_new_with_power_preference(
            window_target,
            width,
            height,
            limits,
            wgpu::PowerPreference::HighPerformance,
        )
    }

    /// Creates a `WindowSurfacePresenter` reusing an existing `Arc<GpuContext>` when surface-compatible
    /// (avoiding duplicate `wgpu::Instance`/`Device`/`Queue` creation).
    pub fn try_new_with_context(
        window_target: impl Into<wgpu::SurfaceTarget<'static>>,
        shared_context: Option<Arc<GpuContext>>,
        width: u32,
        height: u32,
        limits: GpuResourceLimits,
    ) -> Option<Self> {
        let target: wgpu::SurfaceTarget<'static> = window_target.into();
        if let Some(ctx) = shared_context {
            if let (Some(inst), Some(adapter)) = (ctx.instance(), ctx.adapter()) {
                if let Ok(surface) = inst.create_surface(target) {
                    if adapter.is_surface_supported(&surface) {
                        return Self::configure_from_parts(surface, ctx, width, height, limits);
                    }
                }
                return None;
            }
        }
        Self::try_new_with_power_preference(
            target,
            width,
            height,
            limits,
            wgpu::PowerPreference::HighPerformance,
        )
    }

    /// Creates a new `WindowSurfacePresenter` with an explicit [`wgpu::PowerPreference`].
    pub fn try_new_with_power_preference(
        window_target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        limits: GpuResourceLimits,
        power_preference: wgpu::PowerPreference,
    ) -> Option<Self> {
        let (ctx, surface) = GpuContext::try_new_for_surface(window_target, power_preference)?;
        Self::configure_from_parts(surface, ctx, width, height, limits)
    }

    fn configure_from_parts(
        surface: wgpu::Surface<'static>,
        context: Arc<GpuContext>,
        width: u32,
        height: u32,
        limits: GpuResourceLimits,
    ) -> Option<Self> {
        let limits = limits.normalized();
        let adapter = context.adapter()?;
        let config = build_surface_config(&surface, adapter, width, height, limits)?;
        surface.configure(context.device(), &config);
        let _ = context.pipelines_for(config.format);
        let resources = WgpuFrameResources::new(context.device());
        Some(Self {
            surface,
            context,
            config,
            resources,
            limits,
        })
    }

    /// Returns the shared `Arc<GpuContext>` backing this window presenter.
    pub fn gpu_context(&self) -> &Arc<GpuContext> {
        &self.context
    }

    /// Registers an external host `wgpu::TextureView` for [`ExternalTextureId`] rendering.
    pub fn register_external_texture(&self, id: ExternalTextureId, view: &wgpu::TextureView) {
        self.context.register_external_texture(id, view);
    }

    /// Unregisters a previously registered [`ExternalTextureId`].
    pub fn unregister_external_texture(&self, id: ExternalTextureId) {
        self.context.unregister_external_texture(id);
    }

    /// Reconfigures the swapchain dimensions on window resize.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 && (self.config.width != width || self.config.height != height) {
            let max_dim = self.limits.max_viewport_dimension.max(1);
            self.config.width = width.clamp(1, max_dim);
            self.config.height = height.clamp(1, max_dim);
            self.surface.configure(self.context.device(), &self.config);
        }
    }

    /// Composites and presents `scene` onto the window swapchain surface.
    pub fn present_scene(&mut self, scene: &PaintScene) -> Option<FrameRenderStats> {
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(_) => {
                self.surface.configure(self.context.device(), &self.config);
                self.surface.get_current_texture().ok()?
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let surface_scale = if self.config.width > 0 && self.config.height > 0 {
            scene.scale_factor.clamp(1.0, 4.0)
        } else {
            1.0
        };
        let sampleable = self
            .config
            .usage
            .contains(wgpu::TextureUsages::TEXTURE_BINDING);
        let stats = self.resources.composite_scene(
            &self.context,
            scene,
            &view,
            self.config.format,
            sampleable,
            (self.config.width, self.config.height),
            surface_scale,
            self.limits,
        );
        frame.present();
        Some(stats)
    }
}

fn build_surface_config(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    width: u32,
    height: u32,
    limits: GpuResourceLimits,
) -> Option<wgpu::SurfaceConfiguration> {
    // CSS Color 4 blends in gamma-encoded sRGB; prefer a non-`*Srgb` surface so the
    // swapchain matches `Rgba8Unorm` offscreen targets and `TinySkiaRenderer`.
    let caps = surface.get_capabilities(adapter);
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .or_else(|| caps.formats.first().copied())?;
    let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
        wgpu::PresentMode::Mailbox
    } else {
        wgpu::PresentMode::AutoVsync
    };
    let usage = if caps.usages.contains(wgpu::TextureUsages::TEXTURE_BINDING) {
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
    } else {
        wgpu::TextureUsages::RENDER_ATTACHMENT
    };
    Some(wgpu::SurfaceConfiguration {
        usage,
        format,
        width: width.clamp(1, limits.max_viewport_dimension),
        height: height.clamp(1, limits.max_viewport_dimension),
        present_mode,
        alpha_mode: caps
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Auto),
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    })
}
