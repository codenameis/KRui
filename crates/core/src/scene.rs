//! Platform-agnostic scene primitives, 2D canvas drawing commands, and security path validation.

pub use crate::canvas_2d::*;
use crate::media::ImageSource;
use krui_layout::LayoutRect;
use krui_style::{Affine2, Color};
use krui_text::RasterizedGlyph;
use std::path::{Path, PathBuf};

/// Clamps per-scene GPU/CPU resource usage so adversarial or pathological content
/// degrades gracefully instead of exhausting memory or hanging the driver (Section 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuResourceLimits {
    pub max_viewport_dimension: u32,
    pub max_draw_commands_per_frame: usize,
    pub max_vertices_per_frame: usize,
    pub max_indices_per_frame: usize,
}

impl Default for GpuResourceLimits {
    fn default() -> Self {
        Self {
            max_viewport_dimension: 4096,
            max_draw_commands_per_frame: 16_384,
            max_vertices_per_frame: 262_144,
            max_indices_per_frame: 786_432,
        }
    }
}

impl GpuResourceLimits {
    /// Returns a normalized copy where `max_viewport_dimension >= 1` so `clamp(1, max_viewport_dimension)`
    /// never panics even if a caller passes `0`.
    pub const fn normalized(self) -> Self {
        Self {
            max_viewport_dimension: if self.max_viewport_dimension == 0 {
                1
            } else {
                self.max_viewport_dimension
            },
            max_draw_commands_per_frame: self.max_draw_commands_per_frame,
            max_vertices_per_frame: self.max_vertices_per_frame,
            max_indices_per_frame: self.max_indices_per_frame,
        }
    }
}

/// Extensible event envelope so custom game-engine or 6DoF VR/AR controller events can
/// flow alongside standard desktop/web UI events without breaking changes (Section 3.7).
#[derive(Clone, Debug, PartialEq)]
pub enum ExtensibleEvent<CustomPayload = ()> {
    PointerDown { x: f32, y: f32 },
    PointerMove { x: f32, y: f32 },
    PointerUp { x: f32, y: f32 },
    KeyDown { key: String, shift: bool },
    TextInput(String),
    Custom(CustomPayload),
}

/// Box-relative resolved CSS `radial-gradient(...)` geometry and stop endpoints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedRadialGradient {
    /// Horizontal center offset from the box's geometric center in logical px.
    pub center_dx: f32,
    /// Vertical center offset from the box's geometric center in logical px.
    pub center_dy: f32,
    /// Horizontal outer gradient semi-axis in logical px.
    pub radius_x: f32,
    /// Vertical outer gradient semi-axis in logical px.
    pub radius_y: f32,
    /// Inner stop RGBA color (`0%`).
    pub inner_color: Color,
    /// Outer stop RGBA color (`100%`).
    pub outer_color: Color,
    /// Normalized inner stop position (`0.0..=1.0`).
    pub inner_stop: f32,
    /// Normalized outer stop position (`0.0..=1.0`).
    pub outer_stop: f32,
}

