//! Non-layout [`PaintStyle`] diffing and channel-wise compositor transition interpolation.

use crate::affine::Transform2D;
use crate::color::Color;
use crate::gradient_radial::RadialGradient;
use crate::layout_types::CornerRadii;
use crate::paint_types::{BoxShadow, LinearGradient, PaintStyle};
use crate::shadow_stack::ShadowStack;
use crate::standalone_transform::StandaloneTransform;
use crate::transform3d::lerp_transform_list;
use crate::transition::{TransitionClause, TransitionMask, TransitionSpec};

/// Linearly interpolates between two `f32` values (unclamped `t` preserves spring overshoot).
#[inline]
pub fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Linearly interpolates RGBA color channels using premultiplied alpha and
/// preserving visible RGB endpoints across transparent transitions.
#[inline]
pub fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let clamp_u8 = |x: f32| x.round().clamp(0.0, 255.0) as u8;
    if a.a == 0 && b.a == 0 {
        return Color::TRANSPARENT;
    }
    if a.a == 0 {
        return Color::rgba(b.r, b.g, b.b, clamp_u8((b.a as f32) * t));
    }
    if b.a == 0 {
        return Color::rgba(a.r, a.g, a.b, clamp_u8((a.a as f32) * (1.0 - t)));
    }
    let alpha_a = a.a as f32 / 255.0;
    let alpha_b = b.a as f32 / 255.0;
    let out_alpha = lerp_f32(alpha_a, alpha_b, t).clamp(0.0, 1.0);
    if out_alpha <= 1e-6 {
        return Color::TRANSPARENT;
    }
    Color::rgba(
        clamp_u8(lerp_f32(a.r as f32 * alpha_a, b.r as f32 * alpha_b, t) / out_alpha),
        clamp_u8(lerp_f32(a.g as f32 * alpha_a, b.g as f32 * alpha_b, t) / out_alpha),
        clamp_u8(lerp_f32(a.b as f32 * alpha_a, b.b as f32 * alpha_b, t) / out_alpha),
        clamp_u8(out_alpha * 255.0),
    )
}

/// Linearly interpolates a 2D transform (`translate`, `scale`, `rotate_deg`, `skew`).
#[inline]
pub fn lerp_transform(a: Transform2D, b: Transform2D, t: f32) -> Transform2D {
    let matrix_override = match (a.matrix_override, b.matrix_override) {
        (Some(ma), Some(mb)) => Some([
            lerp_f32(ma[0], mb[0], t),
            lerp_f32(ma[1], mb[1], t),
            lerp_f32(ma[2], mb[2], t),
            lerp_f32(ma[3], mb[3], t),
            lerp_f32(ma[4], mb[4], t),
            lerp_f32(ma[5], mb[5], t),
        ]),
        (Some(ma), None) if t < 0.5 => Some(ma),
        (None, Some(mb)) if t >= 0.5 => Some(mb),
        _ => None,
    };
    Transform2D {
        translate_x: lerp_f32(a.translate_x, b.translate_x, t),
        translate_y: lerp_f32(a.translate_y, b.translate_y, t),
        scale_x: lerp_f32(a.scale_x, b.scale_x, t),
        scale_y: lerp_f32(a.scale_y, b.scale_y, t),
        rotate_deg: lerp_f32(a.rotate_deg, b.rotate_deg, t),
        skew_x_deg: lerp_f32(a.skew_x_deg, b.skew_x_deg, t),
        skew_y_deg: lerp_f32(a.skew_y_deg, b.skew_y_deg, t),
        matrix_override,
    }
}

fn scale_shadow(s: BoxShadow, geom_scale: f32, alpha_scale: f32) -> BoxShadow {
    BoxShadow {
        offset_x: s.offset_x * geom_scale,
        offset_y: s.offset_y * geom_scale,
        blur_radius: (s.blur_radius * geom_scale).max(0.0),
        spread_radius: s.spread_radius * geom_scale,
        color: Color::rgba(
            s.color.r,
            s.color.g,
            s.color.b,
            ((s.color.a as f32) * alpha_scale).round() as u8,
        ),
        inset: s.inset,
    }
}

