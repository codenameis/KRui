//! Browser DOM + native CSS + `<canvas>` external slot backend for KRui (`krui-web`).
//!
//! Explicitly excludes `wgpu`, `winit`, `stylo`, `taffy`, and `cosmic-text`, delegating layout,
//! cascading, and text shaping to the host DOM while binding fine-grained `krui-signals` effects.

#![forbid(unsafe_code)]

use krui_signals::{Effect, Scope, Signal};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Opaque DOM node identifier issued by [`DomBackend`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DomNodeId(pub u64);

/// Recorded DOM mutation emitted when reactive signals update mounted web elements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomMutation {
    CreateElement {
        id: DomNodeId,
        tag: String,
    },
    CreateText {
        id: DomNodeId,
        text: String,
    },
    SetText {
        id: DomNodeId,
        text: String,
    },
    SetAttribute {
        id: DomNodeId,
        name: String,
        value: Option<String>,
    },
    SetClass {
        id: DomNodeId,
        class_list: String,
    },
    SetInlineStyle {
        id: DomNodeId,
        name: String,
        value: String,
    },
    AppendChild {
        parent: DomNodeId,
        child: DomNodeId,
    },
    RemoveNode {
        id: DomNodeId,
    },
    RegisterCanvasSlot {
        node: DomNodeId,
        slot_id: u32,
    },
}

/// Abstract DOM bridge implemented by `wasm32`/JS glue (`web-sys`) or [`MockDomBackend`] in tests.
pub trait DomBackend {
    /// Creates an element with HTML `tag`.
    fn create_element(&mut self, tag: &str) -> DomNodeId;
    /// Creates a text node initialized with `initial`.
    fn create_text(&mut self, initial: &str) -> DomNodeId;
    /// Updates text content of a text node.
    fn set_text(&mut self, id: DomNodeId, text: &str);
    /// Sets or removes an attribute on an element node.
    fn set_attribute(&mut self, id: DomNodeId, name: &str, value: Option<&str>);
    /// Replaces an element's `class` attribute.
    fn set_class_list(&mut self, id: DomNodeId, class_list: &str);
    /// Sets an inline CSS property or custom `--*` variable (`style.setProperty`).
    fn set_inline_style(&mut self, id: DomNodeId, name: &str, value: &str);
    /// Appends `child` to `parent`.
    fn append_child(&mut self, parent: DomNodeId, child: DomNodeId);
    /// Detaches and removes `id` from the DOM tree.
    fn remove_node(&mut self, id: DomNodeId);
    /// Registers a `<canvas>` node with an external WebGL/WebGPU/2D host `slot_id`.
    fn register_canvas_slot(&mut self, node: DomNodeId, slot_id: u32);
}

#[derive(Clone, Debug, Default)]
struct MockDomNode {
    tag: String,
    text: String,
    class_list: String,
    attrs: HashMap<String, String>,
    styles: HashMap<String, String>,
    children: Vec<DomNodeId>,
    canvas_slot: Option<u32>,
}

/// Deterministic in-memory DOM backend for native host verification of `krui-web` mounts.
#[derive(Debug, Default)]
pub struct MockDomBackend {
    next_id: u64,
    nodes: HashMap<DomNodeId, MockDomNode>,
    /// Ordered mutation log emitted since initialization.
    pub mutations: Vec<DomMutation>,
}

impl MockDomBackend {
    /// Creates an empty mock DOM backend.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the element tag name stored for `id`.
    pub fn tag_of(&self, id: DomNodeId) -> Option<&str> {
        self.nodes.get(&id).map(|n| n.tag.as_str())
    }

    /// Returns the text stored for `id`.
    pub fn text_of(&self, id: DomNodeId) -> Option<&str> {
        self.nodes.get(&id).map(|n| n.text.as_str())
    }

