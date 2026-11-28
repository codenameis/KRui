//! Pattern-neutral UI event payloads, handler wrappers, and form control kind enums
//! shared across `krui-core`, `krui-web`, and `krui-patterns`.

use krui_signals::Computed;
use std::cell::Cell;
use std::rc::Rc;

thread_local! {
    static PREVENT_DEFAULT_KEY: Cell<bool> = const { Cell::new(false) };
    static PREVENT_DEFAULT_WHEEL: Cell<bool> = const { Cell::new(false) };
}

/// Marks the currently executing `onkeydown` event as handled so KRui skips its default key action.
pub fn prevent_default_key() {
    PREVENT_DEFAULT_KEY.with(|flag| flag.set(true));
}

/// Clears the thread-local `prevent_default_key` flag before dispatching a keydown callback.
pub fn clear_prevent_default_key() {
    PREVENT_DEFAULT_KEY.with(|flag| flag.set(false));
}

/// Reads and clears the thread-local `prevent_default_key` flag after dispatching a keydown callback.
pub fn take_prevent_default_key() -> bool {
    PREVENT_DEFAULT_KEY.with(|flag| flag.replace(false))
}

/// Marks the currently executing `onwheel` handler as handled so ancestor containers do not scroll.
pub fn prevent_default_wheel() {
    PREVENT_DEFAULT_WHEEL.with(|flag| flag.set(true));
}

/// Clears the thread-local `prevent_default_wheel` flag before dispatching a wheel callback.
pub fn clear_prevent_default_wheel() {
    PREVENT_DEFAULT_WHEEL.with(|flag| flag.set(false));
}

/// Reads and clears the thread-local `prevent_default_wheel` flag after dispatching a wheel callback.
pub fn take_prevent_default_wheel() -> bool {
    PREVENT_DEFAULT_WHEEL.with(|flag| flag.replace(false))
}

/// Return-type adapter for `onkeydown` / `onwheel` / `InputTarget` closures.
pub trait KeydownOutcome {
    /// Converts `self` into `true` if the default action should be prevented.
    fn into_prevent_default(self) -> bool;
}

impl KeydownOutcome for () {
    fn into_prevent_default(self) -> bool {
        false
    }
}

impl KeydownOutcome for bool {
    fn into_prevent_default(self) -> bool {
        self
    }
}

/// Semantic `<button type="...">` behavior inside or outside a `<form>`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonKind {
    /// Default `<button>` (`submit` when inside a `<form>`, ordinary button otherwise).
    #[default]
    Auto,
    /// `<button type="button">` (never submits or resets a form).
    Button,
    /// `<button type="submit">` (validates and submits the enclosing `<form>`).
    Submit,
    /// `<button type="reset">` (resets all controls in the enclosing `<form>` to their initial values).
    Reset,
}

/// Virtual keyboard / HTML input type hint for `<input>` elements.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputKind {
    /// Standard plain-text input (`<input type="text">`).
    #[default]
    Text,
    /// Masked password input (`<input type="password">`).
    Password,
    /// Numeric input (`<input type="number">`).
    Number,
    /// Email address input (`<input type="email">`).
    Email,
    /// Search box input (`<input type="search">`).
    Search,
}

/// Active keyboard modifier flags attached to key, pointer, and wheel events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    /// Control key held.
    pub ctrl: bool,
    /// Alt / Option key held.
    pub alt: bool,
    /// Shift key held.
    pub shift: bool,
    /// Super / Command / Windows key held.
    pub super_key: bool,
}

impl Mods {
    /// Returns `true` if `AltGr` (`Ctrl + Alt`) is held on Windows/Linux international keyboards.
    pub const fn is_altgr(self) -> bool {
        self.ctrl && self.alt
    }

    /// Returns `true` if a shortcut command modifier (`Ctrl` or `Super`, excluding `AltGr`) is held.
    pub const fn is_shortcut_ctrl(self) -> bool {
        (self.ctrl && !self.alt) || self.super_key
    }
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

/// Detailed pointer payload passed to `PointerHandler` callbacks.
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
    /// Relative horizontal motion delta in logical pixels.
    pub delta_x: f32,
    /// Relative vertical motion delta in logical pixels.
    pub delta_y: f32,
    /// Button associated with press/release transitions.
    pub button: PointerButton,
    /// Bitmask of currently depressed mouse buttons (`PointerButton::mask`).
    pub buttons: u8,
    /// Multi-click count (`1` single, `2` double, `3` triple).
    pub click_count: u8,
    /// Active keyboard modifier keys.
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
    /// Active keyboard modifier keys.
    pub mods: Mods,
}

/// Event callback wrapper for `.krui` pointer attributes.
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

/// Event callback wrapper for `.krui` `onwheel` returning `bool` (`true` prevents default scroll).
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

/// Pattern-neutral click handler trait implemented by MVVM `Command`, plain closures, and `Rc<dyn Fn()>`.
pub trait ClickTarget {
    /// Converts `self` into an executable click callback (`Rc<dyn Fn()>`).
    fn into_click_fn(self) -> Rc<dyn Fn()>;

    /// Optional reactive `Computed<bool>` guard that controls the target node's `:disabled` state.
    fn can_execute_signal(&self) -> Option<Computed<bool>> {
        None
    }
}

impl ClickTarget for Rc<dyn Fn()> {
    fn into_click_fn(self) -> Rc<dyn Fn()> {
        self
    }
}

impl<F> ClickTarget for F
where
    F: Fn() + 'static,
{
    fn into_click_fn(self) -> Rc<dyn Fn()> {
        Rc::new(self)
    }
}

/// Unified click binding target accepting any [`ClickTarget`].
#[derive(Clone)]
pub struct ClickBinding {
    /// Executable click callback.
    pub action: Rc<dyn Fn()>,
    /// Optional reactive `can_execute` guard driving `:disabled`.
    pub can_execute: Option<Computed<bool>>,
}

impl<T: ClickTarget> From<T> for ClickBinding {
    fn from(target: T) -> Self {
        let can_execute = target.can_execute_signal();
        let action = target.into_click_fn();
        Self {
            action,
            can_execute,
        }
    }
}

/// Pattern-neutral text input handler trait (`oninput`, `onchange`, `onkeydown`).
pub trait InputTarget {
    /// Converts `self` into an executable string callback (`Rc<dyn Fn(String)>`).
    fn into_input_fn(self) -> Rc<dyn Fn(String)>;
}

impl InputTarget for Rc<dyn Fn(String)> {
    fn into_input_fn(self) -> Rc<dyn Fn(String)> {
        self
    }
}

impl<F, R> InputTarget for F
where
    F: Fn(String) -> R + 'static,
    R: KeydownOutcome + 'static,
{
    fn into_input_fn(self) -> Rc<dyn Fn(String)> {
        Rc::new(move |val: String| {
            if self(val).into_prevent_default() {
                prevent_default_key();
            }
        })
    }
}

/// Pattern-neutral target accepted by `set_on_pointer_*` and `set_on_context_menu`.
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

impl<F> PointerTarget for F
where
    F: Fn() + 'static,
{
    fn into_pointer_fn(self) -> Rc<dyn Fn(PointerEvent)> {
        Rc::new(move |_ev| (self)())
    }
}

/// Pattern-neutral target accepted by `set_on_wheel`.
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
