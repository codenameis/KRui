//! Non-blocking CSS representation-limit and CPU-tier (`kiosk-spi`) lints backed by
//! `docs/ENGINE_LIMITS.toml` invariants.

use crate::css_collect_scan::{advance_css_string, starts_with_ci};
use crate::{CompileWarning, CompileWarningKind};
use krui_style::TierProfile;

const ENGINE_LIMIT_PATTERNS: &[(&str, &str, &str)] = &[
    (
        "conic-gradient(",
        "conic-gradient()",
        "`conic-gradient()` exceeds fast-path 3-stop linear / 2-stop radial shaders (tracked in docs/ENGINE_LIMITS.toml)",
    ),
    (
        "perspective(",
        "transform: perspective()",
        "3D perspective projection (`perspective()`) projects affine z=0 only (tracked in docs/ENGINE_LIMITS.toml)",
    ),
    (
        "rotate3d(",
        "transform: rotate3d()",
        "3D transform (`rotate3d()`) projects affine z=0 only (tracked in docs/ENGINE_LIMITS.toml)",
    ),
    (
        "rotatex(",
        "transform: rotateX()",
        "3D transform (`rotateX()`) projects affine z=0 only (tracked in docs/ENGINE_LIMITS.toml)",
    ),
    (
        "rotatey(",
        "transform: rotateY()",
        "3D transform (`rotateY()`) projects affine z=0 only (tracked in docs/ENGINE_LIMITS.toml)",
    ),
    (
        "translatez(",
        "transform: translateZ()",
        "3D transform (`translateZ()`) projects affine z=0 only (tracked in docs/ENGINE_LIMITS.toml)",
    ),
    (
        "matrix3d(",
        "transform: matrix3d()",
        "`matrix3d()` uses z=0 affine projection only (tracked in docs/ENGINE_LIMITS.toml)",
    ),
];

/// Emits non-blocking compiler warnings for engine representation limits and `kiosk-spi` heavy effects.
pub(crate) fn collect_css_profile_warnings(
    css: &str,
    target_profile: Option<TierProfile>,
    out: &mut Vec<CompileWarning>,
) {
    let cleaned = strip_comments_and_strings(css);
    for &(pat, prop, msg) in ENGINE_LIMIT_PATTERNS {
        if contains_ascii_ci(&cleaned, pat) {
            push_unique_warning(
                out,
                CompileWarningKind::EngineRepresentationLimit,
                prop,
                msg,
            );
        }
    }
    check_complex_clip_path(&cleaned, out);
    if target_profile == Some(TierProfile::CpuKiosk) {
        check_kiosk_heavy_effects(&cleaned, out);
    }
}

fn check_complex_clip_path(cleaned: &str, out: &mut Vec<CompileWarning>) {
    let bytes = cleaned.as_bytes();
    let mut i = 0_usize;
    while i < bytes.len() {
        if is_prop_start(bytes, i) && starts_with_ci(&cleaned[i..], "clip-path") {
            if let Some(colon) = cleaned[i..].find(':') {
                let end = cleaned[i + colon..]
                    .find([';', '}'])
                    .map_or(cleaned.len(), |d| i + colon + d);
                let val = &cleaned[i + colon + 1..end];
                if contains_ascii_ci(val, "path(")
                    || contains_ascii_ci(val, "polygon(")
                    || contains_ascii_ci(val, "shape(")
                {
                    push_unique_warning(
                        out,
                        CompileWarningKind::EngineRepresentationLimit,
                        "clip-path",
                        "Complex polygon/path/shape clip-path is tracked in docs/ENGINE_LIMITS.toml; rounded rect clips only are accelerated",
                    );
                    return;
                }
            }
        }
        i += 1;
    }
}

