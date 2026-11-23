use std::path::Path;

/// Authoring targets in `AGENTS.md`: `<= 500` lines/file, `<= 60` lines/fn.
/// Hard CI ceilings include buffer so small local bug fixes and `rustfmt` wraps never force file splits.
const RECOMMENDED_MAX_FILE_LINES: usize = 500;
const HARD_MAX_FILE_LINES: usize = 560;
const RECOMMENDED_MAX_FN_LINES: usize = 60;
const HARD_MAX_FN_LINES: usize = 80;

pub(super) fn verify_crate_forbid_unsafe(rs_file: &Path) {
    let content = std::fs::read_to_string(rs_file).expect("read crate root");
    assert!(
        content.contains("#![forbid(unsafe_code)]"),
        "Crate root `{}` is missing `#![forbid(unsafe_code)]`",
        rs_file.display()
    );
}

pub(super) fn verify_android_ffi_unsafe_budget(crate_dir: &Path) {
    let lib_src = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib_src.contains("#![deny(unsafe_code)]"),
        "krui-android-ffi must declare `#![deny(unsafe_code)]`"
    );
    let readme = std::fs::read_to_string(crate_dir.join("README.md")).unwrap();
    assert!(
        readme.contains("## unsafe 목록"),
        "crates/android-ffi/README.md must document `## unsafe 목록`"
    );
    let mut no_mangle_count = 0usize;
    for entry in std::fs::read_dir(crate_dir.join("src")).unwrap().flatten() {
        if entry.path().extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (idx, line) in lines.iter().enumerate() {
            let code = line.split("//").next().unwrap_or("").trim();
            assert!(
                !code.contains("unsafe {"),
                "Forbidden `unsafe {{}}` block in `{}:{}`",
                entry.path().display(),
                idx + 1
            );
            if code == "#[no_mangle]" {
                no_mangle_count += 1;
                let has_safety = (idx.saturating_sub(4)..idx)
                    .any(|prev| lines[prev].trim_start().starts_with("// SAFETY:"));
                assert!(
                    has_safety,
                    "`#[no_mangle]` at `{}:{}` is missing a preceding `// SAFETY:` comment",
                    entry.path().display(),
                    idx + 1
                );
            }
        }
    }
    assert!(
        no_mangle_count <= 10,
        "krui-android-ffi exceeds <= 10 attribute budget: found {no_mangle_count}"
    );
}

pub(super) fn verify_stylo_bridge_unsafe_budget(crate_dir: &Path, root: &Path) {
    let lib_src = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib_src.contains("#![deny(unsafe_code)]"),
        "krui-stylo-bridge must declare `#![deny(unsafe_code)]`"
    );
    let root_readme = std::fs::read_to_string(root.join("README.md")).unwrap();
    assert!(
        root_readme.contains("krui-stylo-bridge"),
        "Root README.md `## unsafe 목록` must document `krui-stylo-bridge`"
    );
    let mut unsafe_fn_count = 0usize;
    for entry in std::fs::read_dir(crate_dir.join("src")).unwrap().flatten() {
        if entry.path().extension().and_then(|e| e.to_str()) == Some("rs") {
            unsafe_fn_count += audit_stylo_bridge_src_file(&entry.path());
        }
    }
    assert!(
        unsafe_fn_count <= 10,
        "krui-stylo-bridge exceeds <= 10 trait-mandated unsafe fn budget: found {unsafe_fn_count}"
    );
}

