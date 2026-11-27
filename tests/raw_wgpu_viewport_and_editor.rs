//! Wave 3 verification tests: Raw `wgpu` original mode (`RawWgpuViewport` with MSAA + depth),
//! transparent `<canvas>` 3D gizmo overlay over `ExternalTextureId`, and the headless 3D editor widgets.

#![forbid(unsafe_code)]

use krui::{
    krui, CameraBasis3D, CanvasContext2D, CanvasDraw, Color, ColorPickerController,
    ContextMenuController, ContextMenuItem, DockLayoutController, DockPane, DrawPrimitive,
    DropPlacement, ExternalTextureId, GizmoAxis, GizmoController, GizmoMode, GizmoPalette,
    GpuContext, GpuResourceLimits, ImageSource, Mods, RendererBackend, Size, TreeNodeData,
    TreeViewController, UndoHistory, Vec3Controller, ViewTree, WgpuRenderer,
};
use krui_engine_host::{RawWgpuViewport, RawWgpuViewportConfig};

const DEPTH_TRIANGLE_WGSL: &str = r#"
struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
};
struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};
@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip_pos = vec4<f32>(in.pos, 1.0);
    out.color = in.color;
    return out;
}
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
struct TriVertex {
    pos: [f32; 3],
    color: [f32; 4],
}

fn vertices_to_bytes(verts: &[TriVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(verts));
    for v in verts {
        for f in v.pos {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        for f in v.color {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
    }
    bytes
}

fn build_depth_triangle_pipeline(
    device: &wgpu::Device,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Wave3 Depth Triangle Shader"),
        source: wgpu::ShaderSource::Wgsl(DEPTH_TRIANGLE_WGSL.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Wave3 Depth Pipeline Layout"),
        bind_group_layouts: &[],
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
            format: wgpu::VertexFormat::Float32x4,
        },
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Wave3 Depth Pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 28,
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
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: sample_count,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview: None,
        cache: None,
    })
}

fn draw_overlapping_depth_triangles(
    viewport: &RawWgpuViewport,
    pipeline: &wgpu::RenderPipeline,
    vbuf: &wgpu::Buffer,
) {
    viewport.render_pass(
        "RawWgpu Depth+MSAA Scene",
        wgpu::Color {
            r: 0.04,
            g: 0.06,
            b: 0.10,
            a: 1.0,
        },
        Some(1.0),
        |pass| {
            pass.set_pipeline(pipeline);
            pass.set_vertex_buffer(0, vbuf.slice(..));
            pass.draw(0..6, 0..1);
        },
    );
}

fn create_depth_test_vertex_buffer(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Buffer {
    let verts = [
        TriVertex {
            pos: [-0.9, -0.9, 0.2],
            color: [1.0, 0.0, 0.0, 1.0],
        },
        TriVertex {
            pos: [0.9, -0.9, 0.2],
            color: [1.0, 0.0, 0.0, 1.0],
        },
        TriVertex {
            pos: [0.0, 0.9, 0.2],
            color: [1.0, 0.0, 0.0, 1.0],
        },
        TriVertex {
            pos: [-0.9, -0.9, 0.8],
            color: [0.0, 0.0, 1.0, 1.0],
        },
        TriVertex {
            pos: [0.9, -0.9, 0.8],
            color: [0.0, 0.0, 1.0, 1.0],
        },
        TriVertex {
            pos: [0.0, 0.9, 0.8],
            color: [0.0, 0.0, 1.0, 1.0],
        },
    ];
    let raw_bytes = vertices_to_bytes(&verts);
    let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Depth Triangle Verts"),
        size: raw_bytes.len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&vbuf, 0, &raw_bytes);
    vbuf
}

fn build_viewport_gizmo_draw() -> CanvasDraw {
    let gizmo = GizmoController::new([0.0, 0.0, 0.0]);
    let camera = CameraBasis3D::orbit([0.0, 0.0, 0.0], 4.5, 0.65, 0.45, 0.9);
    let palette = GizmoPalette {
        x_axis: Color::rgb(255, 64, 64),
        y_axis: Color::rgb(64, 240, 96),
        z_axis: Color::rgb(64, 140, 255),
        active: Color::rgb(255, 220, 40),
        label: Color::rgb(255, 255, 255),
    };
    CanvasDraw::new(move |ctx: &mut CanvasContext2D, sz: Size| {
        gizmo.draw_to_canvas(ctx, sz, &camera, &palette);
    })
}

