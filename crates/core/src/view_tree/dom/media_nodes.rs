//! Retained media (`<img>`, `<svg>`, `ExternalTexture`) and `<a href>` link state for `ViewTree`.

use crate::media::{DecodedImage, ExternalTextureId, ImageSource, SvgVectorTemplate};
use crate::scene::CanvasCommand;
use crate::view_tree::form_submit::trigger_button_form_action;
use crate::view_tree::grapheme::sync_node_text_context;
use crate::view_tree::{A11yId, NodeId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;
use krui_signals::{batch, Effect, Scope};
#[cfg(not(target_arch = "wasm32"))]
use krui_signals::{spawn_tokio_task, tokio_handle};
use krui_style::{sanitize_untrusted_text, Color};
use std::path::{Path, PathBuf};
use std::rc::Rc;
#[cfg(not(target_arch = "wasm32"))]
use std::rc::Weak;
use std::sync::Arc;

/// Maximum encoded image file size (`64 MiB`) read from disk.
const MAX_IMAGE_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Per-node retained raster image, compiled `<svg>` template, and hyperlink metadata.
#[derive(Default)]
pub(crate) struct MediaData {
    pub(crate) image_source: Option<ImageSource>,
    pub(crate) host_mounted_image: bool,
    pub(crate) svg_template: Option<SvgVectorTemplate>,
    pub(crate) svg_cached: Option<(u32, u32, Color, Arc<[CanvasCommand]>)>,
    pub(crate) link_href: Option<String>,
    pub(crate) loaded_src_key: Option<String>,
    pub(crate) load_request_seq: u64,
    pub(crate) load_scope: Option<Scope>,
}

impl Drop for MediaData {
    fn drop(&mut self) {
        if let Some(scope) = self.load_scope.take() {
            scope.dispose();
        }
    }
}

fn read_and_decode_image_file(path: &Path) -> Result<DecodedImage, String> {
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("Failed to stat image `{}`: {e}", path.display()))?;
    if meta.len() > MAX_IMAGE_FILE_BYTES {
        return Err(format!(
            "Image file `{}` ({} bytes) exceeds {} MiB limit",
            path.display(),
            meta.len(),
            MAX_IMAGE_FILE_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read image `{}`: {e}", path.display()))?;
    DecodedImage::from_encoded_bytes(&bytes)
}

fn promote_node_to_focusable_link(inner: &mut ViewTreeInner, node_id: NodeId) -> bool {
    let Some(node) = inner.node_mut(node_id) else {
        return false;
    };
    if matches!(
        node.role,
        SemanticRole::GenericContainer | SemanticRole::Label
    ) {
        node.role = SemanticRole::Link;
    }
    if node.role.is_focusable() {
        inner
            .a11y_engine
            .register_focusable(A11yId::from(node_id).0);
    }
    true
}

impl ViewTreeInner {
    #[inline]
    pub(crate) fn media(&self, id: NodeId) -> Option<&MediaData> {
        self.sparse.media.get(&id)
    }

    #[inline]
    pub(crate) fn ensure_media(&mut self, id: NodeId) -> &mut MediaData {
        self.sparse.media.entry(id).or_default()
    }
}

impl ViewTree {
    /// Attaches a pre-decoded RGBA8 image (`<img>`) to `node_id` and marks layout + paint dirty.
    pub fn set_node_decoded_image(&mut self, node_id: NodeId, image: DecodedImage) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_none() {
            return;
        }
        let media = inner.ensure_media(node_id);
        media.image_source = Some(ImageSource::Decoded(image));
        media.host_mounted_image = true;
        if let Some(n) = inner.node_mut(node_id) {
            n.set_text_dirty(true);
        }
        sync_node_text_context(&mut inner, node_id);
        inner.paint_dirty = true;
    }

    /// Decodes encoded PNG/JPEG/WebP bytes and binds the resulting image to `node_id`.
    pub fn set_node_image_bytes(
        &mut self,
        node_id: NodeId,
        encoded_bytes: &[u8],
    ) -> Result<(), String> {
        let decoded = DecodedImage::from_encoded_bytes(encoded_bytes)?;
        self.set_node_decoded_image(node_id, decoded);
        Ok(())
    }

    /// Loads and decodes a raster image file from `path` for an `<img>` element `node_id`.
    pub fn set_node_image_file(
        &mut self,
        node_id: NodeId,
        path: impl AsRef<Path>,
    ) -> Result<(), String> {
        match read_and_decode_image_file(path.as_ref()) {
            Ok(decoded) => {
                self.set_node_decoded_image(node_id, decoded);
                if let Some(m) = self.inner.borrow_mut().sparse.media.get_mut(&node_id) {
                    m.host_mounted_image = false;
                }
                Ok(())
            }
            Err(err) => {
                self.clear_node_image_source(node_id);
                Err(err)
            }
        }
    }

    /// Binds a reactive image file path (`<img src="{expr}">`) directly to `node_id`.
    pub fn bind_dynamic_image_src(
        &mut self,
        node_id: NodeId,
        getter: impl Fn() -> String + 'static,
    ) {
        self.bind_dynamic_image_src_rc(node_id, None, Rc::new(getter));
    }

    /// Binds a reactive image file path with an explicit base directory for relative assets.
    pub fn bind_dynamic_image_src_in_dir(
        &mut self,
        node_id: NodeId,
        base_dir: PathBuf,
        getter: impl Fn() -> String + 'static,
    ) {
        self.bind_dynamic_image_src_rc(node_id, Some(base_dir), Rc::new(getter));
    }

    fn bind_dynamic_image_src_rc(
        &mut self,
        node_id: NodeId,
        base_dir: Option<PathBuf>,
        getter: Rc<dyn Fn() -> String>,
    ) {
        if self.inner.borrow().node(node_id).is_none() {
            return;
        }
        let tree_weak = Rc::downgrade(&self.inner);
        let effect = Effect::new(move || {
            let raw_src = getter();
            let Some(rc) = tree_weak.upgrade() else {
                return;
            };
            let trimmed = raw_src.trim().to_string();
            let resolved = resolve_asset_path(&trimmed, base_dir.as_deref());
            #[cfg(not(target_arch = "wasm32"))]
            schedule_dynamic_image_load(&rc, &tree_weak, node_id, trimmed, resolved);
            #[cfg(target_arch = "wasm32")]
            schedule_dynamic_image_load(&rc, node_id, trimmed, resolved);
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }

    /// Mounts a host-owned external GPU texture slot (`ExternalTexture`) onto `node_id`.
    pub fn mount_external_texture(
        &mut self,
        node_id: NodeId,
        id: ExternalTextureId,
        width: u32,
        height: u32,
    ) {
        let next = ImageSource::External { id, width, height };
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_none() {
            return;
        }
        if inner
            .media(node_id)
            .is_some_and(|m| m.host_mounted_image && m.image_source.as_ref() == Some(&next))
        {
            return;
        }
        let media = inner.ensure_media(node_id);
        media.image_source = Some(next);
        media.host_mounted_image = true;
        inner.paint_dirty = true;
    }

    /// Attaches a compiled vector `<svg>` template to `node_id`.
    pub fn set_node_svg_template(&mut self, node_id: NodeId, template: SvgVectorTemplate) {
        let mut inner = self.inner.borrow_mut();
        if inner.node(node_id).is_none() {
            return;
        }
        let media = inner.ensure_media(node_id);
        media.svg_template = Some(template);
        media.svg_cached = None;
        if let Some(n) = inner.node_mut(node_id) {
            n.set_text_dirty(true);
        }
        sync_node_text_context(&mut inner, node_id);
        inner.paint_dirty = true;
    }

    /// Sets the `href` target on an `<a href="...">` hyperlink node and registers it as focusable.
    pub fn set_link_href(&mut self, node_id: NodeId, href: impl Into<String>) {
        let clean = sanitize_untrusted_text(&href.into());
        let mut inner = self.inner.borrow_mut();
        if !promote_node_to_focusable_link(&mut inner, node_id) {
            return;
        }
        inner.ensure_media(node_id).link_href = Some(clean);
        inner.paint_dirty = true;
    }

    /// Returns the hyperlink `href` configured on `node_id`, if any.
    pub fn link_href(&self, node_id: NodeId) -> Option<String> {
        let inner = self.inner.borrow();
        let _ = inner.node(node_id)?;
        inner.media(node_id).and_then(|m| m.link_href.clone())
    }

    /// Registers a host callback invoked whenever an `<a href="...">` link is activated.
    pub fn set_on_link_activate(&mut self, handler: impl Fn(String) + 'static) {
        self.inner.borrow_mut().sparse.on_link_activate = Some(Rc::new(handler));
    }

    /// Returns the most recently activated `<a href="...">` URL on this `ViewTree`, if any.
    pub fn last_activated_link(&self) -> Option<String> {
        self.inner.borrow().sparse.last_activated_link.clone()
    }

    /// Binds a reactive `href` expression (`<a href="{expr}">`) directly to `node_id`.
    pub fn bind_dynamic_link_href(
        &mut self,
        node_id: NodeId,
        getter: impl Fn() -> String + 'static,
    ) {
        self.bind_dynamic_link_href_rc(node_id, Rc::new(getter));
    }

    fn bind_dynamic_link_href_rc(&mut self, node_id: NodeId, getter: Rc<dyn Fn() -> String>) {
        if !promote_node_to_focusable_link(&mut self.inner.borrow_mut(), node_id) {
            return;
        }
        let tree_weak = Rc::downgrade(&self.inner);
        let effect = Effect::new(move || {
            let next_href = sanitize_untrusted_text(&getter());
            if let Some(rc) = tree_weak.upgrade() {
                let mut inner = rc.borrow_mut();
                if inner.node(node_id).is_some() {
                    inner.ensure_media(node_id).link_href = Some(next_href);
                }
            }
        });
        self.inner.borrow_mut().push_effect(node_id, effect);
    }

    fn clear_node_image_source(&mut self, node_id: NodeId) {
        let mut inner = self.inner.borrow_mut();
        if let Some(media) = inner.sparse.media.get_mut(&node_id) {
            media.image_source = None;
            media.host_mounted_image = false;
        }
        if let Some(n) = inner.node_mut(node_id) {
            n.set_text_dirty(true);
        }
        sync_node_text_context(&mut inner, node_id);
        inner.paint_dirty = true;
    }
}

fn schedule_dynamic_image_load(
    rc: &Rc<std::cell::RefCell<ViewTreeInner>>,
    #[cfg(not(target_arch = "wasm32"))] tree_weak: &Weak<std::cell::RefCell<ViewTreeInner>>,
    node_id: NodeId,
    trimmed: String,
    resolved: PathBuf,
) {
    let (prev_scope, maybe_spawn) = {
        let mut inner = rc.borrow_mut();
        if inner.node(node_id).is_none() {
            return;
        }
        let media = inner.ensure_media(node_id);
        if media.loaded_src_key.as_deref() == Some(trimmed.as_str()) {
            return;
        }
        let prev_scope = media.load_scope.take();
        media.load_request_seq = media.load_request_seq.wrapping_add(1);
        let req_seq = media.load_request_seq;
        media.loaded_src_key = Some(trimmed);
        if resolved.as_os_str().is_empty() {
            media.image_source = None;
            media.host_mounted_image = false;
            if let Some(n) = inner.node_mut(node_id) {
                n.set_text_dirty(true);
            }
            sync_node_text_context(&mut inner, node_id);
            inner.paint_dirty = true;
            (prev_scope, None)
        } else {
            let load_scope = Scope::new();
            media.load_scope = Some(load_scope);
            (prev_scope, Some((load_scope, req_seq)))
        }
    };
    if let Some(s) = prev_scope {
        s.dispose();
    }
    let Some((load_scope, req_seq)) = maybe_spawn else {
        return;
    };
    spawn_disk_image_load(
        #[cfg(not(target_arch = "wasm32"))]
        tree_weak,
        load_scope,
        node_id,
        req_seq,
        resolved,
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_disk_image_load(
    tree_weak: &Weak<std::cell::RefCell<ViewTreeInner>>,
    load_scope: Scope,
    node_id: NodeId,
    req_seq: u64,
    resolved: PathBuf,
) {
    let weak_done = tree_weak.clone();
    load_scope.run(move || {
        let _handle = spawn_tokio_task(
            async move {
                tokio_handle()
                    .spawn_blocking(move || read_and_decode_image_file(&resolved).ok())
                    .await
                    .ok()
                    .flatten()
            },
            move |decoded_opt| apply_async_image_result(&weak_done, node_id, req_seq, decoded_opt),
        );
    });
}

#[cfg(target_arch = "wasm32")]
fn spawn_disk_image_load(load_scope: Scope, _node_id: NodeId, _req_seq: u64, _resolved: PathBuf) {
    load_scope.dispose();
}

#[cfg(not(target_arch = "wasm32"))]
fn apply_async_image_result(
    weak_tree: &Weak<std::cell::RefCell<ViewTreeInner>>,
    node_id: NodeId,
    req_seq: u64,
    decoded_opt: Option<DecodedImage>,
) {
    let Some(rc) = weak_tree.upgrade() else {
        return;
    };
    let finished_scope = {
        let mut inner = rc.borrow_mut();
        if inner.node(node_id).is_none() {
            return;
        }
        let media = inner.ensure_media(node_id);
        if media.load_request_seq != req_seq {
            return;
        }
        let sc = media.load_scope.take();
        media.image_source = decoded_opt.map(ImageSource::Decoded);
        media.host_mounted_image = false;
        if let Some(n) = inner.node_mut(node_id) {
            n.set_text_dirty(true);
        }
        sync_node_text_context(&mut inner, node_id);
        inner.paint_dirty = true;
        sc
    };
    if let Some(s) = finished_scope {
        s.dispose();
    }
}

/// Runs enclosing `<form>`, `<details>`/`<summary>`, `[popovertarget]`, and `<a href>` actions after activation.
pub(crate) fn invoke_post_activation_actions(tree: &ViewTree, node_id: NodeId) -> bool {
    let (sem_handled, form_act, link_act) = {
        let mut inner = tree.inner.borrow_mut();
        let sem_handled =
            crate::view_tree::semantics::handle_semantic_post_click(&mut inner, node_id);
        let form_act = trigger_button_form_action(&mut inner, node_id);
        let href = inner
            .node(node_id)
            .filter(|n| !n.is_disabled())
            .and_then(|_| inner.media(node_id))
            .and_then(|m| m.link_href.clone())
            .filter(|u| !u.is_empty());
        let link_act = href.map(|url| {
            inner.sparse.last_activated_link = Some(url.clone());
            (url, inner.sparse.on_link_activate.clone())
        });
        (sem_handled, form_act, link_act)
    };
    let mut handled = sem_handled || form_act.handled;
    form_act.invoke();
    if let Some((url, maybe_cb)) = link_act {
        handled = true;
        if let Some(cb) = maybe_cb {
            batch(|| cb(url));
        }
    }
    handled
}

fn resolve_asset_path(raw: &str, base_dir: Option<&Path>) -> std::path::PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() || base_dir.is_none() {
        p.to_path_buf()
    } else if let Some(dir) = base_dir {
        dir.join(p)
    } else {
        p.to_path_buf()
    }
}

/// Synchronizes Taffy intrinsic measurement when `node_id` backs an `<img>` or `<svg>`.
pub(crate) fn try_sync_media_intrinsic_context(
    inner: &mut ViewTreeInner,
    node_id: NodeId,
    taffy_node: taffy::NodeId,
) -> bool {
    let intrinsic = inner.media(node_id).and_then(|m| {
        if let Some(ImageSource::Decoded(img)) = &m.image_source {
            Some((img.width as f32, img.height as f32))
        } else {
            m.svg_template.as_ref().map(|t| t.intrinsic_size)
        }
    });
    let Some((iw, ih)) = intrinsic else {
        return false;
    };
    let ctx = krui_layout::TextContext {
        intrinsic_size: Some((iw, ih)),
        ..Default::default()
    };
    inner.layout_engine.set_text_context(taffy_node, Some(ctx));
    true
}
