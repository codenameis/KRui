//! DRM dumb-buffer pixel encoding (`RGB565` Bayer dither vs `XRGB8888`) and sink traits.

use krui_renderer::convert_rgba8_to_rgb565_le_dithered;
use krui_style::DamageRect;
use std::sync::{Arc, Mutex};

/// Pixel format backed by the DRM dumb buffer or `hx8357d` kernel conversion path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrmPixelFormat {
    /// Native 16-bit RGB565 little-endian (`convert_rgba8_to_rgb565_le_dithered`, lowest SPI bandwidth).
    Rgb565,
    /// 32-bit XRGB8888 (`drm_fb_xrgb8888_to_rgb565` kernel conversion fallback).
    Xrgb8888,
}

/// Metadata + encoded byte length recorded for a single submitted `DIRTYFB` commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrmFrameCommit {
    /// Clamped damaged region presented in this commit.
    pub rect: DamageRect,
    /// Active buffer pixel format.
    pub format: DrmPixelFormat,
    /// Byte length encoded for the damaged rectangle.
    pub payload_bytes: usize,
}

/// Platform sink receiving encoded damaged scanlines for DRM DumbBuffer + `DIRTYFB` presentation.
pub trait DrmPresentSink {
    /// Presents a single `rect` slice of encoded pixel bytes (`Rgb565` LE or `Xrgb8888` LE).
    fn commit_damage(
        &mut self,
        rect: DamageRect,
        format: DrmPixelFormat,
        encoded_rect_bytes: &[u8],
    ) -> Result<(), String>;
}

/// Encodes only `rect` out of a full-frame `rgba8` (`fb_w * fb_h * 4`) buffer in `format` into `out`.
pub fn encode_damage_rect(
    rgba8: &[u8],
    fb_w: u32,
    fb_h: u32,
    rect: DamageRect,
    format: DrmPixelFormat,
    out: &mut Vec<u8>,
) -> usize {
    if rect.width == 0 || rect.height == 0 || fb_w == 0 || fb_h == 0 {
        out.clear();
        return 0;
    }
    let bpp = match format {
        DrmPixelFormat::Rgb565 => 2_usize,
        DrmPixelFormat::Xrgb8888 => 4_usize,
    };
    let needed = (rect.width as usize)
        .saturating_mul(rect.height as usize)
        .saturating_mul(bpp);
    out.resize(needed, 0);

    let x0 = rect.x.min(fb_w);
    let y0 = rect.y.min(fb_h);
    let w = rect.width.min(fb_w.saturating_sub(x0));
    let h = rect.height.min(fb_h.saturating_sub(y0));
    if w == 0 || h == 0 {
        out.clear();
        return 0;
    }
    match format {
        DrmPixelFormat::Rgb565 => convert_rgba8_to_rgb565_le_dithered(
            rgba8,
            fb_w,
            fb_h,
            DamageRect::new(x0, y0, w, h),
            true,
            out,
        ),
        DrmPixelFormat::Xrgb8888 => encode_rect_xrgb8888(rgba8, fb_w, x0, y0, w, h, out),
    }
}

fn encode_rect_xrgb8888(
    rgba8: &[u8],
    fb_w: u32,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    out: &mut [u8],
) -> usize {
    let mut written = 0_usize;
    for row in 0..h {
        let src_row_px = ((y0 + row) * fb_w + x0) as usize;
        for col in 0..w as usize {
            let s = (src_row_px + col) * 4;
            if s + 3 < rgba8.len() && written + 3 < out.len() {
                // DRM XRGB8888 little-endian byte order: [B, G, R, X]
                out[written] = rgba8[s + 2];
                out[written + 1] = rgba8[s + 1];
                out[written + 2] = rgba8[s];
                out[written + 3] = 0xFF;
                written += 4;
            }
        }
    }
    written
}

/// Thread-safe mock DRM sink recording every `DIRTYFB` commit for automated verification.
#[derive(Clone, Default)]
pub struct MockDrmSink {
    log: Arc<Mutex<Vec<DrmFrameCommit>>>,
}

impl MockDrmSink {
    /// Creates an empty shareable mock sink.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a clone of the shared commit log handle.
    pub fn log_handle(&self) -> Arc<Mutex<Vec<DrmFrameCommit>>> {
        Arc::clone(&self.log)
    }

    /// Returns a snapshot of recorded commits so far.
    pub fn commits_snapshot(&self) -> Vec<DrmFrameCommit> {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl DrmPresentSink for MockDrmSink {
    fn commit_damage(
        &mut self,
        rect: DamageRect,
        format: DrmPixelFormat,
        encoded_rect_bytes: &[u8],
    ) -> Result<(), String> {
        self.log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(DrmFrameCommit {
                rect,
                format,
                payload_bytes: encoded_rect_bytes.len(),
            });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_damage_rect_produces_expected_rgb565_and_xrgb8888_sizes() {
        let rgba = vec![128_u8; 40 * 20 * 4];
        let mut buf = Vec::new();
        let rect = DamageRect::new(5, 4, 10, 6);
        let n565 = encode_damage_rect(&rgba, 40, 20, rect, DrmPixelFormat::Rgb565, &mut buf);
        assert_eq!(n565, 10 * 6 * 2);
        let n8888 = encode_damage_rect(&rgba, 40, 20, rect, DrmPixelFormat::Xrgb8888, &mut buf);
        assert_eq!(n8888, 10 * 6 * 4);
    }
}