#[test]
fn raw_wgpu_viewport_msaa_depth_and_gizmo_canvas_overlay() {
    let Some(gpu) = GpuContext::try_new() else {
        return;
    };
    let mut raw_vp = RawWgpuViewport::new(
        gpu.clone(),
        ExternalTextureId(77),
        RawWgpuViewportConfig {
            width: 200,
            height: 160,
            color_format: wgpu::TextureFormat::Rgba8Unorm,
            sample_count: 4,
            depth_format: Some(wgpu::TextureFormat::Depth32Float),
        },
    );
    assert_eq!(raw_vp.sample_count(), 4);
    let pipeline = build_depth_triangle_pipeline(
        raw_vp.device(),
        raw_vp.color_format(),
        wgpu::TextureFormat::Depth32Float,
        4,
    );
    let vbuf = create_depth_test_vertex_buffer(raw_vp.device(), raw_vp.queue());
    let overlay_draw = build_viewport_gizmo_draw();
    let mut tree = ViewTree::new();
    krui!(
        r#"
        <style>
            .vp-stack { position: relative; width: 200px; height: 160px; }
            .vp-slot { position: absolute; left: 0px; top: 0px; width: 200px; height: 160px; }
            .vp-gizmo { position: absolute; left: 0px; top: 0px; width: 200px; height: 160px; }
        </style>
        <div class="vp-stack">
            <div id="vp" class="vp-slot" />
            <canvas id="gizmo" class="vp-gizmo" draw="{overlay_draw}" />
        </div>
        "#
    )
    .mount(&mut tree, None);
    let vp_node = tree.find_by_id("vp").expect("vp node");
    let _ = tree.build_paint_scene(200, 160);
    let _ = raw_vp.sync_with_node(&mut tree, vp_node, 1.0);
    draw_overlapping_depth_triangles(&raw_vp, &pipeline, &vbuf);

    let (scene, _) = tree.build_paint_scene(200, 160);
    let mut renderer = WgpuRenderer::from_context(
        gpu,
        wgpu::TextureFormat::Rgba8Unorm,
        200,
        160,
        GpuResourceLimits::default(),
    );
    let _ = renderer.render_scene(&scene);
    let fb = renderer.readback_framebuffer().expect("wgpu readback");
    let idx = (120 * 200 + 100) * 4;
    assert!(
        fb[idx] > 180 && fb[idx + 2] < 40,
        "expected near red pixel, got {:?}",
        &fb[idx..idx + 4]
    );
    assert!(raw_vp.resize(320, 240));
    assert_eq!(raw_vp.size(), (320, 240));
}

#[test]
fn editor_dock_layout_and_virtual_tree_reparent() {
    let dock = DockLayoutController::new();
    dock.left_split.begin_drag(200.0);
    dock.left_split.drag_to(300.0, 1000.0);
    dock.left_split.end_drag();
    let (_, _, vp_w, vp_h) = dock.viewport_rect(1000.0, 800.0, 4.0);
    assert!(vp_w > 400.0 && vp_h > 500.0);
    dock.focus_viewport_only(true);
    assert_eq!(
        dock.viewport_rect(1000.0, 800.0, 4.0),
        (0.0, 0.0, 1000.0, 800.0)
    );
    dock.toggle_pane(DockPane::Inspector);
    assert!(dock.show_inspector.get_untracked());

    let tree = TreeViewController::new();
    let mut nodes = vec![
        TreeNodeData {
            id: 1,
            parent: None,
            label: "RootA".into(),
            expanded: true,
        },
        TreeNodeData {
            id: 2,
            parent: Some(1),
            label: "ChildA".into(),
            expanded: true,
        },
    ];
    for id in 3..=2000_u64 {
        nodes.push(TreeNodeData {
            id,
            parent: Some(1),
            label: format!("Mesh_{id}"),
            expanded: false,
        });
    }
    tree.set_nodes(nodes);
    let (win, total_h) = tree.visible_window(100.0, 120.0, 20.0);
    assert_eq!(total_h, 2000.0 * 20.0);
    assert!(win.len() <= 8);

    tree.begin_drag(1);
    tree.set_drop_target(2, DropPlacement::Inside);
    assert!(!tree.commit_drop());
    tree.begin_drag(3);
    tree.set_drop_target(2, DropPlacement::Inside);
    assert!(tree.commit_drop());
    assert!(tree.children_of(Some(2)).contains(&3));
}

