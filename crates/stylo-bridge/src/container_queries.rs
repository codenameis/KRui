//! Compile/parse-time `@container` size-query adapter for Stylo Servo mode.
//!
//! Translates W3C `@container` size rules into Stylo-native `@scope` boundaries keyed by internal
//! `data-krui-cq-<id>` attributes updated after layout passes (`StyleResolver`), preserving
//! selector specificity, `@layer`/`@media` cascade order, and non-matching container isolation.

use crate::container_query_parse::{parse_container_prelude, ContainerQuerySpec};
use crate::dom_store::StyloDom;
use crate::StyloNodeId;
use style::properties::ComputedValues;
use style::selector_parser::SnapshotMap;
use style::stylist::Stylist;
use style::values::specified::box_::ContainerType;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RegisteredContainerQuery {
    pub(crate) id: u32,
    pub(crate) sheet_id: u64,
    pub(crate) spec: ContainerQuerySpec,
}

/// Translates supported `@container` rules and `container:` shorthands into Stylo Servo `@scope`
/// rules, returning the rewritten CSS plus registered container queries for runtime evaluation.
pub(crate) fn preprocess_container_rules(
    css: &str,
    sheet_id: u64,
    next_query_id: &mut u32,
) -> (String, Vec<RegisteredContainerQuery>) {
    if !contains_ascii_ci(css.as_bytes(), b"container") {
        return (css.to_string(), Vec::new());
    }
    let mut queries = Vec::new();
    let rewritten = scan_css_segment(css, sheet_id, next_query_id, &mut queries);
    (rewritten, queries)
}

fn contains_ascii_ci(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|win| win.eq_ignore_ascii_case(needle))
}

