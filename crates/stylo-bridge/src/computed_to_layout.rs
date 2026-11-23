//! Extracts a pure-Rust [`LayoutStyle`] snapshot from Stylo [`ComputedValues`].
//!
//! Taffy layout itself uses zero-copy `TaffyStyloStyle` directly (preserving `calc()` and named
//! grid lines/areas); this snapshot supplies `LayoutStyle` for scrolling, content boxes, inline
//! collapse checks, and Tier-2 layout transition start/end interpolation.

use krui_style::{
    AlignItems, AutoRepeatMode, BoxSizing, Dimension, Edges, FlexDirection, GridAutoFlow,
    GridLinePair, GridMaxTrack, GridMinTrack, GridPlacementAtom, GridTrack, GridTrackList,
    InsetEdges, JustifyContent, LayoutStyle, OverflowMode,
};
use style::computed_values::{flex_direction::T as StyloFlexDir, flex_wrap::T as StyloFlexWrap};
use style::properties::longhands::box_sizing::computed_value::T as StyloBoxSizing;
use style::properties::ComputedValues;
use style::values::computed::length_percentage::Unpacked;
use style::values::computed::{
    CSSPixelLength, GridLine, GridTemplateComponent, LengthPercentage, MaxSize, Size,
};
use style::values::generics::flex::GenericFlexBasis;
use style::values::generics::grid::{RepeatCount, TrackBreadth, TrackListValue, TrackSize};
use style::values::generics::length::{GenericLengthPercentageOrNormal, GenericMargin};
use style::values::generics::position::Inset as GenericInset;
use style::values::generics::NonNegative;
use style::values::specified::align::AlignFlags;
use style::values::specified::GenericGridTemplateComponent;

use crate::computed_to_paint::extract_box_meta;

/// Extracts a [`LayoutStyle`] snapshot from computed Stylo values.
#[must_use]
pub(crate) fn extract_layout_style(cv: &ComputedValues) -> LayoutStyle {
    let meta = extract_box_meta(cv);
    let pos = cv.get_position();
    let (margin, margin_dims) = extract_margins(cv.get_margin());
    let padding_dims = extract_padding_dims(cv);
    let border_style_mask = extract_border_style_mask(cv);
    let (row_gap, col_gap, uniform_gap) = extract_gaps(cv);
    let uniform_border = is_uniform_edges(meta.border);

    LayoutStyle {
        display: meta.display,
        position: meta.position,
        overflow: OverflowMode::Visible,
        overflow_x: Some(meta.overflow_x),
        overflow_y: Some(meta.overflow_y),
        flex_direction: map_flex_direction(pos.flex_direction),
        flex_wrap: pos.flex_wrap.contains(StyloFlexWrap::WRAP)
            || pos.flex_wrap.contains(StyloFlexWrap::WRAP_REVERSE),
        justify_content: map_justify_content(pos.justify_content.primary().value()),
        align_items: map_align(pos.align_items.0).unwrap_or(AlignItems::Stretch),
        width: size_to_dim(&pos.width),
        height: size_to_dim(&pos.height),
        min_width: size_to_dim(&pos.min_width),
        min_height: size_to_dim(&pos.min_height),
        max_width: max_size_to_dim(&pos.max_width),
        max_height: max_size_to_dim(&pos.max_height),
        margin,
        padding: meta.padding,
        border_width: meta.border.top,
        border_edges: (!uniform_border).then_some(meta.border),
        border_style_mask,
        gap: uniform_gap,
        row_gap,
        column_gap: col_gap,
        aspect_ratio: stylo_taffy::convert::aspect_ratio(pos.aspect_ratio),
        flex_grow: pos.flex_grow.0,
        flex_shrink: pos.flex_shrink.0,
        flex_basis: match &pos.flex_basis {
            GenericFlexBasis::Content => Dimension::Auto,
            GenericFlexBasis::Size(sz) => size_to_dim(sz),
        },
        align_self: map_align(pos.align_self.0),
        justify_items: map_align((pos.justify_items.computed.0).0),
        justify_self: map_align(pos.justify_self.0),
        box_sizing: match pos.box_sizing {
            StyloBoxSizing::BorderBox => BoxSizing::BorderBox,
            StyloBoxSizing::ContentBox => BoxSizing::ContentBox,
        },
        margin_dims,
        padding_dims,
        inset: extract_inset_edges(cv),
        grid_columns: 0,
        grid_template_columns: extract_grid_tracks(&pos.grid_template_columns),
        grid_template_rows: extract_grid_tracks(&pos.grid_template_rows),
        grid_auto_flow: map_grid_auto_flow(pos.grid_auto_flow),
        grid_column: GridLinePair {
            start: map_grid_line(&pos.grid_column_start),
            end: map_grid_line(&pos.grid_column_end),
        },
        grid_row: GridLinePair {
            start: map_grid_line(&pos.grid_row_start),
            end: map_grid_line(&pos.grid_row_end),
        },
        z_index: meta.z_index.unwrap_or(0),
        has_z_index: meta.z_index.is_some(),
        display_explicit_flex: matches!(
            meta.display,
            krui_style::DisplayMode::Flex | krui_style::DisplayMode::InlineFlex
        ),
        flex_direction_explicit: pos.flex_direction != StyloFlexDir::Row,
        padding_explicit: meta.padding != Edges::ZERO || padding_dims.is_some(),
        scrollbar_width: meta.scrollbar_width,
    }
}

