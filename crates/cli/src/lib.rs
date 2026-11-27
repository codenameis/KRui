//! # `krui-cli` (`krui` binary)
//!
//! Developer CLI and live preview server (`krui serve`) for KRui.
//!
//! ## Usage
//! - `krui serve` — Watches `.krui`/`.css`/`.rs` files, compiles `.krui` into `VersionedIrBundle` bytes,
//!   and streams `reload(ir)` / `CompileError` updates over local TCP to the running preview app.
//! - `krui serve --example <NAME>` — Launches `cargo run --example <NAME> --features preview`
//!   connected to the live preview TCP server.
//! - `krui serve --frame iphone-15|pixel-8|rpi-800x480` — Wraps the preview viewport in a device
//!   frame bezel with hardware safe-area insets.

#![forbid(unsafe_code)]

mod adb;
mod args;
mod panel_bridge;
pub mod panel_codec;
mod panel_json;
mod process_stage;

pub use adb::{try_setup_adb_reverse, try_setup_adb_reverse_with_bin};
pub use args::{
    default_krui_path_for_target, parse_cli_args, resolve_effective_target, usage_text,
    CliParseError, ServeOptions,
};
pub use panel_bridge::PanelBridgeServer;
pub use process_stage::*;

use krui_compiler::{
    compile_krui_source_with_virtual_buffers, encode_compiled_document, CompileError,
    VirtualBufferMap,
};
use krui_preview::{DeviceFramePreset, PreviewCommand, PreviewResponse, PreviewTcpServer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, SystemTime};

/// Active `krui serve` session managing the TCP server, watched `.krui`/`.css`/`.rs` file timestamps,
/// and compilation into `PreviewCommand::Reload` or `PreviewCommand::CompileError`.
pub struct ServeSession {
    server: PreviewTcpServer,
    watched_krui: PathBuf,
    watched_css: Vec<PathBuf>,
    watched_components: Vec<PathBuf>,
    watched_rust_root: PathBuf,
    timestamps: HashMap<PathBuf, Option<SystemTime>>,
    rust_timestamps: HashMap<PathBuf, Option<SystemTime>>,
    virtual_buffers: VirtualBufferMap,
    last_initial_error: Option<CompileError>,
}

impl ServeSession {
    /// Starts a new `ServeSession` bound to `127.0.0.1:<port>` watching `krui_path`, returning an error
    /// if `krui_path` does not exist or cannot be read at startup.
    pub fn start(
        krui_path: impl AsRef<Path>,
        port: u16,
        frame: Option<DeviceFramePreset>,
    ) -> std::io::Result<Self> {
        Self::start_with_host(krui_path, "127.0.0.1", port, frame)
    }

