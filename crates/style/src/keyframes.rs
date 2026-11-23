//! Two-tier `@keyframes` and CSS `animation` model + interpolation helpers.

use crate::class_patch::{LayoutPatchOp, PaintPatchOp};
use crate::layout_types::LayoutStyle;
use crate::paint_types::PaintStyle;
use crate::transition::{TransitionCurve, TransitionMask};
use crate::transition_lerp::{interpolate_paint, lerp_f32};
use std::sync::{Arc, OnceLock, RwLock};

/// CSS `animation-fill-mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum AnimationFillMode {
    #[default]
    None,
    Forwards,
    Backwards,
    Both,
}

/// CSS `animation-direction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub enum AnimationDirection {
    #[default]
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

/// Single normalized keyframe stop (`0.0..=1.0`).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct KeyframeStop {
    pub offset: f32,
    pub paint_ops: Vec<PaintPatchOp>,
    pub layout_ops: Vec<LayoutPatchOp>,
    /// Per-keyframe `animation-timing-function` governing the interval starting at this stop.
    #[cfg_attr(feature = "preview", serde(default))]
    pub easing: Option<TransitionCurve>,
}

/// Compiled `@keyframes <name>` definition across Tier-1 paint and Tier-2 layout properties.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct KeyframesDef {
    pub name: String,
    pub stops: Vec<KeyframeStop>,
    /// `true` when any stop mutates a Taffy [`LayoutStyle`] field (Tier-2 layout track).
    pub touches_layout: bool,
}

/// Per-node CSS `animation` configuration referencing a compiled [`KeyframesDef`] by name.
#[derive(Clone, Debug, PartialEq, Default)]
#[cfg_attr(feature = "preview", derive(serde::Serialize, serde::Deserialize))]
pub struct AnimationSpec {
    pub name: String,
    pub duration_ms: u32,
    pub delay_ms: i32,
    pub timing: TransitionCurve,
    /// `None` represents `infinite`; `Some(n)` is finite repetition count.
    pub iterations: Option<f32>,
    pub direction: AnimationDirection,
    pub fill_mode: AnimationFillMode,
    pub paused: bool,
}

impl AnimationSpec {
    /// Returns `true` when this animation targets a non-empty `@keyframes` name with non-zero duration.
    #[inline]
    pub fn is_configured(&self) -> bool {
        !self.name.is_empty() && self.duration_ms > 0
    }

    /// Evaluates normalized keyframe timeline progress `[0.0, 1.0]` at `elapsed_ms`.
    /// Returns `None` when inactive (before start without `backwards`/`both`, or completed without `forwards`/`both`).
    pub fn sample_progress(&self, elapsed_ms: f64, reduced_motion: bool) -> Option<f32> {
        if !self.is_configured() {
            return None;
        }
        if reduced_motion {
            return match (self.iterations, self.fill_mode) {
                (Some(_), AnimationFillMode::Forwards | AnimationFillMode::Both) => Some(1.0),
                _ => None,
            };
        }
        let dur = self.duration_ms as f64;
        let active_ms = elapsed_ms - self.delay_ms as f64;
        if active_ms < 0.0 {
            return matches!(
                self.fill_mode,
                AnimationFillMode::Backwards | AnimationFillMode::Both
            )
            .then_some(directed_t(0.0, 0, self.direction));
        }
        if let Some(iters) = self.iterations {
            let total_ms = dur * (iters.max(0.0) as f64);
            if total_ms <= 0.0 || active_ms >= total_ms {
                if matches!(
                    self.fill_mode,
                    AnimationFillMode::Forwards | AnimationFillMode::Both
                ) {
                    let last_iter = (iters.ceil().max(1.0) as u64).saturating_sub(1);
                    let frac = iters.fract();
                    let raw = if frac == 0.0 { 1.0 } else { frac };
                    return Some(directed_t(raw, last_iter, self.direction));
                }
                return None;
            }
        }
        let iter_idx = (active_ms / dur).floor() as u64;
        let iter_t = ((active_ms % dur) / dur) as f32;
        Some(directed_t(iter_t, iter_idx, self.direction))
    }
}

