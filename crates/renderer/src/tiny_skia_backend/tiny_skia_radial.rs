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
    let mut stops = Vec::with_capacity(4);
    let mut scale_span = 1.0_f32;
    if !rg.extra_stops.is_empty() {
        rg.extra_stops.with_stops(|raw| {
            let max_p = if rg.repeating {
                raw.last().map_or(1.0, |s| s.position).clamp(1e-4, 1.0)
            } else {
                1.0
            };
            scale_span = max_p;
            for s in raw {
                stops.push(tiny_skia::GradientStop::new(
                    (s.position / max_p).clamp(0.0, 1.0),
                    to_skia_color(s.color, opacity),
                ));
            }
        });
    } else {
        let inner_col = to_skia_color(rg.inner_color, opacity);
        let outer_col = to_skia_color(rg.outer_color, opacity);
        if p0 > 0.0 {
            stops.push(tiny_skia::GradientStop::new(0.0, inner_col));
        }
        stops.push(tiny_skia::GradientStop::new(p0, inner_col));
        stops.push(tiny_skia::GradientStop::new(p1, outer_col));
        if p1 < 1.0 {
            stops.push(tiny_skia::GradientStop::new(1.0, outer_col));
        }
    }
    let unit_origin = tiny_skia::Point::from_xy(0.0, 0.0);
    let grad_tx =
        tiny_skia::Transform::from_scale(rx * scale_span, ry * scale_span).post_translate(cx, cy);
    let spread = if rg.repeating {
        tiny_skia::SpreadMode::Repeat
    } else {
        tiny_skia::SpreadMode::Pad
    };
    let Some(shader) =
        tiny_skia::RadialGradient::new(unit_origin, unit_origin, 1.0, stops, spread, grad_tx)
    else {
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
