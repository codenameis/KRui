//! Live browser `web-sys` [`DomBackend`] targeting `#krui-root`.

use crate::backend::{DomBackend, DomEventCallback, DomEventKind, DomNodeId, ListenerId};
use crate::browser_canvas2d::CanvasBinding;
use crate::browser_events::{
    attach_delegated_ripple_listener, attach_listener, RegisteredListener,
};
use crate::browser_svg::{populate_svg_children, SVG_NS};
use crate::ua_css::rewrite_inline_asset_urls;
use krui_style::ColorSchemePreference;
use krui_ui_types::{CanvasDraw, SvgVectorTemplate};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::JsCast;

struct LiveNodeEntry {
    node: web_sys::Node,
    is_composing: Rc<RefCell<bool>>,
    listeners: Vec<RegisteredListener>,
    canvas: Option<CanvasBinding>,
    blob_url: Option<String>,
}

impl Drop for LiveNodeEntry {
    fn drop(&mut self) {
        if let Some(url) = self.blob_url.take() {
            let _ = web_sys::Url::revoke_object_url(&url);
        }
    }
}

/// Real browser DOM backend backed by `web-sys` inside `#krui-root`.
pub struct BrowserDomBackend {
    document: web_sys::Document,
    root: web_sys::Element,
    next_id: u32,
    next_listener_id: u64,
    nodes: HashMap<DomNodeId, LiveNodeEntry>,
    style_elements: Vec<web_sys::Element>,
    font_blob_urls: HashMap<String, String>,
    _ripple_listener: Option<RegisteredListener>,
}

impl Drop for BrowserDomBackend {
    fn drop(&mut self) {
        self.nodes.clear();
        for style_el in self.style_elements.drain(..) {
            if let Some(parent) = style_el.parent_node() {
                let _ = parent.remove_child(&style_el);
            }
        }
        for url in self.font_blob_urls.values() {
            let _ = web_sys::Url::revoke_object_url(url);
        }
    }
}

impl BrowserDomBackend {
    /// Creates a [`BrowserDomBackend`] mounted into `#krui-root` (or `None` outside DOM hosts).
    pub fn try_from_document() -> Option<Self> {
        let document = web_sys::window()?.document()?;
        let root = document
            .get_element_by_id("krui-root")
            .or_else(|| document.body().map(|b| b.unchecked_into()))?;
        root.set_inner_html("");
        let _ripple_listener = attach_delegated_ripple_listener(&root);
        Some(Self {
            document,
            root,
            next_id: 1,
            next_listener_id: 1,
            nodes: HashMap::new(),
            style_elements: Vec::new(),
            font_blob_urls: HashMap::new(),
            _ripple_listener,
        })
    }

    /// Returns the underlying `web_sys::Element` for a live [`DomNodeId`].
    pub fn element_by_id(&self, id: DomNodeId) -> Option<web_sys::Element> {
        self.nodes
            .get(&id)
            .and_then(|e| e.node.clone().dyn_into::<web_sys::Element>().ok())
    }

    fn alloc_entry(&mut self, node: web_sys::Node) -> DomNodeId {
        let id = DomNodeId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.nodes.insert(
            id,
            LiveNodeEntry {
                node,
                is_composing: Rc::new(RefCell::new(false)),
                listeners: Vec::new(),
                canvas: None,
                blob_url: None,
            },
        );
        id
    }

    fn parent_node(&self, parent: Option<DomNodeId>) -> &web_sys::Node {
        parent
            .and_then(|pid| self.nodes.get(&pid).map(|e| &e.node))
            .unwrap_or_else(|| self.root.as_ref())
    }

    fn create_blob_url(&self, mime: &str, bytes: &[u8]) -> Option<String> {
        let arr = js_sys::Uint8Array::from(bytes);
        let parts = js_sys::Array::of1(&arr);
        let bag = web_sys::BlobPropertyBag::new();
        bag.set_type(mime);
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &bag).ok()?;
        web_sys::Url::create_object_url_with_blob(&blob).ok()
    }
}

impl DomBackend for BrowserDomBackend {
    fn create_element(&mut self, tag: &str) -> DomNodeId {
        let safe_tag = if tag.is_empty() { "div" } else { tag };
        let node = match self.document.create_element(safe_tag) {
            Ok(el) => el.unchecked_into(),
            Err(_) => {
                let div = self
                    .document
                    .create_element("div")
                    .expect("document.createElement('div')");
                div.unchecked_into()
            }
        };
        self.alloc_entry(node)
    }

    fn create_svg_element(&mut self, tag: &str) -> DomNodeId {
        let node = self
            .document
            .create_element_ns(Some(SVG_NS), tag)
            .expect("createElementNS(svg)")
            .unchecked_into();
        self.alloc_entry(node)
    }

    fn create_text(&mut self, text: &str) -> DomNodeId {
        let node = self.document.create_text_node(text).unchecked_into();
        self.alloc_entry(node)
    }

    fn create_comment(&mut self, data: &str) -> DomNodeId {
        let node = self.document.create_comment(data).unchecked_into();
        self.alloc_entry(node)
    }

