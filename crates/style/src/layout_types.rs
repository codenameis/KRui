//! Box-model, Flexbox, and CSS Grid layout style types (`LayoutStyle`, `Dimension`, `Edges`, `InsetEdges`).

use crate::box_edges::ScrollbarWidth;
use crate::grid::{GridAutoFlow, GridLinePair, GridTrackList};

/// CSS length or sizing unit (`auto`, logical pixels, or container percentage).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum Dimension {
    /// Intrinsic / content-sized dimension (`auto`).
    #[default]
    Auto,
    /// Explicit logical pixel length.
    Px(f32),
    /// Percentage (`0.0..=100.0`) of parent dimension.
    Percent(f32),
}

/// Box edges (`top`, `right`, `bottom`, `left`) in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct Edges {
    /// Top edge in logical pixels.
    pub top: f32,
    /// Right edge in logical pixels.
    pub right: f32,
    /// Bottom edge in logical pixels.
    pub bottom: f32,
    /// Left edge in logical pixels.
    pub left: f32,
}

impl Edges {
    /// Zero-width edges on all four sides.
    pub const ZERO: Self = Self::all(0.0);

    /// Uniform edge thickness `v` on all four sides.
    pub const fn all(v: f32) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }

    /// Symmetric vertical (`top`/`bottom`) and horizontal (`left`/`right`) edges.
    pub const fn symmetric(vertical: f32, horizontal: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }
}

/// Four individual corner radii `[top_left, top_right, bottom_right, bottom_left]` in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CornerRadii {
    /// Top-left corner radius in logical pixels.
    pub top_left: f32,
    /// Top-right corner radius in logical pixels.
    pub top_right: f32,
    /// Bottom-right corner radius in logical pixels.
    pub bottom_right: f32,
    /// Bottom-left corner radius in logical pixels.
    pub bottom_left: f32,
}

impl CornerRadii {
    /// All four corners set to zero radius.
    pub const ZERO: Self = Self {
        top_left: 0.0,
        top_right: 0.0,
        bottom_right: 0.0,
        bottom_left: 0.0,
    };

    /// Creates uniform corner radii across all four corners.
    #[inline]
    pub const fn all(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    /// Returns the largest corner radius among all four corners.
    #[inline]
    pub fn max_radius(self) -> f32 {
        self.top_left
            .max(self.top_right)
            .max(self.bottom_right)
            .max(self.bottom_left)
    }

    /// Returns corners packed as `[top_left, top_right, bottom_right, bottom_left]`.
    #[inline]
    pub const fn to_array(self) -> [f32; 4] {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
    }
}

/// CSS `display` layout mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum DisplayMode {
    /// Flexbox container (`display: flex`).
    #[default]
    Flex,
    /// Inline-level Flexbox container (`display: inline-flex`).
    InlineFlex,
    /// CSS Grid container (`display: grid`).
    Grid,
    /// Inline-level CSS Grid container (`display: inline-grid`).
    InlineGrid,
    /// Block container (`display: block`).
    Block,
    /// Inline flow element (`display: inline`).
    Inline,
    /// Inline-block flow item (`display: inline-block`).
    InlineBlock,
    /// Box-less container whose children participate directly in its parent layout (`display: contents`).
    Contents,
    /// Excluded from layout and hit-testing (`display: none`).
    None,
}

impl DisplayMode {
    /// Returns `true` for Flexbox containers (`flex` or `inline-flex`).
    #[inline]
    pub const fn is_flex(self) -> bool {
        matches!(self, Self::Flex | Self::InlineFlex)
    }

    /// Returns `true` for CSS Grid containers (`grid` or `inline-grid`).
    #[inline]
    pub const fn is_grid(self) -> bool {
        matches!(self, Self::Grid | Self::InlineGrid)
    }

    /// Blockifies inline-level display modes per CSS Display 3 §2.7 / Flexbox §4 / Grid §6.1 / CSS 2.1 §9.7.
    #[inline]
    pub const fn blockified(self) -> Self {
        match self {
            Self::Inline | Self::InlineBlock => Self::Block,
            Self::InlineFlex => Self::Flex,
            Self::InlineGrid => Self::Grid,
            other => other,
        }
    }
}

/// CSS `flex-direction` main axis orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum FlexDirection {
    /// Horizontal main axis (`flex-direction: row`).
    #[default]
    Row,
    /// Vertical main axis (`flex-direction: column`).
    Column,
}

