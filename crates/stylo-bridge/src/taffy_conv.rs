//! Private zero-copy Taffy 0.15 layout solver adapter over Stylo [`ComputedValues`] and
//! [`krui_style::LayoutStyle`] overrides.
//!
//! Neither Stylo types nor raw `*const ()` pointers appear in the public API:
//! [`TaffyLayoutHost`] exchanges only safe `taffy` 0.15 and `krui_style` types while
//! `TaffyBridgeTree` borrows `&StyloStyleEngine` for the duration of `compute_root_layout`.

use krui_style::LayoutStyle;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use style::properties::style_structs::Font as StyleFont;
use style::properties::ComputedValues;
use style::servo_arc::Arc as StyloArc;
use style::Atom;
use stylo_taffy::{StyleFlags, TaffyStyloStyle};
use taffy::{
    AvailableSpace, AxisStaticEdge, AxisStaticPosition, BlockContext, BoxGenerationMode, Cache,
    CacheTree, CoreStyle, ExpandedLengthPercentage, Layout, LayoutBlockContainer,
    LayoutContainingBlock, LayoutFlexboxContainer, LayoutGridContainer, LayoutInput, LayoutOutput,
    LayoutPartialTree, LengthPercentage, NodeId, OofCandidate, OofPositioningArea, Point,
    RoundTree, RunMode, Size, Style, TraversePartialTree, TraverseTree,
};

use crate::hybrid_style::HybridStyle;
use crate::layout_to_taffy::layout_style_to_taffy;
use crate::{StyloNodeId, StyloStyleEngine};

/// Safe host interface implemented by `krui-layout` / `krui-core` using only `taffy` 0.15 and
/// `krui_style` types.
pub trait TaffyLayoutHost {
    /// Returns the paired [`StyloNodeId`] for `node`, if any.
    fn stylo_node_for_layout(&self, node: NodeId) -> Option<StyloNodeId>;

    /// Optional explicit [`LayoutStyle`] override taking precedence over Stylo (synthetic root,
    /// Tier-2 layout transitions, `LayoutSlot` bindings, standalone non-Stylo nodes).
    fn layout_style_override(&self, _node: NodeId) -> Option<&LayoutStyle> {
        None
    }

    fn layout_children(&self, node: NodeId) -> &[NodeId];
    fn layout_cache_mut(&mut self, node: NodeId) -> &mut Cache;
    fn unrounded_layout(&self, node: NodeId) -> &Layout;
    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout);
    fn set_final_layout(&mut self, node: NodeId, layout: &Layout);
    fn measure_leaf(
        &mut self,
        node: NodeId,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        run_mode: RunMode,
    ) -> Size<f32>;
}

/// Runs Taffy 0.15 layout (Block, Flexbox, Grid, `calc()`, OOF) directly against [`StyloStyleEngine`]
/// with optional per-node [`LayoutStyle`] overrides, without exposing Stylo or raw `calc()` pointers.
pub fn run_taffy_layout(
    engine: &StyloStyleEngine,
    host: &mut impl TaffyLayoutHost,
    root: NodeId,
    available_space: Size<AvailableSpace>,
    use_rounding: bool,
) {
    run_taffy_layout_inner(Some(engine), host, root, available_space, use_rounding);
}

/// Runs standalone Taffy 0.15 layout using only [`TaffyLayoutHost::layout_style_override`] nodes.
pub fn run_taffy_layout_standalone(
    host: &mut impl TaffyLayoutHost,
    root: NodeId,
    available_space: Size<AvailableSpace>,
    use_rounding: bool,
) {
    run_taffy_layout_inner(None, host, root, available_space, use_rounding);
}

fn run_taffy_layout_inner(
    engine: Option<&StyloStyleEngine>,
    host: &mut impl TaffyLayoutHost,
    root: NodeId,
    available_space: Size<AvailableSpace>,
    use_rounding: bool,
) {
    let default_style =
        ComputedValues::initial_values_with_font_override(StyleFont::initial_values());
    let override_styles = collect_override_styles(host, root);
    let mut tree = TaffyBridgeTree {
        engine,
        host,
        default_style,
        default_override: Rc::new(Style::DEFAULT),
        retained_styles: HashMap::new(),
        override_styles,
        hoisted_children: HashMap::new(),
        in_inline_flex_parent: false,
    };
    taffy::compute_root_layout(&mut tree, root, available_space);
    if use_rounding {
        taffy::round_layout(&mut tree, root);
    }
}

