//! DOM event, SVG vector icon, 2D `<canvas>`, and dynamic `<img>` bindings on [`WebDomTree`].

use crate::backend::{DomEventCallback, DomEventKind, DomNodeId};
use crate::tree::WebDomTree;
use krui_signals::{Effect, ReactiveSession};
use krui_ui_types::{
    CanvasDraw, ClickBinding, ClickTarget, InputTarget, PointerTarget, SvgVectorTemplate,
    WheelTarget,
};
use std::rc::Rc;

fn session_of(tree: &WebDomTree) -> ReactiveSession {
    tree.inner.borrow().session.clone()
}

fn wrap_notify(session: ReactiveSession, f: Rc<dyn Fn()>) -> DomEventCallback {
    DomEventCallback::Notify(Rc::new(move || {
        let cb = Rc::clone(&f);
        session.enter(move || cb());
    }))
}

fn wrap_text(session: ReactiveSession, f: Rc<dyn Fn(String)>) -> DomEventCallback {
    DomEventCallback::Text(Rc::new(move |s| {
        let cb = Rc::clone(&f);
        session.enter(move || cb(s));
    }))
}

impl WebDomTree {
    /// Attaches a click callback (and optional `can_execute` reactive `:disabled` guard).
    pub fn bind_click(&mut self, id: DomNodeId, binding: impl Into<ClickBinding>) {
        let binding = binding.into();
        if let Some(can_exec) = binding.can_execute {
            self.bind_disabled(id, move || !can_exec.get());
        }
        self.set_on_click(id, binding.action);
    }

    /// Attaches a `click` handler using [`ClickTarget`].
    pub fn set_on_click(&mut self, id: DomNodeId, handler: impl ClickTarget) {
        if let Some(can_exec) = handler.can_execute_signal() {
            self.bind_disabled(id, move || !can_exec.get());
        }
        let session = session_of(self);
        self.inner.borrow_mut().backend.add_event_listener(
            id,
            DomEventKind::Click,
            wrap_notify(session, handler.into_click_fn()),
        );
    }

    /// Attaches an IME-guarded `input` handler on `id`.
    pub fn set_on_input(&mut self, id: DomNodeId, handler: impl InputTarget) {
        let session = session_of(self);
        self.inner.borrow_mut().backend.add_event_listener(
            id,
            DomEventKind::Input,
            wrap_text(session, handler.into_input_fn()),
        );
    }

    /// Attaches a `change` handler on `id`.
    pub fn set_on_change(&mut self, id: DomNodeId, handler: impl InputTarget) {
        let session = session_of(self);
        self.inner.borrow_mut().backend.add_event_listener(
            id,
            DomEventKind::Change,
            wrap_text(session, handler.into_input_fn()),
        );
    }

    /// Attaches a `keydown` handler on `id`.
    pub fn set_on_keydown(&mut self, id: DomNodeId, handler: impl InputTarget) {
        let session = session_of(self);
        self.inner.borrow_mut().backend.add_event_listener(
            id,
            DomEventKind::KeyDown,
            wrap_text(session, handler.into_input_fn()),
        );
    }

    /// Attaches a `focus` handler on `id`.
    pub fn set_on_focus(&mut self, id: DomNodeId, handler: impl Fn() + 'static) {
        let session = session_of(self);
        self.inner.borrow_mut().backend.add_event_listener(
            id,
            DomEventKind::Focus,
            wrap_notify(session, Rc::new(handler)),
        );
    }

    /// Attaches a `blur` handler on `id`.
    pub fn set_on_blur(&mut self, id: DomNodeId, handler: impl Fn() + 'static) {
        let session = session_of(self);
        self.inner.borrow_mut().backend.add_event_listener(
            id,
            DomEventKind::Blur,
            wrap_notify(session, Rc::new(handler)),
        );
    }

    /// Attaches a `pointerdown` handler on `id` (with pointer capture for smooth drags).
    pub fn set_on_pointer_down(&mut self, id: DomNodeId, handler: impl PointerTarget) {
        self.add_pointer_listener(id, DomEventKind::PointerDown, handler);
    }

    /// Attaches a `pointerup` handler on `id`.
    pub fn set_on_pointer_up(&mut self, id: DomNodeId, handler: impl PointerTarget) {
        self.add_pointer_listener(id, DomEventKind::PointerUp, handler);
    }

