//! Extraction of CSS `@keyframes` animations directly from Stylo stylesheets and
//! rule-cascaded `ComputedValues` per stop (resolving `var()`, `calc()`, and Color 4).

use app_units::Au;
use krui_style::{Dimension, KeyframeStop, KeyframesDef, LayoutPatchOp, PaintPatchOp, PaintStyle};
use std::collections::HashSet;
use style::properties::{ComputedValues, LonghandId, PropertyDeclarationBlock};
use style::shared_lock::{Locked, SharedRwLockReadGuard, StylesheetGuards};
use style::stylesheets::keyframes_rule::{KeyframesAnimation, KeyframesRule, KeyframesStepValue};
use style::stylesheets::{CssRule, CustomMediaMap, DocumentStyleSheet, StylesheetInDocument};
use style::stylist::Stylist;
use style::values::generics::length::{
    GenericLengthPercentageOrNormal, GenericMargin, GenericMaxSize, GenericSize,
};
use style::values::generics::position::Inset;

use crate::computed_to_paint::extract_paint_style;
use crate::computed_to_transitions::convert_timing_function;
use crate::dom_node::StyloElement;

const KEYFRAME_BASIS_PX: f32 = 100.0;

fn lp_to_dim(lp: &style::values::computed::LengthPercentage) -> Dimension {
    use style::values::computed::length_percentage::Unpacked;
    match lp.unpack() {
        Unpacked::Percentage(p) => Dimension::Percent(p.0 * 100.0),
        Unpacked::Length(l) => Dimension::Px(l.px()),
        Unpacked::Calc(_) => {
            Dimension::Px(lp.to_pixel_length(Au::from_f32_px(KEYFRAME_BASIS_PX)).px())
        }
    }
}

fn size_to_dim(s: &style::values::computed::Size) -> Dimension {
    match s {
        GenericSize::LengthPercentage(lp) => lp_to_dim(&lp.0),
        _ => Dimension::Auto,
    }
}

fn max_size_to_dim(s: &style::values::computed::MaxSize) -> Dimension {
    match s {
        GenericMaxSize::LengthPercentage(lp) => lp_to_dim(&lp.0),
        GenericMaxSize::None => Dimension::Auto,
        _ => Dimension::Auto,
    }
}

fn margin_to_dim(m: &style::values::computed::Margin) -> Dimension {
    match m {
        GenericMargin::LengthPercentage(lp) => lp_to_dim(lp),
        _ => Dimension::Auto,
    }
}

fn inset_to_dim(i: &style::values::computed::Inset) -> Dimension {
    match i {
        Inset::LengthPercentage(lp) => lp_to_dim(lp),
        _ => Dimension::Auto,
    }
}

fn collect_longhand_ids(
    block: &Locked<PropertyDeclarationBlock>,
    guard: &SharedRwLockReadGuard,
) -> HashSet<LonghandId> {
    let read = block.read_with(guard);
    read.declarations()
        .iter()
        .filter_map(|d| d.id().as_longhand())
        .collect()
}

fn push_paint_ops_for_step(
    ids: &HashSet<LonghandId>,
    paint: &PaintStyle,
    out: &mut Vec<PaintPatchOp>,
) {
    if ids.contains(&LonghandId::BackgroundColor) {
        out.push(PaintPatchOp::BackgroundColor(paint.background_color));
    }
    if ids.contains(&LonghandId::BackgroundImage) {
        if paint.background_gradient.is_some() {
            out.push(PaintPatchOp::BackgroundGradient(paint.background_gradient));
        }
        if paint.radial_gradient.is_some() {
            out.push(PaintPatchOp::RadialGradient(paint.radial_gradient));
        }
    }
    if ids.contains(&LonghandId::BackgroundSize) {
        out.push(PaintPatchOp::BgSizeScale(paint.bg_size_scale));
    }
    if ids.contains(&LonghandId::BackgroundPositionX)
        || ids.contains(&LonghandId::BackgroundPositionY)
    {
        out.push(PaintPatchOp::BgPosition(paint.bg_position));
    }
    if ids.contains(&LonghandId::Color) {
        out.push(PaintPatchOp::TextColor(paint.text_color));
    }
    if ids.contains(&LonghandId::Opacity) {
        out.push(PaintPatchOp::Opacity(paint.opacity));
    }
    if ids.contains(&LonghandId::Transform) {
        out.push(PaintPatchOp::TransformList(paint.transform_list));
    }
    if ids.contains(&LonghandId::Translate) {
        out.push(PaintPatchOp::Translate(
            paint.standalone_transform.translate_x,
            paint.standalone_transform.translate_y,
        ));
    }
    if ids.contains(&LonghandId::Rotate) {
        out.push(PaintPatchOp::RotateDeg(
            paint.standalone_transform.rotate_deg,
        ));
    }
    if ids.contains(&LonghandId::Scale) {
        out.push(PaintPatchOp::Scale(
            paint.standalone_transform.scale_x,
            paint.standalone_transform.scale_y,
        ));
    }
    if ids.contains(&LonghandId::TransformOrigin) {
        out.push(PaintPatchOp::TransformOrigin(
            paint.standalone_transform.origin_x,
            paint.standalone_transform.origin_y,
        ));
    }
    if ids.contains(&LonghandId::BoxShadow) {
        out.push(PaintPatchOp::ShadowStack(paint.shadow_stack));
    }
    if ids.contains(&LonghandId::BackdropFilter) {
        out.push(PaintPatchOp::BackdropBlurPx(paint.backdrop_blur_px));
    }
    push_border_and_outline_ops(ids, paint, out);
}

