//! Stage `A-fix-4` regression tests for benchmark (`krui-bench`), `probe_krui.ps1` gates,
//! source line/function length budgets (recommended `<= 500`/`<= 60`, hard `<= 560`/`<= 80`),
//! `unsafe` inventory guardrails, and background `DevFileWatcher` idle polling invariants.

#![forbid(unsafe_code)]

#[path = "support/probe_gate_helpers.rs"]
mod helpers;

use std::path::Path;
use std::process::Command as ProcessCommand;

#[test]
fn a_fix_4_krui_bench_fails_on_injected_slow_p99_and_missing_wgpu_backend() {
    let bench_exe = env!("CARGO_BIN_EXE_krui-bench");

    let slow_out = ProcessCommand::new(bench_exe)
        .env("KRUI_BENCH_WARMUP_FRAMES", "2")
        .env("KRUI_BENCH_MEASURED_FRAMES", "5")
        .env("KRUI_BENCH_INJECT_SLOW_US", "9000")
        .output()
        .expect("spawn krui-bench with slow p99 injection");
    assert!(
        !slow_out.status.success(),
        "krui-bench must exit non-zero when p99 exceeds 8,333us: stdout={}, stderr={}",
        String::from_utf8_lossy(&slow_out.stdout),
        String::from_utf8_lossy(&slow_out.stderr)
    );

    let missing_wgpu_out = ProcessCommand::new(bench_exe)
        .env("KRUI_BENCH_WARMUP_FRAMES", "2")
        .env("KRUI_BENCH_MEASURED_FRAMES", "5")
        .env("KRUI_BENCH_FORCE_MISSING_WGPU", "1")
        .output()
        .expect("spawn krui-bench with forced missing wgpu");
    assert!(
        !missing_wgpu_out.status.success(),
        "krui-bench must exit non-zero when Wgpu adapter is unavailable without --allow-missing-wgpu"
    );

    let allowed_missing_out = ProcessCommand::new(bench_exe)
        .arg("--allow-missing-wgpu")
        .env("KRUI_BENCH_WARMUP_FRAMES", "2")
        .env("KRUI_BENCH_MEASURED_FRAMES", "5")
        .env("KRUI_BENCH_BUDGET_US", "50000")
        .env("KRUI_BENCH_FORCE_MISSING_WGPU", "1")
        .output()
        .expect("spawn krui-bench with --allow-missing-wgpu");
    assert!(
        allowed_missing_out.status.success(),
        "krui-bench --allow-missing-wgpu must succeed when Wgpu is forced unavailable: stderr={}",
        String::from_utf8_lossy(&allowed_missing_out.stderr)
    );
}

#[test]
fn a_fix_4_probe_script_fails_on_injected_cargo_tree_and_insufficient_frame_gates() {
    let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/krui-harness2-loop/scripts/probe_krui.ps1");
    assert!(script_path.is_file(), "probe_krui.ps1 must exist");

    for injected_gate in ["cargo_tree", "example_frames"] {
        let out = ProcessCommand::new("pwsh")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                script_path.to_str().unwrap(),
            ])
            .env("KRUI_PROBE_INJECT_GATE_FAILURE", injected_gate)
            .output()
            .expect("spawn probe_krui.ps1 with injected gate failure");
        assert!(
            !out.status.success(),
            "probe_krui.ps1 must exit non-zero when gate `{injected_gate}` fails"
        );
    }
}

#[test]
fn a_fix_4_all_source_files_stay_within_500_lines_and_functions_within_60_lines() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let lib_src = std::fs::read_to_string(root.join("src/lib.rs")).expect("read src/lib.rs");
    assert!(
        lib_src.contains("pub mod __private"),
        "Root `src/lib.rs` must expose `#[doc(hidden)] pub mod __private` for macro codegen decoupling"
    );
    let mut violations = Vec::new();
    for dir_name in ["crates", "src", "examples", "tests", "editors"] {
        helpers::verify_dir_line_and_fn_limits(&root.join(dir_name), &mut violations);
    }
    assert!(
        violations.is_empty(),
        "Source budget violations exceeded hard ceilings:\n{}",
        violations.join("\n")
    );
}