/// CSS `justify-content` main-axis distribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum JustifyContent {
    /// Pack items toward main-axis start.
    #[default]
    FlexStart,
    /// Pack items around main-axis center.
    Center,
    /// Pack items toward main-axis end.
    FlexEnd,
    /// Distribute items evenly with outer items flush against edges.
    SpaceBetween,
    /// Distribute items with equal half-size space on outer ends.
    SpaceAround,
    /// Distribute items with equal space between every pair and outer ends.
    SpaceEvenly,
}

/// CSS `align-items` cross-axis alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum AlignItems {
    /// Stretch items to fill cross-axis container size.
    #[default]
    Stretch,
    /// Align items at cross-axis start.
    FlexStart,
    /// Align items at cross-axis center.
    Center,
    /// Align items at cross-axis end.
    FlexEnd,
    /// Align items along their text baseline.
    Baseline,
}

/// CSS `box-sizing` sizing box model (`border-box` vs `content-box`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum BoxSizing {
    /// Specified `width`/`height` includes padding and border (`border-box`).
    #[default]
    BorderBox,
    /// Specified `width`/`height` applies only to the content box (`content-box`).
    ContentBox,
}

/// CSS `position` scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum PositionMode {
    /// Unpositioned normal-flow box (`position: static`).
    #[default]
    Static,
    /// Positioned normal-flow box (`position: relative`).
    Relative,
    /// Out-of-flow inset-positioned box (`position: absolute`).
    Absolute,
    /// Viewport-anchored out-of-flow box (`position: fixed`).
    Fixed,
    /// Scroll-clamped sticky flow box (`position: sticky`).
    Sticky,
}

impl PositionMode {
    /// Returns `true` when this node establishes a CSS positioned box (`position != static`).
    #[inline]
    pub const fn is_positioned(self) -> bool {
        !matches!(self, Self::Static)
    }

    /// Returns `true` when this box is removed from normal flow (`absolute` or `fixed`).
    #[inline]
    pub const fn is_out_of_flow(self) -> bool {
        matches!(self, Self::Absolute | Self::Fixed)
    }
}

/// CSS `overflow` clipping and scroll mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum OverflowMode {
    /// Content overflows unclipped (`overflow: visible`).
    #[default]
    Visible,
    /// Content clipped to box bounds (`overflow: hidden`).
    Hidden,
    /// Scrollable overflow container (`overflow: scroll | auto`).
    Scroll,
}

/// Box inset edges (`top`, `right`, `bottom`, `left`) defaulting to `Dimension::Auto`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct InsetEdges {
    /// Top inset offset.
    pub top: Dimension,
    /// Right inset offset.
    pub right: Dimension,
    /// Bottom inset offset.
    pub bottom: Dimension,
    /// Left inset offset.
    pub left: Dimension,
}

impl InsetEdges {
    /// All four inset edges set to `Dimension::Auto`.
    pub const AUTO: Self = Self {
        top: Dimension::Auto,
        right: Dimension::Auto,
        bottom: Dimension::Auto,
        left: Dimension::Auto,
    };
}