/// `Copy` + `const` representation of a CSS `animation` item emitted by `krui-macros`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StaticAnimationSpec {
    pub name: &'static str,
    pub duration_ms: u32,
    pub delay_ms: i32,
    pub timing: TransitionCurve,
    pub iterations: Option<f32>,
    pub direction: AnimationDirection,
    pub fill_mode: AnimationFillMode,
    pub paused: bool,
}

impl StaticAnimationSpec {
    /// Converts this static spec into an owned [`AnimationSpec`].
    pub fn to_spec(self) -> AnimationSpec {
        AnimationSpec {
            name: self.name.to_owned(),
            duration_ms: self.duration_ms,
            delay_ms: self.delay_ms,
            timing: self.timing,
            iterations: self.iterations,
            direction: self.direction,
            fill_mode: self.fill_mode,
            paused: self.paused,
        }
    }
}

static ANIM_LIST_STORE: OnceLock<RwLock<Vec<Arc<[AnimationSpec]>>>> = OnceLock::new();

fn anim_store() -> &'static RwLock<Vec<Arc<[AnimationSpec]>>> {
    ANIM_LIST_STORE.get_or_init(|| RwLock::new(Vec::new()))
}

fn intern_anim_list(items: Vec<AnimationSpec>) -> u32 {
    if let Ok(read) = anim_store().read() {
        for (idx, existing) in read.iter().enumerate() {
            if existing.as_ref() == items.as_slice() {
                return idx as u32;
            }
        }
    }
    let mut write = anim_store()
        .write()
        .unwrap_or_else(|poison| poison.into_inner());
    for (idx, existing) in write.iter().enumerate() {
        if existing.as_ref() == items.as_slice() {
            return idx as u32;
        }
    }
    let idx = write.len() as u32;
    write.push(Arc::from(items));
    idx
}

/// `Copy` + `const`-constructible list of CSS `animation` declarations stored on styles.
#[derive(Clone, Copy, Debug, Default)]
pub enum AnimationListRef {
    #[default]
    Empty,
    Static(&'static [StaticAnimationSpec]),
    Interned(u32),
}

impl AnimationListRef {
    /// Creates a `const` animation list from a static slice (used by `krui-macros`).
    pub const fn static_ref(items: &'static [StaticAnimationSpec]) -> Self {
        if items.is_empty() {
            Self::Empty
        } else {
            Self::Static(items)
        }
    }

    /// Interns a dynamic CSS `animation` list.
    pub fn from_vec(items: Vec<AnimationSpec>) -> Self {
        if items.is_empty() {
            Self::Empty
        } else {
            Self::Interned(intern_anim_list(items))
        }
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Materializes owned [`AnimationSpec`] items into a temporary `Vec`.
    pub fn to_vec(self) -> Vec<AnimationSpec> {
        match self {
            Self::Empty => Vec::new(),
            Self::Static(items) => items
                .iter()
                .copied()
                .map(StaticAnimationSpec::to_spec)
                .collect(),
            Self::Interned(idx) => {
                let arc = anim_store()
                    .read()
                    .ok()
                    .and_then(|v| v.get(idx as usize).cloned());
                arc.map(|a| a.to_vec()).unwrap_or_default()
            }
        }
    }
}

impl PartialEq for AnimationListRef {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) => true,
            (Self::Static(a), Self::Static(b)) => a == b,
            (Self::Interned(a), Self::Interned(b)) if a == b => true,
            _ => self.to_vec() == other.to_vec(),
        }
    }
}

#[cfg(feature = "preview")]
impl serde::Serialize for AnimationListRef {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_vec().serialize(s)
    }
}

#[cfg(feature = "preview")]
impl<'de> serde::Deserialize<'de> for AnimationListRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let items = Vec::<AnimationSpec>::deserialize(d)?;
        Ok(Self::from_vec(items))
    }
}