fn scan_css_segment(
    css: &str,
    sheet_id: u64,
    next_query_id: &mut u32,
    queries: &mut Vec<RegisteredContainerQuery>,
) -> String {
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len() + 64);
    let mut cursor = 0_usize;
    let mut i = 0_usize;
    while i < bytes.len() {
        if let Some(next_i) = skip_comment_or_string(bytes, i) {
            i = next_i;
            continue;
        }
        if is_at_container_start(bytes, i) {
            if let Some((end, emitted)) =
                try_rewrite_at_container(css, i, sheet_id, next_query_id, queries)
            {
                out.push_str(&css[cursor..i]);
                out.push_str(&emitted);
                cursor = end;
                i = end;
                continue;
            }
        }
        if starts_container_decl(bytes, i) {
            if let Some((end, expanded)) = try_expand_container_shorthand(css, i) {
                out.push_str(&css[cursor..i]);
                out.push_str(&expanded);
                cursor = end;
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&css[cursor..]);
    out
}

fn is_at_container_start(bytes: &[u8], pos: usize) -> bool {
    const KW: &[u8] = b"@container";
    if bytes[pos] != b'@' || bytes[pos..].len() <= KW.len() {
        return false;
    }
    if !bytes[pos..pos + KW.len()].eq_ignore_ascii_case(KW) {
        return false;
    }
    let next = bytes[pos + KW.len()];
    next.is_ascii_whitespace() || next == b'(' || next == b'/'
}

fn try_rewrite_at_container(
    css: &str,
    at_pos: usize,
    sheet_id: u64,
    next_query_id: &mut u32,
    queries: &mut Vec<RegisteredContainerQuery>,
) -> Option<(usize, String)> {
    let prelude_start = at_pos + b"@container".len();
    let brace_open = find_rule_open_brace(css.as_bytes(), prelude_start)?;
    let brace_close = find_matching_brace(css.as_bytes(), brace_open)?;
    let prelude = css[prelude_start..brace_open].trim();
    let raw_body = &css[brace_open + 1..brace_close];
    let body = scan_css_segment(raw_body, sheet_id, next_query_id, queries);
    let Ok(spec) = parse_container_prelude(prelude) else {
        return Some((brace_close + 1, format!("@container {prelude} {{{body}}}")));
    };
    let id = *next_query_id;
    *next_query_id += 1;
    queries.push(RegisteredContainerQuery { id, sheet_id, spec });
    let scoped =
        format!("@scope ([data-krui-cq-{id}=\"1\"]) to ([data-krui-cq-{id}=\"0\"] > *) {{{body}}}");
    Some((brace_close + 1, scoped))
}

fn starts_container_decl(bytes: &[u8], pos: usize) -> bool {
    const KW: &[u8] = b"container";
    if pos + KW.len() >= bytes.len() || !bytes[pos..pos + KW.len()].eq_ignore_ascii_case(KW) {
        return false;
    }
    let prev_ok =
        pos == 0 || matches!(bytes[pos - 1], b'{' | b';') || bytes[pos - 1].is_ascii_whitespace();
    if !prev_ok {
        return false;
    }
    let mut k = pos + KW.len();
    while k < bytes.len() && bytes[k].is_ascii_whitespace() {
        k += 1;
    }
    k < bytes.len() && bytes[k] == b':'
}

fn try_expand_container_shorthand(css: &str, start: usize) -> Option<(usize, String)> {
    let bytes = css.as_bytes();
    let colon = start + css[start..].find(':')?;
    let mut i = colon + 1;
    let mut paren = 0_usize;
    while i < bytes.len() {
        if let Some(ni) = skip_comment_or_string(bytes, i) {
            i = ni;
            continue;
        }
        match bytes[i] {
            b'(' => paren += 1,
            b')' => paren = paren.saturating_sub(1),
            b';' | b'}' if paren == 0 => break,
            _ => {}
        }
        i += 1;
    }
    let val = css[colon + 1..i].trim();
    let (lhs, rhs) = val.split_once('/')?;
    let (name, ctype) = (lhs.trim(), rhs.trim());
    if name.is_empty() || ctype.is_empty() {
        return None;
    }
    let has_semi = i < bytes.len() && bytes[i] == b';';
    let end = if has_semi { i + 1 } else { i };
    let tail = if has_semi { ";" } else { "" };
    Some((
        end,
        format!("container-name: {name}; container-type: {ctype}{tail}"),
    ))
}

/// Synchronizes container dimensions and `data-krui-cq-<id>` attributes for one node.
pub(crate) fn sync_container_node(
    dom: &mut StyloDom,
    snapshots: &mut SnapshotMap,
    stylist: &Stylist,
    queries: &[RegisteredContainerQuery],
    cv: Option<&ComputedValues>,
    (node, width_px, height_px): (StyloNodeId, Option<f32>, Option<f32>),
) -> bool {
    let size_changed = dom.set_container_size(node, width_px, height_px);
    let (track_inline, track_block, names, elem_fs) = match cv {
        Some(style) => {
            let ct = style.get_box().clone_container_type();
            let ti = ct.intersects(ContainerType::INLINE_SIZE | ContainerType::SIZE);
            let tb = ct.contains(ContainerType::SIZE);
            let cnames: Vec<String> = style
                .get_box()
                .clone_container_name()
                .0
                .iter()
                .map(|n| n.0.to_string())
                .collect();
            (
                ti,
                tb,
                cnames,
                style.get_font().font_size.computed_size().px(),
            )
        }
        None => (false, false, Vec::new(), 16.0),
    };
    let root_fs = stylist.device().root_font_size().px();
    let mut attr_changed = false;
    for q in queries {
        let name_ok = q
            .spec
            .name
            .as_deref()
            .is_none_or(|req| names.iter().any(|n| n == req));
        let qualifies = (track_inline || track_block)
            && q.spec.matches_axes(track_inline, track_block)
            && name_ok;
        let desired = qualifies
            .then(|| q.spec.evaluate(width_px, height_px, root_fs, elem_fs))
            .flatten()
            .map(|matched| if matched { "1" } else { "0" });
        let attr_name = format!("data-krui-cq-{}", q.id);
        attr_changed |= dom.set_single_attribute(node, &attr_name, desired, snapshots);
    }
    if attr_changed {
        if let Some(idx) = dom.resolve_live(node) {
            dom.restyle_subtree_slot(idx);
            dom.mark_ancestors_dirty(idx);
        }
    }
    size_changed || attr_changed
}

/// Clears cached container sizes and removes obsolete `data-krui-cq-*` attributes after sheet edits.
pub(crate) fn clear_container_caches(
    dom: &mut StyloDom,
    snapshots: &mut SnapshotMap,
    removed_ids: &[u32],
) {
    let live_ids: Vec<StyloNodeId> = dom
        .slots
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, s)| s.occupied)
        .map(|(i, s)| StyloNodeId::new(i as u32, s.generation))
        .collect();
    for slot in &mut dom.slots {
        slot.container_size = (None, None);
    }
    if removed_ids.is_empty() {
        return;
    }
    for id in live_ids {
        for qid in removed_ids {
            let attr = format!("data-krui-cq-{qid}");
            let _ = dom.set_single_attribute(id, &attr, None, snapshots);
        }
    }
}

fn find_rule_open_brace(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i < bytes.len() {
        if let Some(ni) = skip_comment_or_string(bytes, i) {
            i = ni;
            continue;
        }
        if bytes[i] == b'{' {
            return Some(i);
        }
        if bytes[i] == b';' || bytes[i] == b'}' {
            return None;
        }
        i += 1;
    }
    None
}

fn find_matching_brace(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 1_usize;
    let mut i = open + 1;
    while i < bytes.len() {
        if let Some(ni) = skip_comment_or_string(bytes, i) {
            i = ni;
            continue;
        }
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn skip_comment_or_string(bytes: &[u8], pos: usize) -> Option<usize> {
    if bytes[pos] == b'/' && bytes.get(pos + 1) == Some(&b'*') {
        let mut i = pos + 2;
        while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
            i += 1;
        }
        return Some((i + 2).min(bytes.len()));
    }
    if bytes[pos] == b'"' || bytes[pos] == b'\'' {
        let quote = bytes[pos];
        let mut i = pos + 1;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i += 2;
            } else if bytes[i] == quote {
                return Some(i + 1);
            } else {
                i += 1;
            }
        }
        return Some(bytes.len());
    }
    None
}
