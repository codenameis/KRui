//! Headless offscreen mobile home-screen widget snapshot & action host (`TinySkiaRenderer`).

use krui_core::{
    collect_widget_hotspots, GpuResourceLimits, NodeId, PaintScene, ViewTree, WidgetActionBinding,
    WidgetRenderConfig, WidgetSnapshot,
};
use krui_renderer::{RendererBackend, TinySkiaRenderer};
use krui_signals::{poll_async_tasks, ReactiveSession, Signal};
use krui_style::ColorScheme;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

/// Default widget increment action ID emitted by the sample counter widget.
pub const WIDGET_ACTION_INCREMENT: u32 = 1;
/// Default widget reset action ID emitted by the sample counter widget.
pub const WIDGET_ACTION_RESET: u32 = 2;

const DEFAULT_COUNTER_WIDGET_CSS: &str = r#"
.widget-card {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: 12px;
    row-gap: 8px;
    background-color: #f8fafc;
    border: 1px solid #cbd5e1;
    border-radius: 16px;
}
.widget-title {
    font-size: 13px;
    color: #475569;
}
.widget-count {
    font-size: 24px;
    color: #0f172a;
}
.widget-row {
    display: flex;
    flex-direction: row;
    column-gap: 8px;
}
.widget-btn {
    width: 96px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 6px 12px;
    border-radius: 10px;
    font-size: 14px;
}
.widget-btn-primary {
    background-color: #0284c7;
    color: #ffffff;
}
.widget-btn-secondary {
    background-color: #e2e8f0;
    color: #0f172a;
}
@media (prefers-color-scheme: dark) {
    .widget-card {
        background-color: #111827;
        border-color: #334155;
    }
    .widget-title {
        color: #cbd5e1;
    }
    .widget-count {
        color: #f8fafc;
    }
    .widget-btn-primary {
        background-color: #38bdf8;
        color: #0f172a;
    }
    .widget-btn-secondary {
        background-color: #1e293b;
        color: #e2e8f0;
    }
}
"#;

static NEXT_WIDGET_ID: AtomicI64 = AtomicI64::new(1);

thread_local! {
    static WIDGETS: RefCell<HashMap<i64, AndroidWidgetHost>> = RefCell::new(HashMap::new());
}

/// Callback invoked when a widget action ID is triggered from the OS home-screen launcher.
pub type WidgetActionHandler = Box<dyn FnMut(u32) -> bool>;

/// Offscreen software-rendered host for a mobile home-screen widget backed by `ViewTree` + `TinySkiaRenderer`.
pub struct AndroidWidgetHost {
    /// Isolated reactive runtime owning all widget signals and bindings.
    pub reactive_session: ReactiveSession,
    /// Platform-neutral UI tree laid out at [`WidgetRenderConfig::logical_size`].
    pub tree: ViewTree,
    renderer: TinySkiaRenderer,
    config: WidgetRenderConfig,
    bindings: Vec<WidgetActionBinding>,
    action_handler: Option<WidgetActionHandler>,
    last_snapshot: Option<WidgetSnapshot>,
    revision: u64,
    argb_scratch: Vec<i32>,
    hotspot_scratch: Vec<i32>,
}

impl AndroidWidgetHost {
    /// Creates an empty [`AndroidWidgetHost`] configured for `(width_px, height_px)` and `scale_factor`.
    pub fn new(width_px: u32, height_px: u32, scale_factor: f32, dark_mode: bool) -> Self {
        let scheme = scheme_from_dark_flag(dark_mode);
        let config =
            WidgetRenderConfig::from_physical_px(width_px, height_px, scale_factor, scheme);
        let (lw, lh) = config.logical_size();
        let reactive_session = ReactiveSession::new();
        let (tree, renderer) = reactive_session.enter(|| {
            let mut tree = ViewTree::new();
            tree.set_scale_factor(config.sanitized_scale());
            tree.set_system_color_scheme(scheme);
            let renderer = TinySkiaRenderer::new(lw, lh, GpuResourceLimits::default());
            (tree, renderer)
        });
        Self {
            reactive_session,
            tree,
            renderer,
            config,
            bindings: Vec::new(),
            action_handler: None,
            last_snapshot: None,
            revision: 0,
            argb_scratch: Vec::new(),
            hotspot_scratch: Vec::new(),
        }
    }

    /// Creates an [`AndroidWidgetHost`] pre-populated with KRui's reactive counter sample widget.
    pub fn new_default_widget(
        width_px: u32,
        height_px: u32,
        scale_factor: f32,
        dark_mode: bool,
    ) -> Self {
        let mut host = Self::new(width_px, height_px, scale_factor, dark_mode);
        let count_sig = host.reactive_session.enter(|| Signal::new(0_i32));
        let bindings = host
            .reactive_session
            .enter(|| mount_default_counter_widget(&mut host.tree, count_sig));
        host.bindings = bindings;
        host.set_action_handler(move |action_id| match action_id {
            WIDGET_ACTION_INCREMENT => {
                count_sig.update(|c| *c = c.saturating_add(1));
                true
            }
            WIDGET_ACTION_RESET => {
                count_sig.set(0);
                true
            }
            _ => false,
        });
        host
    }

