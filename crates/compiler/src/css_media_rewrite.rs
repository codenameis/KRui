//! Rewrites `@media (prefers-reduced-motion: ...)` into root attribute selectors for native
//! targets so `ViewTree::set_reduced_motion_preference` invalidates styles in pure CSS.

use crate::css_collect_scan::{
    advance_css_string, find_matching_brace, skip_ws_and_comments_len, split_top_level_commas,
    starts_with_ci,
};

const REDUCE_ROOT_ATTR: &str = ":where([data-krui-motion=\"reduce\"])";
const REDUCE_ANCESTOR: &str = ":where(:root[data-krui-motion=\"reduce\"]) ";
const NOPREF_ROOT_ATTR: &str = ":where(:not([data-krui-motion=\"reduce\"]))";
const NOPREF_ANCESTOR: &str = ":where(:root:not([data-krui-motion=\"reduce\"])) ";

#[derive(Clone, Copy, PartialEq, Eq)]
enum MotionTarget {
    Reduce,
    NoPreference,
}

/// Rewrites `@media (prefers-reduced-motion: ...)` blocks unless `preserve_raw` is set (web DOM).
pub(crate) fn rewrite_prefers_reduced_motion_css(css: &str, preserve_raw: bool) -> String {
    if preserve_raw || !contains_ci(css, "prefers-reduced-motion") {
        return css.to_string();
    }
    rewrite_segment(css)
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    let hb = haystack.as_bytes();
    let nb = needle.as_bytes();
    hb.windows(nb.len()).any(|w| w.eq_ignore_ascii_case(nb))
}

fn rewrite_segment(css: &str) -> String {
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len() + 64);
    let (mut i, mut cursor, mut quote) = (0_usize, 0_usize, None::<u8>);
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
            b'@' if starts_with_ci(&css[i..], "@media") && at_media_boundary(&css[i + 6..]) => {
                if let Some((end, emitted)) = try_rewrite_media_rule(css, i) {
                    out.push_str(&css[cursor..i]);
                    out.push_str(&emitted);
                    cursor = end;
                    i = end;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out.push_str(&css[cursor..]);
    out
}

fn at_media_boundary(rest: &str) -> bool {
    rest.starts_with(char::is_whitespace) || rest.starts_with('(')
}

fn try_rewrite_media_rule(css: &str, at_pos: usize) -> Option<(usize, String)> {
    let prelude_start = at_pos + 6;
    let brace_open = find_top_open_brace(css, prelude_start)?;
    let brace_close = find_matching_brace(css, brace_open)?;
    let prelude = css[prelude_start..brace_open].trim();
    let raw_body = &css[brace_open + 1..brace_close];
    let body = rewrite_segment(raw_body);
    let (target, remainder) = extract_motion_clause(prelude)?;
    let scoped_body = scope_rules_for_motion(&body, target);
    let emitted = if let Some(rem) = remainder {
        format!("@media {rem} {{{scoped_body}}}")
    } else {
        scoped_body
    };
    Some((brace_close + 1, emitted))
}

fn extract_motion_clause(prelude: &str) -> Option<(MotionTarget, Option<String>)> {
    let bytes = prelude.as_bytes();
    let mut i = 0_usize;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            let close = find_matching_paren_bytes(bytes, i)?;
            let inner = prelude[i + 1..close].trim();
            if let Some(target) = parse_motion_feature(inner) {
                let before = prelude[..i].trim_end();
                let after = prelude[close + 1..].trim_start();
                let rem = combine_media_remainder(before, after)?;
                return Some((target, rem));
            }
            i = close + 1;
            continue;
        }
        i += 1;
    }
    None
}

fn parse_motion_feature(inner: &str) -> Option<MotionTarget> {
    if let Some((k, v)) = inner.split_once(':') {
        if !k.trim().eq_ignore_ascii_case("prefers-reduced-motion") {
            return None;
        }
        return match v.trim().to_ascii_lowercase().as_str() {
            "reduce" => Some(MotionTarget::Reduce),
            "no-preference" => Some(MotionTarget::NoPreference),
            _ => None,
        };
    }
    inner
        .eq_ignore_ascii_case("prefers-reduced-motion")
        .then_some(MotionTarget::Reduce)
}

fn trim_edge_and(s: &str, trailing: bool) -> &str {
    let mut t = s.trim();
    if trailing && t.len() >= 3 && t[t.len() - 3..].eq_ignore_ascii_case("and") {
        let head = &t[..t.len() - 3];
        if head.is_empty() || head.ends_with(char::is_whitespace) || head.ends_with(')') {
            t = head.trim_end();
        }
    }
    if !trailing && t.len() >= 3 && t[..3].eq_ignore_ascii_case("and") {
        let tail = &t[3..];
        if tail.is_empty() || tail.starts_with(char::is_whitespace) || tail.starts_with('(') {
            t = tail.trim_start();
        }
    }
    t
}

