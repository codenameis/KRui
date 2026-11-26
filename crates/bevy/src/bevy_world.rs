//! Direct Bevy `0.15` `bevy_ecs` + `bevy_app` bindings enabled by feature `bevy-ecs`.

use crate::ecs_sync::{
    EcsSyncBridge, EntityHandle, EntityNodeSnapshot, InspectorPatch, ReactiveEcsInspector,
    TransformSnapshot,
};
use bevy_app::{App, Plugin, Update};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::system::{ResMut, Resource};
use bevy_ecs::world::World;
use std::sync::{Arc, Mutex};

/// Bevy ECS component exposing an entity's hierarchy metadata and transform to KRui inspectors.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct KruiInspectable {
    /// Human-readable entity name in editor trees.
    pub name: String,
    /// Optional parent entity handle.
    pub parent_id: Option<EntityHandle>,
    /// Visibility flag.
    pub visible: bool,
    /// Local TRS transform.
    pub transform: TransformSnapshot,
}

impl Default for KruiInspectable {
    fn default() -> Self {
        Self {
            name: "Entity".to_string(),
            parent_id: None,
            visible: true,
            transform: TransformSnapshot::default(),
        }
    }
}

/// [`EcsSyncBridge`] adapter wrapping a mutable borrow of Bevy's [`World`].
pub struct BevyWorldAdapter<'w>(pub &'w mut World);

impl EcsSyncBridge for BevyWorldAdapter<'_> {
    fn snapshot_entities(&self) -> Vec<EntityNodeSnapshot> {
        let mut out = Vec::new();
        for arch in self.0.archetypes().iter() {
            for arch_ent in arch.entities() {
                let e = arch_ent.id();
                if let Some(item) = self.0.get::<KruiInspectable>(e) {
                    out.push(EntityNodeSnapshot {
                        id: EntityHandle(e.to_bits()),
                        parent: item.parent_id,
                        name: item.name.clone(),
                        visible: item.visible,
                        transform: item.transform,
                    });
                }
            }
        }
        out.sort_by_key(|snap| snap.id.0);
        out
    }

    fn apply_patch(&mut self, entity: EntityHandle, patch: InspectorPatch) -> bool {
        let ent = Entity::from_bits(entity.0);
        let Some(mut comp) = self.0.get_mut::<KruiInspectable>(ent) else {
            return false;
        };
        match patch {
            InspectorPatch::SetName(name) => comp.name = name,
            InspectorPatch::SetVisible(visible) => comp.visible = visible,
            InspectorPatch::SetTransform(transform) => comp.transform = transform,
            InspectorPatch::Reparent(parent) => comp.parent_id = parent,
        }
        true
    }
}

/// Bevy `Resource` synchronized once per `Update` tick with external KRui UI inspector threads.
#[derive(Resource, Default, Clone)]
pub struct KruiBevyHudResource {
    /// Pending UI -> ECS patches pushed from KRui controls.
    pub pending_patches: Arc<Mutex<Vec<(EntityHandle, InspectorPatch)>>>,
    /// Latest ECS entity snapshots readable by KRui editors.
    pub latest_snapshots: Arc<Mutex<Vec<EntityNodeSnapshot>>>,
}

impl KruiBevyHudResource {
    /// Queues a mutation from UI controls into Bevy's next `Update` sync step.
    pub fn queue_patch(&self, entity: EntityHandle, patch: InspectorPatch) {
        if let Ok(mut guard) = self.pending_patches.lock() {
            guard.push((entity, patch));
        }
    }

    /// Reads the latest mirrored ECS snapshots.
    pub fn snapshots(&self) -> Vec<EntityNodeSnapshot> {
        self.latest_snapshots
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Flushes into a [`ReactiveEcsInspector`] if running on the UI reactive thread.
    pub fn copy_into_inspector(&self, inspector: &ReactiveEcsInspector) {
        let fresh = self.snapshots();
        if inspector.entities.get_untracked() != fresh {
            inspector.entities.set(fresh);
            inspector
                .revision
                .set(inspector.revision.get_untracked().wrapping_add(1));
        }
    }
}

/// Bevy [`Plugin`] adding [`KruiBevyHudResource`] and an `Update` ECS snapshot/patch sync system.
#[derive(Default)]
pub struct KruiBevyPlugin;

impl Plugin for KruiBevyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KruiBevyHudResource>()
            .add_systems(Update, sync_krui_inspectable_world);
    }
}

fn sync_krui_inspectable_world(world: &mut World) {
    let mut hud = world.resource_mut::<KruiBevyHudResource>().clone();
    {
        let mut adapter = BevyWorldAdapter(world);
        if let Ok(mut queue) = hud.pending_patches.lock() {
            for (entity, patch) in queue.drain(..) {
                let _ = adapter.apply_patch(entity, patch);
            }
        }
        let snaps = adapter.snapshot_entities();
        if let Ok(mut dst) = hud.latest_snapshots.lock() {
            *dst = snaps;
        }
    }
    let _ = &mut hud;
}

/// Exposes direct single-call sync between a Bevy [`World`] and [`ReactiveEcsInspector`].
pub fn sync_world_and_inspector(world: &mut World, inspector: &mut ReactiveEcsInspector) -> usize {
    let mut adapter = BevyWorldAdapter(world);
    let applied = inspector.flush_to_ecs(&mut adapter);
    let _ = inspector.pull_from_ecs(&adapter);
    applied
}

#[allow(dead_code)]
fn _assert_res_mut_compiles(_: ResMut<'_, KruiBevyHudResource>) {}