fn extract_padding_dims(cv: &ComputedValues) -> Option<InsetEdges> {
    let pad = cv.get_padding();
    let raw = [
        lp_to_dim(&pad.padding_top.0),
        lp_to_dim(&pad.padding_right.0),
        lp_to_dim(&pad.padding_bottom.0),
        lp_to_dim(&pad.padding_left.0),
    ];
    raw.iter()
        .any(|d| !matches!(d, Dimension::Px(_)))
        .then_some(InsetEdges {
            top: raw[0],
            right: raw[1],
            bottom: raw[2],
            left: raw[3],
        })
}

fn extract_border_style_mask(cv: &ComputedValues) -> u8 {
    let brd = cv.get_border();
    u8::from(!brd.border_top_style.none_or_hidden())
        | (u8::from(!brd.border_right_style.none_or_hidden()) << 1)
        | (u8::from(!brd.border_bottom_style.none_or_hidden()) << 2)
        | (u8::from(!brd.border_left_style.none_or_hidden()) << 3)
}

fn extract_gaps(cv: &ComputedValues) -> (Option<f32>, Option<f32>, f32) {
    let pos = cv.get_position();
    let row_gap = gap_px(&pos.row_gap);
    let col_gap = gap_px(&pos.column_gap);
    let uniform_gap = match (row_gap, col_gap) {
        (Some(r), Some(c)) if (r - c).abs() < 0.01 => r,
        (Some(r), None) => r,
        _ => 0.0,
    };
    (row_gap, col_gap, uniform_gap)
}

fn is_uniform_edges(e: Edges) -> bool {
    (e.top - e.right).abs() < 1e-4
        && (e.top - e.bottom).abs() < 1e-4
        && (e.top - e.left).abs() < 1e-4
}

fn extract_inset_edges(cv: &ComputedValues) -> InsetEdges {
    let pos = cv.get_position();
    InsetEdges {
        top: inset_to_dim(&pos.top),
        right: inset_to_dim(&pos.right),
        bottom: inset_to_dim(&pos.bottom),
        left: inset_to_dim(&pos.left),
    }
}

fn lp_to_dim(lp: &LengthPercentage) -> Dimension {
    match lp.unpack() {
        Unpacked::Length(len) => Dimension::Px(len.px()),
        Unpacked::Percentage(pct) => Dimension::Percent(pct.0 * 100.0),
        Unpacked::Calc(calc_ptr) => Dimension::Px(calc_ptr.resolve(CSSPixelLength::new(0.0)).px()),
    }
}

fn size_to_dim(sz: &Size) -> Dimension {
    match sz {
        Size::LengthPercentage(lp) => lp_to_dim(&lp.0),
        _ => Dimension::Auto,
    }
}