/// Interpolates optional `BoxShadow` smoothly (`None <-> Some` fades radius & alpha cleanly).
pub fn lerp_opt_shadow(a: Option<BoxShadow>, b: Option<BoxShadow>, t: f32) -> Option<BoxShadow> {
    match (a, b) {
        (None, None) => None,
        (Some(sa), None) => (t < 0.995)
            .then(|| scale_shadow(sa, (1.0 - t).clamp(0.0, 1.0), (1.0 - t).clamp(0.0, 1.0))),
        (None, Some(sb)) => {
            (t > 0.005).then(|| scale_shadow(sb, t.clamp(0.0, 1.5), t.clamp(0.0, 1.0)))
        }
        (Some(sa), Some(sb)) => Some(BoxShadow {
            offset_x: lerp_f32(sa.offset_x, sb.offset_x, t),
            offset_y: lerp_f32(sa.offset_y, sb.offset_y, t),
            blur_radius: lerp_f32(sa.blur_radius, sb.blur_radius, t).max(0.0),
            spread_radius: lerp_f32(sa.spread_radius, sb.spread_radius, t),
            color: lerp_color(sa.color, sb.color, t),
            inset: if t < 0.5 { sa.inset } else { sb.inset },
        }),
    }
}

/// Returns `true` if `from` and `to` differ on any property included in `mask`.
pub fn paint_differs_on_mask(from: &PaintStyle, to: &PaintStyle, mask: TransitionMask) -> bool {
    (mask.contains(TransitionMask::TRANSFORM)
        && (from.transform != to.transform
            || from.transform_list != to.transform_list
            || from.standalone_transform != to.standalone_transform))
        || (mask.contains(TransitionMask::OPACITY) && (from.opacity - to.opacity).abs() > 1e-4)
        || (mask.contains(TransitionMask::BG_COLOR) && from.background_color != to.background_color)
        || (mask.contains(TransitionMask::TEXT_COLOR) && from.text_color != to.text_color)
        || (mask.contains(TransitionMask::BORDER_COLOR)
            && (from.border_color != to.border_color
                || from.border_side_colors != to.border_side_colors))
        || (mask.contains(TransitionMask::BOX_SHADOW)
            && (from.box_shadow != to.box_shadow
                || from.secondary_shadow != to.secondary_shadow
                || from.shadow_stack != to.shadow_stack))
        || (mask.contains(TransitionMask::BACKDROP_FILTER)
            && (from.backdrop_blur_px - to.backdrop_blur_px).abs() > 1e-3)
        || (mask.contains(TransitionMask::BORDER_RADIUS)
            && ((from.border_radius - to.border_radius).abs() > 1e-3
                || from.effective_corner_radii() != to.effective_corner_radii()))
        || (mask.contains(TransitionMask::BORDER_WIDTH)
            && ((from.border_width - to.border_width).abs() > 1e-3
                || (from.border_bottom_width - to.border_bottom_width).abs() > 1e-3))
        || (mask.contains(TransitionMask::OUTLINE)
            && (from.outline_color != to.outline_color
                || from.border_bottom_color != to.border_bottom_color
                || (from.outline_width - to.outline_width).abs() > 1e-3
                || (from.outline_offset - to.outline_offset).abs() > 1e-3))
        || (mask.contains(TransitionMask::RADIAL_GRADIENT)
            && (from.radial_gradient != to.radial_gradient
                || from.background_gradient != to.background_gradient
                || from.bg_size_scale != to.bg_size_scale
                || from.bg_position != to.bg_position))
}

fn lerp_opt_linear(
    a: Option<LinearGradient>,
    b: Option<LinearGradient>,
    t: f32,
) -> Option<LinearGradient> {
    match (a, b) {
        (None, None) => None,
        (Some(ga), None) => (t < 0.995).then(|| LinearGradient {
            start_color: lerp_color(ga.start_color, Color::TRANSPARENT, t),
            end_color: lerp_color(ga.end_color, Color::TRANSPARENT, t),
            mid_color: ga.mid_color.map(|m| lerp_color(m, Color::TRANSPARENT, t)),
            ..ga
        }),
        (None, Some(gb)) => (t > 0.005).then(|| LinearGradient {
            start_color: lerp_color(Color::TRANSPARENT, gb.start_color, t),
            end_color: lerp_color(Color::TRANSPARENT, gb.end_color, t),
            mid_color: gb.mid_color.map(|m| lerp_color(Color::TRANSPARENT, m, t)),
            ..gb
        }),
        (Some(ga), Some(gb)) => {
            let mid = match (ga.mid_color, gb.mid_color) {
                (Some(ma), Some(mb)) => Some(lerp_color(ma, mb, t)),
                (Some(ma), None) => Some(lerp_color(
                    ma,
                    lerp_color(gb.start_color, gb.end_color, 0.5),
                    t,
                )),
                (None, Some(mb)) => Some(lerp_color(
                    lerp_color(ga.start_color, ga.end_color, 0.5),
                    mb,
                    t,
                )),
                (None, None) => None,
            };
            Some(LinearGradient {
                angle_deg: lerp_f32(ga.angle_deg, gb.angle_deg, t),
                start_color: lerp_color(ga.start_color, gb.start_color, t),
                end_color: lerp_color(ga.end_color, gb.end_color, t),
                mid_color: mid,
                stop_positions: [
                    lerp_f32(ga.stop_positions[0], gb.stop_positions[0], t).clamp(0.0, 1.0),
                    lerp_f32(ga.stop_positions[1], gb.stop_positions[1], t).clamp(0.0, 1.0),
                    lerp_f32(ga.stop_positions[2], gb.stop_positions[2], t).clamp(0.0, 1.0),
                ],
            })
        }
    }
}

