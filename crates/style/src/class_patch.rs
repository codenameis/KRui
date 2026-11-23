//! Box-layout, typography, and compositor paint keyframe patch operations (`LayoutPatchOp`,
//! `TextPatchOp`, `PaintPatchOp`).

use crate::affine::Transform2D;
use crate::box_edges::{BoxSide, ScrollbarWidth};
use crate::color::Color;
use crate::font::FontFamilyStack;
use crate::gradient_radial::{RadialCoord, RadialGradient};
use crate::grid::{GridAutoFlow, GridLinePair, GridPlacementAtom, GridTrackList};
use crate::keyframes::AnimationListRef;
use crate::layout_types::{
    AlignItems, BoxSizing, CornerRadii, Dimension, DisplayMode, Edges, FlexDirection, InsetEdges,
    JustifyContent, LayoutStyle, OverflowMode, PositionMode,
};
use crate::paint_types::{BoxShadow, LinearGradient, ObjectFit, PaintStyle, UserSelect};
use crate::shadow_stack::ShadowStack;
use crate::standalone_transform::{OriginAxis, TranslateAxis};
use crate::transform3d::TransformListRef;
use crate::transition::TransitionSpec;
use crate::typography::{FontVariationList, LineHeight, TextBoxEdge, TextBoxTrim};
use crate::{TextAlign, TextDirection, TextStyle};

/// Box-layout field mutation used by `@keyframes` and layout transitions.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum LayoutPatchOp {
    /// Sets `layout.display`.
    Display(DisplayMode),
    /// Sets `layout.position`.
    Position(PositionMode),
    /// Sets `layout.overflow` and clears explicit `overflow_x`/`overflow_y` overrides.
    Overflow(OverflowMode),
    /// Sets `layout.flex_direction`.
    FlexDirection(FlexDirection),
    /// Sets `layout.flex_wrap`.
    FlexWrap(bool),
    /// Sets `layout.justify_content`.
    JustifyContent(JustifyContent),
    /// Sets `layout.align_items`.
    AlignItems(AlignItems),
    /// Sets `layout.flex_grow`.
    FlexGrow(f32),
    /// Sets `layout.flex_shrink`.
    FlexShrink(f32),
    /// Sets `layout.grid_columns`.
    GridColumns(u16),
    /// Sets `layout.grid_template_columns`.
    GridTemplateColumns(GridTrackList),
    /// Sets `layout.grid_template_rows`.
    GridTemplateRows(GridTrackList),
    /// Sets `layout.width`.
    Width(Dimension),
    /// Sets `layout.height`.
    Height(Dimension),
    /// Sets `layout.min_width`.
    MinWidth(Dimension),
    /// Sets `layout.min_height`.
    MinHeight(Dimension),
    /// Sets `layout.max_width`.
    MaxWidth(Dimension),
    /// Sets `layout.max_height`.
    MaxHeight(Dimension),
    /// Sets `layout.inset.top`.
    InsetTop(Dimension),
    /// Sets `layout.inset.right`.
    InsetRight(Dimension),
    /// Sets `layout.inset.bottom`.
    InsetBottom(Dimension),
    /// Sets `layout.inset.left`.
    InsetLeft(Dimension),
    /// Sets `layout.padding.top` (syncs `layout.padding_dims` if present).
    PaddingTop(f32),
    /// Sets `layout.padding.right` (syncs `layout.padding_dims` if present).
    PaddingRight(f32),
    /// Sets `layout.padding.bottom` (syncs `layout.padding_dims` if present).
    PaddingBottom(f32),
    /// Sets `layout.padding.left` (syncs `layout.padding_dims` if present).
    PaddingLeft(f32),
    /// Sets `layout.margin.top` (syncs `layout.margin_dims` if present).
    MarginTop(f32),
    /// Sets `layout.margin.right` (syncs `layout.margin_dims` if present).
    MarginRight(f32),
    /// Sets `layout.margin.bottom` (syncs `layout.margin_dims` if present).
    MarginBottom(f32),
    /// Sets `layout.margin.left` (syncs `layout.margin_dims` if present).
    MarginLeft(f32),
    /// Sets `layout.border_width` and clears per-side `layout.border_edges`.
    BorderWidth(f32),
    /// Sets `layout.border_edges`.
    BorderEdges(Option<Edges>),
    /// Sets one border side (`0=top, 1=right, 2=bottom, 3=left`) in `layout.border_edges`.
    BorderEdgeSide(u8, f32),
    /// Sets `layout.border_style_mask` (`0..=0x0F`).
    BorderStyleMask(u8),
    /// Sets `layout.overflow_x`.
    OverflowX(Option<OverflowMode>),
    /// Sets `layout.overflow_y`.
    OverflowY(Option<OverflowMode>),
    /// Sets `layout.gap` and clears explicit `row_gap`/`column_gap` overrides.
    Gap(f32),
    /// Sets `layout.row_gap`.
    RowGap(Option<f32>),
    /// Sets `layout.column_gap`.
    ColumnGap(Option<f32>),
    /// Sets `layout.aspect_ratio`.
    AspectRatio(Option<f32>),
    /// Sets `layout.z_index`.
    ZIndex(i32),
    /// Sets `layout.has_z_index` (`false` for `z-index: auto`).
    HasZIndex(bool),
    /// Sets `layout.display_explicit_flex`.
    DisplayExplicitFlex(bool),
    /// Sets `layout.flex_direction_explicit`.
    FlexDirectionExplicit(bool),
    /// Sets `layout.margin_dims` (and syncs `layout.margin`).
    MarginDims(Option<InsetEdges>),
    /// Sets `layout.padding_dims` (and syncs `layout.padding`).
    PaddingDims(Option<InsetEdges>),
    /// Sets `layout.flex_basis`.
    FlexBasis(Dimension),
    /// Sets `layout.align_self`.
    AlignSelf(Option<AlignItems>),
    /// Sets `layout.justify_items`.
    JustifyItems(Option<AlignItems>),
    /// Sets `layout.justify_self`.
    JustifySelf(Option<AlignItems>),
    /// Sets `layout.box_sizing`.
    BoxSizing(BoxSizing),
    /// Sets `layout.grid_auto_flow`.
    GridAutoFlow(GridAutoFlow),
    /// Sets `layout.grid_column`.
    GridColumn(GridLinePair),
    /// Sets `layout.grid_row`.
    GridRow(GridLinePair),
    /// Sets one margin edge (`BoxSide`) to any `Dimension` while syncing `margin` and `margin_dims`.
    MarginSideDim(BoxSide, Dimension),
    /// Sets one padding edge (`BoxSide`) to any `Dimension` while syncing `padding` and `padding_dims`.
    PaddingSideDim(BoxSide, Dimension),
    /// Sets `layout.grid_column.start`.
    GridColumnStart(GridPlacementAtom),
    /// Sets `layout.grid_column.end`.
    GridColumnEnd(GridPlacementAtom),
    /// Sets `layout.grid_row.start`.
    GridRowStart(GridPlacementAtom),
    /// Sets `layout.grid_row.end`.
    GridRowEnd(GridPlacementAtom),
    /// Sets `layout.padding_explicit`.
    PaddingExplicit(bool),
    /// Sets `layout.scrollbar_width`.
    ScrollbarWidth(ScrollbarWidth),
}

