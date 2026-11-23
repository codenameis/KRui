//! Tier-2 layout property masks and interpolation helpers (`width`, `height`, `padding`,
//! `margin`, `gap`, `inset`, `border-width`, `flex*`, `font-size`).

use crate::layout_types::{Dimension, Edges, InsetEdges, LayoutStyle};
use crate::transition_lerp::lerp_f32;

/// Bitmask of Taffy layout and typography properties eligible for Tier-2 transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct LayoutTransitionMask(pub u16);

impl LayoutTransitionMask {
    /// No layout properties transitioned.
    pub const NONE: Self = Self(0);
    pub const WIDTH: u16 = 1 << 0;
    pub const HEIGHT: u16 = 1 << 1;
    pub const MIN_WIDTH: u16 = 1 << 2;
    pub const MIN_HEIGHT: u16 = 1 << 3;
    pub const MAX_WIDTH: u16 = 1 << 4;
    pub const MAX_HEIGHT: u16 = 1 << 5;
    pub const PADDING: u16 = 1 << 6;
    pub const MARGIN: u16 = 1 << 7;
    pub const GAP: u16 = 1 << 8;
    pub const INSET: u16 = 1 << 9;
    pub const BORDER_WIDTH: u16 = 1 << 10;
    pub const FLEX: u16 = 1 << 11;
    pub const FONT_SIZE: u16 = 1 << 12;
    /// All Tier-2 layout & typography transition bits (`all`).
    pub const ALL: Self = Self(0x1FFF);

    /// Returns `true` if `bit` is enabled in this mask.
    #[inline]
    pub const fn contains(self, bit: u16) -> bool {
        (self.0 & bit) != 0
    }

    /// Combines two masks via bitwise OR.
    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns `true` when at least one Tier-2 layout bit is set.
    #[inline]
    pub const fn is_any(self) -> bool {
        self.0 != 0
    }
}

/// Maps a lowercase CSS property name onto a [`LayoutTransitionMask`] bitmask, if supported on Tier-2.
pub fn layout_property_bit(prop: &str) -> Option<u16> {
    Some(match prop {
        "none" => 0,
        "all" => LayoutTransitionMask::ALL.0,
        "width" => LayoutTransitionMask::WIDTH,
        "height" => LayoutTransitionMask::HEIGHT,
        "min-width" => LayoutTransitionMask::MIN_WIDTH,
        "min-height" => LayoutTransitionMask::MIN_HEIGHT,
        "max-width" => LayoutTransitionMask::MAX_WIDTH,
        "max-height" => LayoutTransitionMask::MAX_HEIGHT,
        "padding" | "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
            LayoutTransitionMask::PADDING
        }
        "margin" | "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
            LayoutTransitionMask::MARGIN
        }
        "gap" | "row-gap" | "column-gap" => LayoutTransitionMask::GAP,
        "top" | "right" | "bottom" | "left" | "inset" => LayoutTransitionMask::INSET,
        "border-width"
        | "border-top-width"
        | "border-right-width"
        | "border-bottom-width"
        | "border-left-width" => LayoutTransitionMask::BORDER_WIDTH,
        "flex" | "flex-grow" | "flex-shrink" | "flex-basis" => LayoutTransitionMask::FLEX,
        "font-size" => LayoutTransitionMask::FONT_SIZE,
        _ => return None,
    })
}

fn dim_differs(a: Dimension, b: Dimension) -> bool {
    match (a, b) {
        (Dimension::Px(x), Dimension::Px(y)) | (Dimension::Percent(x), Dimension::Percent(y)) => {
            (x - y).abs() > 1e-3
        }
        _ => a != b,
    }
}

fn opt_dim_differs(a: Option<Edges>, b: Option<Edges>) -> bool {
    a != b
}