fn collect_override_styles(
    host: &impl TaffyLayoutHost,
    root: NodeId,
) -> HashMap<NodeId, Rc<Style<Atom>>> {
    let mut overrides = HashMap::new();
    let mut visited = HashSet::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if !visited.insert(node) {
            continue;
        }
        if let Some(layout_style) = host.layout_style_override(node) {
            overrides.insert(node, Rc::new(layout_style_to_taffy(layout_style)));
        }
        stack.extend_from_slice(host.layout_children(node));
    }
    overrides
}

fn resolve_calc_safe(val: *const (), parent_size: f32) -> f32 {
    if val.is_null() {
        0.0
    } else {
        stylo_taffy::convert::resolve_calc_value(val, parent_size)
    }
}

struct TaffyBridgeTree<'a, H: TaffyLayoutHost> {
    engine: Option<&'a StyloStyleEngine>,
    host: &'a mut H,
    default_style: StyloArc<ComputedValues>,
    default_override: Rc<Style<Atom>>,
    retained_styles: HashMap<NodeId, StyloArc<ComputedValues>>,
    override_styles: HashMap<NodeId, Rc<Style<Atom>>>,
    hoisted_children: HashMap<NodeId, Vec<NodeId>>,
    in_inline_flex_parent: bool,
}

impl<H: TaffyLayoutHost> TaffyBridgeTree<'_, H> {
    fn computed_values_for(&self, node: NodeId) -> Option<StyloArc<ComputedValues>> {
        self.retained_styles.get(&node).cloned().or_else(|| {
            let engine = self.engine?;
            self.host
                .stylo_node_for_layout(node)
                .and_then(|id| engine.computed_values(id))
        })
    }

    fn style_for(&self, node: NodeId) -> HybridStyle {
        if let Some(ov) = self.override_styles.get(&node) {
            return HybridStyle::Override(Rc::clone(ov));
        }
        if let (Some(engine), Some(sid)) = (self.engine, self.host.stylo_node_for_layout(node)) {
            if engine.is_text_node(sid) {
                return HybridStyle::Override(Rc::clone(&self.default_override));
            }
        }
        let computed = self
            .computed_values_for(node)
            .unwrap_or_else(|| StyloArc::clone(&self.default_style));
        HybridStyle::Stylo(TaffyStyloStyle::new(computed, StyleFlags::empty()))
    }

    fn run_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        if inputs.run_mode == RunMode::PerformHiddenLayout {
            self.host.set_final_layout(node_id, &Layout::with_order(0));
            return taffy::compute_hidden_layout(self, node_id);
        }
        if !self.override_styles.contains_key(&node_id) {
            if let Some(engine) = self.engine {
                if let Some(stylo_id) = self.host.stylo_node_for_layout(node_id) {
                    if let Some(cv) = engine.computed_values(stylo_id) {
                        self.retained_styles.entry(node_id).or_insert(cv);
                    }
                }
            }
        }
        taffy::compute_cached_layout(self, node_id, inputs, |tree, id, inputs| {
            let prev_inline_flex = tree.in_inline_flex_parent;
            tree.in_inline_flex_parent = false;
            let mut output = compute_node_layout_output(tree, id, inputs, block_ctx);
            if inputs.run_mode == RunMode::PerformLayout {
                taffy::compute_oof_layout(tree, id, &mut output);
            }
            tree.in_inline_flex_parent = prev_inline_flex;
            output
        })
    }
}

