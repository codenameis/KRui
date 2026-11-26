//! Rich viewport event payload types and target traits (`PointerButton`, `PointerEvent`,
//! `WheelEvent`, `PointerHandler`, `WheelHandler`, `PointerTarget`, `WheelTarget`).

use crate::view_tree::key_routing::{KeydownOutcome, Mods};
use std::cell::Cell;
use std::rc::Rc;

thread_local! {
    static PREVENT_DEFAULT_WHEEL: Cell<bool> = const { Cell::new(false) };
}

/// Marks the currently executing `onwheel` handler as handled so ancestor containers do not scroll.
pub fn prevent_default_wheel() {
    PREVENT_DEFAULT_WHEEL.with(|flag| flag.set(true));
}

pub(crate) fn clear_prevent_default_wheel() {
    PREVENT_DEFAULT_WHEEL.with(|flag| flag.set(false));
}

pub(crate) fn take_prevent_default_wheel() -> bool {
    PREVENT_DEFAULT_WHEEL.with(|flag| flag.replace(false))
}

/// Mouse button identity reported in [`PointerEvent::button`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PointerButton {
    /// Left / primary mouse button (`code == 0`).
    #[default]
    Primary,
    /// Middle / wheel mouse button (`code == 1`).
    Middle,
    /// Right / context-menu mouse button (`code == 2`).
    Secondary,
    /// Browser back / thumb X1 button (`code == 3`).
    Back,
    /// Browser forward / thumb X2 button (`code == 4`).
    Forward,
}

impl PointerButton {
    /// Converts a wire/platform button index (`0=Left, 1=Middle, 2=Right, 3=Back, 4=Forward`).
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Primary),
            1 => Some(Self::Middle),
            2 => Some(Self::Secondary),
            3 => Some(Self::Back),
            4 => Some(Self::Forward),
            _ => None,
        }
    }

    /// Returns the DOM `PointerEvent.buttons` bitmask flag for this button.
    #[must_use]
    pub const fn mask(self) -> u8 {
        match self {
            Self::Primary => 1 << 0,
            Self::Secondary => 1 << 1,
            Self::Middle => 1 << 2,
            Self::Back => 1 << 3,
            Self::Forward => 1 << 4,
        }
    }
}

/// Detailed pointer payload passed to `PointerHandler` callbacks (`onpointerdown`/`up`/`move`/`oncontextmenu`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PointerEvent {
    /// Window/root logical X coordinate.
    pub x: f32,
    /// Window/root logical Y coordinate.
    pub y: f32,
    /// Node-local X coordinate relative to the target element's top-left corner (`0` at left edge).
    pub local_x: f32,
    /// Node-local Y coordinate relative to the target element's top-left corner (`0` at top edge).
    pub local_y: f32,
    /// Relative horizontal motion delta in logical pixels (or raw delta while pointer-locked).
    pub delta_x: f32,
    /// Relative vertical motion delta in logical pixels (or raw delta while pointer-locked).
    pub delta_y: f32,
    /// Button associated with press/release transitions.
    pub button: PointerButton,
    /// Bitmask of currently depressed mouse buttons (`PointerButton::mask`).
    pub buttons: u8,
    /// Multi-click count (`1` single, `2` double, `3` triple).
    pub click_count: u8,
    /// Active keyboard modifier keys (`Ctrl`/`Alt`/`Shift`/`Super`).
    pub mods: Mods,
    /// `true` when delivered while OS pointer lock is active on a target viewport.
    pub pointer_locked: bool,
}

/// Detailed wheel payload passed to `WheelHandler` callbacks (`onwheel`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct WheelEvent {
    /// Window/root logical X coordinate.
    pub x: f32,
    /// Window/root logical Y coordinate.
    pub y: f32,
    /// Node-local X coordinate relative to the target element's top-left corner.
    pub local_x: f32,
    /// Node-local Y coordinate relative to the target element's top-left corner.
    pub local_y: f32,
    /// Horizontal scroll delta in logical pixels.
    pub delta_x: f32,
    /// Vertical scroll delta in logical pixels (positive = wheel up / scroll up).
    pub delta_y: f32,
    /// Active keyboard modifier keys (`Ctrl`/`Alt`/`Shift`/`Super`).
    pub mods: Mods,
}

