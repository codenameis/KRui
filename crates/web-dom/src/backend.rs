//! Backend abstraction (`DomBackend`) decoupling `WebDomTree` reactivity from browser DOM APIs.

use krui_style::ColorSchemePreference;
use krui_ui_types::{CanvasDraw, PointerEvent, SvgVectorTemplate, WheelEvent};
use std::rc::Rc;

/// Opaque DOM node identifier managed by a [`DomBackend`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DomNodeId(pub u32);

/// Opaque event-listener handle returned by [`DomBackend::add_event_listener`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ListenerId(pub u64);

/// Legacy/recorded DOM mutation event emitted by [`crate::WebMount`] and diagnostic hooks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomMutation {
    /// Element created with `tag`.
    CreateElement {
        /// Assigned node id.
        id: DomNodeId,
        /// HTML tag name.
        tag: String,
    },
    /// Comment anchor created with `data`.
    CreateComment {
        /// Assigned node id.
        id: DomNodeId,
        /// Comment text.
        data: String,
    },
    /// Appended `child` under `parent`.
    AppendChild {
        /// Parent node, or `None` for mount root.
        parent: Option<DomNodeId>,
        /// Child node.
        child: DomNodeId,
    },
    /// Set text content of `id`.
    SetText {
        /// Target node.
        id: DomNodeId,
        /// New text content.
        text: String,
    },
    /// Set HTML attribute on `id`.
    SetAttribute {
        /// Target node.
        id: DomNodeId,
        /// Attribute name.
        name: String,
        /// Attribute value.
        value: String,
    },
    /// Applied CSS classes on `id`.
    SetClasses {
        /// Target node.
        id: DomNodeId,
        /// Space-joined or individual classes.
        classes: Vec<String>,
    },
    /// Applied inline style declaration on `id`.
    SetInlineStyle {
        /// Target node.
        id: DomNodeId,
        /// CSS style string.
        style: String,
    },
    /// Set form element `.value` property.
    SetInputValue {
        /// Target node.
        id: DomNodeId,
        /// New value.
        value: String,
    },
    /// Mounted or replaced rendered SVG template on `id`.
    MountSvgTemplate {
        /// Target node.
        id: DomNodeId,
    },
    /// Removed `id` and its subtree.
    RemoveNode {
        /// Removed node.
        id: DomNodeId,
    },
}

/// Supported DOM event categories bound through [`DomBackend::add_event_listener`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DomEventKind {
    /// Mouse/keyboard activation (`click`).
    Click,
    /// Committed or live text input (`input`, IME composition-guarded).
    Input,
    /// Committed value/checked change (`change`).
    Change,
    /// Keyboard press (`keydown`).
    KeyDown,
    /// Focus gained (`focus`).
    Focus,
    /// Focus lost (`blur`).
    Blur,
    /// Pointer press (`pointerdown`).
    PointerDown,
    /// Pointer release (`pointerup`).
    PointerUp,
    /// Pointer move (`pointermove`).
    PointerMove,
    /// Context menu invocation (`contextmenu`).
    ContextMenu,
    /// Wheel scroll (`wheel`, with negated Y delta matching KRui core).
    Wheel,
    /// Scroll offset notification (`scroll`).
    Scroll,
    /// Form submit (`submit`, default prevented).
    Submit,
    /// Form reset (`reset`, default prevented).
    Reset,
}

/// Registered DOM event handler variant executed by a [`DomBackend`].
#[derive(Clone)]
pub enum DomEventCallback {
    /// Zero-argument event callback (`click`, `focus`, `blur`, `scroll`, `submit`, `reset`).
    Notify(Rc<dyn Fn()>),
    /// String payload callback (`input`, `change`, `keydown`).
    Text(Rc<dyn Fn(String)>),
    /// Boolean payload callback (`checked` change on checkbox/radio).
    Bool(Rc<dyn Fn(bool)>),
    /// Pointer payload callback (`pointerdown`, `pointerup`, `pointermove`, `contextmenu`).
    Pointer(Rc<dyn Fn(PointerEvent)>),
    /// Wheel payload callback (`wheel`), returning `true` to prevent default scrolling.
    Wheel(Rc<dyn Fn(WheelEvent) -> bool>),
}

/// Platform abstraction for mutating live DOM trees, CSS rules, 2D `<canvas>`, and `<svg>` icons.
pub trait DomBackend {
    /// Creates an HTML element with normalized tag `tag`.
    fn create_element(&mut self, tag: &str) -> DomNodeId;

    /// Creates an SVG namespace element with tag `tag`.
    fn create_svg_element(&mut self, tag: &str) -> DomNodeId;