/// Returns `true` if `from` and `to` differ on any Tier-2 property included in `mask`.
pub fn layout_differs_on_mask(
    (from, from_fs): (&LayoutStyle, f32),
    (to, to_fs): (&LayoutStyle, f32),
    mask: LayoutTransitionMask,
) -> bool {
    (mask.contains(LayoutTransitionMask::WIDTH) && dim_differs(from.width, to.width))
        || (mask.contains(LayoutTransitionMask::HEIGHT) && dim_differs(from.height, to.height))
        || (mask.contains(LayoutTransitionMask::MIN_WIDTH)
            && dim_differs(from.min_width, to.min_width))
        || (mask.contains(LayoutTransitionMask::MIN_HEIGHT)
            && dim_differs(from.min_height, to.min_height))
        || (mask.contains(LayoutTransitionMask::MAX_WIDTH)
            && dim_differs(from.max_width, to.max_width))
        || (mask.contains(LayoutTransitionMask::MAX_HEIGHT)
            && dim_differs(from.max_height, to.max_height))
        || (mask.contains(LayoutTransitionMask::PADDING)
            && (from.padding != to.padding || from.padding_dims != to.padding_dims))
        || (mask.contains(LayoutTransitionMask::MARGIN)
            && (from.margin != to.margin || from.margin_dims != to.margin_dims))
        || (mask.contains(LayoutTransitionMask::GAP)
            && ((from.gap - to.gap).abs() > 1e-3
                || from.row_gap != to.row_gap
                || from.column_gap != to.column_gap))
        || (mask.contains(LayoutTransitionMask::INSET) && from.inset != to.inset)
        || (mask.contains(LayoutTransitionMask::BORDER_WIDTH)
            && ((from.border_width - to.border_width).abs() > 1e-3
                || opt_dim_differs(from.border_edges, to.border_edges)))
        || (mask.contains(LayoutTransitionMask::FLEX)
            && ((from.flex_grow - to.flex_grow).abs() > 1e-4
                || (from.flex_shrink - to.flex_shrink).abs() > 1e-4
                || dim_differs(from.flex_basis, to.flex_basis)))
        || (mask.contains(LayoutTransitionMask::FONT_SIZE) && (from_fs - to_fs).abs() > 1e-3)
}

#[inline]
pub(crate) fn lerp_dimension(a: Dimension, b: Dimension, t: f32, clamp_zero: bool) -> Dimension {
    match (a, b) {
        (Dimension::Px(x), Dimension::Px(y)) => {
            let v = lerp_f32(x, y, t);
            Dimension::Px(if clamp_zero { v.max(0.0) } else { v })
        }
        (Dimension::Percent(x), Dimension::Percent(y)) => {
            let v = lerp_f32(x, y, t);
            Dimension::Percent(if clamp_zero { v.max(0.0) } else { v })
        }
        _ if t < 0.5 => a,
        _ => b,
    }
}

fn lerp_edges(a: Edges, b: Edges, t: f32, clamp_zero: bool) -> Edges {
    let clamp = |v: f32| if clamp_zero { v.max(0.0) } else { v };
    Edges {
        top: clamp(lerp_f32(a.top, b.top, t)),
        right: clamp(lerp_f32(a.right, b.right, t)),
        bottom: clamp(lerp_f32(a.bottom, b.bottom, t)),
        left: clamp(lerp_f32(a.left, b.left, t)),
    }
}

fn lerp_inset(a: InsetEdges, b: InsetEdges, t: f32) -> InsetEdges {
    InsetEdges {
        top: lerp_dimension(a.top, b.top, t, false),
        right: lerp_dimension(a.right, b.right, t, false),
        bottom: lerp_dimension(a.bottom, b.bottom, t, false),
        left: lerp_dimension(a.left, b.left, t, false),
    }
}

