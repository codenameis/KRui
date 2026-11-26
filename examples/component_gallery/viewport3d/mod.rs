//! Dual 3D viewport host supporting both direct Raw `wgpu` (`RawWgpuViewport`) and
//! Bevy 0.15 ECS (`krui-bevy` + `BevyEditorViewportHost`) using the same open `.obj` model.

pub mod mesh_obj;
pub mod wgpu_scene;

use krui::{ExternalTextureId, GpuContext, NodeId, ViewTree};
use krui_bevy::{
    bevy_app::App, BevyEditorConfig, BevyEditorViewportHost, BevyWorldAdapter, EntityHandle,
    InspectorPatch, KruiBevyPlugin, KruiInspectable, TransformSnapshot,
};
use krui_engine_host::{RawWgpuViewport, RawWgpuViewportConfig};
use mesh_obj::{load_shared_open_model, SHARED_OPEN_MODEL_NAME};
use std::sync::Arc;
use wgpu_scene::{RenderModelParams, SharedModelGpu};

/// External texture ID reserved for Raw `wgpu` direct mode.
pub const RAW_WGPU_TEX_ID: ExternalTextureId = ExternalTextureId(801);
/// External texture ID reserved for Bevy 0.15 ECS viewport mode.
pub const BEVY_TEX_ID: ExternalTextureId = ExternalTextureId(802);

/// Camera and model state passed each frame into [`Gallery3dHost::sync_and_render`].
#[derive(Clone, Copy, Debug)]
pub struct Viewport3dFrameInput {
    pub engine_mode: i32,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub distance: f32,
    pub model_scale: f32,
}

/// Manages both `Raw wgpu` and `Bevy 0.15 ECS` 3D viewports sharing one uploaded `open_teapot.obj` mesh.
pub struct Gallery3dHost {
    shared_gpu: SharedModelGpu,
    vertex_count: usize,
    tri_count: usize,
    raw_viewport: RawWgpuViewport,
    bevy_host: BevyEditorViewportHost,
    bevy_app: App,
    bevy_entity: EntityHandle,
    mounted_binding: Option<(i32, NodeId)>,
}

impl Gallery3dHost {
    /// Initializes GPU mesh buffers, Raw `wgpu` MSAA target, and a Bevy ECS `World` + entity.
    pub fn new(gpu: Arc<GpuContext>) -> Self {
        let mesh = load_shared_open_model();
        let (vertex_count, tri_count) = (mesh.vertices.len(), mesh.indices.len() / 3);
        let shared_gpu = SharedModelGpu::new(
            gpu.device(),
            gpu.queue(),
            &mesh,
            wgpu::TextureFormat::Rgba8Unorm,
            4,
        );
        let raw_viewport = RawWgpuViewport::new(
            gpu.clone(),
            RAW_WGPU_TEX_ID,
            RawWgpuViewportConfig {
                width: 640,
                height: 400,
                color_format: wgpu::TextureFormat::Rgba8Unorm,
                sample_count: 4,
                depth_format: Some(wgpu::TextureFormat::Depth32Float),
            },
        );
        let mut bevy_host = BevyEditorViewportHost::new(
            gpu,
            BevyEditorConfig {
                texture_id: BEVY_TEX_ID,
                width: 640,
                height: 400,
                color_format: wgpu::TextureFormat::Rgba8Unorm,
                sample_count: 4,
                depth_format: Some(wgpu::TextureFormat::Depth32Float),
            },
        );
        let mut bevy_app = App::new();
        bevy_app.add_plugins(KruiBevyPlugin);
        let ent = bevy_app
            .world_mut()
            .spawn(KruiInspectable {
                name: format!("OpenTeapot ({SHARED_OPEN_MODEL_NAME})"),
                parent_id: None,
                visible: true,
                transform: TransformSnapshot::default(),
            })
            .id();
        let bevy_entity = EntityHandle(ent.to_bits());
        bevy_host.inspector_mut().select(Some(bevy_entity));
        Self {
            shared_gpu,
            vertex_count,
            tri_count,
            raw_viewport,
            bevy_host,
            bevy_app,
            bevy_entity,
            mounted_binding: None,
        }
    }

    /// Returns `(width, height)` physical size of the active mode's offscreen viewport.
    pub fn active_physical_size(&self, engine_mode: i32) -> (u32, u32) {
        if engine_mode == 1 {
            self.bevy_host.viewport().size()
        } else {
            self.raw_viewport.size()
        }
    }

    /// Returns latest Bevy ECS entity snapshot synchronized from `bevy_app.world()`.
    pub fn bevy_entity_snapshot(&self) -> Option<krui_bevy::EntityNodeSnapshot> {
        self.bevy_host.inspector().selected_snapshot()
    }

