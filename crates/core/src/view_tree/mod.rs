//! Retained `ViewTree` core structure, node lifecycle, Stylo cascade, and dev-preview helpers.

pub mod controls;
pub mod dom;
pub mod ime;
pub mod input;
pub mod paint_scene;
pub mod style_bridge;

pub use controls::{a11y, forms, select_popup};
pub(crate) use controls::{form_submit, semantics};
pub use dom::{bindings, control_flow, media_nodes, node};
pub(crate) use dom::{lifecycle::unmount_node_subtree, sparse_tables};
#[cfg(feature = "preview")]
pub use dom::{preview_inspect, preview_remount};
pub use ime::{cursor_nav, ime_host, text_edit};
pub(crate) use ime::{grapheme, ime_actions, text_selection_granularity};
pub use input::{events, focus_pointer, hit_test, key_routing, pointer_capture, scroll, touch};
pub(crate) use input::{focus_transition, hit_test_geom};
pub use paint_scene::paint;
pub(crate) use paint_scene::{
    paint_box_layers, paint_canvas, paint_text, paint_text_decorations, select_popup_paint,
    stacking, tooltip_paint,
};
pub(crate) use style_bridge::{
    clip_shape, content_box, keyframes_driver, layout_modes, pointer_vars, pseudo_sync,
    transitions, transitions_layout,
};
pub use style_bridge::{insets, sync};

pub use a11y::{A11yId, A11Y_ID_OFFSET};
pub use control_flow::BranchMountFn;
pub use form_submit::ButtonKind;
pub use grapheme::TextEditSnapshot;
pub use ime_host::{
    utf16_to_utf8_offset, utf16_to_utf8_range, utf8_to_utf16_offset, utf8_to_utf16_range,
    EditingState, EditorActionKind, ImeEvent, InputKind, TextInputHost,
};
pub use insets::ViewportInsets;
pub use key_routing::{
    caret_rect, key_name_for_keydown, prevent_default_key, route_key, route_key_with_repeat,
    FocusState, KeyAction, KeyInfo, KeydownOutcome, Mods,
};
pub use node::{
    ClickBinding, ClickTarget, FocusOrigin, HoveredCursorKind, ImeState, InputTarget, NodeId,
    ViewFragment,
};
#[cfg(feature = "preview")]
pub use preview_inspect::InspectedNodeInfo;
#[cfg(feature = "preview")]
pub use preview_remount::{normalize_preview_path_key, PreviewSavedNodes, SavedPreviewNode};
pub use touch::{TouchPhaseKind, TOUCH_SLOP_PX};

