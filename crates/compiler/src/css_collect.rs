//! Stylesheet collection, recursive `@import` resolution, relative `@font-face` embedding,
//! and `@layer component` / `@scope` wrapping for built-in and external component stylesheets.

use crate::css_collect_scan::{
    advance_css_string, find_ci, find_ci_at_boundary, find_closing_quote, find_matching_brace,
    find_matching_paren, find_top_level_semicolon, is_ws_or_quote_or_paren, starts_with_ci,
    wrap_component_scope,
};
use crate::source_loader::{read_text_with_virtual, resolve_path_with_virtual, VirtualBufferMap};
use crate::CompileError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Collects component and author CSS stylesheets in W3C cascade order for Stylo evaluation.
#[derive(Default)]
pub(crate) struct CollectedStylesheets {
    pub(crate) unstyled: bool,
    pub(crate) component_sheets: Vec<String>,
    pub(crate) author_sheets: Vec<String>,
    pub(crate) embedded_font_files: Vec<(String, PathBuf)>,
    pub(crate) external_css_paths: Vec<PathBuf>,
    font_key_by_path: HashMap<PathBuf, String>,
    import_stack: Vec<PathBuf>,
    next_font_id: usize,
}

impl CollectedStylesheets {
    pub(crate) fn into_stylesheets(self) -> (Vec<String>, Vec<(String, PathBuf)>, Vec<PathBuf>) {
        let mut all = Vec::with_capacity(self.component_sheets.len() + self.author_sheets.len());
        all.extend(self.component_sheets);
        all.extend(self.author_sheets);
        (all, self.embedded_font_files, self.external_css_paths)
    }

    pub(crate) fn push_builtin_component_css(&mut self, css: &str) {
        let trimmed = css.trim();
        if !trimmed.is_empty() {
            self.component_sheets
                .push(format!("@layer component {{\n{trimmed}\n}}"));
        }
    }

    pub(crate) fn push_component_css(
        &mut self,
        full_source: &str,
        base_offset: usize,
        css: &str,
        file: Option<&Path>,
        comp_scope: Option<&str>,
        vbufs: Option<&VirtualBufferMap>,
    ) -> Result<(), CompileError> {
        let expanded = self.expand_and_rewrite_css(full_source, base_offset, css, file, vbufs)?;
        let trimmed = expanded.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let wrapped = match comp_scope {
            Some(scope) if !scope.is_empty() => wrap_component_scope(scope, trimmed),
            _ => format!("@layer component {{\n{trimmed}\n}}"),
        };
        self.component_sheets.push(wrapped);
        Ok(())
    }

    pub(crate) fn push_author_css(
        &mut self,
        full_source: &str,
        base_offset: usize,
        css: &str,
        file: Option<&Path>,
        vbufs: Option<&VirtualBufferMap>,
    ) -> Result<(), CompileError> {
        let expanded = self.expand_and_rewrite_css(full_source, base_offset, css, file, vbufs)?;
        if !expanded.trim().is_empty() {
            self.author_sheets.push(expanded);
        }
        Ok(())
    }

    fn expand_and_rewrite_css(
        &mut self,
        full_source: &str,
        base_offset: usize,
        css: &str,
        file: Option<&Path>,
        vbufs: Option<&VirtualBufferMap>,
    ) -> Result<String, CompileError> {
        let base_dir = file.and_then(Path::parent);
        let with_imports =
            expand_imports(self, (full_source, base_offset, css), file, base_dir, vbufs)?;
        Ok(rewrite_font_face_urls(self, &with_imports, base_dir))
    }

    fn register_embedded_font(&mut self, resolved: PathBuf) -> String {
        if let Some(existing) = self.font_key_by_path.get(&resolved) {
            return existing.clone();
        }
        let fname = resolved
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("font.bin");
        let sanitized: String = fname
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let key = format!("f{}_{}", self.next_font_id, sanitized);
        self.next_font_id += 1;
        self.font_key_by_path.insert(resolved.clone(), key.clone());
        self.embedded_font_files.push((key.clone(), resolved));
        key
    }
}

fn make_err(
    file: Option<&Path>,
    source: &str,
    offset: usize,
    msg: impl Into<String>,
) -> CompileError {
    let mut err = CompileError::at_offset(source, offset, msg);
    if let Some(p) = file {
        err = err.with_file(p);
    }
    err
}

