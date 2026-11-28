//! Disposable browser DOM event listeners with Korean/CJK IME guard, wheel negation,
//! and pointer capture for drag interactions (`canvas`, sliders, splitters, 3D viewports).

use crate::backend::{DomEventCallback, DomEventKind, ListenerId};
use krui_ui_types::{
    clear_prevent_default_key, clear_prevent_default_wheel, take_prevent_default_key,
    take_prevent_default_wheel, Mods, PointerButton, PointerEvent, WheelEvent,
};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

pub(crate) struct RegisteredListener {
    pub(crate) id: ListenerId,
    pub(crate) target: web_sys::EventTarget,
    pub(crate) event_name: &'static str,
    pub(crate) listener: ListenerClosure,
    pub(crate) extra_closures: Vec<(&'static str, ListenerClosure)>,
}

pub(crate) enum ListenerClosure {
    Event(Closure<dyn FnMut(web_sys::Event)>),
    Pointer(Closure<dyn FnMut(web_sys::PointerEvent)>),
    Wheel(Closure<dyn FnMut(web_sys::WheelEvent)>),
    Keyboard(Closure<dyn FnMut(web_sys::KeyboardEvent)>),
    Composition(Closure<dyn FnMut(web_sys::CompositionEvent)>),
}

impl ListenerClosure {
    pub(crate) fn as_js_fn(&self) -> &js_sys::Function {
        match self {
            Self::Event(c) => c.as_ref().unchecked_ref(),
            Self::Pointer(c) => c.as_ref().unchecked_ref(),
            Self::Wheel(c) => c.as_ref().unchecked_ref(),
            Self::Keyboard(c) => c.as_ref().unchecked_ref(),
            Self::Composition(c) => c.as_ref().unchecked_ref(),
        }
    }
}

impl Drop for RegisteredListener {
    fn drop(&mut self) {
        let _ = self
            .target
            .remove_event_listener_with_callback(self.event_name, self.listener.as_js_fn());
        for (name, extra) in &self.extra_closures {
            let _ = self
                .target
                .remove_event_listener_with_callback(name, extra.as_js_fn());
        }
    }
}

fn read_element_text_value(node: &web_sys::Node) -> String {
    if let Some(el) = node.dyn_ref::<web_sys::HtmlInputElement>() {
        el.value()
    } else if let Some(el) = node.dyn_ref::<web_sys::HtmlTextAreaElement>() {
        el.value()
    } else if let Some(el) = node.dyn_ref::<web_sys::HtmlSelectElement>() {
        el.value()
    } else {
        node.text_content().unwrap_or_default()
    }
}

fn read_element_checked(node: &web_sys::Node) -> bool {
    node.dyn_ref::<web_sys::HtmlInputElement>()
        .map(|el| el.checked())
        .unwrap_or(false)
}

fn read_local_xy(target: &web_sys::Node, client_x: f32, client_y: f32) -> (f32, f32) {
    let Some(el) = target.dyn_ref::<web_sys::Element>() else {
        return (client_x, client_y);
    };
    let rect = el.get_bounding_client_rect();
    (client_x - rect.left() as f32, client_y - rect.top() as f32)
}

fn mods_from_pointer(ev: &web_sys::PointerEvent) -> Mods {
    Mods {
        ctrl: ev.ctrl_key(),
        alt: ev.alt_key(),
        shift: ev.shift_key(),
        super_key: ev.meta_key(),
    }
}

fn mods_from_mouse(ev: &web_sys::MouseEvent) -> Mods {
    Mods {
        ctrl: ev.ctrl_key(),
        alt: ev.alt_key(),
        shift: ev.shift_key(),
        super_key: ev.meta_key(),
    }
}

fn to_pointer_event(target: &web_sys::Node, ev: &web_sys::PointerEvent) -> PointerEvent {
    let x = ev.client_x() as f32;
    let y = ev.client_y() as f32;
    let (local_x, local_y) = read_local_xy(target, x, y);
    let button = PointerButton::from_code(ev.button().clamp(0, 4) as u8).unwrap_or_default();
    PointerEvent {
        x,
        y,
        local_x,
        local_y,
        delta_x: ev.movement_x() as f32,
        delta_y: ev.movement_y() as f32,
        button,
        buttons: ev.buttons() as u8,
        click_count: ev.detail().clamp(1, 255) as u8,
        mods: mods_from_pointer(ev),
        pointer_locked: false,
    }
}

/// Attaches a DOM event listener on `node` and returns a disposable [`RegisteredListener`].
pub(crate) fn attach_listener(
    id: ListenerId,
    node: &web_sys::Node,
    is_composing: Rc<RefCell<bool>>,
    kind: DomEventKind,
    handler: DomEventCallback,
) -> Option<RegisteredListener> {
    let target: web_sys::EventTarget = node.clone().dyn_into().ok()?;
    match (kind, handler) {
        (DomEventKind::Input, DomEventCallback::Text(cb)) => {
            attach_ime_input(id, node, target, is_composing, cb)
        }
        (DomEventKind::Wheel, DomEventCallback::Wheel(cb)) => attach_wheel(id, node, target, cb),
        (DomEventKind::KeyDown, DomEventCallback::Text(cb)) => attach_keydown(id, target, cb),
        (
            DomEventKind::PointerDown
            | DomEventKind::PointerUp
            | DomEventKind::PointerMove
            | DomEventKind::ContextMenu,
            DomEventCallback::Pointer(cb),
        ) => attach_pointer(id, node, target, kind, cb),
        (DomEventKind::Change, DomEventCallback::Bool(cb)) => {
            attach_checked_change(id, node, target, cb)
        }
        (DomEventKind::Change, DomEventCallback::Text(cb)) => {
            attach_value_change(id, node, target, cb)
        }
        (
            DomEventKind::Click
            | DomEventKind::Focus
            | DomEventKind::Blur
            | DomEventKind::Scroll
            | DomEventKind::Submit
            | DomEventKind::Reset,
            DomEventCallback::Notify(cb),
        ) => attach_notify(id, target, kind, cb),
        _ => None,
    }
}

fn attach_ime_input(
    id: ListenerId,
    node: &web_sys::Node,
    target: web_sys::EventTarget,
    is_composing: Rc<RefCell<bool>>,
    cb: Rc<dyn Fn(String)>,
) -> Option<RegisteredListener> {
    let node_ref = node.clone();
    let comp_flag = Rc::clone(&is_composing);
    let cb_input = Rc::clone(&cb);
    let on_input = Closure::wrap(Box::new(move |ev: web_sys::Event| {
        if *comp_flag.borrow() {
            return;
        }
        if let Some(ie) = ev.dyn_ref::<web_sys::InputEvent>() {
            if ie.is_composing() {
                return;
            }
        }
        cb_input(read_element_text_value(&node_ref));
    }) as Box<dyn FnMut(web_sys::Event)>);

    let comp_start_flag = Rc::clone(&is_composing);
    let on_comp_start = Closure::wrap(Box::new(move |_: web_sys::CompositionEvent| {
        *comp_start_flag.borrow_mut() = true;
    }) as Box<dyn FnMut(web_sys::CompositionEvent)>);

    let node_end = node.clone();
    let comp_end_flag = is_composing;
    let on_comp_end = Closure::wrap(Box::new(move |_: web_sys::CompositionEvent| {
        *comp_end_flag.borrow_mut() = false;
        cb(read_element_text_value(&node_end));
    }) as Box<dyn FnMut(web_sys::CompositionEvent)>);

    let _ = target.add_event_listener_with_callback("input", on_input.as_ref().unchecked_ref());
    let _ = target.add_event_listener_with_callback(
        "compositionstart",
        on_comp_start.as_ref().unchecked_ref(),
    );
    let _ = target
        .add_event_listener_with_callback("compositionend", on_comp_end.as_ref().unchecked_ref());

    Some(RegisteredListener {
        id,
        target,
        event_name: "input",
        listener: ListenerClosure::Event(on_input),
        extra_closures: vec![
            (
                "compositionstart",
                ListenerClosure::Composition(on_comp_start),
            ),
            ("compositionend", ListenerClosure::Composition(on_comp_end)),
        ],
    })
}

fn attach_wheel(
    id: ListenerId,
    node: &web_sys::Node,
    target: web_sys::EventTarget,
    cb: Rc<dyn Fn(WheelEvent) -> bool>,
) -> Option<RegisteredListener> {
    let node_ref = node.clone();
    let closure = Closure::wrap(Box::new(move |ev: web_sys::WheelEvent| {
        clear_prevent_default_wheel();
        let x = ev.client_x() as f32;
        let y = ev.client_y() as f32;
        let (local_x, local_y) = read_local_xy(&node_ref, x, y);
        // Browser WheelEvent: deltaY < 0 is scroll UP; KRui WheelEvent: delta_y > 0 is scroll UP.
        let payload = WheelEvent {
            x,
            y,
            local_x,
            local_y,
            delta_x: ev.delta_x() as f32,
            delta_y: -(ev.delta_y() as f32),
            mods: mods_from_mouse(ev.unchecked_ref()),
        };
        let handled = cb(payload) || take_prevent_default_wheel();
        if handled {
            ev.prevent_default();
        }
    }) as Box<dyn FnMut(web_sys::WheelEvent)>);

    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_passive(false);
    let _ = target.add_event_listener_with_callback_and_add_event_listener_options(
        "wheel",
        closure.as_ref().unchecked_ref(),
        &opts,
    );
    Some(RegisteredListener {
        id,
        target,
        event_name: "wheel",
        listener: ListenerClosure::Wheel(closure),
        extra_closures: Vec::new(),
    })
}

fn attach_keydown(
    id: ListenerId,
    target: web_sys::EventTarget,
    cb: Rc<dyn Fn(String)>,
) -> Option<RegisteredListener> {
    let closure = Closure::wrap(Box::new(move |ev: web_sys::KeyboardEvent| {
        if ev.is_composing() {
            return;
        }
        clear_prevent_default_key();
        cb(ev.key());
        if take_prevent_default_key() {
            ev.prevent_default();
        }
    }) as Box<dyn FnMut(web_sys::KeyboardEvent)>);
    let _ = target.add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref());
    Some(RegisteredListener {
        id,
        target,
        event_name: "keydown",
        listener: ListenerClosure::Keyboard(closure),
        extra_closures: Vec::new(),
    })
}

