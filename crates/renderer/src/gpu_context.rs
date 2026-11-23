//! Shared `Arc<GpuContext>` owning the `wgpu::Instance`, `Adapter`, `Device`, `Queue`,
//! compiled shader pipelines, Dual-Kawase blur pipelines, and multi-page `DualGlyphAtlas`.

use crate::blur_pass::BlurPipelineShared;
use crate::dual_atlas::DualGlyphAtlas;
use crate::image_cache::{ExternalTextureRegistry, ImageGpuCache};
use crate::wgpu_pipeline::{
    compile_format_pipelines, create_atlas_bind_group_layout, create_aux_bind_group_layout,
    create_dual_atlas_textures, create_dummy_aux_bind_group, FormatPipelines,
};
use krui_core::{DecodedImage, ExternalTextureId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

static GPU_INIT_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct SharedGpuAtlas {
    pub packer: DualGlyphAtlas,
    pub mask_tex: wgpu::Texture,
    pub color_tex: wgpu::Texture,
    pub _sampler: wgpu::Sampler,
}

/// Shared GPU context (`Arc<GpuContext>`) reused across `WgpuRenderer`, `AdaptiveRenderer`,
/// and `WindowSurfacePresenter` so KRui never creates duplicate `wgpu::Instance`/`Device` pairs.
pub struct GpuContext {
    instance: Option<wgpu::Instance>,
    adapter: Option<wgpu::Adapter>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    pub(crate) atlas_bind_group: wgpu::BindGroup,
    pub(crate) aux_bgl: wgpu::BindGroupLayout,
    pub(crate) _dummy_aux_tex: wgpu::Texture,
    pub(crate) dummy_aux_bg: wgpu::BindGroup,
    pub(crate) blur: BlurPipelineShared,
    pub(crate) atlas: Mutex<SharedGpuAtlas>,
    pub(crate) image_cache: Mutex<ImageGpuCache>,
    pub(crate) external_textures: Mutex<ExternalTextureRegistry>,
    format_pipelines: Mutex<HashMap<wgpu::TextureFormat, Arc<FormatPipelines>>>,
}

impl GpuContext {
    /// Probes the system for a `wgpu` adapter with `PowerPreference::HighPerformance`.
    pub fn try_new() -> Option<Arc<Self>> {
        Self::try_new_with_power_preference(wgpu::PowerPreference::HighPerformance)
    }

    /// Probes the system for a `wgpu` adapter with an explicit [`wgpu::PowerPreference`].
    pub fn try_new_with_power_preference(
        power_preference: wgpu::PowerPreference,
    ) -> Option<Arc<Self>> {
        let _guard = GPU_INIT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        Self::probe_headless_backends(wgpu::Backends::PRIMARY, power_preference)
            .or_else(|| Self::probe_headless_backends(wgpu::Backends::GL, power_preference))
    }

    fn probe_headless_backends(
        backends: wgpu::Backends,
        power_preference: wgpu::PowerPreference,
    ) -> Option<Arc<Self>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))?;
        let (device, queue) = request_adapter_device(&adapter)?;
        Some(Self::build(Some(instance), Some(adapter), device, queue))
    }

    /// Creates a `GpuContext` + `wgpu::Surface` guaranteed compatible with `window_target`.
    pub(crate) fn try_new_for_surface(
        window_target: impl Into<wgpu::SurfaceTarget<'static>>,
        power_preference: wgpu::PowerPreference,
    ) -> Option<(Arc<Self>, wgpu::Surface<'static>)> {
        let _guard = GPU_INIT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY | wgpu::Backends::GL,
            ..Default::default()
        });
        let surface = instance.create_surface(window_target).ok()?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))?;
        let (device, queue) = request_adapter_device(&adapter)?;
        let ctx = Self::build(Some(instance), Some(adapter), device, queue);
        Some((ctx, surface))
    }

    /// Wraps an external host's `wgpu::Device` and `wgpu::Queue` in a shared `Arc<GpuContext>`.
    pub fn from_device(device: wgpu::Device, queue: wgpu::Queue) -> Arc<Self> {
        Self::build(None, None, device, queue)
    }

    fn build(
        instance: Option<wgpu::Instance>,
        adapter: Option<wgpu::Adapter>,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Arc<Self> {
        let atlas_bgl = create_atlas_bind_group_layout(&device);
        let aux_bgl = create_aux_bind_group_layout(&device);
        let (mask_tex, color_tex, sampler, atlas_bg) =
            create_dual_atlas_textures(&device, &atlas_bgl);
        let mut packer = DualGlyphAtlas::new();
        packer.upload_dirty_pages(&queue, &mask_tex, &color_tex);
        let (dummy_tex, dummy_bg) = create_dummy_aux_bind_group(&device, &aux_bgl);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("KRui Phase4 SDF Uber-Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("KRui SDF PipelineLayout"),
            bind_group_layouts: &[&atlas_bgl, &aux_bgl],
            push_constant_ranges: &[],
        });
        let blur = BlurPipelineShared::new(&device);
        let mut map = HashMap::with_capacity(2);
        let rgba_pipes = Arc::new(compile_format_pipelines(
            &device,
            &shader,
            &pipeline_layout,
            wgpu::TextureFormat::Rgba8Unorm,
        ));
        map.insert(wgpu::TextureFormat::Rgba8Unorm, rgba_pipes);

        Arc::new(Self {
            instance,
            adapter,
            device,
            queue,
            shader,
            pipeline_layout,
            atlas_bind_group: atlas_bg,
            aux_bgl,
            _dummy_aux_tex: dummy_tex,
            dummy_aux_bg: dummy_bg,
            blur,
            atlas: Mutex::new(SharedGpuAtlas {
                packer,
                mask_tex,
                color_tex,
                _sampler: sampler,
            }),
            image_cache: Mutex::new(ImageGpuCache::default()),
            external_textures: Mutex::new(ExternalTextureRegistry::default()),
            format_pipelines: Mutex::new(map),
        })
    }

    /// Returns the `wgpu::Instance` if owned by this context.
    pub fn instance(&self) -> Option<&wgpu::Instance> {
        self.instance.as_ref()
    }

    /// Returns the `wgpu::Adapter` if owned by this context.
    pub fn adapter(&self) -> Option<&wgpu::Adapter> {
        self.adapter.as_ref()
    }

    /// Returns a reference to the shared `wgpu::Device`.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Returns a reference to the shared `wgpu::Queue`.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Returns `(cached_glyph_count, eviction_count)` from the shared `DualGlyphAtlas`.
    pub fn atlas_stats(&self) -> (usize, u64) {
        let guard = self.atlas.lock().expect("atlas mutex");
        (
            guard.packer.cached_glyph_count(),
            guard.packer.eviction_count(),
        )
    }

    pub(crate) fn pipelines_for(&self, format: wgpu::TextureFormat) -> Arc<FormatPipelines> {
        let mut guard = self.format_pipelines.lock().expect("pipelines mutex");
        if let Some(p) = guard.get(&format) {
            return Arc::clone(p);
        }
        let compiled = Arc::new(compile_format_pipelines(
            &self.device,
            &self.shader,
            &self.pipeline_layout,
            format,
        ));
        guard.insert(format, Arc::clone(&compiled));
        compiled
    }

    /// Registers an external host `wgpu::TextureView` for [`ExternalTextureId`] slots (`<img>`/`ExternalTexture`).
    pub fn register_external_texture(&self, id: ExternalTextureId, view: &wgpu::TextureView) {
        self.external_textures
            .lock()
            .expect("external textures mutex")
            .register(&self.device, &self.aux_bgl, id, view);
    }

    /// Removes a previously registered [`ExternalTextureId`] binding.
    pub fn unregister_external_texture(&self, id: ExternalTextureId) {
        self.external_textures
            .lock()
            .expect("external textures mutex")
            .unregister(id);
    }

    pub(crate) fn resolve_image_aux_bg(&self, img: &DecodedImage) -> Arc<wgpu::BindGroup> {
        self.image_cache
            .lock()
            .expect("image cache mutex")
            .get_or_upload(&self.device, &self.queue, &self.aux_bgl, img)
    }

    pub(crate) fn resolve_external_aux_bg(
        &self,
        id: ExternalTextureId,
    ) -> Option<Arc<wgpu::BindGroup>> {
        self.external_textures
            .lock()
            .expect("external textures mutex")
            .get(id)
    }
}

fn request_adapter_device(adapter: &wgpu::Adapter) -> Option<(wgpu::Device, wgpu::Queue)> {
    let required_limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
    pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("KRui Shared wgpu Device"),
            required_features: wgpu::Features::empty(),
            required_limits,
            memory_hints: wgpu::MemoryHints::Performance,
        },
        None,
    ))
    .ok()
}