fn max_size_to_dim(sz: &MaxSize) -> Dimension {
    match sz {
        MaxSize::LengthPercentage(lp) => lp_to_dim(&lp.0),
        _ => Dimension::Auto,
    }
}

fn margin_to_dim(m: &GenericMargin<LengthPercentage>) -> Dimension {
    match m {
        GenericMargin::LengthPercentage(lp) => lp_to_dim(lp),
        _ => Dimension::Auto,
    }
}

fn margin_dim_px(d: Dimension) -> f32 {
    match d {
        Dimension::Px(v) => v,
        _ => 0.0,
    }
}

fn inset_to_dim(
    i: &GenericInset<style::values::computed::Percentage, LengthPercentage>,
) -> Dimension {
    match i {
        GenericInset::LengthPercentage(lp) => lp_to_dim(lp),
        _ => Dimension::Auto,
    }
}

fn extract_margins(mar: &style::properties::style_structs::Margin) -> (Edges, Option<InsetEdges>) {
    let top = margin_to_dim(&mar.margin_top);
    let right = margin_to_dim(&mar.margin_right);
    let bottom = margin_to_dim(&mar.margin_bottom);
    let left = margin_to_dim(&mar.margin_left);
    let dims = [top, right, bottom, left];
    let has_non_px = dims.iter().any(|d| !matches!(d, Dimension::Px(_)));
    let px_edges = Edges {
        top: margin_dim_px(top),
        right: margin_dim_px(right),
        bottom: margin_dim_px(bottom),
        left: margin_dim_px(left),
    };
    let margin_dims = has_non_px.then_some(InsetEdges {
        top,
        right,
        bottom,
        left,
    });
    (px_edges, margin_dims)
}

fn gap_px(gap: &GenericLengthPercentageOrNormal<NonNegative<LengthPercentage>>) -> Option<f32> {
    match gap {
        GenericLengthPercentageOrNormal::Normal => None,
        GenericLengthPercentageOrNormal::LengthPercentage(lp) => Some(match lp_to_dim(&lp.0) {
            Dimension::Px(v) => v.max(0.0),
            _ => 0.0,
        }),
    }
}

fn map_flex_direction(dir: StyloFlexDir) -> FlexDirection {
    match dir {
        StyloFlexDir::Column | StyloFlexDir::ColumnReverse => FlexDirection::Column,
        StyloFlexDir::Row | StyloFlexDir::RowReverse => FlexDirection::Row,
    }
}

fn map_justify_content(flags: AlignFlags) -> JustifyContent {
    match flags.value() {
        AlignFlags::CENTER => JustifyContent::Center,
        AlignFlags::FLEX_END | AlignFlags::END | AlignFlags::RIGHT => JustifyContent::FlexEnd,
        AlignFlags::SPACE_BETWEEN => JustifyContent::SpaceBetween,
        AlignFlags::SPACE_AROUND => JustifyContent::SpaceAround,
        AlignFlags::SPACE_EVENLY => JustifyContent::SpaceEvenly,
        _ => JustifyContent::FlexStart,
    }
}

fn map_align(flags: AlignFlags) -> Option<AlignItems> {
    match flags.value() {
        AlignFlags::STRETCH => Some(AlignItems::Stretch),
        AlignFlags::FLEX_START | AlignFlags::START | AlignFlags::SELF_START => {
            Some(AlignItems::FlexStart)
        }
        AlignFlags::CENTER => Some(AlignItems::Center),
        AlignFlags::FLEX_END | AlignFlags::END | AlignFlags::SELF_END => Some(AlignItems::FlexEnd),
        AlignFlags::BASELINE => Some(AlignItems::Baseline),
        _ => None,
    }
}

fn map_grid_auto_flow(flow: style::computed_values::grid_auto_flow::T) -> GridAutoFlow {
    match stylo_taffy::convert::grid_auto_flow(flow) {
        taffy::GridAutoFlow::Row => GridAutoFlow::Row,
        taffy::GridAutoFlow::Column => GridAutoFlow::Column,
        taffy::GridAutoFlow::RowDense => GridAutoFlow::RowDense,
        taffy::GridAutoFlow::ColumnDense => GridAutoFlow::ColumnDense,
    }
}