impl LayoutPatchOp {
    /// Applies this layout field mutation to `layout`.
    pub fn apply_to(self, layout: &mut LayoutStyle) {
        match self {
            Self::Display(v) => layout.display = v,
            Self::Position(v) => layout.position = v,
            Self::Overflow(v) => {
                layout.overflow = v;
                layout.overflow_x = None;
                layout.overflow_y = None;
            }
            Self::OverflowX(v) => layout.overflow_x = v,
            Self::OverflowY(v) => layout.overflow_y = v,
            Self::FlexDirection(v) => layout.flex_direction = v,
            Self::FlexWrap(v) => layout.flex_wrap = v,
            Self::JustifyContent(v) => layout.justify_content = v,
            Self::AlignItems(v) => layout.align_items = v,
            Self::FlexGrow(v) => layout.flex_grow = v,
            Self::FlexShrink(v) => layout.flex_shrink = v,
            Self::GridColumns(v) => layout.grid_columns = v,
            Self::GridTemplateColumns(v) => layout.grid_template_columns = v,
            Self::GridTemplateRows(v) => layout.grid_template_rows = v,
            Self::Width(v) => layout.width = v,
            Self::Height(v) => layout.height = v,
            Self::MinWidth(v) => layout.min_width = v,
            Self::MinHeight(v) => layout.min_height = v,
            Self::MaxWidth(v) => layout.max_width = v,
            Self::MaxHeight(v) => layout.max_height = v,
            other => other.apply_edges_to(layout),
        }
    }

