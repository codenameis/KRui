//! Type conversion and duck-typed field extension traits for [`super::Registry`].

use super::{CallError, DynSignal, Value};
use crate::scene::CanvasDraw;
use crate::view_tree::{PointerHandler, WheelHandler};
use krui_signals::{Computed, FastPathTransform, Signal};
use std::fmt;

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "Value::Bool({b})"),
            Self::Int(n) => write!(f, "Value::Int({n})"),
            Self::Float(v) => write!(f, "Value::Float({v})"),
            Self::Str(s) => write!(f, "Value::Str({s:?})"),
            Self::List(items) => write!(f, "Value::List(len={})", items.len()),
            Self::Record(_) => f.write_str("Value::Record(..)"),
            Self::Command(c) => write!(f, "Value::Command({c:?})"),
            Self::InputHandler(_) => f.write_str("Value::InputHandler(..)"),
            Self::CanvasDraw(_) => f.write_str("Value::CanvasDraw(..)"),
            Self::PointerHandler(_) => f.write_str("Value::PointerHandler(..)"),
            Self::WheelHandler(_) => f.write_str("Value::WheelHandler(..)"),
            Self::FastTransform(_) => f.write_str("Value::FastTransform(..)"),
            Self::Unit => f.write_str("Value::Unit"),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => (a - b).abs() <= f64::EPSILON,
            (Self::Int(a), Self::Float(b)) => (*a as f64 - *b).abs() <= f64::EPSILON,
            (Self::Float(a), Self::Int(b)) => (*a - *b as f64).abs() <= f64::EPSILON,
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::List(a), Self::List(b)) => a == b,
            (Self::Unit, Self::Unit) => true,
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(n) => write!(f, "{n}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Str(s) => f.write_str(s),
            Self::List(items) => write!(f, "[{} items]", items.len()),
            Self::Record(_) => f.write_str("[Object]"),
            Self::Command(_) => f.write_str("[Command]"),
            Self::InputHandler(_) => f.write_str("[InputHandler]"),
            Self::CanvasDraw(_) => f.write_str("[CanvasDraw]"),
            Self::PointerHandler(_) => f.write_str("[PointerHandler]"),
            Self::WheelHandler(_) => f.write_str("[WheelHandler]"),
            Self::FastTransform(_) => f.write_str("[FastTransform]"),
            Self::Unit => Ok(()),
        }
    }
}

impl Value {
    /// Evaluates truthiness for `<if test="{...}">`, `disabled="{...}"`, and logical operators.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Converts numeric values (`Int` or `Float`) to `f64`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(n) => Some(*n as f64),
            Self::Float(v) => Some(*v),
            _ => None,
        }
    }

    /// Converts integer values to `i64`.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            Self::Float(v) => Some(*v as i64),
            _ => None,
        }
    }

    /// Returns the string slice if this is `Value::Str`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

/// Helper trait for converting Rust field values into [`Value`].
pub trait IntoRegistryValue: Sized {
    /// Converts `self` into a dynamically-typed [`Value`].
    fn into_registry_value(self) -> Value;

    /// Returns `Some(sig)` if `Self` is `f32`.
    fn try_as_f32_signal(_sig: Signal<Self>) -> Option<Signal<f32>> {
        None
    }
}

/// Helper trait for converting a [`Value`] back into a concrete Rust parameter or signal type.
pub trait TryFromRegistryValue: Sized {
    /// Attempts to extract `Self` from `value`.
    fn try_from_registry_value(value: &Value) -> Option<Self>;
}

/// Duck-typed extension trait used by `#[view_model]` and `#[controller]` under `preview`
/// to convert arbitrary struct fields (`Signal<T>`, `Computed<T>`, `Command`, nested `Registry`,
/// `[Signal<f32>; N]`, or plain values) into [`Value`] and [`DynSignal`].
pub trait RegistryFieldExt {
    /// Reads the field as a [`Value`] if supported.
    fn registry_field_get(&self) -> Option<Value>;

    /// Obtains a [`DynSignal`] handle for the field if it is a `Signal` or `Computed`.
    fn registry_field_signal(&self) -> Option<DynSignal> {
        None
    }
}