fn combine_media_remainder(before: &str, after: &str) -> Option<Option<String>> {
    if before.to_ascii_lowercase().contains("or") || after.to_ascii_lowercase().contains("or") {
        return None;
    }
    let lhs = trim_edge_and(before, true);
    let rhs = trim_edge_and(after, false);
    if lhs.is_empty() && rhs.is_empty() {
        Some(None)
    } else if lhs.is_empty() {
        Some(Some(rhs.to_string()))
    } else if rhs.is_empty() {
        Some(Some(lhs.to_string()))
    } else {
        Some(Some(format!("{lhs} and {rhs}")))
    }
}

fn scope_rules_for_motion(css: &str, target: MotionTarget) -> String {
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len() + 64);
    let (mut i, mut stmt_start, mut quote) = (0_usize, 0_usize, None::<u8>);
    let (mut pdepth, mut bdepth) = (0_usize, 0_usize);
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
            b'(' => pdepth += 1,
            b')' => pdepth = pdepth.saturating_sub(1),
            b'[' => bdepth += 1,
            b']' => bdepth = bdepth.saturating_sub(1),
            b';' if pdepth == 0 && bdepth == 0 => {
                out.push_str(&css[stmt_start..=i]);
                stmt_start = i + 1;
            }
            b'{' if pdepth == 0 && bdepth == 0 => {
                let Some(close) = find_matching_brace(css, i) else {
                    break;
                };
                emit_motion_rule(&css[stmt_start..i], &css[i + 1..close], target, &mut out);
                i = close + 1;
                stmt_start = i;
                pdepth = 0;
                bdepth = 0;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.push_str(&css[stmt_start..]);
    out
}

fn emit_motion_rule(prelude: &str, body: &str, target: MotionTarget, out: &mut String) {
    let lead = skip_ws_and_comments_len(prelude);
    let trimmed = prelude[lead..].trim_end();
    if trimmed.starts_with('@') {
        out.push_str(prelude);
        out.push('{');
        out.push_str(&scope_rules_for_motion(body, target));
        out.push('}');
        return;
    }
    out.push_str(&prelude[..lead]);
    for (idx, sel) in split_top_level_commas(trimmed).into_iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&rewrite_motion_selector(sel, target));
    }
    if prelude.ends_with(char::is_whitespace) && !trimmed.is_empty() {
        out.push(' ');
    }
    out.push('{');
    out.push_str(body);
    out.push('}');
}

fn rewrite_motion_selector(sel: &str, target: MotionTarget) -> String {
    let lead = skip_ws_and_comments_len(sel);
    let trimmed = sel[lead..].trim();
    if trimmed.is_empty() {
        return sel.to_string();
    }
    let (root_attr, ancestor_prefix) = match target {
        MotionTarget::Reduce => (REDUCE_ROOT_ATTR, REDUCE_ANCESTOR),
        MotionTarget::NoPreference => (NOPREF_ROOT_ATTR, NOPREF_ANCESTOR),
    };
    if let Some(rest) = strip_root_prefix(trimmed) {
        return format!(
            "{}{trimmed_head}{root_attr}{rest}",
            &sel[..lead],
            trimmed_head = &trimmed[..trimmed.len() - rest.len()]
        );
    }
    format!("{}{ancestor_prefix}{trimmed}", &sel[..lead])
}

fn strip_root_prefix(s: &str) -> Option<&str> {
    for kw in [":root", "html"] {
        if starts_with_ci(s, kw) {
            let after = &s[kw.len()..];
            let ok = after.is_empty()
                || after.starts_with(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
            if ok {
                return Some(after);
            }
        }
    }
    None
}

fn find_top_open_brace(css: &str, start: usize) -> Option<usize> {
    let bytes = css.as_bytes();
    let (mut i, mut quote) = (start, None::<u8>);
    while i < bytes.len() {
        if let Some(q) = quote {
            i = advance_css_string(bytes, i, q, &mut quote);
            continue;
        }
        if css[i..].starts_with("/*") {
            let end = css[i + 2..].find("*/")?;
            i += 4 + end;
            continue;
        }
        match bytes[i] {
            b'"' | b'\'' => quote = Some(bytes[i]),
            b'{' => return Some(i),
            b';' | b'}' => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_matching_paren_bytes(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (idx, &b) in bytes.iter().enumerate().skip(open) {
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(idx);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::rewrite_prefers_reduced_motion_css;

    #[test]
    fn rewrites_reduce_nopref_and_compound_queries_unless_raw_preserved() {
        let css = r#"
            @media screen and (prefers-reduced-motion: reduce) {
                :root { --dur: 0ms; }
                .anim { opacity: 0.5; }
            }
            @media (prefers-reduced-motion: no-preference) {
                .anim { transition: opacity 200ms; }
            }
        "#;
        let out = rewrite_prefers_reduced_motion_css(css, false);
        assert!(out.contains("@media screen"));
        assert!(out.contains(":root:where([data-krui-motion=\"reduce\"])"));
        assert!(out.contains(":where(:root[data-krui-motion=\"reduce\"]) .anim"));
        assert!(out.contains(":where(:root:not([data-krui-motion=\"reduce\"])) .anim"));
        assert_eq!(rewrite_prefers_reduced_motion_css(css, true), css);
    }
}
