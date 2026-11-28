//! WebGPU / WebGL2 pipeline, surface configuration, and matrix helpers for `web_viewport3d`.

use super::mesh_obj::{pack_mesh_vertices_le, LoadedMesh};
use wasm_bindgen::closure::Closure;

const MODEL_3D_WGSL: &str = r#"
struct Uniforms {
    mvp: mat4x4<f32>,
    model: mat4x4<f32>,
    base_color: vec4<f32>,
    cam_pos: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) world_pos: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>) -> VsOut {
    var out: VsOut;
    let wp = u.model * vec4<f32>(pos, 1.0);
    out.clip_pos = u.mvp * vec4<f32>(pos, 1.0);
    out.world_pos = wp.xyz;
    out.world_normal = normalize((u.model * vec4<f32>(normal, 0.0)).xyz);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let light_dir = normalize(vec3<f32>(0.55, 0.82, 0.64));
    let rim_dir = normalize(vec3<f32>(-0.65, 0.35, -0.50));
    let view_dir = normalize(u.cam_pos.xyz - in.world_pos);
    let diff = max(dot(n, light_dir), 0.0);
    let rim = max(dot(n, rim_dir), 0.0) * 0.25;
    let half_vec = normalize(light_dir + view_dir);
    let spec = pow(max(dot(n, half_vec), 0.0), 42.0) * 0.42;
    let lit = u.base_color.rgb * (0.24 + diff * 0.78 + rim) + vec3<f32>(spec);
    return vec4<f32>(lit, 1.0);
}
"#;

pub(super) struct WebGpu3dState {
    pub(super) canvas: web_sys::HtmlCanvasElement,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pub(super) observer: Option<web_sys::ResizeObserver>,
    pub(super) _observer_cb: Option<Closure<dyn FnMut(js_sys::Array)>>,
}

impl Drop for WebGpu3dState {
    fn drop(&mut self) {
        if let Some(obs) = self.observer.take() {
            obs.disconnect();
        }
        if let Some(parent) = self.canvas.parent_node() {
            let _ = parent.remove_child(&self.canvas);
        }
    }
}

pub(super) async fn init_wgpu_state(
    canvas: web_sys::HtmlCanvasElement,
    mesh: &LoadedMesh,
) -> Option<(WebGpu3dState, &'static str)> {
    let (pw, ph) = canvas_physical_size(&canvas);
    canvas.set_width(pw);
    canvas.set_height(ph);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
        ..Default::default()
    });
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .ok()?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await?;
    let backend_label = backend_label_for(&adapter);
    let (device, queue) = request_web_device(&adapter).await?;
    let config = configure_surface(&surface, &adapter, &device, pw, ph)?;
    let depth_view = create_depth_view(&device, config.width, config.height);
    let state = build_pipeline_state(canvas, surface, device, queue, config, depth_view, mesh);
    Some((state, backend_label))
}

fn canvas_physical_size(canvas: &web_sys::HtmlCanvasElement) -> (u32, u32) {
    let rect = canvas.get_bounding_client_rect();
    let dpr = web_sys::window()
        .map(|w| w.device_pixel_ratio() as f32)
        .unwrap_or(1.0)
        .max(1.0);
    let pw = ((rect.width() as f32).max(64.0) * dpr).round() as u32;
    let ph = ((rect.height() as f32).max(64.0) * dpr).round() as u32;
    (pw, ph)
}

fn backend_label_for(adapter: &wgpu::Adapter) -> &'static str {
    match adapter.get_info().backend {
        wgpu::Backend::BrowserWebGpu => "WebGPU Native",
        wgpu::Backend::Gl => "WebGL2 Fallback",
        _ => "wgpu Web",
    }
}