/// Fallback method-dispatch trait so `#[controller]` on a `struct` works whether or not
/// an accompanying `#[controller] impl` block is annotated.
pub trait RegistryMethodDispatchFallback {
    /// Fallback method dispatch returning `None`.
    fn __krui_dispatch_method(
        &self,
        _name: &str,
        _args: &[Value],
    ) -> Option<Result<Value, CallError>> {
        None
    }

    /// Fallback side-effect-free method validation returning `None`.
    fn __krui_validate_method(
        &self,
        _name: &str,
        _args: &[Value],
    ) -> Option<Result<Value, CallError>> {
        None
    }
}

impl<T> RegistryMethodDispatchFallback for T {}

/// Autoref-specialized probe wrapper used by `krui!` / `include_krui!` to register root variables
/// in `ScopeRegistry` when `T: RegistryFieldExt` while ignoring raw local test closures.
pub struct ScopeFieldProbe<'a, T>(pub &'a T);

impl<'a, T: RegistryFieldExt> ScopeFieldProbe<'a, T> {
    /// Extracts `(Option<DynSignal>, Option<Value>)` when `T: RegistryFieldExt`.
    pub fn probe_field(&self) -> (Option<DynSignal>, Option<Value>) {
        (self.0.registry_field_signal(), self.0.registry_field_get())
    }
}

/// Fallback trait for [`ScopeFieldProbe`] when `T` does not implement [`RegistryFieldExt`].
pub trait RegistryFieldFallback {
    /// Fallback returning `(None, None)`.
    fn probe_field(&self) -> (Option<DynSignal>, Option<Value>) {
        (None, None)
    }
}

impl<'a, T> RegistryFieldFallback for ScopeFieldProbe<'a, T> {}

impl<T> RegistryFieldExt for Signal<T>
where
    T: Clone + IntoRegistryValue + TryFromRegistryValue + 'static,
{
    fn registry_field_get(&self) -> Option<Value> {
        Some(self.get().into_registry_value())
    }

    fn registry_field_signal(&self) -> Option<DynSignal> {
        Some(DynSignal::from_signal(*self))
    }
}

impl<T> RegistryFieldExt for Computed<T>
where
    T: Clone + IntoRegistryValue + 'static,
{
    fn registry_field_get(&self) -> Option<Value> {
        Some(self.get().into_registry_value())
    }

    fn registry_field_signal(&self) -> Option<DynSignal> {
        Some(DynSignal::from_computed(*self))
    }
}

impl RegistryFieldExt for CanvasDraw {
    fn registry_field_get(&self) -> Option<Value> {
        Some(Value::CanvasDraw(self.clone()))
    }
}

impl RegistryFieldExt for PointerHandler {
    fn registry_field_get(&self) -> Option<Value> {
        Some(Value::PointerHandler(self.clone()))
    }
}

impl RegistryFieldExt for WheelHandler {
    fn registry_field_get(&self) -> Option<Value> {
        Some(Value::WheelHandler(self.clone()))
    }
}

impl RegistryFieldExt for FastPathTransform {
    fn registry_field_get(&self) -> Option<Value> {
        Some(Value::FastTransform(*self))
    }

    fn registry_field_signal(&self) -> Option<DynSignal> {
        Some(DynSignal::from_fast_transform(*self))
    }
}

impl RegistryFieldExt for bool {
    fn registry_field_get(&self) -> Option<Value> {
        Some(Value::Bool(*self))
    }
}
impl RegistryFieldExt for i32 {
    fn registry_field_get(&self) -> Option<Value> {
        Some(Value::Int(*self as i64))
    }
}
impl RegistryFieldExt for i64 {
    fn registry_field_get(&self) -> Option<Value> {
        Some(Value::Int(*self))
    }
}
impl RegistryFieldExt for u32 {
    fn registry_field_get(&self) -> Option<Value> {
        Some(Value::Int(*self as i64))
    }
}
impl RegistryFieldExt for usize {
    fn registry_field_get(&self) -> Option<Value> {
        Some(Value::Int(*self as i64))
    }
}
impl RegistryFieldExt for f32 {
    fn registry_field_get(&self) -> Option<Value> {
        Some(Value::Float(*self as f64))
    }
}
impl RegistryFieldExt for f64 {
    fn registry_field_get(&self) -> Option<Value> {
        Some(Value::Float(*self))
    }
}
impl RegistryFieldExt for String {
    fn registry_field_get(&self) -> Option<Value> {
        Some(Value::Str(self.clone()))
    }
}
impl<T: Clone + IntoRegistryValue + 'static> RegistryFieldExt for Vec<T> {
    fn registry_field_get(&self) -> Option<Value> {
        Some(self.clone().into_registry_value())
    }
}