fn lerp_opt_radial(
    a: Option<RadialGradient>,
    b: Option<RadialGradient>,
    t: f32,
) -> Option<RadialGradient> {
    match (a, b) {
        (None, None) => None,
        (Some(ra), None) => (t < 0.995).then(|| RadialGradient {
            inner_color: lerp_color(ra.inner_color, Color::TRANSPARENT, t),
            outer_color: lerp_color(ra.outer_color, Color::TRANSPARENT, t),
            ..ra
        }),
        (None, Some(rb)) => (t > 0.005).then(|| RadialGradient {
            inner_color: lerp_color(Color::TRANSPARENT, rb.inner_color, t),
            outer_color: lerp_color(Color::TRANSPARENT, rb.outer_color, t),
            ..rb
        }),
        (Some(ra), Some(rb)) => Some(RadialGradient {
            center_x: if t < 0.5 { ra.center_x } else { rb.center_x },
            center_y: if t < 0.5 { ra.center_y } else { rb.center_y },
            is_circle: if t < 0.5 { ra.is_circle } else { rb.is_circle },
            size_scale_x: lerp_f32(ra.size_scale_x, rb.size_scale_x, t).max(0.0),
            size_scale_y: lerp_f32(ra.size_scale_y, rb.size_scale_y, t).max(0.0),
            inner_color: lerp_color(ra.inner_color, rb.inner_color, t),
            outer_color: lerp_color(ra.outer_color, rb.outer_color, t),
            inner_stop: lerp_f32(ra.inner_stop, rb.inner_stop, t).clamp(0.0, 1.0),
            outer_stop: lerp_f32(ra.outer_stop, rb.outer_stop, t).clamp(0.0, 1.0),
        }),
    }
}

/// Samples a bit-masked linear interpolation between `from` and `to` directly into `out`.
pub fn apply_masked_lerp(
    from: &PaintStyle,
    to: &PaintStyle,
    mask: TransitionMask,
    eased_t: f32,
    out: &mut PaintStyle,
) {
    let clamped_t = eased_t.clamp(0.0, 1.0);
    if mask.contains(TransitionMask::TRANSFORM) {
        out.transform = lerp_transform(from.transform, to.transform, eased_t);
        out.transform_list = lerp_transform_list(
            from.transform_list,
            to.transform_list,
            100.0,
            100.0,
            eased_t,
        );
        out.standalone_transform =
            StandaloneTransform::lerp(from.standalone_transform, to.standalone_transform, eased_t);
    }
    if mask.contains(TransitionMask::OPACITY) {
        out.opacity = lerp_f32(from.opacity, to.opacity, clamped_t).clamp(0.0, 1.0);
    }
    if mask.contains(TransitionMask::BG_COLOR) {
        out.background_color = lerp_color(from.background_color, to.background_color, clamped_t);
    }
    if mask.contains(TransitionMask::TEXT_COLOR) {
        out.text_color = lerp_color(from.text_color, to.text_color, clamped_t);
    }
    if mask.contains(TransitionMask::BORDER_COLOR) {
        out.border_color = lerp_color(from.border_color, to.border_color, clamped_t);
        if from.border_side_colors.is_some() || to.border_side_colors.is_some() {
            let fa = from.border_side_colors.unwrap_or([from.border_color; 4]);
            let ta = to.border_side_colors.unwrap_or([to.border_color; 4]);
            out.border_side_colors = Some([
                lerp_color(fa[0], ta[0], clamped_t),
                lerp_color(fa[1], ta[1], clamped_t),
                lerp_color(fa[2], ta[2], clamped_t),
                lerp_color(fa[3], ta[3], clamped_t),
            ]);
        }
    }
    apply_border_and_effects_lerp(from, to, mask, clamped_t, out);
}