fn push_border_and_outline_ops(
    ids: &HashSet<LonghandId>,
    paint: &PaintStyle,
    out: &mut Vec<PaintPatchOp>,
) {
    if ids.contains(&LonghandId::BorderTopColor)
        || ids.contains(&LonghandId::BorderRightColor)
        || ids.contains(&LonghandId::BorderBottomColor)
        || ids.contains(&LonghandId::BorderLeftColor)
    {
        match paint.border_side_colors {
            Some(cols) => out.push(PaintPatchOp::BorderSideColors(Some(cols))),
            None => out.push(PaintPatchOp::BorderColor(paint.border_color)),
        }
    }
    if ids.contains(&LonghandId::BorderTopLeftRadius)
        || ids.contains(&LonghandId::BorderTopRightRadius)
        || ids.contains(&LonghandId::BorderBottomRightRadius)
        || ids.contains(&LonghandId::BorderBottomLeftRadius)
    {
        out.push(PaintPatchOp::CornerRadii(paint.corner_radii));
    }
    if ids.contains(&LonghandId::OutlineColor) {
        out.push(PaintPatchOp::OutlineColor(paint.outline_color));
    }
    if ids.contains(&LonghandId::OutlineWidth) {
        out.push(PaintPatchOp::OutlineWidth(paint.outline_width));
    }
    if ids.contains(&LonghandId::OutlineOffset) {
        out.push(PaintPatchOp::OutlineOffset(paint.outline_offset));
    }
}

fn push_layout_ops_for_step(
    ids: &HashSet<LonghandId>,
    cv: &ComputedValues,
    out: &mut Vec<LayoutPatchOp>,
) {
    let pos = cv.get_position();
    if ids.contains(&LonghandId::Width) {
        out.push(LayoutPatchOp::Width(size_to_dim(&pos.width)));
    }
    if ids.contains(&LonghandId::Height) {
        out.push(LayoutPatchOp::Height(size_to_dim(&pos.height)));
    }
    if ids.contains(&LonghandId::MinWidth) {
        out.push(LayoutPatchOp::MinWidth(size_to_dim(&pos.min_width)));
    }
    if ids.contains(&LonghandId::MinHeight) {
        out.push(LayoutPatchOp::MinHeight(size_to_dim(&pos.min_height)));
    }
    if ids.contains(&LonghandId::MaxWidth) {
        out.push(LayoutPatchOp::MaxWidth(max_size_to_dim(&pos.max_width)));
    }
    if ids.contains(&LonghandId::MaxHeight) {
        out.push(LayoutPatchOp::MaxHeight(max_size_to_dim(&pos.max_height)));
    }
    if ids.contains(&LonghandId::FlexGrow) {
        out.push(LayoutPatchOp::FlexGrow(pos.flex_grow.into()));
    }
    if ids.contains(&LonghandId::FlexShrink) {
        out.push(LayoutPatchOp::FlexShrink(pos.flex_shrink.into()));
    }
    if ids.contains(&LonghandId::RowGap) {
        if let GenericLengthPercentageOrNormal::LengthPercentage(lp) = &pos.row_gap {
            out.push(LayoutPatchOp::RowGap(Some(
                lp.0.to_pixel_length(Au(0)).px(),
            )));
        }
    }
    if ids.contains(&LonghandId::ColumnGap) {
        if let GenericLengthPercentageOrNormal::LengthPercentage(lp) = &pos.column_gap {
            out.push(LayoutPatchOp::ColumnGap(Some(
                lp.0.to_pixel_length(Au(0)).px(),
            )));
        }
    }
    push_spacing_and_inset_ops(ids, cv, out);
}

