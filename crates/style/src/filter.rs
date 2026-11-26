//! W3C CSS `filter` effects (`invert`, `grayscale`, `brightness`, `contrast`, `saturate`, `sepia`, `opacity`, `blur`).

/// Resolved CSS `filter` pipeline parameters stored on [`crate::PaintStyle`] and compositor layers.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct CssFilterEffect {
    pub invert: f32,
    pub grayscale: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturate: f32,
    pub sepia: f32,
    pub opacity: f32,
    pub blur_px: f32,
}

impl CssFilterEffect {
    /// Identity filter (no visual modification).
    pub const IDENTITY: Self = Self {
        invert: 0.0,
        grayscale: 0.0,
        brightness: 1.0,
        contrast: 1.0,
        saturate: 1.0,
        sepia: 0.0,
        opacity: 1.0,
        blur_px: 0.0,
    };

    /// Returns `true` when all filter parameters are at identity within floating-point tolerance.
    #[inline]
    pub fn is_identity(&self) -> bool {
        self.invert.abs() < 1e-4
            && self.grayscale.abs() < 1e-4
            && (self.brightness - 1.0).abs() < 1e-4
            && (self.contrast - 1.0).abs() < 1e-4
            && (self.saturate - 1.0).abs() < 1e-4
            && self.sepia.abs() < 1e-4
            && (self.opacity - 1.0).abs() < 1e-4
            && self.blur_px <= 1e-3
    }

    /// Returns `true` when only `blur_px` is inactive and a color matrix pass suffices.
    #[inline]
    pub fn has_color_matrix(&self) -> bool {
        self.invert.abs() >= 1e-4
            || self.grayscale.abs() >= 1e-4
            || (self.brightness - 1.0).abs() >= 1e-4
            || (self.contrast - 1.0).abs() >= 1e-4
            || (self.saturate - 1.0).abs() >= 1e-4
            || self.sepia.abs() >= 1e-4
            || (self.opacity - 1.0).abs() >= 1e-4
    }

    /// Converts color adjustments into a 4x5 row-major sRGB affine color matrix `[r, g, b, a, bias]`.
    pub fn to_color_matrix_4x5(&self) -> [[f32; 5]; 4] {
        let m = self.apply_hue_chroma_matrices([
            [1.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0, 0.0],
        ]);
        self.apply_tone_matrices(m)
    }

    fn apply_hue_chroma_matrices(&self, mut m: [[f32; 5]; 4]) -> [[f32; 5]; 4] {
        if self.grayscale.abs() >= 1e-4 {
            let p = self.grayscale.clamp(0.0, 1.0);
            let inv = 1.0 - p;
            m = mul_4x5(
                [
                    [0.2126 + 0.7874 * inv, 0.7152 * p, 0.0722 * p, 0.0, 0.0],
                    [0.2126 * p, 0.7152 + 0.2848 * inv, 0.0722 * p, 0.0, 0.0],
                    [0.2126 * p, 0.7152 * p, 0.0722 + 0.9278 * inv, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0, 0.0],
                ],
                m,
            );
        }
        if self.sepia.abs() >= 1e-4 {
            let p = self.sepia.clamp(0.0, 1.0);
            let inv = 1.0 - p;
            m = mul_4x5(
                [
                    [inv + 0.393 * p, 0.769 * p, 0.189 * p, 0.0, 0.0],
                    [0.349 * p, inv + 0.686 * p, 0.168 * p, 0.0, 0.0],
                    [0.272 * p, 0.534 * p, inv + 0.131 * p, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0, 0.0],
                ],
                m,
            );
        }
        if (self.saturate - 1.0).abs() >= 1e-4 {
            let s = self.saturate.max(0.0);
            m = mul_4x5(
                [
                    [
                        0.213 + 0.787 * s,
                        0.715 - 0.715 * s,
                        0.072 - 0.072 * s,
                        0.0,
                        0.0,
                    ],
                    [
                        0.213 - 0.213 * s,
                        0.715 + 0.285 * s,
                        0.072 - 0.072 * s,
                        0.0,
                        0.0,
                    ],
                    [
                        0.213 - 0.213 * s,
                        0.715 - 0.715 * s,
                        0.072 + 0.928 * s,
                        0.0,
                        0.0,
                    ],
                    [0.0, 0.0, 0.0, 1.0, 0.0],
                ],
                m,
            );
        }
        m
    }

