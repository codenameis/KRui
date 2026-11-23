//! Expression-aware HTML state-machine scanner (`{...}`, quoted attributes, `<style>`/`<link>` extraction,
//! and HTML inline whitespace normalization).

use crate::html::{RawAttr, RawHtmlNode};
use crate::CompileError;

/// Extracted `(<style> blocks, <link> hrefs, blanked HTML template)` tuple from [`extract_style_and_link_blocks`].
pub type ExtractedStyleAndLinks = (Vec<(String, usize)>, Vec<(String, usize)>, String);

/// Replaces non-newline bytes of `slice` with spaces in `dst` so byte offsets and line numbers stay intact.
pub fn push_blanked_preserving_newlines(dst: &mut String, slice: &str) {
    for b in slice.bytes() {
        dst.push(if b == b'\n' { '\n' } else { ' ' });
    }
}

/// Scans from an opening `{` at `open_brace_idx` in `input` to its matching closing `}`,
/// atomically skipping `'...'` and `"..."` string literals (with `\` escapes) and nested `{...}` blocks.
///
/// Returns the byte index of the matching `}` in `input`.
pub fn scan_template_expr_hole(input: &str, open_brace_idx: usize) -> Result<usize, &'static str> {
    let bytes = input.as_bytes();
    let mut i = open_brace_idx + 1;
    let mut depth = 1_usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' || b == b'\'' {
            i = scan_expr_string_literal(input, i, b)?;
        } else if b == b'{' {
            depth += 1;
            i += 1;
        } else if b == b'}' {
            depth -= 1;
            if depth == 0 {
                return Ok(i);
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    Err("Unclosed `{` in interpolated expression")
}

fn scan_expr_string_literal(
    input: &str,
    quote_idx: usize,
    quote_byte: u8,
) -> Result<usize, &'static str> {
    let bytes = input.as_bytes();
    let mut i = quote_idx + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == quote_byte {
            return Ok(i + 1);
        }
        if b == b'\\' {
            i += 1;
            if i >= bytes.len() {
                return Err("Unterminated escape in string literal inside `{...}`");
            }
            let ch_len = input[i..].chars().next().map_or(1, char::len_utf8);
            i += ch_len;
        } else {
            let ch_len = input[i..].chars().next().map_or(1, char::len_utf8);
            i += ch_len;
        }
    }
    Err("Unclosed string literal inside `{...}` expression")
}

/// Scans text content starting at `start` until the next real HTML `<` tag boundary,
/// treating `<` and `>` inside `{...}` template expressions (`{a < b}`) as expression operators
/// rather than HTML tags, and respecting `{{` / `}}` escaped braces.
pub fn scan_text_until_tag(input: &str, start: usize) -> Result<usize, CompileError> {
    let bytes = input.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'<' => return Ok(i),
            b'{' => {
                if bytes.get(i + 1) == Some(&b'{') {
                    i += 2;
                } else {
                    let close = scan_template_expr_hole(input, i)
                        .map_err(|msg| CompileError::at_offset(input, i, msg))?;
                    i = close + 1;
                }
            }
            b'}' => {
                if bytes.get(i + 1) == Some(&b'}') {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ => {
                let ch_len = input[i..].chars().next().map_or(1, char::len_utf8);
                i += ch_len;
            }
        }
    }
    Ok(bytes.len())
}

/// Scans a quoted HTML attribute value (`"..."` or `'...'`), atomically stepping over `{...}`
/// template expressions (including inner string literals) so `<`, `>`, and inner quotes never
/// prematurely terminate the attribute or enclosing tag.
pub fn scan_quoted_attr_value(
    input: &str,
    val_offset: usize,
    quote_ch: char,
) -> Result<(String, usize), CompileError> {
    let quote_byte = quote_ch as u8;
    let val_start = val_offset + quote_ch.len_utf8();
    let bytes = input.as_bytes();
    let mut i = val_start;
    while i < bytes.len() {
        let b = bytes[i];
        if b == quote_byte {
            let raw_val = input[val_start..i].to_string();
            return Ok((raw_val, i + quote_ch.len_utf8()));
        }
        if b == b'{' {
            if bytes.get(i + 1) == Some(&b'{') {
                i += 2;
                continue;
            }
            if let Ok(close_idx) = scan_template_expr_hole(input, i) {
                i = close_idx + 1;
                continue;
            }
        }
        let ch_len = input[i..].chars().next().map_or(1, char::len_utf8);
        i += ch_len;
    }
    Err(CompileError::at_offset(
        input,
        val_offset,
        "Unclosed attribute quote",
    ))
}