    fn append_child(&mut self, parent: Option<DomNodeId>, child: DomNodeId) {
        let Some(child_node) = self.nodes.get(&child).map(|e| e.node.clone()) else {
            return;
        };
        let parent_node = self.parent_node(parent);
        let _ = parent_node.append_child(&child_node);
    }

    fn insert_before(&mut self, parent: Option<DomNodeId>, child: DomNodeId, anchor: DomNodeId) {
        let Some(child_node) = self.nodes.get(&child).map(|e| e.node.clone()) else {
            return;
        };
        let anchor_node = self.nodes.get(&anchor).map(|e| e.node.clone());
        let parent_node = self.parent_node(parent);
        let _ = parent_node.insert_before(&child_node, anchor_node.as_ref());
    }

    fn remove_node(&mut self, id: DomNodeId) {
        let Some(mut entry) = self.nodes.remove(&id) else {
            return;
        };
        entry.listeners.clear();
        entry.canvas = None;
        if let Some(url) = entry.blob_url.take() {
            let _ = web_sys::Url::revoke_object_url(&url);
        }
        if let Some(parent) = entry.node.parent_node() {
            let _ = parent.remove_child(&entry.node);
        }
    }

    fn set_text(&mut self, id: DomNodeId, text: &str) {
        let Some(entry) = self.nodes.get(&id) else {
            return;
        };
        if entry.node.text_content().as_deref() != Some(text) {
            entry.node.set_text_content(Some(text));
        }
    }

    fn set_attr(&mut self, id: DomNodeId, name: &str, value: &str) {
        if let Some(el) = self.element_by_id(id) {
            let _ = el.set_attribute(name, value);
        }
    }

    fn remove_attr(&mut self, id: DomNodeId, name: &str) {
        if let Some(el) = self.element_by_id(id) {
            let _ = el.remove_attribute(name);
        }
    }

    fn set_classes(&mut self, id: DomNodeId, classes: &[&str]) {
        if let Some(el) = self.element_by_id(id) {
            if el.namespace_uri().as_deref() == Some(SVG_NS) {
                let _ = el.set_attribute("class", &classes.join(" "));
            } else {
                el.set_class_name(&classes.join(" "));
            }
        }
    }

    fn toggle_class(&mut self, id: DomNodeId, class_name: &str, enabled: bool) {
        if let Some(el) = self.element_by_id(id) {
            let list = el.class_list();
            if enabled {
                let _ = list.add_1(class_name);
            } else {
                let _ = list.remove_1(class_name);
            }
        }
    }

    fn set_inline_style(&mut self, id: DomNodeId, css_text: &str) {
        if let Some(el) = self.element_by_id(id) {
            let _ = el.set_attribute("style", css_text);
        }
    }

    fn set_style_property(&mut self, id: DomNodeId, prop: &str, value: &str) {
        let Some(entry) = self.nodes.get(&id) else {
            return;
        };
        if let Some(html) = entry.node.dyn_ref::<web_sys::HtmlElement>() {
            let _ = html.style().set_property(prop, value);
        }
    }

    fn set_input_value(&mut self, id: DomNodeId, value: &str) {
        let Some(entry) = self.nodes.get(&id) else {
            return;
        };
        if *entry.is_composing.borrow() {
            return;
        }
        if let Some(inp) = entry.node.dyn_ref::<web_sys::HtmlInputElement>() {
            if inp.value() != value {
                inp.set_value(value);
            }
        } else if let Some(ta) = entry.node.dyn_ref::<web_sys::HtmlTextAreaElement>() {
            if ta.value() != value {
                ta.set_value(value);
            }
        } else if let Some(sel) = entry.node.dyn_ref::<web_sys::HtmlSelectElement>() {
            if sel.value() != value {
                sel.set_value(value);
            }
        }
    }

    fn set_checked(&mut self, id: DomNodeId, checked: bool) {
        if let Some(inp) = self
            .nodes
            .get(&id)
            .and_then(|e| e.node.dyn_ref::<web_sys::HtmlInputElement>())
        {
            if inp.checked() != checked {
                inp.set_checked(checked);
            }
        }
    }

    fn set_disabled(&mut self, id: DomNodeId, disabled: bool) {
        let Some(entry) = self.nodes.get(&id) else {
            return;
        };
        if let Some(el) = entry.node.dyn_ref::<web_sys::HtmlButtonElement>() {
            el.set_disabled(disabled);
        } else if let Some(el) = entry.node.dyn_ref::<web_sys::HtmlInputElement>() {
            el.set_disabled(disabled);
        } else if let Some(el) = entry.node.dyn_ref::<web_sys::HtmlSelectElement>() {
            el.set_disabled(disabled);
        } else if let Some(el) = entry.node.dyn_ref::<web_sys::HtmlTextAreaElement>() {
            el.set_disabled(disabled);
        } else if let Some(el) = entry.node.dyn_ref::<web_sys::Element>() {
            if disabled {
                let _ = el.set_attribute("disabled", "true");
            } else {
                let _ = el.remove_attribute("disabled");
            }
        }
    }