#[test]
fn a_fix_4_unsafe_guardrail_forbid_everywhere_and_android_ffi_budget() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    helpers::verify_crate_forbid_unsafe(&root.join("src/lib.rs"));
    helpers::verify_crate_forbid_unsafe(&root.join("src/main.rs"));
    helpers::verify_crate_forbid_unsafe(&root.join("src/bin/krui-bench.rs"));
    for entry in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        let crate_dir = entry.path();
        if !crate_dir.is_dir() {
            continue;
        }
        let name = crate_dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let lib_rs = crate_dir.join("src/lib.rs");
        if name == "android-ffi" {
            helpers::verify_android_ffi_unsafe_budget(&crate_dir);
        } else if name == "stylo-bridge" {
            helpers::verify_stylo_bridge_unsafe_budget(&crate_dir, root);
        } else if lib_rs.is_file() {
            helpers::verify_crate_forbid_unsafe(&lib_rs);
        }
        let tests_dir = crate_dir.join("tests");
        if tests_dir.is_dir() {
            for t in std::fs::read_dir(tests_dir).unwrap().flatten() {
                if t.path().extension().and_then(|e| e.to_str()) == Some("rs") {
                    helpers::verify_crate_forbid_unsafe(&t.path());
                }
            }
        }
    }
    for ex in std::fs::read_dir(root.join("examples")).unwrap().flatten() {
        let main_rs = ex.path().join("main.rs");
        if main_rs.is_file() {
            helpers::verify_crate_forbid_unsafe(&main_rs);
        }
    }
    for t in std::fs::read_dir(root.join("tests")).unwrap().flatten() {
        if t.path().extension().and_then(|e| e.to_str()) == Some("rs") {
            helpers::verify_crate_forbid_unsafe(&t.path());
        }
    }
    assert!(
        std::fs::read_to_string(root.join("README.md"))
            .unwrap()
            .contains("## unsafe 목록"),
        "Root README.md must contain `## unsafe 목록` table"
    );
}

#[test]
fn correct_agents_md_maintains_rule_enforcement_table_and_m3_gallery_respects_stylesheet_tokens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let agents_md = std::fs::read_to_string(root.join("AGENTS.md")).expect("read AGENTS.md");
    assert!(
        agents_md.contains("## 6. 규칙 강제 매트릭스"),
        "Add missing rule enforcement table section `## 6. 규칙 강제 매트릭스` in AGENTS.md"
    );
    for required_ref in [
        "tests/probe_and_bench_gates.rs",
        "probe_krui.ps1",
        "tests/css_compositor_transitions.rs",
    ] {
        assert!(
            agents_md.contains(required_ref),
            "Add missing rule enforcement row referencing `{required_ref}` in AGENTS.md §6"
        );
    }
    let component_krui = std::fs::read_to_string(
        root.join("examples/component_gallery/views/component_gallery.krui"),
    )
    .expect("read examples/component_gallery/views/component_gallery.krui");
    for required in [
        "krui://preset/unstyled",
        "themes/material.css",
        "themes/tailwind.css",
        r#"data-theme="{controller.theme_slug}""#,
    ] {
        assert!(
            component_krui.contains(required),
            "component_gallery.krui must declare `{required}` for pure-CSS theme switching"
        );
    }
    for removed in ["Fluent", "LiquidGlass", "Cosmic"] {
        assert!(
            !component_krui.contains(removed),
            "component_gallery.krui must not reference removed theme `{removed}`"
        );
    }
    verify_examples_styling_ast(root);
}

struct PureCssAstGate {
    rel: String,
    canvas_depth: usize,
    findings: Vec<String>,
}

