//! Device frame presets (`DeviceFramePreset`), debug overlay flags (`PreviewOverlayFlags`),
//! and wire node inspection summaries (`InspectedNodeWire`) for `krui serve`.

use crate::protocol::KRUI_DEVICE_FRAME_ENV;
use serde::{Deserialize, Serialize};

/// Device frame preset (`--frame iphone-15|pixel-8|rpi-800x480`) for previewing mobile and embedded layouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceFramePreset {
    /// Apple iPhone 15 (`393x852` logical px, `@3x`, Dynamic Island + Home Indicator safe areas).
    Iphone15,
    /// Google Pixel 8 (`412x915` logical px, `@2.625x`, punch-hole status bar + gesture nav safe areas).
    Pixel8,
    /// Raspberry Pi 7-inch touch display (`800x480` logical px, `@1x`, industrial kiosk bezel).
    Rpi800x480,
}

impl DeviceFramePreset {
    /// Parses a CLI `--frame` preset identifier (`iphone-15`, `pixel-8`, `rpi-800x480`).
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "iphone-15" | "iphone15" => Some(Self::Iphone15),
            "pixel-8" | "pixel8" => Some(Self::Pixel8),
            "rpi-800x480" | "rpi" => Some(Self::Rpi800x480),
            _ => None,
        }
    }

    /// Reads the active [`DeviceFramePreset`] from `--frame <name>` CLI args or `KRUI_DEVICE_FRAME`.
    pub fn from_env_or_args() -> Option<Self> {
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--frame" {
                if let Some(val) = args.next() {
                    if let Some(p) = Self::parse(&val) {
                        return Some(p);
                    }
                }
            } else if let Some(val) = arg.strip_prefix("--frame=") {
                if let Some(p) = Self::parse(val) {
                    return Some(p);
                }
            }
        }
        std::env::var(KRUI_DEVICE_FRAME_ENV)
            .ok()
            .and_then(|v| Self::parse(&v))
    }

    /// Human-readable display label for the device frame.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Iphone15 => "iPhone 15 (393x852 @3x)",
            Self::Pixel8 => "Pixel 8 (412x915 @2.625x)",
            Self::Rpi800x480 => "Raspberry Pi Touch (800x480 @1x)",
        }
    }

    /// Inner device screen dimensions `(width, height)` in logical pixels.
    pub const fn screen_size(self) -> (u32, u32) {
        match self {
            Self::Iphone15 => (393, 852),
            Self::Pixel8 => (412, 915),
            Self::Rpi800x480 => (800, 480),
        }
    }

    /// Simulated hardware HiDPI scale factor (`f32`) for this device preset.
    pub const fn scale_factor(self) -> f32 {
        match self {
            Self::Iphone15 => 3.0,
            Self::Pixel8 => 2.625,
            Self::Rpi800x480 => 1.0,
        }
    }

    /// Safe-area insets `(top, right, bottom, left)` in logical pixels.
    pub const fn safe_area_insets(self) -> (f32, f32, f32, f32) {
        match self {
            Self::Iphone15 => (59.0, 0.0, 34.0, 0.0),
            Self::Pixel8 => (48.0, 0.0, 24.0, 0.0),
            Self::Rpi800x480 => (0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Usable application layout area `(width, height)` inside safe-area insets in logical pixels.
    pub const fn safe_area_layout_size(self) -> (u32, u32) {
        let (sw, sh) = self.screen_size();
        let (top, right, bottom, left) = self.safe_area_insets();
        let w = (sw as f32 - left - right).max(1.0) as u32;
        let h = (sh as f32 - top - bottom).max(1.0) as u32;
        (w, h)
    }

    /// Origin `(x, y)` of the usable safe-area content region in outer-window logical coordinates.
    pub const fn content_origin(self) -> (f32, f32) {
        let bezel = self.bezel_padding();
        let (top, _, _, left) = self.safe_area_insets();
        (bezel + left, bezel + top)
    }

    /// Inverse-transforms outer-window logical pointer coordinates `(win_x, win_y)` into
    /// application `ViewTree` coordinates, returning `None` if the pointer lies in the outer bezel
    /// outside the physical screen rectangle.
    pub fn map_window_to_screen_point(self, win_x: f32, win_y: f32) -> Option<(f32, f32)> {
        let bezel = self.bezel_padding();
        let (sw, sh) = self.screen_size();
        let sx = win_x - bezel;
        let sy = win_y - bezel;
        if sx < 0.0 || sy < 0.0 || sx > sw as f32 || sy > sh as f32 {
            return None;
        }
        let (top, _, _, left) = self.safe_area_insets();
        Some((sx - left, sy - top))
    }

    /// Transforms outer-window logical pointer coordinates `(win_x, win_y)` into application
    /// `ViewTree` coordinates without rejecting positions outside the screen bezel (used during active pointer capture).
    pub fn map_window_to_screen_point_unclamped(self, win_x: f32, win_y: f32) -> (f32, f32) {
        let bezel = self.bezel_padding();
        let (top, _, _, left) = self.safe_area_insets();
        (win_x - bezel - left, win_y - bezel - top)
    }

    /// Outer bezel thickness in logical pixels around the screen.
    pub const fn bezel_padding(self) -> f32 {
        match self {
            Self::Iphone15 => 24.0,
            Self::Pixel8 => 22.0,
            Self::Rpi800x480 => 28.0,
        }
    }

    /// Outer window size `(width, height)` including the bezel.
    pub const fn outer_window_size(self) -> (u32, u32) {
        let (sw, sh) = self.screen_size();
        let b = (self.bezel_padding() as u32) * 2;
        (sw + b, sh + b)
    }
}

/// Debug overlay kinds selectable via `set_overlay(relayout|fps|hit)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreviewOverlayKind {
    /// Highlights relayout count and whether the latest frame ran Taffy layout.
    Relayout,
    /// Displays live frame time (`us`) and estimated FPS badge.
    Fps,
    /// Draws bounding boxes around interactive hit-testable nodes (`<button>`, `<input>`).
    Hit,
}

impl PreviewOverlayKind {
    /// Parses `"relayout"`, `"fps"`, or `"hit"`.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "relayout" => Some(Self::Relayout),
            "fps" => Some(Self::Fps),
            "hit" => Some(Self::Hit),
            _ => None,
        }
    }
}

