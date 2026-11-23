//! Extraction of CSS `@font-face` rules directly from Stylo's cascaded [`ExtraStyleData`].

use krui_style::{
    FontContainerFormat, FontFaceMetricsOverride, FontFaceRule, FontFaceSource, FontStyleSlope,
    UnicodeRange,
};
use std::sync::Arc;
use style::font_face::{FontStyleRange, MetricsOverride, Source};
use style::shared_lock::SharedRwLockReadGuard;
use style::stylist::Stylist;

fn is_sfnt_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 12
        && matches!(
            &bytes[0..4],
            [0x00, 0x01, 0x00, 0x00] | b"OTTO" | b"true" | b"ttcf"
        )
}

fn is_woff2_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && &bytes[0..4] == b"wOF2"
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut buf, mut bits) = (0_u32, 0_u8);
    for b in input.bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            break;
        }
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

fn try_embedded_url(
    url_str: &str,
    resolve_embedded: &dyn Fn(&str) -> Option<Arc<[u8]>>,
) -> Option<Arc<[u8]>> {
    resolve_embedded(url_str).or_else(|| {
        url_str
            .strip_prefix("krui://inline/")
            .and_then(resolve_embedded)
    })
}

fn resolve_url_font_source(
    url_str: &str,
    detected_format: &mut FontContainerFormat,
    resolve_embedded: &dyn Fn(&str) -> Option<Arc<[u8]>>,
) -> Option<FontFaceSource> {
    let lower = url_str.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return None;
    }
    let bytes: Arc<[u8]> = if lower.starts_with("data:") {
        let (_, b64) = url_str.split_once(";base64,")?;
        Arc::from(decode_base64(b64)?)
    } else if let Some(embedded) = try_embedded_url(url_str, resolve_embedded) {
        embedded
    } else if let Ok(parsed) = url::Url::parse(url_str) {
        let path = parsed.to_file_path().ok()?;
        Arc::from(std::fs::read(path).ok()?)
    } else {
        return None;
    };
    if is_woff2_magic(&bytes) {
        *detected_format = FontContainerFormat::Woff2;
    } else if !is_sfnt_magic(&bytes) {
        return None;
    }
    Some(FontFaceSource::Embedded(bytes))
}

fn metric_override_to_pct(m: Option<&MetricsOverride>) -> Option<f32> {
    match m {
        Some(MetricsOverride::Normal) | None => None,
        Some(MetricsOverride::Override(p)) => p.0.get().map(|v| v.max(0.0)),
    }
}

/// Collects all active `@font-face` declarations from [`Stylist`] extra style data.
#[must_use]
pub(crate) fn extract_font_face_rules_with(
    stylist: &Stylist,
    guard: &SharedRwLockReadGuard,
    resolve_embedded: &dyn Fn(&str) -> Option<Arc<[u8]>>,
) -> Vec<FontFaceRule> {
    let mut rules = Vec::new();
    for (extra, _origin) in stylist.iter_extra_data_origins() {
        for (rule_lock, _layer) in &*extra.font_faces {
            let rule = rule_lock.read_with(guard);
            if let Some(parsed) = parse_single_font_face_rule(rule, resolve_embedded) {
                rules.push(parsed);
            }
        }
    }
    rules
}

fn parse_single_font_face_rule(
    rule: &style::stylesheets::FontFaceRule,
    resolve_embedded: &dyn Fn(&str) -> Option<Arc<[u8]>>,
) -> Option<FontFaceRule> {
    let d = &rule.descriptors;
    let family = d.font_family.as_ref()?.name.to_string();
    let src_list = d.src.as_ref()?;
    let mut format = FontContainerFormat::Sfnt;
    let mut sources = Vec::new();
    for src in &src_list.0 {
        match src {
            Source::Local(fam) => sources.push(FontFaceSource::Local(fam.name.to_string())),
            Source::Url(u) => {
                if let Some(embedded) =
                    resolve_url_font_source(u.url.as_str(), &mut format, resolve_embedded)
                {
                    sources.push(embedded);
                }
            }
        }
    }
    if sources.is_empty() {
        return None;
    }
    let (weight_min, weight_max) =
        d.font_weight
            .as_ref()
            .and_then(|w| w.compute())
            .map_or((400, 400), |r| {
                (
                    r.0.value().round().clamp(1.0, 1000.0) as u16,
                    r.1.value().round().clamp(1.0, 1000.0) as u16,
                )
            });
    let style = match d.font_style.as_ref() {
        Some(FontStyleRange::Italic) => FontStyleSlope::Italic,
        Some(FontStyleRange::Oblique(a, _))
            if a.degrees().is_some_and(|deg| deg.abs() > f32::EPSILON) =>
        {
            FontStyleSlope::Oblique
        }
        _ => FontStyleSlope::Normal,
    };
    let unicode_ranges = d
        .unicode_range
        .as_ref()
        .map(|list| {
            list.iter()
                .map(|r| UnicodeRange {
                    start: r.start,
                    end: r.end,
                })
                .collect()
        })
        .unwrap_or_default();
    Some(FontFaceRule {
        family,
        sources,
        container_format: format,
        weight_min,
        weight_max,
        style,
        unicode_ranges,
        metrics_override: FontFaceMetricsOverride {
            ascent_pct: metric_override_to_pct(d.ascent_override.as_ref()),
            descent_pct: metric_override_to_pct(d.descent_override.as_ref()),
            line_gap_pct: metric_override_to_pct(d.line_gap_override.as_ref()),
        },
    })
}
