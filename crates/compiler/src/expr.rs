//! Restricted template expression AST (`TemplateExpr`) and recursive-descent parser for `.krui` `{...}` bindings.
//!
//! Per `AGENTS.md` Section 3:
//! - Allowed: paths (`vm.cart.total`), literals (`"str"`, `'str'`, `42`, `3.5`, `true`, `false`),
//!   unary (`!`, `-`), binary (`||`, `&&`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `+`, `-`, `*`, `/`, `%`),
//!   parentheses `( ... )`, `<for>` item variables, registered method/command calls (`ctrl.buy(5)`,
//!   `store.on(Msg::Add)`), and enum variant paths (`Msg::Add`).
//! - Forbidden: closures (`|x| ...`), blocks (`{ ... }`), `match`, `if`, indexing (`a[0]`),
//!   arbitrary standalone function calls (`foo(1)`), or Rust syntax outside the template grammar.

mod parser;

pub use parser::parse_template_expr;

use std::collections::HashMap;
use std::fmt;

/// Literal value inside a [`TemplateExpr`].
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum TemplateLit {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// Unary operator in a [`TemplateExpr`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum UnaryOp {
    Not,
    Neg,
}

/// Binary operator in a [`TemplateExpr`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum BinaryOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

/// Validated AST for a `.krui` `{...}` template expression.
///
/// Shared between compile-time proc-macro code generation (`krui-macros`) and the
/// live-preview runtime interpreter (`krui-preview`).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum TemplateExpr {
    /// Dot-separated identifier path (e.g., `vm.cart.total`, `item.title`, `header_shake`).
    Path(Vec<String>),
    /// `::`-separated enum variant path (e.g., `TelemetryAction::ScaleOutNodes`).
    EnumVariant(Vec<String>),
    /// Scalar literal (`"hello"`, `42`, `3.5`, `true`, `false`).
    Literal(TemplateLit),
    /// Unary operation (`!expr` or `-expr`).
    Unary {
        op: UnaryOp,
        expr: Box<TemplateExpr>,
    },
    /// Binary operation (`a && b`, `a == b`, `a + b`, `a % b`, etc.).
    Binary {
        op: BinaryOp,
        left: Box<TemplateExpr>,
        right: Box<TemplateExpr>,
    },
    /// Method or command invocation on a receiver path (`ctrl.buy(5)`, `store.on(Msg::Add)`).
    Call {
        receiver: Vec<String>,
        method: String,
        args: Vec<TemplateExpr>,
    },
}

impl TemplateExpr {
    /// Collects all unique free root identifiers (first segment of `Path` or `Call.receiver`)
    /// referenced by this expression, excluding any `<for>` loop variables in `exclude_vars`.
    pub fn collect_free_root_idents(&self, exclude_vars: &[String], out: &mut Vec<String>) {
        let mut push_root = |root: &String| {
            if !exclude_vars.iter().any(|ex| ex == root) && !out.iter().any(|e| e == root) {
                out.push(root.clone());
            }
        };
        match self {
            Self::Path(segments) => {
                if let Some(root) = segments.first() {
                    push_root(root);
                }
            }
            Self::EnumVariant(_) | Self::Literal(_) => {}
            Self::Unary { expr, .. } => expr.collect_free_root_idents(exclude_vars, out),
            Self::Binary { left, right, .. } => {
                left.collect_free_root_idents(exclude_vars, out);
                right.collect_free_root_idents(exclude_vars, out);
            }
            Self::Call { receiver, args, .. } => {
                if let Some(root) = receiver.first() {
                    push_root(root);
                }
                for arg in args {
                    arg.collect_free_root_idents(exclude_vars, out);
                }
            }
        }
    }

    /// Substitutes component prop bindings (`props`) for matching root identifiers in `self`.
    pub fn substitute_props(&self, props: &HashMap<String, TemplateExpr>) -> Self {
        match self {
            Self::Path(segs) => substitute_path_segs(segs, props),
            Self::EnumVariant(_) | Self::Literal(_) => self.clone(),
            Self::Unary { op, expr } => Self::Unary {
                op: *op,
                expr: Box::new(expr.substitute_props(props)),
            },
            Self::Binary { op, left, right } => Self::Binary {
                op: *op,
                left: Box::new(left.substitute_props(props)),
                right: Box::new(right.substitute_props(props)),
            },
            Self::Call {
                receiver,
                method,
                args,
            } => {
                let new_recv = match receiver.first().and_then(|r| props.get(r)) {
                    Some(Self::Path(prefix)) => {
                        let mut full = prefix.clone();
                        full.extend_from_slice(&receiver[1..]);
                        full
                    }
                    _ => receiver.clone(),
                };
                Self::Call {
                    receiver: new_recv,
                    method: method.clone(),
                    args: args.iter().map(|a| a.substitute_props(props)).collect(),
                }
            }
        }
    }
}

fn substitute_path_segs(segs: &[String], props: &HashMap<String, TemplateExpr>) -> TemplateExpr {
    let Some(first) = segs.first() else {
        return TemplateExpr::Path(Vec::new());
    };
    let Some(bound) = props.get(first) else {
        return TemplateExpr::Path(segs.to_vec());
    };
    if segs.len() == 1 {
        return bound.clone();
    }
    if let TemplateExpr::Path(prefix) = bound {
        let mut combined = prefix.clone();
        combined.extend_from_slice(&segs[1..]);
        return TemplateExpr::Path(combined);
    }
    TemplateExpr::Path(segs.to_vec())
}

impl fmt::Display for TemplateLit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Str(s) => write_escaped_template_str(f, s),
            Self::Int(n) => write!(f, "{n}"),
            Self::Float(v) => {
                let s = v.to_string();
                if s.contains('.') {
                    f.write_str(&s)
                } else {
                    write!(f, "{s}.0")
                }
            }
            Self::Bool(b) => write!(f, "{b}"),
        }
    }
}

fn write_escaped_template_str(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    f.write_str("\"")?;
    for ch in s.chars() {
        match ch {
            '\\' => f.write_str("\\\\")?,
            '"' => f.write_str("\\\"")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            other => write!(f, "{other}")?,
        }
    }
    f.write_str("\"")
}

impl fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Not => "!",
            Self::Neg => "-",
        })
    }
}

impl fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Or => "||",
            Self::And => "&&",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
        })
    }
}

impl fmt::Display for TemplateExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(segs) => f.write_str(&segs.join(".")),
            Self::EnumVariant(segs) => f.write_str(&segs.join("::")),
            Self::Literal(lit) => write!(f, "{lit}"),
            Self::Unary { op, expr } => write!(f, "{op}{expr}"),
            Self::Binary { op, left, right } => write!(f, "({left} {op} {right})"),
            Self::Call {
                receiver,
                method,
                args,
            } => {
                write!(f, "{}.{method}(", receiver.join("."))?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str(")")
            }
        }
    }
}