    /// Replaces the widget tree using `mount` and registers exported hotspot action bindings.
    pub fn mount_widget(&mut self, mount: impl FnOnce(&mut ViewTree) -> Vec<WidgetActionBinding>) {
        let tree = &mut self.tree;
        self.bindings = self.reactive_session.enter(|| mount(tree));
    }

    /// Registers a handler invoked whenever [`Self::dispatch_action`] receives an `action_id`.
    pub fn set_action_handler(&mut self, handler: impl FnMut(u32) -> bool + 'static) {
        self.action_handler = Some(Box::new(handler));
    }

    /// Resizes the widget surface and updates OS light/dark theme scheme via pure-CSS `@media`.
    pub fn resize(
        &mut self,
        width_px: u32,
        height_px: u32,
        scale_factor: f32,
        dark_mode: bool,
    ) -> bool {
        let scheme = scheme_from_dark_flag(dark_mode);
        self.config =
            WidgetRenderConfig::from_physical_px(width_px, height_px, scale_factor, scheme);
        let (lw, lh) = self.config.logical_size();
        let scale = self.config.sanitized_scale();
        let tree = &mut self.tree;
        self.reactive_session.enter(|| {
            tree.set_scale_factor(scale);
            tree.set_system_color_scheme(scheme);
            let _ = tree.sync_viewport_and_media(lw as f32, lh as f32);
        });
        true
    }

    /// Builds layout + paint scene and renders a fresh offscreen [`WidgetSnapshot`].
    pub fn render_snapshot(&mut self) -> Option<&WidgetSnapshot> {
        let (lw, lh) = self.config.logical_size();
        let scale = self.config.sanitized_scale();
        let tree = &mut self.tree;
        let scene: PaintScene = self.reactive_session.enter(|| {
            let _ = poll_async_tasks();
            let _ = tree.sync_viewport_and_media(lw as f32, lh as f32);
            tree.build_paint_scene(lw, lh).0
        });
        let _ = self.renderer.render_scene(&scene);
        let (pw, ph) = self.renderer.framebuffer_dimensions()?;
        let rgba = self.renderer.framebuffer_rgba()?;
        let expected_bytes = (pw as usize).checked_mul(ph as usize)?.checked_mul(4)?;
        if rgba.len() < expected_bytes {
            return None;
        }
        let hotspots = collect_widget_hotspots(&self.tree, &self.bindings);
        self.revision = self.revision.saturating_add(1);
        self.last_snapshot = Some(WidgetSnapshot {
            width_px: pw,
            height_px: ph,
            logical_width: lw,
            logical_height: lh,
            scale_factor: scale,
            revision: self.revision,
            rgba8: rgba[..expected_bytes].to_vec(),
            hotspots,
        });
        self.last_snapshot.as_ref()
    }

    /// Dispatches a home-screen tap `action_id` and re-renders the widget snapshot if handled.
    pub fn dispatch_action(&mut self, action_id: u32) -> bool {
        let handled = if let Some(handler) = &mut self.action_handler {
            self.reactive_session.enter(|| handler(action_id))
        } else if let Some(rect) = self
            .bindings
            .iter()
            .find(|b| b.action_id == action_id)
            .and_then(|b| self.tree.node_layout(b.node_id))
        {
            let tree = &mut self.tree;
            self.reactive_session.enter(|| {
                tree.dispatch_click(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5)
            })
        } else {
            false
        };
        if handled {
            return self.render_snapshot().is_some();
        }
        false
    }

    /// Returns the monotonic snapshot revision number (`0` before the first render).
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the latest rendered snapshot if available.
    pub const fn last_snapshot(&self) -> Option<&WidgetSnapshot> {
        self.last_snapshot.as_ref()
    }

    /// Returns packed physical snapshot size `(width_px << 32) | height_px`, or `0` before render.
    pub fn packed_snapshot_size(&self) -> i64 {
        let Some(snap) = &self.last_snapshot else {
            return 0;
        };
        ((i64::from(snap.width_px)) << 32) | i64::from(snap.height_px)
    }

    /// Returns the number of clickable [`krui_core::WidgetHotspot`] targets in the latest snapshot.
    pub fn hotspot_count(&self) -> usize {
        self.last_snapshot.as_ref().map_or(0, |s| s.hotspots.len())
    }

    /// Converts the latest snapshot pixels into `ARGB_8888` `i32` values and invokes `f`.
    pub fn with_snapshot_argb<R>(&mut self, f: impl FnOnce(u32, u32, &[i32]) -> R) -> Option<R> {
        let snap = self.last_snapshot.as_ref()?;
        snap.to_argb8888(&mut self.argb_scratch);
        Some(f(snap.width_px, snap.height_px, &self.argb_scratch))
    }

