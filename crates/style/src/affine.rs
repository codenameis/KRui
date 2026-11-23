//! 2D transform parameters (`Transform2D`) and unified `2x3` affine transformation matrix (`Affine2`).

use krui_signals::FastPathTransform;
use std::ops::Mul;

/// Compositor-only 2D affine transform parameters.
///
/// Updates to `Transform2D` bypass Taffy layout completely and go straight to paint/compositing (Section 3.1).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct Transform2D {
    /// Horizontal translation in logical pixels.
    pub translate_x: f32,
    /// Vertical translation in logical pixels.
    pub translate_y: f32,
    /// Horizontal scale factor (`1.0` = identity).
    pub scale_x: f32,
    /// Vertical scale factor (`1.0` = identity).
    pub scale_y: f32,
    /// Clockwise rotation angle in degrees.
    pub rotate_deg: f32,
    /// Horizontal skew angle in degrees (`skewX`).
    pub skew_x_deg: f32,
    /// Vertical skew angle in degrees (`skewY`).
    pub skew_y_deg: f32,
    /// Optional explicit 2D affine matrix `[a, b, c, d, tx, ty]` from CSS `matrix(...)`.
    pub matrix_override: Option<[f32; 6]>,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform2D {
    /// Identity transform (no translation, scale `(1, 1)`, no rotation or skew).
    pub const IDENTITY: Self = Self {
        translate_x: 0.0,
        translate_y: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        rotate_deg: 0.0,
        skew_x_deg: 0.0,
        skew_y_deg: 0.0,
        matrix_override: None,
    };

    /// Pure 2D translation transform.
    pub const fn translate(x: f32, y: f32) -> Self {
        Self {
            translate_x: x,
            translate_y: y,
            scale_x: 1.0,
            scale_y: 1.0,
            rotate_deg: 0.0,
            skew_x_deg: 0.0,
            skew_y_deg: 0.0,
            matrix_override: None,
        }
    }

    /// Pure 2D scale transform.
    pub const fn scale(sx: f32, sy: f32) -> Self {
        Self {
            translate_x: 0.0,
            translate_y: 0.0,
            scale_x: sx,
            scale_y: sy,
            rotate_deg: 0.0,
            skew_x_deg: 0.0,
            skew_y_deg: 0.0,
            matrix_override: None,
        }
    }
}

impl From<FastPathTransform> for Transform2D {
    fn from(ft: FastPathTransform) -> Self {
        Self {
            translate_x: ft.translate_x,
            translate_y: ft.translate_y,
            scale_x: ft.scale_x,
            scale_y: ft.scale_y,
            rotate_deg: ft.rotate_deg,
            skew_x_deg: 0.0,
            skew_y_deg: 0.0,
            matrix_override: None,
        }
    }
}

