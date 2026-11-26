//! CSS Transforms Level 2 standalone properties (`translate`, `rotate`, `scale`, `transform-origin`).

use crate::affine::{Affine2, Transform2D};
use crate::transform3d::{Mat4, TransformListRef};
use crate::transition_lerp::lerp_f32;
use std::ops::Mul;

/// Single axis of CSS `translate` (`px` or element-size `%`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum TranslateAxis {
    /// Fixed pixel translation offset.
    Px(f32),
    /// Percentage of the element's own border-box width (for X) or height (for Y).
    Percent(f32),
}

impl Default for TranslateAxis {
    fn default() -> Self {
        Self::ZERO
    }
}

impl TranslateAxis {
    /// Zero pixel offset (`0px`).
    pub const ZERO: Self = Self::Px(0.0);

    #[inline]
    fn is_zero(self) -> bool {
        match self {
            Self::Px(v) | Self::Percent(v) => v.abs() <= 1e-6,
        }
    }

    #[inline]
    fn resolve(self, box_span: f32) -> f32 {
        match self {
            Self::Px(v) => v,
            Self::Percent(p) => box_span * (p * 0.01),
        }
    }

    fn lerp(self, other: Self, t: f32) -> Self {
        match (self, other) {
            (Self::Px(a), Self::Px(b)) => Self::Px(lerp_f32(a, b, t)),
            (Self::Percent(a), Self::Percent(b)) => Self::Percent(lerp_f32(a, b, t)),
            (Self::Px(a), Self::Percent(b)) if a.abs() <= 1e-6 => {
                Self::Percent(lerp_f32(0.0, b, t))
            }
            (Self::Percent(a), Self::Px(b)) if b.abs() <= 1e-6 => {
                Self::Percent(lerp_f32(a, 0.0, t))
            }
            _ if t < 0.5 => self,
            _ => other,
        }
    }
}

/// Single axis of CSS `transform-origin` relative to the element's border box.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum OriginAxis {
    /// Offset in logical pixels from the top-left corner of the border box.
    Px(f32),
    /// Percentage of border-box width/height (`50%` = box center).
    Percent(f32),
}

impl Default for OriginAxis {
    fn default() -> Self {
        Self::CENTER
    }
}

impl OriginAxis {
    /// Default CSS `transform-origin` axis (`50%`).
    pub const CENTER: Self = Self::Percent(50.0);

    /// Resolves this axis against `[box_start, box_start + box_span]` in logical pixels.
    #[inline]
    #[must_use]
    pub fn resolve(self, box_start: f32, box_span: f32) -> f32 {
        match self {
            Self::Px(v) => box_start + v,
            Self::Percent(p) => box_start + box_span * (p * 0.01),
        }
    }

    fn lerp(self, other: Self, t: f32) -> Self {
        match (self, other) {
            (Self::Px(a), Self::Px(b)) => Self::Px(lerp_f32(a, b, t)),
            (Self::Percent(a), Self::Percent(b)) => Self::Percent(lerp_f32(a, b, t)),
            (Self::Px(a), Self::Percent(b)) if a.abs() <= 1e-6 => {
                Self::Percent(lerp_f32(0.0, b, t))
            }
            (Self::Percent(a), Self::Px(b)) if b.abs() <= 1e-6 => {
                Self::Percent(lerp_f32(a, 0.0, t))
            }
            _ if t < 0.5 => self,
            _ => other,
        }
    }
}

/// Independent CSS Transforms Level 2 properties (`translate`, `rotate`, `scale`, `transform-origin`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct StandaloneTransform {
    /// Horizontal offset from the CSS `translate` property.
    pub translate_x: TranslateAxis,
    /// Vertical offset from the CSS `translate` property.
    pub translate_y: TranslateAxis,
    /// Clockwise rotation in degrees from the CSS `rotate` property.
    pub rotate_deg: f32,
    /// Horizontal multiplier from the CSS `scale` property.
    pub scale_x: f32,
    /// Vertical multiplier from the CSS `scale` property.
    pub scale_y: f32,
    /// Horizontal pivot from `transform-origin`.
    pub origin_x: OriginAxis,
    /// Vertical pivot from `transform-origin`.
    pub origin_y: OriginAxis,
    /// Optional parent `perspective: <length>` distance in logical px.
    pub perspective_px: Option<f32>,
    /// Horizontal pivot of `perspective-origin`.
    pub perspective_origin_x: OriginAxis,
    /// Vertical pivot of `perspective-origin`.
    pub perspective_origin_y: OriginAxis,
    /// `true` when `transform-style: preserve-3d` is active.
    pub preserve_3d: bool,
    /// `true` when `backface-visibility: visible` (CSS default).
    pub backface_visible: bool,
}

impl Default for StandaloneTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl StandaloneTransform {
    /// Identity standalone transform (`translate: 0`, `rotate: 0deg`, `scale: 1 1`, `origin: 50% 50%`).
    pub const IDENTITY: Self = Self {
        translate_x: TranslateAxis::ZERO,
        translate_y: TranslateAxis::ZERO,
        rotate_deg: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        origin_x: OriginAxis::CENTER,
        origin_y: OriginAxis::CENTER,
        perspective_px: None,
        perspective_origin_x: OriginAxis::CENTER,
        perspective_origin_y: OriginAxis::CENTER,
        preserve_3d: false,
        backface_visible: true,
    };

