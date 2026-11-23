//! Single-pass HTML entity decoder and `{expr}` / `{{` / `}}` text interpolation parser.

use crate::expr::{parse_template_expr, TemplateExpr};
use crate::html_lexer::scan_template_expr_hole;
use crate::{CompileError, CompiledTextPart};

/// Validates that `ident` is a valid Rust identifier (`[a-zA-Z_][a-zA-Z0-9_]*` and not a strict Rust keyword).
pub fn is_valid_rust_ident(ident: &str) -> bool {
    let trimmed = ident.trim();
    if trimmed.is_empty() || trimmed == "_" {
        return false;
    }
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    !matches!(
        trimmed,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
    )
}

/// Extracts a required `{expr}` binding from an attribute value (`onclick="{...}"`, `bind:value="{...}"`).
/// Rejects unbraced attribute values (`onclick="vm.save"`).
pub fn require_braced_binding<'a>(
    source: &str,
    attr_offset: usize,
    attr_name: &str,
    raw: &'a str,
) -> Result<&'a str, CompileError> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err(CompileError::at_offset(
            source,
            attr_offset,
            format!(
                "Attribute `{attr_name}=\"{raw}\"` requires `{{...}}` braces (use `{attr_name}=\"{{{trimmed}}}\"`)"
            ),
        ));
    }
    let close_idx = scan_template_expr_hole(trimmed, 0)
        .map_err(|msg| CompileError::at_offset(source, attr_offset, msg))?;
    if close_idx + 1 != trimmed.len() {
        return Err(CompileError::at_offset(
            source,
            attr_offset,
            format!(
                "Attribute `{attr_name}=\"{raw}\"` must contain a single `{{...}}` binding expression"
            ),
        ));
    }
    let inner = &trimmed[1..close_idx];
    let clean = inner.trim();
    let _ = parse_template_expr(source, attr_offset, clean)?;
    Ok(clean)
}

/// Extracts and parses a required `{expr}` attribute binding into a [`TemplateExpr`].
pub fn require_braced_template_expr(
    source: &str,
    attr_offset: usize,
    attr_name: &str,
    raw: &str,
) -> Result<(String, TemplateExpr), CompileError> {
    let clean = require_braced_binding(source, attr_offset, attr_name, raw)?;
    let ast = parse_template_expr(source, attr_offset, clean)?;
    Ok((clean.to_string(), ast))
}

/// Decodes HTML character entities in a single pass so `&amp;lt;` decodes to `&lt;` (never `<`).
pub fn decode_html_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after_amp = &rest[amp + 1..];
        if let Some(semi) = after_amp.find(';') {
            let entity = &after_amp[..semi];
            if let Some(decoded) = resolve_html_entity(entity) {
                out.push(decoded);
                rest = &after_amp[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after_amp;
    }
    out.push_str(rest);
    out
}

fn resolve_html_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        "nbsp" => Some('\u{00A0}'),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => {
            let code = u32::from_str_radix(&entity[2..], 16).ok()?;
            char::from_u32(code)
        }
        _ if entity.starts_with('#') => {
            let code = entity[1..].parse::<u32>().ok()?;
            char::from_u32(code)
        }
        _ => None,
    }
}

/// Extracts a single `{expr}` hole (with optional `frame:` prefix) from a CSS declaration value.
pub fn extract_single_hole(val: &str) -> Result<Option<(String, TemplateExpr)>, CompileError> {
    let trimmed = val.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Ok(None);
    }
    let Ok(close_idx) = scan_template_expr_hole(trimmed, 0) else {
        return Ok(None);
    };
    if close_idx + 1 != trimmed.len() {
        return Ok(None);
    }
    let inner = trimmed[1..close_idx].trim();
    if inner.is_empty() {
        return Ok(None);
    }
    let clean = inner.strip_prefix("frame:").unwrap_or(inner).trim();
    let ast = parse_template_expr(clean, 0, clean)?;
    Ok(Some((clean.to_string(), ast)))
}

/// Extracts a `{expr}px` or `{expr}` hole from a pixel-typed CSS declaration value.
pub fn extract_px_hole(val: &str) -> Result<Option<(String, TemplateExpr)>, CompileError> {
    let trimmed = val.trim();
    let without_px = trimmed.strip_suffix("px").unwrap_or(trimmed).trim();
    extract_single_hole(without_px)
}

/// Validates that `expr` parses under KRui's template expression grammar.
pub fn validate_binding_expr(source: &str, offset: usize, expr: &str) -> Result<(), CompileError> {
    let _ = is_valid_rust_ident(expr);
    let _ = parse_template_expr(source, offset, expr)?;
    Ok(())
}

/// Strictly parses text containing `{expr}` interpolation holes and `{{` / `}}` escaped braces.
/// Rejects unclosed `{` or stray `}` with a `CompileError`.
pub fn parse_interpolated_text_strict(
    source: &str,
    base_offset: usize,
    input: &str,
) -> Result<Vec<CompiledTextPart>, CompileError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let mut parts = Vec::new();
    let mut static_buf = String::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'{' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                static_buf.push('{');
                i += 2;
                continue;
            }
            if !static_buf.is_empty() {
                let decoded = decode_html_entities(&std::mem::take(&mut static_buf));
                parts.push(CompiledTextPart::Static(decoded));
            }
            i = consume_interpolation_hole(source, base_offset, input, i, &mut parts)?;
        } else if bytes[i] == b'}' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                static_buf.push('}');
                i += 2;
                continue;
            }
            return Err(CompileError::at_offset(
                source,
                base_offset + i,
                format!(
                    "Unmatched closing `}}` in text `{input}` (use `}}}}` for a literal brace)"
                ),
            ));
        } else {
            let ch = input[i..].chars().next().unwrap_or(' ');
            static_buf.push(ch);
            i += ch.len_utf8();
        }
    }

    if !static_buf.is_empty() {
        parts.push(CompiledTextPart::Static(decode_html_entities(&static_buf)));
    }
    Ok(parts)
}

fn consume_interpolation_hole(
    source: &str,
    base_offset: usize,
    input: &str,
    open_idx: usize,
    parts: &mut Vec<CompiledTextPart>,
) -> Result<usize, CompileError> {
    let close_idx = scan_template_expr_hole(input, open_idx).map_err(|msg| {
        CompileError::at_offset(
            source,
            base_offset + open_idx,
            format!("{msg} in `{input}`"),
        )
    })?;
    let expr_start = open_idx + 1;
    let expr_raw = &input[expr_start..close_idx];
    let expr = expr_raw.trim();
    let ast = parse_template_expr(source, base_offset + expr_start, expr_raw)?;
    parts.push(CompiledTextPart::Dynamic {
        raw: expr.to_string(),
        expr: ast,
    });
    Ok(close_idx + 1)
}
