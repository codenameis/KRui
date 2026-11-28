//! Deterministic in-memory [`DomBackend`] used for native tests and headless verification.

use crate::backend::{
    DomBackend, DomEventCallback, DomEventKind, DomMutation, DomNodeId, ListenerId,
};
use krui_style::ColorSchemePreference;
use krui_ui_types::{CanvasCommand, CanvasContext2D, CanvasDraw, Size, SvgVectorTemplate};
use std::collections::HashMap;

/// Logical node stored inside [`MockDomBackend`].
#[derive(Clone)]
pub struct MockDomNode {
    /// Assigned id.
    pub id: DomNodeId,
    /// Tag name (`"#text"`, `"#comment"`, `"div"`, `"input"`, `"svg"`, `"canvas"`, ...).
    pub tag: String,
    /// Parent node id, or `None` for root children.
    pub parent: Option<DomNodeId>,
    /// Ordered child node ids.
    pub children: Vec<DomNodeId>,
    /// Element text content or comment payload.
    pub text: String,
    /// HTML attributes.
    pub attrs: HashMap<String, String>,
    /// CSS classes.
    pub classes: Vec<String>,
    /// Inline style attribute text.
    pub inline_style: String,
    /// Discrete inline style property map (`transform`, `opacity`, `width`, ...).
    pub style_props: HashMap<String, String>,
    /// Live `<input>` / `<textarea>` / `<select>` `.value`.
    pub value: String,
    /// Live `.checked` flag.
    pub checked: bool,
    /// Live `.disabled` flag.
    pub disabled: bool,
    /// Active IME composition state for input simulation.
    pub is_composing: bool,
    /// `<select>` option entries `(value, label, selected)`.
    pub select_options: Vec<(String, String, bool)>,
    /// Recorded SVG template, if mounted on this node.
    pub svg_template: Option<SvgVectorTemplate>,
    /// Recorded 2D `<canvas>` commands from the most recent draw.
    pub canvas_commands: Vec<CanvasCommand>,
    /// Bound `<canvas>` draw closure.
    pub canvas_draw: Option<CanvasDraw>,
}

/// Deterministic host-free DOM backend for `cargo test -p krui-web` and headless WASM unit tests.
pub struct MockDomBackend {
    next_id: u32,
    next_listener_id: u64,
    pub(super) roots: Vec<DomNodeId>,
    pub(super) nodes: HashMap<DomNodeId, MockDomNode>,
    pub(super) listeners: HashMap<(DomNodeId, ListenerId), (DomEventKind, DomEventCallback)>,
    mutations: Vec<DomMutation>,
    pub(super) stylesheets: Vec<(bool, String)>,
    pub(super) fonts: HashMap<String, Vec<u8>>,
    /// Currently configured CSS color scheme preference on `:root`.
    pub color_scheme: ColorSchemePreference,
    /// Whether `:root[data-krui-anim-paused="true"]` is currently active.
    pub animations_paused: bool,
}

impl Default for MockDomBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MockDomBackend {
    /// Creates an empty [`MockDomBackend`].
    pub fn new() -> Self {
        Self {
            next_id: 1,
            next_listener_id: 1,
            roots: Vec::new(),
            nodes: HashMap::new(),
            listeners: HashMap::new(),
            mutations: Vec::new(),
            stylesheets: Vec::new(),
            fonts: HashMap::new(),
            color_scheme: ColorSchemePreference::System,
            animations_paused: false,
        }
    }

    fn alloc_node(&mut self, tag: &str) -> DomNodeId {
        let id = DomNodeId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.nodes.insert(
            id,
            MockDomNode {
                id,
                tag: tag.to_string(),
                parent: None,
                children: Vec::new(),
                text: String::new(),
                attrs: HashMap::new(),
                classes: Vec::new(),
                inline_style: String::new(),
                style_props: HashMap::new(),
                value: String::new(),
                checked: false,
                disabled: false,
                is_composing: false,
                select_options: Vec::new(),
                svg_template: None,
                canvas_commands: Vec::new(),
                canvas_draw: None,
            },
        );
        id
    }