fn directed_t(t: f32, iter_idx: u64, dir: AnimationDirection) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match dir {
        AnimationDirection::Normal => t,
        AnimationDirection::Reverse => 1.0 - t,
        AnimationDirection::Alternate => {
            if iter_idx.is_multiple_of(2) {
                t
            } else {
                1.0 - t
            }
        }
        AnimationDirection::AlternateReverse => {
            if iter_idx.is_multiple_of(2) {
                1.0 - t
            } else {
                t
            }
        }
    }
}

#[derive(Clone, Copy)]
enum StopChoice {
    Idx(usize),
    Implicit(f32),
}

impl StopChoice {
    fn offset(self, stops: &[KeyframeStop]) -> f32 {
        match self {
            Self::Idx(i) => stops[i].offset,
            Self::Implicit(o) => o,
        }
    }

    fn apply_paint(self, stops: &[KeyframeStop], base: &PaintStyle) -> PaintStyle {
        let mut out = *base;
        if let Self::Idx(i) = self {
            for &op in &stops[i].paint_ops {
                op.apply_to(&mut out);
            }
        }
        out
    }

    fn apply_layout(self, stops: &[KeyframeStop], base: &LayoutStyle) -> LayoutStyle {
        let mut out = *base;
        if let Self::Idx(i) = self {
            for &op in &stops[i].layout_ops {
                op.apply_to(&mut out);
            }
        }
        out
    }

    fn easing(self, stops: &[KeyframeStop]) -> Option<TransitionCurve> {
        match self {
            Self::Idx(i) => stops[i].easing,
            Self::Implicit(_) => None,
        }
    }
}

fn select_stop_interval(stops: &[KeyframeStop], t: f32) -> (StopChoice, StopChoice) {
    let first = stops[0].offset;
    let last_idx = stops.len() - 1;
    let last = stops[last_idx].offset;
    if t <= first {
        return if first <= 1e-5 {
            (StopChoice::Idx(0), StopChoice::Idx(0))
        } else {
            (StopChoice::Implicit(0.0), StopChoice::Idx(0))
        };
    }
    if t >= last {
        return if last >= 1.0 - 1e-5 {
            (StopChoice::Idx(last_idx), StopChoice::Idx(last_idx))
        } else {
            (StopChoice::Idx(last_idx), StopChoice::Implicit(1.0))
        };
    }
    let mut prev_idx = 0usize;
    let mut next_idx = last_idx;
    for (idx, stop) in stops.iter().enumerate() {
        if stop.offset <= t {
            prev_idx = idx;
        }
        if stop.offset >= t {
            next_idx = idx;
            break;
        }
    }
    (StopChoice::Idx(prev_idx), StopChoice::Idx(next_idx))
}

/// Evaluates `def` at eased progress `t in [0.0, 1.0]` using linear interval pacing.
pub fn sample_keyframes(
    def: &KeyframesDef,
    base_paint: &PaintStyle,
    base_layout: &LayoutStyle,
    eased_t: f32,
) -> (PaintStyle, LayoutStyle) {
    sample_keyframes_with_timing(
        def,
        base_paint,
        base_layout,
        eased_t,
        TransitionCurve::Linear,
    )
}

