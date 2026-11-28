//! Reactive DOM text, class, attribute, and form-control property bindings.

use crate::backend::{DomEventCallback, DomEventKind, DomNodeId};
use crate::tree::WebDomTree;
use krui_signals::Effect;
use krui_style::{Color, FontFamilyStack, StyledTextSpan};
use krui_ui_types::{ButtonKind, InputKind};

impl WebDomTree {
    /// Sets static text or input `.value` depending on whether `id` is a form value control.
    pub fn set_static_text(&mut self, id: DomNodeId, text: &str) {
        let is_val = {
            let guard = self.inner.borrow();
            guard
                .nodes
                .get(&id)
                .map(|m| m.is_value_control)
                .unwrap_or(false)
        };
        let mut guard = self.inner.borrow_mut();
        if is_val {
            guard.backend.set_input_value(id, text);
        } else {
            guard.backend.set_text(id, text);
        }
    }

    /// Reactively binds computed text or form `.value` updates to `id`.
    pub fn bind_dynamic_text(&mut self, id: DomNodeId, text_fn: impl Fn() -> String + 'static) {
        let tree = self.clone();
        let _ = Effect::new(move || {
            let next = text_fn();
            let mut tree_mut = tree.clone();
            tree_mut.set_static_text(id, &next);
        });
    }

    /// Replaces children of `id` with inline styled `<span>` runs matching `spans`.
    pub fn set_rich_spans(&mut self, id: DomNodeId, spans: Vec<StyledTextSpan>) {
        let old_children: Vec<DomNodeId> = {
            self.inner
                .borrow()
                .nodes
                .get(&id)
                .map(|m| m.children.clone())
                .unwrap_or_default()
        };
        for child_id in old_children {
            self.remove_subtree(child_id);
        }
        for span_spec in spans {
            let span_id = self.inner.borrow_mut().backend.create_element("span");
            self.record_created_node(span_id, Some(id), "span", false, None);
            let css = format_rich_span_css(&span_spec);
            let mut guard = self.inner.borrow_mut();
            guard.backend.set_inline_style(span_id, &css);
            guard.backend.set_text(span_id, &span_spec.text);
            guard.backend.append_child(Some(id), span_id);
        }
    }

    /// Reactively binds inline styled rich-text spans to `id`.
    pub fn bind_dynamic_rich_spans(
        &mut self,
        id: DomNodeId,
        spans_fn: impl Fn() -> Vec<StyledTextSpan> + 'static,
    ) {
        let tree = self.clone();
        let _ = Effect::new(move || {
            let next = spans_fn();
            let mut tree_mut = tree.clone();
            tree_mut.set_rich_spans(id, next);
        });
    }

    /// Sets static CSS classes on `id`.
    pub fn set_element_classes(&mut self, id: DomNodeId, classes: &[&str]) {
        self.inner.borrow_mut().backend.set_classes(id, classes);
    }

    /// Reactively toggles CSS class tokens produced by `class_fn` while preserving static classes.
    pub fn bind_dynamic_class(&mut self, id: DomNodeId, class_fn: impl Fn() -> String + 'static) {
        let tree = self.clone();
        let prev_tokens: std::rc::Rc<std::cell::RefCell<Vec<String>>> =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let _ = Effect::new(move || {
            let next_raw = class_fn();
            let next: Vec<String> = next_raw.split_whitespace().map(str::to_string).collect();
            let old = std::mem::replace(&mut *prev_tokens.borrow_mut(), next.clone());
            let mut guard = tree.inner.borrow_mut();
            for o in &old {
                if !next.iter().any(|n| n == o) {
                    guard.backend.toggle_class(id, o, false);
                }
            }
            for n in &next {
                guard.backend.toggle_class(id, n, true);
            }
        });
    }

    /// Sets or removes a static HTML attribute on `id`.
    pub fn set_node_attr(&mut self, id: DomNodeId, name: impl AsRef<str>, value: &str) {
        self.inner
            .borrow_mut()
            .backend
            .set_attr(id, name.as_ref(), value);
    }

    /// Reactively binds an optional HTML attribute `name` (`Some(v)` sets, `None` removes).
    pub fn bind_dynamic_attr(
        &mut self,
        id: DomNodeId,
        name: impl Into<String>,
        val_fn: impl Fn() -> Option<String> + 'static,
    ) {
        let name = name.into();
        let tree = self.clone();
        let _ = Effect::new(move || {
            let next = val_fn();
            let mut guard = tree.inner.borrow_mut();
            match next {
                Some(v) => guard.backend.set_attr(id, &name, &v),
                None => guard.backend.remove_attr(id, &name),
            }
        });
    }