#[test]
fn editor_scrubber_color_picker_context_menu_undo_and_gizmo() {
    let vec3 = Vec3Controller::new([1.0, 2.0, 3.0], 0.5, 2);
    vec3.x.begin_scrub(100.0);
    vec3.x.scrub_to(
        110.0,
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    vec3.x.end_scrub();
    assert!((vec3.get()[0] - 1.5).abs() < 1e-4);
    assert!(vec3.y.parse_input("42.25"));
    assert!((vec3.y.value.get_untracked() - 42.25).abs() < 1e-4);

    let picker = ColorPickerController::from_rgba_u8([255, 0, 0, 255]);
    assert_eq!(picker.hex.get_untracked(), "#FF0000");
    assert!(picker.set_hex("#38BDF8"));
    assert_eq!(picker.rgba_u8()[2], 248);

    let menu = ContextMenuController::new();
    let item = ContextMenuItem {
        id: "dup".into(),
        label: "Duplicate".into(),
        shortcut: "Ctrl+D".into(),
        disabled: false,
    };
    menu.open_at(
        (980.0, 790.0),
        Size::new(1000.0, 800.0),
        Size::new(160.0, 120.0),
        vec![item],
    );
    assert_eq!(menu.x.get_untracked(), 840.0);
    assert_eq!(menu.select("dup").as_deref(), Some("dup"));
    assert!(!menu.open.get_untracked());

    let hist = UndoHistory::new(8);
    hist.push_coalesced("Drag X", 99, [0.0_f32, 0.0, 0.0], [1.0, 0.0, 0.0]);
    hist.push_coalesced("Drag X", 99, [1.0_f32, 0.0, 0.0], [3.5, 0.0, 0.0]);
    assert_eq!(hist.undo_len(), 1);
    assert_eq!(hist.undo(), Some([0.0, 0.0, 0.0]));
    assert_eq!(hist.redo(), Some([3.5, 0.0, 0.0]));

    let gizmo = GizmoController::new([0.0, 0.0, 0.0]);
    let cam = CameraBasis3D::orbit([0.0, 0.0, 0.0], 5.0, 0.4, 0.3, 0.9);
    let vp = Size::new(400.0, 300.0);
    let (x_end_x, x_end_y, _) = cam.project([0.8, 0.0, 0.0], vp).unwrap();
    assert_eq!(
        gizmo.begin_drag(x_end_x, x_end_y, &cam, vp),
        Some(GizmoAxis::X)
    );
    assert!(gizmo.drag_to(x_end_x + 50.0, x_end_y));
    assert!(gizmo.position.get_untracked()[0].abs() > 0.1);
    gizmo.mode.set(GizmoMode::Scale);
    gizmo.end_drag();
}

fn mount_hidpi_vp_tree() -> ViewTree {
    let mut tree = ViewTree::new();
    tree.set_scale_factor(1.5);
    krui!(
        r#"
        <style>
            .root { display: flex; flex-direction: column; width: 100%; height: 100%; }
            .bar { height: 48px; flex-shrink: 0; }
            .sub { height: 38px; flex-shrink: 0; }
            .main { flex-grow: 1; display: flex; flex-direction: row; width: 100%; min-height: 0; }
            .vp { flex-grow: 1; height: 100%; min-width: 0; min-height: 0; }
            .ins { width: 280px; height: 100%; flex-shrink: 0; }
        </style>
        <div class="root"><div class="bar"></div><div class="sub"></div><div class="main"><div id="vp" class="vp"></div><div class="ins"></div></div></div>
        "#
    )
    .mount(&mut tree, None);
    tree
}

#[test]
fn raw_wgpu_sync_with_node_hidpi_no_feedback_loop_and_zero_relayout() {
    let Some(gpu) = GpuContext::try_new() else {
        return;
    };
    let mut raw_vp = RawWgpuViewport::new(
        gpu,
        ExternalTextureId(88),
        RawWgpuViewportConfig {
            width: 320,
            height: 240,
            color_format: wgpu::TextureFormat::Rgba8Unorm,
            sample_count: 1,
            depth_format: None,
        },
    );
    let mut tree = mount_hidpi_vp_tree();
    let vp_node = tree.find_by_id("vp").expect("vp node");
    let _ = tree.build_paint_scene(1280, 800);
    let r1 = tree.node_layout(vp_node).expect("vp layout");
    assert!((r1.width - 1000.0).abs() < 1.0 && (r1.height - 714.0).abs() < 1.0);
    assert!(raw_vp.sync_with_node(&mut tree, vp_node, 1.5));
    assert_eq!(raw_vp.size(), (1500, 1071));
    assert!(tree.is_paint_dirty());

    let (scene1, _) = tree.build_paint_scene(1280, 800);
    let ext_ok = scene1.primitives.iter().any(|p| match p {
        DrawPrimitive::Image {
            rect,
            source:
                ImageSource::External {
                    id: ExternalTextureId(88),
                    ..
                },
            ..
        } => (rect.width - r1.width).abs() < 1.0 && (rect.height - r1.height).abs() < 1.0,
        _ => false,
    });
    assert!(
        ext_ok,
        "expected ExternalTexture primitive matching logical vp_rect"
    );

    let relayout_before = tree.relayout_passes();
    for _ in 0..10 {
        assert!(!raw_vp.sync_with_node(&mut tree, vp_node, 1.5));
        assert!(!tree.is_paint_dirty());
        let _ = tree.build_paint_scene(1280, 800);
    }
    assert_eq!(tree.relayout_passes(), relayout_before);
    let r2 = tree.node_layout(vp_node).expect("vp layout steady");
    assert!((r2.width - 1000.0).abs() < 1.0 && (r2.height - 714.0).abs() < 1.0);

    let _ = tree.build_paint_scene(800, 500);
    assert!(raw_vp.sync_with_node(&mut tree, vp_node, 1.5));
    assert_eq!(raw_vp.size(), (780, 621));
}
