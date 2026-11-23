//! Runtime `TemplateExpr` evaluator and `<for>` loop-variable scope overlay for `krui-preview`.

use krui_compiler::{BinaryOp, CompiledTextPart, TemplateExpr, TemplateLit, UnaryOp};
use krui_core::{CallError, DynSignal, Registry, RegistryRef, Value};
use krui_signals::Signal;
use krui_style::sanitize_untrusted_text;
use std::fmt;

/// Runtime evaluation error produced when a `TemplateExpr` fails lookup, type checking, or checked arithmetic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvalError {
    /// A path (e.g. `vm.non_existent`) was not found in the active `Registry`.
    UnknownPath(String),
    /// A writable signal path (for `bind:value`) was not found or is read-only.
    UnknownSignal(String),
    /// A method/command invocation failed (`UnknownMethod`, `ArityMismatch`, `TypeMismatch`, etc.).
    CallFailed(CallError),
    /// Integer or floating-point division by zero (`x / 0`).
    DivisionByZero,
    /// Signed 64-bit integer arithmetic overflow (`i64::MIN` negation/division or `+`/`-`/`*` overflow).
    IntegerOverflow,
    /// Operand type mismatch for unary/binary operators or boolean condition slots.
    TypeError(String),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPath(p) => write!(f, "Unknown template path `{p}` in Registry"),
            Self::UnknownSignal(p) => write!(f, "Unknown or read-only signal `{p}` in Registry"),
            Self::CallFailed(e) => write!(f, "{e}"),
            Self::DivisionByZero => f.write_str("Template expression division by zero"),
            Self::IntegerOverflow => f.write_str("Template expression integer arithmetic overflow"),
            Self::TypeError(msg) => write!(f, "Template expression type error: {msg}"),
        }
    }
}

impl std::error::Error for EvalError {}

/// Scope overlay that binds the `<for each="{item in list}">` item variable `item_var` to a
/// reactive `Signal<Value>` while delegating all other lookups and calls to `parent`.
pub struct ForItemScopeRegistry {
    parent: RegistryRef,
    item_var: String,
    item_sig: Signal<Value>,
}

impl ForItemScopeRegistry {
    /// Creates a new `ForItemScopeRegistry` overlaying `item_var` on top of `parent`.
    pub fn new(parent: RegistryRef, item_var: impl Into<String>, item_sig: Signal<Value>) -> Self {
        Self {
            parent,
            item_var: item_var.into(),
            item_sig,
        }
    }

    fn matches_head<'a>(&self, path: &'a str) -> Option<&'a str> {
        let trimmed = path.trim();
        if trimmed == self.item_var {
            Some("")
        } else {
            trimmed
                .strip_prefix(&self.item_var)
                .and_then(|rest| rest.strip_prefix('.'))
        }
    }
}

impl Registry for ForItemScopeRegistry {
    fn get(&self, path: &str) -> Option<Value> {
        if let Some(tail) = self.matches_head(path) {
            let current = self.item_sig.get();
            if tail.is_empty() {
                return Some(current);
            }
            if let Value::Record(rec) = current {
                return rec.get(tail);
            }
            return None;
        }
        self.parent.get(path)
    }

    fn signal(&self, path: &str) -> Option<DynSignal> {
        if let Some(tail) = self.matches_head(path) {
            if tail.is_empty() {
                return Some(DynSignal::from_signal(self.item_sig));
            }
            if let Value::Record(rec) = self.item_sig.get() {
                return rec.signal(tail);
            }
            return None;
        }
        self.parent.signal(path)
    }

    fn call(&self, name: &str, args: &[Value]) -> Result<Value, CallError> {
        if let Some(tail) = self.matches_head(name) {
            if !tail.is_empty() {
                if let Value::Record(rec) = self.item_sig.get() {
                    return rec.call(tail, args);
                }
            }
        }
        self.parent.call(name, args)
    }

    fn validate_call(&self, name: &str, args: &[Value]) -> Result<Value, CallError> {
        if let Some(tail) = self.matches_head(name) {
            if !tail.is_empty() {
                if let Value::Record(rec) = self.item_sig.get() {
                    return rec.validate_call(tail, args);
                }
            }
        }
        self.parent.validate_call(name, args)
    }
}