    /// Creates a raw DOM `Text` node (used for synthetic inline text runs).
    fn create_text(&mut self, text: &str) -> DomNodeId;

    /// Creates a DOM `Comment` node used as a zero-box `<if>` / `<for>` insertion anchor.
    fn create_comment(&mut self, data: &str) -> DomNodeId;

    /// Appends `child` as the last child of `parent` (or mount root if `None`).
    fn append_child(&mut self, parent: Option<DomNodeId>, child: DomNodeId);

    /// Inserts `child` under `parent` immediately before `anchor`.
    fn insert_before(&mut self, parent: Option<DomNodeId>, child: DomNodeId, anchor: DomNodeId);

    /// Detaches `id` and recursively releases listeners/observers in its subtree.
    fn remove_node(&mut self, id: DomNodeId);

    /// Updates the text content of an element or `Text` node.
    fn set_text(&mut self, id: DomNodeId, text: &str);

    /// Sets HTML attribute `name` to `value` on `id`.
    fn set_attr(&mut self, id: DomNodeId, name: &str, value: &str);

    /// Removes HTML attribute `name` from `id`.
    fn remove_attr(&mut self, id: DomNodeId, name: &str);

    /// Replaces the CSS class token list on `id`.
    fn set_classes(&mut self, id: DomNodeId, classes: &[&str]);

    /// Adds or removes a single CSS class token on `id`.
    fn toggle_class(&mut self, id: DomNodeId, class_name: &str, enabled: bool);

    /// Replaces the inline `style` attribute CSS text on `id`.
    fn set_inline_style(&mut self, id: DomNodeId, css_text: &str);

    /// Sets an individual CSS property on `id.style`.
    fn set_style_property(&mut self, id: DomNodeId, prop: &str, value: &str);

    /// Sets live `.value` on `<input>`, `<textarea>`, or `<select>` without breaking IME composition.
    fn set_input_value(&mut self, id: DomNodeId, value: &str);

    /// Sets live `.checked` on checkbox/radio `<input>` elements.
    fn set_checked(&mut self, id: DomNodeId, checked: bool);

    /// Sets disabled property/attribute state on interactive controls.
    fn set_disabled(&mut self, id: DomNodeId, disabled: bool);

    /// Populates `<option>` children and selection state inside a `<select>` element.
    fn set_select_options(&mut self, id: DomNodeId, options: &[(String, String, bool)]);

    /// Renders an [`SvgVectorTemplate`] directly as crisp SVG DOM vector elements.
    fn set_svg_template(&mut self, id: DomNodeId, template: &SvgVectorTemplate);

    /// Attaches decoded-type raw image bytes (PNG/JPEG/WebP) to an `<img>` element via Blob/Data URL.
    fn set_image_bytes(&mut self, id: DomNodeId, mime: &str, bytes: &[u8]);

    /// Registers embedded font bytes under `krui://inline/<key>` for `@font-face` URL resolution.
    fn register_embedded_font(&mut self, key: &str, bytes: &[u8]);

    /// Injects a compiled or UA stylesheet into the document `<head>`.
    fn inject_stylesheet(&mut self, css: &str, is_ua: bool);

    /// Sets active `data-krui-scheme` and browser color scheme hint.
    fn set_color_scheme_preference(&mut self, pref: ColorSchemePreference);

    /// Sets `:root[data-krui-anim-paused="true"]` when `paused` is `true`.
    fn set_animations_paused(&mut self, paused: bool);

    /// Attaches an event listener on `id`, returning a handle for deterministic cleanup.
    fn add_event_listener(
        &mut self,
        id: DomNodeId,
        kind: DomEventKind,
        handler: DomEventCallback,
    ) -> ListenerId;

    /// Detaches an event listener previously attached on `id`.
    fn remove_event_listener(&mut self, id: DomNodeId, listener_id: ListenerId);

    /// Binds a 2D [`CanvasDraw`] closure to `<canvas>` node `id`.
    fn set_canvas_draw(&mut self, id: DomNodeId, draw: CanvasDraw);

    /// Marks `<canvas>` node `id` dirty so it redraws using its bound [`CanvasDraw`].
    fn request_canvas_redraw(&mut self, id: DomNodeId);

    /// Returns the recorded mutation log in mock/test backends (empty on live browser backend).
    fn drain_mutations(&mut self) -> Vec<DomMutation> {
        Vec::new()
    }

    /// Optional runtime downcast handle for deterministic unit-test inspection.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Optional mutable runtime downcast handle for deterministic event simulation in tests.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
}
