//! DSL template condition, scalar arithmetic/relational helpers, and iterable traits.

use crate::{Computed, Signal};
use std::fmt;

/// Trait allowing `.krui` boolean conditions (`test="{...}"`, `disabled="{...}"`) to accept `bool`, `Signal<bool>`, or `Computed<bool>`.
pub trait KruiBoolCondition {
    /// Evaluates the boolean condition (subscribing if reactive).
    fn eval_krui_bool(&self) -> bool;
}

impl KruiBoolCondition for bool {
    fn eval_krui_bool(&self) -> bool {
        *self
    }
}

impl KruiBoolCondition for Signal<bool> {
    fn eval_krui_bool(&self) -> bool {
        self.try_get().unwrap_or(false)
    }
}

impl KruiBoolCondition for Computed<bool> {
    fn eval_krui_bool(&self) -> bool {
        self.as_signal().try_get().unwrap_or(false)
    }
}

/// Scalar value produced by a `.krui` template expression (`bool`, `i64`, `f64`, or `String`).
#[derive(Clone, Debug)]
pub enum KruiTemplateScalar {
    /// Boolean scalar (`true` / `false`).
    Bool(bool),
    /// 64-bit signed integer scalar.
    Int(i64),
    /// 64-bit floating-point scalar.
    Float(f64),
    /// UTF-8 string scalar.
    Str(String),
}

impl fmt::Display for KruiTemplateScalar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(n) => write!(f, "{n}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Str(s) => f.write_str(s),
        }
    }
}

impl PartialEq for KruiTemplateScalar {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => (a - b).abs() <= f64::EPSILON,
            (Self::Int(a), Self::Float(b)) => (*a as f64 - *b).abs() <= f64::EPSILON,
            (Self::Float(a), Self::Int(b)) => (*a - *b as f64).abs() <= f64::EPSILON,
            (Self::Str(a), Self::Str(b)) => a == b,
            _ => false,
        }
    }
}

impl KruiBoolCondition for KruiTemplateScalar {
    fn eval_krui_bool(&self) -> bool {
        self.is_truthy()
    }
}

impl KruiTemplateScalar {
    /// Returns `true` if this scalar is truthy (`Bool(true)`).
    pub fn is_truthy(&self) -> bool {
        matches!(self, Self::Bool(true))
    }

    /// Converts numeric scalars (`Int` or `Float`) to `f64`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(n) => Some(*n as f64),
            Self::Float(v) => Some(*v),
            _ => None,
        }
    }

    /// Converts numeric scalars (`Int` or `Float`) to `f32`.
    pub fn as_f32(&self) -> f32 {
        self.as_f64().unwrap_or(0.0) as f32
    }

    /// Saturating unary negation (`-expr`); returns `0` / `0.0` on non-numeric scalars.
    pub fn checked_neg(self) -> Self {
        match self {
            Self::Int(n) => Self::Int(n.saturating_neg()),
            Self::Float(v) => Self::Float(-v),
            _ => Self::Int(0),
        }
    }

    /// Saturating addition (`lhs + rhs`), supporting `Int + Int`, `Float` promotion, and `Str` concatenation.
    pub fn checked_add(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Int(a), Self::Int(b)) => Self::Int(a.saturating_add(b)),
            (Self::Str(a), b) => Self::Str(format!("{a}{b}")),
            (a, Self::Str(b)) => Self::Str(format!("{a}{b}")),
            (lhs, rhs) => {
                let l = lhs.as_f64().unwrap_or(0.0);
                let r = rhs.as_f64().unwrap_or(0.0);
                Self::Float(l + r)
            }
        }
    }

    /// Saturating subtraction (`lhs - rhs`).
    pub fn checked_sub(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Int(a), Self::Int(b)) => Self::Int(a.saturating_sub(b)),
            (lhs, rhs) => {
                let l = lhs.as_f64().unwrap_or(0.0);
                let r = rhs.as_f64().unwrap_or(0.0);
                Self::Float(l - r)
            }
        }
    }

    /// Saturating multiplication (`lhs * rhs`).
    pub fn checked_mul(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Int(a), Self::Int(b)) => Self::Int(a.saturating_mul(b)),
            (lhs, rhs) => {
                let l = lhs.as_f64().unwrap_or(0.0);
                let r = rhs.as_f64().unwrap_or(0.0);
                Self::Float(l * r)
            }
        }
    }

    /// Non-panicking division (`lhs / rhs`), returning `0` / `0.0` on zero divisor or `i64::MIN / -1`.
    pub fn safe_div(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Int(a), Self::Int(b)) => {
                if b == 0 {
                    Self::Int(0)
                } else {
                    Self::Int(a.saturating_div(b))
                }
            }
            (lhs, rhs) => {
                let l = lhs.as_f64().unwrap_or(0.0);
                let r = rhs.as_f64().unwrap_or(0.0);
                if r == 0.0 || !r.is_finite() {
                    Self::Float(0.0)
                } else {
                    Self::Float(l / r)
                }
            }
        }
    }

    /// Non-panicking remainder (`lhs % rhs`), returning `0` / `0.0` on zero divisor.
    pub fn safe_rem(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Int(a), Self::Int(b)) => {
                if b == 0 {
                    Self::Int(0)
                } else {
                    Self::Int(a.wrapping_rem(b))
                }
            }
            (lhs, rhs) => {
                let l = lhs.as_f64().unwrap_or(0.0);
                let r = rhs.as_f64().unwrap_or(0.0);
                if r == 0.0 || !r.is_finite() {
                    Self::Float(0.0)
                } else {
                    Self::Float(l % r)
                }
            }
        }
    }

    /// Checked division (`lhs / rhs`), delegating to non-panicking [`Self::safe_div`].
    pub fn checked_div(self, rhs: Self) -> Self {
        self.safe_div(rhs)
    }

    /// Total, NaN-safe relational comparison (`<`, `<=`, `>`, `>=`) on scalar types.
    pub fn compare_ord(&self, rhs: &Self) -> std::cmp::Ordering {
        match (self, rhs) {
            (Self::Int(a), Self::Int(b)) => a.cmp(b),
            (Self::Str(a), Self::Str(b)) => a.cmp(b),
            (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            (lhs, rhs) => {
                let l = lhs.as_f64().unwrap_or(0.0);
                let r = rhs.as_f64().unwrap_or(0.0);
                l.total_cmp(&r)
            }
        }
    }
}