    /// Starts a new `ServeSession` bound to `<host>:<port>` watching `krui_path`.
    pub fn start_with_host(
        krui_path: impl AsRef<Path>,
        host: &str,
        port: u16,
        frame: Option<DeviceFramePreset>,
    ) -> std::io::Result<Self> {
        let watched_krui = krui_path.as_ref().to_path_buf();
        if !watched_krui.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Watched `.krui` file `{}` does not exist",
                    watched_krui.display()
                ),
            ));
        }
        let bind_host = if host.trim().is_empty() {
            "127.0.0.1"
        } else {
            host.trim()
        };
        let addr = format!("{bind_host}:{port}");
        let mut server = PreviewTcpServer::bind(&addr)?;
        server.set_device_frame(frame);
        let watched_rust_root = infer_rust_watch_root(&watched_krui);
        let mut session = Self {
            server,
            watched_krui,
            watched_css: Vec::new(),
            watched_components: Vec::new(),
            watched_rust_root,
            timestamps: HashMap::new(),
            rust_timestamps: HashMap::new(),
            virtual_buffers: HashMap::new(),
            last_initial_error: None,
        };
        if let Err(err) = session.compile_and_broadcast() {
            session.last_initial_error = Some(err);
        }
        session.refresh_timestamps();
        session.refresh_rust_timestamps();
        Ok(session)
    }

    /// Returns the watched root `.krui` path for this `ServeSession`.
    pub fn watched_krui_path(&self) -> &Path {
        &self.watched_krui
    }

    /// Registers or replaces an unsaved editor buffer for zero-save live compilation.
    pub fn set_virtual_buffer(&mut self, path: impl AsRef<Path>, content: String) {
        let raw = path.as_ref();
        let canonical = std::fs::canonicalize(raw).unwrap_or_else(|_| raw.to_path_buf());
        let needle = normalize_virtual_key(&canonical);
        self.virtual_buffers
            .retain(|k, _| normalize_virtual_key(k) != needle);
        self.virtual_buffers.insert(canonical, content);
    }

    /// Clears any unsaved virtual buffer override for `path` (e.g. after disk save).
    pub fn remove_virtual_buffer(&mut self, path: &Path) {
        let needle = normalize_virtual_key(path);
        self.virtual_buffers
            .retain(|k, _| normalize_virtual_key(k) != needle);
    }

    /// Returns the bound `127.0.0.1:<port>` address of the TCP server.
    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.server.local_addr()
    }

    /// Returns the initial `CompileError` if `watched_krui` had a syntax error at startup.
    pub fn initial_compile_error(&self) -> Option<&CompileError> {
        self.last_initial_error.as_ref()
    }

    /// Compiles `watched_krui` (using unsaved `virtual_buffers` first, then disk) and broadcasts
    /// either `PreviewCommand::Reload(ir_bytes)` or `PreviewCommand::CompileError(err)`.
    pub fn compile_and_broadcast(&mut self) -> Result<usize, CompileError> {
        match compile_krui_source_with_virtual_buffers(
            &self.watched_krui,
            None,
            &self.virtual_buffers,
        ) {
            Ok(doc) => {
                self.watched_css = doc.external_css_paths.clone();
                self.watched_components = doc.external_component_paths.clone();
                let bytes = encode_compiled_document(&doc)
                    .map_err(|e| CompileError::new(1, 1, e.to_string()))?;
                let byte_len = bytes.len();
                self.server.broadcast(&PreviewCommand::Reload(bytes));
                Ok(byte_len)
            }
            Err(err) => {
                self.server
                    .broadcast(&PreviewCommand::CompileError(err.clone()));
                Err(err)
            }
        }
    }

    /// Sends an explicit [`PreviewCommand`] (`Restart`, `InspectTree`, `SetOverlay`, `SetDeviceFrame`)
    /// to all connected preview clients.
    pub fn send_command(&mut self, cmd: &PreviewCommand) {
        self.server.broadcast(cmd);
    }

    /// Polls connected preview clients for any [`PreviewResponse`] messages.
    pub fn poll_responses(&mut self) -> Vec<PreviewResponse> {
        self.server.poll_responses()
    }

    /// Checks watched `.krui` and `.css` files for timestamp changes and triggers `compile_and_broadcast`
    /// when a file is saved or removed.
    pub fn poll_file_changes(&mut self) -> Option<Result<usize, CompileError>> {
        self.server.accept_pending();
        if !self.has_timestamp_changed() {
            return None;
        }
        self.refresh_timestamps();
        let res = self.compile_and_broadcast();
        self.refresh_timestamps();
        Some(res)
    }

    /// Checks watched `.rs` / `Cargo.toml` files under the target directory and returns `true`
    /// when a Rust source or manifest change requires a binary rebuild + hot restart.
    pub fn poll_rust_changes(&mut self) -> bool {
        let current = collect_rust_timestamps(&self.watched_rust_root);
        if current == self.rust_timestamps {
            return false;
        }
        self.rust_timestamps = current;
        true
    }

    fn tracked_paths(&self) -> Vec<PathBuf> {
        let cap = self.watched_css.len() + self.watched_components.len() + 2;
        let mut paths = Vec::with_capacity(cap);
        paths.push(self.watched_krui.clone());
        paths.extend(self.watched_css.iter().cloned());
        paths.extend(self.watched_components.iter().cloned());
        paths.push(self.watched_krui.with_extension("css"));
        paths
    }

    fn has_timestamp_changed(&self) -> bool {
        for p in self.tracked_paths() {
            let current = std::fs::metadata(&p).ok().and_then(|m| m.modified().ok());
            if self.timestamps.get(&p).copied().flatten() != current {
                return true;
            }
        }
        false
    }

    fn refresh_timestamps(&mut self) {
        self.timestamps.clear();
        for p in self.tracked_paths() {
            let current = std::fs::metadata(&p).ok().and_then(|m| m.modified().ok());
            self.timestamps.insert(p, current);
        }
    }

    fn refresh_rust_timestamps(&mut self) {
        self.rust_timestamps = collect_rust_timestamps(&self.watched_rust_root);
    }
}

