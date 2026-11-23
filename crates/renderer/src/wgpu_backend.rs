//! Primary `wgpu` GPU backend powered by shared `Arc<GpuContext>` and SDF quad instancing.

use crate::gpu_context::GpuContext;
use crate::wgpu_compositor::WgpuFrameResources;
use crate::wgpu_pipeline::create_sampleable_target;
use crate::RendererBackend;
use krui_core::{
    ActiveBackendKind, ExternalTextureId, FrameRenderStats, GpuResourceLimits, PaintScene,
};
use std::sync::Arc;

/// Primary GPU-accelerated renderer powered by `wgpu` (Vulkan/DX12/Metal/GLES/WebGPU).
pub struct WgpuRenderer {
    context: Arc<GpuContext>,
    resources: WgpuFrameResources,
    target_format: wgpu::TextureFormat,
    offscreen_target: Option<(wgpu::Texture, wgpu::TextureView)>,
    target_size: (u32, u32),
    limits: GpuResourceLimits,
    has_rendered: bool,
    readback_dirty: bool,
    readback_pixels: Vec<u8>,
    readback_staging_buf: Option<(wgpu::Buffer, u64)>,
    readback_staging_allocations: usize,
}

impl WgpuRenderer {
    /// Probes the system for a `wgpu` adapter and creates a GPU renderer with a new [`GpuContext`].
    pub fn try_new(width: u32, height: u32, limits: GpuResourceLimits) -> Option<Self> {
        Self::try_new_with_power_preference(
            width,
            height,
            limits,
            wgpu::PowerPreference::HighPerformance,
        )
    }

    /// Probes the system for a `wgpu` adapter with an explicit [`wgpu::PowerPreference`].
    pub fn try_new_with_power_preference(
        width: u32,
        height: u32,
        limits: GpuResourceLimits,
        power_preference: wgpu::PowerPreference,
    ) -> Option<Self> {
        let ctx = GpuContext::try_new_with_power_preference(power_preference)?;
        Some(Self::from_context(
            ctx,
            wgpu::TextureFormat::Rgba8Unorm,
            width,
            height,
            limits,
        ))
    }

    /// Constructs a `WgpuRenderer` sharing an existing `Arc<GpuContext>`.
    pub fn from_context(
        context: Arc<GpuContext>,
        target_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        limits: GpuResourceLimits,
    ) -> Self {
        let limits = limits.normalized();
        let vw = width.clamp(1, limits.max_viewport_dimension);
        let vh = height.clamp(1, limits.max_viewport_dimension);
        let resources = WgpuFrameResources::new(context.device());
        Self {
            context,
            resources,
            target_format,
            offscreen_target: None,
            target_size: (vw, vh),
            limits,
            has_rendered: false,
            readback_dirty: true,
            readback_pixels: Vec::new(),
            readback_staging_buf: None,
            readback_staging_allocations: 0,
        }
    }