    fn apply_tone_matrices(&self, mut m: [[f32; 5]; 4]) -> [[f32; 5]; 4] {
        if self.invert.abs() >= 1e-4 {
            let p = self.invert.clamp(0.0, 1.0);
            let k = 1.0 - 2.0 * p;
            m = mul_4x5(
                [
                    [k, 0.0, 0.0, 0.0, p],
                    [0.0, k, 0.0, 0.0, p],
                    [0.0, 0.0, k, 0.0, p],
                    [0.0, 0.0, 0.0, 1.0, 0.0],
                ],
                m,
            );
        }
        if (self.brightness - 1.0).abs() >= 1e-4 {
            let b = self.brightness.max(0.0);
            m = mul_4x5(
                [
                    [b, 0.0, 0.0, 0.0, 0.0],
                    [0.0, b, 0.0, 0.0, 0.0],
                    [0.0, 0.0, b, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0, 0.0],
                ],
                m,
            );
        }
        if (self.contrast - 1.0).abs() >= 1e-4 {
            let c = self.contrast.max(0.0);
            let bias = 0.5 * (1.0 - c);
            m = mul_4x5(
                [
                    [c, 0.0, 0.0, 0.0, bias],
                    [0.0, c, 0.0, 0.0, bias],
                    [0.0, 0.0, c, 0.0, bias],
                    [0.0, 0.0, 0.0, 1.0, 0.0],
                ],
                m,
            );
        }
        if (self.opacity - 1.0).abs() >= 1e-4 {
            let o = self.opacity.clamp(0.0, 1.0);
            m[3][3] *= o;
            m[3][4] *= o;
        }
        m
    }

    /// Applies color filter effects to a straight sRGB `[r, g, b, a]` pixel.
    #[inline]
    pub fn apply_rgb_u8(&self, r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
        if !self.has_color_matrix() {
            return [r, g, b, a];
        }
        let m = self.to_color_matrix_4x5();
        apply_matrix_to_straight(&m, r, g, b, a)
    }

    /// Applies color filter effects across a premultiplied RGBA8 pixel slice in place.
    pub fn apply_premultiplied_rgba8_slice(&self, pixels: &mut [u8]) {
        if !self.has_color_matrix() {
            return;
        }
        let m = self.to_color_matrix_4x5();
        for px in pixels.as_chunks_mut::<4>().0 {
            let a = px[3];
            if a == 0 {
                continue;
            }
            let (sr, sg, sb) = if a == 255 {
                (px[0], px[1], px[2])
            } else {
                let inv_a = 255.0 / a as f32;
                (
                    (px[0] as f32 * inv_a).round().clamp(0.0, 255.0) as u8,
                    (px[1] as f32 * inv_a).round().clamp(0.0, 255.0) as u8,
                    (px[2] as f32 * inv_a).round().clamp(0.0, 255.0) as u8,
                )
            };
            let [nr, ng, nb, na] = apply_matrix_to_straight(&m, sr, sg, sb, a);
            if na == 255 {
                px[0] = nr;
                px[1] = ng;
                px[2] = nb;
                px[3] = 255;
            } else {
                let af = na as f32 / 255.0;
                px[0] = (nr as f32 * af).round().clamp(0.0, 255.0) as u8;
                px[1] = (ng as f32 * af).round().clamp(0.0, 255.0) as u8;
                px[2] = (nb as f32 * af).round().clamp(0.0, 255.0) as u8;
                px[3] = na;
            }
        }
    }
}

impl Default for CssFilterEffect {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[inline]
fn apply_matrix_to_straight(m: &[[f32; 5]; 4], r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;
    let af = a as f32 / 255.0;
    let eval = |row: &[f32; 5]| -> u8 {
        let v = row[0] * rf + row[1] * gf + row[2] * bf + row[3] * af + row[4];
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    [eval(&m[0]), eval(&m[1]), eval(&m[2]), eval(&m[3])]
}

fn mul_4x5(a: [[f32; 5]; 4], b: [[f32; 5]; 4]) -> [[f32; 5]; 4] {
    let mut out = [[0.0_f32; 5]; 4];
    for r in 0..4 {
        for c in 0..4 {
            out[r][c] =
                a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c] + a[r][3] * b[3][c];
        }
        out[r][4] =
            a[r][0] * b[0][4] + a[r][1] * b[1][4] + a[r][2] * b[2][4] + a[r][3] * b[3][4] + a[r][4];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_and_grayscale_match_w3c_srgb() {
        let inv = CssFilterEffect {
            invert: 1.0,
            ..Default::default()
        };
        assert_eq!(inv.apply_rgb_u8(239, 68, 68, 255), [16, 187, 187, 255]);

        let gray = CssFilterEffect {
            grayscale: 1.0,
            ..Default::default()
        };
        let out = gray.apply_rgb_u8(200, 100, 50, 255);
        assert_eq!(out[0], out[1]);
        assert_eq!(out[1], out[2]);
        assert_eq!(out[3], 255);
    }
}
