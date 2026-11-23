//! Helper methods for per-side `Dimension` edges (`InsetEdges`, `BoxSide`), `LayoutStyle` margin/padding
//! synchronization, CSS Scrollbars Level 1 `ScrollbarWidth`, and W3C `accent-color` resolution.

use crate::color::Color;
use crate::layout_types::{Dimension, Edges, InsetEdges, LayoutStyle};
use crate::paint_types::PaintStyle;

/// Browser UA fallback color (`#2563eb`) used when `accent-color: auto` is active without a preset override.
pub const UA_AUTO_ACCENT_COLOR: Color = Color {
    r: 37,
    g: 99,
    b: 235,
    a: 255,
};

/// Neutral UA track fill (`#cbd5e1`) for `<input type="range">` inactive slider tracks.
pub const UA_SLIDER_TRACK_COLOR: Color = Color {
    r: 203,
    g: 213,
    b: 225,
    a: 255,
};

/// Neutral UA thumb fill (`rgba(120, 128, 140, 0.66)`) when `scrollbar-color: auto` is active.
pub const UA_SCROLLBAR_THUMB_COLOR: Color = Color {
    r: 120,
    g: 128,
    b: 140,
    a: 168,
};

/// Browser UA tooltip surface background (`#1f2937`).
pub const UA_TOOLTIP_BG_COLOR: Color = Color {
    r: 31,
    g: 41,
    b: 55,
    a: 255,
};

/// Browser UA tooltip text color (`#ffffff`).
pub const UA_TOOLTIP_FG_COLOR: Color = Color {
    r: 255,
    g: 255,
    b: 255,
    a: 255,
};

/// Browser UA popup/overlay background (`#ffffff`).
pub const UA_POPUP_BG_COLOR: Color = Color {
    r: 255,
    g: 255,
    b: 255,
    a: 255,
};

/// Browser UA popup border (`#cbd5e1`).
pub const UA_POPUP_BORDER_COLOR: Color = Color {
    r: 203,
    g: 213,
    b: 225,
    a: 255,
};

/// Browser UA selection highlight fill (`rgba(37, 99, 235, 0.25)`).
pub const UA_SELECTION_BG_COLOR: Color = Color {
    r: 37,
    g: 99,
    b: 235,
    a: 64,
};

/// Browser UA selection highlight border (`#2563eb`).
pub const UA_SELECTION_BORDER_COLOR: Color = Color {
    r: 37,
    g: 99,
    b: 235,
    a: 255,
};

/// Browser UA IME composing target clause background (`rgba(37, 99, 235, 0.20)`).
pub const UA_IME_TARGET_BG_COLOR: Color = Color {
    r: 37,
    g: 99,
    b: 235,
    a: 51,
};

/// Browser UA muted secondary text color (`#64748b`).
pub const UA_SECONDARY_TEXT_COLOR: Color = Color {
    r: 100,
    g: 116,
    b: 139,
    a: 255,
};

/// Browser UA primary text color (`#0f172a`).
pub const UA_PRIMARY_TEXT_COLOR: Color = Color {
    r: 15,
    g: 23,
    b: 42,
    a: 255,
};

/// CSS Scrollbars Styling Module Level 1 `scrollbar-width` (`auto | thin | none`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum ScrollbarWidth {
    /// Default UA scrollbar thickness (`auto`).
    #[default]
    Auto,
    /// Narrower scrollbar variant (`thin`).
    Thin,
    /// Hides the visual scrollbar while preserving wheel/keyboard scrolling (`none`).
    None,
}

impl ScrollbarWidth {
    /// Parses a CSS `scrollbar-width` keyword (`auto | thin | none`).
    #[inline]
    pub fn parse_css(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "thin" => Some(Self::Thin),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    /// Returns `(thumb_width_px, thumb_radius_px, right_inset_px)`, or `None` when `ScrollbarWidth::None`.
    #[inline]
    pub const fn metrics(self) -> Option<(f32, f32, f32)> {
        match self {
            Self::Auto => Some((6.0, 3.0, 9.0)),
            Self::Thin => Some((3.0, 1.5, 6.0)),
            Self::None => None,
        }
    }
}

/// Box edge side identifier for per-side layout dimension updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum BoxSide {
    /// Top box edge.
    Top,
    /// Right box edge.
    Right,
    /// Bottom box edge.
    Bottom,
    /// Left box edge.
    Left,
}

impl InsetEdges {
    /// All four edges initialized to `Dimension::Px(0.0)`.
    pub const ZERO_PX: Self = Self {
        top: Dimension::Px(0.0),
        right: Dimension::Px(0.0),
        bottom: Dimension::Px(0.0),
        left: Dimension::Px(0.0),
    };

    /// Converts fixed pixel [`Edges`] into [`InsetEdges`].
    #[inline]
    pub const fn from_px_edges(e: Edges) -> Self {
        Self {
            top: Dimension::Px(e.top),
            right: Dimension::Px(e.right),
            bottom: Dimension::Px(e.bottom),
            left: Dimension::Px(e.left),
        }
    }

    /// Returns `true` when every edge is a concrete `Dimension::Px` length.
    #[inline]
    pub const fn is_all_px(&self) -> bool {
        matches!(
            (self.top, self.right, self.bottom, self.left),
            (
                Dimension::Px(_),
                Dimension::Px(_),
                Dimension::Px(_),
                Dimension::Px(_)
            )
        )
    }