    /// Sets static inline `style="..."` CSS declaration text on `id`.
    pub fn set_inline_style(&mut self, id: DomNodeId, css_text: &str) {
        self.inner
            .borrow_mut()
            .backend
            .set_inline_style(id, css_text);
    }

    /// Reactively updates inline `style="..."` CSS declaration text on `id`.
    pub fn bind_dynamic_inline_style(
        &mut self,
        id: DomNodeId,
        style_fn: impl Fn() -> String + 'static,
    ) {
        let tree = self.clone();
        let _ = Effect::new(move || {
            let next = style_fn();
            tree.inner.borrow_mut().backend.set_inline_style(id, &next);
        });
    }

    /// Sets static `placeholder` attribute on `id`.
    pub fn set_placeholder(&mut self, id: DomNodeId, text: &str) {
        self.set_node_attr(id, "placeholder", text);
    }

    /// Reactively binds `placeholder` attribute on `id`.
    pub fn bind_dynamic_placeholder(
        &mut self,
        id: DomNodeId,
        val_fn: impl Fn() -> String + 'static,
    ) {
        self.bind_dynamic_attr(id, "placeholder", move || Some(val_fn()));
    }

    /// Sets static `aria-label` attribute on `id`.
    pub fn set_aria_label(&mut self, id: DomNodeId, text: &str) {
        self.set_node_attr(id, "aria-label", text);
    }

    /// Reactively binds `aria-label` attribute on `id`.
    pub fn bind_dynamic_aria_label(
        &mut self,
        id: DomNodeId,
        val_fn: impl Fn() -> String + 'static,
    ) {
        self.bind_dynamic_attr(id, "aria-label", move || Some(val_fn()));
    }

    /// Sets static `title` tooltip attribute on `id`.
    pub fn set_title(&mut self, id: DomNodeId, text: &str) {
        self.set_node_attr(id, "title", text);
    }

    /// Reactively binds `title` tooltip attribute on `id`.
    pub fn bind_dynamic_title(&mut self, id: DomNodeId, val_fn: impl Fn() -> String + 'static) {
        self.bind_dynamic_attr(id, "title", move || Some(val_fn()));
    }

    /// Sets static `<a href="...">` destination on `id`.
    pub fn set_link_href(&mut self, id: DomNodeId, href: &str) {
        self.set_node_attr(id, "href", href);
    }

    /// Reactively binds `<a href="...">` destination on `id`.
    pub fn bind_dynamic_link_href(&mut self, id: DomNodeId, val_fn: impl Fn() -> String + 'static) {
        self.bind_dynamic_attr(id, "href", move || Some(val_fn()));
    }

    /// Appends a single static `<option>` entry inside `<select>` `id`.
    pub fn add_select_option(&mut self, id: DomNodeId, value: &str, label: &str, selected: bool) {
        let mut guard = self.inner.borrow_mut();
        let opt_id = guard.backend.create_element("option");
        guard.backend.set_attr(opt_id, "value", value);
        guard.backend.set_text(opt_id, label);
        if selected {
            guard.backend.set_attr(opt_id, "selected", "true");
        }
        guard.backend.append_child(Some(id), opt_id);
        drop(guard);
        self.record_created_node(opt_id, Some(id), "option", false, None);
    }

    /// Reactively binds `<option>` children inside `<select>` `id`.
    pub fn bind_select_options(
        &mut self,
        id: DomNodeId,
        opts_fn: impl Fn() -> Vec<(String, String, bool)> + 'static,
    ) {
        let tree = self.clone();
        let _ = Effect::new(move || {
            let next = opts_fn();
            tree.inner
                .borrow_mut()
                .backend
                .set_select_options(id, &next);
        });
    }

    /// Sets static `disabled` state on `id`.
    pub fn set_disabled(&mut self, id: DomNodeId, disabled: bool) {
        self.inner.borrow_mut().backend.set_disabled(id, disabled);
    }

    /// Reactively binds `disabled` state on `id`.
    pub fn bind_disabled(&mut self, id: DomNodeId, val_fn: impl Fn() -> bool + 'static) {
        let tree = self.clone();
        let _ = Effect::new(move || {
            let dis = val_fn();
            tree.inner.borrow_mut().backend.set_disabled(id, dis);
        });
    }

    /// Sets static `required` attribute on `id`.
    pub fn set_required(&mut self, id: DomNodeId, required: bool) {
        let mut guard = self.inner.borrow_mut();
        if required {
            guard.backend.set_attr(id, "required", "true");
        } else {
            guard.backend.remove_attr(id, "required");
        }
    }

