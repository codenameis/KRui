//! CSS `cursor` property representation (`CursorStyle`) extracted from Stylo into `PaintStyle`.

/// Computed CSS `cursor` keyword preserved on [`super::PaintStyle`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum CursorStyle {
    /// Browser/KRui semantic fallback (`text` for selectable text, `pointer` for buttons/links).
    #[default]
    Auto,
    /// Standard arrow pointer (`cursor: default`).
    Default,
    /// Hidden OS pointer (`cursor: none`).
    None,
    /// Hand/link pointer (`cursor: pointer`).
    Pointer,
    /// I-beam text selection cursor (`cursor: text`).
    Text,
    /// Precision crosshair (`cursor: crosshair`).
    Crosshair,
    /// Four-way move cursor (`cursor: move`).
    Move,
    /// Open hand grab cursor (`cursor: grab`).
    Grab,
    /// Closed hand dragging cursor (`cursor: grabbing`).
    Grabbing,
    /// Horizontal east-west resize cursor (`cursor: ew-resize` / `e-resize` / `w-resize`).
    EwResize,
    /// Vertical north-south resize cursor (`cursor: ns-resize` / `n-resize` / `s-resize`).
    NsResize,
    /// Diagonal NE-SW resize cursor (`cursor: nesw-resize` / `ne-resize` / `sw-resize`).
    NeswResize,
    /// Diagonal NW-SE resize cursor (`cursor: nwse-resize` / `nw-resize` / `se-resize`).
    NwseResize,
    /// Column divider resize cursor (`cursor: col-resize`).
    ColResize,
    /// Row divider resize cursor (`cursor: row-resize`).
    RowResize,
    /// Action forbidden cursor (`cursor: not-allowed` / `no-drop`).
    NotAllowed,
    /// Busy wait cursor (`cursor: wait`).
    Wait,
    /// Background busy progress cursor (`cursor: progress`).
    Progress,
    /// Magnifying glass zoom-in cursor (`cursor: zoom-in`).
    ZoomIn,
    /// Magnifying glass zoom-out cursor (`cursor: zoom-out`).
    ZoomOut,
    /// Context help cursor (`cursor: help`).
    Help,
    /// Drag copy indicator cursor (`cursor: copy`).
    Copy,
    /// Drag alias/shortcut indicator cursor (`cursor: alias`).
    Alias,
    /// Spreadsheet/grid cell selection cursor (`cursor: cell`).
    Cell,
    /// Omni-directional panning cursor (`cursor: all-scroll`).
    AllScroll,
}
