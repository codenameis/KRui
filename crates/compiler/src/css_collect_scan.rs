//! Low-level CSS lexical scanning, delimiter matching, and component `:where(...)` scoping helpers
//! used by `css_collect.rs`.

pub(crate) fn wrap_component_scope(scope: &str, css: &str) -> String {
    let scoped = scope_css_rule_block(scope, css.trim());
    let trimmed = scoped.trim();
    if trimmed.is_empty() {
        String::from("@layer component {\n}")
    } else {
        format!("@layer component {{\n{trimmed}\n}}")
    }
}

fn scope_css_rule_block(scope: &str, css: &str) -> String {
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len() + 64);
    let (mut i, mut stmt_start, mut quote) = (0usize, 0usize, None::<u8>);
    let (mut paren_depth, mut bracket_depth) = (0usize, 0usize);

    while i < bytes.len() {
        if let Some(q) = quote {
            i = advance_css_string(bytes, i, q, &mut quote);
            continue;
        }
        if css[i..].starts_with("/*") {
            let Some(end) = css[i + 2..].find("*/") else {
                break;
            };
            i += 4 + end;
            continue;
        }
        match bytes[i] {
            b'"' | b'\'' => quote = Some(bytes[i]),
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b';' if paren_depth == 0 && bracket_depth == 0 => {
                out.push_str(&css[stmt_start..=i]);
                stmt_start = i + 1;
            }
            b'{' if paren_depth == 0 && bracket_depth == 0 => {
                let Some(close) = find_matching_brace(css, i) else {
                    break;
                };
                emit_scoped_brace_rule(scope, &css[stmt_start..i], &css[i + 1..close], &mut out);
                i = close + 1;
                stmt_start = i;
                paren_depth = 0;
                bracket_depth = 0;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.push_str(&css[stmt_start..]);
    out
}

fn emit_scoped_brace_rule(scope: &str, prelude: &str, body: &str, out: &mut String) {
    let lead_end = skip_ws_and_comments_len(prelude);
    let leading = &prelude[..lead_end];
    let trimmed_prelude = prelude[lead_end..].trim_end();
    if trimmed_prelude.starts_with('@') {
        out.push_str(prelude);
        out.push('{');
        if is_container_at_rule(trimmed_prelude) {
            out.push_str(&scope_css_rule_block(scope, body));
        } else {
            out.push_str(body);
        }
        out.push('}');
        return;
    }
    out.push_str(leading);
    out.push_str(&scope_selector_list(scope, trimmed_prelude));
    if prelude.ends_with(char::is_whitespace) && !trimmed_prelude.is_empty() {
        out.push(' ');
    }
    out.push('{');
    out.push_str(body);
    out.push('}');
}

fn is_container_at_rule(prelude: &str) -> bool {
    !is_unscoped_at_rule(prelude)
        && (starts_with_ci(prelude, "@media")
            || starts_with_ci(prelude, "@supports")
            || starts_with_ci(prelude, "@layer")
            || starts_with_ci(prelude, "@container")
            || starts_with_ci(prelude, "@starting-style"))
}

pub(crate) fn skip_ws_and_comments_len(s: &str) -> usize {
    let mut i = 0;
    while i < s.len() {
        if let Some(ch) = s[i..].chars().next() {
            if ch.is_whitespace() {
                i += ch.len_utf8();
                continue;
            }
        }
        if s[i..].starts_with("/*") {
            let Some(end) = s[i + 2..].find("*/") else {
                return s.len();
            };
            i += 4 + end;
            continue;
        }
        break;
    }
    i
}

fn scope_selector_list(scope: &str, prelude: &str) -> String {
    let mut out = String::with_capacity(prelude.len() + 40);
    for (idx, part) in split_top_level_commas(prelude).into_iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&scope_single_selector(scope, part));
    }
    out
}

