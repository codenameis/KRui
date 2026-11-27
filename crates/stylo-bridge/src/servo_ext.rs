//! Stylo Servo-profile CSS property extension adapter (`accent-color`, `text-box`, `text-box-trim`,
//! `text-box-edge`). Rewrites those declarations at parse time into internal `--krui-*` custom
//! properties so Stylo cascades, inherits, expands `var()`, and tracks dependencies natively.

use krui_style::{parse_text_box_shorthand, Color, TextBoxEdge, TextBoxTrim};
use std::borrow::Cow;
use style::properties::ComputedValues;
use style::Atom;
use style_traits::values::ToCss;

const EXT_MAPPINGS: [(&str, &str); 6] = [
    ("accent-color", "--krui-accent-color"),
    ("text-box-trim", "--krui-text-box-trim"),
    ("text-box-edge", "--krui-text-box-edge"),
    ("text-box", "--krui-text-box"),
    ("-webkit-line-clamp", "--krui-line-clamp"),
    ("line-clamp", "--krui-line-clamp"),
];

fn is_ident_or_selector_prefix(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'#')
}

fn find_kw_colon_end(bytes: &[u8], pos: usize, kw: &[u8]) -> Option<usize> {
    if pos + kw.len() >= bytes.len() {
        return None;
    }
    if pos > 0 && is_ident_or_selector_prefix(bytes[pos - 1]) {
        return None;
    }
    if !bytes[pos..pos + kw.len()].eq_ignore_ascii_case(kw) {
        return None;
    }
    let mut k = pos + kw.len();
    while k < bytes.len() && bytes[k].is_ascii_whitespace() {
        k += 1;
    }
    (k < bytes.len() && bytes[k] == b':').then_some(pos + kw.len())
}

/// Rewrites `accent-color` and `text-box*` declarations into `--krui-*` custom properties.
pub(crate) fn rewrite_servo_extensions(css: &str) -> Cow<'_, str> {
    let bytes = css.as_bytes();
    let mut i = 0_usize;
    let mut out: Option<String> = None;
    let mut copied_until = 0_usize;

    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        let matched = EXT_MAPPINGS
            .iter()
            .find_map(|&(kw, rep)| find_kw_colon_end(bytes, i, kw.as_bytes()).map(|e| (e, rep)));
        if let Some((end, rep)) = matched {
            let buf = out.get_or_insert_with(|| String::with_capacity(css.len() + 32));
            buf.push_str(&css[copied_until..i]);
            buf.push_str(rep);
            copied_until = end;
            i = end;
        } else {
            i += 1;
        }
    }
    match out {
        Some(mut buf) => {
            buf.push_str(&css[copied_until..]);
            Cow::Owned(buf)
        }
        None => Cow::Borrowed(css),
    }
}

fn read_custom_prop(cv: &ComputedValues, name: &str) -> Option<String> {
    cv.custom_properties()
        .get_for_cssom(&Atom::from(name))
        .map(|v| v.to_css_string())
}

/// Extracts `accent-color` (`Some(None)` = explicit `auto`, `Some(Some(c))` = explicit color, `None` = unset).
pub(crate) fn extract_accent_color(cv: &ComputedValues, current: Color) -> Option<Option<Color>> {
    let raw = read_custom_prop(cv, "krui-accent-color")?;
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("auto") {
        Some(None)
    } else if trimmed.eq_ignore_ascii_case("currentcolor") {
        Some(Some(current))
    } else {
        Color::parse_css(trimmed).map(Some)
    }
}

/// Extracts `text-box`, `text-box-trim`, and `text-box-edge` from Stylo custom properties.
pub(crate) fn extract_text_box(cv: &ComputedValues) -> (TextBoxTrim, TextBoxEdge) {
    let mut trim = TextBoxTrim::None;
    let mut edge = TextBoxEdge::AUTO;
    if let Some(raw_edge) = read_custom_prop(cv, "krui-text-box-edge") {
        if let Some(parsed) = TextBoxEdge::parse_css(&raw_edge) {
            edge = parsed;
        }
    }
    if let Some(raw_trim) = read_custom_prop(cv, "krui-text-box-trim") {
        if let Some(parsed) = TextBoxTrim::parse_css(&raw_trim) {
            trim = parsed;
        }
    }
    if let Some(raw_shorthand) = read_custom_prop(cv, "krui-text-box") {
        if let Some((t, e)) = parse_text_box_shorthand(&raw_shorthand) {
            trim = t;
            edge = e;
        }
    }
    (trim, edge)
}

/// Extracts `line-clamp` / `-webkit-line-clamp` positive integer line count (`None` = `none`).
pub(crate) fn extract_line_clamp(cv: &ComputedValues) -> Option<u16> {
    let raw = read_custom_prop(cv, "krui-line-clamp")?;
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    trimmed.parse::<u16>().ok().filter(|&n| n >= 1)
}

/// Returns `true` if `text-box*` or `line-clamp` extension custom properties changed between `a` and `b`.
pub(crate) fn text_box_props_differ(a: &ComputedValues, b: &ComputedValues) -> bool {
    extract_text_box(a) != extract_text_box(b) || extract_line_clamp(a) != extract_line_clamp(b)
}