fn infer_rust_watch_root(krui_path: &Path) -> PathBuf {
    let mut cur = krui_path.parent();
    while let Some(dir) = cur {
        if dir.file_name().and_then(|n| n.to_str()) == Some("views") {
            if let Some(parent) = dir.parent() {
                if parent.as_os_str().is_empty() {
                    return PathBuf::from(".");
                }
                return parent.to_path_buf();
            }
        }
        cur = dir.parent();
    }
    krui_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn should_skip_watch_dir(dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.starts_with('.')
        || name == "node_modules"
        || name == "target"
        || name.starts_with("target-")
}

const MAX_RS_WATCH_DEPTH: usize = 32;

fn collect_rust_timestamps(root: &Path) -> HashMap<PathBuf, Option<SystemTime>> {
    let mut map = HashMap::new();
    let manifest = PathBuf::from("Cargo.toml");
    if manifest.is_file() {
        map.insert(
            manifest.clone(),
            std::fs::metadata(&manifest)
                .ok()
                .and_then(|m| m.modified().ok()),
        );
    }
    visit_rs_files(root, 0, &mut map);
    map
}

fn visit_rs_files(dir: &Path, depth: usize, out: &mut HashMap<PathBuf, Option<SystemTime>>) {
    if depth > MAX_RS_WATCH_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        let p = entry.path();
        if ft.is_dir() {
            if !should_skip_watch_dir(&p) {
                visit_rs_files(&p, depth + 1, out);
            }
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs")
            || p.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml")
        {
            let ts = std::fs::metadata(&p).ok().and_then(|m| m.modified().ok());
            out.insert(p, ts);
        }
    }
}

fn normalize_virtual_key(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let s = resolved.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        s.to_ascii_lowercase()
    } else {
        s
    }
}

/// Runs the interactive `krui serve` loop until the spawned preview window exits or `Ctrl+C`.
pub fn run_serve(mut opts: ServeOptions) -> Result<(), String> {
    let (inferred_example, watch_path) = resolve_effective_target(&opts);
    opts.example = inferred_example;
    if opts.panel {
        return panel_bridge::run_panel_serve(opts, watch_path);
    }
    let mut session = ServeSession::start_with_host(&watch_path, &opts.host, opts.port, opts.frame)
        .map_err(|e| format!("Failed to start `krui serve`: {e}"))?;
    if opts.android {
        let port = session.local_addr().port();
        if let Err(warn) = try_setup_adb_reverse(port) {
            eprintln!("[krui serve] warning: {warn}");
        }
    }
    for &ov in &opts.overlays {
        session.send_command(&PreviewCommand::SetOverlay {
            kind: ov,
            enabled: true,
        });
    }

    let mut child_guard = ChildKillGuard(if opts.no_spawn || opts.android {
        None
    } else {
        Some(
            spawn_preview_app_process(&opts, session.local_addr())
                .map_err(|e| format!("Failed to spawn `cargo run --features preview`: {e}"))?,
        )
    });

    loop {
        if !step_serve_loop(&opts, &watch_path, &mut session, &mut child_guard.0)? {
            break;
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    Ok(())
}

fn step_serve_loop(
    opts: &ServeOptions,
    watch_path: &Path,
    session: &mut ServeSession,
    child: &mut Option<Child>,
) -> Result<bool, String> {
    if let Some(change) = session.poll_file_changes() {
        match change {
            Ok(bytes) => println!(
                "[krui serve] Compiled `{}` ({bytes} bytes IR) -> broadcasted reload(ir)",
                watch_path.display()
            ),
            Err(err) => eprintln!(
                "[krui serve] CompileError ({}:{}): {}",
                err.line, err.column, err.message
            ),
        }
    }
    if session.poll_rust_changes() {
        let _ = rebuild_and_restart_child(opts, session, child);
    }
    let mut needs_version_restart = false;
    for resp in session.poll_responses() {
        if matches!(resp, PreviewResponse::VersionMismatch { .. }) {
            needs_version_restart = true;
        }
    }
    if needs_version_restart {
        let _ = rebuild_and_restart_child(opts, session, child);
    }
    if let Some(c) = child {
        if let Ok(Some(status)) = c.try_wait() {
            if !status.success() {
                return Err(format!(
                    "Preview application exited with non-zero status: {status}"
                ));
            }
            return Ok(false);
        }
    }
    Ok(true)
}
