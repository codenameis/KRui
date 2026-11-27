//! # `krui-layout`
//!
//! Taffy 0.15 Flexbox, Block, and CSS Grid solver integration for KRui.
//!
//! Backs each layout node in a `SlotMap` slot that implements [`krui_stylo_bridge::TaffyLayoutHost`],
//! reading CSS properties zero-copy from [`StyloStyleEngine`] while allowing explicit [`LayoutStyle`]
//! overrides for the synthetic root, `LayoutSlot` bindings, Tier-2 transitions, and standalone tests.

#![forbid(unsafe_code)]

mod host;
mod measure;

use host::{synthetic_root_layout_style, LayoutHost, LayoutNodeSlot};
use krui_style::LayoutStyle;
use krui_stylo_bridge::{
    run_taffy_layout, run_taffy_layout_standalone, StyloNodeId, StyloStyleEngine,
};
pub use measure::MIN_MEASURED_FONT_SIZE_PX;
use slotmap::{DefaultKey, SlotMap};
use std::sync::Arc;
use taffy::prelude::{AvailableSpace, Size};
use taffy::{Cache, ClearState, Layout, NodeId};

/// Resolved axis-aligned bounding box in logical pixels after Taffy layout.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LayoutRect {
    /// Left edge in logical pixels.
    pub x: f32,
    /// Top edge in logical pixels.
    pub y: f32,
    /// Box width in logical pixels.
    pub width: f32,
    /// Box height in logical pixels.
    pub height: f32,
}

impl LayoutRect {
    /// Returns `true` when `(px, py)` lies inside or on the rectangle.
    pub const fn contains(self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

/// Text and media measurement context attached to leaf nodes in [`LayoutEngine`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextContext {
    /// Measured plain text string.
    pub text: String,
    /// Typography configuration used for shaping and wrapping.
    pub text_style: krui_style::TextStyle,
    /// Reserves at least one full line height when `text` is empty.
    pub min_line_height: bool,
    /// Restricts measurement to a single unwrapped line.
    pub single_line: bool,
    /// Optional intrinsic media dimensions (`<img>`, `<svg>`, `ExternalTexture`) for aspect-preserving `Auto` layout.
    pub intrinsic_size: Option<(f32, f32)>,
    /// Optional multi-span rich paragraph runs for unified single-paragraph wrapping and sizing.
    pub rich_spans: Option<Arc<[krui_style::StyledTextSpan]>>,
}

/// Manages the Taffy 0.15 layout tree with a synthetic viewport root and incremental cache clearing.
pub struct LayoutEngine {
    slots: SlotMap<DefaultKey, LayoutNodeSlot>,
    synthetic_root: NodeId,
    last_viewport_size: (f32, f32),
    relayout_passes: u64,
}

impl Default for LayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine {
    /// Creates a new [`LayoutEngine`] with an initial empty synthetic viewport root.
    pub fn new() -> Self {
        let mut slots = SlotMap::new();
        let root_slot = LayoutNodeSlot::new(
            None,
            LayoutStyle::default(),
            Some(synthetic_root_layout_style(0.0, 0.0)),
            None,
        );
        let synthetic_root = NodeId::from(slots.insert(root_slot));
        Self {
            slots,
            synthetic_root,
            last_viewport_size: (0.0, 0.0),
            relayout_passes: 0,
        }
    }

    /// Returns the total number of Taffy layout passes executed since creation.
    pub fn relayout_passes(&self) -> u64 {
        self.relayout_passes
    }

    /// Returns `true` if any node in the layout tree is dirty or if the viewport dimensions changed.
    pub fn needs_layout(&self, viewport_width: f32, viewport_height: f32) -> bool {
        let size_changed = (self.last_viewport_size.0 - viewport_width).abs() > f32::EPSILON
            || (self.last_viewport_size.1 - viewport_height).abs() > f32::EPSILON;
        size_changed
            || self
                .slots
                .get(self.synthetic_root.into())
                .is_some_and(|s| s.cache.is_empty())
    }

    /// Allocates a new leaf node using an explicit fallback [`LayoutStyle`] (without Stylo binding).
    pub fn create_node(&mut self, style: &LayoutStyle) -> NodeId {
        let slot = LayoutNodeSlot::new(None, *style, None, None);
        NodeId::from(self.slots.insert(slot))
    }

