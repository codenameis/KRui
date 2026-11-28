//! Interactive HTML5 `<canvas>` 2D host for `krui-showcase-wasm` running in `wasm32-unknown-unknown`.

use super::{WasmShowcaseHost, FRAME_DT_SECONDS};
use krui::{Mods, PointerButton};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{
    CanvasRenderingContext2d, HtmlCanvasElement, ImageData, KeyboardEvent, MouseEvent, WheelEvent,
    Window,
};

const CANVAS_ID: &str = "krui-canvas";
const MAX_DPR: f64 = 2.0;
const MIN_DT_SECONDS: f32 = 1.0 / 240.0;
const MAX_DT_SECONDS: f32 = 0.05;

struct BrowserState {
    host: WasmShowcaseHost,
    window: Window,
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    rgba_straight: Vec<u8>,
    last_time_ms: Option<f64>,
    force_redraw: bool,
    raf_pending: bool,
    last_cursor: (f32, f32),
}

impl BrowserState {
    fn sync_canvas_size(&mut self) {
        let dpr = self.window.device_pixel_ratio().clamp(1.0, MAX_DPR);
        let lw = self
            .window
            .inner_width()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(1280.0)
            .max(1.0);
        let lh = self
            .window
            .inner_height()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(840.0)
            .max(1.0);
        let phys_w = (lw * dpr).round().max(1.0) as u32;
        let phys_h = (lh * dpr).round().max(1.0) as u32;
        if self.canvas.width() != phys_w || self.canvas.height() != phys_h {
            self.canvas.set_width(phys_w);
            self.canvas.set_height(phys_h);
        }
        self.host.resize(lw.round() as u32, lh.round() as u32);
        self.host.set_scale_factor(dpr as f32);
        self.force_redraw = true;
    }

    fn render_and_present(&mut self, now_ms: f64) -> bool {
        let dt = self
            .last_time_ms
            .map(|prev| ((now_ms - prev) as f32 / 1000.0).clamp(MIN_DT_SECONDS, MAX_DT_SECONDS))
            .unwrap_or(FRAME_DT_SECONDS);
        self.last_time_ms = Some(now_ms);
        if self.force_redraw || self.host.needs_frame() {
            self.force_redraw = false;
            let _ = self.host.step_frame(dt);
            if let Some((pm_rgba, pw, ph)) = self.host.framebuffer_rgba_and_size() {
                unpremultiply_rgba(pm_rgba, &mut self.rgba_straight);
                if let Ok(img) = ImageData::new_with_u8_clamped_array_and_sh(
                    Clamped(&self.rgba_straight),
                    pw,
                    ph,
                ) {
                    let _ = self.ctx.put_image_data(&img, 0.0, 0.0);
                }
            }
        }
        self.force_redraw || self.host.needs_frame()
    }
}

fn unpremultiply_rgba(src: &[u8], dst: &mut Vec<u8>) {
    dst.resize(src.len(), 0);
    for (s, d) in src
        .as_chunks::<4>()
        .0
        .iter()
        .zip(dst.as_chunks_mut::<4>().0.iter_mut())
    {
        let a = s[3];
        if a == 255 {
            d.copy_from_slice(s);
        } else if a == 0 {
            d.copy_from_slice(&[0, 0, 0, 0]);
        } else {
            let a32 = a as u32;
            d[0] = (((s[0] as u32 * 255) + (a32 >> 1)) / a32).min(255) as u8;
            d[1] = (((s[1] as u32 * 255) + (a32 >> 1)) / a32).min(255) as u8;
            d[2] = (((s[2] as u32 * 255) + (a32 >> 1)) / a32).min(255) as u8;
            d[3] = a;
        }
    }
}

fn canvas_coords(canvas: &HtmlCanvasElement, e: &MouseEvent) -> (f32, f32) {
    let rect = canvas.get_bounding_client_rect();
    (
        (e.client_x() as f64 - rect.left()) as f32,
        (e.client_y() as f64 - rect.top()) as f32,
    )
}

fn mouse_mods(e: &MouseEvent) -> Mods {
    Mods {
        ctrl: e.ctrl_key(),
        alt: e.alt_key(),
        shift: e.shift_key(),
        super_key: e.meta_key(),
    }
}

fn key_mods(e: &KeyboardEvent) -> Mods {
    Mods {
        ctrl: e.ctrl_key(),
        alt: e.alt_key(),
        shift: e.shift_key(),
        super_key: e.meta_key(),
    }
}

fn pointer_button(button_code: i16) -> PointerButton {
    match button_code {
        1 => PointerButton::Middle,
        2 => PointerButton::Secondary,
        3 => PointerButton::Back,
        4 => PointerButton::Forward,
        _ => PointerButton::Primary,
    }
}

fn schedule_frame(state_rc: &Rc<RefCell<BrowserState>>) {
    let win = {
        let mut st = state_rc.borrow_mut();
        if st.raf_pending {
            return;
        }
        st.raf_pending = true;
        st.window.clone()
    };
    let weak = Rc::downgrade(state_rc);
    let cb = Closure::once_into_js(move |now_ms: f64| {
        let Some(strong) = weak.upgrade() else {
            return;
        };
        let keep_running = {
            let mut st = strong.borrow_mut();
            st.raf_pending = false;
            st.render_and_present(now_ms)
        };
        if keep_running {
            schedule_frame(&strong);
        }
    });
    let _ = win.request_animation_frame(cb.unchecked_ref());
}

