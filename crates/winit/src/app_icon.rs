//! Application window icon decoding and desktop close/tray launch options.

use crate::tray::TrayConfig;
use std::fmt;

/// Maximum pixel dimension allowed for a window or system tray [`AppIcon`].
pub const MAX_ICON_DIMENSION: u32 = 1024;

/// Validation or image-decoding error raised when constructing an [`AppIcon`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppIconError {
    /// Width or height was zero or exceeded [`MAX_ICON_DIMENSION`].
    InvalidDimensions { width: u32, height: u32 },
    /// Raw RGBA byte length did not equal `width * height * 4`.
    BufferLengthMismatch { expected: usize, actual: usize },
    /// Encoded PNG bytes could not be decoded.
    DecodeFailed(String),
}

impl fmt::Display for AppIconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions { width, height } => {
                write!(f, "invalid icon dimensions {width}x{height}")
            }
            Self::BufferLengthMismatch { expected, actual } => {
                write!(f, "expected {expected} RGBA bytes, got {actual}")
            }
            Self::DecodeFailed(msg) => write!(f, "icon decode failed: {msg}"),
        }
    }
}

impl std::error::Error for AppIconError {}

/// Platform-neutral RGBA8 icon bitmap applied to desktop windows and system tray icons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIcon {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

impl AppIcon {
    /// Builds an [`AppIcon`] from tightly packed `width * height * 4` RGBA8 bytes.
    pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Result<Self, AppIconError> {
        if width == 0 || height == 0 || width > MAX_ICON_DIMENSION || height > MAX_ICON_DIMENSION {
            return Err(AppIconError::InvalidDimensions { width, height });
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|px| px.checked_mul(4))
            .ok_or(AppIconError::InvalidDimensions { width, height })?;
        if rgba.len() != expected {
            return Err(AppIconError::BufferLengthMismatch {
                expected,
                actual: rgba.len(),
            });
        }
        Ok(Self {
            rgba,
            width,
            height,
        })
    }

    /// Decodes embedded or loaded PNG bytes into an RGBA8 [`AppIcon`].
    pub fn from_png_bytes(bytes: &[u8]) -> Result<Self, AppIconError> {
        let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
            .map_err(|err| AppIconError::DecodeFailed(err.to_string()))?;
        let rgba_img = decoded.into_rgba8();
        let (width, height) = rgba_img.dimensions();
        Self::from_rgba(rgba_img.into_raw(), width, height)
    }

    /// Returns the icon width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the icon height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the raw RGBA8 pixel buffer.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub(crate) fn to_winit_icon(&self) -> Option<winit::window::Icon> {
        winit::window::Icon::from_rgba(self.rgba.clone(), self.width, self.height).ok()
    }

    #[cfg(feature = "tray")]
    pub(crate) fn to_tray_icon(&self) -> Option<tray_icon::Icon> {
        tray_icon::Icon::from_rgba(self.rgba.clone(), self.width, self.height).ok()
    }
}

/// Controls what happens when the user clicks the native desktop window close button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloseBehavior {
    /// Exits the event loop immediately when the window is closed (default).
    #[default]
    Exit,
    /// Hides the window and keeps the app resident with 0% idle CPU while a tray icon is active.
    HideToTray,
}

/// Window icon, background-tray residency, and close behavior configuration for desktop windows.
#[derive(Debug, Clone, Default)]
pub struct WindowOptions {
    /// Optional window titlebar/taskbar icon.
    pub icon: Option<AppIcon>,
    /// Action taken on OS `CloseRequested`.
    pub close_behavior: CloseBehavior,
    /// Starts the app hidden in the background tray when `true` (window shown later on command).
    pub start_hidden: bool,
    /// Optional OS system tray icon + native context menu configuration.
    pub tray: Option<TrayConfig>,
}

impl WindowOptions {
    /// Sets the desktop window icon.
    pub fn with_icon(mut self, icon: AppIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets how `CloseRequested` is handled (`Exit` or `HideToTray`).
    pub fn with_close_behavior(mut self, close_behavior: CloseBehavior) -> Self {
        self.close_behavior = close_behavior;
        self
    }

    /// Starts the window hidden so the application stays resident in the tray until opened.
    pub fn with_start_hidden(mut self, start_hidden: bool) -> Self {
        self.start_hidden = start_hidden;
        self
    }

    /// Attaches a cross-platform OS system tray icon and native context menu.
    pub fn with_tray(mut self, tray: TrayConfig) -> Self {
        self.tray = Some(tray);
        self
    }
}
