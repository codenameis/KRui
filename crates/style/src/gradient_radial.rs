//! Radial gradients (`radial-gradient(...)`), background sizing/positioning,
//! and pointer-bound coordinate tokens (`--krui-pointer-x/y/xp/yp`, `--krui-hover-x/y`).

use crate::color::Color;
use crate::gradient_stops::GradientStopsRef;

/// Axis coordinate for a CSS `radial-gradient` or `background-position`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum RadialCoord {
    /// Percentage of the box dimension (`0.0..=100.0`).
    Percent(f32),
    /// Fixed offset in logical pixels from the box origin.
    Px(f32),
    /// Latched at `pointerdown` (`--krui-pointer-x/y/xp/yp`).
    PointerLatched {
        /// Fallback percentage (`0.0..=100.0`) before any pointer press.
        fallback_pct: f32,
    },
    /// Live pointer hover coordinate (`--krui-hover-x/y`).
    PointerHover {
        /// Fallback percentage (`0.0..=100.0`) while unhovered.
        fallback_pct: f32,
    },
}

impl Default for RadialCoord {
    fn default() -> Self {
        Self::Percent(50.0)
    }
}

impl RadialCoord {
    /// Center coordinate (`50%`).
    pub const CENTER: Self = Self::Percent(50.0);

    /// Resolves this coordinate to logical pixels relative to `extent` (`width` or `height`).
    #[inline]
    pub fn resolve_px(self, extent: f32, latched: Option<f32>, hover: Option<f32>) -> f32 {
        match self {
            Self::Percent(p) => extent * (p * 0.01),
            Self::Px(px) => px,
            Self::PointerLatched { fallback_pct } => {
                latched.unwrap_or(extent * (fallback_pct * 0.01))
            }
            Self::PointerHover { fallback_pct } => hover.unwrap_or(extent * (fallback_pct * 0.01)),
        }
    }

    /// Returns `true` when this coordinate reads a pointerdown-latched variable.
    #[inline]
    pub const fn uses_latched_pointer(self) -> bool {
        matches!(self, Self::PointerLatched { .. })
    }

    /// Returns `true` when this coordinate reads a live hover pointer variable.
    #[inline]
    pub const fn uses_hover_pointer(self) -> bool {
        matches!(self, Self::PointerHover { .. })
    }
}

/// Two-stop CSS `radial-gradient(...)` specification with optional background scale.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct RadialGradient {
    /// Horizontal gradient center coordinate.
    pub center_x: RadialCoord,
    /// Vertical gradient center coordinate.
    pub center_y: RadialCoord,
    /// `true` for `circle`, `false` for `ellipse`.
    pub is_circle: bool,
    /// Normalized gradient box width scale (`1.0` = `100%`).
    pub size_scale_x: f32,
    /// Normalized gradient box height scale (`1.0` = `100%`).
    pub size_scale_y: f32,
    /// Color at `inner_stop` (defaults to `0%`).
    pub inner_color: Color,
    /// Color at `outer_stop` (defaults to `100%`).
    pub outer_color: Color,
    /// Normalized position (`0.0..=1.0`) of the inner stop.
    pub inner_stop: f32,
    /// Normalized position (`0.0..=1.0`) of the outer stop.
    pub outer_stop: f32,
    /// Full stop list when > 2 stops or repeating radial gradient is authored.
    pub extra_stops: GradientStopsRef,
    /// `true` for `repeating-radial-gradient(...)`.
    pub repeating: bool,
}

impl Default for RadialGradient {
    fn default() -> Self {
        Self {
            center_x: RadialCoord::CENTER,
            center_y: RadialCoord::CENTER,
            is_circle: true,
            size_scale_x: 1.0,
            size_scale_y: 1.0,
            inner_color: Color::TRANSPARENT,
            outer_color: Color::TRANSPARENT,
            inner_stop: 0.0,
            outer_stop: 1.0,
            extra_stops: GradientStopsRef::Empty,
            repeating: false,
        }
    }
}