fn register_canvas_listener<E>(
    target: &web_sys::EventTarget,
    event_name: &str,
    state: &Rc<RefCell<BrowserState>>,
    handler: impl Fn(&mut BrowserState, E) -> bool + 'static,
) where
    E: wasm_bindgen::convert::FromWasmAbi + 'static,
{
    let strong = Rc::clone(state);
    let closure = Closure::<dyn FnMut(E)>::wrap(Box::new(move |ev: E| {
        let needs = {
            let mut st = strong.borrow_mut();
            handler(&mut st, ev)
        };
        if needs {
            schedule_frame(&strong);
        }
    }));
    let _ = target.add_event_listener_with_callback(event_name, closure.as_ref().unchecked_ref());
    closure.forget();
}

fn wire_pointer_events(state: &Rc<RefCell<BrowserState>>, canvas: &HtmlCanvasElement) {
    let target: &web_sys::EventTarget = canvas.as_ref();
    register_canvas_listener::<MouseEvent>(target, "mousemove", state, |st, e| {
        let (x, y) = canvas_coords(&st.canvas, &e);
        st.last_cursor = (x, y);
        let moved = st.host.dispatch_pointer_move(x, y, mouse_mods(&e));
        moved || st.host.needs_frame()
    });
    register_canvas_listener::<MouseEvent>(target, "mousedown", state, |st, e| {
        let (x, y) = canvas_coords(&st.canvas, &e);
        st.last_cursor = (x, y);
        let handled = st.host.dispatch_pointer_button(
            x,
            y,
            pointer_button(e.button()),
            true,
            mouse_mods(&e),
            e.time_stamp() as u64,
        );
        handled || st.host.needs_frame()
    });
    register_canvas_listener::<MouseEvent>(target, "mouseup", state, |st, e| {
        let (x, y) = canvas_coords(&st.canvas, &e);
        st.last_cursor = (x, y);
        let handled = st.host.dispatch_pointer_button(
            x,
            y,
            pointer_button(e.button()),
            false,
            mouse_mods(&e),
            e.time_stamp() as u64,
        );
        handled || st.host.needs_frame()
    });
    register_canvas_listener::<MouseEvent>(target, "mouseleave", state, |st, _| {
        let left = st.host.dispatch_pointer_leave();
        left || st.host.needs_frame()
    });
    register_canvas_listener::<WheelEvent>(target, "wheel", state, |st, e| {
        e.prevent_default();
        let (x, y) = canvas_coords(&st.canvas, e.unchecked_ref::<MouseEvent>());
        st.host.dispatch_wheel(
            x,
            y,
            -(e.delta_x() as f32),
            -(e.delta_y() as f32),
            mouse_mods(e.unchecked_ref::<MouseEvent>()),
        );
        st.force_redraw = true;
        true
    });
}

fn wire_window_events(state: &Rc<RefCell<BrowserState>>, win: &Window) {
    let target: &web_sys::EventTarget = win.as_ref();
    register_canvas_listener::<web_sys::Event>(target, "resize", state, |st, _| {
        st.sync_canvas_size();
        true
    });
    register_canvas_listener::<KeyboardEvent>(target, "keydown", state, |st, e| {
        let key = e.key();
        let mods = key_mods(&e);
        let handled = st.host.dispatch_key(&key, mods);
        if matches!(key.as_str(), "Tab" | "F5" | "F6") || (handled && key == " ") {
            e.prevent_default();
        }
        handled || st.host.needs_frame()
    });
}

/// Mounts the KRui Component Gallery showcase onto `#krui-canvas` and starts the browser loop.
pub(super) fn launch_browser_showcase() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(doc) = window.document() else {
        return;
    };
    let Some(el) = doc.get_element_by_id(CANVAS_ID) else {
        return;
    };
    let Ok(canvas) = el.dyn_into::<HtmlCanvasElement>() else {
        return;
    };
    let Ok(Some(raw_ctx)) = canvas.get_context("2d") else {
        return;
    };
    let Ok(ctx) = raw_ctx.dyn_into::<CanvasRenderingContext2d>() else {
        return;
    };
    let mut state = BrowserState {
        host: WasmShowcaseHost::new(),
        window: window.clone(),
        canvas: canvas.clone(),
        ctx,
        rgba_straight: Vec::new(),
        last_time_ms: None,
        force_redraw: true,
        raf_pending: false,
        last_cursor: (0.0, 0.0),
    };
    state.sync_canvas_size();
    let now_ms = window.performance().map_or(0.0, |p| p.now());
    let keep = state.render_and_present(now_ms);
    let state_rc = Rc::new(RefCell::new(state));
    wire_pointer_events(&state_rc, &canvas);
    wire_window_events(&state_rc, &window);
    if keep {
        schedule_frame(&state_rc);
    }
}
