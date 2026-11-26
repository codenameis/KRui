//! `StyleResolver` boundary trait and W3C `@container` size feedback synchronization.

use super::restyle_sync::flush_pending_restyle;
use crate::view_tree::{NodeId, ViewTreeInner};
use krui_style::{ColorScheme, CompiledStyle, LayoutStyle, PaintStyle, TextStyle};
use krui_stylo_bridge::{
    PointerKind, RestyleReport, StyleSheetHandle, StyleSheetOrigin, StyloDomMutation, StyloNodeId,
    StyloStyleEngine,
};

/// Internal style engine abstraction isolating `ViewTree` from direct `StyloStyleEngine` calls.
#[allow(dead_code)]
pub(crate) trait StyleResolver {
    fn add_stylesheet(&mut self, origin: StyleSheetOrigin, css: &str) -> StyleSheetHandle;
    fn remove_stylesheet(&mut self, handle: StyleSheetHandle) -> bool;
    fn apply_mutation(&mut self, mutation: StyloDomMutation);
    fn set_viewport(&mut self, width: f32, height: f32, dpr: f32);
    fn set_color_scheme(&mut self, color_scheme: ColorScheme);
    fn set_pointer(&mut self, pointer: PointerKind);
    fn has_container_queries(&self) -> bool;
    fn set_container_size(
        &mut self,
        node: StyloNodeId,
        width: Option<f32>,
        height: Option<f32>,
    ) -> bool;
    fn container_type_axes(&self, node: StyloNodeId) -> (bool, bool);
    fn restyle(&mut self) -> RestyleReport;
    fn layout_style(&self, node: StyloNodeId) -> Option<LayoutStyle>;
    fn paint_style(&self, node: StyloNodeId) -> Option<PaintStyle>;
    fn text_style(&self, node: StyloNodeId) -> Option<(TextStyle, krui_style::InheritedTextProps)>;
    fn compiled_style(&self, node: StyloNodeId) -> Option<CompiledStyle>;
}

/// Default runtime Stylo resolver backing desktop, kiosk, and Wear OS hosts.
#[allow(dead_code)]
pub(crate) struct StyloRuntimeResolver<'a>(pub(crate) &'a mut StyloStyleEngine);

impl StyleResolver for StyloRuntimeResolver<'_> {
    fn add_stylesheet(&mut self, origin: StyleSheetOrigin, css: &str) -> StyleSheetHandle {
        self.0.add_stylesheet(origin, css)
    }

    fn remove_stylesheet(&mut self, handle: StyleSheetHandle) -> bool {
        self.0.remove_stylesheet(handle)
    }

    fn apply_mutation(&mut self, mutation: StyloDomMutation) {
        self.0.apply_mutation(mutation);
    }

    fn set_viewport(&mut self, width: f32, height: f32, dpr: f32) {
        self.0.set_viewport(width, height, dpr);
    }

    fn set_color_scheme(&mut self, color_scheme: ColorScheme) {
        self.0.set_color_scheme(color_scheme);
    }

    fn set_pointer(&mut self, pointer: PointerKind) {
        self.0.set_pointer(pointer);
    }

    fn has_container_queries(&self) -> bool {
        self.0.has_container_queries()
    }

    fn set_container_size(
        &mut self,
        node: StyloNodeId,
        width: Option<f32>,
        height: Option<f32>,
    ) -> bool {
        self.0.set_container_size(node, width, height)
    }

    fn container_type_axes(&self, node: StyloNodeId) -> (bool, bool) {
        self.0.container_type_axes(node)
    }

    fn restyle(&mut self) -> RestyleReport {
        self.0.restyle()
    }

    fn layout_style(&self, node: StyloNodeId) -> Option<LayoutStyle> {
        self.0.layout_style(node)
    }

    fn paint_style(&self, node: StyloNodeId) -> Option<PaintStyle> {
        self.0.paint_style(node)
    }

    fn text_style(&self, node: StyloNodeId) -> Option<(TextStyle, krui_style::InheritedTextProps)> {
        self.0.text_style(node)
    }

    fn compiled_style(&self, node: StyloNodeId) -> Option<CompiledStyle> {
        let layout = self.0.layout_style(node)?;
        let (text, _) = self.0.text_style(node)?;
        let paint = self.0.paint_style(node)?;
        Some(CompiledStyle {
            layout,
            text,
            paint,
        })
    }
}

/// Feeds laid-out content-box dimensions of CSS `container-type` elements back into Stylo
/// and flushes any resulting `@container` descendant restyles.
pub(crate) fn sync_container_sizes_and_restyle(inner: &mut ViewTreeInner) -> bool {
    if !inner.stylo.has_container_queries() {
        return false;
    }
    let updates: Vec<(NodeId, StyloNodeId, Option<f32>, Option<f32>)> = inner
        .nodes
        .iter()
        .filter_map(|n| {
            if !n.alive() || n.is_anchor() {
                return None;
            }
            let sid = n.stylo_node?;
            let tn = n.taffy_node?;
            let (track_inline, track_block) = inner.stylo.container_type_axes(sid);
            if !track_inline && !track_block {
                return None;
            }
            let rect = inner.layout_engine.local_rect(tn);
            let pad = inner.layout_engine.resolved_padding(tn);
            let brd = inner.layout_engine.resolved_border(tn);
            let cw = (rect.width - pad[1] - pad[3] - brd[1] - brd[3]).max(0.0);
            let ch = (rect.height - pad[0] - pad[2] - brd[0] - brd[2]).max(0.0);
            Some((
                n.id,
                sid,
                track_inline.then_some(cw),
                track_block.then_some(ch),
            ))
        })
        .collect();
    if updates.is_empty() {
        return false;
    }
    let mut any_changed = false;
    for (_vid, sid, w_opt, h_opt) in updates {
        if inner.stylo.set_container_size(sid, w_opt, h_opt) {
            any_changed = true;
        }
    }
    if any_changed {
        inner.stylo_dirty = true;
        flush_pending_restyle(inner);
    }
    any_changed
}