/// Unified 2D affine transformation matrix (`2x3`):
/// - `x' = a * x + c * y + tx`
/// - `y' = b * x + d * y + ty`
///
/// Used identically across `PaintScene`, `WgpuRenderer`, `TinySkiaRenderer`, and pointer hit-testing.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct Affine2 {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Default for Affine2 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Affine2 {
    /// Identity affine matrix.
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    /// Pure translation matrix `T(tx, ty)`.
    pub const fn translation(tx: f32, ty: f32) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx,
            ty,
        }
    }

    /// Pure scaling matrix `S(sx, sy)` around `(0, 0)`.
    pub const fn scale(sx: f32, sy: f32) -> Self {
        Self {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// Pure clockwise rotation matrix `R(deg)` around `(0, 0)` in screen coordinates.
    pub fn rotation_deg(deg: f32) -> Self {
        if deg.abs() <= 1e-6 {
            return Self::IDENTITY;
        }
        let (sin, cos) = deg.to_radians().sin_cos();
        Self {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// Pure skew matrix `K(skew_x_deg, skew_y_deg)` around `(0, 0)` in screen coordinates.
    pub fn skew_deg(skew_x_deg: f32, skew_y_deg: f32) -> Self {
        if skew_x_deg.abs() <= 1e-6 && skew_y_deg.abs() <= 1e-6 {
            return Self::IDENTITY;
        }
        Self {
            a: 1.0,
            b: skew_y_deg.to_radians().tan(),
            c: skew_x_deg.to_radians().tan(),
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// Builds a node's local affine matrix around `(origin_x, origin_y)`:
    /// `T(origin) * T(translate) * R(rotate_deg) * Skew(skew_x, skew_y) * S(scale_x, scale_y) * T(-origin)`.
    pub fn from_transform2d_at_origin(t: Transform2D, origin_x: f32, origin_y: f32) -> Self {
        if let Some([a, b, c, d, tx, ty]) = t.matrix_override {
            return Self::translation(origin_x, origin_y)
                * Self { a, b, c, d, tx, ty }
                * Self::translation(-origin_x, -origin_y);
        }
        Self::translation(origin_x + t.translate_x, origin_y + t.translate_y)
            * Self::rotation_deg(t.rotate_deg)
            * Self::skew_deg(t.skew_x_deg, t.skew_y_deg)
            * Self::scale(t.scale_x, t.scale_y)
            * Self::translation(-origin_x, -origin_y)
    }

    /// Builds a node's local affine matrix with the CSS default transform-origin at the box center
    /// `(x + width * 0.5, y + height * 0.5)`.
    pub fn from_transform2d_for_rect(
        t: Transform2D,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Self {
        Self::from_transform2d_at_origin(t, x + width * 0.5, y + height * 0.5)
    }

    /// Returns `true` if this matrix has no rotation, skew, or scaling (`a == 1, b == 0, c == 0, d == 1`).
    pub fn is_pure_translation(self) -> bool {
        (self.a - 1.0).abs() <= 1e-4
            && self.b.abs() <= 1e-4
            && self.c.abs() <= 1e-4
            && (self.d - 1.0).abs() <= 1e-4
    }

    /// Transforms a 2D point `(x, y)` by this affine matrix.
    #[inline]
    pub fn transform_point(self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }

    /// Computes the inverse affine matrix, or returns `None` if singular (`det == 0`).
    pub fn inverse(self) -> Option<Self> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() <= 1e-8 {
            return None;
        }
        let inv_det = 1.0 / det;
        Some(Self {
            a: self.d * inv_det,
            b: -self.b * inv_det,
            c: -self.c * inv_det,
            d: self.a * inv_det,
            tx: (self.c * self.ty - self.d * self.tx) * inv_det,
            ty: (self.b * self.tx - self.a * self.ty) * inv_det,
        })
    }
}

impl Mul for Affine2 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            a: self.a * rhs.a + self.c * rhs.b,
            b: self.b * rhs.a + self.d * rhs.b,
            c: self.a * rhs.c + self.c * rhs.d,
            d: self.b * rhs.c + self.d * rhs.d,
            tx: self.a * rhs.tx + self.c * rhs.ty + self.tx,
            ty: self.b * rhs.tx + self.d * rhs.ty + self.ty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_around_box_center_preserves_center_coordinates() {
        let x = 100.0;
        let y = 80.0;
        let w = 200.0;
        let h = 60.0;
        let cx = x + w * 0.5;
        let cy = y + h * 0.5;

        let m = Affine2::from_transform2d_for_rect(Transform2D::scale(1.1, 1.1), x, y, w, h);
        let (tx_cx, tx_cy) = m.transform_point(cx, cy);
        assert!((tx_cx - cx).abs() < 1e-4);
        assert!((tx_cy - cy).abs() < 1e-4);
    }

    #[test]
    fn inverse_matrix_round_trips_rotated_and_scaled_points() {
        let parent = Affine2::from_transform2d_for_rect(
            Transform2D {
                translate_x: 40.0,
                translate_y: -15.0,
                scale_x: 1.5,
                scale_y: 1.5,
                rotate_deg: 30.0,
                ..Transform2D::IDENTITY
            },
            0.0,
            0.0,
            400.0,
            300.0,
        );
        let child = Affine2::from_transform2d_for_rect(
            Transform2D::translate(10.0, 20.0),
            50.0,
            60.0,
            120.0,
            44.0,
        );
        let combined = parent * child;
        let inv = combined.inverse().expect("invertible");

        let (wx, wy) = combined.transform_point(110.0, 82.0);
        let (lx, ly) = inv.transform_point(wx, wy);
        assert!((lx - 110.0).abs() < 1e-3);
        assert!((ly - 82.0).abs() < 1e-3);
    }
}
