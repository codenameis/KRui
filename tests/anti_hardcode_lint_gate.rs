#![forbid(unsafe_code)]

#[path = "support/anti_hardcode_baseline.rs"]
mod baseline;

use krui::*;
use std::path::Path;
use syn::visit::{self, Visit};

const TIER_A_RUNTIME_CRATE_SRCS: &[&str] = &[
    "crates/renderer/src",
    "crates/layout/src",
    "crates/a11y/src",
    "crates/signals/src",
    "crates/android-ffi/src",
];

const TIER_B_RUNTIME_CRATE_SRCS: &[&str] =
    &["crates/core/src", "crates/winit/src", "crates/patterns/src"];

struct HardcodedBranchVisitor {
    rel_file: String,
    cur_fn: String,
    findings: Vec<String>,
}

impl HardcodedBranchVisitor {
    fn push(&mut self, kind: &str, value: impl AsRef<str>) {
        self.findings.push(format!(
            "{}|{}|{}|{}",
            self.rel_file,
            self.cur_fn,
            kind,
            value.as_ref()
        ));
    }

    fn check_theme_preset_path(&mut self, path: &syn::Path) {
        if let Some(variant) = theme_preset_variant(path) {
            self.push("theme_preset_branch", variant);
        }
    }

    fn collect_pat_findings(&mut self, pat: &syn::Pat, str_kind: &str) {
        match pat {
            syn::Pat::Lit(pl) => {
                if let syn::Lit::Str(s) = &pl.lit {
                    self.push(str_kind, s.value());
                }
            }
            syn::Pat::Path(pp) => self.check_theme_preset_path(&pp.path),
            syn::Pat::TupleStruct(ts) => {
                self.check_theme_preset_path(&ts.path);
                for elem in &ts.elems {
                    self.collect_pat_findings(elem, str_kind);
                }
            }
            syn::Pat::Struct(ps) => {
                self.check_theme_preset_path(&ps.path);
                for f in &ps.fields {
                    self.collect_pat_findings(&f.pat, str_kind);
                }
            }
            syn::Pat::Or(po) => po
                .cases
                .iter()
                .for_each(|c| self.collect_pat_findings(c, str_kind)),
            syn::Pat::Tuple(pt) => pt
                .elems
                .iter()
                .for_each(|e| self.collect_pat_findings(e, str_kind)),
            syn::Pat::Slice(ps) => ps
                .elems
                .iter()
                .for_each(|e| self.collect_pat_findings(e, str_kind)),
            syn::Pat::Reference(pr) => self.collect_pat_findings(&pr.pat, str_kind),
            syn::Pat::Paren(pp) => self.collect_pat_findings(&pp.pat, str_kind),
            syn::Pat::Type(pt) => self.collect_pat_findings(&pt.pat, str_kind),
            syn::Pat::Ident(pi) => {
                if let Some((_, sub)) = &pi.subpat {
                    self.collect_pat_findings(sub, str_kind);
                }
            }
            _ => {}
        }
    }
}

struct MatchesArgs {
    expr: syn::Expr,
    pat: syn::Pat,
    guard: Option<syn::Expr>,
}

impl syn::parse::Parse for MatchesArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let expr: syn::Expr = input.parse()?;
        let _: syn::Token![,] = input.parse()?;
        let pat = syn::Pat::parse_multi_with_leading_vert(input)?;
        let guard = if input.peek(syn::Token![if]) {
            let _: syn::Token![if] = input.parse()?;
            Some(input.parse()?)
        } else {
            None
        };
        let _ = input.parse::<Option<syn::Token![,]>>();
        Ok(Self { expr, pat, guard })
    }
}

impl<'ast> Visit<'ast> for HardcodedBranchVisitor {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        if !is_test_only_item(&i.attrs) {
            visit::visit_item_mod(self, i);
        }
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if !is_test_only_item(&i.attrs) {
            visit::visit_item_impl(self, i);
        }
    }

    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        if is_test_only_item(&i.attrs) {
            return;
        }
        let prev = std::mem::replace(&mut self.cur_fn, i.sig.ident.to_string());
        visit::visit_item_fn(self, i);
        self.cur_fn = prev;
    }

    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        if is_test_only_item(&i.attrs) {
            return;
        }
        let prev = std::mem::replace(&mut self.cur_fn, i.sig.ident.to_string());
        visit::visit_impl_item_fn(self, i);
        self.cur_fn = prev;
    }

    fn visit_expr_binary(&mut self, b: &'ast syn::ExprBinary) {
        if matches!(b.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) {
            for side in [&*b.left, &*b.right] {
                for lit in extract_str_literals_from_expr(side) {
                    self.push("str_cmp", lit);
                }
                for variant in extract_theme_presets_from_expr(side) {
                    self.push("theme_preset_branch", variant);
                }
            }
        }
        visit::visit_expr_binary(self, b);
    }

    fn visit_expr_let(&mut self, l: &'ast syn::ExprLet) {
        self.collect_pat_findings(&l.pat, "if_let_str");
        visit::visit_expr_let(self, l);
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let method = m.method.to_string();
        if matches!(
            method.as_str(),
            "starts_with" | "ends_with" | "contains" | "eq_ignore_ascii_case"
        ) {
            if let Some(arg) = m.args.first() {
                for lit in extract_str_literals_from_expr(arg) {
                    self.push(&format!("str_{method}"), lit);
                }
            }
        }
        visit::visit_expr_method_call(self, m);
    }

    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        self.collect_pat_findings(&arm.pat, "match_str");
        visit::visit_arm(self, arm);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "matches")
        {
            if let Ok(args) = syn::parse2::<MatchesArgs>(mac.tokens.clone()) {
                self.visit_expr(&args.expr);
                self.collect_pat_findings(&args.pat, "matches_str");
                if let Some(guard) = &args.guard {
                    self.visit_expr(guard);
                }
            }
        }
        visit::visit_macro(self, mac);
    }
}

