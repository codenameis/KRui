//! Dynamic name registry (`Value`, `DynSignal`, `Registry`, `ScopeRegistry`) for live preview.
//!
//! Compiled **only** when `#[cfg(feature = "preview")]` is active so release builds incur
//! zero code-size or runtime overhead.

mod conversions;

pub use conversions::{
    IntoRegistryValue, RegistryFieldExt, RegistryFieldFallback, RegistryMethodDispatchFallback,
    ScopeFieldProbe, TryFromRegistryValue,
};

use crate::scene::CanvasDraw;
use krui_signals::{Computed, FastPathTransform, Signal};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

/// Reference-counted handle to a [`Registry`] object inside [`Value::Record`].
pub type RegistryRef = Rc<dyn Registry>;

/// Executable click command stored inside [`Value::Command`].
#[derive(Clone)]
pub struct DynCommand {
    execute_fn: Rc<dyn Fn()>,
    can_execute: Option<Computed<bool>>,
}

impl DynCommand {
    /// Creates a new `DynCommand` with an optional reactive `can_execute` guard.
    pub fn new(execute_fn: Rc<dyn Fn()>, can_execute: Option<Computed<bool>>) -> Self {
        Self {
            execute_fn,
            can_execute,
        }
    }

    /// Executes the command callback.
    pub fn execute(&self) {
        (self.execute_fn)();
    }

    /// Returns the executable click closure.
    pub fn as_rc_fn(&self) -> Rc<dyn Fn()> {
        self.execute_fn.clone()
    }

    /// Returns the optional reactive `can_execute` guard.
    pub fn can_execute(&self) -> Option<Computed<bool>> {
        self.can_execute
    }
}

impl fmt::Debug for DynCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DynCommand")
            .field("has_can_execute", &self.can_execute.is_some())
            .finish()
    }
}

/// Executable `<input>` string handler stored inside [`Value::InputHandler`].
#[derive(Clone)]
pub struct DynInputHandler(pub Rc<dyn Fn(String)>);

impl fmt::Debug for DynInputHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DynInputHandler(..)")
    }
}

/// Dynamically-typed runtime value used by the live preview interpreter (`preview` feature only).
#[derive(Clone)]
pub enum Value {
    /// Boolean scalar (`true` / `false`).
    Bool(bool),
    /// 64-bit signed integer scalar.
    Int(i64),
    /// 64-bit floating-point scalar.
    Float(f64),
    /// UTF-8 string scalar.
    Str(String),
    /// Homogeneous or heterogeneous list of values (used by `<for each="{item in list}">`).
    List(Vec<Value>),
    /// Nested object/struct/service implementing [`Registry`].
    Record(RegistryRef),
    /// Executable click command (`Command` or `store.on(Msg::...)`).
    Command(DynCommand),
    /// Executable text-input handler (`InputCommand` or `store.on_input(Msg::...)`).
    InputHandler(DynInputHandler),
    /// 2D canvas drawing callback (`CanvasDraw`).
    CanvasDraw(CanvasDraw),
    /// Compositor fast-path transform bundle (`FastPathTransform`).
    FastTransform(FastPathTransform),
    /// Unit return value (`()`).
    Unit,
}

/// Error returned when invoking a method or command on a [`Registry`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// The requested method or receiver path does not exist on the registry.
    UnknownMethod(String),
    /// The number of arguments passed did not match the method signature.
    ArityMismatch {
        method: String,
        expected: usize,
        actual: usize,
    },
    /// An argument could not be converted to the expected parameter type.
    TypeMismatch {
        method: String,
        arg_index: usize,
        expected: &'static str,
        actual: String,
    },
    /// Reading or writing a signal failed.
    ReadOnlySignal(String),
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMethod(m) => write!(f, "Unknown method `{m}` in Registry"),
            Self::ArityMismatch {
                method,
                expected,
                actual,
            } => write!(
                f,
                "Method `{method}` expected {expected} argument(s), got {actual}"
            ),
            Self::TypeMismatch {
                method,
                arg_index,
                expected,
                actual,
            } => write!(
                f,
                "Method `{method}` argument #{arg_index} expected `{expected}`, got `{actual}`"
            ),
            Self::ReadOnlySignal(p) => write!(f, "Signal `{p}` is read-only"),
        }
    }
}

impl std::error::Error for CallError {}

type DynGetter = Rc<dyn Fn() -> Value>;
type DynSetter = Rc<dyn Fn(Value) -> Result<(), CallError>>;

/// Type-erased reactive signal/computed handle for the live preview interpreter.
///
/// Calling [`DynSignal::get`] inside an `Effect` or `Computed` automatically subscribes the
/// caller to the underlying `Signal<T>` or `Computed<T>` in `krui-signals`.
#[derive(Clone)]
pub struct DynSignal {
    getter: DynGetter,
    setter: Option<DynSetter>,
    f32_signal: Option<Signal<f32>>,
    fast_transform: Option<FastPathTransform>,
}

impl fmt::Debug for DynSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DynSignal")
            .field("writable", &self.setter.is_some())
            .field("is_f32_signal", &self.f32_signal.is_some())
            .finish()
    }
}