fn audit_stylo_bridge_src_file(path: &Path) -> usize {
    let text = std::fs::read_to_string(path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let mut count = 0usize;
    for (idx, line) in lines.iter().enumerate() {
        let code = line.split("//").next().unwrap_or("").trim();
        assert!(
            !code.contains("unsafe {") && !code.contains("unsafe impl"),
            "Forbidden `unsafe` block/impl in `{}:{}`",
            path.display(),
            idx + 1
        );
        if code.starts_with("unsafe fn ") || code.contains(" unsafe fn ") {
            count += 1;
            let lo = idx.saturating_sub(4);
            let hi = (idx + 2).min(lines.len().saturating_sub(1));
            let has_safety = (lo..=hi).any(|i| lines[i].trim_start().starts_with("// SAFETY:"));
            assert!(
                has_safety,
                "`unsafe fn` at `{}:{}` is missing a nearby `// SAFETY:` comment",
                path.display(),
                idx + 1
            );
        }
    }
    count
}

pub(super) fn verify_dir_line_and_fn_limits(dir: &Path, violations: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) != Some("node_modules") {
                verify_dir_line_and_fn_limits(&path, violations);
            }
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "rs" | "js" | "kt") {
            let content = std::fs::read_to_string(&path).expect("read source file");
            let lines: Vec<&str> = content.lines().collect();
            if lines.len() > HARD_MAX_FILE_LINES {
                violations.push(format!(
                    "File `{}` has {} lines (recommended <= {RECOMMENDED_MAX_FILE_LINES}, hard limit <= {HARD_MAX_FILE_LINES})",
                    path.display(),
                    lines.len()
                ));
            }
            if matches!(ext, "rs" | "js") {
                verify_file_function_lengths(&path, &lines, ext == "js", violations);
            }
        }
    }
}

fn verify_file_function_lengths(
    path: &Path,
    lines: &[&str],
    is_js: bool,
    violations: &mut Vec<String>,
) {
    let (mut in_fn, mut fn_start, mut depth, mut seen_brace) = (false, 0_usize, 0_i32, false);
    for (idx, &raw_line) in lines.iter().enumerate() {
        let trimmed = raw_line.trim_start();
        let matched = if is_js {
            trimmed.starts_with("function ") || trimmed.starts_with("async function ")
        } else {
            is_rust_fn_header(trimmed)
        };
        if !in_fn && matched {
            in_fn = true;
            fn_start = idx + 1;
            depth = 0;
            seen_brace = false;
        }
        if in_fn {
            let (opens, closes, ends_semi) = count_code_braces(raw_line);
            if opens > 0 {
                seen_brace = true;
            }
            depth += opens - closes;
            if seen_brace && depth <= 0 {
                let fn_len = (idx + 1) - fn_start + 1;
                if fn_len > HARD_MAX_FN_LINES {
                    violations.push(format!(
                        "Function at `{}:{fn_start}` has {fn_len} lines (recommended <= {RECOMMENDED_MAX_FN_LINES}, hard limit <= {HARD_MAX_FN_LINES})",
                        path.display(),
                    ));
                }
                in_fn = false;
            } else if !seen_brace && ends_semi {
                in_fn = false;
            }
        }
    }
}

fn is_rust_fn_header(mut s: &str) -> bool {
    if s.starts_with("//") || s.starts_with('#') {
        return false;
    }
    if let Some(rest) = s.strip_prefix("pub") {
        s = rest.trim_start();
        if s.starts_with('(') {
            if let Some((_, after_vis)) = s.split_once(')') {
                s = after_vis.trim_start();
            }
        }
    }
    for kw in ["const ", "async ", "unsafe "] {
        if let Some(rest) = s.strip_prefix(kw) {
            s = rest.trim_start();
        }
    }
    if let Some(rest) = s.strip_prefix("extern") {
        s = rest.trim_start();
        if s.starts_with('"') {
            if let Some((_, after_abi)) = s[1..].split_once('"') {
                s = after_abi.trim_start();
            }
        }
    }
    s.starts_with("fn ")
}

fn count_code_braces(line: &str) -> (i32, i32, bool) {
    let code = line.split("//").next().unwrap_or("").trim_end();
    let bytes = code.as_bytes();
    let (mut i, mut in_str, mut escaped, mut opens, mut closes) =
        (0_usize, false, false, 0_i32, 0_i32);
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            in_str = true;
        } else if b == b'\'' {
            if i + 2 < bytes.len() && bytes[i + 1] != b'\\' && bytes[i + 2] == b'\'' {
                i += 3;
                continue;
            }
            if i + 3 < bytes.len() && bytes[i + 1] == b'\\' && bytes[i + 3] == b'\'' {
                i += 4;
                continue;
            }
        } else if b == b'{' {
            opens += 1;
        } else if b == b'}' {
            closes += 1;
        }
        i += 1;
    }
    (opens, closes, code.ends_with(';'))
}
