//! Conversion from Stylo background gradients/sizing/positioning into KRui [`LinearGradient`],
//! [`RadialGradient`], `bg_size_scale`, and `bg_position`.

use app_units::Au;
use krui_style::{
    Color, ConicGradient, GradientStop, GradientStopsRef, LinearGradient, RadialCoord,
    RadialGradient,
};
use style::color::mix::{mix_many, ColorInterpolationMethod, ColorMixItem};
use style::color::{AbsoluteColor, ColorSpace};
use style::properties::ComputedValues;
use style::values::computed::image::{Gradient, Image, LineDirection};
use style::values::computed::length_percentage::{LengthPercentage, Unpacked};
use style::values::computed::AngleOrPercentage;
use style::values::generics::background::BackgroundSize;
use style::values::generics::color::ColorMixFlags;
use style::values::generics::image::{EndingShape, GradientFlags, GradientItem};
use style::values::generics::length::LengthPercentageOrAuto;
use style::values::specified::position::{HorizontalPositionKeyword, VerticalPositionKeyword};

use crate::computed_to_paint::absolute_color_to_krui;

const BASIS_PCT_PX: f32 = 100.0;

#[derive(Clone, Copy)]
struct ResolvedStop {
    color: Color,
    pos: Option<f32>,
}

/// Result of extracting background gradients, size scale, and position from [`ComputedValues`].
pub(crate) struct ExtractedBackgroundImages {
    pub(crate) linear: Option<LinearGradient>,
    pub(crate) radial: Option<RadialGradient>,
    pub(crate) conic: Option<ConicGradient>,
    pub(crate) bg_size_scale: Option<(f32, f32)>,
    pub(crate) bg_position: Option<(RadialCoord, RadialCoord)>,
}

/// Converts a Stylo `LengthPercentage` into normalized `0.0..=1.0` stop offset.
fn lp_to_stop_fraction(lp: &LengthPercentage) -> f32 {
    match lp.unpack() {
        Unpacked::Percentage(p) => p.0.clamp(0.0, 1.0),
        Unpacked::Length(l) => (l.px() / BASIS_PCT_PX).clamp(0.0, 1.0),
        Unpacked::Calc(_) => {
            (lp.to_pixel_length(Au::from_f32_px(BASIS_PCT_PX)).px() / BASIS_PCT_PX).clamp(0.0, 1.0)
        }
    }
}

fn ap_to_stop_fraction(ap: &AngleOrPercentage) -> f32 {
    match *ap {
        AngleOrPercentage::Percentage(p) => p.0.clamp(0.0, 1.0),
        AngleOrPercentage::Angle(a) => (a.degrees() / 360.0).clamp(0.0, 1.0),
    }
}

/// Converts a Stylo `LengthPercentage` into a KRui [`RadialCoord`].
fn lp_to_radial_coord(lp: &LengthPercentage) -> RadialCoord {
    match lp.unpack() {
        Unpacked::Percentage(p) => RadialCoord::Percent(p.0 * 100.0),
        Unpacked::Length(l) => RadialCoord::Px(l.px()),
        Unpacked::Calc(_) => {
            RadialCoord::Px(lp.to_pixel_length(Au::from_f32_px(BASIS_PCT_PX)).px())
        }
    }
}

fn collect_stops<C, P>(
    items: &[GradientItem<C, P>],
    resolve_color: impl Fn(&C) -> Color,
    resolve_pos: impl Fn(&P) -> f32,
) -> Vec<(Color, f32)> {
    let mut raw: Vec<ResolvedStop> = items
        .iter()
        .filter_map(|it| match it {
            GradientItem::SimpleColorStop(c) => Some(ResolvedStop {
                color: resolve_color(c),
                pos: None,
            }),
            GradientItem::ComplexColorStop { color, position } => Some(ResolvedStop {
                color: resolve_color(color),
                pos: Some(resolve_pos(position)),
            }),
            GradientItem::InterpolationHint(_) => None,
        })
        .collect();
    let count = raw.len();
    if count < 2 {
        return Vec::new();
    }
    if raw[0].pos.is_none() {
        raw[0].pos = Some(0.0);
    }
    if raw[count - 1].pos.is_none() {
        raw[count - 1].pos = Some(1.0);
    }
    fill_missing_stop_positions(&mut raw);
    raw.into_iter()
        .map(|s| (s.color, s.pos.unwrap_or(0.0)))
        .collect()
}