    fn apply_edges_to(self, layout: &mut LayoutStyle) {
        match self {
            Self::InsetTop(v) => layout.inset.top = v,
            Self::InsetRight(v) => layout.inset.right = v,
            Self::InsetBottom(v) => layout.inset.bottom = v,
            Self::InsetLeft(v) => layout.inset.left = v,
            Self::PaddingTop(v) => layout.set_padding_side(BoxSide::Top, Dimension::Px(v)),
            Self::PaddingRight(v) => layout.set_padding_side(BoxSide::Right, Dimension::Px(v)),
            Self::PaddingBottom(v) => layout.set_padding_side(BoxSide::Bottom, Dimension::Px(v)),
            Self::PaddingLeft(v) => layout.set_padding_side(BoxSide::Left, Dimension::Px(v)),
            Self::MarginTop(v) => layout.set_margin_side(BoxSide::Top, Dimension::Px(v)),
            Self::MarginRight(v) => layout.set_margin_side(BoxSide::Right, Dimension::Px(v)),
            Self::MarginBottom(v) => layout.set_margin_side(BoxSide::Bottom, Dimension::Px(v)),
            Self::MarginLeft(v) => layout.set_margin_side(BoxSide::Left, Dimension::Px(v)),
            Self::MarginSideDim(side, v) => layout.set_margin_side(side, v),
            Self::PaddingSideDim(side, v) => layout.set_padding_side(side, v),
            other => other.apply_extra_layout_ops(layout),
        }
    }

    fn apply_extra_layout_ops(self, layout: &mut LayoutStyle) {
        match self {
            Self::MarginDims(dims) => match dims {
                Some(d) => layout.set_margin_all(d),
                None => layout.margin_dims = None,
            },
            Self::PaddingDims(dims) => match dims {
                Some(d) => layout.set_padding_all(d),
                None => layout.padding_dims = None,
            },
            Self::FlexBasis(v) => layout.flex_basis = v,
            Self::AlignSelf(v) => layout.align_self = v,
            Self::JustifyItems(v) => layout.justify_items = v,
            Self::JustifySelf(v) => layout.justify_self = v,
            Self::BoxSizing(v) => layout.box_sizing = v,
            Self::GridAutoFlow(v) => layout.grid_auto_flow = v,
            Self::GridColumn(v) => layout.grid_column = v,
            Self::GridRow(v) => layout.grid_row = v,
            Self::GridColumnStart(v) => layout.grid_column.start = v,
            Self::GridColumnEnd(v) => layout.grid_column.end = v,
            Self::GridRowStart(v) => layout.grid_row.start = v,
            Self::GridRowEnd(v) => layout.grid_row.end = v,
            Self::BorderWidth(v) => {
                layout.border_width = v;
                layout.border_edges = None;
            }
            Self::BorderEdges(v) => layout.border_edges = v,
            Self::BorderEdgeSide(side, v) => {
                let mut e = layout.raw_border_edges();
                match side & 3 {
                    0 => e.top = v,
                    1 => e.right = v,
                    2 => e.bottom = v,
                    _ => e.left = v,
                }
                layout.border_edges = Some(e);
            }
            Self::BorderStyleMask(m) => layout.border_style_mask = m & 0x0F,
            Self::ZIndex(z) => layout.z_index = z,
            Self::HasZIndex(b) => layout.has_z_index = b,
            Self::DisplayExplicitFlex(b) => layout.display_explicit_flex = b,
            Self::FlexDirectionExplicit(b) => layout.flex_direction_explicit = b,
            Self::PaddingExplicit(b) => layout.padding_explicit = b,
            Self::ScrollbarWidth(sw) => layout.scrollbar_width = sw,
            Self::Gap(v) => {
                layout.gap = v;
                layout.row_gap = None;
                layout.column_gap = None;
            }
            Self::RowGap(v) => layout.row_gap = v,
            Self::ColumnGap(v) => layout.column_gap = v,
            Self::AspectRatio(v) => layout.aspect_ratio = v,
            _ => {}
        }
    }
}

/// Typography field mutation inside a [`StylePatchOp`].
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum TextPatchOp {
    /// Sets `text.font_size`.
    FontSize(f32),
    /// Sets `text.font_weight`.
    FontWeight(u16),
    /// Sets `text.line_height`.
    LineHeight(LineHeight),
    /// Sets `text.font_family`.
    FontFamily(FontFamilyStack),
    /// Sets `text.text_align`.
    TextAlign(TextAlign),
    /// Sets `text.direction`.
    Direction(TextDirection),
    /// Sets `text.text_box_trim`.
    TextBoxTrim(TextBoxTrim),
    /// Sets `text.text_box_edge`.
    TextBoxEdge(TextBoxEdge),
    /// Sets `text.variations`.
    Variations(FontVariationList),
}

