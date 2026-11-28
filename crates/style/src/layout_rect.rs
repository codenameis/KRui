//! Axis-aligned logical-pixel bounding rectangle shared across `krui-style`, `krui-ui-types`,
//! `krui-layout`, `krui-core`, and `krui-renderer`.

/// Resolved axis-aligned bounding box in logical pixels after layout.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LayoutRect {
    /// Left edge in logical pixels.
    pub x: f32,
    /// Top edge in logical pixels.
    pub y: f32,
    /// Box width in logical pixels.
    pub width: f32,
    /// Box height in logical pixels.
    pub height: f32,
}

impl LayoutRect {
    /// Returns `true` when `(px, py)` lies inside or on the rectangle.
    pub const fn contains(self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}