    /// Allocates a new leaf node paired directly with a Stylo DOM node.
    pub fn create_stylo_node(&mut self, stylo_node: StyloNodeId) -> NodeId {
        let slot = LayoutNodeSlot::new(Some(stylo_node), LayoutStyle::default(), None, None);
        NodeId::from(self.slots.insert(slot))
    }

    /// Allocates a new leaf node with an initial [`TextContext`].
    pub fn create_node_with_context(
        &mut self,
        style: &LayoutStyle,
        context: TextContext,
    ) -> NodeId {
        let slot = LayoutNodeSlot::new(None, *style, None, Some(context));
        NodeId::from(self.slots.insert(slot))
    }

    /// Associates or clears the paired [`StyloNodeId`] for `node` and marks it dirty if changed.
    pub fn set_stylo_node(&mut self, node: NodeId, stylo_node: Option<StyloNodeId>) {
        if let Some(slot) = self.slots.get_mut(node.into()) {
            if slot.stylo_node != stylo_node {
                slot.stylo_node = stylo_node;
                self.mark_dirty(node);
            }
        }
    }

    /// Sets an explicit [`LayoutStyle`] override on `node` (used by `LayoutSlot` and Tier-2 transitions).
    pub fn set_layout_override(&mut self, node: NodeId, style: LayoutStyle) {
        if let Some(slot) = self.slots.get_mut(node.into()) {
            if slot.override_style.as_ref() != Some(&style) {
                slot.override_style = Some(style);
                self.mark_dirty(node);
            }
        }
    }

    /// Removes any explicit [`LayoutStyle`] override on `node`, restoring its Stylo or fallback style.
    pub fn clear_layout_override(&mut self, node: NodeId) {
        if let Some(slot) = self.slots.get_mut(node.into()) {
            if slot.override_style.take().is_some() {
                self.mark_dirty(node);
            }
        }
    }

    /// Attaches a top-level user root node under the synthetic root.
    pub fn append_root(&mut self, root_node: NodeId) {
        self.append_child(self.synthetic_root, root_node);
    }

    /// Appends `child` to `parent` in the layout tree (ignoring self-loops and ancestor cycles).
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        if parent == child || child == self.synthetic_root || self.is_ancestor(child, parent) {
            return;
        }
        let old_parent = self.slots.get(child.into()).and_then(|s| s.parent);
        if let Some(old_p) = old_parent {
            if old_p != parent {
                self.detach_child_from_parent(old_p, child);
            } else {
                return;
            }
        }
        let Some(child_slot) = self.slots.get_mut(child.into()) else {
            return;
        };
        child_slot.parent = Some(parent);
        if let Some(parent_slot) = self.slots.get_mut(parent.into()) {
            parent_slot.children.push(child);
            self.mark_dirty(parent);
        }
    }

    /// Removes `child` from `parent` (or from the synthetic root if `parent` is `None`).
    pub fn remove_child(&mut self, parent: Option<NodeId>, child: NodeId) {
        let p = parent.unwrap_or(self.synthetic_root);
        self.detach_child_from_parent(p, child);
        if let Some(slot) = self.slots.get_mut(child.into()) {
            if slot.parent == Some(p) {
                slot.parent = None;
            }
        }
    }

    /// Removes `node` from the layout slab, detaching its parent and clearing children's parent links.
    pub fn remove_node(&mut self, node: NodeId) {
        if node == self.synthetic_root {
            return;
        }
        let Some(removed) = self.slots.remove(node.into()) else {
            return;
        };
        if let Some(p) = removed.parent {
            self.detach_child_from_parent(p, node);
        }
        for child in removed.children {
            if let Some(child_slot) = self.slots.get_mut(child.into()) {
                if child_slot.parent == Some(node) {
                    child_slot.parent = None;
                }
            }
        }
    }

    /// Recursively removes `node` and all of its descendants from the layout slab.
    pub fn remove_subtree(&mut self, node: NodeId) {
        if node == self.synthetic_root {
            return;
        }
        let parent = self.slots.get(node.into()).and_then(|s| s.parent);
        if let Some(p) = parent {
            self.detach_child_from_parent(p, node);
        }
        let mut stack = vec![node];
        while let Some(cur) = stack.pop() {
            if cur == self.synthetic_root {
                continue;
            }
            if let Some(removed) = self.slots.remove(cur.into()) {
                stack.extend(removed.children);
            }
        }
    }