/// Event callback wrapper for `.krui` attributes (`onpointerdown`, `onpointerup`, `onpointermove`, `oncontextmenu`).
#[derive(Clone)]
pub struct PointerHandler(pub Rc<dyn Fn(PointerEvent)>);

impl PointerHandler {
    /// Wraps `f: impl Fn(PointerEvent) + 'static` into a cloneable `.krui` field handler.
    pub fn new(f: impl Fn(PointerEvent) + 'static) -> Self {
        Self(Rc::new(f))
    }

    /// Invokes the underlying handler with `ev`.
    pub fn call(&self, ev: PointerEvent) {
        (self.0)(ev);
    }
}

/// Event callback wrapper for `.krui` `onwheel="{vm.on_wheel}"` returning `bool` (`true` prevents default scroll).
#[derive(Clone)]
pub struct WheelHandler(pub Rc<dyn Fn(WheelEvent) -> bool>);

impl WheelHandler {
    /// Wraps `f: impl Fn(WheelEvent) -> R` (`()` or `bool` via [`KeydownOutcome`]).
    pub fn new<F, R>(f: F) -> Self
    where
        F: Fn(WheelEvent) -> R + 'static,
        R: KeydownOutcome + 'static,
    {
        Self(Rc::new(move |ev: WheelEvent| f(ev).into_prevent_default()))
    }

    /// Invokes the underlying handler and returns `true` when default ancestor scrolling should be suppressed.
    pub fn call(&self, ev: WheelEvent) -> bool {
        (self.0)(ev)
    }
}

/// Pattern-neutral target accepted by `ViewTree::set_on_pointer_*` and `set_on_context_menu`.
pub trait PointerTarget {
    /// Converts `self` into an `Rc<dyn Fn(PointerEvent)>`.
    fn into_pointer_fn(self) -> Rc<dyn Fn(PointerEvent)>;
}

impl PointerTarget for PointerHandler {
    fn into_pointer_fn(self) -> Rc<dyn Fn(PointerEvent)> {
        self.0
    }
}

impl PointerTarget for Rc<dyn Fn(PointerEvent)> {
    fn into_pointer_fn(self) -> Rc<dyn Fn(PointerEvent)> {
        self
    }
}

impl PointerTarget for Rc<dyn Fn()> {
    fn into_pointer_fn(self) -> Rc<dyn Fn(PointerEvent)> {
        Rc::new(move |_ev| (self)())
    }
}

#[cfg(feature = "preview")]
impl PointerTarget for crate::registry::DynCommand {
    fn into_pointer_fn(self) -> Rc<dyn Fn(PointerEvent)> {
        let cb = self.as_rc_fn();
        Rc::new(move |_ev| cb())
    }
}

impl<F> PointerTarget for F
where
    F: Fn() + 'static,
{
    fn into_pointer_fn(self) -> Rc<dyn Fn(PointerEvent)> {
        Rc::new(move |_ev| (self)())
    }
}

/// Pattern-neutral target accepted by `ViewTree::set_on_wheel`.
pub trait WheelTarget {
    /// Converts `self` into an `Rc<dyn Fn(WheelEvent) -> bool>`.
    fn into_wheel_fn(self) -> Rc<dyn Fn(WheelEvent) -> bool>;
}

impl WheelTarget for WheelHandler {
    fn into_wheel_fn(self) -> Rc<dyn Fn(WheelEvent) -> bool> {
        self.0
    }
}

impl WheelTarget for Rc<dyn Fn(WheelEvent) -> bool> {
    fn into_wheel_fn(self) -> Rc<dyn Fn(WheelEvent) -> bool> {
        self
    }
}

impl WheelTarget for Rc<dyn Fn()> {
    fn into_wheel_fn(self) -> Rc<dyn Fn(WheelEvent) -> bool> {
        Rc::new(move |_ev| {
            (self)();
            false
        })
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

impl<F> WheelTarget for F
where
    F: Fn() + 'static,
{
    fn into_wheel_fn(self) -> Rc<dyn Fn(WheelEvent) -> bool> {
        Rc::new(move |_ev| {
            (self)();
            false
        })
    }
}
