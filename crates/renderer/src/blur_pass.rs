//! Dual-Kawase multi-pass GPU backdrop blur pipeline and pooled offscreen `PushLayer` targets.

use crate::wgpu_pipeline::create_sampleable_target;
use std::num::NonZeroU64;

/// Device-shared Dual-Kawase downsample + upsample pipelines (`blur.wgsl`).
pub(crate) struct BlurPipelineShared {
    pub bgl: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    pub downsample_pipeline: wgpu::RenderPipeline,
    pub upsample_pipeline: wgpu::RenderPipeline,
    pub uniform_align: u32,
}

impl BlurPipelineShared {
    pub fn new(device: &wgpu::Device) -> Self {
        let uniform_align = device.limits().min_uniform_buffer_offset_alignment.max(256);
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("KRui DualKawase BindGroupLayout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(16),
                    },
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("KRui DualKawase Linear Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("KRui DualKawase Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blur.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("KRui DualKawase PipelineLayout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let downsample_pipeline = compile_blur_pass(device, &shader, &layout, "fs_downsample");
        let upsample_pipeline = compile_blur_pass(device, &shader, &layout, "fs_upsample");
        Self {
            bgl,
            sampler,
            downsample_pipeline,
            upsample_pipeline,
            uniform_align,
        }
    }

    pub fn create_source_bind_group(
        &self,
        device: &wgpu::Device,
        src_view: &wgpu::TextureView,
        uniform_buf: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("KRui DualKawase Source BindGroup"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(src_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: uniform_buf,
                        offset: 0,
                        size: NonZeroU64::new(16),
                    }),
                },
            ],
        })
    }
}

fn compile_blur_pass(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    fs_entry: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fs_entry),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_blur"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Single blur render pass step inside a Dual-Kawase pyramid execution.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BlurStep {
    pub is_upsample: bool,
    pub src: BlurSourceKind,
    pub dst: BlurTargetKind,
    pub dynamic_offset: u32,
    pub uv_rect: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BlurSourceKind {
    ActiveTarget(usize),
    HalfA,
    HalfB,
    Quarter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BlurTargetKind {
    HalfA,
    HalfB,
    Quarter,
}

pub(crate) struct AllocatedBlurChain {
    pub full_size: (u32, u32),
    pub _half_a_tex: wgpu::Texture,
    pub half_a_view: wgpu::TextureView,
    pub half_a_src_bg: wgpu::BindGroup,
    pub _half_b_tex: wgpu::Texture,
    pub half_b_view: wgpu::TextureView,
    pub half_b_src_bg: wgpu::BindGroup,
    pub half_b_aux_bg: wgpu::BindGroup,
    pub _quarter_tex: wgpu::Texture,
    pub quarter_view: wgpu::TextureView,
    pub quarter_src_bg: wgpu::BindGroup,
    pub uniform_buf: wgpu::Buffer,
    pub uniform_slot_cap: usize,
    pub uniform_buf_epoch: u64,
}

/// Lazily allocated Dual-Kawase ping-pong render targets (`0` bytes allocated on non-blur themes).
#[derive(Default)]
pub(crate) struct BlurChainTargets {
    pub chain: Option<AllocatedBlurChain>,
    epoch_counter: u64,
}

impl BlurChainTargets {
    pub fn ensure(
        &mut self,
        device: &wgpu::Device,
        shared: &BlurPipelineShared,
        aux_bgl: &wgpu::BindGroupLayout,
        (vw, vh): (u32, u32),
        required_slots: usize,
    ) -> &AllocatedBlurChain {
        let slots = required_slots.max(16).next_power_of_two();
        let needs_rebuild = match &self.chain {
            Some(c) => c.full_size != (vw, vh) || c.uniform_slot_cap < slots,
            None => true,
        };
        if needs_rebuild {
            self.epoch_counter = self.epoch_counter.wrapping_add(1);
            self.chain = Some(allocate_blur_chain(
                device,
                shared,
                aux_bgl,
                (vw, vh),
                slots,
                self.epoch_counter,
            ));
        }
        self.chain.as_ref().expect("blur chain initialized")
    }
}

fn allocate_blur_chain(
    device: &wgpu::Device,
    shared: &BlurPipelineShared,
    aux_bgl: &wgpu::BindGroupLayout,
    (vw, vh): (u32, u32),
    slots: usize,
    epoch: u64,
) -> AllocatedBlurChain {
    let half = ((vw / 2).max(1), (vh / 2).max(1));
    let quarter = ((vw / 4).max(1), (vh / 4).max(1));
    let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("KRui DualKawase Dynamic Uniforms"),
        size: (slots as u64) * (shared.uniform_align as u64),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mk = |w, h| {
        create_sampleable_target(
            device,
            aux_bgl,
            w,
            h,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::empty(),
        )
    };
    let (ha_t, ha_v, _) = mk(half.0, half.1);
    let (hb_t, hb_v, hb_aux) = mk(half.0, half.1);
    let (q_t, q_v, _) = mk(quarter.0, quarter.1);
    let ha_bg = shared.create_source_bind_group(device, &ha_v, &uniform_buf);
    let hb_bg = shared.create_source_bind_group(device, &hb_v, &uniform_buf);
    let q_bg = shared.create_source_bind_group(device, &q_v, &uniform_buf);
    AllocatedBlurChain {
        full_size: (vw, vh),
        _half_a_tex: ha_t,
        half_a_view: ha_v,
        half_a_src_bg: ha_bg,
        _half_b_tex: hb_t,
        half_b_view: hb_v,
        half_b_src_bg: hb_bg,
        half_b_aux_bg: hb_aux,
        _quarter_tex: q_t,
        quarter_view: q_v,
        quarter_src_bg: q_bg,
        uniform_buf,
        uniform_slot_cap: slots,
        uniform_buf_epoch: epoch,
    }
}

struct KawasePlanner<'a> {
    steps: &'a mut Vec<BlurStep>,
    buf: &'a mut Vec<u8>,
    align: u32,
    uv_rect: [f32; 4],
}