    /// Returns the `class` string stored for `id`.
    pub fn class_of(&self, id: DomNodeId) -> Option<&str> {
        self.nodes.get(&id).map(|n| n.class_list.as_str())
    }

    /// Returns the registered external canvas slot id for `id`, if any.
    pub fn canvas_slot_of(&self, id: DomNodeId) -> Option<u32> {
        self.nodes.get(&id).and_then(|n| n.canvas_slot)
    }

    /// Returns an attribute value for `id`.
    pub fn attr_of(&self, id: DomNodeId, name: &str) -> Option<&str> {
        self.nodes
            .get(&id)
            .and_then(|n| n.attrs.get(name).map(String::as_str))
    }

    fn alloc_id(&mut self) -> DomNodeId {
        self.next_id += 1;
        DomNodeId(self.next_id)
    }
}

impl DomBackend for MockDomBackend {
    fn create_element(&mut self, tag: &str) -> DomNodeId {
        let id = self.alloc_id();
        self.nodes.insert(
            id,
            MockDomNode {
                tag: tag.to_string(),
                ..Default::default()
            },
        );
        self.mutations.push(DomMutation::CreateElement {
            id,
            tag: tag.to_string(),
        });
        id
    }

    fn create_text(&mut self, initial: &str) -> DomNodeId {
        let id = self.alloc_id();
        self.nodes.insert(
            id,
            MockDomNode {
                text: initial.to_string(),
                ..Default::default()
            },
        );
        self.mutations.push(DomMutation::CreateText {
            id,
            text: initial.to_string(),
        });
        id
    }

    fn set_text(&mut self, id: DomNodeId, text: &str) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.text.clear();
            node.text.push_str(text);
        }
        self.mutations.push(DomMutation::SetText {
            id,
            text: text.to_string(),
        });
    }

    fn set_attribute(&mut self, id: DomNodeId, name: &str, value: Option<&str>) {
        if let Some(node) = self.nodes.get_mut(&id) {
            match value {
                Some(v) => {
                    node.attrs.insert(name.to_string(), v.to_string());
                }
                None => {
                    node.attrs.remove(name);
                }
            }
        }
        self.mutations.push(DomMutation::SetAttribute {
            id,
            name: name.to_string(),
            value: value.map(str::to_string),
        });
    }

    fn set_class_list(&mut self, id: DomNodeId, class_list: &str) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.class_list.clear();
            node.class_list.push_str(class_list);
        }
        self.mutations.push(DomMutation::SetClass {
            id,
            class_list: class_list.to_string(),
        });
    }

    fn set_inline_style(&mut self, id: DomNodeId, name: &str, value: &str) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.styles.insert(name.to_string(), value.to_string());
        }
        self.mutations.push(DomMutation::SetInlineStyle {
            id,
            name: name.to_string(),
            value: value.to_string(),
        });
    }

    fn append_child(&mut self, parent: DomNodeId, child: DomNodeId) {
        if let Some(node) = self.nodes.get_mut(&parent) {
            node.children.push(child);
        }
        self.mutations
            .push(DomMutation::AppendChild { parent, child });
    }

    fn remove_node(&mut self, id: DomNodeId) {
        self.nodes.remove(&id);
        self.mutations.push(DomMutation::RemoveNode { id });
    }

    fn register_canvas_slot(&mut self, node: DomNodeId, slot_id: u32) {
        if let Some(n) = self.nodes.get_mut(&node) {
            n.canvas_slot = Some(slot_id);
        }
        self.mutations
            .push(DomMutation::RegisterCanvasSlot { node, slot_id });
    }
}

/// Reactive DOM mount controller owning an isolated `Scope` for mounted web effects.
pub struct WebMount<B: DomBackend> {
    backend: Rc<RefCell<B>>,
    scope: Scope,
}