/// Layout-triggering style properties mapped directly to Taffy's layout tree.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct LayoutStyle {
    /// Box `display` mode.
    pub display: DisplayMode,
    /// Box `position` mode.
    pub position: PositionMode,
    /// Box `overflow` mode (shorthand fallback for horizontal and vertical axes).
    pub overflow: OverflowMode,
    /// Optional explicit horizontal overflow (`overflow-x`).
    pub overflow_x: Option<OverflowMode>,
    /// Optional explicit vertical overflow (`overflow-y`).
    pub overflow_y: Option<OverflowMode>,
    /// Flexbox main axis.
    pub flex_direction: FlexDirection,
    /// Whether flex items wrap onto multiple lines (`flex-wrap: wrap`).
    pub flex_wrap: bool,
    /// Flexbox main-axis alignment.
    pub justify_content: JustifyContent,
    /// Flexbox cross-axis alignment.
    pub align_items: AlignItems,
    /// Preferred width.
    pub width: Dimension,
    /// Preferred height.
    pub height: Dimension,
    /// Minimum width constraint.
    pub min_width: Dimension,
    /// Minimum height constraint.
    pub min_height: Dimension,
    /// Maximum width constraint.
    pub max_width: Dimension,
    /// Maximum height constraint.
    pub max_height: Dimension,
    /// Outer margin edges.
    pub margin: Edges,
    /// Inner padding edges.
    pub padding: Edges,
    /// Uniform border thickness in logical pixels (authored value before `border_style_mask`).
    pub border_width: f32,
    /// Optional per-side border thicknesses (`top`, `right`, `bottom`, `left`).
    pub border_edges: Option<Edges>,
    /// Per-side W3C `border-*-style` active mask (`bit 0=top, 1=right, 2=bottom, 3=left`; initial `0` = `none`).
    pub border_style_mask: u8,
    /// Uniform row/column gap in logical pixels.
    pub gap: f32,
    /// Optional explicit vertical gap (`row-gap`).
    pub row_gap: Option<f32>,
    /// Optional explicit horizontal gap (`column-gap`).
    pub column_gap: Option<f32>,
    /// Optional box `aspect-ratio` (`width / height`).
    pub aspect_ratio: Option<f32>,
    /// Flex grow factor.
    pub flex_grow: f32,
    /// Flex shrink factor.
    pub flex_shrink: f32,
    /// Initial main size of a flex item (`flex-basis`).
    pub flex_basis: Dimension,
    /// Cross-axis self alignment override (`align-self`).
    pub align_self: Option<AlignItems>,
    /// Default inline-axis alignment for Grid children (`justify-items`).
    pub justify_items: Option<AlignItems>,
    /// Inline-axis self alignment override for Grid items (`justify-self`).
    pub justify_self: Option<AlignItems>,
    /// CSS `box-sizing` model (`BorderBox` or `ContentBox`).
    pub box_sizing: BoxSizing,
    /// Optional non-pixel margin edges (`auto` or `%`); `None` when pure px in `margin`.
    pub margin_dims: Option<InsetEdges>,
    /// Optional non-pixel padding edges (`%`); `None` when pure px in `padding`.
    pub padding_dims: Option<InsetEdges>,
    /// Out-of-flow inset offsets.
    pub inset: InsetEdges,
    /// Fallback column count for `repeat(N, 1fr)` when `grid_template_columns` is empty.
    pub grid_columns: u16,
    /// Explicit CSS Grid column tracks (`grid-template-columns`).
    pub grid_template_columns: GridTrackList,
    /// Explicit CSS Grid row tracks (`grid-template-rows`).
    pub grid_template_rows: GridTrackList,
    /// Auto-placement flow mode (`grid-auto-flow`).
    pub grid_auto_flow: GridAutoFlow,
    /// Explicit Grid column line/span placement (`grid-column`).
    pub grid_column: GridLinePair,
    /// Explicit Grid row line/span placement (`grid-row`).
    pub grid_row: GridLinePair,
    /// Stacking order index for sibling overlap resolution (`z-index`).
    pub z_index: i32,
    /// Whether an integer `z-index` (rather than `auto`) was explicitly declared.
    pub has_z_index: bool,
    /// Whether `display: flex | inline-flex` was explicitly declared in CSS.
    pub display_explicit_flex: bool,
    /// Whether `flex-direction` was explicitly declared in CSS.
    pub flex_direction_explicit: bool,
    /// Whether `padding` (including `padding: 0`) was explicitly declared in CSS.
    pub padding_explicit: bool,
    /// CSS Scrollbars Level 1 `scrollbar-width` (`auto | thin | none`).
    pub scrollbar_width: ScrollbarWidth,
}

impl LayoutStyle {
    /// Resolves `flex-direction` so explicit `display: flex | inline-flex` defaults to `Row`
    /// while preserving any explicit `flex-direction` regardless of CSS declaration/class order.
    #[inline]
    pub const fn effective_flex_direction(&self) -> FlexDirection {
        if self.flex_direction_explicit {
            self.flex_direction
        } else if self.display_explicit_flex && self.display.is_flex() {
            FlexDirection::Row
        } else {
            self.flex_direction
        }
    }

    /// Returns effective horizontal overflow mode (`overflow-x` overriding shorthand `overflow`).
    #[inline]
    pub const fn effective_overflow_x(&self) -> OverflowMode {
        match self.overflow_x {
            Some(v) => v,
            None => self.overflow,
        }
    }

    /// Returns effective vertical overflow mode (`overflow-y` overriding shorthand `overflow`).
    #[inline]
    pub const fn effective_overflow_y(&self) -> OverflowMode {
        match self.overflow_y {
            Some(v) => v,
            None => self.overflow,
        }
    }