    fn set_select_options(&mut self, id: DomNodeId, options: &[(String, String, bool)]) {
        let Some(sel) = self
            .nodes
            .get(&id)
            .and_then(|e| e.node.dyn_ref::<web_sys::HtmlSelectElement>())
        else {
            return;
        };
        sel.set_inner_html("");
        for (val, label, selected) in options {
            if let Some(opt) = self
                .document
                .create_element("option")
                .ok()
                .and_then(|e| e.dyn_into::<web_sys::HtmlOptionElement>().ok())
            {
                opt.set_value(val);
                opt.set_text_content(Some(label));
                opt.set_selected(*selected);
                let _ = sel.append_child(&opt);
            }
        }
    }

    fn set_svg_template(&mut self, id: DomNodeId, template: &SvgVectorTemplate) {
        if let Some(el) = self.element_by_id(id) {
            populate_svg_children(&self.document, &el, template);
        }
    }

    fn set_image_bytes(&mut self, id: DomNodeId, mime: &str, bytes: &[u8]) {
        let Some(url) = self.create_blob_url(mime, bytes) else {
            return;
        };
        let Some(entry) = self.nodes.get_mut(&id) else {
            let _ = web_sys::Url::revoke_object_url(&url);
            return;
        };
        if let Some(old) = entry.blob_url.replace(url.clone()) {
            let _ = web_sys::Url::revoke_object_url(&old);
        }
        if let Some(img) = entry.node.dyn_ref::<web_sys::HtmlImageElement>() {
            img.set_src(&url);
        }
    }

    fn register_embedded_font(&mut self, key: &str, bytes: &[u8]) {
        if self.font_blob_urls.contains_key(key) {
            return;
        }
        let mime = if key.ends_with(".woff2") {
            "font/woff2"
        } else if key.ends_with(".otf") {
            "font/otf"
        } else {
            "font/ttf"
        };
        if let Some(url) = self.create_blob_url(mime, bytes) {
            let token = format!("krui://inline/{key}");
            for style_el in &self.style_elements {
                if let Some(css) = style_el.text_content() {
                    if css.contains(&token) {
                        style_el.set_text_content(Some(&css.replace(&token, &url)));
                    }
                }
            }
            self.font_blob_urls.insert(key.to_string(), url);
        }
    }

    fn inject_stylesheet(&mut self, css: &str, _is_ua: bool) {
        let resolved = rewrite_inline_asset_urls(css, &self.font_blob_urls);
        let Ok(style_el) = self.document.create_element("style") else {
            return;
        };
        style_el.set_text_content(Some(&resolved));
        if let Some(head) = self.document.head() {
            let _ = head.append_child(&style_el);
        } else if let Some(doc_el) = self.document.document_element() {
            let _ = doc_el.append_child(&style_el);
        }
        self.style_elements.push(style_el);
    }

    fn set_color_scheme_preference(&mut self, pref: ColorSchemePreference) {
        let Some(root) = self.document.document_element() else {
            return;
        };
        let val = match pref {
            ColorSchemePreference::System => "system",
            ColorSchemePreference::Light => "light",
            ColorSchemePreference::Dark => "dark",
        };
        let _ = root.set_attribute("data-krui-scheme", val);
    }

    fn set_animations_paused(&mut self, paused: bool) {
        let Some(root) = self.document.document_element() else {
            return;
        };
        if paused {
            let _ = root.set_attribute("data-krui-anim-paused", "true");
        } else {
            let _ = root.remove_attribute("data-krui-anim-paused");
        }
    }

    fn add_event_listener(
        &mut self,
        id: DomNodeId,
        kind: DomEventKind,
        handler: DomEventCallback,
    ) -> ListenerId {
        let lid = ListenerId(self.next_listener_id);
        self.next_listener_id = self.next_listener_id.saturating_add(1);
        let Some(entry) = self.nodes.get_mut(&id) else {
            return lid;
        };
        if let Some(reg) = attach_listener(
            lid,
            &entry.node,
            Rc::clone(&entry.is_composing),
            kind,
            handler,
        ) {
            entry.listeners.push(reg);
        }
        lid
    }

    fn remove_event_listener(&mut self, id: DomNodeId, listener_id: ListenerId) {
        if let Some(entry) = self.nodes.get_mut(&id) {
            entry.listeners.retain(|l| l.id != listener_id);
        }
    }

    fn set_canvas_draw(&mut self, id: DomNodeId, draw: CanvasDraw) {
        let Some(entry) = self.nodes.get_mut(&id) else {
            return;
        };
        if let Some(binding) = &entry.canvas {
            binding.set_draw(draw);
            return;
        }
        if let Some(canvas) = entry.node.dyn_ref::<web_sys::HtmlCanvasElement>().cloned() {
            entry.canvas =
                CanvasBinding::new(canvas, draw, krui_signals::ReactiveSession::current());
        }
    }

    fn request_canvas_redraw(&mut self, id: DomNodeId) {
        if let Some(binding) = self.nodes.get(&id).and_then(|e| e.canvas.as_ref()) {
            binding.redraw();
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
