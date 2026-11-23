//! W3C CSS Easing Functions Level 1 & Level 2 (`ease*`, `cubic-bezier()`, `steps()`, `linear()`).

use crate::transition_lerp::solve_cubic_bezier;
use std::fmt;
use std::sync::{Arc, OnceLock, RwLock};

static LINEAR_STOP_REGISTRY: OnceLock<RwLock<Vec<Arc<[LinearStop]>>>> = OnceLock::new();

fn linear_registry() -> &'static RwLock<Vec<Arc<[LinearStop]>>> {
    LINEAR_STOP_REGISTRY.get_or_init(|| RwLock::new(Vec::new()))
}

/// Step-position keyword for CSS `steps(<integer>, <step-position>?)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum StepPosition {
    /// Step occurs at interval start (`start` / `jump-start`).
    JumpStart,
    /// Step occurs at interval end (`end` / `jump-end`, default).
    JumpEnd,
    /// Neither endpoint steps; `n - 1` jumps (`jump-none`, requires `count >= 2`).
    JumpNone,
    /// Both endpoints step; `n + 1` jumps (`jump-both`).
    JumpBoth,
}

/// Single normalized `(input_progress, output_progress)` stop in a CSS `linear()` curve.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct LinearStop {
    /// Input progress value (normally `0.0..=1.0`, explicit percentages may lie outside).
    pub t: f32,
    /// Output progress value (arbitrary finite `f32`).
    pub y: f32,
}

/// Copyable reference to CSS `linear(...)` piecewise linear stops.
#[derive(Clone, Copy)]
pub enum LinearCurveRef {
    /// Compile-time static slice emitted by `krui-macros`.
    Static(&'static [LinearStop]),
    /// Runtime-interned stop slice stored in a process-wide registry without leaking.
    Interned(u32),
}

impl LinearCurveRef {
    /// Wraps a compile-time static slice of stops into a `LinearCurveRef`.
    #[inline]
    pub const fn static_stops(stops: &'static [LinearStop]) -> Self {
        Self::Static(stops)
    }

    /// Interns a normalized stop list in the runtime registry (deduplicated by float bit patterns).
    pub fn intern(stops: Vec<LinearStop>) -> Self {
        let mut reg = linear_registry().write().unwrap_or_else(|e| e.into_inner());
        if let Some((idx, _)) = reg.iter().enumerate().find(|(_, existing)| {
            existing.len() == stops.len()
                && existing
                    .iter()
                    .zip(stops.iter())
                    .all(|(a, b)| a.t.to_bits() == b.t.to_bits() && a.y.to_bits() == b.y.to_bits())
        }) {
            return Self::Interned(idx as u32);
        }
        let idx = reg.len() as u32;
        reg.push(Arc::from(stops));
        Self::Interned(idx)
    }

    /// Passes the resolved stop slice to `f` without heap allocation or nested locking.
    pub fn with_stops<R>(self, f: impl FnOnce(&[LinearStop]) -> R) -> R {
        match self {
            Self::Static(s) => f(s),
            Self::Interned(id) => {
                let reg = linear_registry().read().unwrap_or_else(|e| e.into_inner());
                let slice = reg.get(id as usize).map_or(&[][..], |arc| &arc[..]);
                f(slice)
            }
        }
    }
}

impl PartialEq for LinearCurveRef {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Static(a), Self::Static(b)) => a == b,
            (Self::Interned(a), Self::Interned(b)) if a == b => true,
            (Self::Interned(a), Self::Interned(b)) => {
                let reg = linear_registry().read().unwrap_or_else(|e| e.into_inner());
                let sa = reg.get(a as usize).map_or(&[][..], |arc| &arc[..]);
                let sb = reg.get(b as usize).map_or(&[][..], |arc| &arc[..]);
                sa == sb
            }
            (Self::Static(s), Self::Interned(id)) | (Self::Interned(id), Self::Static(s)) => {
                let reg = linear_registry().read().unwrap_or_else(|e| e.into_inner());
                let sb = reg.get(id as usize).map_or(&[][..], |arc| &arc[..]);
                s == sb
            }
        }
    }
}

impl fmt::Debug for LinearCurveRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.with_stops(|stops| f.debug_tuple("LinearStops").field(&stops).finish())
    }
}

#[cfg(feature = "preview")]
impl serde::Serialize for LinearCurveRef {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.with_stops(|stops| stops.serialize(serializer))
    }
}

#[cfg(feature = "preview")]
impl<'de> serde::Deserialize<'de> for LinearCurveRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stops = Vec::<LinearStop>::deserialize(deserializer)?;
        Ok(Self::intern(stops))
    }
}