impl RadialGradient {
    /// Returns `true` when either center coordinate depends on a pointerdown-latched variable.
    #[inline]
    pub const fn uses_latched_pointer(&self) -> bool {
        self.center_x.uses_latched_pointer() || self.center_y.uses_latched_pointer()
    }

    /// Returns `true` when either center coordinate depends on a live hover pointer variable.
    #[inline]
    pub const fn uses_hover_pointer(&self) -> bool {
        self.center_x.uses_hover_pointer() || self.center_y.uses_hover_pointer()
    }
}

/// Latched and live-hover local pointer coordinates `(latched_xy, hover_xy)`.
pub type PointerCoordPair = (Option<(f32, f32)>, Option<(f32, f32)>);

/// Resolves box-center-relative origin `(cx_off, cy_off)` and radii `(rx, ry)` in logical px.
pub fn resolve_radial_geometry(
    grad: &RadialGradient,
    bg_pos: Option<(RadialCoord, RadialCoord)>,
    bg_size_scale: Option<(f32, f32)>,
    (box_w, box_h): (f32, f32),
    (latched_xy, hover_xy): PointerCoordPair,
) -> (f32, f32, f32, f32) {
    let (sx, sy) = bg_size_scale.unwrap_or((grad.size_scale_x, grad.size_scale_y));
    let (sx, sy) = (sx.max(0.0), sy.max(0.0));
    let (lx_l, ly_l) = (latched_xy.map(|p| p.0), latched_xy.map(|p| p.1));
    let (lx_h, ly_h) = (hover_xy.map(|p| p.0), hover_xy.map(|p| p.1));
    let pointer_in_bg = bg_pos.is_some_and(|(px, py)| {
        px.uses_latched_pointer()
            || px.uses_hover_pointer()
            || py.uses_latched_pointer()
            || py.uses_hover_pointer()
    });
    let (cx_box, cy_box) = if grad.uses_latched_pointer() || grad.uses_hover_pointer() {
        (
            grad.center_x.resolve_px(box_w, lx_l, lx_h),
            grad.center_y.resolve_px(box_h, ly_l, ly_h),
        )
    } else if pointer_in_bg {
        let (px, py) = bg_pos.unwrap_or_default();
        (
            px.resolve_px(box_w, lx_l, lx_h),
            py.resolve_px(box_h, ly_l, ly_h),
        )
    } else {
        let (tile_w, tile_h) = (box_w * sx, box_h * sy);
        let tile_x0 = resolve_bg_tile_offset(bg_pos.map(|p| p.0), box_w, tile_w, lx_l, lx_h);
        let tile_y0 = resolve_bg_tile_offset(bg_pos.map(|p| p.1), box_h, tile_h, ly_l, ly_h);
        (
            tile_x0 + grad.center_x.resolve_px(tile_w, lx_l, lx_h),
            tile_y0 + grad.center_y.resolve_px(tile_h, ly_l, ly_h),
        )
    };
    let dx_max = cx_box.abs().max((box_w - cx_box).abs()) * sx;
    let dy_max = cy_box.abs().max((box_h - cy_box).abs()) * sy;
    let (rx, ry) = if grad.is_circle {
        let r = dx_max.hypot(dy_max).max(1e-3);
        (r, r)
    } else {
        let s2 = std::f32::consts::SQRT_2;
        ((dx_max * s2).max(1e-3), (dy_max * s2).max(1e-3))
    };
    (cx_box - 0.5 * box_w, cy_box - 0.5 * box_h, rx, ry)
}

fn resolve_bg_tile_offset(
    coord: Option<RadialCoord>,
    box_ext: f32,
    tile_ext: f32,
    latched: Option<f32>,
    hover: Option<f32>,
) -> f32 {
    match coord {
        None => 0.0,
        Some(RadialCoord::Percent(p)) => (box_ext - tile_ext) * (p * 0.01),
        Some(c) => c.resolve_px(box_ext, latched, hover),
    }
}