/// Extracts `<style>` and `<link>` blocks from `source` using the expression- and comment-aware
/// state machine without stripping `<meta>` (so `<meta>` is rejected as an unsupported HTML tag).
pub fn extract_style_and_link_blocks(source: &str) -> Result<ExtractedStyleAndLinks, CompileError> {
    let mut style_blocks = Vec::new();
    let mut linked_hrefs = Vec::new();
    let mut html_out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if source[i..].starts_with("<!--") {
            let end = source[i + 4..]
                .find("-->")
                .map(|rel| i + 4 + rel + 3)
                .ok_or_else(|| {
                    CompileError::at_offset(source, i, "Unclosed HTML comment `<!--`")
                })?;
            html_out.push_str(&source[i..end]);
            i = end;
        } else if bytes[i] == b'{' {
            if bytes.get(i + 1) == Some(&b'{') {
                html_out.push_str("{{");
                i += 2;
            } else if let Ok(close) = scan_template_expr_hole(source, i) {
                html_out.push_str(&source[i..=close]);
                i = close + 1;
            } else {
                html_out.push('{');
                i += 1;
            }
        } else if matches_tag_open_ci(source, i, "style") {
            i = consume_style_block(source, i, &mut style_blocks, &mut html_out)?;
        } else if matches_tag_open_ci(source, i, "link") {
            i = consume_link_tag(source, i, &mut linked_hrefs, &mut html_out)?;
        } else if bytes[i] == b'<' && !source[i..].starts_with("</") {
            let tag_end = scan_open_tag_end(source, i);
            html_out.push_str(&source[i..tag_end]);
            i = tag_end;
        } else {
            let ch = source[i..].chars().next().unwrap_or(' ');
            html_out.push(ch);
            i += ch.len_utf8();
        }
    }
    Ok((style_blocks, linked_hrefs, html_out))
}

fn matches_tag_open_ci(source: &str, idx: usize, name: &str) -> bool {
    let rest = &source[idx..];
    if !rest.starts_with('<') || rest.len() <= 1 + name.len() {
        return false;
    }
    let after_lt = &rest[1..];
    if !after_lt
        .get(..name.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(name))
    {
        return false;
    }
    after_lt[name.len()..]
        .chars()
        .next()
        .is_some_and(|c| c.is_whitespace() || c == '>' || c == '/')
}

fn scan_open_tag_end(source: &str, start: usize) -> usize {
    let bytes = source.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'>' {
            return i + 1;
        }
        if b == b'"' || b == b'\'' {
            if let Ok((_, next_i)) = scan_quoted_attr_value(source, i, b as char) {
                i = next_i;
                continue;
            }
            return bytes.len();
        }
        i += 1;
    }
    bytes.len()
}

fn consume_style_block(
    source: &str,
    start: usize,
    style_blocks: &mut Vec<(String, usize)>,
    html_out: &mut String,
) -> Result<usize, CompileError> {
    let open_end = scan_open_tag_end(source, start);
    if open_end > source.len() || !source[start..open_end].ends_with('>') {
        return Err(CompileError::at_offset(
            source,
            start,
            "Unclosed `<style>` tag in KRui markup",
        ));
    }
    let self_closing = source[start..open_end].ends_with("/>");
    let attr_end = if self_closing {
        open_end - 2
    } else {
        open_end - 1
    };
    let attr_slice = source[start + 6..attr_end].trim();
    if !attr_slice.is_empty() {
        return Err(CompileError::at_offset(
            source,
            start + 6,
            format!("Attributes on `<style>` tags (`{attr_slice}`) are not supported in KRui"),
        ));
    }
    if self_closing {
        push_blanked_preserving_newlines(html_out, &source[start..open_end]);
        return Ok(open_end);
    }
    let content_start = open_end;
    let Some((close_start, full_end)) = find_closing_style_tag(source, content_start) else {
        return Err(CompileError::at_offset(
            source,
            start,
            "Unclosed `<style>` tag in KRui markup",
        ));
    };
    push_blanked_preserving_newlines(html_out, &source[start..full_end]);
    style_blocks.push((
        source[content_start..close_start].to_string(),
        content_start,
    ));
    Ok(full_end)
}

fn find_closing_style_tag(source: &str, from: usize) -> Option<(usize, usize)> {
    let mut cursor = from;
    while let Some(rel) = source[cursor..].find("</") {
        let idx = cursor + rel;
        let after = &source[idx + 2..];
        if after
            .get(..5)
            .is_some_and(|s| s.eq_ignore_ascii_case("style"))
        {
            let tail = &after[5..];
            if let Some(gt) = tail.find('>') {
                if tail[..gt].trim().is_empty() {
                    return Some((idx, idx + 2 + 5 + gt + 1));
                }
            }
        }
        cursor = idx + 2;
    }
    None
}

