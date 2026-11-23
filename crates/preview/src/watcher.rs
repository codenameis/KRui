//! Live `.krui` + `.css` file watcher and IR re-mounter for Dev Preview Mode.

use crate::overlay::{decorate_tree_with_engine, PreviewTcpClient};
use crate::protocol::{DeviceFramePreset, PreviewOverlayFlags, PreviewOverlayKind};
use crate::style_patch::try_hot_patch_compiled_styles_in_place;
pub use crate::watcher_fs::resolve_krui_path;
use crate::watcher_fs::{
    collect_timestamps, combine_extra_paths, compile_worker_event, hash_str, read_initial_doc,
    WorkerFileEvent,
};
use crate::{try_reload_compiled_document_with_registry, StructuralPathKey};
use krui_compiler::{compile_krui_source_from_path, CompileError, CompiledDocument};
use krui_core::{PaintScene, RegistryRef, ScopeRegistry, ViewTree};
use krui_text::TextEngine;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

/// Live `.krui` + `.css` file watcher, `krui serve` TCP client, and IR re-mounter for Dev Preview Mode.
pub struct DevFileWatcher {
    canonical_path: PathBuf,
    watched_css_paths: Vec<PathBuf>,
    watched_component_paths: Vec<PathBuf>,
    shared_extra_paths: Arc<Mutex<Vec<PathBuf>>>,
    last_timestamps: Vec<Option<SystemTime>>,
    last_source_hash: u64,
    registry: RegistryRef,
    has_explicit_registry: bool,
    observed_tree_registry: Option<RegistryRef>,
    last_doc: Option<CompiledDocument>,
    reload_count: u64,
    style_patch_count: u64,
    last_compile_error: Option<CompileError>,
    tcp_client: Option<PreviewTcpClient>,
    worker_rx: Option<Receiver<WorkerFileEvent>>,
    worker_stop: Arc<AtomicBool>,
    worker_handle: Option<JoinHandle<()>>,
    overlays: PreviewOverlayFlags,
    device_frame: Option<DeviceFramePreset>,
    overlay_text_engine: Option<TextEngine>,
}

const WATCHER_STACK_BYTES: usize = 16 * 1024 * 1024;

impl DevFileWatcher {
    /// Creates a new `DevFileWatcher` for `local_krui_path` that automatically resolves
    /// `{vm.*}` / `{store.*}` bindings from `tree.preview_registry()`.
    pub fn new(local_krui_path: impl AsRef<Path>) -> Option<Self> {
        let empty = ScopeRegistry::new();
        Self::build(local_krui_path.as_ref(), Rc::new(empty), false)
    }

    /// Creates a new `DevFileWatcher` bound to `registry` so `{vm.*}` / `{store.*}` expressions
    /// evaluate against live application state on every reload.
    pub fn with_registry(local_krui_path: impl AsRef<Path>, registry: RegistryRef) -> Option<Self> {
        Self::build(local_krui_path.as_ref(), registry, true)
    }

    fn build(local_krui_path: &Path, registry: RegistryRef, explicit: bool) -> Option<Self> {
        if !cfg!(debug_assertions) {
            return None;
        }
        let canonical_path = resolve_krui_path(local_krui_path)?;
        let (last_source_hash, parsed_doc, last_compile_error) = read_initial_doc(&canonical_path);
        let watched_css_paths = parsed_doc
            .as_ref()
            .map(|d| d.external_css_paths.clone())
            .unwrap_or_default();
        let watched_component_paths = parsed_doc
            .as_ref()
            .map(|d| d.external_component_paths.clone())
            .unwrap_or_default();
        let extra = combine_extra_paths(&watched_css_paths, &watched_component_paths);
        let last_timestamps = collect_timestamps(&canonical_path, &extra);
        let shared_extra_paths = Arc::new(Mutex::new(extra));
        let mut tcp_client = PreviewTcpClient::from_env();
        if let (Some(client), Some(doc)) = (&mut tcp_client, parsed_doc.clone()) {
            client.set_initial_document(doc);
        }
        Some(Self {
            canonical_path,
            watched_css_paths,
            watched_component_paths,
            shared_extra_paths,
            last_timestamps,
            last_source_hash,
            registry,
            has_explicit_registry: explicit,
            observed_tree_registry: None,
            last_doc: parsed_doc,
            reload_count: 0,
            style_patch_count: 0,
            last_compile_error,
            tcp_client,
            worker_rx: None,
            worker_stop: Arc::new(AtomicBool::new(false)),
            worker_handle: None,
            overlays: PreviewOverlayFlags::default(),
            device_frame: DeviceFramePreset::from_env_or_args(),
            overlay_text_engine: None,
        })
    }

