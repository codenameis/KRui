//! Conversion from KRui [`LayoutStyle`] overrides into Taffy 0.15 [`taffy::Style<style::Atom>`].
//!
//! This is the sole `LayoutStyle -> taffy::Style` converter in the workspace, used for the
//! synthetic viewport root, Tier-2 layout transitions, `LayoutSlot` dynamic bindings, and
//! standalone layout tests.

use krui_style::{
    AlignItems, AutoRepeatMode, BoxSizing, Dimension, DisplayMode, Edges, FlexDirection,
    GridAutoFlow, GridLinePair, GridMaxTrack, GridMinTrack, GridPlacementAtom, GridTrack,
    GridTrackList, InsetEdges, JustifyContent, LayoutStyle, OverflowMode, PositionMode,
};
use style::Atom;
use taffy::prelude::*;
use taffy::style::TrackSizingFunction;

/// Converts a KRui [`LayoutStyle`] into a Taffy 0.15 [`Style<Atom>`].
#[must_use]
pub(crate) fn layout_style_to_taffy(s: &LayoutStyle) -> Style<Atom> {
    let is_grid = s.display.is_grid();
    Style {
        display: to_taffy_display(s.display),
        box_sizing: match s.box_sizing {
            BoxSizing::BorderBox => taffy::BoxSizing::BorderBox,
            BoxSizing::ContentBox => taffy::BoxSizing::ContentBox,
        },
        position: to_taffy_position(s.position),
        overflow: taffy::Point {
            x: to_taffy_overflow(s.effective_overflow_x()),
            y: to_taffy_overflow(s.effective_overflow_y()),
        },
        aspect_ratio: s.aspect_ratio,
        flex_direction: match s.effective_flex_direction() {
            FlexDirection::Row => taffy::FlexDirection::Row,
            FlexDirection::Column => taffy::FlexDirection::Column,
        },
        flex_wrap: if s.flex_wrap {
            taffy::FlexWrap::Wrap
        } else {
            taffy::FlexWrap::NoWrap
        },
        flex_basis: to_taffy_dim(s.flex_basis),
        justify_content: to_taffy_justify(s.justify_content),
        align_items: to_taffy_align(s.align_items),
        align_self: resolve_taffy_align_self(s),
        justify_items: s
            .justify_items
            .map(to_taffy_align)
            .unwrap_or(taffy::AlignItems::NORMAL),
        justify_self: s.justify_self.map(to_taffy_align),
        size: Size {
            width: to_taffy_dim(s.width),
            height: to_taffy_dim(s.height),
        },
        min_size: Size {
            width: to_taffy_lpa(s.min_width),
            height: to_taffy_lpa(s.min_height),
        },
        max_size: Size {
            width: to_taffy_lpa(s.max_width),
            height: to_taffy_lpa(s.max_height),
        },
        margin: to_taffy_inset(s.effective_margin_dims()),
        padding: to_taffy_rect_lp_dims(s.effective_padding_dims()),
        border: to_taffy_rect_lp(s.effective_border_edges()),
        inset: to_taffy_inset(if s.position == PositionMode::Static {
            InsetEdges::default()
        } else {
            s.inset
        }),
        gap: Size {
            width: length(s.effective_column_gap()),
            height: length(s.effective_row_gap()),
        },
        flex_grow: s.flex_grow,
        flex_shrink: s.flex_shrink,
        grid_template_columns: resolve_taffy_grid_tracks(
            &s.grid_template_columns,
            s.grid_columns,
            is_grid,
        ),
        grid_template_rows: resolve_taffy_grid_tracks(&s.grid_template_rows, 0, is_grid),
        grid_auto_flow: to_taffy_grid_auto_flow(s.grid_auto_flow),
        grid_column: to_taffy_grid_line(s.grid_column),
        grid_row: to_taffy_grid_line(s.grid_row),
        ..Style::default()
    }
}

fn to_taffy_display(display: DisplayMode) -> Display {
    match display {
        DisplayMode::Flex | DisplayMode::InlineFlex => Display::Flex,
        DisplayMode::Grid | DisplayMode::InlineGrid => Display::Grid,
        DisplayMode::Block | DisplayMode::Inline | DisplayMode::InlineBlock => Display::Block,
        DisplayMode::Contents | DisplayMode::None => Display::None,
    }
}