    /// Converts `Px` edges to pixel values, falling back to `0.0` for `Auto` and `Percent`.
    #[inline]
    pub const fn to_px_edges_lossy(&self) -> Edges {
        Edges {
            top: match self.top {
                Dimension::Px(v) => v,
                Dimension::Auto | Dimension::Percent(_) => 0.0,
            },
            right: match self.right {
                Dimension::Px(v) => v,
                Dimension::Auto | Dimension::Percent(_) => 0.0,
            },
            bottom: match self.bottom {
                Dimension::Px(v) => v,
                Dimension::Auto | Dimension::Percent(_) => 0.0,
            },
            left: match self.left {
                Dimension::Px(v) => v,
                Dimension::Auto | Dimension::Percent(_) => 0.0,
            },
        }
    }

    /// Resolves `Px` and `Percent` edges against `basis_px` (using `0.0` for `Auto`).
    #[inline]
    pub fn resolve_px(&self, basis_px: f32) -> Edges {
        let basis = basis_px.max(0.0);
        let resolve = |d: Dimension| match d {
            Dimension::Px(v) => v,
            Dimension::Percent(p) => basis * (p * 0.01),
            Dimension::Auto => 0.0,
        };
        Edges {
            top: resolve(self.top),
            right: resolve(self.right),
            bottom: resolve(self.bottom),
            left: resolve(self.left),
        }
    }

    /// Returns one side of `self`.
    #[inline]
    pub const fn get(&self, side: BoxSide) -> Dimension {
        match side {
            BoxSide::Top => self.top,
            BoxSide::Right => self.right,
            BoxSide::Bottom => self.bottom,
            BoxSide::Left => self.left,
        }
    }

    /// Updates one side of `self`.
    #[inline]
    pub fn set(&mut self, side: BoxSide, value: Dimension) {
        match side {
            BoxSide::Top => self.top = value,
            BoxSide::Right => self.right = value,
            BoxSide::Bottom => self.bottom = value,
            BoxSide::Left => self.left = value,
        }
    }
}

impl LayoutStyle {
    /// Returns effective 4-side margin dimensions (`margin_dims` if `auto`/`%`, otherwise `margin` in px).
    #[inline]
    pub const fn effective_margin_dims(&self) -> InsetEdges {
        match self.margin_dims {
            Some(dims) => dims,
            None => InsetEdges::from_px_edges(self.margin),
        }
    }

    /// Returns effective 4-side padding dimensions (`padding_dims` if `%`, otherwise `padding` in px).
    #[inline]
    pub const fn effective_padding_dims(&self) -> InsetEdges {
        match self.padding_dims {
            Some(dims) => dims,
            None => InsetEdges::from_px_edges(self.padding),
        }
    }

    /// Resolves padding edges in logical pixels given the containing block's `container_width`.
    #[inline]
    pub fn resolve_padding_px(&self, container_width: f32) -> Edges {
        match self.padding_dims {
            Some(dims) => dims.resolve_px(container_width),
            None => self.padding,
        }
    }

    /// Sets all four margin edges while keeping `margin` and `margin_dims` synchronized.
    #[inline]
    pub fn set_margin_all(&mut self, dims: InsetEdges) {
        self.margin = dims.to_px_edges_lossy();
        self.margin_dims = (!dims.is_all_px()).then_some(dims);
    }

    /// Sets a single margin edge while keeping `margin` and `margin_dims` synchronized.
    #[inline]
    pub fn set_margin_side(&mut self, side: BoxSide, value: Dimension) {
        let mut dims = self.effective_margin_dims();
        dims.set(side, value);
        self.set_margin_all(dims);
    }

    /// Sets all four padding edges while keeping `padding` and `padding_dims` synchronized.
    #[inline]
    pub fn set_padding_all(&mut self, dims: InsetEdges) {
        self.padding = dims.to_px_edges_lossy();
        self.padding_dims = (!dims.is_all_px()).then_some(dims);
        self.padding_explicit = true;
    }

    /// Sets a single padding edge while keeping `padding` and `padding_dims` synchronized.
    #[inline]
    pub fn set_padding_side(&mut self, side: BoxSide, value: Dimension) {
        let mut dims = self.effective_padding_dims();
        dims.set(side, value);
        self.set_padding_all(dims);
    }
}

impl PaintStyle {
    /// Resolves CSS `accent-color` (`Some(c)` when explicitly set or inherited, else [`UA_AUTO_ACCENT_COLOR`]).
    #[inline]
    pub const fn resolve_accent_color(&self) -> Color {
        match self.accent_color {
            Some(c) => c,
            None => UA_AUTO_ACCENT_COLOR,
        }
    }

    /// Computes a contrasting glyph/indicator foreground color (`#111827` or `#ffffff`) against `resolve_accent_color()`.
    #[inline]
    pub const fn resolve_accent_foreground(&self) -> Color {
        let ac = self.resolve_accent_color();
        let luma = (ac.r as u32) * 299 + (ac.g as u32) * 587 + (ac.b as u32) * 114;
        if luma >= 155_000 {
            Color {
                r: 17,
                g: 24,
                b: 39,
                a: 255,
            }
        } else {
            Color::WHITE
        }
    }
}