    /// Packs hotspot tuples `[action_id, x_px, y_px, w_px, h_px, ...]` into `i32`s and invokes `f`.
    pub fn with_packed_hotspots<R>(&mut self, f: impl FnOnce(&[i32]) -> R) -> Option<R> {
        let snap = self.last_snapshot.as_ref()?;
        self.hotspot_scratch.clear();
        self.hotspot_scratch.reserve(snap.hotspots.len() * 5);
        for spot in &snap.hotspots {
            self.hotspot_scratch
                .extend_from_slice(&spot.to_physical_i32_tuple(snap.scale_factor));
        }
        Some(f(&self.hotspot_scratch))
    }
}

const fn scheme_from_dark_flag(dark_mode: bool) -> ColorScheme {
    if dark_mode {
        ColorScheme::Dark
    } else {
        ColorScheme::Light
    }
}

fn mount_default_counter_widget(
    tree: &mut ViewTree,
    count_sig: Signal<i32>,
) -> Vec<WidgetActionBinding> {
    tree.set_unstyled(true);
    tree.set_document_stylesheets(&[DEFAULT_COUNTER_WIDGET_CSS]);

    let root = tree.create_element_node(
        None,
        "div",
        Some("widget-root".to_string()),
        None,
        false,
        false,
        false,
        false,
    );
    tree.set_element_classes(root, &["widget-card"]);

    let _title = mount_widget_text(tree, root, "p", &["widget-title"], "KRui Counter Widget");
    let readout = tree.create_element_node(
        Some(root),
        "h2",
        Some("widget-count".to_string()),
        None,
        false,
        false,
        false,
        false,
    );
    tree.set_element_classes(readout, &["widget-count"]);
    tree.bind_dynamic_text(readout, move || format!("Count: {}", count_sig.get()));

    let row = tree.create_element_node(Some(root), "div", None, None, false, false, false, false);
    tree.set_element_classes(row, &["widget-row"]);

    let inc_btn = mount_widget_button(tree, row, &["widget-btn", "widget-btn-primary"], "+1 Count");
    let reset_btn =
        mount_widget_button(tree, row, &["widget-btn", "widget-btn-secondary"], "Reset");
    vec![
        WidgetActionBinding {
            node_id: inc_btn,
            action_id: WIDGET_ACTION_INCREMENT,
        },
        WidgetActionBinding {
            node_id: reset_btn,
            action_id: WIDGET_ACTION_RESET,
        },
    ]
}

fn mount_widget_text(
    tree: &mut ViewTree,
    parent: NodeId,
    tag: &'static str,
    classes: &[&str],
    label: &str,
) -> NodeId {
    let nid = tree.create_element_node(Some(parent), tag, None, None, false, false, false, false);
    tree.set_element_classes(nid, classes);
    tree.set_static_text(nid, label);
    nid
}

fn mount_widget_button(
    tree: &mut ViewTree,
    parent: NodeId,
    classes: &[&str],
    label: &str,
) -> NodeId {
    let nid = tree.create_element_node(
        Some(parent),
        "button",
        None,
        None,
        true,
        false,
        false,
        false,
    );
    tree.set_element_classes(nid, classes);
    tree.set_static_text(nid, label);
    nid
}

/// Registers an [`AndroidWidgetHost`] in the thread-local handle table and returns its ID.
pub fn register_widget_host(host: AndroidWidgetHost) -> i64 {
    let id = NEXT_WIDGET_ID.fetch_add(1, Ordering::Relaxed);
    WIDGETS.with(|map| {
        if let Ok(mut guard) = map.try_borrow_mut() {
            guard.insert(id, host);
        }
    });
    id
}

/// Creates and registers a default counter widget host, returning its positive opaque ID.
pub fn create_registered_widget_host(
    width_px: i32,
    height_px: i32,
    scale_factor: f32,
    dark_mode: bool,
) -> i64 {
    let w = width_px.max(1) as u32;
    let h = height_px.max(1) as u32;
    register_widget_host(AndroidWidgetHost::new_default_widget(
        w,
        h,
        scale_factor,
        dark_mode,
    ))
}

/// Removes and drops a registered widget host, returning `true` if the ID existed.
pub fn destroy_registered_widget_host(widget_id: i64) -> bool {
    if widget_id <= 0 {
        return false;
    }
    let removed = WIDGETS.with(|map| {
        map.try_borrow_mut()
            .ok()
            .and_then(|mut guard| guard.remove(&widget_id))
    });
    let Some(host) = removed else {
        return false;
    };
    let reactive = host.reactive_session.clone();
    drop(host);
    reactive.reset();
    true
}

/// Safely borrows a registered [`AndroidWidgetHost`] by ID, returning `None` on stale/re-entrant access.
pub fn with_registered_widget_host<R>(
    widget_id: i64,
    f: impl FnOnce(&mut AndroidWidgetHost) -> R,
) -> Option<R> {
    if widget_id <= 0 {
        return None;
    }
    WIDGETS.with(|map| {
        let mut guard = map.try_borrow_mut().ok()?;
        let host = guard.get_mut(&widget_id)?;
        Some(f(host))
    })
}
