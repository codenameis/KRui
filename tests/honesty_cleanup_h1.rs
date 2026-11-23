//! Stage H1 honesty cleanup regression tests:
//! 1. Real Tailwind CLI fixture matches real `@tailwindcss/cli` output without fake `@property` overrides.
//! 2. Token pack headers across `examples/component_gallery` pure CSS packs.
//! 3. `syn` AST gate across `examples/**/*.rs` + removal of SHA256 check in `probe_krui.ps1` + pure-token kit swap in `live_preview_component_gallery_e2e.rs`.
//! 4. Measured CSS conformance figure documented in `README.md` and `AGENTS.md`.

#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use syn::visit::{self, Visit};

fn collect_rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_rs_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

struct RustStylingAstVisitor {
    rel_file: String,
    canvas_draw_depth: usize,
    findings: Vec<String>,
}

fn is_canvas_draw_new_call(call: &syn::ExprCall) -> bool {
    let syn::Expr::Path(ep) = &*call.func else {
        return false;
    };
    let segs: Vec<String> = ep
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    segs.len() >= 2 && segs[segs.len() - 2] == "CanvasDraw" && segs[segs.len() - 1] == "new"
}

impl<'ast> Visit<'ast> for RustStylingAstVisitor {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if is_canvas_draw_new_call(call) {
            self.canvas_draw_depth += 1;
            visit::visit_expr_call(self, call);
            self.canvas_draw_depth -= 1;
        } else {
            visit::visit_expr_call(self, call);
        }
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let name = m.method.to_string();
        if matches!(
            name.as_str(),
            "switch_theme_preset" | "set_theme" | "set_app_tokens" | "apply_theme_preset"
        ) {
            self.findings
                .push(format!("{}: .{}()", self.rel_file, name));
        }
        visit::visit_expr_method_call(self, m);
    }

    fn visit_path(&mut self, p: &'ast syn::Path) {
        let segs: Vec<String> = p.segments.iter().map(|s| s.ident.to_string()).collect();
        if segs.iter().any(|s| s == "ThemePreset") {
            self.findings
                .push(format!("{}: {}", self.rel_file, segs.join("::")));
        }
        if self.canvas_draw_depth == 0 && segs.len() >= 2 {
            let pair = (segs[segs.len() - 2].as_str(), segs[segs.len() - 1].as_str());
            if matches!(pair, ("Color", "rgb") | ("Color", "rgba")) {
                self.findings
                    .push(format!("{}: {}::{}", self.rel_file, pair.0, pair.1));
            }
        }
        visit::visit_path(self, p);
    }
}

#[test]
fn h1_tailwind_fixture_is_unmodified_real_cli_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let real_cli =
        fs::read_to_string(root.join("tests/fixtures/real_css/tailwind_v4/tailwind_v4.css"))
            .expect("read real tailwind_v4.css")
            .replace("\r\n", "\n");
    for forbidden in [
        "initial-value: #6366f1",
        "initial-value: #4f46e5",
        "initial-value: #06b6d4",
        "min-height: 760px",
        ".tw-popover",
    ] {
        assert!(
            !real_cli.contains(forbidden),
            "tailwind_v4.css fixture must not contain fake disguise `{forbidden}`"
        );
    }
    assert!(
        real_cli.contains("tailwindcss v4.3.5"),
        "tailwind_v4.css must retain real @tailwindcss/cli banner"
    );
}

#[test]
fn h1_token_pack_headers_use_pure_css_packs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for rel in [
        "examples/component_gallery/views/themes/material.css",
        "examples/component_gallery/views/themes/tailwind.css",
        "examples/component_gallery/views/micl.css",
    ] {
        let content = fs::read_to_string(root.join(rel)).expect(rel);
        let head: String = content.lines().take(8).collect::<Vec<_>>().join("\n");
        assert!(
            head.contains("KRui 토큰 팩("),
            "`{rel}` header must declare `KRui 토큰 팩(○○ 팔레트 참고)`"
        );
    }
}

#[test]
fn h1_examples_syn_ast_gate_probe_cleanup_and_docs_conformance() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_rs_files(&root.join("examples"), &mut files);
    files.sort();
    let mut violations = Vec::new();
    for file in files {
        let src = fs::read_to_string(&file).expect("read example .rs");
        let ast = syn::parse_file(&src).expect("parse example .rs with syn");
        let rel = file
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let mut visitor = RustStylingAstVisitor {
            rel_file: rel,
            canvas_draw_depth: 0,
            findings: Vec::new(),
        };
        visitor.visit_file(&ast);
        violations.extend(visitor.findings);
    }
    assert!(
        violations.is_empty(),
        "examples/**/*.rs must have zero Rust styling AST violations:\n{}",
        violations.join("\n")
    );

    let probe =
        fs::read_to_string(root.join(".agents/skills/krui-harness2-loop/scripts/probe_krui.ps1"))
            .expect("read probe_krui.ps1");
    assert!(
        !probe.contains("Get-FileHash") && !probe.contains("5 distinct SHA256"),
        "probe_krui.ps1 must not check SHA256 stylesheet distinctness"
    );

    let live_e2e =
        fs::read_to_string(root.join("tests/live_preview_component_gallery_e2e.rs")).unwrap();
    assert!(
        !live_e2e.contains(".gallery-shell { background-color:"),
        "live_preview_component_gallery_e2e.rs kit swap must rely on CSS tokens only"
    );

    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    let agents = fs::read_to_string(root.join("AGENTS.md")).unwrap();
    assert!(
        readme.contains("226/229") && agents.contains("226/229"),
        "README.md and AGENTS.md must state measured Tailwind v4 conformance (226/229)"
    );
}