    /// Reactively binds `required` attribute on `id`.
    pub fn bind_required(&mut self, id: DomNodeId, val_fn: impl Fn() -> bool + 'static) {
        let tree = self.clone();
        let _ = Effect::new(move || {
            let req = val_fn();
            let mut tree_mut = tree.clone();
            tree_mut.set_required(id, req);
        });
    }

    /// Sets `.checked` state on checkbox/radio `id`.
    pub fn set_checked(&mut self, id: DomNodeId, checked: bool) {
        self.inner.borrow_mut().backend.set_checked(id, checked);
    }

    /// Reactively binds `.checked` state on checkbox/radio `id`.
    pub fn bind_checked(&mut self, id: DomNodeId, val_fn: impl Fn() -> bool + 'static) {
        let tree = self.clone();
        let _ = Effect::new(move || {
            let chk = val_fn();
            tree.inner.borrow_mut().backend.set_checked(id, chk);
        });
    }

    /// Binds a `change` listener reporting `.checked` boolean updates.
    pub fn set_on_checked_change(&mut self, id: DomNodeId, handler: impl Fn(bool) + 'static) {
        let session = self.inner.borrow().session.clone();
        let cb = std::rc::Rc::new(handler);
        self.inner.borrow_mut().backend.add_event_listener(
            id,
            DomEventKind::Change,
            DomEventCallback::Bool(std::rc::Rc::new(move |checked| {
                let f = std::rc::Rc::clone(&cb);
                session.enter(move || f(checked));
            })),
        );
    }

    /// Sets `<label for="...">` attribute on `id`.
    pub fn set_label_for(&mut self, id: DomNodeId, target: &str) {
        self.set_node_attr(id, "for", target);
    }

    /// Sets `name="..."` form control attribute on `id`.
    pub fn set_control_name(&mut self, id: DomNodeId, name: &str) {
        self.set_node_attr(id, "name", name);
    }

    /// Configures `<button type="...">` on `id`.
    pub fn set_button_kind(&mut self, id: DomNodeId, kind: ButtonKind) {
        let t = match kind {
            ButtonKind::Auto => return,
            ButtonKind::Button => "button",
            ButtonKind::Submit => "submit",
            ButtonKind::Reset => "reset",
        };
        self.set_node_attr(id, "type", t);
    }

    /// Marks checkbox semantics (`type="checkbox"`) on `id`.
    pub fn set_checkbox_role(&mut self, id: DomNodeId) {
        self.set_node_attr(id, "type", "checkbox");
    }

    /// Marks radio semantics (`type="radio"`) on `id`.
    pub fn set_radio_role(&mut self, id: DomNodeId) {
        self.set_node_attr(id, "type", "radio");
    }

    /// Configures `<input type="range" min max step>` bounds on `id`.
    pub fn set_range_bounds(&mut self, id: DomNodeId, min: f32, max: f32, step: f32) {
        let mut guard = self.inner.borrow_mut();
        guard.backend.set_attr(id, "type", "range");
        guard.backend.set_attr(id, "min", &min.to_string());
        guard.backend.set_attr(id, "max", &max.to_string());
        guard.backend.set_attr(id, "step", &step.to_string());
    }

    /// Configures `<input type="...">` on `id`.
    pub fn set_input_kind(&mut self, id: DomNodeId, kind: InputKind) {
        let t = match kind {
            InputKind::Text => "text",
            InputKind::Password => "password",
            InputKind::Number => "number",
            InputKind::Email => "email",
            InputKind::Search => "search",
        };
        self.set_node_attr(id, "type", t);
    }

    /// Records non-standard tag metadata via `data-krui-tag` on `id`.
    pub fn set_custom_tag(&mut self, id: DomNodeId, tag: &str) {
        self.set_node_attr(id, "data-krui-tag", tag);
    }
}

fn format_rich_span_css(span: &StyledTextSpan) -> String {
    let family = match span.font_family {
        FontFamilyStack::CosmicMono => "monospace",
        FontFamilyStack::Serif => "serif",
        FontFamilyStack::Custom(_) => {
            if let Some(names) = span.font_family.custom_families() {
                return format_span_with_family(span, &names.join(", "));
            }
            "sans-serif"
        }
        _ => "Roboto, sans-serif",
    };
    format_span_with_family(span, family)
}

fn format_span_with_family(span: &StyledTextSpan, family: &str) -> String {
    let style = if span.italic { "italic" } else { "normal" };
    let color_part = match span.color {
        Some(Color { r, g, b, a }) => {
            format!("color: rgba({r}, {g}, {b}, {:.4}); ", a as f32 / 255.0)
        }
        None => String::new(),
    };
    format!(
        "font-weight: {}; font-style: {style}; font-family: {family}; {color_part}",
        span.font_weight
    )
}