impl<B: DomBackend + 'static> WebMount<B> {
    /// Creates a new [`WebMount`] over `backend` with a fresh reactive disposal [`Scope`].
    pub fn new(backend: B) -> Self {
        Self {
            backend: Rc::new(RefCell::new(backend)),
            scope: Scope::new(),
        }
    }

    /// Borrows the underlying backend for reading or driving test assertions.
    pub fn with_backend<R>(&self, f: impl FnOnce(&B) -> R) -> R {
        f(&self.backend.borrow())
    }

    /// Creates an element and appends it to `parent` if specified.
    pub fn create_element(&self, parent: Option<DomNodeId>, tag: &str) -> DomNodeId {
        let mut b = self.backend.borrow_mut();
        let id = b.create_element(tag);
        if let Some(p) = parent {
            b.append_child(p, id);
        }
        id
    }

    /// Creates a reactive text node driven by `signal` and appends it to `parent`.
    pub fn bind_text(&self, parent: DomNodeId, signal: Signal<String>) -> DomNodeId {
        let text_id = {
            let mut b = self.backend.borrow_mut();
            let id = b.create_text(&signal.get_untracked());
            b.append_child(parent, id);
            id
        };
        let backend = Rc::clone(&self.backend);
        self.scope.run(move || {
            let _ = Effect::new(move || {
                let val = signal.get();
                backend.borrow_mut().set_text(text_id, &val);
            });
        });
        text_id
    }

    /// Binds an element's `class` list reactively to `signal`.
    pub fn bind_class(&self, node: DomNodeId, signal: Signal<String>) {
        let backend = Rc::clone(&self.backend);
        self.scope.run(move || {
            let _ = Effect::new(move || {
                let val = signal.get();
                backend.borrow_mut().set_class_list(node, &val);
            });
        });
    }

    /// Binds a CSS variable or inline property (`style.setProperty`) reactively to `signal`.
    pub fn bind_style(&self, node: DomNodeId, property: &'static str, signal: Signal<String>) {
        let backend = Rc::clone(&self.backend);
        self.scope.run(move || {
            let _ = Effect::new(move || {
                let val = signal.get();
                backend.borrow_mut().set_inline_style(node, property, &val);
            });
        });
    }

    /// Creates a `<canvas>` element bound to an external render slot (`ExternalTextureId` analog).
    pub fn mount_canvas_slot(&self, parent: DomNodeId, slot_id: u32) -> DomNodeId {
        let mut b = self.backend.borrow_mut();
        let canvas = b.create_element("canvas");
        b.register_canvas_slot(canvas, slot_id);
        b.append_child(parent, canvas);
        canvas
    }

    /// Disposes the owning `Scope`, unregistering every reactive DOM effect attached to this mount.
    pub fn dispose(self) {
        self.scope.dispose();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reactive_signals_update_mock_dom_and_stop_after_dispose() {
        let mount = WebMount::new(MockDomBackend::new());
        let root = mount.create_element(None, "div");
        let cls = Signal::new("btn primary".to_string());
        let label = Signal::new("Count: 0".to_string());
        mount.bind_class(root, cls);
        let txt = mount.bind_text(root, label);
        let canvas = mount.mount_canvas_slot(root, 42);

        mount.with_backend(|b| {
            assert_eq!(b.class_of(root), Some("btn primary"));
            assert_eq!(b.text_of(txt), Some("Count: 0"));
            assert_eq!(b.canvas_slot_of(canvas), Some(42));
        });

        cls.set("btn active".to_string());
        label.set("Count: 1".to_string());
        mount.with_backend(|b| {
            assert_eq!(b.class_of(root), Some("btn active"));
            assert_eq!(b.text_of(txt), Some("Count: 1"));
        });

        let mut_len_before_dispose = mount.with_backend(|b| b.mutations.len());
        let backend_handle = Rc::clone(&mount.backend);
        mount.dispose();
        cls.set("after dispose".to_string());
        label.set("ignored".to_string());
        assert_eq!(
            backend_handle.borrow().mutations.len(),
            mut_len_before_dispose
        );
    }
}