    /// Returns `true` once the diagnostic overlay `TextEngine` has been lazily initialized.
    pub fn has_initialized_overlay_text_engine(&self) -> bool {
        self.overlay_text_engine.is_some()
    }

    /// Spawns a background worker thread that watches local `.krui`/`.css` files and wakes the UI loop.
    pub fn start_background_worker(&mut self, wakeup: impl Fn() + Send + 'static) {
        if self.worker_rx.is_some() {
            return;
        }
        self.worker_stop.store(false, Ordering::Relaxed);
        let stop_flag = Arc::clone(&self.worker_stop);
        let (tx, rx) = mpsc::channel();
        self.worker_rx = Some(rx);
        let krui_path = self.canonical_path.clone();
        let shared_extra = self.shared_extra_paths.clone();
        let mut prev_ts = self.last_timestamps.clone();
        let mut prev_hash = self.last_source_hash;

        let handle = match std::thread::Builder::new()
            .name("krui-preview-watcher".to_string())
            .stack_size(WATCHER_STACK_BYTES)
            .spawn(move || loop {
                for _ in 0..6 {
                    if stop_flag.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                if stop_flag.load(Ordering::Relaxed) {
                    return;
                }
                let extra_list = shared_extra.lock().map(|g| g.clone()).unwrap_or_default();
                let cur_ts = collect_timestamps(&krui_path, &extra_list);
                let cur_src_res = std::fs::read_to_string(&krui_path);
                let cur_hash = cur_src_res.as_ref().map(|s| hash_str(s)).unwrap_or(0);
                if cur_ts == prev_ts && cur_hash == prev_hash {
                    continue;
                }
                prev_ts = cur_ts;
                prev_hash = cur_hash;
                let evt =
                    compile_worker_event(&krui_path, cur_src_res, &shared_extra, &mut prev_ts);
                if tx.send(evt).is_err() {
                    break;
                }
                krui_signals::wake_ui_loop();
                wakeup();
            }) {
            Ok(h) => h,
            Err(_) => return,
        };
        self.worker_handle = Some(handle);
    }

    /// Returns `true` if periodic 150ms polling is needed.
    pub fn needs_periodic_poll(&self) -> bool {
        match &self.tcp_client {
            Some(client) => !client.has_wakeup_listener(),
            None => self.worker_rx.is_none(),
        }
    }

    /// Starts event-driven TCP wakeup reader/writer threads on the connected `krui serve` socket.
    pub fn start_tcp_wakeup_listener(&mut self, wakeup: impl Fn() + Send + 'static) {
        if let Some(client) = &mut self.tcp_client {
            client.start_wakeup_listener(wakeup);
        }
    }

    /// Connects this `DevFileWatcher` to a `krui serve` TCP server at `addr`.
    pub fn connect_tcp(&mut self, addr: &str) -> bool {
        if !cfg!(debug_assertions) {
            return false;
        }
        if let Some(mut client) = PreviewTcpClient::connect(addr) {
            if let Ok(src) = std::fs::read_to_string(&self.canonical_path) {
                if let Ok(doc) = compile_krui_source_from_path(&self.canonical_path, &src) {
                    client.set_initial_document(doc);
                }
            }
            self.tcp_client = Some(client);
            true
        } else {
            false
        }
    }

    /// Returns the active [`DeviceFramePreset`], if any.
    pub fn device_frame(&self) -> Option<DeviceFramePreset> {
        self.tcp_client
            .as_ref()
            .and_then(|c| c.device_frame())
            .or(self.device_frame)
    }

    /// Sets the active [`DeviceFramePreset`].
    pub fn set_device_frame(&mut self, frame: Option<DeviceFramePreset>) {
        self.device_frame = frame;
    }

    /// Sets a debug overlay flag (`relayout`, `fps`, `hit`).
    pub fn set_overlay(&mut self, kind: PreviewOverlayKind, enabled: bool) {
        self.overlays.set(kind, enabled);
    }

    /// Returns the effective [`PreviewOverlayFlags`].
    pub fn overlays(&self) -> PreviewOverlayFlags {
        let tcp = self
            .tcp_client
            .as_ref()
            .map(|c| c.overlays())
            .unwrap_or_default();
        PreviewOverlayFlags {
            relayout: self.overlays.relayout || tcp.relayout,
            fps: self.overlays.fps || tcp.fps,
            hit: self.overlays.hit || tcp.hit,
        }
    }

    /// Decorates `scene` with any active `CompileError` banner, debug overlay, and device bezel.
    pub fn decorate_paint_scene(
        &mut self,
        tree: &ViewTree,
        scene: &mut PaintScene,
        last_frame_us: u128,
    ) {
        let overlays = self.overlays();
        let frame = self.device_frame();
        let err_clone = self.last_compile_error.clone();
        decorate_tree_with_engine(
            tree,
            scene,
            &mut self.overlay_text_engine,
            err_clone.as_ref(),
            overlays,
            frame,
            last_frame_us,
        );
    }

    /// Replaces the active [`RegistryRef`] used when re-mounting the watched `.krui` document.
    pub fn set_registry(&mut self, registry: RegistryRef) {
        self.registry = registry;
        self.has_explicit_registry = true;
        self.observed_tree_registry = None;
    }

    /// Returns the canonical path of the watched `.krui` file.
    pub fn watched_path(&self) -> &Path {
        &self.canonical_path
    }

    /// Returns the canonical paths of any watched external/companion `.css` files.
    pub fn watched_css_paths(&self) -> &[PathBuf] {
        &self.watched_css_paths
    }

    /// Returns the number of hot-reloads performed so far (including CSS-only fast patches).
    pub fn reload_count(&self) -> u64 {
        self.reload_count
    }

    /// Returns the number of reloads that took the in-place CSS-only fast path without unmounting DOM nodes.
    pub fn style_patch_count(&self) -> u64 {
        self.style_patch_count
    }

    /// Returns the most recent `CompileError` if a hot-reload failed to compile or read.
    pub fn last_compile_error(&self) -> Option<&CompileError> {
        self.last_compile_error.as_ref()
    }

    fn resolve_effective_registry(&mut self, tree: &ViewTree) -> RegistryRef {
        let Some(live_reg) = tree.preview_registry() else {
            return self.registry.clone();
        };
        if !self.has_explicit_registry {
            return live_reg;
        }
        match &self.observed_tree_registry {
            Some(prev) if !Rc::ptr_eq(prev, &live_reg) => {
                self.observed_tree_registry = Some(live_reg.clone());
                self.registry = live_reg.clone();
                self.has_explicit_registry = false;
                live_reg
            }
            None => {
                self.observed_tree_registry = Some(live_reg);
                self.registry.clone()
            }
            Some(_) => self.registry.clone(),
        }
    }

    fn try_apply_compiled_doc(
        &mut self,
        tree: &mut ViewTree,
        doc: &CompiledDocument,
    ) -> Option<usize> {
        let reg = self.resolve_effective_registry(tree);
        if let Some(prev_doc) = &self.last_doc {
            match try_hot_patch_compiled_styles_in_place(tree, prev_doc, doc, &reg) {
                Ok(Some(count)) => {
                    self.last_compile_error = None;
                    self.last_doc = Some(doc.clone());
                    self.reload_count += 1;
                    self.style_patch_count += 1;
                    return Some(count);
                }
                Ok(None) => {}
                Err(err) => {
                    self.last_compile_error = Some(err);
                    return None;
                }
            }
        }
        match try_reload_compiled_document_with_registry(tree, doc, reg) {
            Ok(count) => {
                self.last_compile_error = None;
                self.last_doc = Some(doc.clone());
                self.observed_tree_registry = tree.preview_registry();
                self.reload_count += 1;
                Some(count)
            }
            Err(err) => {
                self.last_compile_error = Some(err);
                None
            }
        }
    }

    /// Reloads `tree` directly from a pre-compiled `CompiledDocument` (e.g. received over `krui serve` TCP).
    pub fn apply_compiled_document(
        &mut self,
        tree: &mut ViewTree,
        doc: &CompiledDocument,
    ) -> usize {
        self.try_apply_compiled_doc(tree, doc)
            .unwrap_or_else(|| tree.node_count())
    }

    /// Forces an immediate re-compilation and IR re-mount (or CSS fast-patch) from disk.
    pub fn force_reload(&mut self, tree: &mut ViewTree) -> Option<usize> {
        if !cfg!(debug_assertions) {
            return None;
        }
        let source = match std::fs::read_to_string(&self.canonical_path) {
            Ok(s) => s,
            Err(io_err) => {
                self.last_compile_error = Some(
                    CompileError::new(
                        1,
                        1,
                        format!(
                            "Failed to read `{}`: {io_err}",
                            self.canonical_path.display()
                        ),
                    )
                    .with_file(self.canonical_path.clone()),
                );
                return None;
            }
        };
        self.last_source_hash = hash_str(&source);
        let doc = match compile_krui_source_from_path(&self.canonical_path, &source) {
            Ok(d) => d,
            Err(err) => {
                self.last_compile_error = Some(err);
                return None;
            }
        };
        self.watched_css_paths = doc.external_css_paths.clone();
        self.watched_component_paths = doc.external_component_paths.clone();
        let extra = combine_extra_paths(&self.watched_css_paths, &self.watched_component_paths);
        if let Ok(mut g) = self.shared_extra_paths.lock() {
            *g = extra.clone();
        }
        self.last_timestamps = collect_timestamps(&self.canonical_path, &extra);
        self.try_apply_compiled_doc(tree, &doc)
    }

    /// Returns `true` when this watcher is connected to a `krui serve` TCP server.
    pub fn has_tcp_client(&self) -> bool {
        self.tcp_client.is_some()
    }

    /// Returns `true` if the connected `krui serve` TCP socket closed or errored.
    pub fn is_tcp_disconnected(&self) -> bool {
        self.tcp_client
            .as_ref()
            .is_some_and(PreviewTcpClient::is_disconnected)
    }

    /// Drains any pending interactive panel input events queued by `krui serve --panel`.
    pub fn drain_panel_inputs(&mut self) -> Vec<crate::panel_wire::PanelInputEvent> {
        self.tcp_client
            .as_mut()
            .map(PreviewTcpClient::drain_panel_inputs)
            .unwrap_or_default()
    }

    /// Streams a rendered framebuffer + state packet back to `krui serve --panel`.
    pub fn send_panel_frame(&mut self, state: crate::panel_wire::PanelFrameState) -> bool {
        self.tcp_client.as_mut().is_some_and(|c| {
            c.send_response(&crate::PreviewResponse::PanelFrame(state))
                .is_ok()
        })
    }

    /// Polls any connected `krui serve` TCP stream, background worker events, or local `.krui`/`.css` files.
    pub fn poll_and_patch(&mut self, tree: &mut ViewTree) -> Option<usize> {
        if !cfg!(debug_assertions) {
            return None;
        }
        let mut changed = false;
        if let Some(client) = &mut self.tcp_client {
            if client.poll(tree, &mut self.last_compile_error, &mut self.reload_count) {
                changed = true;
            }
            return changed.then(|| tree.node_count());
        }
        if self.worker_rx.is_some() {
            changed |= self.drain_worker_events(tree);
            return changed.then(|| tree.node_count());
        }
        let extra = combine_extra_paths(&self.watched_css_paths, &self.watched_component_paths);
        let current_timestamps = collect_timestamps(&self.canonical_path, &extra);
        let current_hash = std::fs::read_to_string(&self.canonical_path)
            .ok()
            .map(|s| hash_str(&s))
            .unwrap_or(0);
        if current_timestamps == self.last_timestamps && current_hash == self.last_source_hash {
            return changed.then(|| tree.node_count());
        }
        self.last_timestamps = current_timestamps;
        let prev_err = self.last_compile_error.clone();
        let res = self.force_reload(tree);
        if res.is_none() && self.last_compile_error != prev_err {
            return Some(tree.node_count());
        }
        res.or_else(|| changed.then(|| tree.node_count()))
    }

    fn drain_worker_events(&mut self, tree: &mut ViewTree) -> bool {
        let mut events = Vec::new();
        if let Some(rx) = &self.worker_rx {
            while let Ok(evt) = rx.try_recv() {
                events.push(evt);
            }
        }
        let mut changed = false;
        for evt in events {
            match evt {
                WorkerFileEvent::Compiled(doc) => {
                    self.watched_css_paths = doc.external_css_paths.clone();
                    self.watched_component_paths = doc.external_component_paths.clone();
                    let _ = self.apply_compiled_document(tree, &doc);
                    changed = true;
                }
                WorkerFileEvent::Error(err) => {
                    self.last_compile_error = Some(err);
                    changed = true;
                }
            }
        }
        changed
    }
}

impl Drop for DevFileWatcher {
    fn drop(&mut self) {
        self.worker_stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

/// Formats a structural path key `(parent_path, tag, element_id, for_key, sibling_index)`.
#[inline]
pub fn format_structural_key(key: &StructuralPathKey<'_>) -> String {
    key.to_path_string()
}