fn to_taffy_position(pos: PositionMode) -> Position {
    match pos {
        PositionMode::Static => Position::Static,
        PositionMode::Relative | PositionMode::Sticky => Position::Relative,
        PositionMode::Absolute => Position::Absolute,
        PositionMode::Fixed => Position::Fixed,
    }
}

fn resolve_taffy_align_self(s: &LayoutStyle) -> Option<taffy::AlignItems> {
    s.align_self.map(to_taffy_align).or_else(|| {
        (matches!(
            s.display,
            DisplayMode::Inline
                | DisplayMode::InlineBlock
                | DisplayMode::InlineFlex
                | DisplayMode::InlineGrid
        ) && s.width == Dimension::Auto)
            .then_some(taffy::AlignItems::FLEX_START)
    })
}

fn to_taffy_grid_auto_flow(flow: GridAutoFlow) -> taffy::GridAutoFlow {
    match flow {
        GridAutoFlow::Row => taffy::GridAutoFlow::Row,
        GridAutoFlow::Column => taffy::GridAutoFlow::Column,
        GridAutoFlow::RowDense => taffy::GridAutoFlow::RowDense,
        GridAutoFlow::ColumnDense => taffy::GridAutoFlow::ColumnDense,
    }
}

fn to_taffy_overflow(overflow: OverflowMode) -> taffy::Overflow {
    match overflow {
        OverflowMode::Visible => taffy::Overflow::Visible,
        OverflowMode::Hidden => taffy::Overflow::Hidden,
        OverflowMode::Scroll => taffy::Overflow::Scroll,
    }
}

fn to_taffy_justify(jc: JustifyContent) -> taffy::JustifyContent {
    match jc {
        JustifyContent::FlexStart => taffy::JustifyContent::FLEX_START,
        JustifyContent::Center => taffy::JustifyContent::CENTER,
        JustifyContent::FlexEnd => taffy::JustifyContent::FLEX_END,
        JustifyContent::SpaceBetween => taffy::JustifyContent::SPACE_BETWEEN,
        JustifyContent::SpaceAround => taffy::JustifyContent::SPACE_AROUND,
        JustifyContent::SpaceEvenly => taffy::JustifyContent::SPACE_EVENLY,
    }
}

fn to_taffy_align(ai: AlignItems) -> taffy::AlignItems {
    match ai {
        AlignItems::Stretch => taffy::AlignItems::STRETCH,
        AlignItems::FlexStart => taffy::AlignItems::FLEX_START,
        AlignItems::Center => taffy::AlignItems::CENTER,
        AlignItems::FlexEnd => taffy::AlignItems::FLEX_END,
        AlignItems::Baseline => taffy::AlignItems::BASELINE,
    }
}

fn to_taffy_dim(dim: Dimension) -> taffy::Dimension {
    match dim {
        Dimension::Auto => auto(),
        Dimension::Px(v) => length(v),
        Dimension::Percent(p) => percent(p / 100.0),
    }
}

fn to_taffy_lpa(dim: Dimension) -> LengthPercentageAuto {
    match dim {
        Dimension::Auto => auto(),
        Dimension::Px(v) => length(v),
        Dimension::Percent(p) => percent(p / 100.0),
    }
}

fn to_taffy_lp_dim(dim: Dimension) -> LengthPercentage {
    match dim {
        Dimension::Px(v) => length(v),
        Dimension::Percent(p) => percent(p / 100.0),
        Dimension::Auto => length(0.0_f32),
    }
}

fn to_taffy_inset(inset: InsetEdges) -> Rect<LengthPercentageAuto> {
    Rect {
        top: to_taffy_lpa(inset.top),
        right: to_taffy_lpa(inset.right),
        bottom: to_taffy_lpa(inset.bottom),
        left: to_taffy_lpa(inset.left),
    }
}