fn fill_missing_stop_positions(stops: &mut [ResolvedStop]) {
    let mut idx = 0;
    while idx < stops.len() {
        if stops[idx].pos.is_some() {
            idx += 1;
            continue;
        }
        let start = idx - 1;
        let mut end = idx;
        while end < stops.len() && stops[end].pos.is_none() {
            end += 1;
        }
        let p0 = stops[start].pos.unwrap_or(0.0);
        let p1 = stops[end].pos.unwrap_or(1.0);
        let spans = (end - start) as f32;
        for (k, slot) in stops[(start + 1)..end].iter_mut().enumerate() {
            slot.pos = Some(p0 + (p1 - p0) * ((k + 1) as f32 / spans));
        }
        idx = end + 1;
    }
}

fn line_direction_deg(dir: LineDirection) -> f32 {
    match dir {
        LineDirection::Angle(a) => a.degrees(),
        LineDirection::Horizontal(HorizontalPositionKeyword::Right) => 90.0,
        LineDirection::Horizontal(HorizontalPositionKeyword::Left) => 270.0,
        LineDirection::Vertical(VerticalPositionKeyword::Top) => 0.0,
        LineDirection::Vertical(VerticalPositionKeyword::Bottom) => 180.0,
        LineDirection::Corner(HorizontalPositionKeyword::Right, VerticalPositionKeyword::Top) => {
            45.0
        }
        LineDirection::Corner(
            HorizontalPositionKeyword::Right,
            VerticalPositionKeyword::Bottom,
        ) => 135.0,
        LineDirection::Corner(HorizontalPositionKeyword::Left, VerticalPositionKeyword::Bottom) => {
            225.0
        }
        LineDirection::Corner(HorizontalPositionKeyword::Left, VerticalPositionKeyword::Top) => {
            315.0
        }
    }
}

fn to_gradient_stops_ref(stops: &[(Color, f32)]) -> GradientStopsRef {
    let list: Vec<GradientStop> = stops
        .iter()
        .map(|&(color, position)| GradientStop { color, position })
        .collect();
    GradientStopsRef::from_stops(&list)
}

fn extract_linear(
    direction: LineDirection,
    interpolation: ColorInterpolationMethod,
    items: &[GradientItem<style::values::computed::Color, LengthPercentage>],
    flags: GradientFlags,
    current_color: &AbsoluteColor,
) -> Option<LinearGradient> {
    let stops = collect_stops(
        items,
        |c| absolute_color_to_krui(&c.resolve_to_absolute(current_color)),
        lp_to_stop_fraction,
    );
    if stops.len() < 2 {
        return None;
    }
    let repeating = flags.contains(GradientFlags::REPEATING);
    let last = stops.len() - 1;
    let extra_stops = if stops.len() > 3 || repeating {
        to_gradient_stops_ref(&stops)
    } else {
        GradientStopsRef::Empty
    };
    let (mid_color, mid_pos) = if stops.len() >= 3 {
        let mid = stops.len() / 2;
        (Some(stops[mid].0), stops[mid].1)
    } else if interpolation.space != ColorSpace::Srgb {
        let abs_endpoints: Vec<AbsoluteColor> = items
            .iter()
            .filter_map(|it| match it {
                GradientItem::SimpleColorStop(c) => Some(c.resolve_to_absolute(current_color)),
                GradientItem::ComplexColorStop { color, .. } => {
                    Some(color.resolve_to_absolute(current_color))
                }
                GradientItem::InterpolationHint(_) => None,
            })
            .collect();
        let mid_c = (abs_endpoints.len() == 2).then(|| {
            let mixed = mix_many(
                interpolation,
                [
                    ColorMixItem::new(abs_endpoints[0], 0.5),
                    ColorMixItem::new(abs_endpoints[1], 0.5),
                ],
                ColorMixFlags::NORMALIZE_WEIGHTS,
            );
            absolute_color_to_krui(&mixed)
        });
        (mid_c, (stops[0].1 + stops[last].1) * 0.5)
    } else {
        (None, (stops[0].1 + stops[last].1) * 0.5)
    };
    Some(LinearGradient {
        angle_deg: line_direction_deg(direction),
        start_color: stops[0].0,
        end_color: stops[last].0,
        mid_color,
        stop_positions: [stops[0].1, mid_pos, stops[last].1],
        extra_stops,
        repeating,
    })
}

fn extract_bg_size_scale(style: &ComputedValues) -> Option<(f32, f32)> {
    let size = style.get_background().background_size.0.first()?;
    match size {
        BackgroundSize::ExplicitSize { width, height } => {
            let sx = match width {
                LengthPercentageOrAuto::LengthPercentage(lp) => match lp.0.unpack() {
                    Unpacked::Percentage(p) => Some(p.0),
                    _ => None,
                },
                LengthPercentageOrAuto::Auto => None,
            };
            let sy = match height {
                LengthPercentageOrAuto::LengthPercentage(lp) => match lp.0.unpack() {
                    Unpacked::Percentage(p) => Some(p.0),
                    _ => None,
                },
                LengthPercentageOrAuto::Auto => None,
            };
            match (sx, sy) {
                (Some(x), Some(y)) => Some((x, y)),
                (Some(x), None) => Some((x, x)),
                (None, Some(y)) => Some((y, y)),
                (None, None) => None,
            }
        }
        _ => None,
    }
}