/// Converts a value or reactive signal (`Signal<T>`, `Computed<T>`) into a [`KruiTemplateScalar`]
/// so `.krui` arithmetic, comparison, and boolean operators work identically in macro codegen and preview.
pub trait IntoKruiTemplateScalar {
    /// Reads `self` (subscribing if reactive) and converts it into a [`KruiTemplateScalar`].
    #[allow(clippy::wrong_self_convention)]
    fn into_krui_scalar(&self) -> KruiTemplateScalar;
}

impl IntoKruiTemplateScalar for KruiTemplateScalar {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        self.clone()
    }
}

impl IntoKruiTemplateScalar for bool {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        KruiTemplateScalar::Bool(*self)
    }
}

impl IntoKruiTemplateScalar for i32 {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        KruiTemplateScalar::Int(*self as i64)
    }
}

impl IntoKruiTemplateScalar for i64 {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        KruiTemplateScalar::Int(*self)
    }
}

impl IntoKruiTemplateScalar for u32 {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        KruiTemplateScalar::Int(*self as i64)
    }
}

impl IntoKruiTemplateScalar for usize {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        KruiTemplateScalar::Int(*self as i64)
    }
}

impl IntoKruiTemplateScalar for f32 {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        KruiTemplateScalar::Float(*self as f64)
    }
}

impl IntoKruiTemplateScalar for f64 {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        KruiTemplateScalar::Float(*self)
    }
}

impl IntoKruiTemplateScalar for String {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        KruiTemplateScalar::Str(self.clone())
    }
}

impl IntoKruiTemplateScalar for str {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        KruiTemplateScalar::Str(self.to_string())
    }
}

impl IntoKruiTemplateScalar for &str {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        KruiTemplateScalar::Str((*self).to_string())
    }
}

impl<T: IntoKruiTemplateScalar + Clone + 'static> IntoKruiTemplateScalar for Signal<T> {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        self.with(IntoKruiTemplateScalar::into_krui_scalar)
    }
}

impl<T: IntoKruiTemplateScalar + Clone + 'static> IntoKruiTemplateScalar for Computed<T> {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        self.with(IntoKruiTemplateScalar::into_krui_scalar)
    }
}

/// Trait allowing `.krui` `<for each="{item in list}">` to iterate over `Signal<Vec<T>>`, `Computed<Vec<T>>`, or `Vec<T>`.
pub trait KruiIterableSource {
    /// Cloned element item type.
    type Item: Clone + 'static;
    /// Reads and returns the current items list.
    fn read_krui_items(&self) -> Vec<Self::Item>;
}

impl<T: Clone + 'static> KruiIterableSource for Signal<Vec<T>> {
    type Item = T;
    fn read_krui_items(&self) -> Vec<T> {
        self.try_get().unwrap_or_default()
    }
}

impl<T: Clone + 'static> KruiIterableSource for Computed<Vec<T>> {
    type Item = T;
    fn read_krui_items(&self) -> Vec<T> {
        self.as_signal().try_get().unwrap_or_default()
    }
}

impl<T: Clone + 'static> KruiIterableSource for Vec<T> {
    type Item = T;
    fn read_krui_items(&self) -> Vec<T> {
        self.clone()
    }
}

impl<T: Clone + 'static> KruiIterableSource for [T] {
    type Item = T;
    fn read_krui_items(&self) -> Vec<T> {
        self.to_vec()
    }
}

/// Zero-allocation stack wrapper giving non-reactive `<for>` item bindings (`key_fn`, `<select><for>`)
/// the same `.get()` access syntax as `Signal<T>`.
#[derive(Clone, Debug)]
pub struct KruiLoopItem<T>(pub T);

impl<T: Clone> KruiLoopItem<T> {
    /// Returns a clone of the wrapped loop item without allocating a reactive signal.
    #[inline]
    pub fn get(&self) -> T {
        self.0.clone()
    }

    /// Non-panicking parity helper matching `Signal::try_get` on loop items.
    #[inline]
    pub fn try_get(&self) -> Option<T> {
        Some(self.0.clone())
    }

    /// Borrows the wrapped loop item without cloning (`Some(f(&self.0))`).
    #[inline]
    pub fn try_with<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        Some(f(&self.0))
    }
}

impl<T: IntoKruiTemplateScalar + Clone> IntoKruiTemplateScalar for KruiLoopItem<T> {
    fn into_krui_scalar(&self) -> KruiTemplateScalar {
        self.0.into_krui_scalar()
    }
}