    /// Attaches a `pointermove` handler on `id`.
    pub fn set_on_pointer_move(&mut self, id: DomNodeId, handler: impl PointerTarget) {
        self.add_pointer_listener(id, DomEventKind::PointerMove, handler);
    }

    /// Attaches a `contextmenu` handler on `id`.
    pub fn set_on_context_menu(&mut self, id: DomNodeId, handler: impl PointerTarget) {
        self.add_pointer_listener(id, DomEventKind::ContextMenu, handler);
    }

    fn add_pointer_listener(
        &mut self,
        id: DomNodeId,
        kind: DomEventKind,
        handler: impl PointerTarget,
    ) {
        let session = session_of(self);
        let cb = handler.into_pointer_fn();
        self.inner.borrow_mut().backend.add_event_listener(
            id,
            kind,
            DomEventCallback::Pointer(Rc::new(move |ev| {
                let f = Rc::clone(&cb);
                session.enter(move || f(ev));
            })),
        );
    }

    /// Attaches a non-passive `wheel` handler on `id` (`delta_y > 0` = scroll UP).
    pub fn set_on_wheel(&mut self, id: DomNodeId, handler: impl WheelTarget) {
        let session = session_of(self);
        let cb = handler.into_wheel_fn();
        self.inner.borrow_mut().backend.add_event_listener(
            id,
            DomEventKind::Wheel,
            DomEventCallback::Wheel(Rc::new(move |ev| {
                let f = Rc::clone(&cb);
                session.enter(move || f(ev))
            })),
        );
    }

    /// Attaches a `scroll` handler on `id`.
    pub fn set_on_scroll(&mut self, id: DomNodeId, handler: impl Fn() + 'static) {
        let session = session_of(self);
        self.inner.borrow_mut().backend.add_event_listener(
            id,
            DomEventKind::Scroll,
            wrap_notify(session, Rc::new(handler)),
        );
    }

    /// Attaches a `<form>` `submit` handler on `id` (calling `preventDefault`).
    pub fn set_on_submit(&mut self, id: DomNodeId, handler: impl Fn() + 'static) {
        let session = session_of(self);
        self.inner.borrow_mut().backend.add_event_listener(
            id,
            DomEventKind::Submit,
            wrap_notify(session, Rc::new(handler)),
        );
    }

    /// Attaches a `<form>` `reset` handler on `id`.
    pub fn set_on_reset(&mut self, id: DomNodeId, handler: impl Fn() + 'static) {
        let session = session_of(self);
        self.inner.borrow_mut().backend.add_event_listener(
            id,
            DomEventKind::Reset,
            wrap_notify(session, Rc::new(handler)),
        );
    }

    /// Populates `<svg>` element `id` with crisp browser vector primitives from `template`.
    pub fn set_node_svg_template(&mut self, id: DomNodeId, template: SvgVectorTemplate) {
        self.inner
            .borrow_mut()
            .backend
            .set_svg_template(id, &template);
    }

    /// Binds a reactive 2D [`CanvasDraw`] closure to `<canvas>` element `id`.
    pub fn set_canvas_draw(&mut self, id: DomNodeId, draw: impl Into<CanvasDraw>) {
        let draw = draw.into();
        self.inner.borrow_mut().backend.set_canvas_draw(id, draw);
        let tree = self.clone();
        let _ = Effect::new(move || {
            tree.request_canvas_redraw(id);
        });
    }

    /// Reactively binds dynamic `<img>` `src` attribute on `id`.
    pub fn bind_dynamic_image_src(&mut self, id: DomNodeId, src_fn: impl Fn() -> String + 'static) {
        self.bind_dynamic_attr(id, "src", move || Some(src_fn()));
    }

    /// Reactively binds dynamic `<img>` `src` resolved relative to `base_dir` on `id`.
    pub fn bind_dynamic_image_src_in_dir(
        &mut self,
        id: DomNodeId,
        base_dir: &'static str,
        src_fn: impl Fn() -> String + 'static,
    ) {
        self.bind_dynamic_attr(id, "src", move || {
            let raw = src_fn();
            if raw.starts_with("http://")
                || raw.starts_with("https://")
                || raw.starts_with("data:")
                || raw.starts_with("blob:")
                || raw.starts_with('/')
                || base_dir.is_empty()
            {
                Some(raw)
            } else {
                Some(format!("{}/{}", base_dir.trim_end_matches('/'), raw))
            }
        });
    }
}