/// Applies Tier-2 layout interpolation onto `(out_layout, out_font_size)` for bits in `mask`.
pub fn apply_layout_masked_lerp(
    (from, from_fs): (&LayoutStyle, f32),
    (to, to_fs): (&LayoutStyle, f32),
    mask: LayoutTransitionMask,
    eased_t: f32,
    out_layout: &mut LayoutStyle,
    out_font_size: &mut f32,
) {
    let t = eased_t;
    if mask.contains(LayoutTransitionMask::WIDTH) {
        out_layout.width = lerp_dimension(from.width, to.width, t, true);
    }
    if mask.contains(LayoutTransitionMask::HEIGHT) {
        out_layout.height = lerp_dimension(from.height, to.height, t, true);
    }
    if mask.contains(LayoutTransitionMask::MIN_WIDTH) {
        out_layout.min_width = lerp_dimension(from.min_width, to.min_width, t, true);
    }
    if mask.contains(LayoutTransitionMask::MIN_HEIGHT) {
        out_layout.min_height = lerp_dimension(from.min_height, to.min_height, t, true);
    }
    if mask.contains(LayoutTransitionMask::MAX_WIDTH) {
        out_layout.max_width = lerp_dimension(from.max_width, to.max_width, t, true);
    }
    if mask.contains(LayoutTransitionMask::MAX_HEIGHT) {
        out_layout.max_height = lerp_dimension(from.max_height, to.max_height, t, true);
    }
    if mask.contains(LayoutTransitionMask::PADDING) {
        out_layout.padding = lerp_edges(from.padding, to.padding, t, true);
        out_layout.padding_dims = if t < 0.5 {
            from.padding_dims
        } else {
            to.padding_dims
        };
    }
    if mask.contains(LayoutTransitionMask::MARGIN) {
        out_layout.margin = lerp_edges(from.margin, to.margin, t, false);
        out_layout.margin_dims = if t < 0.5 {
            from.margin_dims
        } else {
            to.margin_dims
        };
    }
    if mask.contains(LayoutTransitionMask::GAP) {
        out_layout.gap = lerp_f32(from.gap, to.gap, t).max(0.0);
        out_layout.row_gap = match (from.row_gap, to.row_gap) {
            (Some(a), Some(b)) => Some(lerp_f32(a, b, t).max(0.0)),
            (Some(a), None) => Some(lerp_f32(a, to.gap, t).max(0.0)),
            (None, Some(b)) => Some(lerp_f32(from.gap, b, t).max(0.0)),
            (None, None) => None,
        };
        out_layout.column_gap = match (from.column_gap, to.column_gap) {
            (Some(a), Some(b)) => Some(lerp_f32(a, b, t).max(0.0)),
            (Some(a), None) => Some(lerp_f32(a, to.gap, t).max(0.0)),
            (None, Some(b)) => Some(lerp_f32(from.gap, b, t).max(0.0)),
            (None, None) => None,
        };
    }
    if mask.contains(LayoutTransitionMask::INSET) {
        out_layout.inset = lerp_inset(from.inset, to.inset, t);
    }
    if mask.contains(LayoutTransitionMask::BORDER_WIDTH) {
        out_layout.border_width = lerp_f32(from.border_width, to.border_width, t).max(0.0);
        out_layout.border_edges = match (from.border_edges, to.border_edges) {
            (Some(a), Some(b)) => Some(lerp_edges(a, b, t, true)),
            (Some(a), None) => Some(lerp_edges(a, Edges::all(to.border_width), t, true)),
            (None, Some(b)) => Some(lerp_edges(Edges::all(from.border_width), b, t, true)),
            (None, None) => None,
        };
    }
    if mask.contains(LayoutTransitionMask::FLEX) {
        out_layout.flex_grow = lerp_f32(from.flex_grow, to.flex_grow, t).max(0.0);
        out_layout.flex_shrink = lerp_f32(from.flex_shrink, to.flex_shrink, t).max(0.0);
        out_layout.flex_basis = lerp_dimension(from.flex_basis, to.flex_basis, t, true);
    }
    if mask.contains(LayoutTransitionMask::FONT_SIZE) {
        *out_font_size = lerp_f32(from_fs, to_fs, t).max(1.0);
    }
}