impl TextPatchOp {
    /// Applies this typography mutation to `text`.
    pub fn apply_to(self, text: &mut TextStyle) {
        match self {
            Self::FontSize(v) => text.font_size = v,
            Self::FontWeight(v) => text.font_weight = v,
            Self::LineHeight(v) => text.line_height = v,
            Self::FontFamily(v) => text.font_family = v,
            Self::TextAlign(v) => {
                text.text_align = v;
                text.text_align_explicit = true;
            }
            Self::Direction(v) => text.direction = v,
            Self::TextBoxTrim(v) => text.text_box_trim = v,
            Self::TextBoxEdge(v) => text.text_box_edge = v,
            Self::Variations(v) => text.variations = v,
        }
    }
}

/// Paint/compositing field mutation inside a [`StylePatchOp`].
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum PaintPatchOp {
    /// Sets `paint.background_color`.
    BackgroundColor(Color),
    /// Sets `paint.background_gradient`.
    BackgroundGradient(Option<LinearGradient>),
    /// Sets `paint.text_color`, clears `inherits_text_color`, and marks `provides_inherited_text_color`.
    TextColor(Color),
    /// Sets `paint.border_color` and clears per-side `paint.border_side_colors`.
    BorderColor(Color),
    /// Sets authored `paint.raw_border_width` and syncs computed `paint.border_width`.
    BorderWidth(f32),
    /// Sets `paint.border_style_mask` and syncs computed `paint.border_width`.
    BorderStyleMask(u8),
    /// Sets `paint.border_radius`.
    BorderRadius(f32),
    /// Sets `paint.corner_radii` and syncs `paint.border_radius`.
    CornerRadii(CornerRadii),
    /// Sets one corner radius (`0=tl, 1=tr, 2=br, 3=bl`) and syncs `paint.border_radius`.
    CornerRadiusSide(u8, f32),
    /// Sets `paint.border_bottom_width`.
    BorderBottomWidth(f32),
    /// Sets `paint.border_bottom_color`.
    BorderBottomColor(Color),
    /// Sets `paint.border_side_colors`.
    BorderSideColors(Option<[Color; 4]>),
    /// Sets one border side color (`0=top, 1=right, 2=bottom, 3=left`) in `paint.border_side_colors`.
    BorderSideColor(u8, Color),
    /// Sets `paint.outline_width`.
    OutlineWidth(f32),
    /// Sets `paint.outline_offset`.
    OutlineOffset(f32),
    /// Sets `paint.outline_color`.
    OutlineColor(Color),
    /// Sets `paint.opacity`.
    Opacity(f32),
    /// Sets `paint.transform`.
    Transform(Transform2D),
    /// Sets `paint.box_shadow`.
    BoxShadow(Option<BoxShadow>),
    /// Sets `paint.secondary_shadow`.
    SecondaryShadow(Option<BoxShadow>),
    /// Sets `paint.backdrop_blur_px`.
    BackdropBlurPx(f32),
    /// Sets `paint.user_select`.
    UserSelect(UserSelect),
    /// Sets `paint.object_fit`.
    ObjectFit(ObjectFit),
    /// Sets `paint.transition`.
    Transition(TransitionSpec),
    /// Sets `(paint.caret_color, paint.caret_uses_current_color)`.
    CaretColor(Option<Color>, bool),
    /// Sets `paint.selection_background`.
    SelectionBackground(Option<Color>),
    /// Sets `paint.selection_color`.
    SelectionColor(Option<Color>),
    /// Sets `paint.placeholder_color`.
    PlaceholderColor(Option<Color>),
    /// Sets `paint.placeholder_opacity`.
    PlaceholderOpacity(f32),
    /// Sets `(paint.accent_color, paint.author_accent_color)`.
    AccentColor(Option<Color>, bool),
    /// Sets `paint.scrollbar_color`.
    ScrollbarColor(Option<(Color, Color)>),
    /// Sets `paint.radial_gradient`.
    RadialGradient(Option<RadialGradient>),
    /// Sets `paint.bg_size_scale`.
    BgSizeScale(Option<(f32, f32)>),
    /// Sets `paint.bg_position`.
    BgPosition(Option<(RadialCoord, RadialCoord)>),
    /// Sets `paint.appearance_none`.
    AppearanceNone(bool),
    /// Sets `paint.shadow_stack` and syncs `box_shadow`/`secondary_shadow`.
    ShadowStack(ShadowStack),
    /// Sets `paint.standalone_transform.translate_x` and `translate_y`.
    Translate(TranslateAxis, TranslateAxis),
    /// Sets `paint.standalone_transform.rotate_deg`.
    RotateDeg(f32),
    /// Sets `paint.standalone_transform.scale_x` and `scale_y`.
    Scale(f32, f32),
    /// Sets `paint.standalone_transform.origin_x` and `origin_y`.
    TransformOrigin(OriginAxis, OriginAxis),
    /// Sets `paint.transform_list`.
    TransformList(TransformListRef),
    /// Sets `paint.animations`.
    Animations(AnimationListRef),
}

