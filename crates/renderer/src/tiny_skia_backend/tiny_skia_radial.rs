//! CSS `radial-gradient(...)` rasterization parity for `TinySkiaRenderer`.

use super::tiny_skia_shapes::to_skia_color;
use krui_core::ResolvedRadialGradient;
use krui_layout::LayoutRect;

/// Fills `outer_path` with a premultiplied elliptical/circular radial gradient matching wgpu SDF.
pub(super) fn fill_radial_gradient_skia(
    pixmap: &mut tiny_skia::Pixmap,
    outer_path: &tiny_skia::Path,
    rect: LayoutRect,
    rg: ResolvedRadialGradient,
    opacity: f32,
    skia_tx: tiny_skia::Transform,
    mask: Option<&tiny_skia::Mask>,
) {
    let cx = rect.x + 0.5 * rect.width + rg.center_dx;
    let cy = rect.y + 0.5 * rect.height + rg.center_dy;
    let rx = rg.radius_x.max(1e-3);
    let ry = rg.radius_y.max(1e-3);
    let p0 = rg.inner_stop.clamp(0.0, 1.0);
    let p1 = rg.outer_stop.clamp(p0, 1.0);
    let inner_col = to_skia_color(rg.inner_color, opacity);
    let outer_col = to_skia_color(rg.outer_color, opacity);
    let mut stops = Vec::with_capacity(4);
    if p0 > 0.0 {
        stops.push(tiny_skia::GradientStop::new(0.0, inner_col));
    }
    stops.push(tiny_skia::GradientStop::new(p0, inner_col));
    stops.push(tiny_skia::GradientStop::new(p1, outer_col));
    if p1 < 1.0 {
        stops.push(tiny_skia::GradientStop::new(1.0, outer_col));
    }
    let unit_origin = tiny_skia::Point::from_xy(0.0, 0.0);
    let grad_tx = tiny_skia::Transform::from_scale(rx, ry).post_translate(cx, cy);
    let Some(shader) = tiny_skia::RadialGradient::new(
        unit_origin,
        unit_origin,
        1.0,
        stops,
        tiny_skia::SpreadMode::Pad,
        grad_tx,
    ) else {
        return;
    };
    let paint = tiny_skia::Paint {
        shader,
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
