//! Bidirectional ECS <-> KRui `Signal` synchronization primitives and bridge trait.

use krui_signals::{batch, Signal};

/// Opaque 64-bit entity identifier (directly compatible with `bevy_ecs::entity::Entity::to_bits()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntityHandle(pub u64);

/// TRS transform snapshot shared between ECS components and KRui editor inspectors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformSnapshot {
    /// World/local translation `[x, y, z]`.
    pub translation: [f32; 3],
    /// Euler rotation in degrees `[pitch, yaw, roll]`.
    pub rotation_deg: [f32; 3],
    /// Non-uniform scale `[x, y, z]`.
    pub scale: [f32; 3],
}

impl Default for TransformSnapshot {
    fn default() -> Self {
        Self {
            translation: [0.0, 0.0, 0.0],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

/// Read-side snapshot of one inspectable ECS entity node for Hierarchy/Inspector views.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityNodeSnapshot {
    /// Stable entity handle (`Entity::to_bits()`).
    pub id: EntityHandle,
    /// Optional parent entity handle in the scene hierarchy.
    pub parent: Option<EntityHandle>,
    /// Display name shown in Hierarchy / Inspector.
    pub name: String,
    /// Active visibility flag.
    pub visible: bool,
    /// Entity transform.
    pub transform: TransformSnapshot,
}

/// UI-originated mutation queued for the next ECS writeback flush.
#[derive(Clone, Debug, PartialEq)]
pub enum InspectorPatch {
    /// Renames the target entity.
    SetName(String),
    /// Toggles visibility of the target entity.
    SetVisible(bool),
    /// Updates local TRS transform of the target entity.
    SetTransform(TransformSnapshot),
    /// Changes the parent entity (`None` moves to root).
    Reparent(Option<EntityHandle>),
}

/// Host ECS synchronization contract implemented for Bevy `World` or custom ECS backends.
pub trait EcsSyncBridge {
    /// Collects the latest entity hierarchy & transform snapshots from the ECS world.
    fn snapshot_entities(&self) -> Vec<EntityNodeSnapshot>;

    /// Applies an [`InspectorPatch`] from KRui UI to `entity`, returning `true` if committed.
    fn apply_patch(&mut self, entity: EntityHandle, patch: InspectorPatch) -> bool;
}

/// Reactive bidirectional inspector state connecting ECS snapshots with KRui UI `Signal`s.
pub struct ReactiveEcsInspector {
    /// Reactive entity snapshot list for Hierarchy / TreeView binding.
    pub entities: Signal<Vec<EntityNodeSnapshot>>,
    /// Currently selected entity handle in the editor UI.
    pub selected: Signal<Option<EntityHandle>>,
    /// Monotonic revision bumped whenever snapshots change.
    pub revision: Signal<u64>,
    pending_patches: Vec<(EntityHandle, InspectorPatch)>,
}

impl Default for ReactiveEcsInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl ReactiveEcsInspector {
    /// Creates an empty reactive inspector state.
    pub fn new() -> Self {
        Self {
            entities: Signal::new(Vec::new()),
            selected: Signal::new(None),
            revision: Signal::new(0),
            pending_patches: Vec::new(),
        }
    }

    /// Selects `entity` in the reactive inspector.
    pub fn select(&self, entity: Option<EntityHandle>) {
        self.selected.set(entity);
    }

    /// Returns the currently selected entity snapshot, if present.
    pub fn selected_snapshot(&self) -> Option<EntityNodeSnapshot> {
        let sel = self.selected.get_untracked()?;
        self.entities
            .get_untracked()
            .into_iter()
            .find(|e| e.id == sel)
    }

    /// Queues a UI-side inspector edit to be flushed into the ECS world on the next frame sync.
    pub fn queue_patch(&mut self, entity: EntityHandle, patch: InspectorPatch) {
        self.pending_patches.push((entity, patch));
    }

    /// Returns the number of queued UI -> ECS patches waiting to flush.
    pub fn pending_patch_count(&self) -> usize {
        self.pending_patches.len()
    }

    /// Flushes all pending UI edits into `ecs` and returns the count of applied patches.
    pub fn flush_to_ecs(&mut self, ecs: &mut impl EcsSyncBridge) -> usize {
        let mut applied = 0usize;
        for (entity, patch) in self.pending_patches.drain(..) {
            if ecs.apply_patch(entity, patch) {
                applied += 1;
            }
        }
        applied
    }

    /// Pulls fresh snapshots from `ecs` and updates reactive `entities`/`revision` if changed.
    pub fn pull_from_ecs(&self, ecs: &impl EcsSyncBridge) -> bool {
        let fresh = ecs.snapshot_entities();
        if self.entities.get_untracked() == fresh {
            return false;
        }
        let next_rev = self.revision.get_untracked().wrapping_add(1);
        let entities = self.entities;
        let revision = self.revision;
        batch(move || {
            entities.set(fresh);
            revision.set(next_rev);
        });
        true
    }
}
