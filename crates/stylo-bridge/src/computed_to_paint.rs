//! Internal conversion from Stylo [`ComputedValues`] to KRui [`PaintStyle`],
//! [`BoxMeta`], and eager/lazy pseudo-element `content`.

use app_units::Au;
use krui_style::{
    BoxShadow, Color, CornerRadii, DisplayMode, Edges, ObjectFit, OverflowMode, PaintStyle,
    PositionMode, ScrollbarWidth, ShadowStack, UserSelect,
};
use style::color::{AbsoluteColor, ColorSpace};
use style::properties::longhands::box_sizing::computed_value::T as StyloBoxSizing;
use style::properties::longhands::object_fit::computed_value::T as StyloObjectFit;
use style::properties::longhands::overflow_x::computed_value::T as StyloOverflow;
use style::properties::longhands::scrollbar_width::computed_value::T as StyloScrollbarWidth;
use style::properties::longhands::visibility::computed_value::T as StyloVisibility;
use style::properties::ComputedValues;
use style::values::computed::{Color as StyloColor, Size};
use style::values::generics::color::ColorOrAuto;
use style::values::generics::counters::{Content, ContentItem};
use style::values::generics::effects::Filter;
use style::values::generics::position::ZIndex;
use style::values::generics::ui::ScrollbarColor as StyloScrollbarColor;
use style::values::specified::border::BorderStyle;
use style::values::specified::box_::{Appearance, DisplayInside, DisplayOutside, PositionProperty};
use style::values::specified::outline::OutlineStyle;
use style::values::specified::ui::{PointerEvents, UserSelect as StyloUserSelect};

use crate::computed_to_gradient::extract_gradients;
use crate::computed_to_transform::extract_transforms;
use crate::computed_to_transitions::{extract_animations, extract_transition_spec};
use crate::BoxMeta;

const U8_MAX_F32: f32 = 255.0;
const RADIUS_PCT_BASIS_PX: f32 = 100.0;

