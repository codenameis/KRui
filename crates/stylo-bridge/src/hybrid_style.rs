//! Zero-copy Taffy 0.15 style wrapper uniting zero-copy Stylo [`TaffyStyloStyle`] with explicit
//! [`taffy::Style<style::Atom>`] overrides (synthetic root, Tier-2 layout transitions, `LayoutSlot`).

use std::rc::Rc;
use style::Atom;
use taffy::geometry::{Line, Point, Rect, Size};
use taffy::style::{
    AlignContent, AlignItems, AlignSelf, BoxGenerationMode, BoxSizing, Contain,
    ContainingBlockClaims, CoreStyle, Dimension, Direction, Display, FlexDirection, FlexWrap,
    GridAutoFlow, GridContainerStyle, GridItemStyle, GridPlacement, JustifyContent,
    LengthPercentage, LengthPercentageAuto, OofItemStyle, Overflow, Position, TextAlign,
};
use taffy::{BlockContainerStyle, BlockItemStyle, FlexboxContainerStyle, FlexboxItemStyle};

use crate::hybrid_iter::{
    HybridIter, HybridLineNamesIter, HybridRepetition, HybridTemplateTrackList, OverrideStyle,
    StyloStyle,
};

/// Unified zero-copy Taffy style over Stylo `ComputedValues` or an explicit `LayoutStyle` override.
pub(crate) enum HybridStyle {
    Stylo(StyloStyle),
    Override(Rc<OverrideStyle>),
}

impl HybridStyle {
    /// Returns the effective Taffy layout display mode for algorithm dispatch.
    #[inline]
    pub(crate) fn display_mode(&self) -> Display {
        match self {
            Self::Stylo(s) => stylo_taffy::convert::display(s.effective_display()),
            Self::Override(o) => o.display,
        }
    }

    /// Returns whether this box is `position: absolute` or `position: fixed`.
    #[inline]
    pub(crate) fn is_out_of_flow_box(&self) -> bool {
        let pos = self.position();
        pos.is_out_of_flow() && self.box_generation_mode() != BoxGenerationMode::None
    }

    /// Returns `true` when CSS outer display is `inline` (`inline`, `inline-block`, `inline-flex`, `inline-grid`).
    #[inline]
    pub(crate) fn is_inline_outside(&self) -> bool {
        match self {
            Self::Stylo(s) => {
                let d = s.effective_display();
                !d.is_none()
                    && d.outside() == style::values::specified::box_::DisplayOutside::Inline
            }
            Self::Override(_) => false,
        }
    }
}

impl CoreStyle for HybridStyle {
    type CustomIdent = Atom;