/// Strictly evaluates a `TemplateExpr` AST against `registry`, returning an [`EvalError`] on
/// missing paths, failed calls, type mismatches, integer overflow (`i64::MIN` neg/div), or division by zero.
pub fn try_eval_template_expr(
    expr: &TemplateExpr,
    registry: &dyn Registry,
) -> Result<Value, EvalError> {
    eval_template_expr_inner(expr, registry, false)
}

/// Strictly validates a `TemplateExpr` AST against `registry` without executing side-effecting
/// controller methods (`Registry::validate_call`).
pub fn validate_template_expr(
    expr: &TemplateExpr,
    registry: &dyn Registry,
) -> Result<Value, EvalError> {
    eval_template_expr_inner(expr, registry, true)
}

fn eval_template_expr_inner(
    expr: &TemplateExpr,
    registry: &dyn Registry,
    validate_only: bool,
) -> Result<Value, EvalError> {
    match expr {
        TemplateExpr::Path(segments) => {
            let joined = segments.join(".");
            registry.get(&joined).ok_or(EvalError::UnknownPath(joined))
        }
        TemplateExpr::EnumVariant(segments) => {
            Ok(Value::Str(segments.last().cloned().unwrap_or_default()))
        }
        TemplateExpr::Literal(lit) => Ok(match lit {
            TemplateLit::Bool(b) => Value::Bool(*b),
            TemplateLit::Int(n) => Value::Int(*n),
            TemplateLit::Float(v) => Value::Float(*v),
            TemplateLit::Str(s) => Value::Str(s.clone()),
        }),
        TemplateExpr::Unary { op, expr: inner } => eval_unary_op(
            *op,
            &eval_template_expr_inner(inner, registry, validate_only)?,
        ),
        TemplateExpr::Binary { op, left, right } => {
            eval_binary_op(*op, left, right, registry, validate_only)
        }
        TemplateExpr::Call {
            receiver,
            method,
            args,
        } => eval_call_expr(receiver, method, args, registry, validate_only),
    }
}

fn eval_unary_op(op: UnaryOp, val: &Value) -> Result<Value, EvalError> {
    match op {
        UnaryOp::Not => Ok(Value::Bool(!try_eval_bool(val)?)),
        UnaryOp::Neg => match val {
            Value::Int(n) => n
                .checked_neg()
                .map(Value::Int)
                .ok_or(EvalError::IntegerOverflow),
            Value::Float(v) => Ok(Value::Float(-v)),
            other => Err(EvalError::TypeError(format!(
                "unary `-` requires Int or Float, got {other:?}"
            ))),
        },
    }
}

fn eval_call_expr(
    receiver: &[String],
    method: &str,
    args: &[TemplateExpr],
    registry: &dyn Registry,
    validate_only: bool,
) -> Result<Value, EvalError> {
    let mut full_name = receiver.join(".");
    if !full_name.is_empty() {
        full_name.push('.');
    }
    full_name.push_str(method);
    let mut evaluated_args = Vec::with_capacity(args.len());
    for a in args {
        evaluated_args.push(eval_template_expr_inner(a, registry, validate_only)?);
    }
    let res = if validate_only {
        registry.validate_call(&full_name, &evaluated_args)
    } else {
        registry.call(&full_name, &evaluated_args)
    };
    res.map_err(EvalError::CallFailed)
}

/// Evaluates a `TemplateExpr` AST against `registry`.
///
/// Calling `eval_template_expr` inside an `Effect` or `Computed` subscribes to all reactive
/// signals accessed along the expression path.
pub fn eval_template_expr(expr: &TemplateExpr, registry: &dyn Registry) -> Value {
    try_eval_template_expr(expr, registry).unwrap_or(Value::Unit)
}

