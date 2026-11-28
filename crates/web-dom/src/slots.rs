//! Imperative slot host registry allowing native canvas/3D islands (e.g. WebGPU/WebGL2)
//! to mount/unmount inside declarative `.krui` placeholder containers (`id="dev-3d-viewport-slot"`).

use crate::backend::DomNodeId;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

type SlotMountHook = Rc<dyn Fn(DomNodeId)>;
type SlotUnmountHook = Rc<dyn Fn(DomNodeId)>;

#[derive(Clone)]
struct SlotHostEntry {
    on_mount: SlotMountHook,
    on_unmount: SlotUnmountHook,
    active_node: Option<DomNodeId>,
}

/// Tracks imperative host hooks keyed by DOM element `id="..."` across control-flow lifecycles.
#[derive(Default)]
pub(crate) struct SlotHostRegistry {
    hooks: HashMap<String, SlotHostEntry>,
    active_by_node: HashMap<DomNodeId, String>,
}

impl SlotHostRegistry {
    /// Registers mount/unmount callbacks for elements carrying `id == html_id`.
    pub(crate) fn register(
        &mut self,
        html_id: &str,
        on_mount: impl Fn(DomNodeId) + 'static,
        on_unmount: impl Fn(DomNodeId) + 'static,
    ) {
        let entry = SlotHostEntry {
            on_mount: Rc::new(on_mount),
            on_unmount: Rc::new(on_unmount),
            active_node: None,
        };
        self.hooks.insert(html_id.to_string(), entry);
    }

    fn prepare_mount(
        &mut self,
        node_id: DomNodeId,
        html_id: &str,
    ) -> (Option<(DomNodeId, SlotUnmountHook)>, Option<SlotMountHook>) {
        let Some(entry) = self.hooks.get_mut(html_id) else {
            return (None, None);
        };
        let prev_unmount = entry.active_node.take().and_then(|prev| {
            if prev != node_id {
                self.active_by_node.remove(&prev);
                Some((prev, Rc::clone(&entry.on_unmount)))
            } else {
                None
            }
        });
        entry.active_node = Some(node_id);
        self.active_by_node.insert(node_id, html_id.to_string());
        (prev_unmount, Some(Rc::clone(&entry.on_mount)))
    }

    fn prepare_unmount(&mut self, node_id: DomNodeId) -> Option<SlotUnmountHook> {
        let html_id = self.active_by_node.remove(&node_id)?;
        let entry = self.hooks.get_mut(&html_id)?;
        if entry.active_node == Some(node_id) {
            entry.active_node = None;
            return Some(Rc::clone(&entry.on_unmount));
        }
        None
    }
}

/// Shared handle wrapper to re-notify slot hooks without holding overlapping borrows.
pub(crate) type SharedSlotRegistry = Rc<RefCell<SlotHostRegistry>>;

pub(crate) fn notify_slot_mounted(slots: &SharedSlotRegistry, node_id: DomNodeId, html_id: &str) {
    let (prev_unmount, mount_hook) = slots.borrow_mut().prepare_mount(node_id, html_id);
    if let Some((prev_id, unmount_hook)) = prev_unmount {
        unmount_hook(prev_id);
    }
    if let Some(mount_hook) = mount_hook {
        mount_hook(node_id);
    }
}

pub(crate) fn notify_slot_unmounting(slots: &SharedSlotRegistry, node_id: DomNodeId) {
    let unmount_hook = slots.borrow_mut().prepare_unmount(node_id);
    if let Some(hook) = unmount_hook {
        hook(node_id);
    }
}