/// W3C CSS easing / timing function (`transition-timing-function`, `animation-timing-function`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum TransitionCurve {
    /// CSS `linear` (`(0,0) -> (1,1)`).
    Linear,
    /// CSS `ease` (`0.25, 0.1, 0.25, 1.0`, W3C default).
    #[default]
    Ease,
    /// CSS `ease-in` (`0.42, 0.0, 1.0, 1.0`).
    EaseIn,
    /// CSS `ease-out` (`0.0, 0.0, 0.58, 1.0`).
    EaseOut,
    /// CSS `ease-in-out` (`0.42, 0.0, 0.58, 1.0`).
    EaseInOut,
    /// Arbitrary CSS `cubic-bezier(x1, y1, x2, y2)`.
    CubicBezier { x1: f32, y1: f32, x2: f32, y2: f32 },
    /// Discrete CSS `steps(count, pos)` / `step-start` / `step-end`.
    Steps { count: u16, pos: StepPosition },
    /// W3C CSS Easing L2 piecewise `linear(...)` curve.
    LinearStops(LinearCurveRef),
}

impl TransitionCurve {
    /// Constructs a `TransitionCurve::LinearStops` from compile-time static stops.
    #[inline]
    pub const fn linear_static(stops: &'static [LinearStop]) -> Self {
        Self::LinearStops(LinearCurveRef::static_stops(stops))
    }

    /// Invokes `f` with the stops slice when `self` is `LinearStops`.
    pub fn with_linear_stops<R>(self, f: impl FnOnce(&[LinearStop]) -> R) -> Option<R> {
        match self {
            Self::LinearStops(r) => Some(r.with_stops(f)),
            _ => None,
        }
    }

    /// Parses a standard CSS easing function (`linear`, `ease*`, `cubic-bezier(...)`, `steps(...)`, `linear(...)`).
    pub fn parse_css(raw: &str) -> Option<Self> {
        let lower = raw.trim().to_ascii_lowercase();
        match lower.as_str() {
            "linear" => Some(Self::Linear),
            "ease" => Some(Self::Ease),
            "ease-in" => Some(Self::EaseIn),
            "ease-out" => Some(Self::EaseOut),
            "ease-in-out" => Some(Self::EaseInOut),
            "step-start" => Some(Self::Steps {
                count: 1,
                pos: StepPosition::JumpStart,
            }),
            "step-end" => Some(Self::Steps {
                count: 1,
                pos: StepPosition::JumpEnd,
            }),
            _ if lower.starts_with("cubic-bezier(") && lower.ends_with(')') => {
                parse_cubic_bezier(&lower["cubic-bezier(".len()..lower.len() - 1])
            }
            _ if lower.starts_with("steps(") && lower.ends_with(')') => {
                parse_steps_fn(&lower["steps(".len()..lower.len() - 1])
            }
            _ if lower.starts_with("linear(") && lower.ends_with(')') => {
                parse_linear_fn(&lower["linear(".len()..lower.len() - 1])
            }
            _ => None,
        }
    }

    /// Samples this easing curve at progress `t` (with default forward `before = false`).
    #[inline]
    pub fn sample(self, t: f32) -> f32 {
        self.sample_with_before(t, false)
    }

    /// Samples this easing curve at progress `t` using W3C `before` flag semantics.
    #[inline]
    pub fn sample_with_before(self, t: f32, before: bool) -> f32 {
        match self {
            Self::Linear => t,
            Self::Ease => solve_cubic_bezier(0.25, 0.1, 0.25, 1.0, t),
            Self::EaseIn => solve_cubic_bezier(0.42, 0.0, 1.0, 1.0, t),
            Self::EaseOut => solve_cubic_bezier(0.0, 0.0, 0.58, 1.0, t),
            Self::EaseInOut => solve_cubic_bezier(0.42, 0.0, 0.58, 1.0, t),
            Self::CubicBezier { x1, y1, x2, y2 } => solve_cubic_bezier(x1, y1, x2, y2, t),
            Self::Steps { count, pos } => sample_steps(count, pos, t, before),
            Self::LinearStops(stops) => {
                stops.with_stops(|slice| sample_linear_stops(slice, t, before))
            }
        }
    }
}

fn parse_cubic_bezier(inner: &str) -> Option<TransitionCurve> {
    let nums: Vec<f32> = inner
        .split(',')
        .map(|s| s.trim().parse::<f32>().ok().filter(|v| v.is_finite()))
        .collect::<Option<Vec<_>>>()?;
    if nums.len() != 4 || !(0.0..=1.0).contains(&nums[0]) || !(0.0..=1.0).contains(&nums[2]) {
        return None;
    }
    Some(TransitionCurve::CubicBezier {
        x1: nums[0],
        y1: nums[1],
        x2: nums[2],
        y2: nums[3],
    })
}

fn parse_steps_fn(inner: &str) -> Option<TransitionCurve> {
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.is_empty() || parts.len() > 2 {
        return None;
    }
    let count = parts[0].parse::<u16>().ok().filter(|&n| n >= 1)?;
    let pos = match parts.get(1).copied() {
        None | Some("end" | "jump-end") => StepPosition::JumpEnd,
        Some("start" | "jump-start") => StepPosition::JumpStart,
        Some("jump-none") if count >= 2 => StepPosition::JumpNone,
        Some("jump-both") => StepPosition::JumpBoth,
        _ => return None,
    };
    Some(TransitionCurve::Steps { count, pos })
}

