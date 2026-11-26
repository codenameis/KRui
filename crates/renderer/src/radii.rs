//! Shared border-radius normalization and elliptical arc cubics helpers used by both
//! `WgpuRenderer` (`gpu` feature) and `TinySkiaRenderer` (`CPU` / `kiosk`).

/// Center-parameterized arc starting point followed by `<= 4` cubic Bezier control-point triples.
pub(crate) type ArcCubics = ((f32, f32), Vec<[(f32, f32); 3]>);

/// Resolves 4-corner radii `[tl, tr, br, bl]`, falling back to uniform `border_radius`
/// when all per-corner radii are zero.
#[inline]
pub(crate) fn effective_radii_4(border_radius: f32, corner_radii: [f32; 4]) -> [f32; 4] {
    if corner_radii == [0.0; 4] && border_radius > 0.0 {
        let clamped = if border_radius.is_nan() {
            0.0
        } else {
            border_radius.clamp(0.0, 1.0e6)
        };
        [clamped; 4]
    } else {
        corner_radii.map(|r| if r.is_nan() { 0.0 } else { r.clamp(0.0, 1.0e6) })
    }
}

/// Applies W3C CSS Backgrounds & Borders §5.5 proportional corner-overlap normalization.
#[inline]
pub(crate) fn normalize_radii_4(w: f32, h: f32, radii: [f32; 4]) -> [f32; 4] {
    let r = radii.map(|v| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0e6) });
    if w <= 0.0 || h <= 0.0 {
        return [0.0; 4];
    }
    let f = [
        w / (r[0] + r[1]).max(1e-4),
        w / (r[3] + r[2]).max(1e-4),
        h / (r[0] + r[3]).max(1e-4),
        h / (r[1] + r[2]).max(1e-4),
    ]
    .into_iter()
    .fold(1.0_f32, f32::min);
    r.map(|v| v * f)
}

/// Converts a center-parameterized elliptical arc into `<= 4` cubic Bezier segments.
pub(crate) fn flatten_arc_to_cubics(
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    start_rad: f32,
    sweep_rad: f32,
    x_rot: f32,
) -> ArcCubics {
    let (rx, ry) = (rx.abs().max(1e-4), ry.abs().max(1e-4));
    let (sin_r, cos_r) = x_rot.sin_cos();
    let eval_pt = |ang: f32| {
        let (sa, ca) = ang.sin_cos();
        let (lx, ly) = (rx * ca, ry * sa);
        (cx + cos_r * lx - sin_r * ly, cy + sin_r * lx + cos_r * ly)
    };
    let eval_tan = |ang: f32| {
        let (sa, ca) = ang.sin_cos();
        let (dx, dy) = (-rx * sa, ry * ca);
        (cos_r * dx - sin_r * dy, sin_r * dx + cos_r * dy)
    };
    let start_pt = eval_pt(start_rad);
    if sweep_rad.abs() <= 1e-5 {
        return (start_pt, Vec::new());
    }
    let segs = ((sweep_rad.abs() / std::f32::consts::FRAC_PI_2).ceil() as usize).clamp(1, 4);
    let dt = sweep_rad / segs as f32;
    let alpha = (4.0 / 3.0) * (dt * 0.25).tan();
    let mut cubics = Vec::with_capacity(segs);
    for i in 0..segs {
        let t0 = start_rad + i as f32 * dt;
        let t1 = t0 + dt;
        let p0 = eval_pt(t0);
        let d0 = eval_tan(t0);
        let p1 = eval_pt(t1);
        let d1 = eval_tan(t1);
        cubics.push([
            (p0.0 + alpha * d0.0, p0.1 + alpha * d0.1),
            (p1.0 - alpha * d1.0, p1.1 - alpha * d1.1),
            p1,
        ]);
    }
    (start_pt, cubics)
}