    /// Returns `true` when computed `overflow-x` or `overflow-y` establishes a scroll container.
    #[inline]
    pub const fn is_scroll_container(&self) -> bool {
        matches!(self.effective_overflow_x(), OverflowMode::Scroll)
            || matches!(self.effective_overflow_y(), OverflowMode::Scroll)
    }

    /// Returns `true` when computed `overflow-x` or `overflow-y` clips descendants (`hidden` or `scroll`).
    #[inline]
    pub const fn clips_overflow(&self) -> bool {
        !matches!(self.effective_overflow_x(), OverflowMode::Visible)
            || !matches!(self.effective_overflow_y(), OverflowMode::Visible)
    }

    /// Returns authored unmasked per-side border widths (`border_edges` overriding uniform `border_width`).
    #[inline]
    pub const fn raw_border_edges(&self) -> Edges {
        match self.border_edges {
            Some(edges) => edges,
            None => Edges::all(self.border_width),
        }
    }

    /// Returns computed per-side border widths (`0` on any side whose `border-*-style` is `none`/`hidden`).
    #[inline]
    pub const fn effective_border_edges(&self) -> Edges {
        let raw = self.raw_border_edges();
        let m = self.border_style_mask;
        Edges {
            top: if (m & 1) != 0 { raw.top } else { 0.0 },
            right: if (m & 2) != 0 { raw.right } else { 0.0 },
            bottom: if (m & 4) != 0 { raw.bottom } else { 0.0 },
            left: if (m & 8) != 0 { raw.left } else { 0.0 },
        }
    }

    /// Returns effective vertical (`row-gap`) spacing in logical pixels.
    #[inline]
    pub const fn effective_row_gap(&self) -> f32 {
        match self.row_gap {
            Some(v) => v,
            None => self.gap,
        }
    }

    /// Returns effective horizontal (`column-gap`) spacing in logical pixels.
    #[inline]
    pub const fn effective_column_gap(&self) -> f32 {
        match self.column_gap {
            Some(v) => v,
            None => self.gap,
        }
    }

    /// Returns `true` when this box establishes a CSS Flex or CSS Grid formatting context
    /// whose in-flow children undergo blockification (CSS Flexbox §4, CSS Grid §6.1).
    #[inline]
    pub const fn is_flex_or_grid_container(&self) -> bool {
        matches!(
            self.display,
            DisplayMode::Grid | DisplayMode::InlineGrid | DisplayMode::InlineFlex
        ) || (matches!(self.display, DisplayMode::Flex) && self.display_explicit_flex)
    }

    /// Blockifies `self.display` in place while preserving explicit flex container semantics
    /// when `inline-flex` becomes `flex`.
    #[inline]
    pub fn blockify_in_place(&mut self) {
        let next = self.display.blockified();
        if next != self.display {
            if self.display == DisplayMode::InlineFlex {
                self.display_explicit_flex = true;
            }
            self.display = next;
        }
    }
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            display: DisplayMode::Flex,
            position: PositionMode::Static,
            overflow: OverflowMode::Visible,
            overflow_x: None,
            overflow_y: None,
            flex_direction: FlexDirection::Column,
            flex_wrap: false,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Stretch,
            width: Dimension::Auto,
            height: Dimension::Auto,
            min_width: Dimension::Auto,
            min_height: Dimension::Auto,
            max_width: Dimension::Auto,
            max_height: Dimension::Auto,
            margin: Edges::ZERO,
            padding: Edges::ZERO,
            border_width: 0.0,
            border_edges: None,
            border_style_mask: 0,
            gap: 0.0,
            row_gap: None,
            column_gap: None,
            aspect_ratio: None,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Dimension::Auto,
            align_self: None,
            justify_items: None,
            justify_self: None,
            box_sizing: BoxSizing::BorderBox,
            margin_dims: None,
            padding_dims: None,
            inset: InsetEdges::AUTO,
            grid_columns: 0,
            grid_template_columns: GridTrackList::EMPTY,
            grid_template_rows: GridTrackList::EMPTY,
            grid_auto_flow: GridAutoFlow::Row,
            grid_column: GridLinePair::AUTO,
            grid_row: GridLinePair::AUTO,
            z_index: 0,
            has_z_index: false,
            display_explicit_flex: false,
            flex_direction_explicit: false,
            padding_explicit: false,
            scrollbar_width: ScrollbarWidth::Auto,
        }
    }
}