    /// Returns the total number of live layout nodes (including the synthetic root).
    pub fn total_node_count(&self) -> usize {
        self.slots.len()
    }

    fn is_ancestor(&self, candidate: NodeId, mut node: NodeId) -> bool {
        let mut hops = 0_usize;
        while let Some(p) = self.slots.get(node.into()).and_then(|s| s.parent) {
            if p == candidate {
                return true;
            }
            node = p;
            hops += 1;
            if hops > self.slots.len() {
                break;
            }
        }
        false
    }

    /// Replaces the ordered children of `parent` (or the synthetic root if `parent` is `None`).
    pub fn set_children(&mut self, parent: Option<NodeId>, children: &[NodeId]) {
        let p = parent.unwrap_or(self.synthetic_root);
        let Some(slot) = self.slots.get(p.into()) else {
            return;
        };
        if slot.children == children {
            return;
        }
        let old_children = slot.children.clone();
        for old_c in old_children {
            if !children.contains(&old_c) {
                if let Some(cs) = self.slots.get_mut(old_c.into()) {
                    if cs.parent == Some(p) {
                        cs.parent = None;
                    }
                }
            }
        }
        let mut attached = Vec::with_capacity(children.len());
        for &c in children {
            if c == self.synthetic_root || c == p || !self.slots.contains_key(c.into()) {
                continue;
            }
            if let Some(prev) = self.slots.get(c.into()).and_then(|s| s.parent) {
                if prev != p {
                    self.detach_child_from_parent(prev, c);
                }
            }
            if let Some(cs) = self.slots.get_mut(c.into()) {
                cs.parent = Some(p);
            }
            attached.push(c);
        }
        if let Some(ps) = self.slots.get_mut(p.into()) {
            ps.children = attached;
        }
        self.mark_dirty(p);
    }

    /// Replaces children of `synthetic_root` with user roots followed by `position: fixed` viewport nodes.
    pub fn set_root_and_fixed_children(
        &mut self,
        root_children: &[NodeId],
        fixed_children: &[NodeId],
    ) {
        let mut all = Vec::with_capacity(root_children.len() + fixed_children.len());
        all.extend_from_slice(root_children);
        for &nid in fixed_children {
            if !all.contains(&nid) {
                all.push(nid);
            }
        }
        self.set_children(None, &all);
    }

    /// Updates a node's fallback [`LayoutStyle`] and marks it dirty only when changed.
    pub fn set_layout_style(&mut self, node: NodeId, style: &LayoutStyle) {
        let Some(slot) = self.slots.get_mut(node.into()) else {
            return;
        };
        if slot.fallback_style == *style {
            return;
        }
        slot.fallback_style = *style;
        self.mark_dirty(node);
    }

    /// Sets or clears the [`TextContext`] on `node` and marks `node` dirty when changed.
    pub fn set_text_context(&mut self, node: NodeId, context: Option<TextContext>) {
        self.set_text_context_with_dirty(node, context, true);
    }

    /// Sets or clears the [`TextContext`] on `node` when it differs from the stored context.
    pub fn set_text_context_with_dirty(
        &mut self,
        node: NodeId,
        context: Option<TextContext>,
        mark_dirty: bool,
    ) {
        let Some(slot) = self.slots.get_mut(node.into()) else {
            return;
        };
        let effective = context;
        if slot.text_context == effective {
            return;
        }
        slot.text_context = effective;
        if mark_dirty {
            self.mark_dirty(node);
        }
    }

    /// Marks `node` and its ancestors up to the synthetic root dirty for incremental relayout.
    pub fn mark_dirty(&mut self, node: NodeId) {
        let mut cur = Some(node);
        while let Some(id) = cur {
            let Some(slot) = self.slots.get_mut(id.into()) else {
                break;
            };
            if matches!(slot.cache.clear(), ClearState::AlreadyEmpty) {
                break;
            }
            cur = slot.parent;
        }
    }

    /// Computes layout against [`StyloStyleEngine`] and any explicit overrides when dirty.
    pub fn compute_if_dirty_with_stylo(
        &mut self,
        engine: &StyloStyleEngine,
        viewport_width: f32,
        viewport_height: f32,
        measure: impl FnMut(&TextContext, Option<f32>) -> (f32, f32),
    ) -> bool {
        self.compute_if_dirty_inner(Some(engine), viewport_width, viewport_height, measure)
    }