    fn detach_from_parent(&mut self, child: DomNodeId) {
        let prev_parent = self.nodes.get(&child).and_then(|n| n.parent);
        match prev_parent {
            Some(pid) => {
                if let Some(p) = self.nodes.get_mut(&pid) {
                    p.children.retain(|&c| c != child);
                }
            }
            None => self.roots.retain(|&r| r != child),
        }
    }

    fn collect_subtree(&self, id: DomNodeId, out: &mut Vec<DomNodeId>) {
        out.push(id);
        if let Some(node) = self.nodes.get(&id) {
            for &child in &node.children {
                self.collect_subtree(child, out);
            }
        }
    }
}

impl DomBackend for MockDomBackend {
    fn create_element(&mut self, tag: &str) -> DomNodeId {
        let id = self.alloc_node(tag);
        self.mutations.push(DomMutation::CreateElement {
            id,
            tag: tag.to_string(),
        });
        id
    }

    fn create_svg_element(&mut self, tag: &str) -> DomNodeId {
        let id = self.alloc_node(tag);
        self.mutations.push(DomMutation::CreateElement {
            id,
            tag: tag.to_string(),
        });
        id
    }

    fn create_text(&mut self, text: &str) -> DomNodeId {
        let id = self.alloc_node("#text");
        if let Some(node) = self.nodes.get_mut(&id) {
            node.text = text.to_string();
        }
        self.mutations.push(DomMutation::SetText {
            id,
            text: text.to_string(),
        });
        id
    }

    fn create_comment(&mut self, data: &str) -> DomNodeId {
        let id = self.alloc_node("#comment");
        if let Some(node) = self.nodes.get_mut(&id) {
            node.text = data.to_string();
        }
        self.mutations.push(DomMutation::CreateComment {
            id,
            data: data.to_string(),
        });
        id
    }

    fn append_child(&mut self, parent: Option<DomNodeId>, child: DomNodeId) {
        self.detach_from_parent(child);
        if let Some(node) = self.nodes.get_mut(&child) {
            node.parent = parent;
        }
        match parent {
            Some(pid) => {
                if let Some(p) = self.nodes.get_mut(&pid) {
                    p.children.push(child);
                }
            }
            None => self.roots.push(child),
        }
        self.mutations
            .push(DomMutation::AppendChild { parent, child });
    }

    fn insert_before(&mut self, parent: Option<DomNodeId>, child: DomNodeId, anchor: DomNodeId) {
        self.detach_from_parent(child);
        if let Some(node) = self.nodes.get_mut(&child) {
            node.parent = parent;
        }
        let target_list = match parent {
            Some(pid) => match self.nodes.get_mut(&pid) {
                Some(p) => &mut p.children,
                None => return,
            },
            None => &mut self.roots,
        };
        if let Some(pos) = target_list.iter().position(|&id| id == anchor) {
            target_list.insert(pos, child);
        } else {
            target_list.push(child);
        }
        self.mutations
            .push(DomMutation::AppendChild { parent, child });
    }

    fn remove_node(&mut self, id: DomNodeId) {
        self.detach_from_parent(id);
        let mut to_remove = Vec::new();
        self.collect_subtree(id, &mut to_remove);
        for rem in to_remove {
            self.nodes.remove(&rem);
            self.listeners.retain(|&(nid, _), _| nid != rem);
        }
        self.mutations.push(DomMutation::RemoveNode { id });
    }

