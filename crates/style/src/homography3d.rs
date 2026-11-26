//! Planar 3D-to-2D projective homography math for CSS `perspective` and `preserve-3d`.

use crate::transform3d::Mat4;

/// Planar screen projection of a 3D-transformed border box (`z = 0` plane in local space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectiveQuad {
    /// Column/row inverse 3x3 homography `[h00, h01, h02, h10, h11, h12, h20, h21, h22]` mapping
    /// target `(sx, sy, 1)` to homogeneous local coords `(X, Y, W) -> (X/W, Y/W)`.
    pub inv_homography: [f32; 9],
    /// Projected screen corners `[top_left, top_right, bottom_right, bottom_left]`.
    pub screen_corners: [[f32; 2]; 4],
    /// Tight clamped screen-space bounding rectangle `[min_x, min_y, max_x, max_y]`.
    pub screen_aabb: [f32; 4],
    /// Projected normalized depth `z/w` at box center (larger `z/w` = closer to viewer under CSS +Z toward eye).
    pub center_z: f32,
    /// `true` when the quad's projected winding flipped CW<->CCW (`backface-visibility: hidden` culls it).
    pub is_backface: bool,
}

/// Returns `true` when `m` contains non-affine perspective projection terms (`w != 1`).
#[inline]
pub fn mat4_has_perspective(m: Mat4) -> bool {
    let v = m.0;
    v[3].abs() > 1e-6 || v[7].abs() > 1e-6 || v[11].abs() > 1e-6 || (v[15] - 1.0).abs() > 1e-5
}

/// Builds a CSS `perspective(d)` matrix centered at `(ox, oy)` (`T(ox, oy, 0) * P(d) * T(-ox, -oy, 0)`).
#[inline]
pub fn mat4_perspective_origin(d: f32, ox: f32, oy: f32) -> Mat4 {
    if !d.is_finite() || d <= 1e-3 {
        return Mat4::IDENTITY;
    }
    Mat4::translation(ox, oy, 0.0) * Mat4::perspective(d) * Mat4::translation(-ox, -oy, 0.0)
}

#[inline]
fn project_xyw(m: &[f32; 16], x: f32, y: f32) -> Option<[f32; 3]> {
    let sx = m[0] * x + m[4] * y + m[12];
    let sy = m[1] * x + m[5] * y + m[13];
    let sz = m[2] * x + m[6] * y + m[14];
    let sw = m[3] * x + m[7] * y + m[15];
    if !sw.is_finite() || sw <= 1e-4 {
        return None;
    }
    Some([sx / sw, sy / sw, sz / sw])
}

#[inline]
fn invert_mat3(m: [f32; 9]) -> Option<[f32; 9]> {
    let [a, b, c, d, e, f, g, h, i] = m;
    let c00 = e * i - f * h;
    let c01 = -(d * i - f * g);
    let c02 = d * h - e * g;
    let det = a * c00 + b * c01 + c * c02;
    if !det.is_finite() || det.abs() <= 1e-9 {
        return None;
    }
    let inv_d = 1.0 / det;
    Some([
        c00 * inv_d,
        (-(b * i - c * h)) * inv_d,
        (b * f - c * e) * inv_d,
        c01 * inv_d,
        (a * i - c * g) * inv_d,
        (-(a * f - c * d)) * inv_d,
        c02 * inv_d,
        (-(a * h - b * g)) * inv_d,
        (a * e - b * d) * inv_d,
    ])
}

/// Computes exact 3x3 planar homography, inverse mapping, AABB, center depth, and backface flag.
pub fn compute_projective_quad(
    mat: Mat4,
    rect_x: f32,
    rect_y: f32,
    rect_w: f32,
    rect_h: f32,
) -> Option<ProjectiveQuad> {
    if rect_w <= 0.0 || rect_h <= 0.0 {
        return None;
    }
    let m = mat.0;
    let p0 = project_xyw(&m, rect_x, rect_y)?;
    let p1 = project_xyw(&m, rect_x + rect_w, rect_y)?;
    let p2 = project_xyw(&m, rect_x + rect_w, rect_y + rect_h)?;
    let p3 = project_xyw(&m, rect_x, rect_y + rect_h)?;
    let pc = project_xyw(&m, rect_x + 0.5 * rect_w, rect_y + 0.5 * rect_h)?;

    let forward_h = [
        m[0], m[4], m[12], //
        m[1], m[5], m[13], //
        m[3], m[7], m[15], //
    ];
    let inv_homography = invert_mat3(forward_h)?;

    let corners = [
        [p0[0], p0[1]],
        [p1[0], p1[1]],
        [p2[0], p2[1]],
        [p3[0], p3[1]],
    ];
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for [cx, cy] in corners {
        min_x = min_x.min(cx);
        min_y = min_y.min(cy);
        max_x = max_x.max(cx);
        max_y = max_y.max(cy);
    }
    let cross = (p1[0] - p0[0]) * (p3[1] - p0[1]) - (p1[1] - p0[1]) * (p3[0] - p0[0]);
    Some(ProjectiveQuad {
        inv_homography,
        screen_corners: corners,
        screen_aabb: [min_x, min_y, max_x, max_y],
        center_z: pc[2],
        is_backface: cross <= 0.0,
    })
}

/// Maps screen point `(sx, sy)` back into un-projected local plane coordinates via `inv_homography`.
#[inline]
pub fn inverse_project_point(inv_h: &[f32; 9], sx: f32, sy: f32) -> Option<(f32, f32)> {
    let w = inv_h[6] * sx + inv_h[7] * sy + inv_h[8];
    if !w.is_finite() || w.abs() <= 1e-5 {
        return None;
    }
    let lx = (inv_h[0] * sx + inv_h[1] * sy + inv_h[2]) / w;
    let ly = (inv_h[3] * sx + inv_h[4] * sy + inv_h[5]) / w;
    (lx.is_finite() && ly.is_finite()).then_some((lx, ly))
}