use krui_a11y::AccessibilityEngine;
use krui_layout::LayoutEngine;
use krui_signals::{FrameClock, Scope, Signal};
use krui_style::{ColorScheme, ColorSchemePreference, CompiledStyle, ReducedMotionPreference};
use krui_stylo_bridge::{
    DeviceConfig, StyleSheetHandle, StyloDomMutation, StyloNodeId, StyloStyleEngine,
};
use krui_text::TextEngine;
use node::{resolve_tag_semantic_role, sync_node_text_context, ElementNode};
use select_popup::OverlayStack;
use sparse_tables::{
    attach_compiled_node_to_parent, init_sparse_for_compiled_node, SparseComponents,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// Shared interior state of a `ViewTree` so fine-grained signal `Effect`s can mutate
/// specific node paint/layout slots in constant time without tree diffing.
pub(crate) struct ViewTreeInner {
    pub(crate) nodes: Vec<ElementNode>,
    pub(crate) sparse: SparseComponents,
    pub(crate) overlay: OverlayStack,
    pub(crate) generations: Vec<u32>,
    pub(crate) free_nodes: Vec<u32>,
    pub(crate) roots: Vec<NodeId>,
    pub(crate) root_scope: Scope,
    pub(crate) layout_engine: LayoutEngine,
    pub(crate) text_engine: TextEngine,
    pub(crate) a11y_engine: AccessibilityEngine,
    pub(crate) hovered_node: Option<NodeId>,
    pub(crate) pressed_node: Option<NodeId>,
    pub(crate) selecting_node: Option<NodeId>,
    pub(crate) scrollbar_drag: Option<(NodeId, f32)>,
    pub(crate) last_pointer_pos: Option<(f32, f32)>,
    pub(crate) last_viewport: (f32, f32),
    pub(crate) ime_target: Option<NodeId>,
    pub(crate) caret_visible: bool,
    pub(crate) interaction: pointer_capture::PointerFocusState,
    pub(crate) stylo: StyloStyleEngine,
    pub(crate) stylo_doc_root: StyloNodeId,
    pub(crate) stylo_body_root: StyloNodeId,
    pub(crate) stylo_to_view: HashMap<StyloNodeId, NodeId>,
    pub(crate) stylo_dirty: bool,
    pub(crate) fonts_keyframes_dirty: bool,
    pub(crate) doc_sheet_handles: Vec<StyleSheetHandle>,
    pub(crate) doc_sheet_hash: Option<u64>,
    pub(crate) propagated_canvas_node: Option<NodeId>,
    pub(crate) color_scheme_pref: ColorSchemePreference,
    pub(crate) system_color_scheme: ColorScheme,
    pub(crate) reduced_motion_pref: ReducedMotionPreference,
    pub(crate) os_reduced_motion: bool,
    pub(crate) embedded_fonts: HashMap<String, Arc<[u8]>>,
    pub(crate) scale_factor: f32,
    pub(crate) paint_dirty: bool,
    pub(crate) viewport_insets: insets::ViewportInsets,
    pub(crate) touch: touch::TouchTracker,
    pub(crate) animations_paused: bool,
    pub(crate) keyframes: keyframes_driver::KeyframeDriver,
    pub(crate) fixed_nodes: Vec<NodeId>,
    pub(crate) open_popovers: Vec<NodeId>,
    pub(crate) mount_batch_depth: u32,
    pub(crate) pending_mount_selector_eval: bool,
    #[cfg(feature = "preview")]
    pub(crate) preview_registry: Option<crate::registry::RegistryRef>,
}

impl ViewTreeInner {
    #[inline]
    pub(crate) fn node(&self, id: NodeId) -> Option<&ElementNode> {
        self.nodes
            .get(id.index as usize)
            .filter(|n| n.alive() && n.id.generation == id.generation)
    }

    #[inline]
    pub(crate) fn node_mut(&mut self, id: NodeId) -> Option<&mut ElementNode> {
        self.nodes
            .get_mut(id.index as usize)
            .filter(|n| n.alive() && n.id.generation == id.generation)
    }
}

impl Drop for ViewTreeInner {
    fn drop(&mut self) {
        let (effs, scopes) = self.sparse.clear_all_for_reset();
        self.root_scope.dispose();
        for eff in effs {
            eff.dispose();
        }
        for scope in scopes {
            scope.dispose();
        }
    }
}

/// Retained view tree constructed once at mount time and updated directly by fine-grained signals.
#[derive(Clone)]
pub struct ViewTree {
    pub(crate) inner: Rc<RefCell<ViewTreeInner>>,
}

impl Default for ViewTree {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewTree {
    /// Creates an empty `ViewTree`.
    pub fn new() -> Self {
        let mut stylo = StyloStyleEngine::new(DeviceConfig {
            unstyled: true,
            ..DeviceConfig::default()
        });
        let stylo_doc_root = stylo.alloc_element("html");
        let stylo_body_root = stylo.alloc_element("body");
        stylo.set_root_element(stylo_doc_root);
        stylo.apply_mutation(StyloDomMutation::AppendChild {
            parent: stylo_doc_root,
            child: stylo_body_root,
        });
        Self {
            inner: Rc::new(RefCell::new(ViewTreeInner {
                nodes: Vec::with_capacity(64),
                sparse: SparseComponents::default(),
                overlay: OverlayStack::default(),
                generations: Vec::with_capacity(64),
                free_nodes: Vec::new(),
                roots: Vec::new(),
                root_scope: Scope::new(),
                layout_engine: LayoutEngine::new(),
                text_engine: TextEngine::new(),
                a11y_engine: AccessibilityEngine::new(),
                hovered_node: None,
                pressed_node: None,
                selecting_node: None,
                scrollbar_drag: None,
                last_pointer_pos: None,
                last_viewport: (1024.0, 768.0),
                ime_target: None,
                caret_visible: true,
                interaction: pointer_capture::PointerFocusState::default(),
                stylo,
                stylo_doc_root,
                stylo_body_root,
                stylo_to_view: HashMap::new(),
                stylo_dirty: true,
                fonts_keyframes_dirty: true,
                doc_sheet_handles: Vec::new(),
                doc_sheet_hash: None,
                propagated_canvas_node: None,
                color_scheme_pref: ColorSchemePreference::System,
                system_color_scheme: ColorScheme::Light,
                reduced_motion_pref: ReducedMotionPreference::System,
                os_reduced_motion: false,
                embedded_fonts: HashMap::new(),
                scale_factor: 1.0,
                paint_dirty: true,
                viewport_insets: insets::ViewportInsets::default(),
                touch: touch::TouchTracker::default(),
                animations_paused: false,
                keyframes: keyframes_driver::KeyframeDriver::default(),
                fixed_nodes: Vec::new(),
                open_popovers: Vec::new(),
                mount_batch_depth: 0,
                pending_mount_selector_eval: false,
                #[cfg(feature = "preview")]
                preview_registry: None,
            })),
        }
    }

    /// Returns the in-memory byte size of a single core `ElementNode` slot.
    pub const fn element_node_byte_size() -> usize {
        std::mem::size_of::<ElementNode>()
    }

    /// Returns the total number of live entries across all sparse secondary component tables.
    pub fn sparse_component_entry_count(&self) -> usize {
        self.inner.borrow().sparse.total_entry_count()
    }

    /// Returns the first root `NodeId` of the tree, if any.
    pub fn first_root(&self) -> Option<NodeId> {
        self.inner.borrow().roots.first().copied()
    }

    /// Mounts a compiled `ViewFragment` (such as the output of `krui!(...)`) into the tree.
    pub fn mount(&mut self, fragment: ViewFragment) -> Vec<NodeId> {
        let scope = self.inner.borrow().root_scope;
        scope.run(|| fragment.mount(self, None))
    }

    /// Evaluates `build` and mounts the resulting `ViewFragment` inside the page's `root_scope`.
    pub fn mount_with(&mut self, build: impl FnOnce() -> ViewFragment) -> Vec<NodeId> {
        let scope = self.inner.borrow().root_scope;
        let fragment = scope.run(build);
        scope.run(|| fragment.mount(self, None))
    }

    /// Safely clears all existing nodes, disposes the previous root `Scope`, and mounts `fragment`.
    pub fn replace_root(&mut self, fragment: ViewFragment) -> Vec<NodeId> {
        self.replace_root_with(|| fragment)
    }

    /// Safely clears all existing nodes, disposes the previous root `Scope`, and runs `build`.
    pub fn replace_root_with(&mut self, build: impl FnOnce() -> ViewFragment) -> Vec<NodeId> {
        let next_scope = Scope::new();
        let (old_scope, leftover_effects, leftover_scopes) =
            self.inner.borrow_mut().reset_for_replace_root(next_scope);
        old_scope.dispose();
        for eff in leftover_effects {
            eff.dispose();
        }
        for scope in leftover_scopes {
            scope.dispose();
        }
        let fragment = next_scope.run(build);
        next_scope.run(|| fragment.mount(self, None))
    }

    /// Replaces the current root view with `fragment` and triggers a page transition.
    pub fn transition_to(&mut self, clock: &mut FrameClock, fragment: ViewFragment) -> Vec<NodeId> {
        self.transition_to_with(clock, |_| fragment)
    }

    /// Replaces the current root view by running `build` inside a fresh page `root_scope`.
    pub fn transition_to_with(
        &mut self,
        clock: &mut FrameClock,
        build: impl FnOnce(&mut FrameClock) -> ViewFragment,
    ) -> Vec<NodeId> {
        let next_scope = Scope::new();
        let (old_scope, leftover_effects, leftover_scopes) =
            self.inner.borrow_mut().reset_for_replace_root(next_scope);
        old_scope.dispose();
        for eff in leftover_effects {
            eff.dispose();
        }
        for scope in leftover_scopes {
            scope.dispose();
        }
        let fragment = next_scope.run(|| build(clock));
        let roots = next_scope.run(|| fragment.mount(self, None));
        next_scope.run(|| {
            let slide_target = Signal::new(0.0_f32);
            let fade_target = Signal::new(1.0_f32);
            let slide_x = clock.spring_to(32.0_f32, slide_target, 260.0, 26.0);
            let fade_alpha = clock.spring_to(0.12_f32, fade_target, 220.0, 24.0);
            for &root_id in &roots {
                self.bind_paint_translate_x(root_id, slide_x);
                self.bind_paint_opacity(root_id, fade_alpha);
            }
        });
        roots
    }

    /// Returns the number of alive element nodes in the view tree (excluding `<if>`/`<for>` anchors).
    pub fn node_count(&self) -> usize {
        self.inner
            .borrow()
            .nodes
            .iter()
            .filter(|n| n.alive() && !n.is_anchor())
            .count()
    }

    /// Returns the total number of alive retained node slots (including `<if>`/`<for>` anchors).
    pub fn retained_node_count(&self) -> usize {
        self.inner
            .borrow()
            .nodes
            .iter()
            .filter(|n| n.alive())
            .count()
    }

    /// Returns the total number of live Taffy layout nodes in the internal `LayoutEngine`.
    pub fn taffy_node_count(&self) -> usize {
        self.inner.borrow().layout_engine.total_node_count()
    }

    /// Returns the total number of Taffy layout passes performed on this tree.
    pub fn relayout_passes(&self) -> u64 {
        self.inner.borrow().layout_engine.relayout_passes()
    }

    /// Returns the number of shaped text layouts currently cached in the `TextEngine`.
    pub fn cached_text_layout_count(&self) -> usize {
        self.inner.borrow().text_engine.cached_layout_count()
    }

    /// Called by compiled `krui!` code to allocate a node with pre-resolved `CompiledStyle`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_compiled_node(
        &mut self,
        parent: Option<NodeId>,
        tag: &'static str,
        element_id: Option<String>,
        aria_label: Option<String>,
        style: CompiledStyle,
        is_button: bool,
        is_input: bool,
        is_canvas: bool,
        is_secret: bool,
    ) -> NodeId {
        let role = resolve_tag_semantic_role(tag, is_button, is_input, is_canvas, is_secret);
        let is_scroll = style.layout.is_scroll_container();
        let mut inner = self.inner.borrow_mut();
        let taffy_node = inner.layout_engine.create_node(&style.layout);
        let (slot_idx, generation) = inner.allocate_slot();
        let node_id = NodeId::new(slot_idx, generation);
        if role.is_focusable() {
            inner
                .a11y_engine
                .register_focusable(A11yId::from(node_id).0);
        }
        let retained = ElementNode::new_element(
            node_id, parent, tag, element_id, aria_label, role, is_secret, taffy_node, style,
        );
        if (slot_idx as usize) < inner.nodes.len() {
            inner.nodes[slot_idx as usize] = retained;
        } else {
            inner.nodes.push(retained);
        }
        init_sparse_for_compiled_node(&mut inner, node_id, role, is_secret, is_canvas, is_scroll);
        attach_compiled_node_to_parent(&mut inner, parent, node_id, taffy_node);
        semantics::sync_semantic_visibility(&mut inner, node_id);
        sync_node_text_context(&mut inner, node_id);
        inner.paint_dirty = true;
        node_id
    }

    /// Returns `true` if any visual state changed since the last `build_paint_scene`.
    pub fn is_paint_dirty(&self) -> bool {
        self.inner.borrow().paint_dirty
    }

    /// Sets whether the focused `<input>` caret is currently in the visible phase of its blink cycle.
    pub fn set_caret_visible(&mut self, visible: bool) -> bool {
        let mut inner = self.inner.borrow_mut();
        if inner.caret_visible != visible {
            inner.caret_visible = visible;
            if inner.a11y_engine.focused_id().is_some() {
                inner.paint_dirty = true;
                return true;
            }
        }
        false
    }

    /// Updates the display HiDPI scale factor (`1.0..=4.0`), marking text layouts dirty when changed.
    pub fn set_scale_factor(&mut self, scale_factor: f32) {
        let clamped = scale_factor.clamp(1.0, 4.0);
        let mut inner = self.inner.borrow_mut();
        if (inner.scale_factor - clamped).abs() > 0.005 {
            inner.scale_factor = clamped;
            let (vw, vh) = inner.last_viewport;
            inner.stylo.set_viewport(vw, vh, clamped);
            inner.stylo_dirty = true;
            for node in inner.nodes.iter_mut().filter(|n| n.alive()) {
                node.set_text_dirty(true);
            }
            inner.paint_dirty = true;
        }
    }

    /// Looks up an alive node by its HTML `id="..."` attribute.
    pub fn find_by_id(&self, element_id: &str) -> Option<NodeId> {
        self.inner
            .borrow()
            .nodes
            .iter()
            .find(|n| n.alive() && !n.is_anchor() && n.element_id.as_deref() == Some(element_id))
            .map(|n| n.id)
    }

    /// Returns the text content of `node_id`, or `None` if `node_id` is stale.
    pub fn node_text(&self, node_id: NodeId) -> Option<String> {
        self.inner.borrow().node(node_id).map(|n| n.text.clone())
    }

    /// Returns the caret character index of `node_id`, or `None` if `node_id` is stale.
    pub fn node_cursor_char_idx(&self, node_id: NodeId) -> Option<usize> {
        let inner = self.inner.borrow();
        let _ = inner.node(node_id)?;
        Some(
            inner
                .text_edit(node_id)
                .map(|e| e.cursor_char_idx)
                .unwrap_or(0),
        )
    }

    /// Programmatically sets accessibility/keyboard focus to `node_id` if it is alive, focusable, and enabled.
    pub fn set_focus_to_node(&mut self, node_id: NodeId) -> bool {
        let focus_cbs = {
            let mut inner = self.inner.borrow_mut();
            let has_focus_listener = inner
                .listeners(node_id)
                .is_some_and(|l| l.on_focus.is_some() || l.on_blur.is_some());
            let can_focus = a11y::is_node_a11y_visible(&inner, node_id)
                && inner.node(node_id).is_some_and(|n| {
                    !n.is_disabled() && (n.role.is_focusable() || has_focus_listener)
                });
            if !can_focus {
                return false;
            }
            let cbs = focus_pointer::transition_focus(&mut inner, Some(node_id));
            inner.caret_visible = true;
            inner.paint_dirty = true;
            cbs
        };
        focus_cbs.invoke();
        true
    }
}
