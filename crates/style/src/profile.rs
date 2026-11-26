//! Shared target tier profile, damage tracking, and dynamic slot types.

/// Controls whether `ViewTree` tracks per-primitive bounding rects for partial display refresh
/// or only sets a frame dirty boolean (zero rect overhead for 120 FPS 3D engine hosts).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum DamageTrackingMode {
    /// Only track frame-level dirty state (`krui-engine-host`, desktop `wgpu`).
    #[default]
    DirtyFlagOnly,
    /// Compute exact bounding damage rectangles with union-bbox commit splitting (`kiosk-spi`, Wear CPU).
    BoundingRects {
        /// Split threshold (`10` = `1.0x`, `18` = `1.8x` sum of areas).
        split_area_ratio_x10: u16,
        /// Maximum damage rects emitted per frame before coalescing.
        max_rects_per_commit: u8,
    },
}

/// Integer pixel damage rectangle in logical or physical framebuffer coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct DamageRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl DamageRect {
    /// Creates a pixel damage rectangle.
    #[inline]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns `true` if either dimension is zero.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Returns rectangle area in pixels as `u64`.
    #[inline]
    pub const fn area(self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Returns exclusive right edge (`x + width`, saturating).
    #[inline]
    pub const fn right(self) -> u32 {
        self.x.saturating_add(self.width)
    }

    /// Returns exclusive bottom edge (`y + height`, saturating).
    #[inline]
    pub const fn bottom(self) -> u32 {
        self.y.saturating_add(self.height)
    }

    /// Returns `true` if `self` and `other` overlap with non-empty intersection.
    #[inline]
    pub const fn intersects(self, other: Self) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// Computes the bounding union rectangle containing both `self` and `other`.
    #[inline]
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = self.right().max(other.right());
        let y1 = self.bottom().max(other.bottom());
        Self {
            x: x0,
            y: y0,
            width: x1.saturating_sub(x0),
            height: y1.saturating_sub(y0),
        }
    }

    /// Clamps `self` to `[0..max_w, 0..max_h]`.
    #[inline]
    pub fn clamp_to(self, max_w: u32, max_h: u32) -> Self {
        let x0 = self.x.min(max_w);
        let y0 = self.y.min(max_h);
        let x1 = self.right().min(max_w);
        let y1 = self.bottom().min(max_h);
        Self {
            x: x0,
            y: y0,
            width: x1.saturating_sub(x0),
            height: y1.saturating_sub(y0),
        }
    }
}

/// Target compiler/host rendering tier profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum TierProfile {
    #[default]
    FullGpu,
    CpuKiosk,
}

/// Physical display silhouette exposed on root element via `data-krui-shape`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum DisplayShape {
    #[default]
    Rectangular,
    Round,
}

/// Output color precision class exposed on root element via `data-krui-color-depth`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum ColorDepthBits {
    #[default]
    Bpp24OrMore,
    Bpp16Rgb565,
}

/// Typed host attributes synchronized onto the synthetic `<html>` root node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct RootProfileAttrs {
    pub tier: TierProfile,
    pub shape: DisplayShape,
    pub color_depth: ColorDepthBits,
    pub reduced_motion: bool,
}

/// Identifier of a high-frequency dynamic slot updated without layout/style rebuilds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct DynamicSlotId(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_rect_union_and_intersection_behave_cleanly() {
        let a = DamageRect::new(10, 10, 20, 10);
        let b = DamageRect::new(25, 15, 15, 20);
        assert!(a.intersects(b));
        assert_eq!(a.union(b), DamageRect::new(10, 10, 30, 25));
        let empty = DamageRect::new(0, 0, 0, 10);
        assert!(empty.is_empty());
        assert!(!a.intersects(empty));
        assert_eq!(a.union(empty), a);
    }
}
