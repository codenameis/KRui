//! Integration tests for `krui-bevy` Mode A (Editor Host), Mode B (In-Pass HUD over shared Device/Queue),
//! and opt-in real `bevy_ecs::world::World` + `KruiBevyPlugin` synchronization.

#![forbid(unsafe_code)]

use krui_bevy::{
    BevyEditorConfig, BevyEditorViewportHost, BevyHudConfig, BevyInPassHudHost, EcsSyncBridge,
    EntityHandle, EntityNodeSnapshot, InspectorPatch, TransformSnapshot,
};
use krui_core::{ExternalTextureId, Mods, PointerButton, ViewTree};
use krui_renderer::GpuContext;
use krui_style::{Color, CompiledStyle, Dimension, LayoutStyle, PaintStyle};
use std::collections::HashMap;
use std::sync::mpsc;

struct MockEcsWorld {
    entities: HashMap<EntityHandle, EntityNodeSnapshot>,
}

impl EcsSyncBridge for MockEcsWorld {
    fn snapshot_entities(&self) -> Vec<EntityNodeSnapshot> {
        let mut out: Vec<_> = self.entities.values().cloned().collect();
        out.sort_by_key(|e| e.id.0);
        out
    }

    fn apply_patch(&mut self, entity: EntityHandle, patch: InspectorPatch) -> bool {
        let Some(node) = self.entities.get_mut(&entity) else {
            return false;
        };
        match patch {
            InspectorPatch::SetName(n) => node.name = n,
            InspectorPatch::SetVisible(v) => node.visible = v,
            InspectorPatch::SetTransform(t) => node.transform = t,
            InspectorPatch::Reparent(p) => node.parent = p,
        }
        true
    }
}

fn hud_style(w: f32, h: f32, bg: Color) -> CompiledStyle {
    CompiledStyle {
        layout: LayoutStyle {
            width: Dimension::Px(w),
            height: Dimension::Px(h),
            ..LayoutStyle::default()
        },
        paint: PaintStyle {
            background_color: bg,
            ..PaintStyle::default()
        },
        ..CompiledStyle::default()
    }
}

fn read_rgba_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    w: u32,
    h: u32,
) -> Vec<u8> {
    let bpr = (w * 4 + 255) & !255;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Bevy Integration Readback"),
        size: (bpr * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    enc.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &buf,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bpr),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(enc.finish()));
    let slice = buf.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| {
        let _ = tx.send(res);
    });
    device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    let mapped = slice.get_mapped_range();
    let mut out = vec![0_u8; (w * h * 4) as usize];
    for row in 0..h as usize {
        let s = row * bpr as usize;
        let d = row * (w as usize * 4);
        out[d..d + (w as usize * 4)].copy_from_slice(&mapped[s..s + (w as usize * 4)]);
    }
    drop(mapped);
    buf.unmap();
    out
}

#[test]
fn bevy_mode_a_editor_host_syncs_viewport_and_ecs_inspector() {
    let Some(gpu) = GpuContext::try_new() else {
        return;
    };
    let mut host = BevyEditorViewportHost::new(
        gpu,
        BevyEditorConfig {
            texture_id: ExternalTextureId(200),
            width: 64,
            height: 64,
            ..BevyEditorConfig::default()
        },
    );
    let mut tree = ViewTree::new();
    let vp_node = tree.create_compiled_node(
        None,
        "div",
        None,
        None,
        hud_style(160.0, 120.0, Color::TRANSPARENT),
        false,
        false,
        false,
        false,
    );
    let _ = tree.build_paint_scene(160, 120);

    let ent = EntityHandle(42);
    let mut world = MockEcsWorld {
        entities: HashMap::from([(
            ent,
            EntityNodeSnapshot {
                id: ent,
                parent: None,
                name: "Camera3D".into(),
                visible: true,
                transform: TransformSnapshot::default(),
            },
        )]),
    };

    let (_, pulled_first) = host.sync_frame(&mut tree, vp_node, 1.0, &mut world);
    assert!(pulled_first);
    assert_eq!(host.viewport().size(), (160, 120));
    host.inspector().select(Some(ent));
    assert_eq!(
        host.inspector().selected_snapshot().unwrap().name,
        "Camera3D"
    );

    host.inspector_mut()
        .queue_patch(ent, InspectorPatch::SetName("MainCamera".into()));
    let (applied, pulled_after) = host.sync_frame(&mut tree, vp_node, 1.0, &mut world);
    assert_eq!(applied, 1);
    assert!(pulled_after);
    assert_eq!(world.entities[&ent].name, "MainCamera");
}