fn map_grid_line(line: &GridLine) -> GridPlacementAtom {
    if line.is_auto() {
        GridPlacementAtom::Auto
    } else if line.is_span {
        GridPlacementAtom::Span(line.line_num.clamp(1, u16::MAX as i32) as u16)
    } else if line.line_num != 0 {
        GridPlacementAtom::Line(line.line_num.clamp(i16::MIN as i32, i16::MAX as i32) as i16)
    } else {
        GridPlacementAtom::Auto
    }
}

fn extract_grid_tracks(comp: &GridTemplateComponent) -> GridTrackList {
    let GenericGridTemplateComponent::TrackList(list) = comp else {
        return GridTrackList::EMPTY;
    };
    let mut out = Vec::new();
    for val in list.values.iter() {
        match val {
            TrackListValue::TrackSize(ts) => out.push(map_track_size(ts)),
            TrackListValue::TrackRepeat(rep) => match rep.count {
                RepeatCount::AutoFit | RepeatCount::AutoFill => {
                    if let Some(TrackSize::Minmax(min_b, max_b)) = rep.track_sizes.first() {
                        out.push(GridTrack::AutoRepeat {
                            mode: if matches!(rep.count, RepeatCount::AutoFit) {
                                AutoRepeatMode::AutoFit
                            } else {
                                AutoRepeatMode::AutoFill
                            },
                            min: map_min_track(min_b),
                            max: map_max_track(max_b),
                        });
                    }
                }
                RepeatCount::Number(n) => {
                    for _ in 0..(n.max(0) as usize).min(16) {
                        for ts in rep.track_sizes.iter() {
                            out.push(map_track_size(ts));
                        }
                    }
                }
            },
        }
    }
    GridTrackList::from_slice(&out)
}

fn map_min_track(b: &TrackBreadth<LengthPercentage>) -> GridMinTrack {
    match b {
        TrackBreadth::Auto => GridMinTrack::Auto,
        TrackBreadth::MinContent => GridMinTrack::MinContent,
        TrackBreadth::MaxContent => GridMinTrack::MaxContent,
        TrackBreadth::Breadth(lp) => match lp_to_dim(lp) {
            Dimension::Px(v) => GridMinTrack::Px(v),
            Dimension::Percent(p) => GridMinTrack::Percent(p),
            Dimension::Auto => GridMinTrack::Auto,
        },
        TrackBreadth::Flex(_) => GridMinTrack::Auto,
    }
}

fn map_max_track(b: &TrackBreadth<LengthPercentage>) -> GridMaxTrack {
    match b {
        TrackBreadth::Auto => GridMaxTrack::Auto,
        TrackBreadth::MinContent => GridMaxTrack::MinContent,
        TrackBreadth::MaxContent => GridMaxTrack::MaxContent,
        TrackBreadth::Flex(v) => GridMaxTrack::Fr(v.0),
        TrackBreadth::Breadth(lp) => match lp_to_dim(lp) {
            Dimension::Px(v) => GridMaxTrack::Px(v),
            Dimension::Percent(p) => GridMaxTrack::Percent(p),
            Dimension::Auto => GridMaxTrack::Auto,
        },
    }
}

fn map_track_size(ts: &TrackSize<LengthPercentage>) -> GridTrack {
    match ts {
        TrackSize::Breadth(b) => match b {
            TrackBreadth::Auto => GridTrack::Auto,
            TrackBreadth::MinContent => GridTrack::MinContent,
            TrackBreadth::MaxContent => GridTrack::MaxContent,
            TrackBreadth::Flex(v) => GridTrack::Fr(v.0),
            TrackBreadth::Breadth(lp) => match lp_to_dim(lp) {
                Dimension::Px(v) => GridTrack::Px(v),
                Dimension::Percent(p) => GridTrack::Percent(p),
                Dimension::Auto => GridTrack::Auto,
            },
        },
        TrackSize::Minmax(min_b, max_b) => GridTrack::MinMax {
            min: map_min_track(min_b),
            max: map_max_track(max_b),
        },
        TrackSize::FitContent(_) => GridTrack::Auto,
    }
}