fn apply_border_and_effects_lerp(
    from: &PaintStyle,
    to: &PaintStyle,
    mask: TransitionMask,
    clamped_t: f32,
    out: &mut PaintStyle,
) {
    if mask.contains(TransitionMask::BOX_SHADOW) {
        out.box_shadow = lerp_opt_shadow(from.box_shadow, to.box_shadow, clamped_t);
        out.secondary_shadow =
            lerp_opt_shadow(from.secondary_shadow, to.secondary_shadow, clamped_t);
        out.shadow_stack = ShadowStack::lerp(from.shadow_stack, to.shadow_stack, clamped_t);
    }
    if mask.contains(TransitionMask::BACKDROP_FILTER) {
        out.backdrop_blur_px =
            lerp_f32(from.backdrop_blur_px, to.backdrop_blur_px, clamped_t).max(0.0);
    }
    if mask.contains(TransitionMask::BORDER_RADIUS) {
        let (a, b) = (from.effective_corner_radii(), to.effective_corner_radii());
        let cr = CornerRadii {
            top_left: lerp_f32(a.top_left, b.top_left, clamped_t).max(0.0),
            top_right: lerp_f32(a.top_right, b.top_right, clamped_t).max(0.0),
            bottom_right: lerp_f32(a.bottom_right, b.bottom_right, clamped_t).max(0.0),
            bottom_left: lerp_f32(a.bottom_left, b.bottom_left, clamped_t).max(0.0),
        };
        out.corner_radii = cr;
        out.border_radius = cr.max_radius();
    }
    if mask.contains(TransitionMask::BORDER_WIDTH) {
        let bw = lerp_f32(from.border_width, to.border_width, clamped_t).max(0.0);
        out.border_width = bw;
        out.raw_border_width = bw;
        if bw > 0.0 {
            out.border_style_mask = from.border_style_mask | to.border_style_mask;
        }
        out.border_bottom_width =
            lerp_f32(from.border_bottom_width, to.border_bottom_width, clamped_t).max(0.0);
    }
    if mask.contains(TransitionMask::OUTLINE) {
        out.outline_color = lerp_color(from.outline_color, to.outline_color, clamped_t);
        out.border_bottom_color =
            lerp_color(from.border_bottom_color, to.border_bottom_color, clamped_t);
        out.outline_width = lerp_f32(from.outline_width, to.outline_width, clamped_t).max(0.0);
        out.outline_offset = lerp_f32(from.outline_offset, to.outline_offset, clamped_t);
    }
    if mask.contains(TransitionMask::RADIAL_GRADIENT) {
        apply_radial_lerp(from, to, clamped_t, out);
    }
}

fn apply_radial_lerp(from: &PaintStyle, to: &PaintStyle, clamped_t: f32, out: &mut PaintStyle) {
    out.radial_gradient = lerp_opt_radial(from.radial_gradient, to.radial_gradient, clamped_t);
    out.background_gradient =
        lerp_opt_linear(from.background_gradient, to.background_gradient, clamped_t);
    out.bg_size_scale = match (from.bg_size_scale, to.bg_size_scale) {
        (Some(sa), Some(sb)) => Some((
            lerp_f32(sa.0, sb.0, clamped_t).max(0.0),
            lerp_f32(sa.1, sb.1, clamped_t).max(0.0),
        )),
        (Some(sa), None) => Some((
            lerp_f32(sa.0, 1.0, clamped_t).max(0.0),
            lerp_f32(sa.1, 1.0, clamped_t).max(0.0),
        )),
        (None, Some(sb)) => Some((
            lerp_f32(1.0, sb.0, clamped_t).max(0.0),
            lerp_f32(1.0, sb.1, clamped_t).max(0.0),
        )),
        (None, None) => None,
    };
    out.bg_position = if clamped_t < 0.5 {
        from.bg_position
    } else {
        to.bg_position
    };
}

/// Samples a multi-clause [`TransitionSpec`] at `elapsed_ms`, applying each clause's own delay and curve.
pub fn sample_paint_clauses(
    from: &PaintStyle,
    to: &PaintStyle,
    spec: TransitionSpec,
    elapsed_ms: f32,
) -> PaintStyle {
    let mut out = *to;
    let clauses = spec.collect_clauses();
    for cl in clauses {
        sample_one_clause(from, to, cl, elapsed_ms, &mut out);
    }
    out
}