fn attach_pointer(
    id: ListenerId,
    node: &web_sys::Node,
    target: web_sys::EventTarget,
    kind: DomEventKind,
    cb: Rc<dyn Fn(PointerEvent)>,
) -> Option<RegisteredListener> {
    let event_name = match kind {
        DomEventKind::PointerDown => "pointerdown",
        DomEventKind::PointerUp => "pointerup",
        DomEventKind::PointerMove => "pointermove",
        DomEventKind::ContextMenu => "contextmenu",
        _ => return None,
    };
    let node_ref = node.clone();
    let closure = Closure::wrap(Box::new(move |ev: web_sys::PointerEvent| {
        if kind == DomEventKind::PointerDown {
            if let Some(el) = node_ref.dyn_ref::<web_sys::Element>() {
                let _ = el.set_pointer_capture(ev.pointer_id());
            }
        } else if kind == DomEventKind::ContextMenu {
            ev.prevent_default();
        }
        cb(to_pointer_event(&node_ref, &ev));
    }) as Box<dyn FnMut(web_sys::PointerEvent)>);
    let _ = target.add_event_listener_with_callback(event_name, closure.as_ref().unchecked_ref());
    Some(RegisteredListener {
        id,
        target,
        event_name,
        listener: ListenerClosure::Pointer(closure),
        extra_closures: Vec::new(),
    })
}