    fn set_text(&mut self, id: DomNodeId, text: &str) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.text = text.to_string();
        }
        self.mutations.push(DomMutation::SetText {
            id,
            text: text.to_string(),
        });
    }

    fn set_attr(&mut self, id: DomNodeId, name: &str, value: &str) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.attrs.insert(name.to_string(), value.to_string());
        }
        self.mutations.push(DomMutation::SetAttribute {
            id,
            name: name.to_string(),
            value: value.to_string(),
        });
    }

    fn remove_attr(&mut self, id: DomNodeId, name: &str) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.attrs.remove(name);
        }
    }

    fn set_classes(&mut self, id: DomNodeId, classes: &[&str]) {
        let owned: Vec<String> = classes.iter().map(|s| (*s).to_string()).collect();
        if let Some(node) = self.nodes.get_mut(&id) {
            node.classes = owned.clone();
        }
        self.mutations
            .push(DomMutation::SetClasses { id, classes: owned });
    }

    fn toggle_class(&mut self, id: DomNodeId, class_name: &str, enabled: bool) {
        if let Some(node) = self.nodes.get_mut(&id) {
            if enabled {
                if !node.classes.iter().any(|c| c == class_name) {
                    node.classes.push(class_name.to_string());
                }
            } else {
                node.classes.retain(|c| c != class_name);
            }
        }
    }

    fn set_inline_style(&mut self, id: DomNodeId, css_text: &str) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.inline_style = css_text.to_string();
        }
        self.mutations.push(DomMutation::SetInlineStyle {
            id,
            style: css_text.to_string(),
        });
    }

    fn set_style_property(&mut self, id: DomNodeId, prop: &str, value: &str) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.style_props.insert(prop.to_string(), value.to_string());
        }
    }

    fn set_input_value(&mut self, id: DomNodeId, value: &str) {
        if let Some(node) = self.nodes.get_mut(&id) {
            if !node.is_composing && node.value != value {
                node.value = value.to_string();
            }
        }
        self.mutations.push(DomMutation::SetInputValue {
            id,
            value: value.to_string(),
        });
    }

    fn set_checked(&mut self, id: DomNodeId, checked: bool) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.checked = checked;
        }
    }

    fn set_disabled(&mut self, id: DomNodeId, disabled: bool) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.disabled = disabled;
        }
    }

    fn set_select_options(&mut self, id: DomNodeId, options: &[(String, String, bool)]) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.select_options = options.to_vec();
            if let Some((val, _, _)) = options.iter().find(|(_, _, sel)| *sel) {
                node.value = val.clone();
            }
        }
    }

    fn set_svg_template(&mut self, id: DomNodeId, template: &SvgVectorTemplate) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.svg_template = Some(template.clone());
        }
        self.mutations.push(DomMutation::MountSvgTemplate { id });
    }

    fn set_image_bytes(&mut self, id: DomNodeId, mime: &str, bytes: &[u8]) {
        self.set_attr(id, "data-inline-mime", mime);
        self.set_attr(id, "data-inline-bytes", &bytes.len().to_string());
    }

    fn register_embedded_font(&mut self, key: &str, bytes: &[u8]) {
        self.fonts.insert(key.to_string(), bytes.to_vec());
    }

    fn inject_stylesheet(&mut self, css: &str, is_ua: bool) {
        self.stylesheets.push((is_ua, css.to_string()));
    }

    fn set_color_scheme_preference(&mut self, pref: ColorSchemePreference) {
        self.color_scheme = pref;
    }

    fn set_animations_paused(&mut self, paused: bool) {
        self.animations_paused = paused;
    }

    fn add_event_listener(
        &mut self,
        id: DomNodeId,
        kind: DomEventKind,
        handler: DomEventCallback,
    ) -> ListenerId {
        let lid = ListenerId(self.next_listener_id);
        self.next_listener_id = self.next_listener_id.saturating_add(1);
        self.listeners.insert((id, lid), (kind, handler));
        lid
    }

    fn remove_event_listener(&mut self, id: DomNodeId, listener_id: ListenerId) {
        self.listeners.remove(&(id, listener_id));
    }

    fn set_canvas_draw(&mut self, id: DomNodeId, draw: CanvasDraw) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.canvas_draw = Some(draw);
        }
        self.request_canvas_redraw(id);
    }

    fn request_canvas_redraw(&mut self, id: DomNodeId) {
        let Some(draw) = self.nodes.get(&id).and_then(|n| n.canvas_draw.clone()) else {
            return;
        };
        let mut ctx = CanvasContext2D::with_size(428.0, 260.0);
        draw.draw(&mut ctx, Size::new(428.0, 260.0));
        if let Some(node) = self.nodes.get_mut(&id) {
            node.canvas_commands = ctx.into_commands();
        }
    }

    fn drain_mutations(&mut self) -> Vec<DomMutation> {
        std::mem::take(&mut self.mutations)
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