/// A draw item in a frame's `PaintScene`, combining layout bounds with compositor-only
/// `Affine2` transform matrix, `opacity`, and optional ancestor clip rectangle (`overflow: hidden/scroll`).
#[derive(Clone, Debug, PartialEq)]
pub enum DrawPrimitive {
    /// Styled box (`<div>`, `<button>`, input frame, shadow, blur, gradient).
    StyledBox {
        rect: LayoutRect,
        clip: Option<LayoutRect>,
        clip_radius: f32,
        clip_radii: [f32; 4],
        transform: Affine2,
        opacity: f32,
        background_color: Color,
        background_gradient: Option<krui_style::LinearGradient>,
        radial_gradient: Option<ResolvedRadialGradient>,
        border_color: Color,
        border_width: f32,
        border_radius: f32,
        corner_radii: [f32; 4],
        box_shadow: Option<krui_style::BoxShadow>,
        backdrop_blur_px: f32,
    },
    /// Rasterized text glyph run.
    TextGlyphs {
        origin_x: f32,
        origin_y: f32,
        clip: Option<LayoutRect>,
        clip_radius: f32,
        clip_radii: [f32; 4],
        transform: Affine2,
        opacity: f32,
        color: Color,
        glyphs: std::sync::Arc<[RasterizedGlyph]>,
    },
    /// 2D vector command batch (`<canvas>`, `<svg>`, vector controls).
    CanvasBatch {
        viewport: LayoutRect,
        clip: Option<LayoutRect>,
        clip_radius: f32,
        clip_radii: [f32; 4],
        transform: Affine2,
        opacity: f32,
        commands: std::sync::Arc<[CanvasCommand]>,
    },
    /// Raster `<img>` or host `ExternalTexture` draw item with `uv_rect` and `border_radius`.
    Image {
        rect: LayoutRect,
        uv_rect: [f32; 4],
        clip: Option<LayoutRect>,
        clip_radius: f32,
        clip_radii: [f32; 4],
        transform: Affine2,
        opacity: f32,
        border_radius: f32,
        source: ImageSource,
    },
    /// Begins an isolated group-opacity or nested/rotated-clip compositing layer so parent fills
    /// and overlapping children composite cleanly without double-alpha bleed or clip loss.
    PushLayer {
        bounds: LayoutRect,
        clip: Option<LayoutRect>,
        clip_radius: f32,
        clip_radii: [f32; 4],
        local_clip: Option<(LayoutRect, [f32; 4], Affine2)>,
        opacity: f32,
    },
    /// Ends the current isolated group-opacity compositing layer and composites it into its parent.
    PopLayer,
}

impl DrawPrimitive {
    /// Returns the ancestor clip rectangle in logical window coordinates, if any.
    pub fn clip_rect(&self) -> Option<LayoutRect> {
        match self {
            DrawPrimitive::StyledBox { clip, .. }
            | DrawPrimitive::TextGlyphs { clip, .. }
            | DrawPrimitive::CanvasBatch { clip, .. }
            | DrawPrimitive::Image { clip, .. }
            | DrawPrimitive::PushLayer { clip, .. } => *clip,
            DrawPrimitive::PopLayer => None,
        }
    }
}

/// Complete per-frame paint scene submitted to the active `RendererBackend`.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintScene {
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub clear_color: Color,
    pub primitives: Vec<DrawPrimitive>,
}

impl PaintScene {
    /// Creates an empty `PaintScene` sized to `(width, height)` logical pixels.
    pub fn new(width: u32, height: u32, clear_color: Color) -> Self {
        Self {
            width,
            height,
            scale_factor: 1.0,
            clear_color,
            primitives: Vec::with_capacity(64),
        }
    }
}

/// Telemetry returned after rendering a frame (used for CI frame-budget regression gating).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct FrameRenderStats {
    pub backend_kind: ActiveBackendKind,
    pub draw_primitives_submitted: usize,
    pub draw_primitives_clamped: usize,
    pub vertices_generated: usize,
    pub indices_generated: usize,
}

/// Active rendering backend reported in [`FrameRenderStats`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ActiveBackendKind {
    #[default]
    Wgpu,
    TinySkiaCpuFallback,
}

/// Startup backend selection policy (Section 3.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RenderBackendPreference {
    /// Probe `wgpu` hardware/adapter first; fall back automatically to `tiny-skia` if unavailable.
    #[default]
    Auto,
    /// Explicitly require `wgpu`.
    ForceWgpu,
    /// Explicitly force the low-power `tiny-skia` CPU rasterizer (for testing and weak embedded targets).
    ForceTinySkia,
    /// Explicitly request the CPU fallback rasterizer (`TinySkiaRenderer` + `softbuffer` desktop blitting).
    CpuFallback,
}

/// Validates and canonicalizes an external file path (from drag-and-drop or clipboard paste)
/// before any filesystem access is permitted (Section 6).
pub fn validate_external_file_path(untrusted_path: &Path) -> Result<PathBuf, String> {
    let path_str = untrusted_path
        .to_str()
        .ok_or_else(|| "Rejected non-UTF8 external path".to_string())?;
    if path_str.contains('\0') {
        return Err("Rejected external path containing NUL byte".to_string());
    }
    std::fs::canonicalize(untrusted_path)
        .map_err(|e| format!("Failed to canonicalize external path `{path_str}`: {e}"))
}
