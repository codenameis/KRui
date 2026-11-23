//! Linked/companion CSS loading and in-memory `VirtualBufferMap` overrides.

use crate::css_collect::CollectedStylesheets;
use crate::CompileError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Map of workspace file paths to unsaved in-memory UTF-8 buffer contents for live preview.
pub type VirtualBufferMap = HashMap<PathBuf, String>;

fn lexical_clean_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(
                    out.components().next_back(),
                    None | Some(Component::ParentDir | Component::RootDir | Component::Prefix(_))
                ) {
                    out.pop();
                } else {
                    out.push(comp);
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn normalize_virtual_key(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| lexical_clean_path(path));
    let s = resolved.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        s.to_ascii_lowercase()
    } else {
        s
    }
}

/// Looks up `path` inside an optional [`VirtualBufferMap`] using exact or normalized path matching.
pub(crate) fn lookup_virtual_buffer<'a>(
    virtual_buffers: Option<&'a VirtualBufferMap>,
    path: &Path,
) -> Option<&'a str> {
    let map = virtual_buffers?;
    if let Some(direct) = map.get(path) {
        return Some(direct.as_str());
    }
    let needle = normalize_virtual_key(path);
    for (k, v) in map {
        if normalize_virtual_key(k) == needle {
            return Some(v.as_str());
        }
    }
    None
}

/// Resolves `path` to canonical form or accepts it when an in-memory virtual buffer exists.
pub(crate) fn resolve_path_with_virtual(
    path: &Path,
    virtual_buffers: Option<&VirtualBufferMap>,
) -> Result<PathBuf, std::io::Error> {
    match std::fs::canonicalize(path) {
        Ok(canonical) => Ok(canonical),
        Err(err) => {
            if lookup_virtual_buffer(virtual_buffers, path).is_some() {
                Ok(path.to_path_buf())
            } else {
                Err(err)
            }
        }
    }
}

/// Reads UTF-8 source from `virtual_buffers` if present, otherwise from disk at `path`.
pub(crate) fn read_text_with_virtual(
    path: &Path,
    virtual_buffers: Option<&VirtualBufferMap>,
) -> Result<String, std::io::Error> {
    if let Some(buf) = lookup_virtual_buffer(virtual_buffers, path) {
        return Ok(buf.to_string());
    }
    std::fs::read_to_string(path)
}

/// Returns `true` when `href` selects the minimal browser-UA unstyled preset (`krui://preset/unstyled`).
pub(crate) fn is_unstyled_preset_uri(href: &str) -> bool {
    matches!(
        href.trim().to_ascii_lowercase().as_str(),
        "krui://preset/unstyled" | "krui:unstyled" | "unstyled"
    )
}

pub(crate) fn load_linked_and_companion_css(
    source: &str,
    linked_hrefs: &[(String, usize)],
    krui_path: Option<&Path>,
    virtual_buffers: Option<&VirtualBufferMap>,
    comp_scope: Option<&str>,
    collector: &mut CollectedStylesheets,
) -> Result<(), CompileError> {
    let base_dir = krui_path.and_then(|p| p.parent()).unwrap_or(Path::new("."));
    for (href, link_offset) in linked_hrefs {
        load_single_linked_css(
            source,
            href,
            *link_offset,
            krui_path,
            base_dir,
            virtual_buffers,
            comp_scope,
            collector,
        )?;
    }
    if let Some(kpath) = krui_path {
        load_companion_css(kpath, virtual_buffers, comp_scope, collector)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn load_single_linked_css(
    source: &str,
    href: &str,
    link_offset: usize,
    krui_path: Option<&Path>,
    base_dir: &Path,
    virtual_buffers: Option<&VirtualBufferMap>,
    comp_scope: Option<&str>,
    collector: &mut CollectedStylesheets,
) -> Result<(), CompileError> {
    if href.contains('\0') {
        return Err(CompileError::at_offset(
            source,
            link_offset,
            "Rejected `<link href>` containing NUL byte",
        ));
    }
    if is_unstyled_preset_uri(href) {
        collector.unstyled = true;
        return Ok(());
    }
    if href.trim().to_ascii_lowercase().starts_with("krui:") {
        return Err(CompileError::at_offset(
            source,
            link_offset,
            format!(
                "Built-in theme preset URI `{href}` was removed; use `krui://preset/unstyled` with pure CSS `[data-theme]` rules"
            ),
        ));
    }
    let Some(_kpath) = krui_path else {
        return Err(CompileError::at_offset(
            source,
            link_offset,
            format!(
                "External `<link href=\"{href}\">` requires file-backed `.krui` compilation (`include_krui!`), not inline string compilation"
            ),
        ));
    };
    let candidate = base_dir.join(href);
    let canonical = resolve_path_with_virtual(&candidate, virtual_buffers).map_err(|e| {
        CompileError::at_offset(
            source,
            link_offset,
            format!(
                "Failed to read linked CSS file `{href}` (`{}`): {e}",
                candidate.display()
            ),
        )
    })?;
    if !collector.external_css_paths.contains(&canonical) {
        let file_css = read_text_with_virtual(&canonical, virtual_buffers).map_err(|e| {
            CompileError::at_offset(
                source,
                link_offset,
                format!("Failed to read CSS file `{}`: {e}", canonical.display()),
            )
        })?;
        collector.external_css_paths.push(canonical.clone());
        if let Some(scope) = comp_scope {
            collector.push_component_css(
                &file_css,
                0,
                &file_css,
                Some(&canonical),
                Some(scope),
                virtual_buffers,
            )?;
        } else {
            collector.push_author_css(
                &file_css,
                0,
                &file_css,
                Some(&canonical),
                virtual_buffers,
            )?;
        }
    }
    Ok(())
}

fn load_companion_css(
    kpath: &Path,
    virtual_buffers: Option<&VirtualBufferMap>,
    comp_scope: Option<&str>,
    collector: &mut CollectedStylesheets,
) -> Result<(), CompileError> {
    let companion_css = kpath.with_extension("css");
    if !companion_css.exists() && lookup_virtual_buffer(virtual_buffers, &companion_css).is_none() {
        return Ok(());
    }
    let canonical_companion =
        resolve_path_with_virtual(&companion_css, virtual_buffers).map_err(|e| {
            CompileError::new(
                1,
                1,
                format!(
                    "Failed to resolve companion CSS file `{}`: {e}",
                    companion_css.display()
                ),
            )
        })?;
    if !collector.external_css_paths.contains(&canonical_companion) {
        let file_css =
            read_text_with_virtual(&canonical_companion, virtual_buffers).map_err(|e| {
                CompileError::new(
                    1,
                    1,
                    format!(
                        "Failed to read companion CSS file `{}`: {e}",
                        canonical_companion.display()
                    ),
                )
            })?;
        collector
            .external_css_paths
            .push(canonical_companion.clone());
        if let Some(scope) = comp_scope {
            collector.push_component_css(
                &file_css,
                0,
                &file_css,
                Some(&canonical_companion),
                Some(scope),
                virtual_buffers,
            )?;
        } else {
            collector.push_author_css(
                &file_css,
                0,
                &file_css,
                Some(&canonical_companion),
                virtual_buffers,
            )?;
        }
    }
    Ok(())
}