fn to_taffy_rect_lp_dims(e: InsetEdges) -> Rect<LengthPercentage> {
    Rect {
        top: to_taffy_lp_dim(e.top),
        right: to_taffy_lp_dim(e.right),
        bottom: to_taffy_lp_dim(e.bottom),
        left: to_taffy_lp_dim(e.left),
    }
}

fn to_taffy_rect_lp(e: Edges) -> Rect<LengthPercentage> {
    Rect {
        top: length(e.top),
        right: length(e.right),
        bottom: length(e.bottom),
        left: length(e.left),
    }
}

fn to_taffy_grid_line(pair: GridLinePair) -> taffy::Line<taffy::GridPlacement<Atom>> {
    let map_atom = |atom: GridPlacementAtom| match atom {
        GridPlacementAtom::Auto => taffy::GridPlacement::Auto,
        GridPlacementAtom::Line(i) => line(i),
        GridPlacementAtom::Span(n) => span(n),
    };
    taffy::Line {
        start: map_atom(pair.start),
        end: map_atom(pair.end),
    }
}

fn to_min_sizing(min: GridMinTrack) -> MinTrackSizingFunction {
    match min {
        GridMinTrack::Auto => auto(),
        GridMinTrack::MinContent => min_content(),
        GridMinTrack::MaxContent => max_content(),
        GridMinTrack::Px(v) => length(v),
        GridMinTrack::Percent(p) => percent(p / 100.0),
    }
}

fn to_max_sizing(max: GridMaxTrack) -> MaxTrackSizingFunction {
    match max {
        GridMaxTrack::Auto => auto(),
        GridMaxTrack::MinContent => min_content(),
        GridMaxTrack::MaxContent => max_content(),
        GridMaxTrack::Px(v) => length(v),
        GridMaxTrack::Percent(p) => percent(p / 100.0),
        GridMaxTrack::Fr(v) => fr(v),
    }
}

fn to_taffy_track(track: GridTrack) -> taffy::GridTemplateComponent<Atom> {
    match track {
        GridTrack::Auto => taffy::GridTemplateComponent::from(TrackSizingFunction::AUTO),
        GridTrack::MinContent => {
            taffy::GridTemplateComponent::from(TrackSizingFunction::MIN_CONTENT)
        }
        GridTrack::MaxContent => {
            taffy::GridTemplateComponent::from(TrackSizingFunction::MAX_CONTENT)
        }
        GridTrack::Px(v) => taffy::GridTemplateComponent::from(TrackSizingFunction::from_length(v)),
        GridTrack::Percent(p) => {
            taffy::GridTemplateComponent::from(TrackSizingFunction::from_percent(p / 100.0))
        }
        GridTrack::Fr(v) => taffy::GridTemplateComponent::Single(minmax(length(0.0_f32), fr(v))),
        GridTrack::MinMaxPxFr { min_px, max_fr } => {
            taffy::GridTemplateComponent::Single(minmax(length(min_px), fr(max_fr)))
        }
        GridTrack::MinMax { min, max } => {
            taffy::GridTemplateComponent::Single(minmax(to_min_sizing(min), to_max_sizing(max)))
        }
        GridTrack::AutoRepeat { mode, min, max } => {
            let rep = match mode {
                AutoRepeatMode::AutoFit => RepetitionCount::AutoFit,
                AutoRepeatMode::AutoFill => RepetitionCount::AutoFill,
            };
            let single = minmax(to_min_sizing(min), to_max_sizing(max));
            repeat(rep, vec![single])
        }
    }
}

fn resolve_taffy_grid_tracks(
    tracks: &GridTrackList,
    legacy_cols: u16,
    is_grid: bool,
) -> Vec<taffy::GridTemplateComponent<Atom>> {
    if !is_grid {
        return Vec::new();
    }
    if !tracks.is_empty() {
        tracks
            .as_slice()
            .iter()
            .copied()
            .map(to_taffy_track)
            .collect()
    } else if legacy_cols > 0 {
        vec![
            taffy::GridTemplateComponent::Single(minmax(length(0.0_f32), fr(1.0_f32)));
            legacy_cols as usize
        ]
    } else {
        Vec::new()
    }
}