impl DynSignal {
    /// Wraps a read-write `Signal<T>` into a `DynSignal`.
    pub fn from_signal<T>(sig: Signal<T>) -> Self
    where
        T: Clone + IntoRegistryValue + TryFromRegistryValue + 'static,
    {
        let f32_signal = T::try_as_f32_signal(sig);
        Self {
            getter: Rc::new(move || sig.get().into_registry_value()),
            setter: Some(Rc::new(move |val: Value| {
                let parsed =
                    T::try_from_registry_value(&val).ok_or_else(|| CallError::TypeMismatch {
                        method: "Signal::set".to_string(),
                        arg_index: 0,
                        expected: std::any::type_name::<T>(),
                        actual: format!("{val:?}"),
                    })?;
                sig.set(parsed);
                Ok(())
            })),
            f32_signal,
            fast_transform: None,
        }
    }

    /// Wraps a read-only `Signal<T>` (when `T` does not implement `TryFromRegistryValue`) into a `DynSignal`.
    pub fn from_readonly_signal<T>(sig: Signal<T>) -> Self
    where
        T: Clone + IntoRegistryValue + 'static,
    {
        let f32_signal = T::try_as_f32_signal(sig);
        Self {
            getter: Rc::new(move || sig.get().into_registry_value()),
            setter: None,
            f32_signal,
            fast_transform: None,
        }
    }

    /// Wraps a read-only `Computed<T>` into a `DynSignal`.
    pub fn from_computed<T>(comp: Computed<T>) -> Self
    where
        T: Clone + IntoRegistryValue + 'static,
    {
        let f32_signal = T::try_as_f32_signal(comp.as_signal());
        Self {
            getter: Rc::new(move || comp.get().into_registry_value()),
            setter: None,
            f32_signal,
            fast_transform: None,
        }
    }

    /// Wraps a `FastPathTransform` into a `DynSignal`.
    pub fn from_fast_transform(ft: FastPathTransform) -> Self {
        Self {
            getter: Rc::new(move || Value::FastTransform(ft)),
            setter: None,
            f32_signal: None,
            fast_transform: Some(ft),
        }
    }

    /// Reads the current value, subscribing any active `Effect` or `Computed`.
    pub fn get(&self) -> Value {
        (self.getter)()
    }

    /// Writes a new value if this `DynSignal` wraps a writable `Signal<T>`.
    pub fn set(&self, value: Value) -> Result<(), CallError> {
        let Some(setter) = &self.setter else {
            return Err(CallError::ReadOnlySignal("<computed>".to_string()));
        };
        setter(value)
    }

    /// Returns `true` if this `DynSignal` supports [`DynSignal::set`].
    pub fn is_writable(&self) -> bool {
        self.setter.is_some()
    }

    /// Returns the underlying `Signal<f32>` if this signal's native type is `f32`
    /// (used for zero-overhead paint/layout slot bindings in preview mode).
    pub fn as_f32_signal(&self) -> Option<Signal<f32>> {
        self.f32_signal
    }

    /// Returns the underlying `FastPathTransform` if this wraps a `FastPathTransform`.
    pub fn as_fast_transform(&self) -> Option<FastPathTransform> {
        self.fast_transform
    }
}

/// Name-to-value/signal/method lookup table used by the live preview interpreter (`preview` feature only).
pub trait Registry {
    /// Evaluates and returns the current [`Value`] at `path` (e.g. `"product_title"` or `"model.cash_text"`).
    ///
    /// Calling `get` inside an `Effect` subscribes the `Effect` to any underlying reactive signals.
    fn get(&self, path: &str) -> Option<Value>;

    /// Looks up a reactive [`DynSignal`] handle at `path` (supports `get`, `set` for `bind:value`,
    /// and `as_f32_signal` for paint/layout slot bindings).
    fn signal(&self, path: &str) -> Option<DynSignal>;

    /// Invokes a registered method or command at `name` (e.g. `"buy_equity"`, `"model.reset"`, or `"on"`)
    /// with dynamically evaluated arguments `args`.
    fn call(&self, name: &str, args: &[Value]) -> Result<Value, CallError>;

    /// Validates that a method or command at `name` exists and accepts `args` without executing
    /// side-effecting controller mutations during preflight validation.
    fn validate_call(&self, name: &str, args: &[Value]) -> Result<Value, CallError> {
        self.call(name, args)
    }
}

/// Composite root registry that maps template root variable names (e.g., `"catalog_vm"`, `"cart_vm"`,
/// `"controller"`, `"store"`) to their respective [`Registry`] or [`DynSignal`] instances.
#[derive(Default, Clone)]
pub struct ScopeRegistry {
    registries: HashMap<String, RegistryRef>,
    signals: HashMap<String, DynSignal>,
    values: HashMap<String, Value>,
}

impl ScopeRegistry {
    /// Creates an empty `ScopeRegistry`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if no sub-registries, signals, or values have been registered.
    pub fn is_empty(&self) -> bool {
        self.registries.is_empty() && self.signals.is_empty() && self.values.is_empty()
    }

