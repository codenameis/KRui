//! CPU fallback desktop window presenter backed by `softbuffer` and `TinySkiaRenderer`.

use crate::session::EngineSession;
use krui_core::{ActiveBackendKind, FrameRenderStats, GpuResourceLimits, PaintScene};
use krui_renderer::{RendererBackend, TinySkiaRenderer, WindowSurfacePresenter};
use std::num::NonZeroU32;
use std::sync::Arc;
use winit::window::Window;

/// Converts premultiplied RGBA8 pixels (`[r, g, b, a]` bytes from `TinySkiaRenderer`) into
/// `softbuffer`'s `0x00RRGGBB` `u32` surface format.
pub fn pack_premultiplied_rgba_to_softbuffer_u32(rgba: &[u8], dst: &mut [u32]) {
    for (&[r, g, b, a], out_px) in rgba.as_chunks::<4>().0.iter().zip(dst.iter_mut()) {
        *out_px = rgba_bytes_to_u32(r, g, b, a);
    }
}

#[inline]
fn rgba_bytes_to_u32(r: u8, g: u8, b: u8, a: u8) -> u32 {
    match a {
        255 => ((r as u32) << 16) | ((g as u32) << 8) | (b as u32),
        0 => 0,
        alpha => {
            let a32 = alpha as u32;
            let half = a32 / 2;
            let ur = ((r as u32 * 255 + half) / a32).min(255);
            let ug = ((g as u32 * 255 + half) / a32).min(255);
            let ub = ((b as u32 * 255 + half) / a32).min(255);
            (ur << 16) | (ug << 8) | ub
        }
    }
}

/// CPU window presenter blitting `TinySkiaRenderer` frames onto a desktop `winit::window::Window`
/// via `softbuffer`.
pub struct SoftbufferPresenter {
    _context: softbuffer::Context<Arc<Window>>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    renderer: TinySkiaRenderer,
    width: u32,
    height: u32,
    configured_size: (u32, u32),
}

impl SoftbufferPresenter {
    /// Initializes a `softbuffer` CPU presenter for `window` at physical dimensions `(width, height)`.
    pub fn try_new(
        window: Arc<Window>,
        width: u32,
        height: u32,
        limits: GpuResourceLimits,
    ) -> Option<Self> {
        let context = softbuffer::Context::new(window.clone()).ok()?;
        let mut surface = softbuffer::Surface::new(&context, window).ok()?;
        let limits = limits.normalized();
        let w = width.clamp(1, limits.max_viewport_dimension);
        let h = height.clamp(1, limits.max_viewport_dimension);
        let (nz_w, nz_h) = (NonZeroU32::new(w)?, NonZeroU32::new(h)?);
        surface.resize(nz_w, nz_h).ok()?;
        let renderer = TinySkiaRenderer::new(w, h, limits);
        Some(Self {
            _context: context,
            surface,
            renderer,
            width: w,
            height: h,
            configured_size: (w, h),
        })
    }

    /// Updates the target physical surface dimensions when the window resizes.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;
        }
    }

    /// Rasterizes `scene` on the CPU via `TinySkiaRenderer`, converts RGBA8 pixels into `0x00RRGGBB`,
    /// and presents the buffer to the native OS window.
    pub fn present_scene(&mut self, scene: &PaintScene) -> Option<FrameRenderStats> {
        let stats = self.renderer.render_scene(scene);
        let src_w = self.renderer.physical_width().max(1);
        let src_h = self.renderer.physical_height().max(1);
        let dst_w = self.width.max(1);
        let dst_h = self.height.max(1);
        if self.configured_size != (dst_w, dst_h) {
            let (nz_w, nz_h) = (NonZeroU32::new(dst_w)?, NonZeroU32::new(dst_h)?);
            self.surface.resize(nz_w, nz_h).ok()?;
            self.configured_size = (dst_w, dst_h);
        }
        let rgba = self.renderer.framebuffer_rgba()?;
        let mut buffer = self.surface.buffer_mut().ok()?;
        blit_rgba_into_surface_buffer(rgba, src_w, src_h, &mut buffer, dst_w, dst_h, scene);
        buffer.present().ok()?;
        Some(stats)
    }
}

fn blit_rgba_into_surface_buffer(
    rgba: &[u8],
    src_w: u32,
    src_h: u32,
    dst: &mut [u32],
    dst_w: u32,
    dst_h: u32,
    scene: &PaintScene,
) {
    if src_w == dst_w && src_h == dst_h {
        pack_premultiplied_rgba_to_softbuffer_u32(rgba, dst);
        return;
    }
    let fill = rgba_bytes_to_u32(
        scene.clear_color.r,
        scene.clear_color.g,
        scene.clear_color.b,
        255,
    );
    dst.fill(fill);
    let copy_w = src_w.min(dst_w) as usize;
    let copy_h = src_h.min(dst_h) as usize;
    for row in 0..copy_h {
        let src_start = (row * src_w as usize) * 4;
        let src_end = src_start + copy_w * 4;
        let dst_start = row * dst_w as usize;
        let dst_end = dst_start + copy_w;
        if src_end <= rgba.len() && dst_end <= dst.len() {
            pack_premultiplied_rgba_to_softbuffer_u32(
                &rgba[src_start..src_end],
                &mut dst[dst_start..dst_end],
            );
        }
    }
}

pub(crate) enum DesktopWindowPresenter {
    Wgpu(Box<WindowSurfacePresenter>),
    Softbuffer(Box<SoftbufferPresenter>),
}

impl DesktopWindowPresenter {
    pub(crate) fn try_create(
        window: Arc<Window>,
        session: &EngineSession,
        width: u32,
        height: u32,
        limits: GpuResourceLimits,
    ) -> Option<Self> {
        if session.renderer.backend_kind() == ActiveBackendKind::Wgpu {
            if let Some(wgpu_presenter) = WindowSurfacePresenter::try_new_with_context(
                window.clone(),
                session.renderer.gpu_context(),
                width,
                height,
                limits,
            ) {
                return Some(Self::Wgpu(Box::new(wgpu_presenter)));
            }
        }
        SoftbufferPresenter::try_new(window, width, height, limits)
            .map(|p| Self::Softbuffer(Box::new(p)))
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        match self {
            Self::Wgpu(p) => p.resize(width, height),
            Self::Softbuffer(p) => p.resize(width, height),
        }
    }

    pub(crate) fn present_scene(&mut self, scene: &PaintScene) -> Option<FrameRenderStats> {
        match self {
            Self::Wgpu(p) => p.present_scene(scene),
            Self::Softbuffer(p) => p.present_scene(scene),
        }
    }
}