fn extract_bg_position(style: &ComputedValues) -> Option<(RadialCoord, RadialCoord)> {
    let bg = style.get_background();
    let x = bg.background_position_x.0.first().map(lp_to_radial_coord)?;
    let y = bg.background_position_y.0.first().map(lp_to_radial_coord)?;
    if matches!(x, RadialCoord::Percent(p) if p.abs() <= f32::EPSILON)
        && matches!(y, RadialCoord::Percent(p) if p.abs() <= f32::EPSILON)
    {
        return None;
    }
    Some((x, y))
}

fn extract_radial(
    is_circle: bool,
    position: &style::values::computed::Position,
    items: &[GradientItem<style::values::computed::Color, LengthPercentage>],
    flags: GradientFlags,
    bg_size_scale: Option<(f32, f32)>,
    current_color: &AbsoluteColor,
) -> Option<RadialGradient> {
    let stops = collect_stops(
        items,
        |c| absolute_color_to_krui(&c.resolve_to_absolute(current_color)),
        lp_to_stop_fraction,
    );
    if stops.len() < 2 {
        return None;
    }
    let repeating = flags.contains(GradientFlags::REPEATING);
    let extra_stops = if stops.len() > 2 || repeating {
        to_gradient_stops_ref(&stops)
    } else {
        GradientStopsRef::Empty
    };
    let (sx, sy) = bg_size_scale.unwrap_or((1.0, 1.0));
    Some(RadialGradient {
        center_x: lp_to_radial_coord(&position.horizontal),
        center_y: lp_to_radial_coord(&position.vertical),
        is_circle,
        size_scale_x: sx,
        size_scale_y: sy,
        inner_color: stops[0].0,
        outer_color: stops[stops.len() - 1].0,
        inner_stop: stops[0].1,
        outer_stop: stops[stops.len() - 1].1,
        extra_stops,
        repeating,
    })
}

fn extract_conic(
    angle: style::values::computed::Angle,
    position: &style::values::computed::Position,
    items: &[GradientItem<style::values::computed::Color, AngleOrPercentage>],
    flags: GradientFlags,
    current_color: &AbsoluteColor,
) -> Option<ConicGradient> {
    let stops = collect_stops(
        items,
        |c| absolute_color_to_krui(&c.resolve_to_absolute(current_color)),
        ap_to_stop_fraction,
    );
    (stops.len() >= 2).then(|| ConicGradient {
        from_angle_deg: angle.degrees(),
        center_x: lp_to_radial_coord(&position.horizontal),
        center_y: lp_to_radial_coord(&position.vertical),
        stops: to_gradient_stops_ref(&stops),
        repeating: flags.contains(GradientFlags::REPEATING),
    })
}

/// Extracts linear/radial background gradients, background size scale, and background position.
#[must_use]
pub(crate) fn extract_gradients(
    style: &ComputedValues,
    current_color: &AbsoluteColor,
) -> ExtractedBackgroundImages {
    let bg_size_scale = extract_bg_size_scale(style);
    let bg_position = extract_bg_position(style);
    let (mut linear, mut radial, mut conic) = (None, None, None);
    for img in &*style.get_background().background_image.0 {
        let Image::Gradient(grad) = img else {
            continue;
        };
        match &**grad {
            Gradient::Linear {
                direction,
                color_interpolation_method,
                items,
                flags,
                ..
            } if linear.is_none() => {
                linear = extract_linear(
                    *direction,
                    *color_interpolation_method,
                    items,
                    *flags,
                    current_color,
                );
            }
            Gradient::Radial {
                shape,
                position,
                items,
                flags,
                ..
            } if radial.is_none() => {
                radial = extract_radial(
                    matches!(shape, EndingShape::Circle(_)),
                    position,
                    items,
                    *flags,
                    bg_size_scale,
                    current_color,
                );
            }
            Gradient::Conic {
                angle,
                position,
                items,
                flags,
                ..
            } if conic.is_none() => {
                conic = extract_conic(*angle, position, items, *flags, current_color);
            }
            _ => {}
        }
    }
    ExtractedBackgroundImages {
        linear,
        radial,
        conic,
        bg_size_scale,
        bg_position,
    }
}
