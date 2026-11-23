//! Filesystem timestamp, hash, and path resolution helpers for [`DevFileWatcher`](crate::DevFileWatcher).

use krui_compiler::{compile_krui_source_from_path, CompileError, CompiledDocument};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

pub(crate) enum WorkerFileEvent {
    Compiled(Box<CompiledDocument>),
    Error(CompileError),
}

pub(crate) fn compile_worker_event(
    krui_path: &Path,
    cur_src_res: std::io::Result<String>,
    shared_extra: &Arc<Mutex<Vec<PathBuf>>>,
    prev_ts: &mut Vec<Option<SystemTime>>,
) -> WorkerFileEvent {
    match cur_src_res {
        Ok(src) => match compile_krui_source_from_path(krui_path, &src) {
            Ok(doc) => {
                let next_extra =
                    combine_extra_paths(&doc.external_css_paths, &doc.external_component_paths);
                if let Ok(mut g) = shared_extra.lock() {
                    *g = next_extra.clone();
                }
                *prev_ts = collect_timestamps(krui_path, &next_extra);
                WorkerFileEvent::Compiled(Box::new(doc))
            }
            Err(err) => WorkerFileEvent::Error(err),
        },
        Err(io_err) => WorkerFileEvent::Error(
            CompileError::new(
                1,
                1,
                format!("Failed to read `{}`: {io_err}", krui_path.display()),
            )
            .with_file(krui_path.to_path_buf()),
        ),
    }
}

pub(crate) fn read_initial_doc(
    path: &Path,
) -> (u64, Option<CompiledDocument>, Option<CompileError>) {
    match std::fs::read_to_string(path) {
        Ok(src) => {
            let hash = hash_str(&src);
            match compile_krui_source_from_path(path, &src) {
                Ok(doc) => (hash, Some(doc), None),
                Err(err) => (hash, None, Some(err)),
            }
        }
        Err(io_err) => {
            let err = CompileError::new(
                1,
                1,
                format!("Failed to read `{}`: {io_err}", path.display()),
            )
            .with_file(path.to_path_buf());
            (0, None, Some(err))
        }
    }
}

pub(crate) fn combine_extra_paths(css: &[PathBuf], components: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(css.len() + components.len());
    out.extend_from_slice(css);
    out.extend_from_slice(components);
    out
}

pub(crate) fn hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

pub(crate) fn collect_timestamps(
    krui_path: &Path,
    watched_css_paths: &[PathBuf],
) -> Vec<Option<SystemTime>> {
    let mut out = Vec::with_capacity(watched_css_paths.len() + 2);
    out.push(
        std::fs::metadata(krui_path)
            .ok()
            .and_then(|m| m.modified().ok()),
    );
    for css_p in watched_css_paths {
        out.push(
            std::fs::metadata(css_p)
                .ok()
                .and_then(|m| m.modified().ok()),
        );
    }
    let companion = krui_path.with_extension("css");
    out.push(
        std::fs::metadata(companion)
            .ok()
            .and_then(|m| m.modified().ok()),
    );
    out
}

/// Resolves a relative `.krui` path against the current working directory, executable ancestors,
/// or workspace root.
pub fn resolve_krui_path(rel: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::fs::canonicalize(rel) {
        return Some(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut ancestor = exe.parent();
        while let Some(dir) = ancestor {
            let candidate = dir.join(rel);
            if let Ok(p) = std::fs::canonicalize(&candidate) {
                return Some(p);
            }
            ancestor = dir.parent();
        }
    }
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Ok(p) = std::fs::canonicalize(manifest_dir.join(rel)) {
        return Some(p);
    }
    if let Some(ws_root) = manifest_dir.parent().and_then(|p| p.parent()) {
        if let Ok(p) = std::fs::canonicalize(ws_root.join(rel)) {
            return Some(p);
        }
    }
    None
}