fn eval_binary_op(
    op: BinaryOp,
    left: &TemplateExpr,
    right: &TemplateExpr,
    registry: &dyn Registry,
    validate_only: bool,
) -> Result<Value, EvalError> {
    match op {
        BinaryOp::And => {
            let l = try_eval_bool(&eval_template_expr_inner(left, registry, validate_only)?)?;
            if !l {
                if validate_only {
                    let _ =
                        try_eval_bool(&eval_template_expr_inner(right, registry, validate_only)?)?;
                }
                Ok(Value::Bool(false))
            } else {
                let r = try_eval_bool(&eval_template_expr_inner(right, registry, validate_only)?)?;
                Ok(Value::Bool(r))
            }
        }
        BinaryOp::Or => {
            let l = try_eval_bool(&eval_template_expr_inner(left, registry, validate_only)?)?;
            if l {
                if validate_only {
                    let _ =
                        try_eval_bool(&eval_template_expr_inner(right, registry, validate_only)?)?;
                }
                Ok(Value::Bool(true))
            } else {
                let r = try_eval_bool(&eval_template_expr_inner(right, registry, validate_only)?)?;
                Ok(Value::Bool(r))
            }
        }
        _ => {
            let lv = eval_template_expr_inner(left, registry, validate_only)?;
            let rv = eval_template_expr_inner(right, registry, validate_only)?;
            eval_eager_binary(op, &lv, &rv)
        }
    }
}

fn eval_eager_binary(op: BinaryOp, lv: &Value, rv: &Value) -> Result<Value, EvalError> {
    match op {
        BinaryOp::Eq => {
            ensure_comparable_eq(lv, rv)?;
            Ok(Value::Bool(lv == rv))
        }
        BinaryOp::Ne => {
            ensure_comparable_eq(lv, rv)?;
            Ok(Value::Bool(lv != rv))
        }
        BinaryOp::Lt => Ok(Value::Bool(compare_values(lv, rv)?.is_lt())),
        BinaryOp::Le => Ok(Value::Bool(compare_values(lv, rv)?.is_le())),
        BinaryOp::Gt => Ok(Value::Bool(compare_values(lv, rv)?.is_gt())),
        BinaryOp::Ge => Ok(Value::Bool(compare_values(lv, rv)?.is_ge())),
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
            eval_numeric_or_string_binary(op, lv, rv)
        }
        BinaryOp::And | BinaryOp::Or => unreachable!(),
    }
}

fn eval_numeric_or_string_binary(op: BinaryOp, lv: &Value, rv: &Value) -> Result<Value, EvalError> {
    match op {
        BinaryOp::Add => match (lv, rv) {
            (Value::Int(a), Value::Int(b)) => a
                .checked_add(*b)
                .map(Value::Int)
                .ok_or(EvalError::IntegerOverflow),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
            (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{a}{b}"))),
            (Value::Str(a), b @ (Value::Int(_) | Value::Float(_) | Value::Bool(_))) => {
                Ok(Value::Str(format!("{a}{b}")))
            }
            (a @ (Value::Int(_) | Value::Float(_) | Value::Bool(_)), Value::Str(b)) => {
                Ok(Value::Str(format!("{a}{b}")))
            }
            _ => Err(EvalError::TypeError(format!(
                "`+` requires matching numeric or string operands, got {lv:?} and {rv:?}"
            ))),
        },
        BinaryOp::Sub => match (lv, rv) {
            (Value::Int(a), Value::Int(b)) => a
                .checked_sub(*b)
                .map(Value::Int)
                .ok_or(EvalError::IntegerOverflow),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
            _ => Err(EvalError::TypeError(format!(
                "`-` requires numeric operands, got {lv:?} and {rv:?}"
            ))),
        },
        BinaryOp::Mul => match (lv, rv) {
            (Value::Int(a), Value::Int(b)) => a
                .checked_mul(*b)
                .map(Value::Int)
                .ok_or(EvalError::IntegerOverflow),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * *b as f64)),
            _ => Err(EvalError::TypeError(format!(
                "`*` requires numeric operands, got {lv:?} and {rv:?}"
            ))),
        },
        BinaryOp::Div => eval_safe_div_or_rem(lv, rv, false),
        BinaryOp::Rem => eval_safe_div_or_rem(lv, rv, true),
        _ => unreachable!(),
    }
}

