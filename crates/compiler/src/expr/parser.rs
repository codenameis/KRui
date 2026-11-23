//! Recursive-descent parser for `.krui` `{...}` template expressions.

use super::{BinaryOp, TemplateExpr, TemplateLit, UnaryOp};
use crate::interpolation::is_valid_rust_ident;
use crate::CompileError;

/// Parses and validates a `.krui` template expression string into a [`TemplateExpr`] AST.
///
/// Any syntax outside KRui's template expression grammar returns a [`CompileError`] with the
/// exact source `(line, column)` derived from `base_offset`.
pub fn parse_template_expr(
    source: &str,
    base_offset: usize,
    raw: &str,
) -> Result<TemplateExpr, CompileError> {
    let lead_ws = raw.len() - raw.trim_start().len();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CompileError::at_offset(
            source,
            base_offset,
            "Empty `{}` binding expression is not allowed",
        ));
    }
    let mut parser = ExprParser {
        source,
        base_offset: base_offset + lead_ws,
        input: trimmed,
        pos: 0,
    };
    let expr = parser.parse_or()?;
    parser.skip_ws();
    if parser.pos < parser.input.len() {
        return Err(parser.err_here(format!(
            "Unexpected token `{}` in template expression `{{{trimmed}}}`",
            &parser.input[parser.pos..]
        )));
    }
    Ok(expr)
}

struct ExprParser<'a> {
    source: &'a str,
    base_offset: usize,
    input: &'a str,
    pos: usize,
}

impl<'a> ExprParser<'a> {
    fn err_at(&self, rel_pos: usize, msg: impl Into<String>) -> CompileError {
        CompileError::at_offset(self.source, self.base_offset + rel_pos, msg)
    }