fn build_hud_tree() -> ViewTree {
    let mut tree = ViewTree::new();
    let root = tree.create_compiled_node(
        None,
        "div",
        None,
        None,
        hud_style(160.0, 120.0, Color::TRANSPARENT),
        false,
        false,
        false,
        false,
    );
    let _bar = tree.create_compiled_node(
        Some(root),
        "div",
        None,
        None,
        hud_style(
            60.0,
            24.0,
            Color {
                r: 240,
                g: 30,
                b: 30,
                a: 255,
            },
        ),
        false,
        false,
        false,
        false,
    );
    tree
}

fn create_cleared_3d_target(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Mock Bevy 3D Frame Target"),
        size: wgpu::Extent3d {
            width: 160,
            height: 120,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Mock Bevy 3D Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.4,
                        b: 0.9,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    queue.submit(std::iter::once(enc.finish()));
    (tex, view)
}

#[test]
fn bevy_mode_b_in_pass_hud_preserves_3d_pixels_and_zero_relayouts() {
    let Some(gpu) = GpuContext::try_new() else {
        return;
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let mut hud = BevyInPassHudHost::new(
        build_hud_tree(),
        rt.handle().clone(),
        BevyHudConfig {
            width: 160,
            height: 120,
            scale_factor: 1.0,
            target_format: wgpu::TextureFormat::Rgba8Unorm,
        },
    );
    hud.attach_gpu_context(gpu.clone());
    let _ = hud.forward_cursor_moved(20.0, 10.0);
    assert!(!hud.forward_pointer_button(
        140.0,
        100.0,
        PointerButton::Secondary,
        true,
        Mods::default(),
        0
    ));
    let _ = hud.forward_mouse_wheel(20.0, 10.0, 0.0, -10.0, Mods::default());

    let (tex_3d, view_3d) = create_cleared_3d_target(gpu.device(), gpu.queue());
    let _ = hud.tick(1.0 / 60.0);
    let base_relayouts = hud.relayout_passes();
    let _ = hud.composite_over_3d_view(&view_3d).expect("composite HUD");

    let px = read_rgba_texture(gpu.device(), gpu.queue(), &tex_3d, 160, 120);
    let (hud_idx, bg_idx) = ((10 * 160 + 20) * 4, (100 * 160 + 130) * 4);
    assert!(px[hud_idx] > 200 && px[hud_idx + 2] < 50);
    assert!(px[bg_idx + 2] > 200 && px[bg_idx] < 20);
    let (_, relayout_after) = hud.tick(1.0 / 60.0);
    assert!(!relayout_after);
    assert_eq!(hud.relayout_passes(), base_relayouts);
}

#[cfg(feature = "bevy-ecs")]
#[test]
fn real_bevy_ecs_world_plugin_syncs_entities_bidirectionally() {
    use krui_bevy::{
        bevy_world::sync_world_and_inspector, KruiBevyHudResource, KruiBevyPlugin, KruiInspectable,
        ReactiveEcsInspector,
    };

    let mut app = bevy_app::App::new();
    app.add_plugins(KruiBevyPlugin);
    let cube = app
        .world_mut()
        .spawn(KruiInspectable {
            name: "HeroCube".into(),
            ..Default::default()
        })
        .id();
    app.update();

    let hud_res = app.world().resource::<KruiBevyHudResource>().clone();
    assert_eq!(hud_res.snapshots().len(), 1);
    assert_eq!(hud_res.snapshots()[0].name, "HeroCube");

    let handle = EntityHandle(cube.to_bits());
    hud_res.queue_patch(
        handle,
        InspectorPatch::SetTransform(TransformSnapshot {
            translation: [3.0, 4.0, 5.0],
            ..Default::default()
        }),
    );
    app.update();

    let comp = app.world().get::<KruiInspectable>(cube).unwrap();
    assert_eq!(comp.transform.translation, [3.0, 4.0, 5.0]);

    let mut inspector = ReactiveEcsInspector::new();
    let _ = sync_world_and_inspector(app.world_mut(), &mut inspector);
    assert_eq!(
        inspector.entities.get_untracked()[0].transform.translation,
        [3.0, 4.0, 5.0]
    );
}