fn compute_node_layout_output<H: TaffyLayoutHost>(
    tree: &mut TaffyBridgeTree<'_, H>,
    id: NodeId,
    inputs: LayoutInput,
    block_ctx: Option<&mut BlockContext<'_>>,
) -> LayoutOutput {
    let child_count = tree.child_count(id);
    let has_in_flow_child = (0..child_count).any(|i| {
        let cs = tree.style_for(tree.get_child_id(id, i));
        cs.box_generation_mode() != BoxGenerationMode::None && !cs.is_out_of_flow_box()
    });
    let style = tree.style_for(id);
    if style.box_generation_mode() == BoxGenerationMode::None {
        return taffy::compute_hidden_layout(tree, id);
    }
    match (style.display_mode(), has_in_flow_child) {
        (taffy::Display::None, _) => taffy::compute_hidden_layout(tree, id),
        (taffy::Display::Block | taffy::Display::FlowRoot, true)
            if all_in_flow_children_inline(tree, id, child_count) =>
        {
            compute_inline_container_as_flex(tree, id, inputs)
        }
        (taffy::Display::Block, true) => taffy::compute_block_layout(tree, id, inputs, block_ctx),
        (taffy::Display::FlowRoot, true) => taffy::compute_block_layout(tree, id, inputs, None),
        (taffy::Display::Flex, true) => taffy::compute_flexbox_layout(tree, id, inputs),
        (taffy::Display::Grid, true) => taffy::compute_grid_layout(tree, id, inputs),
        (_, false) => {
            let run_mode = inputs.run_mode;
            let mut leaf_out =
                taffy::compute_leaf_layout(inputs, &style, resolve_calc_safe, |k, a| {
                    tree.host.measure_leaf(id, k, a, run_mode)
                });
            if child_count > 0 && run_mode == RunMode::PerformLayout {
                populate_leaf_oof_and_hidden_children(
                    tree,
                    id,
                    &style,
                    inputs.parent_size,
                    &mut leaf_out,
                );
            }
            leaf_out
        }
    }
}

fn all_in_flow_children_inline<H: TaffyLayoutHost>(
    tree: &TaffyBridgeTree<'_, H>,
    id: NodeId,
    child_count: usize,
) -> bool {
    (0..child_count).all(|i| {
        let cid = tree.get_child_id(id, i);
        let cs = tree.style_for(cid);
        if cs.box_generation_mode() == BoxGenerationMode::None || cs.is_out_of_flow_box() {
            return true;
        }
        cs.is_inline_outside()
            || tree
                .host
                .stylo_node_for_layout(cid)
                .and_then(|sid| Some(tree.engine?.is_text_node(sid)))
                .unwrap_or(false)
    })
}

fn compute_inline_container_as_flex<H: TaffyLayoutHost>(
    tree: &mut TaffyBridgeTree<'_, H>,
    id: NodeId,
    inputs: LayoutInput,
) -> LayoutOutput {
    let mut ov = if let Some(existing) = tree.override_styles.get(&id) {
        existing.as_ref().clone()
    } else {
        let cv = tree
            .computed_values_for(id)
            .unwrap_or_else(|| StyloArc::clone(&tree.default_style));
        layout_style_to_taffy(&crate::computed_to_layout::extract_layout_style(&cv))
    };
    ov.display = taffy::Display::Flex;
    ov.flex_direction = taffy::FlexDirection::Row;
    ov.flex_wrap = taffy::FlexWrap::Wrap;
    ov.align_items = taffy::AlignItems::FLEX_START;
    let prev = tree.override_styles.insert(id, Rc::new(ov));
    let prev_flag = tree.in_inline_flex_parent;
    tree.in_inline_flex_parent = true;
    let out = taffy::compute_flexbox_layout(tree, id, inputs);
    tree.in_inline_flex_parent = prev_flag;
    match prev {
        Some(p) => {
            tree.override_styles.insert(id, p);
        }
        None => {
            tree.override_styles.remove(&id);
        }
    }
    out
}

fn resolve_lp_or_zero(lp: LengthPercentage, basis: Option<f32>) -> f32 {
    match lp.expand() {
        ExpandedLengthPercentage::Length(v) => v,
        ExpandedLengthPercentage::Percent(p) => basis.map_or(0.0, |b| b * p),
        ExpandedLengthPercentage::Calc(ptr) => basis.map_or(0.0, |b| resolve_calc_safe(ptr, b)),
    }
}