    /// Constructs a `WgpuRenderer` directly from an external host's `wgpu::Device` and `wgpu::Queue`.
    pub fn from_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        Self::from_device_with_size(
            device,
            queue,
            target_format,
            800,
            600,
            GpuResourceLimits::default(),
        )
    }

    /// Constructs a `WgpuRenderer` from an external `wgpu::Device` with explicit initial dimensions.
    pub fn from_device_with_size(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        limits: GpuResourceLimits,
    ) -> Self {
        let ctx = GpuContext::from_device(device, queue);
        Self::from_context(ctx, target_format, width, height, limits)
    }

    /// Returns the shared `Arc<GpuContext>` backing this renderer.
    pub fn shared_context(&self) -> &Arc<GpuContext> {
        &self.context
    }

    /// Returns a reference to the underlying `wgpu::Device`.
    pub fn device(&self) -> &wgpu::Device {
        self.context.device()
    }

    /// Returns a reference to the underlying `wgpu::Queue`.
    pub fn queue(&self) -> &wgpu::Queue {
        self.context.queue()
    }

    /// Returns the configured external target `wgpu::TextureFormat`.
    pub fn target_format(&self) -> wgpu::TextureFormat {
        self.target_format
    }

    /// Returns the physical target size `(width, height)` in pixels.
    pub fn target_size(&self) -> (u32, u32) {
        self.target_size
    }

    /// Registers an external host `wgpu::TextureView` for [`ExternalTextureId`] slots.
    pub fn register_external_texture(&self, id: ExternalTextureId, view: &wgpu::TextureView) {
        self.context.register_external_texture(id, view);
    }

    /// Removes a previously registered [`ExternalTextureId`] binding.
    pub fn unregister_external_texture(&self, id: ExternalTextureId) {
        self.context.unregister_external_texture(id);
    }

    /// Returns `(cache_hits, cache_misses)` for the `<canvas>` LRU mesh cache.
    pub fn canvas_cache_stats(&self) -> (u64, u64) {
        self.resources.canvas_cache.stats()
    }

    /// Returns the number of GPU staging buffer allocations performed for readback.
    pub fn readback_staging_allocations(&self) -> usize {
        self.readback_staging_allocations
    }

    /// Returns `true` when the lazy CPU-readback offscreen target texture has been allocated.
    pub fn has_offscreen_target(&self) -> bool {
        self.offscreen_target.is_some()
    }

    /// Renders `scene` directly into an external host's `wgpu::TextureView` (clearing with `scene.clear_color`).
    pub fn render_to_view(
        &mut self,
        scene: &PaintScene,
        view: &wgpu::TextureView,
        size: (u32, u32),
        scale_factor: f32,
    ) -> FrameRenderStats {
        self.render_to_view_internal(scene, view, size, scale_factor, true)
    }

    /// Renders `scene` as a HUD overlay on top of an external host's `wgpu::TextureView`
    /// (`wgpu::LoadOp::Load`), preserving existing 3D engine or video pixels underneath.
    pub fn render_overlay_to_view(
        &mut self,
        scene: &PaintScene,
        view: &wgpu::TextureView,
        size: (u32, u32),
        scale_factor: f32,
    ) -> FrameRenderStats {
        self.render_to_view_internal(scene, view, size, scale_factor, false)
    }

    fn render_to_view_internal(
        &mut self,
        scene: &PaintScene,
        view: &wgpu::TextureView,
        size: (u32, u32),
        scale_factor: f32,
        clear_root: bool,
    ) -> FrameRenderStats {
        let max_dim = self.limits.max_viewport_dimension.max(1);
        let vw = size.0.clamp(1, max_dim);
        let vh = size.1.clamp(1, max_dim);
        let surface_scale = scale_factor.clamp(1.0, 4.0);
        let stats = self.resources.composite_scene_with_load(
            &self.context,
            scene,
            view,
            self.target_format,
            false,
            clear_root,
            (vw, vh),
            surface_scale,
            self.limits,
        );
        self.context.device().poll(wgpu::Maintain::Poll);
        stats
    }

    fn ensure_offscreen_target(&mut self, vw: u32, vh: u32) {
        if self.offscreen_target.is_none() || self.target_size != (vw, vh) {
            let (tex, view, _) = create_sampleable_target(
                self.context.device(),
                &self.context.aux_bgl,
                vw,
                vh,
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureUsages::COPY_SRC,
            );
            self.offscreen_target = Some((tex, view));
            self.target_size = (vw, vh);
            self.resources.invalidate_root_blur_bg();
            self.readback_pixels.resize((vw * vh * 4) as usize, 0);
            self.readback_dirty = true;
        }
    }

    fn ensure_readback_staging_buffer(&mut self, required_bytes: u64) {
        let needs_new_buf = match &self.readback_staging_buf {
            Some((_, cap)) => *cap < required_bytes,
            None => true,
        };
        if needs_new_buf {
            let new_buf = self
                .context
                .device()
                .create_buffer(&wgpu::BufferDescriptor {
                    label: Some("KRui GPU Readback Buffer"),
                    size: required_bytes,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
            self.readback_staging_buf = Some((new_buf, required_bytes));
            self.readback_staging_allocations += 1;
        }
    }

    /// Reads the GPU color target back into CPU RGBA8 memory and returns the pixel slice.
    pub fn readback_framebuffer(&mut self) -> Option<&[u8]> {
        if !self.has_rendered {
            return None;
        }
        if !self.readback_dirty {
            return Some(&self.readback_pixels);
        }
        let (vw, vh) = self.target_size;
        let bytes_per_row = (vw * 4 + 255) & !255;
        let required_bytes = (bytes_per_row * vh) as u64;
        self.ensure_readback_staging_buffer(required_bytes);
        self.submit_readback_copy(vw, vh, bytes_per_row);
        self.copy_mapped_staging_to_pixels(vw, vh, bytes_per_row, required_bytes);
        self.readback_dirty = false;
        Some(&self.readback_pixels)
    }

    fn submit_readback_copy(&self, vw: u32, vh: u32, bytes_per_row: u32) {
        let Some((target_tex, _)) = &self.offscreen_target else {
            return;
        };
        let (readback_buf, _) = self
            .readback_staging_buf
            .as_ref()
            .expect("staging buffer initialized");
        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("KRui Readback Copy Encoder"),
                });
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: target_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: readback_buf,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(vh),
                },
            },
            wgpu::Extent3d {
                width: vw,
                height: vh,
                depth_or_array_layers: 1,
            },
        );
        self.context
            .queue()
            .submit(std::iter::once(encoder.finish()));
    }

    fn copy_mapped_staging_to_pixels(
        &mut self,
        vw: u32,
        vh: u32,
        bytes_per_row: u32,
        required_bytes: u64,
    ) {
        let (readback_buf, _) = self
            .readback_staging_buf
            .as_ref()
            .expect("staging buffer initialized");
        let slice = readback_buf.slice(0..required_bytes);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.context.device().poll(wgpu::Maintain::Wait);
        let mapped = slice.get_mapped_range();
        self.readback_pixels.resize((vw * vh * 4) as usize, 0);
        for row in 0..vh as usize {
            let src_start = row * bytes_per_row as usize;
            let dst_start = row * (vw as usize * 4);
            self.readback_pixels[dst_start..dst_start + (vw as usize * 4)]
                .copy_from_slice(&mapped[src_start..src_start + (vw as usize * 4)]);
        }
        drop(mapped);
        readback_buf.unmap();
    }

    /// Renders `scene` via `wgpu`, reads back the attachment, and saves it as a PNG file.
    pub fn render_and_save_png(
        &mut self,
        scene: &PaintScene,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), String> {
        let _ = self.render_scene(scene);
        let (vw, vh) = self.target_size;
        let rgba = self
            .readback_framebuffer()
            .ok_or("No rendered framebuffer available")?;
        let mut pixmap = tiny_skia::Pixmap::new(vw, vh).ok_or("Failed to allocate pixmap")?;
        pixmap.data_mut().copy_from_slice(rgba);
        pixmap
            .save_png(path)
            .map_err(|e| format!("Failed to save WGPU PNG: {e}"))
    }
}