fn is_test_only_item(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test") || (attr.path().is_ident("cfg") && cfg_mentions_test(attr))
    })
}

fn cfg_mentions_test(attr: &syn::Attribute) -> bool {
    let mut found = false;
    let _ = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("test") {
            found = true;
        } else if meta.path.is_ident("all") || meta.path.is_ident("any") {
            let _ = meta.parse_nested_meta(|inner| {
                if inner.path.is_ident("test") {
                    found = true;
                }
                Ok(())
            });
        }
        Ok(())
    });
    found
}

fn theme_preset_variant(path: &syn::Path) -> Option<String> {
    let n = path.segments.len();
    (n >= 2 && path.segments[n - 2].ident == "ThemePreset")
        .then(|| path.segments[n - 1].ident.to_string())
}

fn unwrap_wrapped_expr(expr: &syn::Expr) -> &syn::Expr {
    match expr {
        syn::Expr::Paren(p) => unwrap_wrapped_expr(&p.expr),
        syn::Expr::Group(g) => unwrap_wrapped_expr(&g.expr),
        syn::Expr::Reference(r) => unwrap_wrapped_expr(&r.expr),
        syn::Expr::Call(c)
            if c.args.len() == 1
                && matches!(&*c.func, syn::Expr::Path(ep)
                    if ep.path.segments.last().is_some_and(|s| s.ident == "Some")) =>
        {
            unwrap_wrapped_expr(&c.args[0])
        }
        _ => expr,
    }
}