    /// Pre-binds the active engine mode's [`ExternalTextureId`] before scene building.
    pub fn ensure_bound(&mut self, tree: &mut ViewTree, viewport_node: NodeId, mode: i32) -> bool {
        let mode = mode.clamp(0, 1);
        if self.mounted_binding == Some((mode, viewport_node)) {
            return false;
        }
        self.ensure_node_texture_binding(tree, viewport_node, mode);
        true
    }

    /// Returns an invariant telemetry label for the active 3D backend and shared open mesh.
    pub fn mode_summary(&self, engine_mode: i32) -> String {
        if engine_mode == 1 {
            format!(
                "Bevy 0.15 ECS (Entity #{}) · {} ({}v/{}t)",
                self.bevy_entity.0, SHARED_OPEN_MODEL_NAME, self.vertex_count, self.tri_count
            )
        } else {
            format!(
                "Raw wgpu Direct (4x MSAA + Depth32Float) · {} ({}v/{}t)",
                SHARED_OPEN_MODEL_NAME, self.vertex_count, self.tri_count
            )
        }
    }

    fn ensure_node_texture_binding(
        &mut self,
        tree: &mut ViewTree,
        viewport_node: NodeId,
        mode: i32,
    ) {
        if self.mounted_binding == Some((mode, viewport_node)) {
            return;
        }
        let tex_id = if mode == 1 {
            BEVY_TEX_ID
        } else {
            RAW_WGPU_TEX_ID
        };
        let (pw, ph) = self.active_physical_size(mode);
        tree.mount_external_texture(viewport_node, tex_id, pw.max(1), ph.max(1));
        self.mounted_binding = Some((mode, viewport_node));
    }

    /// Resizes active offscreen target, runs Raw `wgpu` or Bevy ECS update, and draws `open_teapot.obj`.
    pub fn sync_and_render(
        &mut self,
        tree: &mut ViewTree,
        viewport_node: NodeId,
        scale_factor: f32,
        input: Viewport3dFrameInput,
    ) -> String {
        let mode = input.engine_mode.clamp(0, 1);
        self.ensure_node_texture_binding(tree, viewport_node, mode);
        if mode == 1 {
            self.render_bevy_frame(tree, viewport_node, scale_factor, input)
        } else {
            self.render_raw_wgpu_frame(tree, viewport_node, scale_factor, input)
        }
    }

    fn render_bevy_frame(
        &mut self,
        tree: &mut ViewTree,
        viewport_node: NodeId,
        scale_factor: f32,
        input: Viewport3dFrameInput,
    ) -> String {
        self.bevy_host.inspector_mut().queue_patch(
            self.bevy_entity,
            InspectorPatch::SetTransform(TransformSnapshot {
                translation: [0.0, 0.0, 0.0],
                rotation_deg: [input.pitch_deg, input.yaw_deg, 0.0],
                scale: [input.model_scale; 3],
            }),
        );
        {
            let mut bridge = BevyWorldAdapter(self.bevy_app.world_mut());
            let _ = self
                .bevy_host
                .sync_frame(tree, viewport_node, scale_factor, &mut bridge);
        }
        self.bevy_app.update();
        let tr = self
            .bevy_entity_snapshot()
            .map_or(TransformSnapshot::default(), |s| s.transform);
        self.shared_gpu.render_into(
            self.bevy_host.viewport(),
            RenderModelParams {
                yaw_deg: tr.rotation_deg[1],
                pitch_deg: tr.rotation_deg[0],
                distance: input.distance,
                model_scale: tr.scale[0],
                clear_rgba: [0.05, 0.12, 0.13, 1.0],
                base_color: [0.92, 0.56, 0.36, 1.0],
            },
        );
        format!(
            "Bevy 0.15 ECS (Entity #{}) · rot=({:.0}°, {:.0}°) scale={:.2} · {} ({}v/{}t)",
            self.bevy_entity.0,
            tr.rotation_deg[0],
            tr.rotation_deg[1],
            tr.scale[0],
            SHARED_OPEN_MODEL_NAME,
            self.vertex_count,
            self.tri_count
        )
    }

    fn render_raw_wgpu_frame(
        &mut self,
        tree: &mut ViewTree,
        viewport_node: NodeId,
        scale_factor: f32,
        input: Viewport3dFrameInput,
    ) -> String {
        let _ = self
            .raw_viewport
            .sync_with_node(tree, viewport_node, scale_factor);
        self.shared_gpu.render_into(
            &self.raw_viewport,
            RenderModelParams {
                yaw_deg: input.yaw_deg,
                pitch_deg: input.pitch_deg,
                distance: input.distance,
                model_scale: input.model_scale,
                clear_rgba: [0.06, 0.09, 0.16, 1.0],
                base_color: [0.55, 0.49, 0.92, 1.0],
            },
        );
        format!(
            "Raw wgpu Direct (4x MSAA + Depth32Float) · rot=({:.0}°, {:.0}°) · {} ({}v/{}t)",
            input.pitch_deg,
            input.yaw_deg,
            SHARED_OPEN_MODEL_NAME,
            self.vertex_count,
            self.tri_count
        )
    }
}
