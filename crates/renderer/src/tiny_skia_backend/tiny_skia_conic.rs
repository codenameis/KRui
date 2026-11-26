//! CSS `conic-gradient(...)` rasterization parity for `TinySkiaRenderer`.

use krui_core::ResolvedConicGradient;
use krui_layout::LayoutRect;
use krui_style::sample_premul_rgba;
use std::f32::consts::TAU;

const MAX_CONIC_RASTER_DIM: u32 = 2048;

/// Fills `outer_path` with a CSS `conic-gradient(...)` matching the WGSL SDF shader.
pub(super) fn fill_conic_gradient_skia(
    pixmap: &mut tiny_skia::Pixmap,
    outer_path: &tiny_skia::Path,
    rect: LayoutRect,
    cg: ResolvedConicGradient,
    opacity: f32,
    skia_tx: tiny_skia::Transform,
    mask: Option<&tiny_skia::Mask>,
) {
    if rect.width <= 0.0 || rect.height <= 0.0 || opacity <= 0.0 {
        return;
    }
    let scale_x = skia_tx.sx.hypot(skia_tx.kx).max(1.0);
    let scale_y = skia_tx.sy.hypot(skia_tx.ky).max(1.0);
    let pw = ((rect.width * scale_x).ceil() as u32).clamp(2, MAX_CONIC_RASTER_DIM);
    let ph = ((rect.height * scale_y).ceil() as u32).clamp(2, MAX_CONIC_RASTER_DIM);
    let Some(mut ramp_px) = tiny_skia::Pixmap::new(pw, ph) else {
        return;
    };
    let start_rad = cg.from_angle_deg.to_radians();
    let op = opacity.clamp(0.0, 1.0);
    cg.stops.with_stops(|stops| {
        if stops.is_empty() {
            return;
        }
        let period = if cg.repeating {
            stops.last().map_or(1.0, |s| s.position).clamp(1e-4, 1.0)
        } else {
            1.0
        };
        let data = ramp_px.data_mut();
        for iy in 0..ph {
            let ly = ((iy as f32 + 0.5) / ph as f32 - 0.5) * rect.height - cg.center_dy;
            for ix in 0..pw {
                let lx = ((ix as f32 + 0.5) / pw as f32 - 0.5) * rect.width - cg.center_dx;
                let norm = (lx.atan2(-ly) - start_rad).rem_euclid(TAU) / TAU;
                let t = if cg.repeating { norm % period } else { norm };
                let pm = sample_premul_rgba(stops, t);
                let off = ((iy * pw + ix) * 4) as usize;
                data[off] = (pm[0] * op * 255.0).round().clamp(0.0, 255.0) as u8;
                data[off + 1] = (pm[1] * op * 255.0).round().clamp(0.0, 255.0) as u8;
                data[off + 2] = (pm[2] * op * 255.0).round().clamp(0.0, 255.0) as u8;
                data[off + 3] = (pm[3] * op * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    });
    let pat_tx = tiny_skia::Transform::from_scale(rect.width / pw as f32, rect.height / ph as f32)
        .post_translate(rect.x, rect.y);
    let paint = tiny_skia::Paint {
        shader: tiny_skia::Pattern::new(
            ramp_px.as_ref(),
            tiny_skia::SpreadMode::Pad,
            tiny_skia::FilterQuality::Bilinear,
            1.0,
            pat_tx,
        ),
        anti_alias: true,
        ..Default::default()
    };
    pixmap.fill_path(
        outer_path,
        &paint,
        tiny_skia::FillRule::Winding,
        skia_tx,
        mask,
    );
}
