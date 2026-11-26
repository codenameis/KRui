//! Shared `wgpu` 3D pipeline rendering `open_teapot.obj` into a [`RawWgpuViewport`]
//! (used by both Raw `wgpu` direct mode and Bevy ECS-driven mode).

use super::mesh_obj::{pack_mesh_vertices_le, LoadedMesh, MeshVertex};
use krui_engine_host::RawWgpuViewport;

const MODEL_3D_WGSL: &str = r#"
struct Uniforms {
    mvp: mat4x4<f32>,
    model: mat4x4<f32>,
    base_color: vec4<f32>,
    cam_pos: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    let wp = u.model * vec4<f32>(in.pos, 1.0);
    out.clip_pos = u.mvp * vec4<f32>(in.pos, 1.0);
    out.world_pos = wp.xyz;
    out.world_normal = normalize((u.model * vec4<f32>(in.normal, 0.0)).xyz);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let v = normalize(u.cam_pos.xyz - in.world_pos);
    let key_dir = normalize(vec3<f32>(0.55, 0.85, 0.65));
    let fill_dir = normalize(vec3<f32>(-0.75, 0.25, -0.50));
    let key = max(dot(n, key_dir), 0.0);
    let fill = max(dot(n, fill_dir), 0.0) * 0.35;
    let hemi = 0.22 + 0.16 * n.y;
    let half_vec = normalize(key_dir + v);
    let spec = pow(max(dot(n, half_vec), 0.0), 42.0) * 0.45;
    let rim = pow(1.0 - max(abs(dot(n, v)), 0.0), 3.0) * 0.28;
    let lit = u.base_color.rgb * (hemi + key * 0.92 + fill) + vec3<f32>(spec + rim);
    return vec4<f32>(clamp(lit, vec3<f32>(0.0), vec3<f32>(1.0)), u.base_color.a);
}
"#;

/// Per-frame camera + object transform parameters passed to [`SharedModelGpu::render_into`].
#[derive(Clone, Copy, Debug)]
pub struct RenderModelParams {
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub distance: f32,
    pub model_scale: f32,
    pub clear_rgba: [f64; 4],
    pub base_color: [f32; 4],
}