/// Active debug overlay toggle state on a preview session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewOverlayFlags {
    /// Show relayout pass counter and layout dirty indicator.
    pub relayout: bool,
    /// Show frame timing (`us`) and FPS overlay.
    pub fps: bool,
    /// Draw interactive hit-test wireframes on buttons and text inputs.
    pub hit: bool,
}

impl PreviewOverlayFlags {
    /// Sets `kind` to `enabled`.
    pub fn set(&mut self, kind: PreviewOverlayKind, enabled: bool) {
        match kind {
            PreviewOverlayKind::Relayout => self.relayout = enabled,
            PreviewOverlayKind::Fps => self.fps = enabled,
            PreviewOverlayKind::Hit => self.hit = enabled,
        }
    }
}

/// Serialized node summary returned by `PreviewResponse::TreeInspection`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InspectedNodeWire {
    /// Structural path key (`"root/0:div/1:input"`).
    pub path_key: String,
    /// HTML tag name.
    pub tag: String,
    /// Optional HTML `id`.
    pub element_id: Option<String>,
    /// Tree depth.
    pub depth: usize,
    /// World-space bounding box `(x, y, width, height)`.
    pub rect: (f32, f32, f32, f32),
    /// Current node text.
    pub text: String,
    /// 1-based source line of the originating `.krui` opening tag.
    pub source_line: u32,
    /// 1-based source column of the originating `.krui` opening tag.
    pub source_column: u32,
    /// Computed padding `(top, right, bottom, left)` in logical pixels.
    pub padding: (f32, f32, f32, f32),
    /// Bound CSS custom property names (`--krui-*`).
    pub tokens: Vec<String>,
}
