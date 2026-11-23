//! Multi-layer CSS `box-shadow` storage (up to 5 active layers).

use crate::color::Color;
use crate::paint_types::BoxShadow;
use crate::transition_lerp::lerp_opt_shadow;

/// Maximum number of active CSS `box-shadow` layers stored per element (matches Tailwind v4 stacks).
pub const MAX_BOX_SHADOW_LAYERS: usize = 5;

const ZERO_SHADOW: BoxShadow = BoxShadow {
    offset_x: 0.0,
    offset_y: 0.0,
    blur_radius: 0.0,
    spread_radius: 0.0,
    color: Color::TRANSPARENT,
    inset: false,
};

/// Ordered stack of up to [`MAX_BOX_SHADOW_LAYERS`] active CSS `box-shadow` layers.
///
/// CSS paint order renders layer `0` closest to the box (on top of layers `1..len`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct ShadowStack {
    /// Fixed-capacity array of shadow layers (`0..len` are valid).
    pub layers: [BoxShadow; MAX_BOX_SHADOW_LAYERS],
    /// Number of active layers (`0..=MAX_BOX_SHADOW_LAYERS`).
    pub len: u8,
}

impl Default for ShadowStack {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl ShadowStack {
    /// Empty shadow stack (equivalent to `box-shadow: none`).
    pub const EMPTY: Self = Self {
        layers: [ZERO_SHADOW; MAX_BOX_SHADOW_LAYERS],
        len: 0,
    };

    /// Returns `true` when no shadow layers are active.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// Returns active layers in CSS declaration order (`layers[0]` = frontmost).
    #[inline]
    pub fn as_slice(&self) -> &[BoxShadow] {
        let count = (self.len as usize).min(MAX_BOX_SHADOW_LAYERS);
        &self.layers[..count]
    }

    /// Returns `true` when any layer has non-zero alpha.
    #[inline]
    pub fn has_visible(self) -> bool {
        self.as_slice().iter().any(|s| s.color.a > 0)
    }

    /// Builds a [`ShadowStack`] from parsed CSS layers (up to [`MAX_BOX_SHADOW_LAYERS`]), preserving
    /// transparent `alpha == 0` placeholder layers so multi-layer shadow interpolation stays 1:1.
    pub fn from_layers(items: &[BoxShadow]) -> Self {
        let mut out = Self::EMPTY;
        for &item in items {
            let idx = out.len as usize;
            if idx >= MAX_BOX_SHADOW_LAYERS {
                break;
            }
            out.layers[idx] = item;
            out.len += 1;
        }
        out
    }

    /// Returns the first active layer (`layers[0]`), if present.
    #[inline]
    pub const fn first(self) -> Option<BoxShadow> {
        if self.len >= 1 {
            Some(self.layers[0])
        } else {
            None
        }
    }

    /// Returns the second active layer (`layers[1]`), if present.
    #[inline]
    pub const fn second(self) -> Option<BoxShadow> {
        if self.len >= 2 {
            Some(self.layers[1])
        } else {
            None
        }
    }

    /// Interpolates two [`ShadowStack`] values layer-by-layer at factor `t`.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let count = (a.len.max(b.len) as usize).min(MAX_BOX_SHADOW_LAYERS);
        let mut out = Self::EMPTY;
        for i in 0..count {
            let sa = (i < a.len as usize).then_some(a.layers[i]);
            let sb = (i < b.len as usize).then_some(b.layers[i]);
            if let Some(sh) = lerp_opt_shadow(sa, sb, t) {
                let idx = out.len as usize;
                if idx < MAX_BOX_SHADOW_LAYERS {
                    out.layers[idx] = sh;
                    out.len += 1;
                }
            }
        }
        out
    }
}