impl RendererBackend for WgpuRenderer {
    fn backend_kind(&self) -> ActiveBackendKind {
        ActiveBackendKind::Wgpu
    }

    fn gpu_context(&self) -> Option<Arc<GpuContext>> {
        Some(self.context.clone())
    }

    fn has_offscreen_target(&self) -> bool {
        self.offscreen_target.is_some()
    }

    fn render_scene(&mut self, scene: &PaintScene) -> FrameRenderStats {
        let max_dim = self.limits.max_viewport_dimension.max(1);
        let scale = scene.scale_factor.clamp(1.0, 4.0);
        let vw = ((scene.width as f32 * scale).round() as u32).clamp(1, max_dim);
        let vh = ((scene.height as f32 * scale).round() as u32).clamp(1, max_dim);
        self.ensure_offscreen_target(vw, vh);
        let target_view = &self
            .offscreen_target
            .as_ref()
            .expect("offscreen target allocated")
            .1;
        let stats = self.resources.composite_scene(
            &self.context,
            scene,
            target_view,
            wgpu::TextureFormat::Rgba8Unorm,
            true,
            (vw, vh),
            scale,
            self.limits,
        );
        self.context.device().poll(wgpu::Maintain::Poll);
        self.has_rendered = true;
        self.readback_dirty = true;
        stats
    }

    fn framebuffer_dimensions(&self) -> Option<(u32, u32)> {
        self.has_rendered.then_some(self.target_size)
    }

    fn framebuffer_rgba(&mut self) -> Option<&[u8]> {
        self.readback_framebuffer()
    }

    fn finish_gpu_work(&mut self) {
        self.context.device().poll(wgpu::Maintain::Wait);
    }
}