impl KawasePlanner<'_> {
    fn down(&mut self, src: BlurSourceKind, dst: BlurTargetKind, texel: (f32, f32), scale: f32) {
        self.push(false, src, dst, texel, scale);
    }

    fn up(&mut self, src: BlurSourceKind, dst: BlurTargetKind, texel: (f32, f32), scale: f32) {
        self.push(true, src, dst, texel, scale);
    }

    fn push(
        &mut self,
        is_upsample: bool,
        src: BlurSourceKind,
        dst: BlurTargetKind,
        texel: (f32, f32),
        scale: f32,
    ) {
        let dynamic_offset = push_blur_uniform(self.buf, self.align, texel.0, texel.1, scale);
        self.steps.push(BlurStep {
            is_upsample,
            src,
            dst,
            dynamic_offset,
            uv_rect: self.uv_rect,
        });
    }
}

/// Appends the Dual-Kawase downsample + upsample steps for `blur_phys_px` into `steps` and `uniform_bytes`.
pub(crate) fn plan_dual_kawase_steps(
    src_target_id: usize,
    (vw, vh): (u32, u32),
    box_bounds: [f32; 4],
    blur_phys_px: f32,
    uniform_align: u32,
    uniform_bytes: &mut Vec<u8>,
    steps: &mut Vec<BlurStep>,
) {
    use BlurSourceKind as S;
    use BlurTargetKind as T;
    let full_tex = (1.0 / vw as f32, 1.0 / vh as f32);
    let half_tex = (1.0 / (vw / 2).max(1) as f32, 1.0 / (vh / 2).max(1) as f32);
    let quarter_tex = (1.0 / (vw / 4).max(1) as f32, 1.0 / (vh / 4).max(1) as f32);
    let r = blur_phys_px.clamp(0.5, 64.0);
    let pad = (r * 3.0).max(16.0);
    let uv_rect = [
        ((box_bounds[0] - pad) * full_tex.0).clamp(0.0, 1.0),
        ((box_bounds[1] - pad) * full_tex.1).clamp(0.0, 1.0),
        ((box_bounds[2] + pad) * full_tex.0).clamp(0.0, 1.0),
        ((box_bounds[3] + pad) * full_tex.1).clamp(0.0, 1.0),
    ];
    let s1 = (r / 8.0).clamp(0.65, 2.5);
    let mut p = KawasePlanner {
        steps,
        buf: uniform_bytes,
        align: uniform_align,
        uv_rect,
    };
    p.down(S::ActiveTarget(src_target_id), T::HalfA, full_tex, s1);
    if r <= 5.0 {
        p.up(S::HalfA, T::HalfB, half_tex, s1);
        return;
    }
    p.down(S::HalfA, T::Quarter, half_tex, s1);
    if r > 18.0 {
        let s2 = ((r - 12.0) / 10.0).clamp(1.0, 2.8);
        p.up(S::Quarter, T::HalfB, quarter_tex, s2);
        p.down(S::HalfB, T::Quarter, half_tex, s2);
    }
    p.up(S::Quarter, T::HalfB, quarter_tex, s1);
}