fn sanitize_unit_channel(val: f32) -> f32 {
    if val.is_finite() {
        val.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn channel_to_u8(val: f32) -> u8 {
    (sanitize_unit_channel(val) * U8_MAX_F32).round() as u8
}

/// Converts a Stylo `AbsoluteColor` into an 8-bit sRGB KRui [`Color`].
#[must_use]
pub(crate) fn absolute_color_to_krui(color: &AbsoluteColor) -> Color {
    let srgb = color.to_color_space(ColorSpace::Srgb);
    let c = srgb.raw_components();
    Color {
        r: channel_to_u8(c[0]),
        g: channel_to_u8(c[1]),
        b: channel_to_u8(c[2]),
        a: channel_to_u8(srgb.alpha),
    }
}

#[must_use]
pub(crate) fn stylo_color_to_krui(color: &StyloColor, current_color: &AbsoluteColor) -> Color {
    absolute_color_to_krui(&color.resolve_to_absolute(current_color))
}

fn extract_borders(style: &ComputedValues, current_color: &AbsoluteColor, paint: &mut PaintStyle) {
    let border = style.get_border();
    let sides = [
        (
            !border.border_top_style.none_or_hidden(),
            &border.border_top_width,
            &border.border_top_color,
        ),
        (
            !border.border_right_style.none_or_hidden(),
            &border.border_right_width,
            &border.border_right_color,
        ),
        (
            !border.border_bottom_style.none_or_hidden(),
            &border.border_bottom_width,
            &border.border_bottom_color,
        ),
        (
            !border.border_left_style.none_or_hidden(),
            &border.border_left_width,
            &border.border_left_color,
        ),
    ];
    let mut mask = 0_u8;
    let mut widths = [0.0_f32; 4];
    let mut colors = [Color::TRANSPARENT; 4];
    for (idx, &(active, width, col)) in sides.iter().enumerate() {
        if active {
            mask |= 1 << idx;
            widths[idx] = width.0.to_f32_px().max(0.0);
        }
        colors[idx] = stylo_color_to_krui(col, current_color);
    }
    paint.border_style_mask = mask;
    let max_w = widths.iter().copied().fold(0.0_f32, f32::max);
    paint.raw_border_width = max_w;
    paint.border_width = if mask != 0 { max_w } else { 0.0 };
    paint.border_color = colors[0];
    if colors[1] != colors[0] || colors[2] != colors[0] || colors[3] != colors[0] {
        paint.border_side_colors = Some(colors);
    }

    let basis = Au::from_f32_px(RADIUS_PCT_BASIS_PX);
    let corner_px = |r: &style::values::computed::BorderCornerRadius| {
        r.0.width().0.to_pixel_length(basis).px().max(0.0)
    };
    let radii = CornerRadii {
        top_left: corner_px(&border.border_top_left_radius),
        top_right: corner_px(&border.border_top_right_radius),
        bottom_right: corner_px(&border.border_bottom_right_radius),
        bottom_left: corner_px(&border.border_bottom_left_radius),
    };
    paint.corner_radii = radii;
    paint.border_radius = radii.max_radius();
}

fn extract_effects(style: &ComputedValues, current_color: &AbsoluteColor, paint: &mut PaintStyle) {
    let outline = style.get_outline();
    if !matches!(
        outline.outline_style,
        OutlineStyle::BorderStyle(BorderStyle::None | BorderStyle::Hidden)
    ) && outline.outline_width.0.to_f32_px() > 0.0
    {
        paint.outline_width = outline.outline_width.0.to_f32_px().max(0.0);
        paint.outline_offset = outline.outline_offset.to_f32_px();
        paint.outline_color = stylo_color_to_krui(&outline.outline_color, current_color);
    }

    let effects = style.get_effects();
    paint.opacity = sanitize_unit_channel(effects.opacity);
    let shadows: Vec<BoxShadow> = effects
        .box_shadow
        .0
        .iter()
        .map(|s| BoxShadow {
            offset_x: s.base.horizontal.px(),
            offset_y: s.base.vertical.px(),
            blur_radius: s.base.blur.0.px().max(0.0),
            spread_radius: s.spread.px(),
            color: stylo_color_to_krui(&s.base.color, current_color),
            inset: s.inset,
        })
        .collect();
    paint.shadow_stack = ShadowStack::from_layers(&shadows);
    paint.box_shadow = paint.shadow_stack.first();
    paint.secondary_shadow = paint.shadow_stack.second();

    for filter in &*effects.backdrop_filter.0 {
        if let Filter::Blur(radius) = filter {
            paint.backdrop_blur_px = radius.0.px().max(0.0);
            break;
        }
    }
}

fn extract_ui_paint(style: &ComputedValues, current_color: &AbsoluteColor, paint: &mut PaintStyle) {
    let box_s = style.get_box();
    let ui = style.get_ui();
    let inh_ui = style.get_inherited_ui();
    paint.appearance_none = box_s.appearance == Appearance::None;
    paint.user_select = map_user_select(ui.user_select);
    paint.object_fit = map_object_fit(style.get_position().object_fit);

    match &inh_ui.caret_color.0 {
        ColorOrAuto::Auto => {}
        ColorOrAuto::Color(c) if c.is_currentcolor() => paint.caret_uses_current_color = true,
        ColorOrAuto::Color(c) => paint.caret_color = Some(stylo_color_to_krui(c, current_color)),
    }
    if let StyloScrollbarColor::Colors { thumb, track } = &inh_ui.scrollbar_color {
        paint.scrollbar_color = Some((
            stylo_color_to_krui(thumb, current_color),
            stylo_color_to_krui(track, current_color),
        ));
    }

    if let Some(accent) =
        crate::servo_ext::extract_accent_color(style, absolute_color_to_krui(current_color))
    {
        match accent {
            Some(col) => {
                paint.accent_color = Some(col);
                paint.author_accent_color = true;
            }
            None => {
                paint.accent_color = None;
                paint.author_accent_color = false;
            }
        }
    }

    paint.transition = extract_transition_spec(ui);
    paint.animations = extract_animations(ui);
    let (tf2d, tf_list, standalone) = extract_transforms(style);
    paint.transform = tf2d;
    paint.transform_list = tf_list;
    paint.standalone_transform = standalone;
}

/// Extracts compositor paint properties from a resolved Stylo [`ComputedValues`] snapshot.
#[must_use]
pub(crate) fn extract_paint_style(style: &ComputedValues) -> PaintStyle {
    let mut paint = PaintStyle::default();
    let current_color = style.get_inherited_text().clone_color();
    paint.text_color = absolute_color_to_krui(&current_color);

    let bg = style.get_background();
    paint.background_color = stylo_color_to_krui(&bg.background_color, &current_color);

    let imgs = extract_gradients(style, &current_color);
    paint.background_gradient = imgs.linear;
    paint.radial_gradient = imgs.radial;
    paint.bg_size_scale = imgs.bg_size_scale;
    paint.bg_position = imgs.bg_position;

    extract_borders(style, &current_color, &mut paint);
    extract_effects(style, &current_color, &mut paint);
    extract_ui_paint(style, &current_color, &mut paint);
    paint
}

fn map_display(style: &ComputedValues) -> DisplayMode {
    let d = style.get_box().clone_display();
    if d.is_none() {
        return DisplayMode::None;
    }
    match (d.outside(), d.inside()) {
        (DisplayOutside::Inline, DisplayInside::Flex) => DisplayMode::InlineFlex,
        (_, DisplayInside::Flex) => DisplayMode::Flex,
        (DisplayOutside::Inline, DisplayInside::Grid) => DisplayMode::InlineGrid,
        (_, DisplayInside::Grid) => DisplayMode::Grid,
        (_, DisplayInside::Contents) => DisplayMode::Contents,
        (DisplayOutside::Inline, DisplayInside::FlowRoot) => DisplayMode::InlineBlock,
        (DisplayOutside::Inline, DisplayInside::Flow) => DisplayMode::Inline,
        _ => DisplayMode::Block,
    }
}

fn map_overflow(o: StyloOverflow) -> OverflowMode {
    match o {
        StyloOverflow::Visible => OverflowMode::Visible,
        StyloOverflow::Hidden | StyloOverflow::Clip => OverflowMode::Hidden,
        StyloOverflow::Scroll | StyloOverflow::Auto => OverflowMode::Scroll,
    }
}

fn map_user_select(u: StyloUserSelect) -> UserSelect {
    match u {
        StyloUserSelect::Auto => UserSelect::Auto,
        StyloUserSelect::Text => UserSelect::Text,
        StyloUserSelect::None => UserSelect::None,
        StyloUserSelect::All => UserSelect::All,
        #[allow(unreachable_patterns)]
        _ => UserSelect::Auto,
    }
}

fn map_object_fit(o: StyloObjectFit) -> ObjectFit {
    match o {
        StyloObjectFit::Fill => ObjectFit::Fill,
        StyloObjectFit::Contain => ObjectFit::Contain,
        StyloObjectFit::Cover => ObjectFit::Cover,
        StyloObjectFit::None => ObjectFit::None,
        StyloObjectFit::ScaleDown => ObjectFit::ScaleDown,
    }
}

/// Extracts layout-adjacent box and interaction metadata from [`ComputedValues`].
#[must_use]
pub(crate) fn extract_box_meta(style: &ComputedValues) -> BoxMeta {
    let b = style.get_box();
    let pad = style.get_padding();
    let brd = style.get_border();
    let ui = style.get_ui();
    let inh_ui = style.get_inherited_ui();
    let pos = style.get_position();
    let border_side = |active: bool, w: &style::values::computed::BorderSideWidth| {
        if active {
            w.0.to_f32_px().max(0.0)
        } else {
            0.0
        }
    };
    BoxMeta {
        display: map_display(style),
        position: match b.position {
            PositionProperty::Static => PositionMode::Static,
            PositionProperty::Relative => PositionMode::Relative,
            PositionProperty::Absolute => PositionMode::Absolute,
            PositionProperty::Fixed => PositionMode::Fixed,
            PositionProperty::Sticky => PositionMode::Sticky,
        },
        overflow_x: map_overflow(b.overflow_x),
        overflow_y: map_overflow(b.overflow_y),
        padding: Edges {
            top: pad.padding_top.0.to_pixel_length(Au(0)).px().max(0.0),
            right: pad.padding_right.0.to_pixel_length(Au(0)).px().max(0.0),
            bottom: pad.padding_bottom.0.to_pixel_length(Au(0)).px().max(0.0),
            left: pad.padding_left.0.to_pixel_length(Au(0)).px().max(0.0),
        },
        border: Edges {
            top: border_side(
                !brd.border_top_style.none_or_hidden(),
                &brd.border_top_width,
            ),
            right: border_side(
                !brd.border_right_style.none_or_hidden(),
                &brd.border_right_width,
            ),
            bottom: border_side(
                !brd.border_bottom_style.none_or_hidden(),
                &brd.border_bottom_width,
            ),
            left: border_side(
                !brd.border_left_style.none_or_hidden(),
                &brd.border_left_width,
            ),
        },
        scrollbar_width: match ui.scrollbar_width {
            StyloScrollbarWidth::Auto => ScrollbarWidth::Auto,
            StyloScrollbarWidth::Thin => ScrollbarWidth::Thin,
            StyloScrollbarWidth::None => ScrollbarWidth::None,
        },
        z_index: match pos.z_index {
            ZIndex::Integer(z) => Some(z),
            ZIndex::Auto => None,
        },
        visibility_hidden: style.get_inherited_box().visibility != StyloVisibility::Visible,
        pointer_events_none: inh_ui.pointer_events == PointerEvents::None,
        object_fit: map_object_fit(pos.object_fit),
        user_select: map_user_select(ui.user_select),
    }
}

/// Extracts generated `content` text for `::before` / `::after` pseudo-elements.
#[must_use]
pub(crate) fn extract_pseudo_content(style: &ComputedValues) -> Option<String> {
    let Content::Items(items) = &style.get_counters().content else {
        return None;
    };
    let mut out = String::new();
    for item in items.items.iter() {
        if let ContentItem::String(s) = item {
            out.push_str(s);
        }
    }
    Some(out)
}

fn border_layout_differs(
    a: &style::properties::style_structs::Border,
    b: &style::properties::style_structs::Border,
) -> bool {
    a.border_top_width != b.border_top_width
        || a.border_right_width != b.border_right_width
        || a.border_bottom_width != b.border_bottom_width
        || a.border_left_width != b.border_left_width
        || a.border_top_style != b.border_top_style
        || a.border_right_style != b.border_right_style
        || a.border_bottom_style != b.border_bottom_style
        || a.border_left_style != b.border_left_style
}

fn position_layout_differs(
    a: &style::properties::style_structs::Position,
    b: &style::properties::style_structs::Position,
) -> bool {
    a.width != b.width
        || a.height != b.height
        || a.min_width != b.min_width
        || a.min_height != b.min_height
        || a.max_width != b.max_width
        || a.max_height != b.max_height
        || a.top != b.top
        || a.right != b.right
        || a.bottom != b.bottom
        || a.left != b.left
        || a.flex_basis != b.flex_basis
        || a.flex_grow != b.flex_grow
        || a.flex_shrink != b.flex_shrink
        || a.flex_direction != b.flex_direction
        || a.flex_wrap != b.flex_wrap
        || a.justify_content != b.justify_content
        || a.align_content != b.align_content
        || a.align_items != b.align_items
        || a.align_self != b.align_self
        || a.justify_items != b.justify_items
        || a.justify_self != b.justify_self
        || a.row_gap != b.row_gap
        || a.column_gap != b.column_gap
        || a.box_sizing != b.box_sizing
        || a.aspect_ratio != b.aspect_ratio
        || a.order != b.order
        || a.grid_auto_flow != b.grid_auto_flow
        || a.grid_auto_columns != b.grid_auto_columns
        || a.grid_auto_rows != b.grid_auto_rows
        || a.grid_template_columns != b.grid_template_columns
        || a.grid_template_rows != b.grid_template_rows
        || a.grid_template_areas != b.grid_template_areas
        || a.grid_column_start != b.grid_column_start
        || a.grid_column_end != b.grid_column_end
        || a.grid_row_start != b.grid_row_start
        || a.grid_row_end != b.grid_row_end
}

fn is_fixed_border_box(p: &style::properties::style_structs::Position) -> bool {
    p.box_sizing == StyloBoxSizing::BorderBox
        && matches!(p.width, Size::LengthPercentage(_))
        && matches!(p.height, Size::LengthPercentage(_))
}

/// Returns `true` only when Taffy-relevant layout or typography metrics changed between `old` and `new`
/// (ensuring pure compositor `transform`/`opacity`/color/leaf-border changes return `Repaint`, 0 relayouts).
#[must_use]
pub(crate) fn needs_taffy_relayout(
    old: Option<&ComputedValues>,
    new: Option<&ComputedValues>,
    has_element_children: bool,
) -> bool {
    let (a, b) = match (old, new) {
        (None, None) => return false,
        (Some(a), Some(b)) => (a, b),
        _ => return true,
    };
    if std::ptr::eq(a, b) {
        return false;
    }
    let (ba, bb) = (a.get_box(), b.get_box());
    let (pa, pb) = (a.get_position(), b.get_position());
    let (ita, itb) = (a.get_inherited_text(), b.get_inherited_text());
    let border_triggers_relayout = border_layout_differs(a.get_border(), b.get_border())
        && (has_element_children || !(is_fixed_border_box(pa) && is_fixed_border_box(pb)));
    ba.display != bb.display
        || ba.position != bb.position
        || ba.overflow_x != bb.overflow_x
        || ba.overflow_y != bb.overflow_y
        || a.get_padding() != b.get_padding()
        || a.get_margin() != b.get_margin()
        || a.get_font() != b.get_font()
        || border_triggers_relayout
        || position_layout_differs(pa, pb)
        || a.get_inherited_box().direction != b.get_inherited_box().direction
        || a.get_ui().scrollbar_width != b.get_ui().scrollbar_width
        || a.get_counters().content != b.get_counters().content
        || ita.text_align != itb.text_align
        || ita.text_wrap_mode != itb.text_wrap_mode
        || ita.white_space_collapse != itb.white_space_collapse
        || a.get_text().text_overflow != b.get_text().text_overflow
        || crate::servo_ext::text_box_props_differ(a, b)
}
