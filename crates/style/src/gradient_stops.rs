//! Shared multi-stop gradient slices and CSS `conic-gradient(...)` style types.

use crate::color::Color;
use crate::gradient_radial::RadialCoord;
use std::sync::{Arc, OnceLock, RwLock};

/// Single normalized color stop inside a multi-stop or conic gradient (`0.0..=1.0`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct GradientStop {
    /// Stop color in 8-bit sRGB.
    pub color: Color,
    /// Normalized stop position along the gradient axis or angle span (`0.0..=1.0`).
    pub position: f32,
}

static GRADIENT_STOPS: OnceLock<RwLock<Vec<Arc<[GradientStop]>>>> = OnceLock::new();

fn stops_store() -> &'static RwLock<Vec<Arc<[GradientStop]>>> {
    GRADIENT_STOPS.get_or_init(|| RwLock::new(Vec::new()))
}

fn intern_gradient_stops(stops: Vec<GradientStop>) -> u32 {
    if let Ok(read) = stops_store().read() {
        for (idx, existing) in read.iter().enumerate() {
            if existing.as_ref() == stops.as_slice() {
                return idx as u32;
            }
        }
    }
    let mut write = stops_store()
        .write()
        .unwrap_or_else(|poison| poison.into_inner());
    for (idx, existing) in write.iter().enumerate() {
        if existing.as_ref() == stops.as_slice() {
            return idx as u32;
        }
    }
    let idx = write.len() as u32;
    write.push(Arc::from(stops));
    idx
}

/// `Copy` + `const`-constructible handle to ordered multi-stop gradient colors.
#[derive(Clone, Copy, Debug, Default)]
pub enum GradientStopsRef {
    #[default]
    Empty,
    Static(&'static [GradientStop]),
    Interned(u32),
}

impl GradientStopsRef {
    /// Creates a compile-time static stop slice reference.
    pub const fn static_ref(stops: &'static [GradientStop]) -> Self {
        if stops.is_empty() {
            Self::Empty
        } else {
            Self::Static(stops)
        }
    }

    /// Interns a dynamic slice of gradient stops into a `Copy` handle.
    pub fn from_stops(stops: &[GradientStop]) -> Self {
        if stops.is_empty() {
            Self::Empty
        } else {
            Self::Interned(intern_gradient_stops(stops.to_vec()))
        }
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Passes the underlying [`GradientStop`] slice to `f`.
    pub fn with_stops<R>(self, f: impl FnOnce(&[GradientStop]) -> R) -> R {
        match self {
            Self::Empty => f(&[]),
            Self::Static(s) => f(s),
            Self::Interned(idx) => {
                let arc = stops_store()
                    .read()
                    .ok()
                    .and_then(|vec| vec.get(idx as usize).cloned());
                match arc {
                    Some(a) => f(&a),
                    None => f(&[]),
                }
            }
        }
    }

    /// Returns a fast deterministic 64-bit content hash for GPU LUT caching.
    pub fn cache_key(self) -> u64 {
        self.with_stops(|stops| {
            let mut h = 0xcbf2_9ce4_8422_2325_u64 ^ (stops.len() as u64);
            for s in stops {
                let packed = u32::from_le_bytes([s.color.r, s.color.g, s.color.b, s.color.a]);
                h = (h ^ packed as u64).wrapping_mul(0x0100_0000_01b3);
                h = (h ^ s.position.to_bits() as u64).wrapping_mul(0x0100_0000_01b3);
            }
            h
        })
    }
}

impl PartialEq for GradientStopsRef {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) => true,
            (Self::Static(a), Self::Static(b)) => a == b,
            (Self::Interned(a), Self::Interned(b)) if a == b => true,
            _ => {
                let copy_b: Vec<GradientStop> = other.with_stops(|s| s.to_vec());
                self.with_stops(|a| a == copy_b.as_slice())
            }
        }
    }
}
impl Eq for GradientStopsRef {}