pub(crate) fn split_top_level_commas(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let (mut parts, mut start, mut i) = (Vec::new(), 0usize, 0usize);
    let (mut pdepth, mut bdepth, mut quote) = (0usize, 0usize, None::<u8>);
    while i < bytes.len() {
        if let Some(q) = quote {
            i = advance_css_string(bytes, i, q, &mut quote);
            continue;
        }
        if s[i..].starts_with("/*") {
            let Some(end) = s[i + 2..].find("*/") else {
                break;
            };
            i += 4 + end;
            continue;
        }
        match bytes[i] {
            b'"' | b'\'' => quote = Some(bytes[i]),
            b'(' => pdepth += 1,
            b')' => pdepth = pdepth.saturating_sub(1),
            b'[' => bdepth += 1,
            b']' => bdepth = bdepth.saturating_sub(1),
            b',' if pdepth == 0 && bdepth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
}

fn scope_single_selector(scope: &str, raw: &str) -> String {
    let lead_end = skip_ws_and_comments_len(raw);
    let leading = &raw[..lead_end];
    let rest = &raw[lead_end..];
    let trimmed = rest.trim_end();
    let trailing = &rest[trimmed.len()..];
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(":root") {
        return raw.to_string();
    }
    let split_at = find_pseudo_element_offset(trimmed).unwrap_or(trimmed.len());
    let (head, tail) = trimmed.split_at(split_at);
    format!("{leading}{head}:where([data-krui-scope~=\"{scope}\"]){tail}{trailing}")
}

fn find_pseudo_element_offset(selector: &str) -> Option<usize> {
    let bytes = selector.as_bytes();
    let (mut i, mut pdepth, mut bdepth, mut quote) = (0usize, 0usize, 0usize, None::<u8>);
    while i < bytes.len() {
        if let Some(q) = quote {
            i = advance_css_string(bytes, i, q, &mut quote);
            continue;
        }
        if selector[i..].starts_with("/*") {
            let Some(end) = selector[i + 2..].find("*/") else {
                break;
            };
            i += 4 + end;
            continue;
        }
        match bytes[i] {
            b'"' | b'\'' => quote = Some(bytes[i]),
            b'(' => pdepth += 1,
            b')' => pdepth = pdepth.saturating_sub(1),
            b'[' => bdepth += 1,
            b']' => bdepth = bdepth.saturating_sub(1),
            b':' if pdepth == 0
                && bdepth == 0
                && (bytes.get(i + 1) == Some(&b':')
                    || is_legacy_single_colon_pseudo(&selector[i..])) =>
            {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn is_legacy_single_colon_pseudo(s: &str) -> bool {
    [":before", ":after", ":first-line", ":first-letter"]
        .iter()
        .any(|kw| {
            starts_with_ci(s, kw)
                && s.as_bytes()
                    .get(kw.len())
                    .is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'-' && *b != b'_')
        })
}

pub(crate) fn advance_css_string(
    bytes: &[u8],
    i: usize,
    active_quote: u8,
    quote: &mut Option<u8>,
) -> usize {
    if bytes[i] == b'\\' && i + 1 < bytes.len() {
        return i + 2;
    }
    if bytes[i] == active_quote {
        *quote = None;
    }
    i + 1
}

pub(crate) fn is_unscoped_at_rule(s: &str) -> bool {
    starts_with_ci(s, "@keyframes")
        || starts_with_ci(s, "@font-face")
        || starts_with_ci(s, "@property")
}

pub(crate) fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.get(..prefix.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
}

pub(crate) fn is_ws_or_quote_or_paren(s: &str) -> bool {
    s.chars()
        .next()
        .is_some_and(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '('))
}

pub(crate) fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.to_ascii_lowercase().find(&needle.to_ascii_lowercase())
}

pub(crate) fn find_ci_at_boundary(hay: &str, needle: &str) -> Option<usize> {
    find_ci(hay, needle)
}

pub(crate) fn find_top_level_semicolon(s: &str, from: usize) -> Option<usize> {
    let (mut i, mut pdepth, mut quote) = (from, 0usize, None::<u8>);
    let b = s.as_bytes();
    while i < b.len() {
        if let Some(q) = quote {
            i = advance_css_string(b, i, q, &mut quote);
            continue;
        }
        match b[i] {
            b'"' | b'\'' => quote = Some(b[i]),
            b'(' => pdepth += 1,
            b')' => pdepth = pdepth.saturating_sub(1),
            b';' if pdepth == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

pub(crate) fn find_closing_quote(s: &str, q: char) -> Option<usize> {
    let qb = q as u8;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            i += 2;
            continue;
        }
        if b[i] == qb {
            return Some(i);
        }
        i += 1;
    }
    None
}

pub(crate) fn find_matching_paren(s: &str, open_idx: usize) -> Option<usize> {
    find_matching_delim(s, open_idx, b'(', b')')
}

pub(crate) fn find_matching_brace(s: &str, open_idx: usize) -> Option<usize> {
    find_matching_delim(s, open_idx, b'{', b'}')
}

pub(crate) fn find_matching_delim(s: &str, open_idx: usize, open: u8, close: u8) -> Option<usize> {
    let b = s.as_bytes();
    let (mut i, mut depth, mut quote) = (open_idx + 1, 1usize, None::<u8>);
    while i < b.len() {
        if let Some(q) = quote {
            i = advance_css_string(b, i, q, &mut quote);
            continue;
        }
        if b[i] == b'"' || b[i] == b'\'' {
            quote = Some(b[i]);
        } else if b[i] == open {
            depth += 1;
        } else if b[i] == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}
