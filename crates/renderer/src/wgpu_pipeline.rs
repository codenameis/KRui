//! `wgpu` bind group layouts, dual-array glyph atlas GPU textures, and compiled SDF + Canvas pipelines.

use crate::dual_atlas::{COLOR_ATLAS_LAYERS, DUAL_ATLAS_SIZE, MASK_ATLAS_LAYERS};
use crate::sdf_instance::{SdfQuadInstance, SDF_INSTANCE_ATTRIBUTES};
use crate::tessellate::GpuVertex;

const CANVAS_VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 3] = [
    wgpu::VertexAttribute {
        offset: 0,
        shader_location: 0,
        format: wgpu::VertexFormat::Float32x2,
    },
    wgpu::VertexAttribute {
        offset: 8,
        shader_location: 1,
        format: wgpu::VertexFormat::Float32x2,
    },
    wgpu::VertexAttribute {
        offset: 16,
        shader_location: 2,
        format: wgpu::VertexFormat::Float32x4,
    },
];

/// Compiled SDF quad pipeline and `<canvas>` triangle pipeline for a specific target `TextureFormat`.
pub(crate) struct FormatPipelines {
    pub sdf_pipeline: wgpu::RenderPipeline,
    pub canvas_pipeline: wgpu::RenderPipeline,
}

pub(crate) fn create_atlas_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("KRui DualAtlas BindGroupLayout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

pub(crate) fn create_aux_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("KRui Aux Texture BindGroupLayout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                multisampled: false,
                view_dimension: wgpu::TextureViewDimension::D2,
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
            },
            count: None,
        }],
    })
}

pub(crate) fn create_dual_atlas_textures(
    device: &wgpu::Device,
    atlas_bgl: &wgpu::BindGroupLayout,
) -> (wgpu::Texture, wgpu::Texture, wgpu::Sampler, wgpu::BindGroup) {
    let mask_tex = create_array_texture(
        device,
        "KRui R8 Mask Atlas Array",
        MASK_ATLAS_LAYERS,
        wgpu::TextureFormat::R8Unorm,
    );
    let color_tex = create_array_texture(
        device,
        "KRui RGBA8 Color Atlas Array",
        COLOR_ATLAS_LAYERS,
        wgpu::TextureFormat::Rgba8Unorm,
    );
    let mask_view = mask_tex.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let color_view = color_tex.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("KRui Trilinear Clamp Sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("KRui DualAtlas BindGroup"),
        layout: atlas_bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&mask_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&color_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    (mask_tex, color_tex, sampler, bg)
}

fn create_array_texture(
    device: &wgpu::Device,
    label: &str,
    layers: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: DUAL_ATLAS_SIZE,
            height: DUAL_ATLAS_SIZE,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

pub(crate) fn create_dummy_aux_bind_group(
    device: &wgpu::Device,
    aux_bgl: &wgpu::BindGroupLayout,
) -> (wgpu::Texture, wgpu::BindGroup) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("KRui Dummy 1x1 Aux Texture"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("KRui Dummy Aux BindGroup"),
        layout: aux_bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
    });
    (tex, bg)
}

pub(crate) fn compile_format_pipelines(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
) -> FormatPipelines {
    let sdf_buf_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<SdfQuadInstance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &SDF_INSTANCE_ATTRIBUTES,
    };
    let canvas_buf_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<GpuVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &CANVAS_VERTEX_ATTRIBUTES,
    };
    let (sdf_fs, canvas_fs) = if target_format.is_srgb() {
        ("fs_main_srgb", "fs_canvas_srgb")
    } else {
        ("fs_main", "fs_canvas")
    };
    FormatPipelines {
        sdf_pipeline: create_premul_pipeline(
            device,
            shader,
            layout,
            target_format,
            "vs_main",
            sdf_fs,
            sdf_buf_layout,
        ),
        canvas_pipeline: create_premul_pipeline(
            device,
            shader,
            layout,
            target_format,
            "vs_canvas",
            canvas_fs,
            canvas_buf_layout,
        ),
    }
}

fn create_premul_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    vs_entry: &str,
    fs_entry: &str,
    buf_layout: wgpu::VertexBufferLayout<'_>,
) -> wgpu::RenderPipeline {
    let color_targets = [Some(wgpu::ColorTargetState {
        format: target_format,
        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(vs_entry),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vs_entry),
            compilation_options: Default::default(),
            buffers: &[buf_layout],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs_entry),
            compilation_options: Default::default(),
            targets: &color_targets,
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

pub(crate) fn create_sampleable_target(
    device: &wgpu::Device,
    aux_bgl: &wgpu::BindGroupLayout,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    extra_usage: wgpu::TextureUsages,
) -> (wgpu::Texture, wgpu::TextureView, wgpu::BindGroup) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("KRui Sampleable Color Target"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | extra_usage,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let aux_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("KRui Target Aux BindGroup"),
        layout: aux_bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
    });
    (tex, view, aux_bg)
}

pub(crate) fn create_sdf_and_canvas_buffers(
    device: &wgpu::Device,
    sdf_cap: usize,
    v_cap: usize,
    i_cap: usize,
) -> (wgpu::Buffer, wgpu::Buffer, wgpu::Buffer) {
    let sdf_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("KRui SDF Instance Buffer"),
        size: (sdf_cap.max(64) * std::mem::size_of::<SdfQuadInstance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("KRui Canvas Vertex Buffer"),
        size: (v_cap.max(64) * std::mem::size_of::<GpuVertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let ibuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("KRui Canvas Index Buffer"),
        size: (i_cap.max(64) * std::mem::size_of::<u32>()) as u64,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    (sdf_buf, vbuf, ibuf)
}
