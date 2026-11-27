#![forbid(unsafe_code)]

//! Design token completeness, Material/Tailwind layout-token parity, and zero-ungrounded-px gate (`G1-tokens`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn parse_theme_custom_props(css: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in css.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("--") {
            continue;
        }
        let Some(colon) = trimmed.find(':') else {
            continue;
        };
        let name = trimmed[..colon].trim().to_string();
        let val = trimmed[colon + 1..]
            .trim()
            .trim_end_matches(';')
            .trim()
            .to_string();
        map.insert(name, val);
    }
    map
}

const REQUIRED_MEASURED_M3_TOKENS: &[(&str, &str)] = &[
    ("--krui-icon-size", "24px"),
    ("--krui-icon-size-sm", "18px"),
    ("--gallery-type-hero-size", "88px"),
    ("--gallery-type-hero-line-height", "96px"),
    ("--gallery-type-title-large-size", "22px"),
    ("--gallery-type-title-large-line-height", "30px"),
    ("--gallery-type-title-medium-size", "16px"),
    ("--gallery-type-title-medium-line-height", "24px"),
    ("--gallery-type-title-small-size", "14px"),
    ("--gallery-type-label-medium-size", "12px"),
    ("--gallery-type-label-medium-line-height", "16px"),
    ("--gallery-type-label-small-size", "11px"),
    ("--gallery-corner-extra-large", "24px"),
    ("--gallery-corner-tabbar", "40px"),
    ("--gallery-rail-width", "88px"),
    ("--gallery-search-fab-size", "56px"),
    ("--gallery-rail-pill-width", "56px"),
    ("--gallery-rail-pill-height", "32px"),
    ("--gallery-rail-toggle-size", "48px"),
    ("--gallery-drawer-width", "240px"),
    ("--gallery-drawer-item-height", "44px"),
    ("--gallery-header-padding", "56px"),
    ("--gallery-tabbar-height", "80px"),
    ("--gallery-toc-item-height", "40px"),
];

#[test]
fn material_and_tailwind_define_measured_m3_tokens_with_layout_parity() {
    let views = repo_root().join("examples/component_gallery/views");
    let mat_css = fs::read_to_string(views.join("themes/material.css")).expect("read material.css");
    let tw_css = fs::read_to_string(views.join("themes/tailwind.css")).expect("read tailwind.css");

    assert!(
        !tw_css.contains("--md-"),
        "tailwind.css must remain independent of Material `--md-*` custom property names"
    );

    let mat_props = parse_theme_custom_props(&mat_css);
    let tw_props = parse_theme_custom_props(&tw_css);

    for &(token, expected_mat_val) in REQUIRED_MEASURED_M3_TOKENS {
        let actual_mat = mat_props
            .get(token)
            .unwrap_or_else(|| panic!("material.css missing measured token `{token}`"));
        assert_eq!(
            actual_mat, expected_mat_val,
            "material.css `{token}` value mismatch vs reference/README.md"
        );
        let actual_tw = tw_props
            .get(token)
            .unwrap_or_else(|| panic!("tailwind.css missing token `{token}`"));
        if !token.starts_with("--gallery-corner-") {
            assert_eq!(
                actual_mat, actual_tw,
                "layout-affecting token `{token}` must match across Material and Tailwind for 0-relayout switch"
            );
        }
    }
}

fn collect_gallery_component_css(views: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let main_css = views.join("component_gallery.css");
    out.push((
        "component_gallery.css".to_string(),
        fs::read_to_string(&main_css).expect("read component_gallery.css"),
    ));
    let styles_dir = views.join("styles");
    if styles_dir.exists() {
        let mut entries: Vec<PathBuf> = fs::read_dir(&styles_dir)
            .expect("read styles dir")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("css"))
            .collect();
        entries.sort();
        for p in entries {
            let name = format!("styles/{}", p.file_name().unwrap().to_string_lossy());
            out.push((name, fs::read_to_string(&p).expect("read zone css")));
        }
    }
    out
}

#[test]
fn gallery_stylesheets_eliminate_ungrounded_legacy_dimensions() {
    let views = repo_root().join("examples/component_gallery/views");
    let sheets = collect_gallery_component_css(&views);
    let banned_legacy = ["height: 58px", "width: 228px", "font-size: 36px"];
    for (name, css) in &sheets {
        for needle in banned_legacy {
            assert!(
                !css.contains(needle),
                "{name} still contains ungrounded legacy dimension `{needle}`"
            );
        }
    }
    let all_concat: String = sheets.iter().map(|(_, c)| c.as_str()).collect();
    assert!(
        all_concat.contains("var(--gallery-rail-width)"),
        "rail CSS must reference var(--gallery-rail-width)"
    );
    assert!(
        all_concat.contains("var(--gallery-tabbar-height)"),
        "tab bar CSS must reference var(--gallery-tabbar-height)"
    );
    assert!(
        all_concat.contains("var(--gallery-type-label-medium-size)"),
        "rail label CSS must reference var(--gallery-type-label-medium-size)"
    );
}
