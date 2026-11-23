//! `SlotMap` layout node storage and [`TaffyLayoutHost`] adapter for [`crate::LayoutEngine`].

use crate::measure::measure_taffy_leaf;
use crate::TextContext;
use krui_style::{AlignItems, Dimension, DisplayMode, FlexDirection, LayoutStyle, PositionMode};
use krui_stylo_bridge::{StyloNodeId, TaffyLayoutHost};
use slotmap::{DefaultKey, SlotMap};
use taffy::prelude::{AvailableSpace, Size};
use taffy::{Cache, Layout, NodeId, RunMode};

pub(crate) struct LayoutNodeSlot {
    pub(crate) parent: Option<NodeId>,
    pub(crate) children: Vec<NodeId>,
    pub(crate) cache: Cache,
    pub(crate) unrounded: Layout,
    pub(crate) final_layout: Layout,
    pub(crate) text_context: Option<TextContext>,
    pub(crate) stylo_node: Option<StyloNodeId>,
    pub(crate) fallback_style: LayoutStyle,
    pub(crate) override_style: Option<LayoutStyle>,
}

impl LayoutNodeSlot {
    pub(crate) fn new(
        stylo_node: Option<StyloNodeId>,
        fallback_style: LayoutStyle,
        override_style: Option<LayoutStyle>,
        text_context: Option<TextContext>,
    ) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            cache: Cache::new(),
            unrounded: Layout::new(),
            final_layout: Layout::new(),
            text_context,
            stylo_node,
            fallback_style,
            override_style,
        }
    }
}

pub(crate) fn synthetic_root_layout_style(
    viewport_width: f32,
    viewport_height: f32,
) -> LayoutStyle {
    LayoutStyle {
        display: DisplayMode::Flex,
        position: PositionMode::Relative,
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Stretch,
        width: Dimension::Px(viewport_width),
        height: Dimension::Px(viewport_height),
        ..LayoutStyle::default()
    }
}

pub(crate) struct LayoutHost<'a, F> {
    pub(crate) slots: &'a mut SlotMap<DefaultKey, LayoutNodeSlot>,
    pub(crate) measure: &'a mut F,
    pub(crate) orphan_cache: Cache,
    pub(crate) orphan_layout: Layout,
}

impl<F> TaffyLayoutHost for LayoutHost<'_, F>
where
    F: FnMut(&TextContext, Option<f32>) -> (f32, f32),
{
    fn stylo_node_for_layout(&self, node: NodeId) -> Option<StyloNodeId> {
        self.slots.get(node.into()).and_then(|s| s.stylo_node)
    }

    fn layout_style_override(&self, node: NodeId) -> Option<&LayoutStyle> {
        let slot = self.slots.get(node.into())?;
        if let Some(ref ov) = slot.override_style {
            Some(ov)
        } else if slot.stylo_node.is_none() {
            Some(&slot.fallback_style)
        } else {
            None
        }
    }

    fn layout_children(&self, node: NodeId) -> &[NodeId] {
        self.slots
            .get(node.into())
            .map_or(&[], |s| s.children.as_slice())
    }

    fn layout_cache_mut(&mut self, node: NodeId) -> &mut Cache {
        self.slots
            .get_mut(node.into())
            .map_or(&mut self.orphan_cache, |s| &mut s.cache)
    }

    fn unrounded_layout(&self, node: NodeId) -> &Layout {
        self.slots
            .get(node.into())
            .map_or(&self.orphan_layout, |s| &s.unrounded)
    }

    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout) {
        if let Some(slot) = self.slots.get_mut(node.into()) {
            slot.unrounded = *layout;
        }
    }

    fn set_final_layout(&mut self, node: NodeId, layout: &Layout) {
        if let Some(slot) = self.slots.get_mut(node.into()) {
            slot.final_layout = *layout;
        }
    }

    fn measure_leaf(
        &mut self,
        node: NodeId,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        _run_mode: RunMode,
    ) -> Size<f32> {
        let ctx = self
            .slots
            .get(node.into())
            .and_then(|s| s.text_context.as_ref());
        measure_taffy_leaf(known_dimensions, available_space, ctx, self.measure)
    }
}