    fn box_generation_mode(&self) -> BoxGenerationMode {
        match self {
            Self::Stylo(s) => s.box_generation_mode(),
            Self::Override(o) => CoreStyle::box_generation_mode(o.as_ref()),
        }
    }
    fn is_block(&self) -> bool {
        match self {
            Self::Stylo(s) => s.is_block(),
            Self::Override(o) => CoreStyle::is_block(o.as_ref()),
        }
    }
    fn is_compressible_replaced(&self) -> bool {
        match self {
            Self::Stylo(s) => s.is_compressible_replaced(),
            Self::Override(o) => CoreStyle::is_compressible_replaced(o.as_ref()),
        }
    }
    fn is_replaced(&self) -> bool {
        match self {
            Self::Stylo(s) => s.is_replaced(),
            Self::Override(o) => CoreStyle::is_replaced(o.as_ref()),
        }
    }
    fn box_sizing(&self) -> BoxSizing {
        match self {
            Self::Stylo(s) => s.box_sizing(),
            Self::Override(o) => CoreStyle::box_sizing(o.as_ref()),
        }
    }
    fn direction(&self) -> Direction {
        match self {
            Self::Stylo(s) => s.direction(),
            Self::Override(o) => CoreStyle::direction(o.as_ref()),
        }
    }
    fn overflow(&self) -> Point<Overflow> {
        match self {
            Self::Stylo(s) => s.overflow(),
            Self::Override(o) => CoreStyle::overflow(o.as_ref()),
        }
    }
    fn scrollbar_width(&self) -> f32 {
        match self {
            Self::Stylo(s) => s.scrollbar_width(),
            Self::Override(o) => CoreStyle::scrollbar_width(o.as_ref()),
        }
    }
    fn contain(&self) -> Contain {
        match self {
            Self::Stylo(s) => s.contain(),
            Self::Override(o) => CoreStyle::contain(o.as_ref()),
        }
    }
    fn position(&self) -> Position {
        match self {
            Self::Stylo(s) => s.position(),
            Self::Override(o) => CoreStyle::position(o.as_ref()),
        }
    }
    fn is_containing_block(&self) -> ContainingBlockClaims {
        match self {
            Self::Stylo(s) => s.is_containing_block(),
            Self::Override(o) => CoreStyle::is_containing_block(o.as_ref()),
        }
    }
    fn inset(&self) -> Rect<LengthPercentageAuto> {
        match self {
            Self::Stylo(s) => s.inset(),
            Self::Override(o) => CoreStyle::inset(o.as_ref()),
        }
    }
    fn size(&self) -> Size<Dimension> {
        match self {
            Self::Stylo(s) => s.size(),
            Self::Override(o) => CoreStyle::size(o.as_ref()),
        }
    }
    fn min_size(&self) -> Size<LengthPercentageAuto> {
        match self {
            Self::Stylo(s) => s.min_size(),
            Self::Override(o) => CoreStyle::min_size(o.as_ref()),
        }
    }
    fn max_size(&self) -> Size<LengthPercentageAuto> {
        match self {
            Self::Stylo(s) => s.max_size(),
            Self::Override(o) => CoreStyle::max_size(o.as_ref()),
        }
    }
    fn aspect_ratio(&self) -> Option<f32> {
        match self {
            Self::Stylo(s) => s.aspect_ratio(),
            Self::Override(o) => CoreStyle::aspect_ratio(o.as_ref()),
        }
    }
    fn margin(&self) -> Rect<LengthPercentageAuto> {
        match self {
            Self::Stylo(s) => s.margin(),
            Self::Override(o) => CoreStyle::margin(o.as_ref()),
        }
    }
    fn padding(&self) -> Rect<LengthPercentage> {
        match self {
            Self::Stylo(s) => s.padding(),
            Self::Override(o) => CoreStyle::padding(o.as_ref()),
        }
    }
    fn border(&self) -> Rect<LengthPercentage> {
        match self {
            Self::Stylo(s) => s.border(),
            Self::Override(o) => CoreStyle::border(o.as_ref()),
        }
    }
}

impl BlockContainerStyle for HybridStyle {
    fn text_align(&self) -> TextAlign {
        match self {
            Self::Stylo(s) => s.text_align(),
            Self::Override(o) => BlockContainerStyle::text_align(o.as_ref()),
        }
    }
    fn align_content(&self) -> AlignContent {
        match self {
            Self::Stylo(s) => BlockContainerStyle::align_content(s),
            Self::Override(o) => BlockContainerStyle::align_content(o.as_ref()),
        }
    }
    fn justify_items(&self) -> AlignItems {
        match self {
            Self::Stylo(s) => BlockContainerStyle::justify_items(s),
            Self::Override(o) => BlockContainerStyle::justify_items(o.as_ref()),
        }
    }
}

impl BlockItemStyle for HybridStyle {
    fn is_table(&self) -> bool {
        match self {
            Self::Stylo(s) => BlockItemStyle::is_table(s),
            Self::Override(o) => BlockItemStyle::is_table(o.as_ref()),
        }
    }
    fn align_self(&self) -> Option<AlignSelf> {
        match self {
            Self::Stylo(s) => BlockItemStyle::align_self(s),
            Self::Override(o) => BlockItemStyle::align_self(o.as_ref()),
        }
    }
    fn justify_self(&self) -> Option<AlignSelf> {
        match self {
            Self::Stylo(s) => BlockItemStyle::justify_self(s),
            Self::Override(o) => BlockItemStyle::justify_self(o.as_ref()),
        }
    }
    fn align_content(&self) -> AlignContent {
        match self {
            Self::Stylo(s) => BlockItemStyle::align_content(s),
            Self::Override(o) => BlockItemStyle::align_content(o.as_ref()),
        }
    }
}

