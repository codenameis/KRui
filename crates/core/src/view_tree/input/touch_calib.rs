//! 3-point affine touch calibration solver for resistive and SPI kiosk panels (`krui-kms`).

/// Pair of raw touch panel coordinates and expected logical screen coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibrationPair {
    /// Raw hardware coordinates from `evdev` or touch controller.
    pub raw: (f32, f32),
    /// Corresponding logical screen coordinates in `ViewTree` space.
    pub screen: (f32, f32),
}

/// 2x3 affine matrix mapping raw touch coordinates `(rx, ry)` to logical screen coordinates `(sx, sy)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchCalibrationMatrix {
    /// Affine coefficients `[a, b, c, d, e, f]`: `sx = a*rx + b*ry + c`, `sy = d*rx + e*ry + f`.
    pub coeffs: [f32; 6],
}

impl Default for TouchCalibrationMatrix {
    fn default() -> Self {
        Self::identity()
    }
}

impl TouchCalibrationMatrix {
    /// Identity touch mapping (no rotation, scale, or offset).
    pub const fn identity() -> Self {
        Self {
            coeffs: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        }
    }

    /// Computes a 3-point affine calibration matrix, returning `None` if the raw points are collinear.
    pub fn from_three_points(pairs: [CalibrationPair; 3]) -> Option<Self> {
        let [p0, p1, p2] = pairs;
        let det = determinant_3x3(
            [p0.raw.0, p0.raw.1, 1.0],
            [p1.raw.0, p1.raw.1, 1.0],
            [p2.raw.0, p2.raw.1, 1.0],
        );
        if !det.is_finite() || det.abs() <= 1e-4 {
            return None;
        }
        let inv_det = 1.0 / det;
        let [a, b, c] = solve_channel(
            [p0.raw, p1.raw, p2.raw],
            [p0.screen.0, p1.screen.0, p2.screen.0],
            inv_det,
        );
        let [d, e, f] = solve_channel(
            [p0.raw, p1.raw, p2.raw],
            [p0.screen.1, p1.screen.1, p2.screen.1],
            inv_det,
        );
        Some(Self {
            coeffs: [a, b, c, d, e, f],
        })
    }

    /// Transforms raw touch coordinates into logical screen coordinates.
    #[inline]
    pub fn map_point(&self, raw_x: f32, raw_y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.coeffs;
        (a * raw_x + b * raw_y + c, d * raw_x + e * raw_y + f)
    }
}

fn determinant_3x3(r0: [f32; 3], r1: [f32; 3], r2: [f32; 3]) -> f32 {
    r0[0] * (r1[1] * r2[2] - r1[2] * r2[1]) - r0[1] * (r1[0] * r2[2] - r1[2] * r2[0])
        + r0[2] * (r1[0] * r2[1] - r1[1] * r2[0])
}

fn solve_channel(raw: [(f32, f32); 3], target: [f32; 3], inv_det: f32) -> [f32; 3] {
    let col_x = [raw[0].0, raw[1].0, raw[2].0];
    let col_y = [raw[0].1, raw[1].1, raw[2].1];
    let ones = [1.0, 1.0, 1.0];
    let da = determinant_3x3_cols(target, col_y, ones) * inv_det;
    let db = determinant_3x3_cols(col_x, target, ones) * inv_det;
    let dc = determinant_3x3_cols(col_x, col_y, target) * inv_det;
    [da, db, dc]
}

fn determinant_3x3_cols(c0: [f32; 3], c1: [f32; 3], c2: [f32; 3]) -> f32 {
    determinant_3x3(
        [c0[0], c1[0], c2[0]],
        [c0[1], c1[1], c2[1]],
        [c0[2], c1[2], c2[2]],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_point_solver_handles_90deg_rotated_scaled_panel() {
        // Simulates a 480x320 SPI TFT mounted 90 deg rotated over raw 0..4096 ADC range.
        let pairs = [
            CalibrationPair {
                raw: (4000.0, 100.0),
                screen: (0.0, 0.0),
            },
            CalibrationPair {
                raw: (4000.0, 3900.0),
                screen: (480.0, 0.0),
            },
            CalibrationPair {
                raw: (200.0, 100.0),
                screen: (0.0, 320.0),
            },
        ];
        let mat = TouchCalibrationMatrix::from_three_points(pairs).expect("non-collinear");
        let (x, y) = mat.map_point(2100.0, 2000.0);
        assert!((x - 240.0).abs() < 1e-2, "x={x}");
        assert!((y - 160.0).abs() < 1e-2, "y={y}");
    }

    #[test]
    fn collinear_points_return_none() {
        let pairs = [
            CalibrationPair {
                raw: (0.0, 0.0),
                screen: (0.0, 0.0),
            },
            CalibrationPair {
                raw: (100.0, 100.0),
                screen: (50.0, 50.0),
            },
            CalibrationPair {
                raw: (200.0, 200.0),
                screen: (100.0, 100.0),
            },
        ];
        assert!(TouchCalibrationMatrix::from_three_points(pairs).is_none());
    }
}