/// Evaluates `def` at un-eased directed progress `t in [0.0, 1.0]` honoring W3C per-interval easing
/// and implicit `0%` / `100%` underlying stops when omitted.
pub fn sample_keyframes_with_timing(
    def: &KeyframesDef,
    base_paint: &PaintStyle,
    base_layout: &LayoutStyle,
    progress_t: f32,
    default_timing: TransitionCurve,
) -> (PaintStyle, LayoutStyle) {
    if def.stops.is_empty() {
        return (*base_paint, *base_layout);
    }
    let t = progress_t.clamp(0.0, 1.0);
    let (prev, next) = select_stop_interval(&def.stops, t);
    let off_a = prev.offset(&def.stops);
    let off_b = next.offset(&def.stops);
    let span = (off_b - off_a).abs();
    let local_t = if span <= 1e-5 {
        0.0
    } else {
        let raw = ((t - off_a) / span).clamp(0.0, 1.0);
        prev.easing(&def.stops)
            .unwrap_or(default_timing)
            .sample(raw)
    };
    let paint_a = prev.apply_paint(&def.stops, base_paint);
    let paint_b = next.apply_paint(&def.stops, base_paint);
    let out_paint = if local_t <= 0.0 {
        paint_a
    } else if local_t >= 1.0 {
        paint_b
    } else {
        interpolate_paint(&paint_a, &paint_b, TransitionMask::ALL, local_t)
    };
    if !def.touches_layout {
        return (out_paint, *base_layout);
    }
    let layout_a = prev.apply_layout(&def.stops, base_layout);
    let layout_b = next.apply_layout(&def.stops, base_layout);
    let out_layout = interpolate_layout_numeric(&layout_a, &layout_b, local_t);
    (out_paint, out_layout)
}

fn interpolate_layout_numeric(a: &LayoutStyle, b: &LayoutStyle, t: f32) -> LayoutStyle {
    let mut out = if t < 0.5 { *a } else { *b };
    out.width = lerp_dim(a.width, b.width, t);
    out.height = lerp_dim(a.height, b.height, t);
    out.min_width = lerp_dim(a.min_width, b.min_width, t);
    out.min_height = lerp_dim(a.min_height, b.min_height, t);
    out.max_width = lerp_dim(a.max_width, b.max_width, t);
    out.max_height = lerp_dim(a.max_height, b.max_height, t);
    out.gap = lerp_f32(a.gap, b.gap, t).max(0.0);
    out.padding = crate::layout_types::Edges {
        top: lerp_f32(a.padding.top, b.padding.top, t).max(0.0),
        right: lerp_f32(a.padding.right, b.padding.right, t).max(0.0),
        bottom: lerp_f32(a.padding.bottom, b.padding.bottom, t).max(0.0),
        left: lerp_f32(a.padding.left, b.padding.left, t).max(0.0),
    };
    out.margin = crate::layout_types::Edges {
        top: lerp_f32(a.margin.top, b.margin.top, t),
        right: lerp_f32(a.margin.right, b.margin.right, t),
        bottom: lerp_f32(a.margin.bottom, b.margin.bottom, t),
        left: lerp_f32(a.margin.left, b.margin.left, t),
    };
    out
}

fn lerp_dim(
    a: crate::layout_types::Dimension,
    b: crate::layout_types::Dimension,
    t: f32,
) -> crate::layout_types::Dimension {
    use crate::layout_types::Dimension;
    match (a, b) {
        (Dimension::Px(x), Dimension::Px(y)) => Dimension::Px(lerp_f32(x, y, t).max(0.0)),
        (Dimension::Percent(x), Dimension::Percent(y)) => {
            Dimension::Percent(lerp_f32(x, y, t).max(0.0))
        }
        _ if t < 0.5 => a,
        _ => b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::affine::Transform2D;

    #[test]
    fn samples_compositor_keyframes_and_directions() {
        let def = KeyframesDef {
            name: "spin".into(),
            touches_layout: false,
            stops: vec![
                KeyframeStop {
                    offset: 0.0,
                    paint_ops: vec![PaintPatchOp::Transform(Transform2D::IDENTITY)],
                    layout_ops: Vec::new(),
                    easing: None,
                },
                KeyframeStop {
                    offset: 1.0,
                    paint_ops: vec![PaintPatchOp::Transform(Transform2D {
                        rotate_deg: 360.0,
                        ..Transform2D::IDENTITY
                    })],
                    layout_ops: Vec::new(),
                    easing: None,
                },
            ],
        };
        let (p, _) = sample_keyframes(&def, &PaintStyle::default(), &LayoutStyle::default(), 0.5);
        assert!((p.transform.rotate_deg - 180.0).abs() < 0.01);
    }
}
