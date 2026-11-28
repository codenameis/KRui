//! Inspection helpers and synthetic DOM event dispatchers for [`MockDomBackend`].

use crate::backend::{DomEventCallback, DomEventKind, DomNodeId};
use crate::mock_backend::{MockDomBackend, MockDomNode};
use krui_ui_types::{PointerEvent, WheelEvent};

impl MockDomBackend {
    /// Returns a node snapshot by id, or `None` if removed.
    pub fn node(&self, id: DomNodeId) -> Option<&MockDomNode> {
        self.nodes.get(&id)
    }

    /// Returns ordered top-level root child ids.
    pub fn roots(&self) -> &[DomNodeId] {
        &self.roots
    }

    /// Returns live child ids of `parent`, preserving DOM insertion order.
    pub fn children_of(&self, parent: DomNodeId) -> &[DomNodeId] {
        self.nodes
            .get(&parent)
            .map(|n| n.children.as_slice())
            .unwrap_or(&[])
    }

    /// Finds the first live node having HTML attribute `id == html_id`.
    pub fn find_by_html_id(&self, html_id: &str) -> Option<DomNodeId> {
        self.nodes
            .values()
            .find(|n| n.attrs.get("id").map(String::as_str) == Some(html_id))
            .map(|n| n.id)
    }

    /// Total number of currently registered active event listeners across all nodes.
    pub fn active_listener_count(&self) -> usize {
        self.listeners.len()
    }

    /// Returns injected stylesheets as `(is_ua, css_text)` slices.
    pub fn stylesheets(&self) -> &[(bool, String)] {
        &self.stylesheets
    }

    /// Returns registered embedded font keys.
    pub fn font_keys(&self) -> Vec<&str> {
        self.fonts.keys().map(String::as_str).collect()
    }

    /// Returns registered `click` handlers on `id` so tests can invoke them without holding tree borrows.
    pub fn click_handlers(&self, id: DomNodeId) -> Vec<std::rc::Rc<dyn Fn()>> {
        self.listeners
            .iter()
            .filter(|((nid, _), (kind, _))| *nid == id && *kind == DomEventKind::Click)
            .filter_map(|(_, (_, cb))| match cb {
                DomEventCallback::Notify(f) => Some(f.clone()),
                _ => None,
            })
            .collect()
    }

    /// Returns registered `pointer*` handlers on `id`.
    pub fn pointer_handlers(
        &self,
        id: DomNodeId,
        kind: DomEventKind,
    ) -> Vec<std::rc::Rc<dyn Fn(PointerEvent)>> {
        self.listeners
            .iter()
            .filter(|((nid, _), (k, _))| *nid == id && *k == kind)
            .filter_map(|(_, (_, cb))| match cb {
                DomEventCallback::Pointer(f) => Some(f.clone()),
                _ => None,
            })
            .collect()
    }

    /// Returns registered `input` handlers on `id`.
    pub fn input_handlers(&self, id: DomNodeId) -> Vec<std::rc::Rc<dyn Fn(String)>> {
        self.listeners
            .iter()
            .filter(|((nid, _), (k, _))| *nid == id && *k == DomEventKind::Input)
            .filter_map(|(_, (_, cb))| match cb {
                DomEventCallback::Text(f) => Some(f.clone()),
                _ => None,
            })
            .collect()
    }

    /// Dispatches a synthetic `click` event on `id`.
    pub fn dispatch_click(&self, id: DomNodeId) {
        let handlers: Vec<_> = self
            .listeners
            .iter()
            .filter(|((nid, _), (kind, _))| *nid == id && *kind == DomEventKind::Click)
            .map(|(_, (_, cb))| cb.clone())
            .collect();
        for cb in handlers {
            if let DomEventCallback::Notify(f) = cb {
                f();
            }
        }
    }

    /// Dispatches a text `input` event on `id`, skipping callbacks if IME composing is active.
    pub fn dispatch_input(&mut self, id: DomNodeId, text: &str) {
        let is_composing = self.nodes.get(&id).map(|n| n.is_composing).unwrap_or(false);
        if let Some(node) = self.nodes.get_mut(&id) {
            node.value = text.to_string();
        }
        if is_composing {
            return;
        }
        self.fire_text_handlers(id, DomEventKind::Input, text);
    }

    /// Simulates IME `compositionstart` -> intermediate `input` (suppressed) -> `compositionend`.
    pub fn dispatch_ime_composition_input(
        &mut self,
        id: DomNodeId,
        preedit: &str,
        committed: &str,
    ) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.is_composing = true;
            node.value = preedit.to_string();
        }
        self.dispatch_input(id, preedit);
        if let Some(node) = self.nodes.get_mut(&id) {
            node.is_composing = false;
            node.value = committed.to_string();
        }
        self.fire_text_handlers(id, DomEventKind::Input, committed);
    }

    /// Dispatches a `keydown` event on `id`.
    pub fn dispatch_keydown(&self, id: DomNodeId, key: &str) {
        self.fire_text_handlers(id, DomEventKind::KeyDown, key);
    }

    /// Dispatches a `pointerdown` / `pointermove` / `pointerup` event on `id`.
    pub fn dispatch_pointer(&self, id: DomNodeId, kind: DomEventKind, ev: PointerEvent) {
        let handlers: Vec<_> = self
            .listeners
            .iter()
            .filter(|((nid, _), (k, _))| *nid == id && *k == kind)
            .map(|(_, (_, cb))| cb.clone())
            .collect();
        for cb in handlers {
            if let DomEventCallback::Pointer(f) = cb {
                f(ev);
            }
        }
    }

    /// Dispatches a `wheel` event on `id` (`delta_y > 0` = scroll/wheel UP).
    pub fn dispatch_wheel(&self, id: DomNodeId, ev: WheelEvent) -> bool {
        let handlers: Vec<_> = self
            .listeners
            .iter()
            .filter(|((nid, _), (k, _))| *nid == id && *k == DomEventKind::Wheel)
            .map(|(_, (_, cb))| cb.clone())
            .collect();
        let mut prevented = false;
        for cb in handlers {
            if let DomEventCallback::Wheel(f) = cb {
                if f(ev) {
                    prevented = true;
                }
            }
        }
        prevented
    }

    fn fire_text_handlers(&self, id: DomNodeId, kind: DomEventKind, payload: &str) {
        let handlers: Vec<_> = self
            .listeners
            .iter()
            .filter(|((nid, _), (k, _))| *nid == id && *k == kind)
            .map(|(_, (_, cb))| cb.clone())
            .collect();
        for cb in handlers {
            if let DomEventCallback::Text(f) = cb {
                f(payload.to_string());
            }
        }
    }
}