    /// Returns `true` when `translate`, `rotate`, and `scale` are all identity values.
    #[inline]
    pub fn is_identity(self) -> bool {
        self.translate_x.is_zero()
            && self.translate_y.is_zero()
            && self.rotate_deg.abs() <= 1e-6
            && (self.scale_x - 1.0).abs() <= 1e-6
            && (self.scale_y - 1.0).abs() <= 1e-6
            && self.perspective_px.is_none()
            && !self.preserve_3d
            && self.backface_visible
    }

    /// Composes `self` (`translate`, `rotate`, `scale` around `transform-origin`) followed by
    /// `tf` (`transform`) in W3C CSS Transforms Level 2 order.
    pub fn compose_with_transform2d(
        self,
        tf: Transform2D,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Affine2 {
        if self == Self::IDENTITY {
            return Affine2::from_transform2d_for_rect(tf, x, y, width, height);
        }
        let ox = self.origin_x.resolve(x, width);
        let oy = self.origin_y.resolve(y, height);
        let base_tf = Affine2::from_transform2d_at_origin(tf, ox, oy);
        if self.is_identity() {
            return base_tf;
        }
        let tx = self.translate_x.resolve(width);
        let ty = self.translate_y.resolve(height);
        Affine2::translation(ox + tx, oy + ty)
            * Affine2::rotation_deg(self.rotate_deg)
            * Affine2::scale(self.scale_x, self.scale_y)
            * Affine2::translation(-ox, -oy)
            * base_tf
    }

    /// Composes `self` (`translate`, `rotate`, `scale`) followed by ordered [`TransformListRef`]
    /// and dynamic slot `tf` around `transform-origin` in W3C CSS Transforms Level 2 order.
    pub fn compose_with_list(
        self,
        tf: Transform2D,
        list: TransformListRef,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Affine2 {
        if list.is_empty() {
            return self.compose_with_transform2d(tf, x, y, width, height);
        }
        let ox = self.origin_x.resolve(x, width);
        let oy = self.origin_y.resolve(y, height);
        let list_af = list
            .eval_mat4_at_rect(x, y, width, height, self.origin_x, self.origin_y)
            .to_affine2_projected();
        let slot_af = if tf == Transform2D::IDENTITY {
            Affine2::IDENTITY
        } else {
            Affine2::from_transform2d_at_origin(tf, ox, oy)
        };
        let combined = list_af * slot_af;
        if self.is_identity() {
            return combined;
        }
        let tx = self.translate_x.resolve(width);
        let ty = self.translate_y.resolve(height);
        Affine2::translation(ox + tx, oy + ty)
            * Affine2::rotation_deg(self.rotate_deg)
            * Affine2::scale(self.scale_x, self.scale_y)
            * Affine2::translation(-ox, -oy)
            * combined
    }

    /// Composes `self`, [`TransformListRef`], and slot `tf` into a full 4x4 matrix around `transform-origin`.
    pub fn compose_mat4_with_list(
        self,
        tf: Transform2D,
        list: TransformListRef,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Mat4 {
        let ox = self.origin_x.resolve(x, width);
        let oy = self.origin_y.resolve(y, height);
        let list_m = if list.is_empty() {
            Mat4::IDENTITY
        } else {
            list.eval_mat4_at_rect(x, y, width, height, self.origin_x, self.origin_y)
        };
        let slot_m = if tf == Transform2D::IDENTITY {
            Mat4::IDENTITY
        } else {
            let af = Affine2::from_transform2d_at_origin(tf, ox, oy);
            Mat4::from_matrix2d([af.a, af.b, af.c, af.d, af.tx, af.ty])
        };
        let combined = list_m.mul(slot_m);
        if self.is_identity() {
            return combined;
        }
        let tx = self.translate_x.resolve(width);
        let ty = self.translate_y.resolve(height);
        let s_af = Affine2::translation(ox + tx, oy + ty)
            * Affine2::rotation_deg(self.rotate_deg)
            * Affine2::scale(self.scale_x, self.scale_y)
            * Affine2::translation(-ox, -oy);
        Mat4::from_matrix2d([s_af.a, s_af.b, s_af.c, s_af.d, s_af.tx, s_af.ty]).mul(combined)
    }

    /// Interpolates two [`StandaloneTransform`] values at factor `t`.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            translate_x: a.translate_x.lerp(b.translate_x, t),
            translate_y: a.translate_y.lerp(b.translate_y, t),
            rotate_deg: lerp_f32(a.rotate_deg, b.rotate_deg, t),
            scale_x: lerp_f32(a.scale_x, b.scale_x, t),
            scale_y: lerp_f32(a.scale_y, b.scale_y, t),
            origin_x: a.origin_x.lerp(b.origin_x, t),
            origin_y: a.origin_y.lerp(b.origin_y, t),
            perspective_px: match (a.perspective_px, b.perspective_px) {
                (Some(x), Some(y)) => Some(lerp_f32(x, y, t).max(1.0)),
                (Some(x), None) if t < 0.5 => Some(x),
                (None, Some(y)) if t >= 0.5 => Some(y),
                _ => None,
            },
            perspective_origin_x: a.perspective_origin_x.lerp(b.perspective_origin_x, t),
            perspective_origin_y: a.perspective_origin_y.lerp(b.perspective_origin_y, t),
            preserve_3d: if t < 0.5 {
                a.preserve_3d
            } else {
                b.preserve_3d
            },
            backface_visible: if t < 0.5 {
                a.backface_visible
            } else {
                b.backface_visible
            },
        }
    }
}