fn attach_checked_change(
    id: ListenerId,
    node: &web_sys::Node,
    target: web_sys::EventTarget,
    cb: Rc<dyn Fn(bool)>,
) -> Option<RegisteredListener> {
    let node_ref = node.clone();
    let closure = Closure::wrap(Box::new(move |_: web_sys::Event| {
        cb(read_element_checked(&node_ref));
    }) as Box<dyn FnMut(web_sys::Event)>);
    let _ = target.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
    Some(RegisteredListener {
        id,
        target,
        event_name: "change",
        listener: ListenerClosure::Event(closure),
        extra_closures: Vec::new(),
    })
}

fn attach_value_change(
    id: ListenerId,
    node: &web_sys::Node,
    target: web_sys::EventTarget,
    cb: Rc<dyn Fn(String)>,
) -> Option<RegisteredListener> {
    let node_ref = node.clone();
    let closure = Closure::wrap(Box::new(move |_: web_sys::Event| {
        cb(read_element_text_value(&node_ref));
    }) as Box<dyn FnMut(web_sys::Event)>);
    let _ = target.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
    Some(RegisteredListener {
        id,
        target,
        event_name: "change",
        listener: ListenerClosure::Event(closure),
        extra_closures: Vec::new(),
    })
}

fn attach_notify(
    id: ListenerId,
    target: web_sys::EventTarget,
    kind: DomEventKind,
    cb: Rc<dyn Fn()>,
) -> Option<RegisteredListener> {
    let event_name = match kind {
        DomEventKind::Click => "click",
        DomEventKind::Focus => "focus",
        DomEventKind::Blur => "blur",
        DomEventKind::Scroll => "scroll",
        DomEventKind::Submit => "submit",
        DomEventKind::Reset => "reset",
        _ => return None,
    };
    let prevent = matches!(kind, DomEventKind::Submit | DomEventKind::Reset);
    let closure = Closure::wrap(Box::new(move |ev: web_sys::Event| {
        if prevent {
            ev.prevent_default();
        }
        cb();
    }) as Box<dyn FnMut(web_sys::Event)>);
    let _ = target.add_event_listener_with_callback(event_name, closure.as_ref().unchecked_ref());
    Some(RegisteredListener {
        id,
        target,
        event_name,
        listener: ListenerClosure::Event(closure),
        extra_closures: Vec::new(),
    })
}