fn consume_link_tag(
    source: &str,
    start: usize,
    linked_hrefs: &mut Vec<(String, usize)>,
    html_out: &mut String,
) -> Result<usize, CompileError> {
    let mut temp_cursor = start;
    let (_, attrs, _, _) = crate::html::parse_tag_open(source, &mut temp_cursor)?;
    let full_end = temp_cursor;
    let mut found_href = false;
    for (k, v, _, off) in &attrs {
        match k.as_str() {
            "href" => {
                let trimmed = v.trim();
                if trimmed.is_empty() {
                    return Err(CompileError::at_offset(
                        source,
                        *off,
                        "`<link>` `href` attribute cannot be empty",
                    ));
                }
                linked_hrefs.push((trimmed.to_string(), start));
                found_href = true;
            }
            "rel" => {
                let rel = v.trim();
                if !rel.eq_ignore_ascii_case("stylesheet")
                    && !rel.eq_ignore_ascii_case("krui-preset")
                {
                    return Err(CompileError::at_offset(
                        source,
                        *off,
                        format!(
                            "Unsupported `<link rel=\"{v}\">` (expected `rel=\"stylesheet\"` or `rel=\"krui-preset\"`)"
                        ),
                    ));
                }
            }
            "type" => {}
            other => {
                return Err(CompileError::at_offset(
                    source,
                    *off,
                    format!("Unknown `<link>` attribute `{other}`"),
                ));
            }
        }
    }
    if !found_href {
        return Err(CompileError::at_offset(
            source,
            start,
            "`<link>` tag requires an `href` attribute",
        ));
    }
    push_blanked_preserving_newlines(html_out, &source[start..full_end]);
    Ok(full_end)
}

/// Collapses contiguous ASCII whitespace runs (`' '`, `'\t'`, `'\r'`, `'\n'`) into a single `' '`.
pub fn collapse_ascii_ws(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_ws = false;
    for ch in raw.chars() {
        if ch.is_ascii_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(ch);
            in_ws = false;
        }
    }
    out
}

fn is_inline_sibling(node: &RawHtmlNode) -> bool {
    match node {
        RawHtmlNode::TextRun { .. } => true,
        RawHtmlNode::Element { tag, .. } => tag.is_inline_element(),
        RawHtmlNode::Component { .. } => false,
        RawHtmlNode::Slot { .. } => false,
        RawHtmlNode::If { branches, .. } => branches
            .iter()
            .all(|b| b.children.iter().all(is_inline_sibling)),
        RawHtmlNode::For { body, .. } => body.iter().all(is_inline_sibling),
    }
}

/// Applies HTML inline whitespace collapsing to a sequence of sibling `RawHtmlNode`s:
/// collapses internal whitespace runs while preserving single spaces adjacent to inline siblings
/// inside text-flow containers, and drops pure-whitespace inter-element indentation in block containers.
pub fn normalize_inline_children(nodes: Vec<RawHtmlNode>, in_text_flow: bool) -> Vec<RawHtmlNode> {
    let len = nodes.len();
    let mut out = Vec::with_capacity(len);
    for (idx, node) in nodes.into_iter().enumerate() {
        let RawHtmlNode::TextRun { text, offset } = node else {
            out.push(node);
            continue;
        };
        if !in_text_flow && text.chars().all(char::is_whitespace) {
            continue;
        }
        let mut collapsed = collapse_ascii_ws(&text);
        let prev_inline = idx > 0 && out.last().is_some_and(is_inline_sibling);
        if !prev_inline && collapsed.starts_with(' ') {
            collapsed.remove(0);
        }
        if idx + 1 == len && collapsed.ends_with(' ') {
            collapsed.pop();
        }
        if !collapsed.is_empty() {
            out.push(RawHtmlNode::TextRun {
                text: collapsed,
                offset,
            });
        }
    }
    trim_spaces_before_block_siblings(&mut out);
    out
}

fn trim_spaces_before_block_siblings(nodes: &mut Vec<RawHtmlNode>) {
    let mut i = 0;
    while i < nodes.len() {
        let next_is_inline = nodes.get(i + 1).is_some_and(is_inline_sibling);
        let has_next = i + 1 < nodes.len();
        if has_next && !next_is_inline {
            if let RawHtmlNode::TextRun { text, .. } = &mut nodes[i] {
                if text.ends_with(' ') {
                    text.pop();
                }
                if text.is_empty() {
                    nodes.remove(i);
                    continue;
                }
            }
        }
        i += 1;
    }
    let _ = std::mem::size_of::<RawAttr>();
}