fn expand_imports(
    collector: &mut CollectedStylesheets,
    (full_source, base_offset, css): (&str, usize, &str),
    file: Option<&Path>,
    base_dir: Option<&Path>,
    vbufs: Option<&VirtualBufferMap>,
) -> Result<String, CompileError> {
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len());
    let (mut i, mut copy_from, mut depth, mut quote) = (0usize, 0usize, 0usize, None::<u8>);

    while i < bytes.len() {
        if let Some(q) = quote {
            i = advance_css_string(bytes, i, q, &mut quote);
            continue;
        }
        if css[i..].starts_with("/*") {
            let Some(end) = css[i + 2..].find("*/") else {
                return Err(make_err(
                    file,
                    full_source,
                    base_offset + i,
                    "Unclosed `/*` CSS comment",
                ));
            };
            i += 4 + end;
            continue;
        }
        match bytes[i] {
            b'"' | b'\'' => quote = Some(bytes[i]),
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'@' if depth == 0
                && starts_with_ci(&css[i..], "@import")
                && is_ws_or_quote_or_paren(&css[i + 7..]) =>
            {
                let stmt_end = find_top_level_semicolon(css, i).ok_or_else(|| {
                    make_err(
                        file,
                        full_source,
                        base_offset + i,
                        "Unterminated `@import` rule",
                    )
                })?;
                out.push_str(&css[copy_from..i]);
                let inlined = inline_single_import(
                    collector,
                    (full_source, base_offset + i, &css[i..stmt_end]),
                    file,
                    base_dir,
                    vbufs,
                )?;
                out.push_str(&inlined);
                out.push('\n');
                i = stmt_end + 1;
                copy_from = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.push_str(&css[copy_from..]);
    Ok(out)
}

fn inline_single_import(
    collector: &mut CollectedStylesheets,
    (full_source, abs_off, stmt): (&str, usize, &str),
    file: Option<&Path>,
    base_dir: Option<&Path>,
    vbufs: Option<&VirtualBufferMap>,
) -> Result<String, CompileError> {
    let after_kw = stmt[7..].trim();
    let (target, tail) = parse_import_target_and_tail(after_kw).ok_or_else(|| {
        make_err(
            file,
            full_source,
            abs_off,
            format!("Invalid CSS `@import` syntax: `{stmt}`"),
        )
    })?;
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:") {
        return Ok(format!("{stmt};"));
    }
    let dir = base_dir.ok_or_else(|| {
        make_err(
            file,
            full_source,
            abs_off,
            format!("External `@import \"{target}\"` requires file-backed `.krui` compilation (`include_krui!`)"),
        )
    })?;
    let candidate = dir.join(target);
    let canonical = resolve_path_with_virtual(&candidate, vbufs).map_err(|e| {
        make_err(
            file,
            full_source,
            abs_off,
            format!(
                "Failed to resolve imported CSS `{target}` (`{}`): {e}",
                candidate.display()
            ),
        )
    })?;
    if collector.import_stack.contains(&canonical) {
        return Err(make_err(
            file,
            full_source,
            abs_off,
            format!(
                "Cyclic CSS `@import` detected for `{}`",
                canonical.display()
            ),
        ));
    }
    if !collector.external_css_paths.contains(&canonical) {
        collector.external_css_paths.push(canonical.clone());
    }
    let imported_src = read_text_with_virtual(&canonical, vbufs).map_err(|e| {
        make_err(
            file,
            full_source,
            abs_off,
            format!("Failed to read imported CSS `{}`: {e}", canonical.display()),
        )
    })?;
    collector.import_stack.push(canonical.clone());
    let expanded =
        collector.expand_and_rewrite_css(&imported_src, 0, &imported_src, Some(&canonical), vbufs);
    collector.import_stack.pop();
    Ok(wrap_import_modifiers(&expanded?, tail))
}

fn parse_import_target_and_tail(raw: &str) -> Option<(&str, &str)> {
    let trimmed = raw.trim();
    if let Some(first) = trimmed.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let end = find_closing_quote(&trimmed[1..], first)?;
        return Some((&trimmed[1..1 + end], trimmed[1 + end + 1..].trim()));
    }
    if starts_with_ci(trimmed, "url(") {
        let paren_end = find_matching_paren(trimmed, 3)?;
        let inner = trimmed[4..paren_end]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'');
        return Some((inner, trimmed[paren_end + 1..].trim()));
    }
    None
}

