//! Node inspection snapshot (`InspectedNodeInfo`) and source-span helpers (`#[cfg(feature = "preview")]`).

use crate::view_tree::{NodeId, ViewTree, ViewTreeInner};
use krui_a11y::SemanticRole;

/// Snapshot of an alive node in the `ViewTree` for `krui serve` (`inspect_tree`) and debug overlays.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectedNodeInfo {
    /// Generational node handle (`NodeId`).
    pub id: NodeId,
    /// HTML tag name (`"div"`, `"input"`, `"button"`).
    pub tag: String,
    /// Optional HTML `id="..."` attribute.
    pub element_id: Option<String>,
    /// Structural path key (`"root/0:div/1:input"`).
    pub path_key: String,
    /// Tree depth (`0` for root elements).
    pub depth: usize,
    /// Computed world-space layout rectangle (`x`, `y`, `width`, `height`).
    pub world_rect: krui_layout::LayoutRect,
    /// Current text content.
    pub text: String,
    /// Whether this node is interactive (button or text input) for `hit` overlay visualization.
    pub is_interactive: bool,
    /// 1-based source line of the originating `.krui` opening tag (`0` if unknown).
    pub source_line: u32,
    /// 1-based source column of the originating `.krui` opening tag (`0` if unknown).
    pub source_column: u32,
    /// Computed padding `(top, right, bottom, left)` in logical pixels.
    pub padding: (f32, f32, f32, f32),
    /// Deduplicated `--krui-*` / app token names bound on this node.
    pub tokens: Vec<String>,
}

impl ViewTree {
    /// Returns a pre-order list of all alive non-anchor nodes for `inspect_tree` and debug overlays.
    pub fn inspect_nodes(&self) -> Vec<InspectedNodeInfo> {
        let inner = self.inner.borrow();
        let mut out = Vec::new();
        for &root_id in &inner.roots {
            collect_inspected_nodes(&inner, root_id, 0, &mut out);
        }
        out
    }

    /// Sets the 1-based `.krui` source line and column on `node_id` (`#[cfg(feature = "preview")]`).
    pub fn set_node_source_span(&mut self, node_id: NodeId, line: u32, column: u32) {
        let mut inner = self.inner.borrow_mut();
        if let Some(node) = inner.node_mut(node_id) {
            node.preview_source_span = (line, column);
        }
    }
}

fn collect_inspected_node_tokens(inner: &ViewTreeInner, node_id: NodeId) -> Vec<String> {
    inner
        .sparse
        .classes
        .get(&node_id)
        .cloned()
        .unwrap_or_default()
}

fn collect_inspected_nodes(
    inner: &ViewTreeInner,
    node_id: NodeId,
    depth: usize,
    out: &mut Vec<InspectedNodeInfo>,
) {
    let Some(node) = inner.node(node_id) else {
        return;
    };
    let next_depth = if node.is_anchor() {
        depth
    } else {
        let pad = node.style.layout.padding;
        out.push(InspectedNodeInfo {
            id: node.id,
            tag: node._tag.to_string(),
            element_id: node.element_id.clone(),
            path_key: node.preview_path_key.clone(),
            depth,
            world_rect: node.world_rect,
            text: node.text.clone(),
            is_interactive: node.role == SemanticRole::Button || node.role.is_focusable(),
            source_line: node.preview_source_span.0,
            source_column: node.preview_source_span.1,
            padding: (pad.top, pad.right, pad.bottom, pad.left),
            tokens: collect_inspected_node_tokens(inner, node_id),
        });
        depth + 1
    };
    for &child_id in &node.children {
        collect_inspected_nodes(inner, child_id, next_depth, out);
    }
}
