//! Platform-neutral mobile home-screen widget snapshot & interactive hotspot contract.
//!
//! Shared by `krui-android-ffi` (`AppWidgetProvider` + `RemoteViews`) and future
//! iOS `WidgetKit` extensions without depending on OS UI types or `winit`/`wgpu`.

use crate::view_tree::{NodeId, ViewTree};
use krui_style::ColorScheme;

/// Minimum allowed widget display scale factor.
pub const MIN_WIDGET_SCALE: f32 = 1.0;
/// Maximum allowed widget display scale factor.
pub const MAX_WIDGET_SCALE: f32 = 4.0;
/// Maximum pixel dimension allowed on any widget snapshot edge.
pub const MAX_WIDGET_EDGE_PX: u32 = 2048;

/// Coarse OS home-screen widget size category mapped to default logical dp dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WidgetSizeClass {
    /// Compact 2x2 launcher slot (`160x160` dp).
    Small,
    /// Wide 4x2 launcher slot (`320x160` dp).
    #[default]
    Medium,
    /// Large 4x4 launcher slot (`320x320` dp).
    Large,
    /// Tablet/expanded 4x5 launcher slot (`420x360` dp).
    ExtraLarge,
    /// Explicit host-provided logical width and height in dp.
    Custom {
        /// Width in logical dp.
        width_dp: u16,
        /// Height in logical dp.
        height_dp: u16,
    },
}

impl WidgetSizeClass {
    /// Returns the default logical `(width_dp, height_dp)` size for this widget class.
    pub const fn default_dp(self) -> (u32, u32) {
        match self {
            Self::Small => (160, 160),
            Self::Medium => (320, 160),
            Self::Large => (320, 320),
            Self::ExtraLarge => (420, 360),
            Self::Custom {
                width_dp,
                height_dp,
            } => (
                if width_dp == 0 { 1 } else { width_dp as u32 },
                if height_dp == 0 { 1 } else { height_dp as u32 },
            ),
        }
    }
}

/// Host rendering parameters describing a widget surface size, scale, and OS color scheme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidgetRenderConfig {
    /// Coarse launcher size class or custom logical dp dimensions.
    pub size_class: WidgetSizeClass,
    /// Display density scale clamped to `[1.0, 4.0]`.
    pub scale_factor: f32,
    /// Active OS dark/light mode (`ColorScheme::Light` or `ColorScheme::Dark`).
    pub color_scheme: ColorScheme,
}

impl Default for WidgetRenderConfig {
    fn default() -> Self {
        Self {
            size_class: WidgetSizeClass::Medium,
            scale_factor: 1.0,
            color_scheme: ColorScheme::Light,
        }
    }
}

impl WidgetRenderConfig {
    /// Creates a [`WidgetRenderConfig`] with sanitized `scale_factor`.
    pub const fn new(
        size_class: WidgetSizeClass,
        scale_factor: f32,
        color_scheme: ColorScheme,
    ) -> Self {
        Self {
            size_class,
            scale_factor: sanitize_widget_scale(scale_factor),
            color_scheme,
        }
    }

    /// Derives logical dp dimensions from host physical pixel bounds and density scale.
    pub fn from_physical_px(
        width_px: u32,
        height_px: u32,
        scale_factor: f32,
        color_scheme: ColorScheme,
    ) -> Self {
        let scale = sanitize_widget_scale(scale_factor);
        let cw = width_px.clamp(1, MAX_WIDGET_EDGE_PX);
        let ch = height_px.clamp(1, MAX_WIDGET_EDGE_PX);
        let lw = ((cw as f32) / scale).round().clamp(1.0, u16::MAX as f32) as u16;
        let lh = ((ch as f32) / scale).round().clamp(1.0, u16::MAX as f32) as u16;
        Self {
            size_class: WidgetSizeClass::Custom {
                width_dp: lw,
                height_dp: lh,
            },
            scale_factor: scale,
            color_scheme,
        }
    }

    /// Returns sanitized density scale in `[1.0, 4.0]`.
    pub const fn sanitized_scale(&self) -> f32 {
        sanitize_widget_scale(self.scale_factor)
    }

    /// Returns logical `(width, height)` in dp.
    pub const fn logical_size(&self) -> (u32, u32) {
        self.size_class.default_dp()
    }

    /// Returns physical bitmap `(width_px, height_px)` clamped to `[1, MAX_WIDGET_EDGE_PX]`.
    pub fn physical_size(&self) -> (u32, u32) {
        let (lw, lh) = self.logical_size();
        let scale = self.sanitized_scale();
        let pw = ((lw as f32) * scale).round() as u32;
        let ph = ((lh as f32) * scale).round() as u32;
        (
            pw.clamp(1, MAX_WIDGET_EDGE_PX),
            ph.clamp(1, MAX_WIDGET_EDGE_PX),
        )
    }
}

