//! Rich viewport event payload types and target traits (`PointerButton`, `PointerEvent`,
//! `WheelEvent`, `PointerHandler`, `WheelHandler`, `PointerTarget`, `WheelTarget`).

pub use krui_ui_types::events::{
    clear_prevent_default_wheel, prevent_default_wheel, take_prevent_default_wheel, PointerButton,
    PointerEvent, PointerHandler, PointerTarget, WheelEvent, WheelHandler, WheelTarget,
};
#[cfg(feature = "preview")]
use std::rc::Rc;

#[cfg(feature = "preview")]
impl PointerTarget for crate::registry::DynCommand {
    fn into_pointer_fn(self) -> Rc<dyn Fn(PointerEvent)> {
        let cb = self.as_rc_fn();
        Rc::new(move |_ev| cb())
    }
}

#[cfg(feature = "preview")]
impl WheelTarget for crate::registry::DynCommand {
    fn into_wheel_fn(self) -> Rc<dyn Fn(WheelEvent) -> bool> {
        let cb = self.as_rc_fn();
        Rc::new(move |_ev| {
            cb();
            false
        })
    }
}