fn populate_leaf_oof_and_hidden_children<H: TaffyLayoutHost>(
    tree: &mut TaffyBridgeTree<'_, H>,
    id: NodeId,
    style: &HybridStyle,
    parent_size: Size<Option<f32>>,
    output: &mut LayoutOutput,
) {
    let brd = style.border();
    let pw = parent_size.width;
    let bl = resolve_lp_or_zero(brd.left, pw);
    let br = resolve_lp_or_zero(brd.right, pw);
    let bt = resolve_lp_or_zero(brd.top, pw);
    let bb = resolve_lp_or_zero(brd.bottom, pw);
    output.oof_positioning_area = Some(OofPositioningArea {
        size: Size {
            width: (output.size.width - bl - br).max(0.0),
            height: (output.size.height - bt - bb).max(0.0),
        },
        offset: Point { x: bl, y: bt },
    });
    let child_count = tree.child_count(id);
    for order in 0..child_count {
        let child = tree.get_child_id(id, order);
        let child_style = tree.style_for(child);
        if child_style.box_generation_mode() == BoxGenerationMode::None {
            tree.host
                .set_unrounded_layout(child, &Layout::with_order(order as u32));
            let _ = taffy::compute_hidden_layout(tree, child);
        } else if child_style.is_out_of_flow_box() {
            output.oof_candidates.push(OofCandidate {
                node: child,
                order: order as u32,
                position: child_style.position(),
                static_position: Point {
                    x: AxisStaticPosition::from_edge(bl, AxisStaticEdge::Start),
                    y: AxisStaticPosition::from_edge(bt, AxisStaticEdge::Start),
                },
            });
        }
    }
}

struct ChildIter<'a>(std::slice::Iter<'a, NodeId>);

impl Iterator for ChildIter<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied()
    }
}

impl<H: TaffyLayoutHost> TraversePartialTree for TaffyBridgeTree<'_, H> {
    type ChildIter<'c>
        = ChildIter<'c>
    where
        Self: 'c;

    fn child_ids(&self, parent_node_id: NodeId) -> Self::ChildIter<'_> {
        ChildIter(self.host.layout_children(parent_node_id).iter())
    }
    fn child_count(&self, parent_node_id: NodeId) -> usize {
        self.host.layout_children(parent_node_id).len()
    }
    fn get_child_id(&self, parent_node_id: NodeId, child_index: usize) -> NodeId {
        self.host.layout_children(parent_node_id)[child_index]
    }
}

impl<H: TaffyLayoutHost> TraverseTree for TaffyBridgeTree<'_, H> {}

impl<H: TaffyLayoutHost> CacheTree for TaffyBridgeTree<'_, H> {
    fn cache_get(&mut self, node_id: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.host.layout_cache_mut(node_id).get(input)
    }
    fn cache_store(&mut self, node_id: NodeId, input: &LayoutInput, layout_output: LayoutOutput) {
        self.host
            .layout_cache_mut(node_id)
            .store(input, layout_output);
    }
    fn cache_clear(&mut self, node_id: NodeId) {
        self.host.layout_cache_mut(node_id).clear();
    }
}

impl<H: TaffyLayoutHost> LayoutPartialTree for TaffyBridgeTree<'_, H> {
    type CoreContainerStyle<'c>
        = HybridStyle
    where
        Self: 'c;
    type CustomIdent = Atom;

    fn get_core_container_style(&self, node_id: NodeId) -> Self::CoreContainerStyle<'_> {
        self.style_for(node_id)
    }
    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.host.set_unrounded_layout(node_id, layout);
    }
    fn resolve_calc_value(&self, val: *const (), basis: f32) -> f32 {
        resolve_calc_safe(val, basis)
    }
    fn compute_child_layout(&mut self, node_id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        self.run_child_layout(node_id, inputs, None)
    }
}

