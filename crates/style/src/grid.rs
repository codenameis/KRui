//! CSS Grid track sizing and placement (`GridTrack`, `GridTrackList`, `GridLinePair`) for `LayoutStyle`.

/// Maximum number of explicit CSS Grid tracks stored inline in a [`GridTrackList`].
pub const MAX_GRID_TRACKS: usize = 16;

/// CSS Grid `repeat(...)` auto-placement mode (`auto-fit` vs `auto-fill`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum AutoRepeatMode {
    /// Collapse empty repeated tracks (`repeat(auto-fit, ...)`).
    #[default]
    AutoFit,
    /// Preserve empty repeated tracks (`repeat(auto-fill, ...)`).
    AutoFill,
}

/// Minimum bound of a CSS Grid track or `minmax(...)` function.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum GridMinTrack {
    /// Automatic intrinsic minimum (`auto`).
    #[default]
    Auto,
    /// Minimum intrinsic content size (`min-content`).
    MinContent,
    /// Maximum intrinsic content size (`max-content`).
    MaxContent,
    /// Fixed logical pixel minimum (`200px`).
    Px(f32),
    /// Percentage of grid container dimension (`25%`).
    Percent(f32),
}

/// Maximum bound of a CSS Grid track or `minmax(...)` function.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum GridMaxTrack {
    /// Automatic intrinsic maximum (`auto`).
    #[default]
    Auto,
    /// Minimum intrinsic content size (`min-content`).
    MinContent,
    /// Maximum intrinsic content size (`max-content`).
    MaxContent,
    /// Fixed logical pixel maximum (`320px`).
    Px(f32),
    /// Percentage of grid container dimension (`50%`).
    Percent(f32),
    /// Flexible fraction of remaining free space (`1fr`).
    Fr(f32),
}

/// CSS `grid-auto-flow` placement algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum GridAutoFlow {
    /// Place items by filling each row in turn (`row`).
    #[default]
    Row,
    /// Place items by filling each column in turn (`column`).
    Column,
    /// Fill holes left by larger items while scanning rows (`row dense`).
    RowDense,
    /// Fill holes left by larger items while scanning columns (`column dense`).
    ColumnDense,
}

/// Single grid line placement endpoint (`grid-column-start`, `grid-row-end`, etc.).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum GridPlacementAtom {
    /// Automatic grid placement (`auto`).
    #[default]
    Auto,
    /// 1-based explicit grid line index (negative counts from the explicit end).
    Line(i16),
    /// Positive span count (`span 4`).
    Span(u16),
}

/// Start and end grid line placement pair (`grid-column` / `grid-row`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct GridLinePair {
    /// Starting grid line placement.
    pub start: GridPlacementAtom,
    /// Ending grid line placement.
    pub end: GridPlacementAtom,
}

impl GridLinePair {
    /// Default automatic placement (`auto / auto`).
    pub const AUTO: Self = Self {
        start: GridPlacementAtom::Auto,
        end: GridPlacementAtom::Auto,
    };
}

/// Single CSS Grid track sizing definition (`grid-template-columns` / `grid-template-rows`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum GridTrack {
    /// Content-sized track (`auto`).
    #[default]
    Auto,
    /// Minimum intrinsic content track (`min-content`).
    MinContent,
    /// Maximum intrinsic content track (`max-content`).
    MaxContent,
    /// Fixed pixel track (`120px`).
    Px(f32),
    /// Percentage of grid container size (`25%`).
    Percent(f32),
    /// Flexible fraction of remaining free space (`1fr`, `2fr`).
    Fr(f32),
    /// Shorthand bounded flexible track (`minmax(200px, 1fr)`).
    MinMaxPxFr {
        /// Minimum track size in logical pixels.
        min_px: f32,
        /// Maximum flexible fraction weight.
        max_fr: f32,
    },
    /// General `minmax(min, max)` track.
    MinMax {
        /// Minimum track sizing bound.
        min: GridMinTrack,
        /// Maximum track sizing bound.
        max: GridMaxTrack,
    },
    /// Responsive `repeat(auto-fit | auto-fill, minmax(min, max))` track generator.
    AutoRepeat {
        /// `auto-fit` vs `auto-fill` repetition strategy.
        mode: AutoRepeatMode,
        /// Minimum track size for each repeated column/row.
        min: GridMinTrack,
        /// Maximum track size for each repeated column/row.
        max: GridMaxTrack,
    },
}

/// Fixed-capacity inline list of up to [`MAX_GRID_TRACKS`] CSS Grid track definitions.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct GridTrackList {
    /// Inline array of track definitions (`tracks[..len as usize]` are active).
    pub tracks: [GridTrack; MAX_GRID_TRACKS],
    /// Number of active tracks (`0..=16`).
    pub len: u8,
}

impl Default for GridTrackList {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl GridTrackList {
    /// Empty track list (`0` explicit tracks).
    pub const EMPTY: Self = Self {
        tracks: [GridTrack::Auto; MAX_GRID_TRACKS],
        len: 0,
    };

    /// Builds a [`GridTrackList`] from up to [`MAX_GRID_TRACKS`] tracks (clamping excess tracks to 16).
    pub fn from_slice(slice: &[GridTrack]) -> Self {
        let count = slice.len().min(MAX_GRID_TRACKS);
        let mut tracks = [GridTrack::Auto; MAX_GRID_TRACKS];
        tracks[..count].copy_from_slice(&slice[..count]);
        Self {
            tracks,
            len: count as u8,
        }
    }

    /// Builds `count` (`0..=16`) identical tracks of `track`.
    pub const fn repeat_track(count: u8, track: GridTrack) -> Self {
        let clamped = if (count as usize) > MAX_GRID_TRACKS {
            MAX_GRID_TRACKS
        } else {
            count as usize
        };
        let mut tracks = [GridTrack::Auto; MAX_GRID_TRACKS];
        let mut i = 0;
        while i < clamped {
            tracks[i] = track;
            i += 1;
        }
        Self {
            tracks,
            len: clamped as u8,
        }
    }

    /// Builds `repeat(count, <fr_value>fr)` up to [`MAX_GRID_TRACKS`] tracks.
    pub const fn repeat_fr(count: u8, fr_value: f32) -> Self {
        Self::repeat_track(count, GridTrack::Fr(fr_value))
    }

    /// Returns the active slice of [`GridTrack`] definitions.
    pub fn as_slice(&self) -> &[GridTrack] {
        let n = (self.len as usize).min(MAX_GRID_TRACKS);
        &self.tracks[..n]
    }

    /// Number of active tracks (`0..=16`).
    pub const fn len(&self) -> usize {
        if (self.len as usize) > MAX_GRID_TRACKS {
            MAX_GRID_TRACKS
        } else {
            self.len as usize
        }
    }

    /// Returns `true` when no explicit grid tracks are configured.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}