fn push_blur_uniform(buf: &mut Vec<u8>, align: u32, tx: f32, ty: f32, scale: f32) -> u32 {
    let offset = buf.len() as u32;
    let stride = align.max(16) as usize;
    buf.resize(buf.len() + stride, 0);
    let slot = &mut buf[offset as usize..offset as usize + 16];
    slot[0..4].copy_from_slice(&tx.to_ne_bytes());
    slot[4..8].copy_from_slice(&ty.to_ne_bytes());
    slot[8..12].copy_from_slice(&scale.to_ne_bytes());
    slot[12..16].copy_from_slice(&0.0_f32.to_ne_bytes());
    offset
}

/// Pooled offscreen color attachment used for `PushLayer`/`PopLayer` group opacity and swapchain blur.
pub(crate) struct OffscreenLayerTarget {
    pub size: (u32, u32),
    pub format: wgpu::TextureFormat,
    pub _tex: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub aux_bg: wgpu::BindGroup,
    pub blur_src_bg: Option<(u64, wgpu::BindGroup)>,
}

/// Pool of reusable offscreen layer textures for nested `PushLayer`/`PopLayer` scopes.
#[derive(Default)]
pub(crate) struct LayerTargetPool {
    pub layers: Vec<OffscreenLayerTarget>,
}

impl LayerTargetPool {
    pub fn ensure_count(
        &mut self,
        device: &wgpu::Device,
        aux_bgl: &wgpu::BindGroupLayout,
        count: usize,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) {
        for slot in self.layers.iter_mut().take(count) {
            if slot.size != size || slot.format != format {
                let (tex, view, aux_bg) = create_sampleable_target(
                    device,
                    aux_bgl,
                    size.0,
                    size.1,
                    format,
                    wgpu::TextureUsages::empty(),
                );
                *slot = OffscreenLayerTarget {
                    size,
                    format,
                    _tex: tex,
                    view,
                    aux_bg,
                    blur_src_bg: None,
                };
            }
        }
        while self.layers.len() < count {
            let (tex, view, aux_bg) = create_sampleable_target(
                device,
                aux_bgl,
                size.0,
                size.1,
                format,
                wgpu::TextureUsages::empty(),
            );
            self.layers.push(OffscreenLayerTarget {
                size,
                format,
                _tex: tex,
                view,
                aux_bg,
                blur_src_bg: None,
            });
        }
    }

    pub fn trim_idle(&mut self, max_idle_count: usize) {
        if self.layers.len() > max_idle_count {
            self.layers.truncate(max_idle_count);
        }
    }
}