fn push_spacing_and_inset_ops(
    ids: &HashSet<LonghandId>,
    cv: &ComputedValues,
    out: &mut Vec<LayoutPatchOp>,
) {
    let pos = cv.get_position();
    let pad = cv.get_padding();
    let mar = cv.get_margin();
    if ids.contains(&LonghandId::PaddingTop) {
        out.push(LayoutPatchOp::PaddingTop(
            pad.padding_top.0.to_pixel_length(Au(0)).px(),
        ));
    }
    if ids.contains(&LonghandId::PaddingRight) {
        out.push(LayoutPatchOp::PaddingRight(
            pad.padding_right.0.to_pixel_length(Au(0)).px(),
        ));
    }
    if ids.contains(&LonghandId::PaddingBottom) {
        out.push(LayoutPatchOp::PaddingBottom(
            pad.padding_bottom.0.to_pixel_length(Au(0)).px(),
        ));
    }
    if ids.contains(&LonghandId::PaddingLeft) {
        out.push(LayoutPatchOp::PaddingLeft(
            pad.padding_left.0.to_pixel_length(Au(0)).px(),
        ));
    }
    if ids.contains(&LonghandId::MarginTop) {
        out.push(LayoutPatchOp::MarginSideDim(
            krui_style::BoxSide::Top,
            margin_to_dim(&mar.margin_top),
        ));
    }
    if ids.contains(&LonghandId::MarginRight) {
        out.push(LayoutPatchOp::MarginSideDim(
            krui_style::BoxSide::Right,
            margin_to_dim(&mar.margin_right),
        ));
    }
    if ids.contains(&LonghandId::MarginBottom) {
        out.push(LayoutPatchOp::MarginSideDim(
            krui_style::BoxSide::Bottom,
            margin_to_dim(&mar.margin_bottom),
        ));
    }
    if ids.contains(&LonghandId::MarginLeft) {
        out.push(LayoutPatchOp::MarginSideDim(
            krui_style::BoxSide::Left,
            margin_to_dim(&mar.margin_left),
        ));
    }
    if ids.contains(&LonghandId::Top) {
        out.push(LayoutPatchOp::InsetTop(inset_to_dim(&pos.top)));
    }
    if ids.contains(&LonghandId::Right) {
        out.push(LayoutPatchOp::InsetRight(inset_to_dim(&pos.right)));
    }
    if ids.contains(&LonghandId::Bottom) {
        out.push(LayoutPatchOp::InsetBottom(inset_to_dim(&pos.bottom)));
    }
    if ids.contains(&LonghandId::Left) {
        out.push(LayoutPatchOp::InsetLeft(inset_to_dim(&pos.left)));
    }
}

fn convert_keyframes_rule(
    stylist: &Stylist,
    rule: &KeyframesRule,
    parent_cv: &ComputedValues,
    guard: &SharedRwLockReadGuard,
) -> Option<KeyframesDef> {
    let anim =
        KeyframesAnimation::from_keyframes(&rule.keyframes, rule.vendor_prefix.clone(), guard);
    if anim.steps.is_empty() {
        return None;
    }
    let guards = StylesheetGuards::same(guard);
    let mut stops = Vec::new();
    let mut touches_layout = false;
    for step in &anim.steps {
        let KeyframesStepValue::Declarations { block } = &step.value else {
            continue;
        };
        let ids = collect_longhand_ids(block, guard);
        let step_cv =
            stylist.compute_for_declarations::<StyloElement>(&guards, parent_cv, block.clone());
        let step_paint = extract_paint_style(&step_cv);
        let mut paint_ops = Vec::new();
        let mut layout_ops = Vec::new();
        push_paint_ops_for_step(&ids, &step_paint, &mut paint_ops);
        push_layout_ops_for_step(&ids, &step_cv, &mut layout_ops);
        if !layout_ops.is_empty() {
            touches_layout = true;
        }
        let easing = if step.declared_timing_function {
            Some(convert_timing_function(
                &step_cv.get_ui().animation_timing_function_mod(0),
            ))
        } else {
            None
        };
        stops.push(KeyframeStop {
            offset: step.start_offset.percentage.0.clamp(0.0, 1.0),
            paint_ops,
            layout_ops,
            easing,
        });
    }
    if stops.is_empty() {
        return None;
    }
    Some(KeyframesDef {
        name: rule.name.as_atom().to_string(),
        stops,
        touches_layout,
    })
}

/// Extracts all effective `@keyframes` definitions from registered Stylo stylesheets.
#[must_use]
pub(crate) fn extract_keyframes_defs(
    stylist: &Stylist,
    sheets: &[DocumentStyleSheet],
    root_cv: Option<&ComputedValues>,
    guard: &SharedRwLockReadGuard,
) -> Vec<KeyframesDef> {
    let parent_cv = root_cv.unwrap_or_else(|| stylist.device().default_computed_values());
    let device = stylist.device();
    let custom_media = CustomMediaMap::default();
    let mut out = Vec::new();
    for sheet in sheets {
        let contents = sheet.0.contents(guard);
        for rule in contents.effective_rules(device, &custom_media, guard) {
            if let CssRule::Keyframes(kf_lock) = rule {
                let kf = kf_lock.read_with(guard);
                if let Some(def) = convert_keyframes_rule(stylist, kf, parent_cv, guard) {
                    if let Some(prev) = out
                        .iter_mut()
                        .find(|d: &&mut KeyframesDef| d.name == def.name)
                    {
                        *prev = def;
                    } else {
                        out.push(def);
                    }
                }
            }
        }
    }
    out
}
