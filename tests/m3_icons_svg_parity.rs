#![forbid(unsafe_code)]

//! Byte-level SVG path parity and emoji/Unicode-icon ban gate for `component_gallery` (`G1-icons`).

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn extract_attr<'a>(s: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    let start = s.find(&needle)? + needle.len();
    let end = s[start..].find('"')? + start;
    Some(&s[start..end])
}

fn extract_all_path_d(svg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(idx) = rest.find("<path") {
        let after = &rest[idx..];
        let Some(close) = after.find('>') else {
            break;
        };
        let tag = &after[..=close];
        if let Some(d) = extract_attr(tag, "d") {
            out.push(d.to_string());
        }
        rest = &after[close + 1..];
    }
    out
}

fn parse_source_md_entries(source_md: &str) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for line in source_md.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("| `Icon") {
            continue;
        }
        let cols: Vec<&str> = trimmed.split('|').map(str::trim).collect();
        if cols.len() < 6 {
            continue;
        }
        let comp = cols[1].trim_matches('`').to_string();
        let rel_source = cols[4].trim_matches('`').to_string();
        rows.push((comp, rel_source));
    }
    rows
}

#[test]
fn generated_m3_icons_match_official_material_symbols_byte_for_byte() {
    let root = repo_root();
    let icons_dir = root.join("examples/component_gallery/views/icons");
    let source_md_path = icons_dir.join("SOURCE.md");
    let source_md = fs::read_to_string(&source_md_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", source_md_path.display()));
    let entries = parse_source_md_entries(&source_md);
    assert!(
        entries.len() >= 40,
        "expected at least 40 generated Material Symbols in SOURCE.md, found {}",
        entries.len()
    );

    let krui_files: Vec<PathBuf> = fs::read_dir(&icons_dir)
        .expect("read views/icons")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("krui"))
        .collect();
    assert_eq!(
        krui_files.len(),
        entries.len(),
        "every .krui file in views/icons must be listed in SOURCE.md"
    );

    let node_modules = root.join("tools/node_modules");
    for (comp, rel_source) in &entries {
        let krui_path = icons_dir.join(format!("{comp}.krui"));
        let krui_raw = fs::read_to_string(&krui_path)
            .unwrap_or_else(|e| panic!("missing icon component {}: {e}", krui_path.display()));
        assert!(
            krui_raw.contains("fill=\"currentColor\""),
            "{comp}.krui must use fill=\"currentColor\""
        );
        let krui_vb = extract_attr(&krui_raw, "viewBox")
            .unwrap_or_else(|| panic!("{comp}.krui missing viewBox"));
        let krui_paths = extract_all_path_d(&krui_raw);
        assert!(!krui_paths.is_empty(), "{comp}.krui has no <path d=.../>");

        let official_path = node_modules.join(rel_source);
        if official_path.exists() {
            let official_raw = fs::read_to_string(&official_path).expect("read official svg");
            let official_vb =
                extract_attr(&official_raw, "viewBox").expect("official svg missing viewBox");
            let official_paths = extract_all_path_d(&official_raw);
            assert_eq!(krui_vb, official_vb, "viewBox mismatch on {comp}");
            assert_eq!(
                krui_paths, official_paths,
                "SVG <path d> byte mismatch against official {rel_source} on {comp}"
            );
        }
    }
}

fn collect_non_generated_gallery_krui_files(views_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in [views_dir.to_path_buf(), views_dir.join("components")] {
        if !dir.exists() {
            continue;
        }
        let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
            .expect("read gallery views dir")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("krui"))
            .collect();
        entries.sort();
        files.extend(entries);
    }
    files
}

fn is_banned_icon_char(ch: char) -> bool {
    let cp = ch as u32;
    if (0x1F300..=0x1FAFF).contains(&cp) {
        return true;
    }
    matches!(
        ch,
        '▶' | '◀'
            | '▲'
            | '▼'
            | '◈'
            | '▣'
            | '☰'
            | '☑'
            | '✎'
            | '•'
            | '✦'
            | '▾'
            | '★'
            | '♥'
            | '✓'
            | '▪'
            | '↗'
            | '✕'
            | '⚙'
            | '▤'
            | '▥'
            | '◎'
    )
}

#[test]
fn gallery_views_contain_zero_emojis_unicode_icons_or_hand_drawn_svgs() {
    let views_dir = repo_root().join("examples/component_gallery/views");
    let files = collect_non_generated_gallery_krui_files(&views_dir);
    assert!(!files.is_empty(), "expected gallery .krui view files");

    let mut violations = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(repo_root())
            .unwrap_or(&path)
            .display()
            .to_string();
        let src = fs::read_to_string(&path).expect("read .krui");
        for (line_idx, line) in src.lines().enumerate() {
            let line_no = line_idx + 1;
            if line.contains("<svg") {
                violations.push(format!("{rel}:{line_no}: inline hand-drawn <svg> found"));
            }
            for ch in line.chars() {
                if is_banned_icon_char(ch) {
                    violations.push(format!(
                        "{rel}:{line_no}: banned emoji/Unicode icon '{ch}' (U+{:04X})",
                        ch as u32
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found {} banned icon/SVG occurrences in gallery views:\n{}",
        violations.len(),
        violations.join("\n")
    );
}