    fn err_here(&self, msg: impl Into<String>) -> CompileError {
        self.err_at(self.pos, msg)
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.rest().chars().next() {
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn rest(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn eat(&mut self, prefix: &str) -> bool {
        self.skip_ws();
        if self.rest().starts_with(prefix) {
            self.pos += prefix.len();
            true
        } else {
            false
        }
    }

    fn parse_or(&mut self) -> Result<TemplateExpr, CompileError> {
        let mut left = self.parse_and()?;
        while self.eat("||") {
            let right = self.parse_and()?;
            left = TemplateExpr::Binary {
                op: BinaryOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<TemplateExpr, CompileError> {
        let mut left = self.parse_cmp()?;
        while self.eat("&&") {
            let right = self.parse_cmp()?;
            left = TemplateExpr::Binary {
                op: BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_cmp(&mut self) -> Result<TemplateExpr, CompileError> {
        let mut left = self.parse_add()?;
        let ops = [
            ("==", BinaryOp::Eq),
            ("!=", BinaryOp::Ne),
            ("<=", BinaryOp::Le),
            (">=", BinaryOp::Ge),
            ("<", BinaryOp::Lt),
            (">", BinaryOp::Gt),
        ];
        loop {
            self.skip_ws();
            let Some((_, op)) = ops.iter().find(|(tok, _)| self.eat(tok)) else {
                break;
            };
            let right = self.parse_add()?;
            left = TemplateExpr::Binary {
                op: *op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_add(&mut self) -> Result<TemplateExpr, CompileError> {
        let mut left = self.parse_mul()?;
        loop {
            self.skip_ws();
            if self.rest().starts_with("->") {
                break;
            }
            let op = if self.eat("+") {
                BinaryOp::Add
            } else if self.eat("-") {
                BinaryOp::Sub
            } else {
                break;
            };
            let right = self.parse_mul()?;
            left = TemplateExpr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<TemplateExpr, CompileError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = if self.eat("*") {
                BinaryOp::Mul
            } else if self.eat("/") {
                BinaryOp::Div
            } else if self.eat("%") {
                BinaryOp::Rem
            } else {
                break;
            };
            let right = self.parse_unary()?;
            left = TemplateExpr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<TemplateExpr, CompileError> {
        self.skip_ws();
        if !self.rest().starts_with("!=") && self.eat("!") {
            let expr = self.parse_unary()?;
            return Ok(TemplateExpr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            });
        }
        if !self.rest().starts_with("->") && self.eat("-") {
            let expr = self.parse_unary()?;
            return Ok(TemplateExpr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(expr),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<TemplateExpr, CompileError> {
        self.skip_ws();
        let Some(ch) = self.rest().chars().next() else {
            return Err(self.err_here(format!(
                "Unexpected end of template expression `{{{}}}`",
                self.input
            )));
        };
        if ch == '|' {
            return Err(self.err_here(format!(
                "Closures are forbidden in `.krui` template expressions (`{{{}}}`); move logic to a ViewModel `Computed` or method",
                self.input
            )));
        }
        if ch == '{' || ch == '}' {
            return Err(self.err_here(format!(
                "Block expressions are forbidden in `.krui` template expressions (`{{{}}}`)",
                self.input
            )));
        }
        if ch == '(' {
            let open_pos = self.pos;
            self.pos += 1;
            let inner = self.parse_or()?;
            if !self.eat(")") {
                return Err(self.err_at(
                    open_pos,
                    format!("Unclosed `(` in template expression `{{{}}}`", self.input),
                ));
            }
            return Ok(inner);
        }
        if ch == '"' || ch == '\'' {
            return self.parse_string_literal(ch);
        }
        if ch.is_ascii_digit() {
            return self.parse_number_literal();
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            return self.parse_ident_or_path_or_call();
        }
        Err(self.err_here(format!(
            "Unsupported syntax `{}` in `.krui` template expression `{{{}}}`",
            self.rest(),
            self.input
        )))
    }

    fn parse_string_literal(&mut self, quote_ch: char) -> Result<TemplateExpr, CompileError> {
        let start = self.pos;
        self.pos += quote_ch.len_utf8();
        let mut out = String::new();
        while let Some(ch) = self.rest().chars().next() {
            self.pos += ch.len_utf8();
            if ch == quote_ch {
                return Ok(TemplateExpr::Literal(TemplateLit::Str(out)));
            }
            if ch == '\\' {
                let Some(esc) = self.rest().chars().next() else {
                    return Err(self.err_at(start, "Unterminated escape in string literal"));
                };
                self.pos += esc.len_utf8();
                out.push(match esc {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                });
            } else {
                out.push(ch);
            }
        }
        Err(self.err_at(
            start,
            format!("Unclosed string literal `{quote_ch}` in template expression"),
        ))
    }

    fn parse_number_literal(&mut self) -> Result<TemplateExpr, CompileError> {
        let start = self.pos;
        let mut is_float = false;
        while let Some(ch) = self.rest().chars().next() {
            if ch.is_ascii_digit() || ch == '_' {
                self.pos += 1;
            } else if ch == '.'
                && !is_float
                && self.rest()[1..].starts_with(|c: char| c.is_ascii_digit())
            {
                is_float = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        let clean = self.input[start..self.pos].replace('_', "");
        if is_float {
            let v = clean
                .parse::<f64>()
                .map_err(|_| self.err_at(start, format!("Invalid float literal `{clean}`")))?;
            Ok(TemplateExpr::Literal(TemplateLit::Float(v)))
        } else {
            let n = clean
                .parse::<i64>()
                .map_err(|_| self.err_at(start, format!("Invalid integer literal `{clean}`")))?;
            Ok(TemplateExpr::Literal(TemplateLit::Int(n)))
        }
    }

    fn read_raw_word(&mut self) -> &'a str {
        let start = self.pos;
        while let Some(ch) = self.rest().chars().next() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        &self.input[start..self.pos]
    }

    fn parse_ident_or_path_or_call(&mut self) -> Result<TemplateExpr, CompileError> {
        let start = self.pos;
        let first = self.read_raw_word();
        match first {
            "true" => return Ok(TemplateExpr::Literal(TemplateLit::Bool(true))),
            "false" => return Ok(TemplateExpr::Literal(TemplateLit::Bool(false))),
            "match" | "if" | "else" | "for" | "while" | "loop" | "let" | "fn" | "move"
            | "unsafe" => {
                return Err(self.err_at(
                    start,
                    format!(
                        "Rust keyword `{first}` is forbidden in `.krui` template expressions (`{{{}}}`)",
                        self.input
                    ),
                ));
            }
            _ => {}
        }
        if self.rest().starts_with("::") {
            return self.parse_enum_variant_tail(start, first);
        }
        if !is_valid_rust_ident(first) {
            return Err(self.err_at(
                start,
                format!(
                    "Invalid identifier `{first}` in template expression `{{{}}}`",
                    self.input
                ),
            ));
        }
        self.parse_dot_path_or_method_call(start, first)
    }

    fn parse_enum_variant_tail(
        &mut self,
        start: usize,
        first: &str,
    ) -> Result<TemplateExpr, CompileError> {
        let mut segments = vec![first.to_string()];
        while self.rest().starts_with("::") {
            self.pos += 2;
            let seg_pos = self.pos;
            let seg = self.read_raw_word();
            if !is_valid_rust_ident(seg) {
                return Err(self.err_at(
                    seg_pos,
                    format!("Expected identifier after `::` in `{{{}}}`", self.input),
                ));
            }
            segments.push(seg.to_string());
        }
        self.skip_ws();
        if self.rest().starts_with('(') {
            return Err(self.err_at(
                start,
                format!(
                    "Arbitrary function call `{}()` is forbidden in `.krui` template expressions (`{{{}}}`)",
                    segments.join("::"),
                    self.input
                ),
            ));
        }
        Ok(TemplateExpr::EnumVariant(segments))
    }

    fn parse_dot_path_or_method_call(
        &mut self,
        start: usize,
        first: &str,
    ) -> Result<TemplateExpr, CompileError> {
        let mut segments = vec![first.to_string()];
        while self.rest().starts_with('.') {
            self.pos += 1;
            let seg_pos = self.pos;
            let seg = self.read_raw_word();
            if !is_valid_rust_ident(seg) {
                return Err(self.err_at(
                    seg_pos,
                    format!(
                        "Expected field or method name after `.` in `{{{}}}`",
                        self.input
                    ),
                ));
            }
            segments.push(seg.to_string());
        }
        self.skip_ws();
        if self.rest().starts_with(':') {
            return Err(self.err_here(format!(
                "Invalid binding expression `{{{}}}` (`frame:` prefix is only valid inside CSS declarations)",
                self.input
            )));
        }
        if self.rest().starts_with('(') {
            return self.parse_method_call_tail(start, first, segments);
        }
        Ok(TemplateExpr::Path(segments))
    }

    fn parse_method_call_tail(
        &mut self,
        start: usize,
        first: &str,
        mut segments: Vec<String>,
    ) -> Result<TemplateExpr, CompileError> {
        if segments.len() < 2 {
            return Err(self.err_at(
                start,
                format!(
                    "Standalone function call `{first}(...)` is forbidden in `.krui` template expressions; call a registered method on a ViewModel, Controller, or Store (e.g. `ctrl.{first}(...)`)"
                ),
            ));
        }
        self.pos += 1;
        let mut args = Vec::new();
        self.skip_ws();
        if !self.eat(")") {
            loop {
                args.push(self.parse_or()?);
                if self.eat(",") {
                    if self.eat(")") {
                        break;
                    }
                } else if self.eat(")") {
                    break;
                } else {
                    return Err(self.err_here(format!(
                        "Expected `,` or `)` in method call arguments inside `{{{}}}`",
                        self.input
                    )));
                }
            }
        }
        let method = segments.pop().unwrap_or_default();
        Ok(TemplateExpr::Call {
            receiver: segments,
            method,
            args,
        })
    }
}
