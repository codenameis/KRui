//! `@container` size-query condition AST, CSS Level 4 range parser, and evaluator.

const PT_TO_PX: f32 = 96.0 / 72.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ContainerQuerySpec {
    pub(crate) name: Option<String>,
    pub(crate) expr: Option<CqExpr>,
    pub(crate) requires_inline: bool,
    pub(crate) requires_block: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CqExpr {
    And(Vec<CqExpr>),
    Or(Vec<CqExpr>),
    Not(Box<CqExpr>),
    Width(CqCmp, CqLen),
    Height(CqCmp, CqLen),
    InlineSize(CqCmp, CqLen),
    BlockSize(CqCmp, CqLen),
    HasInlineSize,
    HasBlockSize,
    AspectRatio(CqCmp, f32),
    Landscape(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CqCmp {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CqLen {
    Px(f32),
    Rem(f32),
    Em(f32),
}

impl CqLen {
    fn resolve(self, root_font_px: f32, elem_font_px: f32) -> f32 {
        match self {
            Self::Px(v) => v,
            Self::Rem(v) => v * root_font_px.max(1.0),
            Self::Em(v) => v * elem_font_px.max(1.0),
        }
    }
}

impl CqCmp {
    fn test(self, lhs: f32, rhs: f32) -> bool {
        match self {
            Self::Lt => lhs < rhs,
            Self::Le => lhs <= rhs,
            Self::Eq => (lhs - rhs).abs() <= 0.01,
            Self::Ge => lhs >= rhs,
            Self::Gt => lhs > rhs,
        }
    }

    fn flip(self) -> Self {
        match self {
            Self::Lt => Self::Gt,
            Self::Le => Self::Ge,
            Self::Eq => Self::Eq,
            Self::Ge => Self::Le,
            Self::Gt => Self::Lt,
        }
    }
}

impl ContainerQuerySpec {
    pub(crate) fn matches_axes(&self, has_inline: bool, has_block: bool) -> bool {
        (!self.requires_inline || has_inline) && (!self.requires_block || has_block)
    }

    pub(crate) fn evaluate(
        &self,
        width_px: Option<f32>,
        height_px: Option<f32>,
        root_font_px: f32,
        elem_font_px: f32,
    ) -> Option<bool> {
        let Some(expr) = &self.expr else {
            return Some(true);
        };
        expr.eval(width_px, height_px, root_font_px, elem_font_px)
    }
}

impl CqExpr {
    fn collect_axes(&self, inline: &mut bool, block: &mut bool) {
        match self {
            Self::And(list) | Self::Or(list) => {
                list.iter().for_each(|x| x.collect_axes(inline, block))
            }
            Self::Not(inner) => inner.collect_axes(inline, block),
            Self::Width(..) | Self::InlineSize(..) | Self::HasInlineSize => *inline = true,
            Self::Height(..) | Self::BlockSize(..) | Self::HasBlockSize => *block = true,
            Self::AspectRatio(..) | Self::Landscape(..) => (*inline, *block) = (true, true),
        }
    }

    fn eval(&self, w: Option<f32>, h: Option<f32>, root_fs: f32, elem_fs: f32) -> Option<bool> {
        match self {
            Self::And(list) => eval_and(list, w, h, root_fs, elem_fs),
            Self::Or(list) => eval_or(list, w, h, root_fs, elem_fs),
            Self::Not(inner) => inner.eval(w, h, root_fs, elem_fs).map(|v| !v),
            Self::Width(op, len) | Self::InlineSize(op, len) => {
                w.map(|v| op.test(v, len.resolve(root_fs, elem_fs)))
            }
            Self::Height(op, len) | Self::BlockSize(op, len) => {
                h.map(|v| op.test(v, len.resolve(root_fs, elem_fs)))
            }
            Self::HasInlineSize => w.map(|v| v > 0.0),
            Self::HasBlockSize => h.map(|v| v > 0.0),
            Self::AspectRatio(op, target) => match (w, h) {
                (Some(wv), Some(hv)) if hv > 0.0 => Some(op.test(wv / hv, *target)),
                _ => None,
            },
            Self::Landscape(want_landscape) => match (w, h) {
                (Some(wv), Some(hv)) => Some((wv >= hv) == *want_landscape),
                _ => None,
            },
        }
    }
}

fn eval_and(list: &[CqExpr], w: Option<f32>, h: Option<f32>, rfs: f32, efs: f32) -> Option<bool> {
    let mut saw_unknown = false;
    for item in list {
        match item.eval(w, h, rfs, efs) {
            Some(false) => return Some(false),
            Some(true) => {}
            None => saw_unknown = true,
        }
    }
    (!saw_unknown).then_some(true)
}

fn eval_or(list: &[CqExpr], w: Option<f32>, h: Option<f32>, rfs: f32, efs: f32) -> Option<bool> {
    let mut saw_unknown = false;
    for item in list {
        match item.eval(w, h, rfs, efs) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => saw_unknown = true,
        }
    }
    (!saw_unknown).then_some(false)
}

pub(crate) fn parse_container_prelude(prelude: &str) -> Result<ContainerQuerySpec, ()> {
    let clean = strip_css_comments(prelude);
    let trimmed = clean.trim();
    if trimmed.is_empty() {
        return Err(());
    }
    let (name, rest) = split_container_name_and_cond(trimmed);
    let expr = if rest.is_empty() {
        if name.is_none() {
            return Err(());
        }
        None
    } else {
        Some(parse_or_expr(rest)?)
    };
    let (mut requires_inline, mut requires_block) = (false, false);
    if let Some(e) = &expr {
        e.collect_axes(&mut requires_inline, &mut requires_block);
    }
    Ok(ContainerQuerySpec {
        name,
        expr,
        requires_inline,
        requires_block,
    })
}

fn split_container_name_and_cond(s: &str) -> (Option<String>, &str) {
    if s.starts_with('(') {
        return (None, s);
    }
    let first_sep = s
        .find(|c: char| c.is_ascii_whitespace() || c == '(')
        .unwrap_or(s.len());
    let head = &s[..first_sep];
    if head.eq_ignore_ascii_case("not")
        || head.eq_ignore_ascii_case("and")
        || head.eq_ignore_ascii_case("or")
    {
        return (None, s);
    }
    (Some(head.to_string()), s[first_sep..].trim_start())
}

fn parse_or_expr(input: &str) -> Result<CqExpr, ()> {
    let parts = split_top_level_kw(input, "or");
    if parts.len() == 1 {
        return parse_and_expr(parts[0]);
    }
    Ok(CqExpr::Or(
        parts
            .into_iter()
            .map(parse_and_expr)
            .collect::<Result<Vec<_>, _>>()?,
    ))
}

fn parse_and_expr(input: &str) -> Result<CqExpr, ()> {
    let parts = split_top_level_kw(input, "and");
    if parts.len() == 1 {
        return parse_not_or_atom(parts[0]);
    }
    Ok(CqExpr::And(
        parts
            .into_iter()
            .map(parse_not_or_atom)
            .collect::<Result<Vec<_>, _>>()?,
    ))
}

fn parse_not_or_atom(raw: &str) -> Result<CqExpr, ()> {
    let s = raw.trim();
    if let Some(rest) = strip_leading_kw_ci(s, "not") {
        return Ok(CqExpr::Not(Box::new(parse_not_or_atom(rest)?)));
    }
    let inner = unwrap_outer_parens(s).ok_or(())?;
    if !inner.starts_with('(')
        && split_top_level_kw(inner, "or").len() == 1
        && split_top_level_kw(inner, "and").len() == 1
        && strip_leading_kw_ci(inner, "not").is_none()
    {
        return parse_feature_inner(inner);
    }
    parse_or_expr(inner)
}

fn parse_feature_inner(raw: &str) -> Result<CqExpr, ()> {
    let s = raw.trim();
    if let Some((k, v)) = s.split_once(':') {
        return parse_colon_feature(k.trim(), v.trim());
    }
    if let Some(range) = parse_range_feature(s)? {
        return Ok(range);
    }
    match s.to_ascii_lowercase().as_str() {
        "width" | "inline-size" => Ok(CqExpr::HasInlineSize),
        "height" | "block-size" => Ok(CqExpr::HasBlockSize),
        _ => Err(()),
    }
}

fn parse_colon_feature(key: &str, val: &str) -> Result<CqExpr, ()> {
    let k = key.to_ascii_lowercase();
    match k.as_str() {
        "min-width" | "min-inline-size" | "min-height" | "min-block-size" => {
            Ok(build_axis_expr(&k, CqCmp::Ge, parse_cq_len(val)?))
        }
        "max-width" | "max-inline-size" | "max-height" | "max-block-size" => {
            Ok(build_axis_expr(&k, CqCmp::Le, parse_cq_len(val)?))
        }
        "width" | "inline-size" | "height" | "block-size" => {
            Ok(build_axis_expr(&k, CqCmp::Eq, parse_cq_len(val)?))
        }
        "orientation" => match val.to_ascii_lowercase().as_str() {
            "landscape" => Ok(CqExpr::Landscape(true)),
            "portrait" => Ok(CqExpr::Landscape(false)),
            _ => Err(()),
        },
        "aspect-ratio" => Ok(CqExpr::AspectRatio(CqCmp::Eq, parse_ratio(val)?)),
        "min-aspect-ratio" => Ok(CqExpr::AspectRatio(CqCmp::Ge, parse_ratio(val)?)),
        "max-aspect-ratio" => Ok(CqExpr::AspectRatio(CqCmp::Le, parse_ratio(val)?)),
        _ => Err(()),
    }
}

fn parse_range_feature(s: &str) -> Result<Option<CqExpr>, ()> {
    let ops = find_comparison_ops(s);
    if ops.is_empty() {
        return Ok(None);
    }
    if ops.len() == 1 {
        let (start, end, op) = ops[0];
        let (lhs, rhs) = (s[..start].trim(), s[end..].trim());
        if is_axis_ident(lhs) {
            return Ok(Some(make_range_cmp(lhs, op, rhs)?));
        }
        if is_axis_ident(rhs) {
            return Ok(Some(make_range_cmp(rhs, op.flip(), lhs)?));
        }
        return Err(());
    }
    if ops.len() == 2 {
        let ((s1, e1, op1), (s2, e2, op2)) = (ops[0], ops[1]);
        let mid = s[e1..s2].trim();
        if !is_axis_ident(mid) {
            return Err(());
        }
        return Ok(Some(CqExpr::And(vec![
            make_range_cmp(mid, op1.flip(), s[..s1].trim())?,
            make_range_cmp(mid, op2, s[e2..].trim())?,
        ])));
    }
    Err(())
}

fn make_range_cmp(axis: &str, op: CqCmp, val: &str) -> Result<CqExpr, ()> {
    if axis.eq_ignore_ascii_case("aspect-ratio") {
        return Ok(CqExpr::AspectRatio(op, parse_ratio(val)?));
    }
    Ok(build_axis_expr(axis, op, parse_cq_len(val)?))
}

fn build_axis_expr(name: &str, op: CqCmp, len: CqLen) -> CqExpr {
    let lower = name.to_ascii_lowercase();
    if lower.contains("height") {
        CqExpr::Height(op, len)
    } else if lower.contains("block-size") {
        CqExpr::BlockSize(op, len)
    } else if lower.contains("inline-size") {
        CqExpr::InlineSize(op, len)
    } else {
        CqExpr::Width(op, len)
    }
}

fn is_axis_ident(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "width" | "height" | "inline-size" | "block-size" | "aspect-ratio"
    )
}

fn parse_cq_len(raw: &str) -> Result<CqLen, ()> {
    let s = raw.trim().to_ascii_lowercase();
    if s == "0" {
        return Ok(CqLen::Px(0.0));
    }
    if let Some(num) = s.strip_suffix("rem") {
        return num.trim().parse::<f32>().map(CqLen::Rem).map_err(|_| ());
    }
    if let Some(num) = s.strip_suffix("em") {
        return num.trim().parse::<f32>().map(CqLen::Em).map_err(|_| ());
    }
    if let Some(num) = s.strip_suffix("px") {
        return num.trim().parse::<f32>().map(CqLen::Px).map_err(|_| ());
    }
    if let Some(num) = s.strip_suffix("pt") {
        return num
            .trim()
            .parse::<f32>()
            .map(|v| CqLen::Px(v * PT_TO_PX))
            .map_err(|_| ());
    }
    Err(())
}

fn parse_ratio(raw: &str) -> Result<f32, ()> {
    if let Some((num, den)) = raw.split_once('/') {
        let (a, b) = (
            num.trim().parse::<f32>().map_err(|_| ())?,
            den.trim().parse::<f32>().map_err(|_| ())?,
        );
        return (b > 0.0).then_some(a / b).ok_or(());
    }
    let val = raw.trim().parse::<f32>().map_err(|_| ())?;
    (val > 0.0).then_some(val).ok_or(())
}

fn find_comparison_ops(s: &str) -> Vec<(usize, usize, CqCmp)> {
    let bytes = s.as_bytes();
    let (mut ops, mut i) = (Vec::new(), 0_usize);
    while i < bytes.len() {
        let found = match (bytes[i], bytes.get(i + 1)) {
            (b'<', Some(b'=')) => Some((i + 2, CqCmp::Le)),
            (b'>', Some(b'=')) => Some((i + 2, CqCmp::Ge)),
            (b'<', _) => Some((i + 1, CqCmp::Lt)),
            (b'>', _) => Some((i + 1, CqCmp::Gt)),
            (b'=', _) => Some((i + 1, CqCmp::Eq)),
            _ => None,
        };
        if let Some((end, op)) = found {
            ops.push((i, end, op));
            i = end;
        } else {
            i += 1;
        }
    }
    ops
}

fn split_top_level_kw<'a>(s: &'a str, kw: &str) -> Vec<&'a str> {
    let (bytes, mut parts, mut depth, mut last, mut i) =
        (s.as_bytes(), Vec::new(), 0_usize, 0_usize, 0_usize);
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ if depth == 0
                && bytes[i..].len() >= kw.len()
                && bytes[i..i + kw.len()].eq_ignore_ascii_case(kw.as_bytes()) =>
            {
                let prev_ok = i > 0 && (bytes[i - 1].is_ascii_whitespace() || bytes[i - 1] == b')');
                let after = i + kw.len();
                let next_ok = after < bytes.len()
                    && (bytes[after].is_ascii_whitespace() || bytes[after] == b'(');
                if prev_ok && next_ok {
                    parts.push(s[last..i].trim());
                    last = after;
                    i = after;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(s[last..].trim());
    parts
}

fn strip_leading_kw_ci<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let bytes = s.as_bytes();
    if bytes.len() < kw.len() || !bytes[..kw.len()].eq_ignore_ascii_case(kw.as_bytes()) {
        return None;
    }
    let rest = s.get(kw.len()..)?;
    (rest.starts_with(char::is_whitespace) || rest.starts_with('(')).then(|| rest.trim_start())
}

fn unwrap_outer_parens(s: &str) -> Option<&str> {
    if !s.starts_with('(') || !s.ends_with(')') {
        return None;
    }
    let mut depth = 0_usize;
    for (idx, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && idx + 1 < s.len() {
                    return None;
                }
            }
            _ => {}
        }
    }
    (depth == 0).then_some(s[1..s.len() - 1].trim())
}

fn strip_css_comments(s: &str) -> String {
    let bytes = s.as_bytes();
    let (mut out, mut i) = (String::with_capacity(s.len()), 0_usize);
    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            out.push(' ');
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}