impl<T, const N: usize> RegistryFieldExt for [T; N] {
    fn registry_field_get(&self) -> Option<Value> {
        None
    }
}

impl IntoRegistryValue for bool {
    fn into_registry_value(self) -> Value {
        Value::Bool(self)
    }
}
impl TryFromRegistryValue for bool {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        value
            .as_bool()
            .or_else(|| value.as_str()?.trim().parse::<bool>().ok())
    }
}

impl IntoRegistryValue for i32 {
    fn into_registry_value(self) -> Value {
        Value::Int(self as i64)
    }
}
impl TryFromRegistryValue for i32 {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        value
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .or_else(|| value.as_str()?.trim().parse::<i32>().ok())
    }
}

impl IntoRegistryValue for i64 {
    fn into_registry_value(self) -> Value {
        Value::Int(self)
    }
}
impl TryFromRegistryValue for i64 {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        value
            .as_i64()
            .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
    }
}

impl IntoRegistryValue for u32 {
    fn into_registry_value(self) -> Value {
        Value::Int(self as i64)
    }
}
impl TryFromRegistryValue for u32 {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        value
            .as_i64()
            .and_then(|v| u32::try_from(v).ok())
            .or_else(|| value.as_str()?.trim().parse::<u32>().ok())
    }
}

impl IntoRegistryValue for usize {
    fn into_registry_value(self) -> Value {
        Value::Int(self as i64)
    }
}
impl TryFromRegistryValue for usize {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        value
            .as_i64()
            .and_then(|v| usize::try_from(v).ok())
            .or_else(|| value.as_str()?.trim().parse::<usize>().ok())
    }
}

impl IntoRegistryValue for f32 {
    fn into_registry_value(self) -> Value {
        Value::Float(self as f64)
    }
    fn try_as_f32_signal(sig: Signal<Self>) -> Option<Signal<f32>> {
        Some(sig)
    }
}
impl TryFromRegistryValue for f32 {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        value
            .as_f64()
            .map(|v| v as f32)
            .or_else(|| value.as_str()?.trim().parse::<f32>().ok())
    }
}

impl IntoRegistryValue for f64 {
    fn into_registry_value(self) -> Value {
        Value::Float(self)
    }
}
impl TryFromRegistryValue for f64 {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        value
            .as_f64()
            .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
    }
}

impl IntoRegistryValue for String {
    fn into_registry_value(self) -> Value {
        Value::Str(self)
    }
}
impl TryFromRegistryValue for String {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        match value {
            Value::Str(s) => Some(s.clone()),
            other => Some(other.to_string()),
        }
    }
}

impl IntoRegistryValue for &'static str {
    fn into_registry_value(self) -> Value {
        Value::Str(self.to_string())
    }
}

impl IntoRegistryValue for () {
    fn into_registry_value(self) -> Value {
        Value::Unit
    }
}

impl IntoRegistryValue for Value {
    fn into_registry_value(self) -> Value {
        self
    }
}
impl TryFromRegistryValue for Value {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        Some(value.clone())
    }
}

impl IntoRegistryValue for CanvasDraw {
    fn into_registry_value(self) -> Value {
        Value::CanvasDraw(self)
    }
}

impl IntoRegistryValue for FastPathTransform {
    fn into_registry_value(self) -> Value {
        Value::FastTransform(self)
    }
}

impl TryFromRegistryValue for FastPathTransform {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        match value {
            Value::FastTransform(ft) => Some(*ft),
            _ => None,
        }
    }
}

impl<T: IntoRegistryValue> IntoRegistryValue for Vec<T> {
    fn into_registry_value(self) -> Value {
        Value::List(self.into_iter().map(T::into_registry_value).collect())
    }
}

impl<T: TryFromRegistryValue> TryFromRegistryValue for Vec<T> {
    fn try_from_registry_value(value: &Value) -> Option<Self> {
        match value {
            Value::List(items) => items.iter().map(T::try_from_registry_value).collect(),
            _ => None,
        }
    }
}