fn wrap_import_modifiers(body: &str, mut tail: &str) -> String {
    let mut wrapped = body.to_string();
    let mut layers = Vec::new();
    let mut supports_cond = None;
    loop {
        tail = tail.trim();
        if tail.is_empty() {
            break;
        }
        if starts_with_ci(tail, "layer(") {
            if let Some(end) = find_matching_paren(tail, 5) {
                layers.push(Some(tail[6..end].trim().to_string()));
                tail = &tail[end + 1..];
                continue;
            }
        } else if starts_with_ci(tail, "layer")
            && tail[5..].chars().next().is_none_or(|c| c.is_whitespace())
        {
            layers.push(None);
            tail = &tail[5..];
            continue;
        } else if starts_with_ci(tail, "supports(") {
            if let Some(end) = find_matching_paren(tail, 8) {
                supports_cond = Some(tail[9..end].trim().to_string());
                tail = &tail[end + 1..];
                continue;
            }
        }
        break;
    }
    if !tail.is_empty() {
        wrapped = format!("@media {tail} {{\n{wrapped}\n}}");
    }
    if let Some(sup) = supports_cond {
        let cond = if sup.starts_with('(') {
            sup
        } else {
            format!("({sup})")
        };
        wrapped = format!("@supports {cond} {{\n{wrapped}\n}}");
    }
    for layer_opt in layers.into_iter().rev() {
        match layer_opt {
            Some(name) if !name.is_empty() => wrapped = format!("@layer {name} {{\n{wrapped}\n}}"),
            _ => wrapped = format!("@layer {{\n{wrapped}\n}}"),
        }
    }
    wrapped
}

fn rewrite_font_face_urls(
    collector: &mut CollectedStylesheets,
    css: &str,
    base_dir: Option<&Path>,
) -> String {
    let Some(dir) = base_dir else {
        return css.to_string();
    };
    let mut out = String::with_capacity(css.len());
    let (mut i, mut copy_from) = (0usize, 0usize);
    while i < css.len() {
        if let Some(rel) = find_ci_at_boundary(&css[i..], "@font-face") {
            let abs = i + rel;
            let Some(brace_rel) = css[abs..].find('{') else {
                break;
            };
            let body_start = abs + brace_rel + 1;
            let Some(block_end) = find_matching_brace(css, abs + brace_rel) else {
                break;
            };
            out.push_str(&css[copy_from..body_start]);
            out.push_str(&rewrite_urls_in_block(
                collector,
                &css[body_start..block_end],
                dir,
            ));
            i = block_end;
            copy_from = block_end;
        } else {
            break;
        }
    }
    out.push_str(&css[copy_from..]);
    out
}

fn rewrite_urls_in_block(
    collector: &mut CollectedStylesheets,
    block: &str,
    base_dir: &Path,
) -> String {
    let mut out = String::with_capacity(block.len());
    let (mut i, mut copy_from) = (0usize, 0usize);
    while let Some(rel) = find_ci(&block[i..], "url(") {
        let start = i + rel;
        let Some(end) = find_matching_paren(block, start + 3) else {
            break;
        };
        let raw_url = block[start + 4..end]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .trim();
        let lower = raw_url.to_ascii_lowercase();
        let is_remote_or_data = raw_url.is_empty()
            || lower.starts_with("data:")
            || lower.starts_with("http:")
            || lower.starts_with("https:")
            || lower.starts_with("krui:")
            || raw_url.starts_with('#');
        if !is_remote_or_data {
            let cand = base_dir.join(raw_url);
            if cand.is_file() {
                let canon = std::fs::canonicalize(&cand).unwrap_or(cand);
                let key = collector.register_embedded_font(canon);
                out.push_str(&block[copy_from..start]);
                out.push_str(&format!("url(\"krui://inline/{key}\")"));
                i = end + 1;
                copy_from = i;
                continue;
            }
        }
        i = end + 1;
    }
    out.push_str(&block[copy_from..]);
    out
}
