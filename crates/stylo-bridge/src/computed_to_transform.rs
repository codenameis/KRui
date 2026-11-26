//! Conversion from Stylo CSS Transforms Level 1 & Level 2 (`transform`,
//! `transform-origin`, `translate`, `rotate`, `scale`) into KRui transform types.

use app_units::Au;
use krui_style::{
    OriginAxis, StandaloneTransform, Transform2D, TransformLength, TransformListRef, TransformOp,
    TranslateAxis,
};
use style::properties::longhands::backface_visibility::computed_value::T as BackfaceVisibility;
use style::properties::ComputedValues;
use style::values::computed::length_percentage::{LengthPercentage, Unpacked};
use style::values::generics::box_::GenericPerspective;
use style::values::generics::transform::{
    GenericPerspectiveFunction, GenericRotate, GenericScale, GenericTranslate, TransformOperation,
    TransformStyle,
};

const BASIS_PCT_PX: f32 = 100.0;

fn lp_to_transform_length(lp: &LengthPercentage) -> TransformLength {
    match lp.unpack() {
        Unpacked::Length(l) => TransformLength::px(l.px()),
        Unpacked::Percentage(p) => TransformLength::percent(p.0 * 100.0),
        Unpacked::Calc(_) => {
            TransformLength::px(lp.to_pixel_length(Au::from_f32_px(BASIS_PCT_PX)).px())
        }
    }
}

fn lp_to_translate_axis(lp: &LengthPercentage) -> TranslateAxis {
    match lp.unpack() {
        Unpacked::Length(l) => TranslateAxis::Px(l.px()),
        Unpacked::Percentage(p) => TranslateAxis::Percent(p.0 * 100.0),
        Unpacked::Calc(_) => {
            TranslateAxis::Px(lp.to_pixel_length(Au::from_f32_px(BASIS_PCT_PX)).px())
        }
    }
}

fn lp_to_origin_axis(lp: &LengthPercentage) -> OriginAxis {
    match lp.unpack() {
        Unpacked::Length(l) => OriginAxis::Px(l.px()),
        Unpacked::Percentage(p) => OriginAxis::Percent(p.0 * 100.0),
        Unpacked::Calc(_) => OriginAxis::Px(lp.to_pixel_length(Au::from_f32_px(BASIS_PCT_PX)).px()),
    }
}

fn convert_transform_op(
    op: &TransformOperation<
        style::values::computed::Angle,
        style::values::computed::Number,
        style::values::computed::Length,
        style::values::computed::Integer,
        LengthPercentage,
    >,
) -> Option<TransformOp> {
    Some(match op {
        TransformOperation::Matrix(m) => TransformOp::Matrix2d([m.a, m.b, m.c, m.d, m.e, m.f]),
        TransformOperation::Matrix3D(m) => TransformOp::Matrix3d([
            m.m11, m.m12, m.m13, m.m14, m.m21, m.m22, m.m23, m.m24, m.m31, m.m32, m.m33, m.m34,
            m.m41, m.m42, m.m43, m.m44,
        ]),
        TransformOperation::Skew(ax, ay) => TransformOp::Skew(ax.degrees(), ay.degrees()),
        TransformOperation::SkewX(ax) => TransformOp::Skew(ax.degrees(), 0.0),
        TransformOperation::SkewY(ay) => TransformOp::Skew(0.0, ay.degrees()),
        TransformOperation::Translate(x, y) => {
            TransformOp::Translate3d(lp_to_transform_length(x), lp_to_transform_length(y), 0.0)
        }
        TransformOperation::TranslateX(x) => {
            TransformOp::Translate3d(lp_to_transform_length(x), TransformLength::px(0.0), 0.0)
        }
        TransformOperation::TranslateY(y) => {
            TransformOp::Translate3d(TransformLength::px(0.0), lp_to_transform_length(y), 0.0)
        }
        TransformOperation::TranslateZ(z) => {
            TransformOp::Translate3d(TransformLength::px(0.0), TransformLength::px(0.0), z.px())
        }
        TransformOperation::Translate3D(x, y, z) => {
            TransformOp::Translate3d(lp_to_transform_length(x), lp_to_transform_length(y), z.px())
        }
        TransformOperation::Scale(sx, sy) => TransformOp::Scale3d(*sx, *sy, 1.0),
        TransformOperation::ScaleX(sx) => TransformOp::Scale3d(*sx, 1.0, 1.0),
        TransformOperation::ScaleY(sy) => TransformOp::Scale3d(1.0, *sy, 1.0),
        TransformOperation::ScaleZ(sz) => TransformOp::Scale3d(1.0, 1.0, *sz),
        TransformOperation::Scale3D(sx, sy, sz) => TransformOp::Scale3d(*sx, *sy, *sz),
        TransformOperation::Rotate(ang) | TransformOperation::RotateZ(ang) => {
            TransformOp::RotateZ(ang.degrees())
        }
        TransformOperation::RotateX(ang) => TransformOp::RotateX(ang.degrees()),
        TransformOperation::RotateY(ang) => TransformOp::RotateY(ang.degrees()),
        TransformOperation::Rotate3D(x, y, z, ang) => {
            TransformOp::Rotate3d(*x, *y, *z, ang.degrees())
        }
        TransformOperation::Perspective(GenericPerspectiveFunction::Length(l)) => {
            TransformOp::Perspective(l.px())
        }
        _ => return None,
    })
}

/// Extracts `Transform2D`, `TransformListRef`, and `StandaloneTransform` from [`ComputedValues`].
#[must_use]
pub(crate) fn extract_transforms(
    style: &ComputedValues,
) -> (Transform2D, TransformListRef, StandaloneTransform) {
    let b = style.get_box();
    let (translate_x, translate_y) = match &b.translate {
        GenericTranslate::None => (TranslateAxis::ZERO, TranslateAxis::ZERO),
        GenericTranslate::Translate(x, y, _z) => (lp_to_translate_axis(x), lp_to_translate_axis(y)),
    };
    let rotate_deg = match b.rotate {
        GenericRotate::None => 0.0,
        GenericRotate::Rotate(ang) => ang.degrees(),
        GenericRotate::Rotate3D(_x, _y, z, ang) => {
            if z < 0.0 {
                -ang.degrees()
            } else {
                ang.degrees()
            }
        }
    };
    let (scale_x, scale_y) = match b.scale {
        GenericScale::None => (1.0, 1.0),
        GenericScale::Scale(sx, sy, _sz) => (sx, sy),
    };
    let perspective_px = match b.perspective {
        GenericPerspective::None => None,
        GenericPerspective::Length(ref l) => Some(l.px()),
    };
    let standalone = StandaloneTransform {
        translate_x,
        translate_y,
        rotate_deg,
        scale_x,
        scale_y,
        origin_x: lp_to_origin_axis(&b.transform_origin.horizontal),
        origin_y: lp_to_origin_axis(&b.transform_origin.vertical),
        perspective_px,
        perspective_origin_x: lp_to_origin_axis(&b.perspective_origin.horizontal),
        perspective_origin_y: lp_to_origin_axis(&b.perspective_origin.vertical),
        preserve_3d: b.transform_style == TransformStyle::Preserve3d,
        backface_visible: b.backface_visibility == BackfaceVisibility::Visible,
    };
    let ops: Vec<TransformOp> = b
        .transform
        .0
        .iter()
        .filter_map(convert_transform_op)
        .collect();
    let transform_list = TransformListRef::from_ops(&ops);
    (Transform2D::IDENTITY, transform_list, standalone)
}
