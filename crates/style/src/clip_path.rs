//! CSS `clip-path` basic-shape representations (`polygon()`, `path()`, `circle()`, `ellipse()`, `inset()`).

use std::sync::{Arc, OnceLock, RwLock};

/// SVG / CSS polygon fill rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum ClipFillRule {
    /// Non-zero winding rule (CSS/SVG default).
    #[default]
    NonZero,
    /// Even-odd alternating parity rule.
    EvenOdd,
}

/// 1D coordinate resolving in logical pixels or percent of box width/height.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct ClipCoord {
    /// Fixed offset in logical pixels.
    pub px: f32,
    /// Normalized box fraction (`0.5` = `50%`).
    pub frac: f32,
}

impl ClipCoord {
    pub const ZERO: Self = Self { px: 0.0, frac: 0.0 };

    #[inline]
    pub const fn px(px: f32) -> Self {
        Self { px, frac: 0.0 }
    }

    #[inline]
    pub const fn frac(frac: f32) -> Self {
        Self { px: 0.0, frac }
    }

    #[inline]
    pub fn resolve(self, extent: f32) -> f32 {
        self.px + extent * self.frac
    }
}

/// CSS `circle()` / `ellipse()` radius length or keyword.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum ClipRadius {
    /// Explicit length / percentage against reference dimension.
    Length(ClipCoord),
    /// Distance from shape center to the closest box side.
    #[default]
    ClosestSide,
    /// Distance from shape center to the farthest box side.
    FarthestSide,
}

/// SVG `path()` drawing command inside `clip-path: path(...)`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum ClipPathCmd {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
}

static CLIP_POLYGONS: OnceLock<RwLock<Vec<Arc<[ClipCoord]>>>> = OnceLock::new();
static CLIP_PATH_CMDS: OnceLock<RwLock<Vec<Arc<[ClipPathCmd]>>>> = OnceLock::new();

fn intern_vec<T: PartialEq + Send + Sync + 'static>(
    lock: &'static OnceLock<RwLock<Vec<Arc<[T]>>>>,
    items: Vec<T>,
) -> u32 {
    let store = lock.get_or_init(|| RwLock::new(Vec::new()));
    if let Ok(read) = store.read() {
        for (i, ex) in read.iter().enumerate() {
            if ex.as_ref() == items.as_slice() {
                return i as u32;
            }
        }
    }
    let mut write = store.write().unwrap_or_else(|p| p.into_inner());
    for (i, ex) in write.iter().enumerate() {
        if ex.as_ref() == items.as_slice() {
            return i as u32;
        }
    }
    let idx = write.len() as u32;
    write.push(Arc::from(items));
    idx
}

/// Interned `Copy` handle storing `[x0, y0, x1, y1, ...]` vertices for `polygon(...)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct ClipPolygonRef(pub u32);

impl ClipPolygonRef {
    /// Interns `(x, y)` coordinate pairs into `ClipPolygonRef`.
    pub fn from_pairs(pairs: &[(ClipCoord, ClipCoord)]) -> Self {
        let mut flat = Vec::with_capacity(pairs.len() * 2);
        for &(x, y) in pairs {
            flat.push(x);
            flat.push(y);
        }
        Self(intern_vec(&CLIP_POLYGONS, flat))
    }

    /// Passes `(x, y)` slice in flat `ClipCoord` pairs to `f`.
    pub fn with_coords<R>(self, f: impl FnOnce(&[ClipCoord]) -> R) -> R {
        let arc = CLIP_POLYGONS
            .get()
            .and_then(|s| s.read().ok())
            .and_then(|v| v.get(self.0 as usize).cloned());
        match arc {
            Some(a) => f(&a),
            None => f(&[]),
        }
    }
}

/// Interned `Copy` handle storing normalized SVG `path(...)` commands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct ClipPathCmdsRef(pub u32);

impl ClipPathCmdsRef {
    /// Interns a slice of normalized [`ClipPathCmd`]s.
    pub fn from_cmds(cmds: &[ClipPathCmd]) -> Self {
        Self(intern_vec(&CLIP_PATH_CMDS, cmds.to_vec()))
    }

    /// Passes underlying [`ClipPathCmd`] slice to `f`.
    pub fn with_cmds<R>(self, f: impl FnOnce(&[ClipPathCmd]) -> R) -> R {
        let arc = CLIP_PATH_CMDS
            .get()
            .and_then(|s| s.read().ok())
            .and_then(|v| v.get(self.0 as usize).cloned());
        match arc {
            Some(a) => f(&a),
            None => f(&[]),
        }
    }
}

/// Compact `Copy` CSS `clip-path` geometry stored on `PaintStyle`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum ClipShape {
    Polygon {
        fill_rule: ClipFillRule,
        points: ClipPolygonRef,
    },
    Path {
        fill_rule: ClipFillRule,
        cmds: ClipPathCmdsRef,
    },
    Circle {
        cx: ClipCoord,
        cy: ClipCoord,
        radius: ClipRadius,
    },
    Ellipse {
        cx: ClipCoord,
        cy: ClipCoord,
        rx: ClipRadius,
        ry: ClipRadius,
    },
    Inset {
        top: ClipCoord,
        right: ClipCoord,
        bottom: ClipCoord,
        left: ClipCoord,
        radii: [f32; 4],
    },
}