    /// Registers a named sub-registry (such as a ViewModel, Controller, or Store).
    pub fn insert_registry(&mut self, name: impl Into<String>, reg: impl Registry + 'static) {
        self.registries.insert(name.into(), Rc::new(reg));
    }

    /// Registers a shared [`RegistryRef`] under `name`.
    pub fn insert_registry_ref(&mut self, name: impl Into<String>, reg: RegistryRef) {
        self.registries.insert(name.into(), reg);
    }

    /// Registers a standalone [`DynSignal`] under `name` (e.g. local signals like `x_pos` or `header_shake`).
    pub fn insert_signal(&mut self, name: impl Into<String>, sig: DynSignal) {
        self.signals.insert(name.into(), sig);
    }

    /// Registers a constant or callback [`Value`] under `name`.
    pub fn insert_value(&mut self, name: impl Into<String>, val: Value) {
        self.values.insert(name.into(), val);
    }
    /// Inserts a root template variable (`Signal<T>`, `Computed<T>`, `Registry` struct, `CanvasDraw`, etc.)
    /// using [`RegistryFieldExt`].
    pub fn insert_field<F: RegistryFieldExt>(&mut self, name: impl Into<String>, field: &F) {
        let name = name.into();
        if let Some(sig) = field.registry_field_signal() {
            self.signals.insert(name, sig);
        } else if let Some(val) = field.registry_field_get() {
            if let Value::Record(rec) = val {
                self.registries.insert(name, rec);
            } else {
                self.values.insert(name, val);
            }
        }
    }

    /// Inserts an already extracted `(Option<DynSignal>, Option<Value>)` pair into this `ScopeRegistry`.
    pub fn insert_probed_field(
        &mut self,
        name: impl Into<String>,
        probed: (Option<DynSignal>, Option<Value>),
    ) {
        let name = name.into();
        if let Some(sig) = probed.0 {
            self.signals.insert(name, sig);
        } else if let Some(val) = probed.1 {
            if let Value::Record(rec) = val {
                self.registries.insert(name, rec);
            } else {
                self.values.insert(name, val);
            }
        }
    }
}

impl Registry for ScopeRegistry {
    fn get(&self, path: &str) -> Option<Value> {
        let trimmed = path.trim();
        if let Some((head, tail)) = trimmed.split_once('.') {
            if let Some(sub) = self.registries.get(head) {
                return sub.get(tail);
            }
            if let Some(Value::Record(sub)) = self.values.get(head) {
                return sub.get(tail);
            }
            if let Some(sig) = self.signals.get(head) {
                if let Value::Record(sub) = sig.get() {
                    return sub.get(tail);
                }
            }
            None
        } else if let Some(sig) = self.signals.get(trimmed) {
            Some(sig.get())
        } else if let Some(val) = self.values.get(trimmed) {
            Some(val.clone())
        } else {
            self.registries
                .get(trimmed)
                .map(|r| Value::Record(r.clone()))
        }
    }

    fn signal(&self, path: &str) -> Option<DynSignal> {
        let trimmed = path.trim();
        if let Some((head, tail)) = trimmed.split_once('.') {
            if let Some(sub) = self.registries.get(head) {
                return sub.signal(tail);
            }
            if let Some(Value::Record(sub)) = self.values.get(head) {
                return sub.signal(tail);
            }
            None
        } else {
            self.signals.get(trimmed).cloned()
        }
    }

    fn call(&self, name: &str, args: &[Value]) -> Result<Value, CallError> {
        let trimmed = name.trim();
        if let Some((head, tail)) = trimmed.split_once('.') {
            if let Some(sub) = self.registries.get(head) {
                return sub.call(tail, args);
            }
            if let Some(Value::Record(sub)) = self.values.get(head) {
                return sub.call(tail, args);
            }
        }
        if let Some(Value::Command(cmd)) = self.get(trimmed) {
            if !args.is_empty() {
                return Err(CallError::ArityMismatch {
                    method: trimmed.to_string(),
                    expected: 0,
                    actual: args.len(),
                });
            }
            cmd.execute();
            return Ok(Value::Unit);
        }
        Err(CallError::UnknownMethod(trimmed.to_string()))
    }

    fn validate_call(&self, name: &str, args: &[Value]) -> Result<Value, CallError> {
        let trimmed = name.trim();
        if let Some((head, tail)) = trimmed.split_once('.') {
            if let Some(sub) = self.registries.get(head) {
                return sub.validate_call(tail, args);
            }
            if let Some(Value::Record(sub)) = self.values.get(head) {
                return sub.validate_call(tail, args);
            }
        }
        if let Some(Value::Command(_)) = self.get(trimmed) {
            if !args.is_empty() {
                return Err(CallError::ArityMismatch {
                    method: trimmed.to_string(),
                    expected: 0,
                    actual: args.len(),
                });
            }
            return Ok(Value::Unit);
        }
        Err(CallError::UnknownMethod(trimmed.to_string()))
    }
}