fn sample_one_clause(
    from: &PaintStyle,
    to: &PaintStyle,
    cl: TransitionClause,
    elapsed_ms: f32,
    out: &mut PaintStyle,
) {
    let delay = cl.delay_ms as f32;
    let dur = cl.effective_duration_ms();
    if dur <= 0.0 {
        apply_masked_lerp(from, to, cl.mask, 1.0, out);
        return;
    }
    if elapsed_ms <= delay {
        apply_masked_lerp(from, to, cl.mask, 0.0, out);
        return;
    }
    let raw_t = ((elapsed_ms - delay) / dur).clamp(0.0, 1.0);
    let eased = if raw_t >= 1.0 {
        1.0
    } else {
        cl.curve.sample(raw_t)
    };
    apply_masked_lerp(from, to, cl.mask, eased, out);
}

/// Produces an interpolated `PaintStyle` between `from` and `to` for masked properties.
pub fn interpolate_paint(
    from: &PaintStyle,
    to: &PaintStyle,
    mask: TransitionMask,
    eased_t: f32,
) -> PaintStyle {
    let mut out = *to;
    apply_masked_lerp(from, to, mask, eased_t, &mut out);
    out
}

#[inline]
fn cubic_bezier_coord(p1: f32, p2: f32, u: f32) -> f32 {
    let inv = 1.0 - u;
    3.0 * inv * inv * u * p1 + 3.0 * inv * u * u * p2 + u * u * u
}

#[inline]
fn cubic_bezier_deriv(p1: f32, p2: f32, u: f32) -> f32 {
    let inv = 1.0 - u;
    3.0 * inv * inv * p1 + 6.0 * inv * u * (p2 - p1) + 3.0 * u * u * (1.0 - p2)
}

/// Evaluates a CSS `cubic-bezier(x1, y1, x2, y2)` easing curve at `target_x`.
pub fn solve_cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, target_x: f32) -> f32 {
    let (mut lo, mut hi, mut u) = (0.0_f32, 1.0_f32, target_x);
    for _ in 0..8 {
        let err = cubic_bezier_coord(x1, x2, u) - target_x;
        if err.abs() < 1e-5 {
            return cubic_bezier_coord(y1, y2, u);
        }
        if err < 0.0 {
            lo = u;
        } else {
            hi = u;
        }
        let d = cubic_bezier_deriv(x1, x2, u);
        u = if d.abs() > 1e-5 {
            (u - err / d).clamp(lo, hi)
        } else {
            0.5 * (lo + hi)
        };
    }
    for _ in 0..8 {
        let err = cubic_bezier_coord(x1, x2, u) - target_x;
        if err.abs() < 1e-5 {
            break;
        }
        if err < 0.0 {
            lo = u;
        } else {
            hi = u;
        }
        u = 0.5 * (lo + hi);
    }
    cubic_bezier_coord(y1, y2, u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_color_transparent_preserves_visible_rgb_midpoint() {
        let m3_hover = Color::rgb(232, 222, 248);
        let mid_in = lerp_color(Color::TRANSPARENT, m3_hover, 0.5);
        assert_eq!((mid_in.r, mid_in.g, mid_in.b), (232, 222, 248));
        assert!((mid_in.a as i32 - 128).abs() <= 1);

        let mid_out = lerp_color(m3_hover, Color::TRANSPARENT, 0.5);
        assert_eq!((mid_out.r, mid_out.g, mid_out.b), (232, 222, 248));
        assert!((mid_out.a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn interpolate_paint_clamps_color_overshoot_while_keeping_transform_overshoot() {
        let from = PaintStyle {
            background_color: Color::rgb(100, 100, 100),
            ..Default::default()
        };
        let to = PaintStyle {
            background_color: Color::rgb(200, 200, 200),
            transform: Transform2D {
                translate_x: 100.0,
                ..Transform2D::default()
            },
            ..Default::default()
        };
        let out = interpolate_paint(
            &from,
            &to,
            TransitionMask(TransitionMask::TRANSFORM | TransitionMask::BG_COLOR),
            1.15,
        );
        assert_eq!(out.background_color, Color::rgb(200, 200, 200));
        assert!((out.transform.translate_x - 115.0).abs() < 1e-3);
    }
}