    /// Computes standalone layout using explicit [`LayoutStyle`] nodes when dirty.
    pub fn compute_if_dirty_with_measure(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        measure: impl FnMut(&TextContext, Option<f32>) -> (f32, f32),
    ) -> bool {
        self.compute_if_dirty_inner(None, viewport_width, viewport_height, measure)
    }

    /// Computes layout for `root` (attaching it under `synthetic_root` if needed) when dirty.
    pub fn compute_if_dirty(
        &mut self,
        root: NodeId,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        if let Some(root_slot) = self.slots.get(self.synthetic_root.into()) {
            if !root_slot.children.contains(&root) {
                self.append_root(root);
            }
        }
        self.compute_if_dirty_with_measure(viewport_width, viewport_height, |_, _| (0.0, 0.0))
    }

    /// Reads the computed local layout rectangle of `node` relative to its parent.
    pub fn local_rect(&self, node: NodeId) -> LayoutRect {
        let Some(slot) = self.slots.get(node.into()) else {
            return LayoutRect::default();
        };
        LayoutRect {
            x: slot.final_layout.location.x,
            y: slot.final_layout.location.y,
            width: slot.final_layout.size.width,
            height: slot.final_layout.size.height,
        }
    }

    /// Returns resolved `(top, right, bottom, left)` padding pixels computed by Taffy for `node`.
    pub fn resolved_padding(&self, node: NodeId) -> [f32; 4] {
        let Some(slot) = self.slots.get(node.into()) else {
            return [0.0; 4];
        };
        let p = slot.final_layout.padding;
        [p.top, p.right, p.bottom, p.left]
    }

    /// Returns resolved `(top, right, bottom, left)` border pixels computed by Taffy for `node`.
    pub fn resolved_border(&self, node: NodeId) -> [f32; 4] {
        let Some(slot) = self.slots.get(node.into()) else {
            return [0.0; 4];
        };
        let b = slot.final_layout.border;
        [b.top, b.right, b.bottom, b.left]
    }

    /// Returns the scrollable content dimensions `(width, height)` computed by Taffy for `node`.
    pub fn content_size(&self, node: NodeId) -> (f32, f32) {
        let Some(slot) = self.slots.get(node.into()) else {
            return (0.0, 0.0);
        };
        let layout = &slot.final_layout;
        (
            layout.scrollable_overflow_rect.right.max(layout.size.width),
            layout
                .scrollable_overflow_rect
                .bottom
                .max(layout.size.height),
        )
    }

    fn detach_child_from_parent(&mut self, parent: NodeId, child: NodeId) {
        if let Some(p_slot) = self.slots.get_mut(parent.into()) {
            let before = p_slot.children.len();
            p_slot.children.retain(|&c| c != child);
            if p_slot.children.len() != before {
                self.mark_dirty(parent);
            }
        }
    }

    fn compute_if_dirty_inner(
        &mut self,
        engine: Option<&StyloStyleEngine>,
        viewport_width: f32,
        viewport_height: f32,
        mut measure: impl FnMut(&TextContext, Option<f32>) -> (f32, f32),
    ) -> bool {
        let size_changed = (self.last_viewport_size.0 - viewport_width).abs() > f32::EPSILON
            || (self.last_viewport_size.1 - viewport_height).abs() > f32::EPSILON;
        if size_changed {
            self.set_layout_override(
                self.synthetic_root,
                synthetic_root_layout_style(viewport_width, viewport_height),
            );
        }
        if !self
            .slots
            .get(self.synthetic_root.into())
            .is_some_and(|s| s.cache.is_empty())
        {
            return false;
        }
        let available = Size {
            width: AvailableSpace::Definite(viewport_width),
            height: AvailableSpace::Definite(viewport_height),
        };
        let mut host = LayoutHost {
            slots: &mut self.slots,
            measure: &mut measure,
            orphan_cache: Cache::new(),
            orphan_layout: Layout::new(),
        };
        match engine {
            Some(eng) => run_taffy_layout(eng, &mut host, self.synthetic_root, available, true),
            None => run_taffy_layout_standalone(&mut host, self.synthetic_root, available, true),
        }
        self.last_viewport_size = (viewport_width, viewport_height);
        self.relayout_passes += 1;
        true
    }
}