async fn request_web_device(adapter: &wgpu::Adapter) -> Option<(wgpu::Device, wgpu::Queue)> {
    adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: Some("KRui Web 3D Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        )
        .await
        .ok()
}

fn configure_surface(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    pw: u32,
    ph: u32,
) -> Option<wgpu::SurfaceConfiguration> {
    let caps = surface.get_capabilities(adapter);
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .or_else(|| caps.formats.first().copied())?;
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: pw.max(1),
        height: ph.max(1),
        present_mode: wgpu::PresentMode::AutoVsync,
        alpha_mode: caps
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Auto),
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(device, &config);
    Some(config)
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("KRui Web 3D Depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24Plus,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

fn create_mesh_and_uniform_buffers(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mesh: &LoadedMesh,
) -> (wgpu::Buffer, wgpu::Buffer, wgpu::Buffer) {
    let vbytes = pack_mesh_vertices_le(&mesh.vertices);
    let mut ibytes = Vec::with_capacity(mesh.indices.len() * 4);
    for idx in &mesh.indices {
        ibytes.extend_from_slice(&idx.to_le_bytes());
    }
    let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Web Teapot Verts"),
        size: vbytes.len().max(24) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let index_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Web Teapot Indices"),
        size: ibytes.len().max(12) as u64,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&vertex_buf, 0, &vbytes);
    queue.write_buffer(&index_buf, 0, &ibytes);
    let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Web Teapot Uniforms"),
        size: 160,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    (vertex_buf, index_buf, uniform_buf)
}

fn create_render_pipeline(
    device: &wgpu::Device,
    bg_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Web 3D Shader"),
        source: wgpu::ShaderSource::Wgsl(MODEL_3D_WGSL.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Web 3D Pipeline Layout"),
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
        label: Some("Web 3D Teapot Pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 24,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attrs,
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
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
            format: wgpu::TextureFormat::Depth24Plus,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn build_pipeline_state(
    canvas: web_sys::HtmlCanvasElement,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    mesh: &LoadedMesh,
) -> WebGpu3dState {
    let (vertex_buf, index_buf, uniform_buf) =
        create_mesh_and_uniform_buffers(&device, &queue, mesh);
    let bg_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Web 3D Layout"),
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
        label: Some("Web 3D BindGroup"),
        layout: &bg_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buf.as_entire_binding(),
        }],
    });
    let pipeline = create_render_pipeline(&device, &bg_layout, config.format);
    WebGpu3dState {
        canvas,
        surface,
        device,
        queue,
        config,
        depth_view,
        pipeline,
        vertex_buf,
        index_buf,
        index_count: mesh.indices.len() as u32,
        uniform_buf,
        bind_group,
        observer: None,
        _observer_cb: None,
    }
}

impl WebGpu3dState {
    pub(super) fn resize_to_canvas(&mut self) {
        let (pw, ph) = canvas_physical_size(&self.canvas);
        if pw == self.config.width && ph == self.config.height {
            return;
        }
        self.canvas.set_width(pw);
        self.canvas.set_height(ph);
        self.config.width = pw.max(1);
        self.config.height = ph.max(1);
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, self.config.width, self.config.height);
    }

    pub(super) fn render(&self, yaw_deg: f32, pitch_deg: f32, distance: f32, model_scale: f32) {
        self.upload_uniforms(yaw_deg, pitch_deg, distance, model_scale);
        let Ok(frame) = self.surface.get_current_texture() else {
            return;
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.encode_render_pass(&mut encoder, &view);
        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }

    fn upload_uniforms(&self, yaw_deg: f32, pitch_deg: f32, distance: f32, model_scale: f32) {
        let aspect = self.config.width as f32 / self.config.height.max(1) as f32;
        let dist = distance.clamp(1.6, 8.0);
        let cam_pos = [0.0, 0.45, dist, 1.0];
        let view = mat4_look_at_rh([0.0, 0.45, dist], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let proj = mat4_perspective_rh(42.0_f32.to_radians(), aspect, 0.1, 40.0);
        let model = mat4_mul(
            mat4_rot_x(pitch_deg.to_radians()),
            mat4_mul(
                mat4_rot_y(yaw_deg.to_radians()),
                mat4_scale(model_scale.clamp(0.4, 2.2)),
            ),
        );
        let mvp = mat4_mul(proj, mat4_mul(view, model));
        let color = [0.40_f32, 0.56, 0.92, 1.0];
        let mut ubytes = Vec::with_capacity(160);
        for slice in [&mvp[..], &model[..], &color[..], &cam_pos[..]] {
            for f in slice {
                ubytes.extend_from_slice(&f.to_le_bytes());
            }
        }
        self.queue.write_buffer(&self.uniform_buf, 0, &ubytes);
    }

    fn encode_render_pass(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("KRui Web 3D Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.06,
                        g: 0.07,
                        b: 0.11,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buf.slice(..));
        pass.set_index_buffer(self.index_buf.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }
}

fn mat4_mul(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut out = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            out[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    out
}

fn mat4_scale(s: f32) -> [f32; 16] {
    [
        s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn mat4_rot_x(r: f32) -> [f32; 16] {
    let (s, c) = r.sin_cos();
    [
        1.0, 0.0, 0.0, 0.0, 0.0, c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn mat4_rot_y(r: f32) -> [f32; 16] {
    let (s, c) = r.sin_cos();
    [
        c, 0.0, -s, 0.0, 0.0, 1.0, 0.0, 0.0, s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn mat4_perspective_rh(fov_y_rad: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1.0 / (fov_y_rad * 0.5).tan();
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
        far / (near - far),
        -1.0,
        0.0,
        0.0,
        (far * near) / (near - far),
        0.0,
    ]
}

fn mat4_look_at_rh(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let norm = |v: [f32; 3]| {
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
        [v[0] / len, v[1] / len, v[2] / len]
    };
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let f = norm(sub(target, eye));
    let s = norm(cross(f, up));
    let u = cross(s, f);
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
