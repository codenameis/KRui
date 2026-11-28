//! Minimal on-demand `requestAnimationFrame` driver for `krui-web` (`0%` idle CPU).

use crate::WasmShowcaseHost;
use krui::{set_ui_waker, Effect, UiWaker};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

type RafClosureSlot = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;

thread_local! {
    static SHOWCASE_KEEPALIVE: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
    static SCHEDULE_RAF_HOOK: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

struct WasmRafWaker;

impl UiWaker for WasmRafWaker {
    fn wake(&self) {
        let _ = SCHEDULE_RAF_HOOK.try_with(|cell| {
            if let Some(cb) = cell.borrow().as_ref() {
                cb();
            }
        });
    }
}

/// Launches the browser native-DOM showcase loop without touching any full-screen canvas.
pub fn run_browser_showcase() {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("KRUI WASM PANIC: {info}").into());
    }));
    let host = Rc::new(RefCell::new(WasmShowcaseHost::new()));
    let win = web_sys::window().expect("browser window");
    let raf_scheduled = Rc::new(RefCell::new(false));
    let raf_cb: RafClosureSlot = Rc::new(RefCell::new(None));
    let schedule_raf = create_raf_scheduler(&win, &raf_scheduled, &raf_cb);

    SCHEDULE_RAF_HOOK.with(|cell| {
        *cell.borrow_mut() = Some(Rc::clone(&schedule_raf));
    });
    bind_navigation_and_spin_effects(&host, &win, &schedule_raf);
    install_raf_tick_closure(&host, &raf_scheduled, &raf_cb, &schedule_raf);

    if host.borrow().needs_spin_tick() {
        schedule_raf();
    }
    SHOWCASE_KEEPALIVE.with(|cell| {
        *cell.borrow_mut() = Some(Box::new((host, raf_cb)));
    });
}

fn create_raf_scheduler(
    win: &web_sys::Window,
    raf_scheduled: &Rc<RefCell<bool>>,
    raf_cb: &RafClosureSlot,
) -> Rc<dyn Fn()> {
    let win = win.clone();
    let flag = Rc::clone(raf_scheduled);
    let cb_slot = Rc::clone(raf_cb);
    Rc::new(move || {
        if *flag.borrow() {
            return;
        }
        if let Some(cb) = cb_slot.borrow().as_ref() {
            *flag.borrow_mut() = true;
            let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
        }
    })
}

fn bind_navigation_and_spin_effects(
    host: &Rc<RefCell<WasmShowcaseHost>>,
    win: &web_sys::Window,
    schedule_raf: &Rc<dyn Fn()>,
) {
    host.borrow().enter(|| {
        set_ui_waker(Some(Arc::new(WasmRafWaker)));
        let sec_sig = host.borrow().controller().active_section;
        let doc_sig = host.borrow().controller().active_doc_page;
        let sub_sig = host.borrow().controller().w4.studio.develop_subroute;
        let spin_sig = host.borrow().controller().w4.studio.auto_spin;
        let sched = Rc::clone(schedule_raf);
        let _ = Effect::new(move || {
            let sec = sec_sig.get();
            let sub = sub_sig.get();
            let spin = spin_sig.get();
            if sec == 4 && sub == 2 && spin {
                sched();
            }
        });
        let doc_el_win = win.clone();
        let _ = Effect::new(move || {
            let _ = sec_sig.get();
            let _ = doc_sig.get();
            let _ = sub_sig.get();
            if let Some(vp) = doc_el_win
                .document()
                .and_then(|d| d.get_element_by_id("m3-doc-viewport"))
            {
                vp.set_scroll_top(0);
            }
        });
    });
}

fn install_raf_tick_closure(
    host: &Rc<RefCell<WasmShowcaseHost>>,
    raf_scheduled: &Rc<RefCell<bool>>,
    raf_cb: &RafClosureSlot,
    schedule_raf: &Rc<dyn Fn()>,
) {
    let last_ms = Rc::new(RefCell::new(0.0_f64));
    let host_cb = Rc::clone(host);
    let flag_cb = Rc::clone(raf_scheduled);
    let schedule_loop = Rc::clone(schedule_raf);
    *raf_cb.borrow_mut() = Some(Closure::wrap(Box::new(move |now_ms: f64| {
        *flag_cb.borrow_mut() = false;
        let prev = {
            let mut m = last_ms.borrow_mut();
            let p = *m;
            *m = now_ms;
            p
        };
        let dt = if prev > 0.0 {
            ((now_ms - prev) as f32 / 1000.0).clamp(0.001, 0.05)
        } else {
            1.0 / 60.0
        };
        if host_cb.borrow_mut().tick(dt) {
            schedule_loop();
        }
    }) as Box<dyn FnMut(f64)>));
}