#[cfg(feature = "preview")]
impl serde::Serialize for GradientStopsRef {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.with_stops(|stops| stops.serialize(s))
    }
}

#[cfg(feature = "preview")]
impl<'de> serde::Deserialize<'de> for GradientStopsRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let stops = Vec::<GradientStop>::deserialize(d)?;
        Ok(Self::from_stops(&stops))
    }
}

/// Evaluates normalized stop offset `t` (`0.0..=1.0`) in premultiplied RGBA `[0..1; 4]`.
pub fn sample_premul_rgba(stops: &[GradientStop], t: f32) -> [f32; 4] {
    if stops.is_empty() {
        return [0.0; 4];
    }
    let tc = t.clamp(0.0, 1.0);
    if tc <= stops[0].position {
        return to_premul_f32(stops[0].color);
    }
    let last = stops[stops.len() - 1];
    if tc >= last.position {
        return to_premul_f32(last.color);
    }
    for win in stops.windows(2) {
        let (a, b) = (win[0], win[1]);
        if tc <= b.position {
            let span = (b.position - a.position).max(1e-5);
            let f = ((tc - a.position) / span).clamp(0.0, 1.0);
            let pa = to_premul_f32(a.color);
            let pb = to_premul_f32(b.color);
            return [
                pa[0] + (pb[0] - pa[0]) * f,
                pa[1] + (pb[1] - pa[1]) * f,
                pa[2] + (pb[2] - pa[2]) * f,
                pa[3] + (pb[3] - pa[3]) * f,
            ];
        }
    }
    to_premul_f32(last.color)
}

#[inline]
fn to_premul_f32(c: Color) -> [f32; 4] {
    let a = c.a as f32 / 255.0;
    [
        (c.r as f32 / 255.0) * a,
        (c.g as f32 / 255.0) * a,
        (c.b as f32 / 255.0) * a,
        a,
    ]
}

/// Rasterizes `stops` across `[0.0..=max_pos]` into a premultiplied `width x 1` RGBA8 LUT.
pub fn rasterize_lut_rgba8(stops: &[GradientStop], width: usize, max_pos: f32) -> Vec<u8> {
    let w = width.max(2);
    let span = max_pos.clamp(1e-4, 1.0);
    let mut out = Vec::with_capacity(w * 4);
    for i in 0..w {
        let t = (i as f32 / (w - 1) as f32) * span;
        let pm = sample_premul_rgba(stops, t);
        for ch in pm {
            out.push((ch.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    out
}

/// CSS `conic-gradient(...)` specification with pointer-bound center coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct ConicGradient {
    /// Starting angle in degrees clockwise from 12 o'clock (`from <angle>`).
    pub from_angle_deg: f32,
    /// Horizontal center coordinate.
    pub center_x: RadialCoord,
    /// Vertical center coordinate.
    pub center_y: RadialCoord,
    /// Ordered stop list (`position` normalized in `0.0..=1.0`).
    pub stops: GradientStopsRef,
    /// `true` for `repeating-conic-gradient(...)`.
    pub repeating: bool,
}

impl Default for ConicGradient {
    fn default() -> Self {
        Self {
            from_angle_deg: 0.0,
            center_x: RadialCoord::CENTER,
            center_y: RadialCoord::CENTER,
            stops: GradientStopsRef::Empty,
            repeating: false,
        }
    }
}

impl ConicGradient {
    /// Returns `true` when either center coordinate reads `--krui-pointer-*`.
    #[inline]
    pub const fn uses_latched_pointer(&self) -> bool {
        self.center_x.uses_latched_pointer() || self.center_y.uses_latched_pointer()
    }

    /// Returns `true` when either center coordinate reads `--krui-hover-*`.
    #[inline]
    pub const fn uses_hover_pointer(&self) -> bool {
        self.center_x.uses_hover_pointer() || self.center_y.uses_hover_pointer()
    }
}