fn check_kiosk_heavy_effects(cleaned: &str, out: &mut Vec<CompileWarning>) {
    let bytes = cleaned.as_bytes();
    let mut i = 0_usize;
    while i < bytes.len() {
        if !is_prop_start(bytes, i) {
            i += 1;
            continue;
        }
        if starts_with_ci(&cleaned[i..], "backdrop-filter") && has_decl_colon(&cleaned[i + 15..]) {
            push_unique_warning(
                out,
                CompileWarningKind::CpuTierHeavyEffect,
                "backdrop-filter",
                "`backdrop-filter` is ignored or slow on kiosk-spi CPU rasterizer; guard via `html[data-krui-tier=\"cpu\"]`",
            );
        } else if starts_with_ci(&cleaned[i..], "filter") && has_decl_colon(&cleaned[i + 6..]) {
            let val_end = cleaned[i..]
                .find([';', '}'])
                .map_or(cleaned.len(), |d| i + d);
            if contains_ascii_ci(&cleaned[i..val_end], "blur(") {
                push_unique_warning(
                    out,
                    CompileWarningKind::CpuTierHeavyEffect,
                    "filter",
                    "`filter: blur(...)` requires offscreen raster blur on kiosk-spi CPU tier",
                );
            }
        }
        i += 1;
    }
}

fn push_unique_warning(
    out: &mut Vec<CompileWarning>,
    kind: CompileWarningKind,
    property: &str,
    message: &str,
) {
    if out.iter().any(|w| w.kind == kind && w.property == property) {
        return;
    }
    out.push(CompileWarning {
        kind,
        suppressed: false,
        node_tag: "style".to_string(),
        property: property.to_string(),
        signal_expr: String::new(),
        message: message.to_string(),
        suggested_fix:
            "Review docs/ENGINE_LIMITS.toml or provide a `html[data-krui-tier=\"cpu\"]` override."
                .to_string(),
    });
}

fn strip_comments_and_strings(css: &str) -> String {
    let bytes = css.as_bytes();
    let (mut out, mut i, mut quote) = (String::with_capacity(css.len()), 0_usize, None::<u8>);
    while i < bytes.len() {
        if let Some(q) = quote {
            i = advance_css_string(bytes, i, q, &mut quote);
            out.push(' ');
            continue;
        }
        if css[i..].starts_with("/*") {
            let Some(end) = css[i + 2..].find("*/") else {
                break;
            };
            i += 4 + end;
            out.push(' ');
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            quote = Some(bytes[i]);
            i += 1;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

fn contains_ascii_ci(haystack: &str, needle: &str) -> bool {
    let hb = haystack.as_bytes();
    let nb = needle.as_bytes();
    hb.windows(nb.len()).any(|w| w.eq_ignore_ascii_case(nb))
}

fn is_prop_start(bytes: &[u8], pos: usize) -> bool {
    pos == 0 || matches!(bytes[pos - 1], b'{' | b';') || bytes[pos - 1].is_ascii_whitespace()
}

fn has_decl_colon(after: &str) -> bool {
    after.trim_start().starts_with(':')
}

#[cfg(test)]
mod tests {
    use super::collect_css_profile_warnings;
    use crate::CompileWarningKind;
    use krui_style::TierProfile;

    #[test]
    fn emits_non_blocking_engine_limit_and_cpu_kiosk_warnings() {
        let css = r#"
            .a { background: conic-gradient(red, blue); transform: perspective(400px) rotateY(15deg); }
            .b { clip-path: polygon(0 0, 100% 0, 50% 100%); backdrop-filter: blur(8px); filter: blur(4px); }
        "#;
        let mut full_warns = Vec::new();
        collect_css_profile_warnings(css, Some(TierProfile::FullGpu), &mut full_warns);
        assert!(full_warns
            .iter()
            .any(|w| w.kind == CompileWarningKind::EngineRepresentationLimit
                && w.property == "conic-gradient()"));
        assert!(full_warns
            .iter()
            .any(|w| w.kind == CompileWarningKind::EngineRepresentationLimit
                && w.property == "clip-path"));
        assert!(!full_warns
            .iter()
            .any(|w| w.kind == CompileWarningKind::CpuTierHeavyEffect));

        let mut kiosk_warns = Vec::new();
        collect_css_profile_warnings(css, Some(TierProfile::CpuKiosk), &mut kiosk_warns);
        assert!(kiosk_warns
            .iter()
            .any(|w| w.kind == CompileWarningKind::CpuTierHeavyEffect
                && w.property == "backdrop-filter"));
        assert!(kiosk_warns
            .iter()
            .any(|w| w.kind == CompileWarningKind::CpuTierHeavyEffect && w.property == "filter"));
    }
}