impl FlexboxContainerStyle for HybridStyle {
    fn flex_direction(&self) -> FlexDirection {
        match self {
            Self::Stylo(s) => s.flex_direction(),
            Self::Override(o) => FlexboxContainerStyle::flex_direction(o.as_ref()),
        }
    }
    fn flex_wrap(&self) -> FlexWrap {
        match self {
            Self::Stylo(s) => s.flex_wrap(),
            Self::Override(o) => FlexboxContainerStyle::flex_wrap(o.as_ref()),
        }
    }
    fn flex_line_count(&self) -> u16 {
        match self {
            Self::Stylo(s) => s.flex_line_count(),
            Self::Override(o) => FlexboxContainerStyle::flex_line_count(o.as_ref()),
        }
    }
    fn gap(&self) -> Size<LengthPercentage> {
        match self {
            Self::Stylo(s) => FlexboxContainerStyle::gap(s),
            Self::Override(o) => FlexboxContainerStyle::gap(o.as_ref()),
        }
    }
    fn align_content(&self) -> AlignContent {
        match self {
            Self::Stylo(s) => FlexboxContainerStyle::align_content(s),
            Self::Override(o) => FlexboxContainerStyle::align_content(o.as_ref()),
        }
    }
    fn align_items(&self) -> AlignItems {
        match self {
            Self::Stylo(s) => FlexboxContainerStyle::align_items(s),
            Self::Override(o) => FlexboxContainerStyle::align_items(o.as_ref()),
        }
    }
    fn justify_content(&self) -> JustifyContent {
        match self {
            Self::Stylo(s) => FlexboxContainerStyle::justify_content(s),
            Self::Override(o) => FlexboxContainerStyle::justify_content(o.as_ref()),
        }
    }
}

impl FlexboxItemStyle for HybridStyle {
    fn flex_basis(&self) -> Dimension {
        match self {
            Self::Stylo(s) => s.flex_basis(),
            Self::Override(o) => FlexboxItemStyle::flex_basis(o.as_ref()),
        }
    }
    fn flex_grow(&self) -> f32 {
        match self {
            Self::Stylo(s) => s.flex_grow(),
            Self::Override(o) => FlexboxItemStyle::flex_grow(o.as_ref()),
        }
    }
    fn flex_shrink(&self) -> f32 {
        match self {
            Self::Stylo(s) => s.flex_shrink(),
            Self::Override(o) => FlexboxItemStyle::flex_shrink(o.as_ref()),
        }
    }
    fn align_self(&self) -> Option<AlignSelf> {
        match self {
            Self::Stylo(s) => FlexboxItemStyle::align_self(s),
            Self::Override(o) => FlexboxItemStyle::align_self(o.as_ref()),
        }
    }
}

impl GridContainerStyle for HybridStyle {
    type Repetition<'a>
        = HybridRepetition<'a>
    where
        Self: 'a;
    type TemplateTrackList<'a>
        = HybridTemplateTrackList<'a>
    where
        Self: 'a;
    type AutoTrackList<'a>
        = HybridIter<
        <StyloStyle as GridContainerStyle>::AutoTrackList<'a>,
        <OverrideStyle as GridContainerStyle>::AutoTrackList<'a>,
    >
    where
        Self: 'a;
    type TemplateLineNames<'a>
        = HybridLineNamesIter<'a>
    where
        Self: 'a;
    type GridTemplateAreas<'a>
        = HybridIter<
        <StyloStyle as GridContainerStyle>::GridTemplateAreas<'a>,
        <OverrideStyle as GridContainerStyle>::GridTemplateAreas<'a>,
    >
    where
        Self: 'a;