const fn sanitize_widget_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale >= MIN_WIDGET_SCALE {
        if scale > MAX_WIDGET_SCALE {
            MAX_WIDGET_SCALE
        } else {
            scale
        }
    } else {
        MIN_WIDGET_SCALE
    }
}

/// Interactive tap target exported alongside a rendered widget bitmap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidgetHotspot {
    /// Application-defined action ID dispatched when this region is tapped.
    pub action_id: u32,
    /// Logical X origin relative to the widget top-left in dp.
    pub x: f32,
    /// Logical Y origin relative to the widget top-left in dp.
    pub y: f32,
    /// Logical width in dp.
    pub width: f32,
    /// Logical height in dp.
    pub height: f32,
}

impl WidgetHotspot {
    /// Returns `true` when logical point `(lx, ly)` lies inside `[x, x + width) x [y, y + height)`.
    pub fn contains_point(&self, lx: f32, ly: f32) -> bool {
        self.width > 0.0
            && self.height > 0.0
            && lx >= self.x
            && lx < (self.x + self.width)
            && ly >= self.y
            && ly < (self.y + self.height)
    }

    /// Packs `[action_id, x_px, y_px, width_px, height_px]` for compact C-ABI/JNI array transfer.
    pub fn to_physical_i32_tuple(&self, scale_factor: f32) -> [i32; 5] {
        let s = sanitize_widget_scale(scale_factor);
        [
            self.action_id as i32,
            (self.x * s).round().max(0.0) as i32,
            (self.y * s).round().max(0.0) as i32,
            (self.width * s).round().max(0.0) as i32,
            (self.height * s).round().max(0.0) as i32,
        ]
    }
}

/// Associates a mounted [`NodeId`] in a widget's [`ViewTree`] with a stable numeric `action_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetActionBinding {
    /// Target node whose resolved layout rectangle becomes a clickable hotspot.
    pub node_id: NodeId,
    /// Action ID emitted to the widget host when tapped.
    pub action_id: u32,
}

/// Rendered offscreen RGBA8 widget frame together with layout-derived clickable hotspots.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetSnapshot {
    /// Physical bitmap width in pixels.
    pub width_px: u32,
    /// Physical bitmap height in pixels.
    pub height_px: u32,
    /// Logical width in dp.
    pub logical_width: u32,
    /// Logical height in dp.
    pub logical_height: u32,
    /// Sanitized scale factor used to render `rgba8`.
    pub scale_factor: f32,
    /// Monotonically increasing snapshot revision counter.
    pub revision: u64,
    /// Tightly packed RGBA8 bitmap buffer (`width_px * height_px * 4` bytes).
    pub rgba8: Vec<u8>,
    /// Ordered tap targets from back to front.
    pub hotspots: Vec<WidgetHotspot>,
}

impl WidgetSnapshot {
    /// Converts `rgba8` into Android `ARGB_8888` signed 32-bit pixels `(A<<24 | R<<16 | G<<8 | B)`.
    pub fn to_argb8888(&self, out: &mut Vec<i32>) {
        let pixel_count = (self.width_px as usize)
            .saturating_mul(self.height_px as usize)
            .min(self.rgba8.len() / 4);
        out.clear();
        out.reserve(pixel_count);
        for &[r, g, b, a] in self.rgba8[..pixel_count * 4].as_chunks::<4>().0 {
            let argb =
                (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
            out.push(argb as i32);
        }
    }

    /// Hit-tests `hotspots` from front to back in logical dp coordinates and returns matching `action_id`.
    pub fn hit_test_action(&self, logical_x: f32, logical_y: f32) -> Option<u32> {
        self.hotspots
            .iter()
            .rev()
            .find(|spot| spot.contains_point(logical_x, logical_y))
            .map(|spot| spot.action_id)
    }
}

/// Collects valid, positively sized [`WidgetHotspot`] rectangles from current `ViewTree` layouts.
pub fn collect_widget_hotspots(
    tree: &ViewTree,
    bindings: &[WidgetActionBinding],
) -> Vec<WidgetHotspot> {
    let mut hotspots = Vec::with_capacity(bindings.len());
    for binding in bindings {
        if let Some(rect) = tree.node_layout(binding.node_id) {
            if rect.width > 0.0 && rect.height > 0.0 {
                hotspots.push(WidgetHotspot {
                    action_id: binding.action_id,
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                });
            }
        }
    }
    hotspots
}