/// Attaches a delegated `pointerdown` ripple origin handler (`--krui-pointer-x/y`) on root.
pub(crate) fn attach_delegated_ripple_listener(
    root: &web_sys::Element,
) -> Option<RegisteredListener> {
    let target: web_sys::EventTarget = root.clone().dyn_into().ok()?;
    let closure = Closure::wrap(Box::new(move |ev: web_sys::PointerEvent| {
        let Some(raw_target) = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        let host = raw_target
            .closest("button, a, [role='button']")
            .ok()
            .flatten()
            .unwrap_or(raw_target);
        let rect = host.get_bounding_client_rect();
        let lx = ev.client_x() as f32 - rect.left() as f32;
        let ly = ev.client_y() as f32 - rect.top() as f32;
        if let Some(html) = host.dyn_ref::<web_sys::HtmlElement>() {
            let style = html.style();
            let _ = style.set_property("--krui-pointer-x", &format!("{lx:.2}px"));
            let _ = style.set_property("--krui-pointer-y", &format!("{ly:.2}px"));
        }
    }) as Box<dyn FnMut(web_sys::PointerEvent)>);
    let _ =
        target.add_event_listener_with_callback("pointerdown", closure.as_ref().unchecked_ref());
    Some(RegisteredListener {
        id: ListenerId(0),
        target,
        event_name: "pointerdown",
        listener: ListenerClosure::Pointer(closure),
        extra_closures: Vec::new(),
    })
}