impl PaintPatchOp {
    /// Applies this paint mutation to `paint`.
    pub fn apply_to(self, paint: &mut PaintStyle) {
        match self {
            Self::BackgroundColor(v) => paint.background_color = v,
            Self::BackgroundGradient(v) => paint.background_gradient = v,
            Self::TextColor(v) => {
                paint.text_color = v;
                paint.inherits_text_color = false;
                paint.provides_inherited_text_color = true;
            }
            Self::BorderColor(v) => (paint.border_color, paint.border_side_colors) = (v, None),
            Self::BorderWidth(v) => {
                paint.set_raw_border_width(v);
            }
            Self::BorderStyleMask(m) => {
                paint.set_border_style_mask(m);
            }
            Self::BorderRadius(v) => {
                (paint.border_radius, paint.corner_radii) = (v, CornerRadii::all(v));
            }
            Self::CornerRadii(v) => {
                (paint.corner_radii, paint.border_radius) = (v, v.max_radius());
            }
            Self::CornerRadiusSide(idx, v) => {
                let mut cr = paint.effective_corner_radii();
                match idx & 3 {
                    0 => cr.top_left = v,
                    1 => cr.top_right = v,
                    2 => cr.bottom_right = v,
                    _ => cr.bottom_left = v,
                }
                (paint.corner_radii, paint.border_radius) = (cr, cr.max_radius());
            }
            Self::BorderBottomWidth(v) => paint.border_bottom_width = v,
            Self::BorderBottomColor(v) => paint.border_bottom_color = v,
            Self::BorderSideColors(v) => paint.border_side_colors = v,
            Self::BorderSideColor(side, c) => {
                let mut s = paint.border_side_colors.unwrap_or([paint.border_color; 4]);
                s[(side as usize) & 3] = c;
                paint.border_side_colors = Some(s);
            }
            Self::OutlineWidth(v) => paint.outline_width = v,
            Self::OutlineOffset(v) => paint.outline_offset = v,
            Self::OutlineColor(v) => paint.outline_color = v,
            other => other.apply_compositor_to(paint),
        }
    }

    fn apply_compositor_to(self, paint: &mut PaintStyle) {
        match self {
            Self::Opacity(v) => paint.opacity = v,
            Self::Transform(v) => paint.transform = v,
            Self::BoxShadow(v) => paint.box_shadow = v,
            Self::SecondaryShadow(v) => paint.secondary_shadow = v,
            Self::BackdropBlurPx(v) => paint.backdrop_blur_px = v,
            Self::UserSelect(v) => paint.user_select = v,
            Self::ObjectFit(v) => paint.object_fit = v,
            Self::Transition(v) => paint.transition = v,
            Self::CaretColor(c, cur) => {
                paint.caret_color = c;
                paint.caret_uses_current_color = cur;
            }
            Self::SelectionBackground(v) => paint.selection_background = v,
            Self::SelectionColor(v) => paint.selection_color = v,
            Self::PlaceholderColor(v) => paint.placeholder_color = v,
            Self::PlaceholderOpacity(v) => paint.placeholder_opacity = v,
            Self::AccentColor(c, auth) => {
                paint.accent_color = c;
                paint.author_accent_color = auth;
            }
            Self::ScrollbarColor(v) => paint.scrollbar_color = v,
            Self::RadialGradient(v) => paint.radial_gradient = v,
            Self::BgSizeScale(v) => paint.bg_size_scale = v,
            Self::BgPosition(v) => paint.bg_position = v,
            Self::AppearanceNone(v) => paint.appearance_none = v,
            Self::ShadowStack(v) => {
                paint.shadow_stack = v;
                paint.box_shadow = v.first();
                paint.secondary_shadow = v.second();
            }
            Self::Translate(tx, ty) => {
                paint.standalone_transform.translate_x = tx;
                paint.standalone_transform.translate_y = ty;
            }
            Self::RotateDeg(deg) => paint.standalone_transform.rotate_deg = deg,
            Self::Scale(sx, sy) => {
                paint.standalone_transform.scale_x = sx;
                paint.standalone_transform.scale_y = sy;
            }
            Self::TransformOrigin(ox, oy) => {
                paint.standalone_transform.origin_x = ox;
                paint.standalone_transform.origin_y = oy;
            }
            Self::TransformList(tl) => paint.transform_list = tl,
            Self::Animations(anims) => paint.animations = anims,
            _ => {}
        }
    }
}