fn eval_safe_div_or_rem(lv: &Value, rv: &Value, is_rem: bool) -> Result<Value, EvalError> {
    if let (Value::Int(a), Value::Int(b)) = (lv, rv) {
        if *b == 0 {
            return Ok(Value::Int(0));
        }
        let out = if is_rem {
            a.wrapping_rem(*b)
        } else {
            a.wrapping_div(*b)
        };
        return Ok(Value::Int(out));
    }
    let op_str = if is_rem { "%" } else { "/" };
    let l = lv.as_f64().ok_or_else(|| {
        EvalError::TypeError(format!(
            "`{op_str}` requires numeric operands, got {lv:?} and {rv:?}"
        ))
    })?;
    let r = rv.as_f64().ok_or_else(|| {
        EvalError::TypeError(format!(
            "`{op_str}` requires numeric operands, got {lv:?} and {rv:?}"
        ))
    })?;
    if r == 0.0 || !r.is_finite() {
        Ok(Value::Float(0.0))
    } else if is_rem {
        Ok(Value::Float(l % r))
    } else {
        Ok(Value::Float(l / r))
    }
}

fn ensure_comparable_eq(lv: &Value, rv: &Value) -> Result<(), EvalError> {
    match (lv, rv) {
        (Value::Bool(_), Value::Bool(_))
        | (Value::Int(_), Value::Int(_))
        | (Value::Float(_), Value::Float(_))
        | (Value::Int(_), Value::Float(_))
        | (Value::Float(_), Value::Int(_))
        | (Value::Str(_), Value::Str(_)) => Ok(()),
        _ => Err(EvalError::TypeError(format!(
            "cannot compare {lv:?} and {rv:?} for equality"
        ))),
    }
}

fn compare_values(lv: &Value, rv: &Value) -> Result<std::cmp::Ordering, EvalError> {
    match (lv, rv) {
        (Value::Int(a), Value::Int(b)) => Ok(a.cmp(b)),
        (Value::Str(a), Value::Str(b)) => Ok(a.cmp(b)),
        (Value::Float(a), Value::Float(b)) => a
            .partial_cmp(b)
            .ok_or_else(|| EvalError::TypeError("NaN comparison".to_string())),
        (Value::Int(a), Value::Float(b)) => (*a as f64)
            .partial_cmp(b)
            .ok_or_else(|| EvalError::TypeError("NaN comparison".to_string())),
        (Value::Float(a), Value::Int(b)) => a
            .partial_cmp(&(*b as f64))
            .ok_or_else(|| EvalError::TypeError("NaN comparison".to_string())),
        _ => Err(EvalError::TypeError(format!(
            "relational comparison requires numeric or string operands, got {lv:?} and {rv:?}"
        ))),
    }
}

/// Strictly extracts a boolean from `val` for `<if test>`, `disabled`, `!`, `&&`, and `||`.
pub fn try_eval_bool(val: &Value) -> Result<bool, EvalError> {
    match val {
        Value::Bool(b) => Ok(*b),
        other => Err(EvalError::TypeError(format!(
            "expected Bool in boolean condition, got {other:?}"
        ))),
    }
}

/// Evaluates truthiness of a runtime `Value` for `<if test>` and `disabled`.
pub fn eval_truthy(val: &Value) -> bool {
    try_eval_bool(val).unwrap_or(false)
}

/// Evaluates a slice of `CompiledTextPart`s against `registry`, sanitizing dynamic segments.
pub fn eval_interpolated_parts(
    parts: &[CompiledTextPart],
    registry: &dyn Registry,
    sanitize_dynamic: bool,
) -> String {
    let mut buf = String::new();
    for part in parts {
        match part {
            CompiledTextPart::Static(s) => buf.push_str(s),
            CompiledTextPart::Dynamic { expr, .. } => {
                let raw = eval_template_expr(expr, registry).to_string();
                if sanitize_dynamic {
                    buf.push_str(&sanitize_untrusted_text(&raw));
                } else {
                    buf.push_str(&raw);
                }
            }
        }
    }
    buf
}

/// Resolves a `DynSignal` for a `TemplateExpr::Path` if one exists in `registry`.
pub fn resolve_signal_from_expr(expr: &TemplateExpr, registry: &dyn Registry) -> Option<DynSignal> {
    if let TemplateExpr::Path(segments) = expr {
        let joined = segments.join(".");
        registry.signal(&joined)
    } else {
        None
    }
}