/// GPU buffers + pipeline holding the shared `open_teapot.obj` mesh on KRui's `GpuContext`.
pub struct SharedModelGpu {
    pipeline: wgpu::RenderPipeline,
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

fn mat4_mul(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut out = [0.0_f32; 16];
    for c in 0..4 {
        for r in 0..4 {
            out[c * 4 + r] = a[r] * b[c * 4]
                + a[4 + r] * b[c * 4 + 1]
                + a[8 + r] * b[c * 4 + 2]
                + a[12 + r] * b[c * 4 + 3];
        }
    }
    out
}

fn mat4_rot_y(rad: f32) -> [f32; 16] {
    let (s, c) = rad.sin_cos();
    [
        c, 0.0, -s, 0.0, 0.0, 1.0, 0.0, 0.0, s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn mat4_rot_x(rad: f32) -> [f32; 16] {
    let (s, c) = rad.sin_cos();
    [
        1.0, 0.0, 0.0, 0.0, 0.0, c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn mat4_scale(s: f32) -> [f32; 16] {
    [
        s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn mat4_perspective_rh(fov_y_rad: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1.0 / (0.5 * fov_y_rad).tan();
    let nf = 1.0 / (near - far);
    [
        f / aspect.max(0.1),
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        far * nf,
        -1.0,
        0.0,
        0.0,
        (far * near) * nf,
        0.0,
    ]
}

fn mat4_look_at_rh(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let norm = |v: [f32; 3]| -> [f32; 3] {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let f = norm(sub(target, eye));
    let s = norm(cross(f, up));
    let u = cross(s, f);
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    [
        s[0],
        u[0],
        -f[0],
        0.0,
        s[1],
        u[1],
        -f[1],
        0.0,
        s[2],
        u[2],
        -f[2],
        0.0,
        -dot(s, eye),
        -dot(u, eye),
        dot(f, eye),
        1.0,
    ]
}

fn create_model_pipeline(
    device: &wgpu::Device,
    bg_layout: &wgpu::BindGroupLayout,
    color_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Gallery 3D Model Shader"),
        source: wgpu::ShaderSource::Wgsl(MODEL_3D_WGSL.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Gallery 3D Pipeline Layout"),
        bind_group_layouts: &[bg_layout],
        push_constant_ranges: &[],
    });
    let attrs = [
        wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x3,
        },
        wgpu::VertexAttribute {
            offset: 12,
            shader_location: 1,
            format: wgpu::VertexFormat::Float32x3,
        },
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Gallery 3D Open Model Pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<MeshVertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attrs,
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: sample_count.max(1),
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview: None,
        cache: None,
    })
}

fn create_index_bytes(indices: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(indices.len() * 4);
    for idx in indices {
        out.extend_from_slice(&idx.to_le_bytes());
    }
    out
}

fn pack_uniforms(mvp: [f32; 16], model: [f32; 16], color: [f32; 4], cam: [f32; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity(160);
    for slice in [&mvp[..], &model[..], &color[..], &cam[..]] {
        for f in slice {
            out.extend_from_slice(&f.to_le_bytes());
        }
    }
    out
}

impl SharedModelGpu {
    /// Uploads `mesh` (`open_teapot.obj`) once and compiles the shared 3D render pipeline.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mesh: &LoadedMesh,
        color_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let vbytes = pack_mesh_vertices_le(&mesh.vertices);
        let ibytes = create_index_bytes(&mesh.indices);
        let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("OpenTeapot Verts"),
            size: vbytes.len().max(24) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("OpenTeapot Indices"),
            size: ibytes.len().max(12) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertex_buf, 0, &vbytes);
        queue.write_buffer(&index_buf, 0, &ibytes);
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("OpenTeapot Uniforms"),
            size: 160,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bg_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Gallery 3D Uniform Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Gallery 3D BindGroup"),
            layout: &bg_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });
        let pipeline = create_model_pipeline(device, &bg_layout, color_format, sample_count);
        Self {
            pipeline,
            vertex_buf,
            index_buf,
            index_count: mesh.indices.len() as u32,
            uniform_buf,
            bind_group,
        }
    }

    /// Renders `open_teapot.obj` into `viewport` using camera/model transform `params`.
    pub fn render_into(&self, viewport: &RawWgpuViewport, params: RenderModelParams) {
        let (w, h) = viewport.size();
        let aspect = (w as f32) / (h.max(1) as f32);
        let dist = params.distance.clamp(1.6, 8.0);
        let cam_pos = [0.0, 0.45, dist, 1.0];
        let view = mat4_look_at_rh(
            [cam_pos[0], cam_pos[1], cam_pos[2]],
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        );
        let proj = mat4_perspective_rh(42.0_f32.to_radians(), aspect, 0.1, 40.0);
        let model = mat4_mul(
            mat4_rot_x(params.pitch_deg.to_radians()),
            mat4_mul(
                mat4_rot_y(params.yaw_deg.to_radians()),
                mat4_scale(params.model_scale.clamp(0.4, 2.2)),
            ),
        );
        let mvp = mat4_mul(proj, mat4_mul(view, model));
        let ubytes = pack_uniforms(mvp, model, params.base_color, cam_pos);
        viewport.queue().write_buffer(&self.uniform_buf, 0, &ubytes);
        let clear = wgpu::Color {
            r: params.clear_rgba[0],
            g: params.clear_rgba[1],
            b: params.clear_rgba[2],
            a: params.clear_rgba[3],
        };
        viewport.render_pass("KRui 3D OpenTeapot Pass", clear, Some(1.0), |pass| {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buf.slice(..));
            pass.set_index_buffer(self.index_buf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.index_count, 0, 0..1);
        });
    }
}