fn parse_linear_fn(inner: &str) -> Option<TransitionCurve> {
    let mut raw_points: Vec<(f32, Option<f32>)> = Vec::new();
    for seg in inner.split(',') {
        let tokens: Vec<&str> = seg.split_whitespace().collect();
        if tokens.is_empty() || tokens.len() > 3 {
            return None;
        }
        let y = tokens[0].parse::<f32>().ok().filter(|v| v.is_finite())?;
        if tokens.len() == 1 {
            raw_points.push((y, None));
        } else {
            for pct_tok in &tokens[1..] {
                let pct = pct_tok.strip_suffix('%')?.parse::<f32>().ok()?;
                if !pct.is_finite() {
                    return None;
                }
                raw_points.push((y, Some(pct / 100.0)));
            }
        }
    }
    if raw_points.len() < 2 {
        return None;
    }
    let stops = normalize_linear_stops(&mut raw_points);
    Some(TransitionCurve::LinearStops(LinearCurveRef::intern(stops)))
}

fn normalize_linear_stops(raw: &mut [(f32, Option<f32>)]) -> Vec<LinearStop> {
    if raw[0].1.is_none() {
        raw[0].1 = Some(0.0);
    }
    let mut max_in = raw[0].1.unwrap_or(0.0);
    for (_, pos) in raw.iter_mut() {
        if let Some(p) = pos {
            if *p < max_in {
                *p = max_in;
            } else {
                max_in = *p;
            }
        }
    }
    let last = raw.len() - 1;
    if raw[last].1.is_none() {
        raw[last].1 = Some(max_in.max(1.0));
    }
    let mut i = 0;
    while i < raw.len() {
        if raw[i].1.is_some() {
            i += 1;
            continue;
        }
        let prev_idx = i - 1;
        let next_idx = ((i + 1)..raw.len())
            .find(|&k| raw[k].1.is_some())
            .unwrap_or(last);
        let t0 = raw[prev_idx].1.unwrap_or(0.0);
        let t1 = raw[next_idx].1.unwrap_or(1.0);
        let span = (next_idx - prev_idx) as f32;
        for (offset, slot) in raw[i..next_idx].iter_mut().enumerate() {
            let frac = (i + offset - prev_idx) as f32 / span;
            slot.1 = Some(t0 + (t1 - t0) * frac);
        }
        i = next_idx + 1;
    }
    raw.iter()
        .map(|(y, t)| LinearStop {
            t: t.unwrap_or(0.0),
            y: *y,
        })
        .collect()
}

fn sample_steps(count: u16, pos: StepPosition, t: f32, before: bool) -> f32 {
    let n = f64::from(count);
    let prod = f64::from(t) * n;
    let mut step = prod.floor() as i32;
    if matches!(pos, StepPosition::JumpStart | StepPosition::JumpBoth) {
        step += 1;
    }
    if before && (prod - prod.round()).abs() < 1e-6 {
        step -= 1;
    }
    if t >= 0.0 && step < 0 {
        step = 0;
    }
    let jumps = match pos {
        StepPosition::JumpStart | StepPosition::JumpEnd => i32::from(count),
        StepPosition::JumpNone => i32::from(count) - 1,
        StepPosition::JumpBoth => i32::from(count) + 1,
    };
    if t <= 1.0 && step > jumps {
        step = jumps;
    }
    let denom = match pos {
        StepPosition::JumpStart | StepPosition::JumpEnd => n,
        StepPosition::JumpNone => f64::from(count - 1).max(1.0),
        StepPosition::JumpBoth => n + 1.0,
    };
    (f64::from(step) / denom) as f32
}

fn sample_linear_stops(stops: &[LinearStop], x: f32, before: bool) -> f32 {
    if stops.is_empty() {
        return x;
    }
    if stops.len() == 1 {
        return stops[0].y;
    }
    if x < stops[0].t {
        let dx = stops[1].t - stops[0].t;
        return if dx.abs() > 1e-6 {
            stops[0].y + (stops[1].y - stops[0].y) * ((x - stops[0].t) / dx)
        } else {
            stops[0].y
        };
    }
    let last = stops.len() - 1;
    if x > stops[last].t {
        let dx = stops[last].t - stops[last - 1].t;
        return if dx.abs() > 1e-6 {
            stops[last].y + (stops[last].y - stops[last - 1].y) * ((x - stops[last].t) / dx)
        } else {
            stops[last].y
        };
    }
    if before {
        if let Some(first_eq) = stops.iter().position(|s| (s.t - x).abs() <= 1e-6) {
            return stops[first_eq].y;
        }
    }
    let a = stops.iter().rposition(|s| s.t <= x).unwrap_or(0);
    if a == last {
        return stops[last].y;
    }
    let b = a + 1;
    let dx = stops[b].t - stops[a].t;
    if dx.abs() <= 1e-6 {
        stops[a].y
    } else {
        stops[a].y + (stops[b].y - stops[a].y) * ((x - stops[a].t) / dx)
    }
}