fn extract_str_literals_from_expr(expr: &syn::Expr) -> Vec<String> {
    match unwrap_wrapped_expr(expr) {
        syn::Expr::Lit(el) => match &el.lit {
            syn::Lit::Str(s) => vec![s.value()],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn extract_theme_presets_from_expr(expr: &syn::Expr) -> Vec<String> {
    match unwrap_wrapped_expr(expr) {
        syn::Expr::Path(ep) => theme_preset_variant(&ep.path).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn scan_rust_source(rel_file: &str, src: &str) -> Vec<String> {
    let ast = syn::parse_file(src).expect("parse rust source");
    let mut visitor = HardcodedBranchVisitor {
        rel_file: rel_file.to_string(),
        cur_fn: "<module>".to_string(),
        findings: Vec::new(),
    };
    visitor.visit_file(&ast);
    visitor.findings.sort();
    visitor.findings
}

fn collect_rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
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

fn scan_crate_dirs(root: &Path, dirs: &[&str]) -> Vec<String> {
    let mut files = Vec::new();
    for dir in dirs {
        collect_rs_files(&root.join(dir), &mut files);
    }
    files.sort();
    let mut all = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let src = std::fs::read_to_string(&f).expect("read crate rs");
        all.extend(scan_rust_source(&rel, &src));
    }
    all.sort();
    all
}

#[test]
fn tier_a_runtime_crates_have_zero_hardcoded_string_or_theme_branches() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let findings = scan_crate_dirs(root, TIER_A_RUNTIME_CRATE_SRCS);
    assert!(
        findings.is_empty(),
        "Tier A runtime crates must have ZERO hardcoded string or ThemePreset branches:\n{}",
        findings.join("\n")
    );
}

fn diff_sorted_multiset(actual: &[String], expected: &[String]) -> (Vec<String>, Vec<String>) {
    let (mut unexpected, mut stale) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0_usize, 0_usize);
    while i < actual.len() && j < expected.len() {
        match actual[i].cmp(&expected[j]) {
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => {
                unexpected.push(actual[i].clone());
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                stale.push(expected[j].clone());
                j += 1;
            }
        }
    }
    unexpected.extend_from_slice(&actual[i..]);
    stale.extend_from_slice(&expected[j..]);
    (unexpected, stale)
}

#[test]
fn tier_b_runtime_crates_enforce_strict_bidirectional_hardcode_ratchet() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let actual = scan_crate_dirs(root, TIER_B_RUNTIME_CRATE_SRCS);
    assert!(
        !actual
            .iter()
            .any(|f| f.starts_with("crates/core/")
                && (f.ends_with("|group") || f.ends_with("|group/"))),
        "crates/core must never hardcode Tailwind group classes: {actual:?}"
    );
    let mut expected: Vec<String> = baseline::LEGACY_RUNTIME_HARDCODE_BASELINE
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    expected.sort();
    let (unexpected, stale) = diff_sorted_multiset(&actual, &expected);
    assert!(
        unexpected.is_empty() && stale.is_empty(),
        "Tier B anti-hardcode ratchet violated!\n\
         Use typed enums, bitflags (InvalidationFlags), or CSS selectors instead of hardcoded `if ==` / `match` string branches.\n\
         Unexpected NEW findings ({}):\n  {}\n\
         Stale REMOVED baseline entries ({}) — shrink LEGACY_RUNTIME_HARDCODE_BASELINE:\n  {}",
        unexpected.len(),
        if unexpected.is_empty() { "<none>".into() } else { unexpected.join("\n  ") },
        stale.len(),
        if stale.is_empty() { "<none>".into() } else { stale.join("\n  ") },
    );
}

#[test]
fn anti_hardcode_lint_catches_synthetic_violations_and_allows_typed_code() {
    let bad = r#"
        fn bad(tag: &str, class: &str, opt: Option<&str>, preset: ThemePreset) -> i32 {
            if class == "group" || "button" == tag || opt == Some("active") { return 1; }
            if class.starts_with("group/") || tag.eq_ignore_ascii_case("dark") { return 2; }
            if matches!(tag, "material" | "fluent") { return 3; }
            if preset == ThemePreset::Cosmic { return 4; }
            if let Some("raw") = opt { return 5; }
            match tag { "dialog" => 6, _ => 0 }
        }
    "#;
    let found = scan_rust_source("synthetic.rs", bad);
    assert_eq!(
        found,
        vec![
            "synthetic.rs|bad|if_let_str|raw",
            "synthetic.rs|bad|match_str|dialog",
            "synthetic.rs|bad|matches_str|fluent",
            "synthetic.rs|bad|matches_str|material",
            "synthetic.rs|bad|str_cmp|active",
            "synthetic.rs|bad|str_cmp|button",
            "synthetic.rs|bad|str_cmp|group",
            "synthetic.rs|bad|str_eq_ignore_ascii_case|dark",
            "synthetic.rs|bad|str_starts_with|group/",
            "synthetic.rs|bad|theme_preset_branch|Cosmic",
        ]
    );

    let good = r#"
        fn ok(flags: InvalidationFlags, role: SemanticRole) -> bool {
            if flags.contains(InvalidationFlags::SELF_HOVER) { return true; }
            match role { SemanticRole::Button => true, _ => false }
        }
        #[cfg(test)]
        mod tests {
            #[test]
            fn test_only() { let _ = ("a" == "b"); }
        }
    "#;
    assert!(scan_rust_source("clean.rs", good).is_empty());
}

#[test]
fn generic_ancestor_hover_works_without_tailwind_group_hardcoding() {
    let mut tree = ViewTree::new();
    tree.mount_with(|| {
        krui!(
            r#"
            <link rel="krui-preset" href="krui://preset/unstyled" />
            <style>
                .custom-card { width: 220px; height: 120px; padding: 16px; background-color: #1e293b; }
                .custom-badge { width: 60px; height: 24px; background-color: #ef4444; }
                .custom-card:hover .custom-badge { background-color: #22c55e; }
            </style>
            <div id="card" class="custom-card">
                <div id="badge" class="custom-badge"></div>
            </div>
            "#
        )
    });
    let _ = tree.build_paint_scene(320, 200);
    let base_relayouts = tree.relayout_passes();
    let card_r = tree.node_layout(tree.find_by_id("card").unwrap()).unwrap();
    let badge_id = tree.find_by_id("badge").unwrap();
    let before = tree
        .node_effective_paint(badge_id)
        .unwrap()
        .background_color;
    assert_ne!(before, Color::parse_css("#22c55e").unwrap());

    assert!(tree.dispatch_pointer_move(card_r.x + 8.0, card_r.y + 8.0));
    let _ = tree.build_paint_scene(320, 200);
    let after = tree
        .node_effective_paint(badge_id)
        .unwrap()
        .background_color;
    assert_eq!(after, Color::parse_css("#22c55e").unwrap());
    assert_eq!(tree.relayout_passes(), base_relayouts);
}