impl<'ast> syn::visit::Visit<'ast> for PureCssAstGate {
    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        let name = i.sig.ident.to_string();
        if name == "attach_spinner_driver" || name.starts_with("m3_baseline_") {
            self.findings.push(format!("{}: fn {name}", self.rel));
        }
        syn::visit::visit_item_fn(self, i);
    }

    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        let is_canvas = matches!(&*c.func, syn::Expr::Path(ep) if {
            let s: Vec<_> = ep.path.segments.iter().map(|x| x.ident.to_string()).collect();
            s.len() >= 2 && s[s.len() - 2] == "CanvasDraw" && s[s.len() - 1] == "new"
        });
        if is_canvas {
            self.canvas_depth += 1;
            syn::visit::visit_expr_call(self, c);
            self.canvas_depth -= 1;
        } else {
            syn::visit::visit_expr_call(self, c);
        }
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let id = m.method.to_string();
        if matches!(
            id.as_str(),
            "switch_theme_preset" | "set_theme" | "set_app_tokens" | "apply_theme_preset"
        ) {
            self.findings.push(format!("{}: .{id}()", self.rel));
        }
        syn::visit::visit_expr_method_call(self, m);
    }

    fn visit_path(&mut self, p: &'ast syn::Path) {
        let segs: Vec<String> = p.segments.iter().map(|s| s.ident.to_string()).collect();
        if segs
            .iter()
            .any(|s| s == "ThemePreset" || s.starts_with("m3_baseline_"))
        {
            self.findings
                .push(format!("{}: {}", self.rel, segs.join("::")));
        }
        if self.canvas_depth == 0 && segs.len() >= 2 {
            let tail = (segs[segs.len() - 2].as_str(), segs[segs.len() - 1].as_str());
            if matches!(tail, ("Color", "rgb") | ("Color", "rgba")) {
                self.findings
                    .push(format!("{}: {}::{}", self.rel, tail.0, tail.1));
            }
        }
        syn::visit::visit_path(self, p);
    }
}

fn scan_dir_with_pure_css_ast(root: &Path, sub: &str) -> Vec<String> {
    use syn::visit::Visit as _;
    let mut stack = vec![root.join(sub)];
    let mut rs_files = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                rs_files.push(p);
            }
        }
    }
    rs_files.sort();
    let mut out = Vec::new();
    for f in rs_files {
        let src = std::fs::read_to_string(&f).expect("read rs for AST gate");
        let ast = syn::parse_file(&src).expect("syn parse rs for AST gate");
        let rel = f
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let mut gate = PureCssAstGate {
            rel,
            canvas_depth: 0,
            findings: Vec::new(),
        };
        gate.visit_file(&ast);
        out.extend(gate.findings);
    }
    out
}

fn verify_examples_styling_ast(root: &Path) {
    let example_violations = scan_dir_with_pure_css_ast(root, "examples");
    assert!(
        example_violations.is_empty(),
        "examples/** must stay pure CSS (`syn` AST gate found {} violation(s)):\n{}",
        example_violations.len(),
        example_violations.join("\n")
    );
}

#[cfg(feature = "preview")]
#[test]
fn a_fix_4_dev_file_watcher_without_tcp_does_not_force_periodic_ui_thread_polling() {
    let temp_krui = std::env::temp_dir().join(format!(
        "krui_a_fix_4_idle_watch_{}.krui",
        std::process::id()
    ));
    std::fs::write(&temp_krui, "<div><span>유휴 워커 테스트</span></div>").unwrap();

    let mut watcher =
        krui::dev_hot_reload::DevFileWatcher::new(&temp_krui).expect("attach watcher");
    assert!(
        watcher.needs_periodic_poll(),
        "Before background worker starts, DevFileWatcher requires polling"
    );
    watcher.start_background_worker(|| {});
    assert!(
        !watcher.needs_periodic_poll(),
        "Background DevFileWatcher without TCP client must use waker notifications instead of 200ms UI polling"
    );

    let _ = std::fs::remove_file(&temp_krui);
}
