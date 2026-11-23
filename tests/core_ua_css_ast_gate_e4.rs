//! Stage E4 `syn` AST gate verifying that `crates/renderer/src` has zero HTML tag or
//! `SemanticRole` branching and `crates/core/src/view_tree` visual/scroll/hit paths only
//! reference the explicit HTML5 / replaced-control semantic allowlist.

#![forbid(unsafe_code)]

use std::path::Path;
use syn::visit::{self, Visit};

struct VisualBranchVisitor {
    file: String,
    cur_fn: String,
    findings: Vec<(String, String, String)>,
}

impl<'ast> Visit<'ast> for VisualBranchVisitor {
    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        let prev = std::mem::replace(&mut self.cur_fn, i.sig.ident.to_string());
        visit::visit_item_fn(self, i);
        self.cur_fn = prev;
    }

    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        let prev = std::mem::replace(&mut self.cur_fn, i.sig.ident.to_string());
        visit::visit_impl_item_fn(self, i);
        self.cur_fn = prev;
    }

    fn visit_path(&mut self, p: &'ast syn::Path) {
        if p.segments.len() >= 2 {
            let n = p.segments.len();
            if p.segments[n - 2].ident == "SemanticRole" {
                let var = p.segments[n - 1].ident.to_string();
                self.findings.push((
                    self.file.clone(),
                    self.cur_fn.clone(),
                    format!("SemanticRole::{var}"),
                ));
            }
        }
        visit::visit_path(self, p);
    }

    fn visit_expr_binary(&mut self, b: &'ast syn::ExprBinary) {
        if matches!(b.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) {
            let lhs = quote_ident_tail(&b.left);
            let rhs = quote_str_lit(&b.right);
            if lhs.as_deref() == Some("_tag") {
                if let Some(tag) = rhs {
                    self.findings.push((
                        self.file.clone(),
                        self.cur_fn.clone(),
                        format!("_tag:{tag}"),
                    ));
                }
            }
        }
        visit::visit_expr_binary(self, b);
    }
}

fn quote_ident_tail(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Field(f) => match &f.member {
            syn::Member::Named(id) => Some(id.to_string()),
            _ => None,
        },
        _ => None,
    }
}

fn quote_str_lit(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => Some(s.value()),
        _ => None,
    }
}

const VISUAL_SEMANTICS_ALLOWLIST: &[(&str, &str)] = &[
    // Replaced form/canvas widget indicator & single-line text measurement semantics
    ("paint.rs", "SemanticRole::Checkbox"),
    ("paint.rs", "SemanticRole::RadioButton"),
    ("paint.rs", "SemanticRole::Slider"),
    ("paint.rs", "SemanticRole::ComboBox"),
    ("paint.rs", "SemanticRole::Canvas"),
    ("scoped_tokens.rs", "_tag:html"),
    ("scoped_tokens.rs", "_tag:body"),
    ("paint_canvas.rs", "SemanticRole::Checkbox"),
    ("paint_canvas.rs", "SemanticRole::RadioButton"),
    ("paint_canvas.rs", "SemanticRole::Slider"),
    ("paint_canvas.rs", "SemanticRole::ComboBox"),
    ("paint_text.rs", "SemanticRole::Button"),
    ("paint_text.rs", "SemanticRole::ComboBox"),
    ("paint_text.rs", "SemanticRole::TextInput"),
    ("paint_text.rs", "SemanticRole::PasswordInput"),
    ("paint_text.rs", "SemanticRole::MultilineTextInput"),
    // HTML5 Top-Layer <dialog> modal promotion
    ("stacking.rs", "_tag:dialog"),
    ("stacking_toplayer.rs", "_tag:dialog"),
    // Editable <textarea> internal line buffer scroll
    ("scroll.rs", "SemanticRole::MultilineTextInput"),
    // Single-line text shaping cache invalidation & interactive hit semantics
    ("hit_test.rs", "SemanticRole::Button"),
    ("hit_test.rs", "_tag:label"),
    ("hit_test.rs", "_tag:summary"),
    ("hit_test_geom.rs", "SemanticRole::Button"),
    ("hit_test_geom.rs", "SemanticRole::TextInput"),
    ("hit_test_geom.rs", "SemanticRole::PasswordInput"),
];

#[test]
fn syn_ast_gate_forbids_visual_tag_or_role_branching_in_core_and_renderer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert_renderer_has_zero_tag_or_role_branches(root);
    assert_core_visual_files_obey_allowlist(root);
}

fn assert_renderer_has_zero_tag_or_role_branches(root: &Path) {
    for entry in std::fs::read_dir(root.join("crates/renderer/src")).expect("renderer/src") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let fname = path.file_name().unwrap().to_string_lossy().to_string();
        let ast = syn::parse_file(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut vis = VisualBranchVisitor {
            file: fname,
            cur_fn: String::new(),
            findings: Vec::new(),
        };
        vis.visit_file(&ast);
        assert!(
            vis.findings.is_empty(),
            "renderer findings: {:?}",
            vis.findings
        );
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn assert_core_visual_files_obey_allowlist(root: &Path) {
    let core_dir = root.join("crates/core/src/view_tree");
    let mut files = Vec::new();
    collect_rs_files(&core_dir, &mut files);
    for path in files {
        let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let is_visual = fname.starts_with("paint")
            || fname.starts_with("stacking")
            || fname == "scroll.rs"
            || fname == "scoped_tokens.rs"
            || fname.starts_with("hit_test")
            || fname.starts_with("insets");
        if !is_visual {
            continue;
        }
        let ast = syn::parse_file(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut vis = VisualBranchVisitor {
            file: fname.to_string(),
            cur_fn: String::new(),
            findings: Vec::new(),
        };
        vis.visit_file(&ast);
        for (file, func, kind) in vis.findings {
            assert!(
                VISUAL_SEMANTICS_ALLOWLIST.contains(&(file.as_str(), kind.as_str())),
                "Forbidden visual tag/role branch `{kind}` in `{file}::{func}`"
            );
        }
    }
}