    fn grid_template_rows(&self) -> Option<Self::TemplateTrackList<'_>> {
        match self {
            Self::Stylo(s) => s.grid_template_rows().map(HybridTemplateTrackList::Stylo),
            Self::Override(o) => o
                .grid_template_rows()
                .map(HybridTemplateTrackList::Override),
        }
    }
    fn grid_template_columns(&self) -> Option<Self::TemplateTrackList<'_>> {
        match self {
            Self::Stylo(s) => s
                .grid_template_columns()
                .map(HybridTemplateTrackList::Stylo),
            Self::Override(o) => o
                .grid_template_columns()
                .map(HybridTemplateTrackList::Override),
        }
    }
    fn grid_auto_rows(&self) -> Self::AutoTrackList<'_> {
        match self {
            Self::Stylo(s) => HybridIter::Left(s.grid_auto_rows()),
            Self::Override(o) => HybridIter::Right(o.grid_auto_rows()),
        }
    }
    fn grid_auto_columns(&self) -> Self::AutoTrackList<'_> {
        match self {
            Self::Stylo(s) => HybridIter::Left(s.grid_auto_columns()),
            Self::Override(o) => HybridIter::Right(o.grid_auto_columns()),
        }
    }
    fn grid_template_areas(&self) -> Option<Self::GridTemplateAreas<'_>> {
        match self {
            Self::Stylo(s) => s.grid_template_areas().map(HybridIter::Left),
            Self::Override(o) => o.grid_template_areas().map(HybridIter::Right),
        }
    }
    fn grid_template_area_row_count(&self) -> u16 {
        match self {
            Self::Stylo(s) => s.grid_template_area_row_count(),
            Self::Override(o) => o.grid_template_area_row_count(),
        }
    }
    fn grid_template_area_column_count(&self) -> u16 {
        match self {
            Self::Stylo(s) => s.grid_template_area_column_count(),
            Self::Override(o) => o.grid_template_area_column_count(),
        }
    }
    fn grid_template_column_names(&self) -> Option<Self::TemplateLineNames<'_>> {
        match self {
            Self::Stylo(s) => s
                .grid_template_column_names()
                .map(HybridLineNamesIter::Stylo),
            Self::Override(o) => o
                .grid_template_column_names()
                .map(HybridLineNamesIter::Override),
        }
    }
    fn grid_template_row_names(&self) -> Option<Self::TemplateLineNames<'_>> {
        match self {
            Self::Stylo(s) => s.grid_template_row_names().map(HybridLineNamesIter::Stylo),
            Self::Override(o) => o
                .grid_template_row_names()
                .map(HybridLineNamesIter::Override),
        }
    }
    fn grid_auto_flow(&self) -> GridAutoFlow {
        match self {
            Self::Stylo(s) => s.grid_auto_flow(),
            Self::Override(o) => GridContainerStyle::grid_auto_flow(o.as_ref()),
        }
    }
    fn gap(&self) -> Size<LengthPercentage> {
        match self {
            Self::Stylo(s) => GridContainerStyle::gap(s),
            Self::Override(o) => GridContainerStyle::gap(o.as_ref()),
        }
    }
    fn align_content(&self) -> AlignContent {
        match self {
            Self::Stylo(s) => GridContainerStyle::align_content(s),
            Self::Override(o) => GridContainerStyle::align_content(o.as_ref()),
        }
    }
    fn justify_content(&self) -> JustifyContent {
        match self {
            Self::Stylo(s) => GridContainerStyle::justify_content(s),
            Self::Override(o) => GridContainerStyle::justify_content(o.as_ref()),
        }
    }
    fn align_items(&self) -> AlignItems {
        match self {
            Self::Stylo(s) => GridContainerStyle::align_items(s),
            Self::Override(o) => GridContainerStyle::align_items(o.as_ref()),
        }
    }
    fn justify_items(&self) -> AlignItems {
        match self {
            Self::Stylo(s) => GridContainerStyle::justify_items(s),
            Self::Override(o) => GridContainerStyle::justify_items(o.as_ref()),
        }
    }
}

impl GridItemStyle for HybridStyle {
    fn grid_row(&self) -> Line<GridPlacement<Atom>> {
        match self {
            Self::Stylo(s) => GridItemStyle::grid_row(s),
            Self::Override(o) => GridItemStyle::grid_row(o.as_ref()),
        }
    }
    fn grid_column(&self) -> Line<GridPlacement<Atom>> {
        match self {
            Self::Stylo(s) => GridItemStyle::grid_column(s),
            Self::Override(o) => GridItemStyle::grid_column(o.as_ref()),
        }
    }
    fn align_self(&self) -> Option<AlignSelf> {
        match self {
            Self::Stylo(s) => GridItemStyle::align_self(s),
            Self::Override(o) => GridItemStyle::align_self(o.as_ref()),
        }
    }
    fn justify_self(&self) -> Option<AlignSelf> {
        match self {
            Self::Stylo(s) => GridItemStyle::justify_self(s),
            Self::Override(o) => GridItemStyle::justify_self(o.as_ref()),
        }
    }
}

impl OofItemStyle for HybridStyle {
    fn align_self(&self) -> Option<AlignSelf> {
        match self {
            Self::Stylo(s) => OofItemStyle::align_self(s),
            Self::Override(o) => OofItemStyle::align_self(o.as_ref()),
        }
    }
    fn justify_self(&self) -> Option<AlignSelf> {
        match self {
            Self::Stylo(s) => OofItemStyle::justify_self(s),
            Self::Override(o) => OofItemStyle::justify_self(o.as_ref()),
        }
    }
    fn is_table(&self) -> bool {
        match self {
            Self::Stylo(s) => OofItemStyle::is_table(s),
            Self::Override(o) => OofItemStyle::is_table(o.as_ref()),
        }
    }
    fn grid_row(&self) -> Line<GridPlacement<Atom>> {
        match self {
            Self::Stylo(s) => OofItemStyle::grid_row(s),
            Self::Override(o) => OofItemStyle::grid_row(o.as_ref()),
        }
    }
    fn grid_column(&self) -> Line<GridPlacement<Atom>> {
        match self {
            Self::Stylo(s) => OofItemStyle::grid_column(s),
            Self::Override(o) => OofItemStyle::grid_column(o.as_ref()),
        }
    }
}