impl<H: TaffyLayoutHost> LayoutContainingBlock for TaffyBridgeTree<'_, H> {
    type OofItemStyle<'c>
        = HybridStyle
    where
        Self: 'c;

    fn get_oof_item_style(&self, node_id: NodeId) -> Self::OofItemStyle<'_> {
        self.style_for(node_id)
    }
    fn clear_hoisted_children(&mut self, node_id: NodeId) {
        self.hoisted_children.remove(&node_id);
    }
    fn add_hoisted_children(&mut self, node_id: NodeId, hoisted: &[NodeId]) {
        self.hoisted_children
            .entry(node_id)
            .or_default()
            .extend_from_slice(hoisted);
    }
}

impl<H: TaffyLayoutHost> LayoutBlockContainer for TaffyBridgeTree<'_, H> {
    type BlockContainerStyle<'c>
        = HybridStyle
    where
        Self: 'c;
    type BlockItemStyle<'c>
        = HybridStyle
    where
        Self: 'c;

    fn get_block_container_style(&self, node_id: NodeId) -> Self::BlockContainerStyle<'_> {
        self.style_for(node_id)
    }
    fn get_block_child_style(&self, child_node_id: NodeId) -> Self::BlockItemStyle<'_> {
        self.style_for(child_node_id)
    }
    fn compute_block_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        self.run_child_layout(node_id, inputs, block_ctx)
    }
}

impl<H: TaffyLayoutHost> LayoutFlexboxContainer for TaffyBridgeTree<'_, H> {
    type FlexboxContainerStyle<'c>
        = HybridStyle
    where
        Self: 'c;
    type FlexboxItemStyle<'c>
        = HybridStyle
    where
        Self: 'c;

    fn get_flexbox_container_style(&self, node_id: NodeId) -> Self::FlexboxContainerStyle<'_> {
        self.style_for(node_id)
    }
    fn get_flexbox_child_style(&self, child_node_id: NodeId) -> Self::FlexboxItemStyle<'_> {
        if self.in_inline_flex_parent {
            let is_text = self
                .host
                .stylo_node_for_layout(child_node_id)
                .and_then(|sid| Some(self.engine?.is_text_node(sid)))
                .unwrap_or(false);
            if !is_text {
                if let Some(ov) = self.override_styles.get(&child_node_id) {
                    if ov.flex_shrink != 0.0 {
                        let mut st = ov.as_ref().clone();
                        st.flex_shrink = 0.0;
                        return HybridStyle::Override(Rc::new(st));
                    }
                } else if let Some(cv) = self.computed_values_for(child_node_id) {
                    if cv.get_position().flex_shrink.0 != 0.0 {
                        let mut st = layout_style_to_taffy(
                            &crate::computed_to_layout::extract_layout_style(&cv),
                        );
                        st.flex_shrink = 0.0;
                        return HybridStyle::Override(Rc::new(st));
                    }
                }
            }
        }
        self.style_for(child_node_id)
    }
}

impl<H: TaffyLayoutHost> LayoutGridContainer for TaffyBridgeTree<'_, H> {
    type GridContainerStyle<'c>
        = HybridStyle
    where
        Self: 'c;
    type GridItemStyle<'c>
        = HybridStyle
    where
        Self: 'c;

    fn get_grid_container_style(&self, node_id: NodeId) -> Self::GridContainerStyle<'_> {
        self.style_for(node_id)
    }
    fn get_grid_child_style(&self, child_node_id: NodeId) -> Self::GridItemStyle<'_> {
        self.style_for(child_node_id)
    }
}

impl<H: TaffyLayoutHost> RoundTree for TaffyBridgeTree<'_, H> {
    fn get_unrounded_layout(&self, node_id: NodeId) -> Layout {
        *self.host.unrounded_layout(node_id)
    }
    fn set_final_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.host.set_final_layout(node_id, layout);
    }
    fn is_out_of_flow(&self, node_id: NodeId) -> bool {
        self.style_for(node_id).is_out_of_flow_box()
    }
    fn hoisted_child_count(&self, node_id: NodeId) -> usize {
        self.hoisted_children.get(&node_id).map_or(0, Vec::len)
    }
    fn get_hoisted_child_id(&self, node_id: NodeId, index: usize) -> NodeId {
        self.hoisted_children[&node_id][index]
    }
}
